use super::*;

fn utf16(value: &str) -> Vec<u8> {
    value
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn packet(flags: u32, fields: &[Vec<u8>]) -> Vec<u8> {
    let mut body = vec![0, 4, 0, 0];
    body.extend_from_slice(&27_u32.to_le_bytes());
    body.extend_from_slice(&flags.to_le_bytes());
    for field in fields {
        body.extend_from_slice(&(field.len() as u32).to_le_bytes());
        body.extend_from_slice(field);
    }
    let length = u16::try_from(body.len()).unwrap();
    body[2..4].copy_from_slice(&length.to_le_bytes());
    let mut data = vec![0, 0, 0x1a, 0, 0xeb, 3, 0, 0];
    data.extend(body);
    let total = u16::try_from(data.len()).unwrap();
    data[..2].copy_from_slice(&total.to_le_bytes());
    ironrdp_core::encode_vec(&ironrdp_pdu::x224::X224(
        ironrdp_pdu::mcs::McsMessage::SendDataIndication(ironrdp_pdu::mcs::SendDataIndication {
            initiator_id: 1001,
            channel_id: 1003,
            user_data: std::borrow::Cow::Owned(data),
        }),
    ))
    .unwrap()
}

#[test]
fn enhanced_redirection_preserves_routing_credentials_and_ignores_other_channels() {
    let bytes = packet(
        2 | 4 | 8 | 16 | 256,
        &[
            b"Cookie: msts=owned-token\r\n".to_vec(),
            utf16("one-time-user"),
            utf16(""),
            utf16("one-time-password"),
            utf16("unreachable.invalid"),
        ],
    );
    assert!(detect(&bytes, 1004).unwrap().is_none());
    let redirect = detect(&bytes, 1003).unwrap().unwrap();
    assert_eq!(redirect.session_id, 27);
    assert_eq!(
        redirect.routing.as_deref(),
        Some("Cookie: msts=owned-token")
    );
    let mut config = crate::config::ConfigBuilder::new()
        .with_destination(Destination::from_parts("127.0.0.1", 3389))
        .with_username("original")
        .with_password("original-password")
        .with_client_build(19045)
        .with_client_dir("C:\\owned")
        .with_client_name("owned")
        .with_platform(ironrdp_pdu::rdp::capability_sets::MajorPlatformType::WINDOWS)
        .build()
        .unwrap();
    redirect.apply(&mut config).unwrap();
    assert_eq!(config.destination.name(), "127.0.0.1");
    match &config.connector.credentials {
        ironrdp_connector::Credentials::UsernamePassword { username, password } => {
            assert_eq!(username, "one-time-user");
            assert_eq!(password, "one-time-password");
        }
        _ => panic!("Expected redirected password credentials"),
    }
    assert!(!format!("{config:?}").contains("one-time-password"));
    assert!(
        detect(&packet(128, &[]), 1003)
            .unwrap()
            .unwrap()
            .informational()
    );
}

#[test]
fn malformed_redirection_lengths_strings_and_missing_one_time_fields_are_rejected() {
    for field in [
        vec![b'a'],
        vec![b'a', 0],
        vec![0, 0, 1, 0],
        vec![0, 0xd8, 0, 0],
    ] {
        assert!(detect(&packet(4, &[field]), 1003).is_err());
    }
    let redirect = detect(
        &packet(4 | 16 | 0x4000, &[utf16("owned"), vec![0xff, 0, 1, 0]]),
        1003,
    )
    .unwrap()
    .unwrap();
    let mut config = crate::config::ConfigBuilder::new()
        .with_destination(Destination::from_parts("127.0.0.1", 3389))
        .with_username("original")
        .with_password("original-password")
        .with_client_build(19045)
        .with_client_dir("C:\\owned")
        .with_client_name("owned")
        .with_platform(ironrdp_pdu::rdp::capability_sets::MajorPlatformType::WINDOWS)
        .build()
        .unwrap();
    assert!(redirect.apply(&mut config).is_err());
    match &config.connector.credentials {
        ironrdp_connector::Credentials::UsernamePassword { password, .. } => {
            assert_eq!(password, "original-password")
        }
        _ => panic!("Expected original password credentials"),
    }
}

#[test]
fn rdstls_forwards_binary_ciphertext_and_validates_the_authentication_result() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let cipher = vec![0, 255, 128, 0, 254, 0];
            let guid = utf16("owned-guid");
            let redirect = detect(
                &packet(
                    4 | 8 | 16 | 0x4000 | 0x8000,
                    &[utf16("owned"), utf16(""), cipher.clone(), guid.clone()],
                ),
                1003,
            )
            .unwrap()
            .unwrap();
            assert_eq!(redirect.opaque_password(), Some(cipher.as_slice()));
            for result in [0_u32, 5] {
                let (mut client, mut server) = tokio::io::duplex(1024);
                let expected = [guid.clone(), utf16("owned"), utf16(""), cipher.clone()];
                let peer = tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
                    server.write_all(&[1, 0, 1, 0, 1, 0, 1, 0]).await.unwrap();
                    let mut header = [0; 6];
                    server.read_exact(&mut header).await.unwrap();
                    assert_eq!(header, [1, 0, 2, 0, 1, 0]);
                    for field in expected {
                        let count = server.read_u16_le().await.unwrap();
                        let mut actual = vec![0; usize::from(count)];
                        server.read_exact(&mut actual).await.unwrap();
                        assert_eq!(actual, field);
                    }
                    server.write_all(&[1, 0, 4, 0, 1, 0]).await.unwrap();
                    server.write_u32_le(result).await.unwrap();
                });
                let accepted = authenticate(&mut client, &redirect).await;
                assert_eq!(accepted.is_ok(), result == 0);
                if let Err(error) = accepted {
                    assert!(error.to_string().contains("0x00000005"));
                }
                peer.await.unwrap();
            }
        });
}
