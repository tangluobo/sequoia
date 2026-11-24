use super::*;
use ironrdp_core::{decode, encode_vec};
use ironrdp_pdu::{mcs, x224::X224};
use std::borrow::Cow;
const ENCRYPT: u16 = 8;
const SECURE_CHECKSUM: u16 = 0x800;
pub(super) fn length(bytes: &[u8]) -> Result<Option<usize>> {
    if bytes.len() < 2 {
        return Ok(None);
    }
    let size = if bytes[0] == 3 {
        if bytes.len() < 4 {
            return Ok(None);
        }
        ensure!(bytes[1] == 0, "RDP TPKT 保留字段无效");
        let size = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
        ensure!(size >= 7, "RDP TPKT 帧长度无效");
        size
    } else if bytes[1] & 0x80 != 0 {
        if bytes.len() < 3 {
            return Ok(None);
        }
        usize::from(u16::from_be_bytes([bytes[1] & 0x7f, bytes[2]]))
    } else {
        usize::from(bytes[1])
    };
    ensure!((2..=65535).contains(&size), "RDP 数据帧长度无效");
    Ok(Some(size))
}
pub(super) fn is_send_request(bytes: &[u8]) -> bool {
    bytes.first() == Some(&3) && bytes.get(7).is_some_and(|v| v >> 2 == 25)
}
fn fastpath(header: u8, body: Vec<u8>) -> Result<Vec<u8>> {
    let size = body.len() + 3;
    ensure!(size <= 32767, "RDP FastPath 帧超过限制");
    let mut out = Vec::with_capacity(size);
    out.push(header);
    out.push(((size >> 8) as u8) | 0x80);
    out.push(size as u8);
    out.extend_from_slice(&body);
    Ok(out)
}
impl Context {
    pub(super) fn outgoing(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        if self.send.is_none() {
            return Ok(bytes.to_vec());
        }
        if bytes.first() != Some(&3) {
            let offset = if bytes[1] & 0x80 != 0 { 3 } else { 2 };
            ensure!(bytes.len() >= offset, "RDP FastPath 头不完整");
            let data = self
                .send
                .as_mut()
                .expect("active cipher")
                .encrypt(&bytes[offset..], false);
            return fastpath((bytes[0] & 0x3f) | 0x80, data);
        }
        if !is_send_request(bytes) {
            return Ok(bytes.to_vec());
        }
        let X224(mut data) = decode::<X224<mcs::SendDataRequest<'_>>>(bytes)?;
        let kind = self.tags.pop_front().unwrap_or_else(|| {
            if Some(data.channel_id) == self.message_channel {
                Outgoing::Header
            } else {
                Outgoing::Payload
            }
        });
        if matches!(kind, Outgoing::Exchange) {
            return Ok(bytes.to_vec());
        }
        let (flags, payload) = if matches!(kind, Outgoing::Header) {
            ensure!(data.user_data.len() >= 4, "RDP Security 头不完整");
            (
                u16::from_le_bytes(data.user_data[..2].try_into()?),
                &data.user_data[4..],
            )
        } else {
            (0, data.user_data.as_ref())
        };
        let mut protected = Vec::with_capacity(payload.len() + 12);
        protected.extend_from_slice(&(flags | ENCRYPT).to_le_bytes());
        protected.extend_from_slice(&[0; 2]);
        protected.extend_from_slice(
            &self
                .send
                .as_mut()
                .expect("active cipher")
                .encrypt(payload, false),
        );
        data.user_data = Cow::Owned(protected);
        Ok(encode_vec(&X224(data))?)
    }
    pub(super) fn incoming(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        if self.receive.is_none() {
            return Ok(bytes.to_vec());
        }
        if bytes.first() != Some(&3) {
            if bytes[0] & 0x80 == 0 {
                ensure!(self.low, "RDP 收到未加密的 FastPath 数据");
                return Ok(bytes.to_vec());
            }
            let offset = if bytes[1] & 0x80 != 0 { 3 } else { 2 };
            ensure!(bytes.len() >= offset + 8, "RDP FastPath 头不完整");
            let data = self
                .receive
                .as_mut()
                .expect("active cipher")
                .decrypt(&bytes[offset..], bytes[0] & 0x40 != 0)?;
            return fastpath(bytes[0] & 0x3f, data);
        }
        if !bytes.get(7).is_some_and(|v| v >> 2 == 26) {
            return Ok(bytes.to_vec());
        }
        let X224(mut data) = decode::<X224<mcs::SendDataIndication<'_>>>(bytes)?;
        ensure!(data.user_data.len() >= 4, "RDP Security 头不完整");
        let flags = u16::from_le_bytes(data.user_data[..2].try_into()?);
        let payload = &data.user_data[4..];
        let decoded = if flags & ENCRYPT != 0 {
            self.receive
                .as_mut()
                .expect("active cipher")
                .decrypt(payload, flags & SECURE_CHECKSUM != 0)?
        } else {
            ensure!(self.low || flags & 0x80 != 0, "RDP 收到未加密数据");
            payload.to_vec()
        };
        // RESET_SEQNO and IGNORE_SEQNO are reserved and MUST be ignored.
        let flags = flags & !(ENCRYPT | SECURE_CHECKSUM | 0x10 | 0x20);
        let mut plain = Vec::with_capacity(decoded.len() + 4);
        if flags != 0 {
            plain.extend_from_slice(&flags.to_le_bytes());
            plain.extend_from_slice(&[0; 2]);
        }
        plain.extend_from_slice(&decoded);
        data.user_data = Cow::Owned(plain);
        Ok(encode_vec(&X224(data))?)
    }
}
