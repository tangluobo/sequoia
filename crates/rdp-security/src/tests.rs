use super::*;
use ironrdp_core::{decode, encode_vec};
use ironrdp_pdu::{mcs, x224::X224};
use std::borrow::Cow;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

fn randoms() -> ([u8; 32], [u8; 32]) {
    (
        std::array::from_fn(|i| i as u8),
        std::array::from_fn(|i| (i + 32) as u8),
    )
}
fn protected() -> Shared {
    let shared = context();
    let (client, server) = randoms();
    let (send, receive) = crypto::keys(&client, &server, Strength::Bits128);
    {
        let mut security = shared.lock().unwrap();
        security.send = Some(send);
        security.receive = Some(receive);
    }
    shared
}
fn request(data: Vec<u8>) -> Result<Vec<u8>> {
    Ok(encode_vec(&X224(mcs::SendDataRequest {
        initiator_id: 1002,
        channel_id: 1003,
        user_data: Cow::Owned(data),
    }))?)
}
async fn frame(socket: &mut (impl tokio::io::AsyncRead + Unpin)) -> Result<Vec<u8>> {
    let mut output = vec![0; 2];
    socket.read_exact(&mut output).await?;
    if output[0] == 3 || output[1] & 0x80 != 0 {
        output.push(socket.read_u8().await?);
    }
    if output[0] == 3 {
        output.push(socket.read_u8().await?);
    }
    let length = packet::length(&output)?.unwrap();
    let start = output.len();
    output.resize(length, 0);
    socket.read_exact(&mut output[start..]).await?;
    Ok(output)
}
#[tokio::test]
async fn packet_stream_survives_one_byte_backpressure_coalesced_frames_and_salted_reply()
-> Result<()> {
    let security = protected();
    let info = request([&0x40u32.to_le_bytes()[..], b"fixture-credentials"].concat())?;
    let payload = request(vec![0x80, 0, 0, 0, 1, 2, 3, 4])?;
    {
        let mut state = security.lock().unwrap();
        state.tag(&info, Outgoing::Header)?;
        state.tag(&payload, Outgoing::Payload)?;
    }
    let input = [info, payload, vec![4, 4, 0x60, 0]].concat();
    let (client, mut server) = tokio::io::duplex(1);
    let peer = tokio::spawn(async move {
        let (client, server_random) = randoms();
        let (mut receive, mut send) = crypto::keys(&client, &server_random, Strength::Bits128);
        for expected in [
            &b"fixture-credentials"[..],
            &[0x80, 0, 0, 0, 1, 2, 3, 4][..],
        ] {
            let bytes = frame(&mut server).await?;
            let X224(data) = decode::<X224<mcs::SendDataRequest<'_>>>(&bytes)?;
            assert_eq!(u16::from_le_bytes(data.user_data[..2].try_into()?) & 8, 8);
            assert_eq!(receive.decrypt(&data.user_data[4..], false)?, expected);
        }
        let bytes = frame(&mut server).await?;
        assert_eq!(bytes[0], 0x84);
        assert_eq!(receive.decrypt(&bytes[3..], false)?, [0x60, 0]);
        let body = vec![10, 20, 30, 40];
        let mut protected = 0x808u32.to_le_bytes().to_vec();
        protected.extend_from_slice(&send.encrypt(&body, true));
        let response = encode_vec(&X224(mcs::SendDataIndication {
            initiator_id: 1002,
            channel_id: 1003,
            user_data: Cow::Owned(protected),
        }))?;
        server.write_all(&response).await?;
        Ok::<_, anyhow::Error>(encode_vec(&X224(mcs::SendDataIndication {
            initiator_id: 1002,
            channel_id: 1003,
            user_data: Cow::Owned(body),
        }))?)
    });
    let mut stream = Stream::new(client, security);
    stream.write_all(&input).await?;
    let bytes =
        tokio::time::timeout(std::time::Duration::from_secs(3), frame(&mut stream)).await??;
    assert_eq!(bytes, peer.await??);
    Ok(())
}
#[test]
fn unexpected_cleartext_corrupt_signature_and_oversized_packets_are_rejected() -> Result<()> {
    let security = protected();
    let body = vec![1; 16];
    let clear = encode_vec(&X224(mcs::SendDataIndication {
        initiator_id: 1002,
        channel_id: 1003,
        user_data: Cow::Owned([vec![0; 4], body].concat()),
    }))?;
    assert!(security.lock().unwrap().incoming(&clear).is_err());
    let (client, server) = randoms();
    let (_, mut peer) = crypto::keys(&client, &server, Strength::Bits128);
    let mut data = peer.encrypt(b"fixture", true);
    data[0] ^= 1;
    let corrupted = encode_vec(&X224(mcs::SendDataIndication {
        initiator_id: 1002,
        channel_id: 1003,
        user_data: Cow::Owned([0x808u32.to_le_bytes().to_vec(), data].concat()),
    }))?;
    assert!(security.lock().unwrap().incoming(&corrupted).is_err());
    assert!(packet::length(&[3, 0, 0, 1]).is_err());
    assert!(packet::length(&[3, 1, 0, 24]).is_err());
    assert!(packet::length(&[0, 1]).is_err());
    Ok(())
}
#[test]
fn no_encryption_keeps_original_frames_and_rejects_fips_before_key_setup() -> Result<()> {
    let shared = context();
    let mut state = shared.lock().unwrap();
    assert!(
        state
            .prepare(&ServerSecurityData::no_security(), [0; 32], None)?
            .is_none()
    );
    let bytes = request(vec![0x80, 0, 0, 0])?;
    assert_eq!(state.outgoing(&bytes)?, bytes);
    assert!(
        state
            .prepare(
                &ServerSecurityData {
                    encryption_method: EncryptionMethod::FIPS,
                    encryption_level: EncryptionLevel::Fips,
                    server_random: Some([0; 32]),
                    server_cert: vec![]
                },
                [0; 32],
                None
            )
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn simultaneous_full_duplex_backpressure_never_blocks_the_read_half() -> Result<()> {
    let sent = request(b"client-fixture".to_vec())?;
    let response = request(b"server-fixture".to_vec())?;
    let expected = response.clone();
    let (client, mut server) = tokio::io::duplex(1);
    let peer = tokio::spawn(async move {
        server.write_all(&response).await?;
        frame(&mut server).await
    });
    let mut stream = Stream::new(client, context());
    stream.write_all(&sent).await?;
    let received =
        tokio::time::timeout(std::time::Duration::from_secs(3), frame(&mut stream)).await??;
    assert_eq!(received, expected);
    stream.flush().await?;
    assert_eq!(peer.await??, sent);
    Ok(())
}
