//! Per-hop routing data is deliberately omitted from debug logs.
pub(super) struct Session {
    pub session_id: u32,
    pub routing_token: Option<String>,
    pub opaque_password: Option<Vec<u8>>,
}
impl core::fmt::Debug for Session {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RedirectedSession")
            .field("session_id", &self.session_id)
            .field("routing_token_present", &self.routing_token.is_some())
            .field("rdstls", &self.opaque_password.is_some())
            .finish()
    }
}

pub(super) struct Encoded(pub Vec<u8>);
impl ironrdp_core::Encode for Encoded {
    fn encode(&self, dst: &mut ironrdp_core::WriteCursor<'_>) -> ironrdp_core::EncodeResult<()> {
        ironrdp_core::ensure_size!(in: dst, size: self.0.len());
        dst.write_slice(&self.0);
        Ok(())
    }
    fn name(&self) -> &'static str {
        "Redirected Client Info"
    }
    fn size(&self) -> usize {
        self.0.len()
    }
}

pub(super) fn client_info(mut bytes: Vec<u8>, password: &[u8]) -> crate::ConnectorResult<Encoded> {
    if bytes.len() < 22 || bytes.len() + password.len() > 65535 {
        return Err(crate::reason_err!(
            "RDSTLS client info",
            "invalid packet size"
        ));
    }
    let domain = usize::from(u16::from_le_bytes([bytes[12], bytes[13]]));
    let user = usize::from(u16::from_le_bytes([bytes[14], bytes[15]]));
    let old_password = usize::from(u16::from_le_bytes([bytes[16], bytes[17]]));
    let start = 22 + domain + 2 + user + 2;
    let end = start + old_password;
    if end + 2 > bytes.len() || password.len() > 65535 {
        return Err(crate::reason_err!(
            "RDSTLS client info",
            "invalid credential bounds"
        ));
    }
    let size = u16::try_from(password.len())
        .map_err(|_| crate::reason_err!("RDSTLS client info", "encrypted password too long"))?;
    bytes[16..18].copy_from_slice(&size.to_le_bytes());
    bytes.splice(start..end, password.iter().copied());
    Ok(Encoded(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opaque_client_info_keeps_binary_password_bytes_and_null_terminator() {
        let mut original = vec![0; 22];
        original[8] = 0x10;
        original[12..14].copy_from_slice(&2_u16.to_le_bytes());
        original[14..16].copy_from_slice(&4_u16.to_le_bytes());
        original.extend_from_slice(&[b'D', 0, 0, 0, b'u', 0, b'1', 0, 0, 0, 0, 0]);
        original.extend_from_slice(&[7, 8, 9, 10]);
        let cipher = [0, 255, 128, 0, 253, 0];
        let actual = client_info(original.clone(), &cipher).unwrap().0;
        assert_eq!(&actual[16..18], &6_u16.to_le_bytes());
        assert_eq!(&actual[32..38], &cipher);
        assert_eq!(&actual[38..], &original[32..]);
        assert_eq!(&actual[18..32], &original[18..32]);
        assert!(client_info(vec![0; 10], &cipher).is_err());
        assert!(client_info(vec![0; 22], &cipher).is_err());
        assert!(
            !format!(
                "{:?}",
                Session {
                    session_id: 27,
                    routing_token: Some("private-token".into()),
                    opaque_password: Some(cipher.to_vec())
                }
            )
            .contains("private-token")
        );
    }
}
