//! Optional Standard RDP Security. TLS sessions never construct this adapter.
pub mod crypto;
mod packet;
mod stream;
use anyhow::{Context as _, Result, ensure};
use crypto::{Direction, Strength};
use ironrdp_pdu::{
    gcc::{EncryptionLevel, EncryptionMethod, ServerSecurityData},
    rdp::server_license::{ServerCertificate, cert::CertificateType},
};
use rsa::{BigUint, RsaPublicKey, pkcs1::DecodeRsaPublicKey as _, traits::PublicKeyParts as _};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
pub use stream::Stream;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub enum Outgoing {
    Exchange,
    Header,
    Payload,
}
pub struct Context {
    send: Option<Direction>,
    receive: Option<Direction>,
    low: bool,
    tags: VecDeque<Outgoing>,
    message_channel: Option<u16>,
}
impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StandardRdpSecurity")
            .field("encrypted", &self.send.is_some())
            .finish_non_exhaustive()
    }
}
pub type Shared = Arc<Mutex<Context>>;
pub fn context() -> Shared {
    Arc::new(Mutex::new(Context {
        send: None,
        receive: None,
        low: false,
        tags: VecDeque::new(),
        message_channel: None,
    }))
}
impl Context {
    /// Parse and validate the bounded server key before activating encryption.
    /// Returns the Security Exchange user data, including its Basic header.
    pub fn prepare(
        &mut self,
        server: &ServerSecurityData,
        client: [u8; 32],
        message_channel: Option<u16>,
    ) -> Result<Option<Vec<u8>>> {
        if server.encryption_method.is_empty() && server.encryption_level == EncryptionLevel::None {
            return Ok(None);
        }
        ensure!(
            server.encryption_level != EncryptionLevel::Fips,
            "原 Java 不支持的 RDP FIPS 安全层"
        );
        ensure!(
            matches!(
                server.encryption_level,
                EncryptionLevel::Low | EncryptionLevel::ClientCompatible | EncryptionLevel::High
            ),
            "RDP 加密级别无效"
        );
        let strength = match server.encryption_method {
            EncryptionMethod::BIT_40 => Strength::Bits40,
            EncryptionMethod::BIT_56 => Strength::Bits56,
            EncryptionMethod::BIT_128 => Strength::Bits128,
            _ => anyhow::bail!("RDP 服务器选择的加密方法无效"),
        };
        ensure!(
            server.encryption_level != EncryptionLevel::High
                || matches!(strength, Strength::Bits128),
            "RDP High 安全层必须使用128位密钥"
        );
        let random = server.server_random.context("RDP 服务器缺少随机数")?;
        ensure!(
            server.server_cert.len() <= 64 * 1024,
            "RDP 服务器证书超过64 KiB"
        );
        let certificate = ironrdp_core::decode::<ServerCertificate>(&server.server_cert)?;
        let key = match &certificate.certificate {
            CertificateType::Proprietary(cert) => {
                let length = cert.public_key.modulus.len();
                ensure!(
                    (72..=520).contains(&length)
                        && cert.public_key.modulus[length - 8..]
                            .iter()
                            .all(|v| *v == 0),
                    "RDP RSA 公钥长度或填充无效"
                );
                RsaPublicKey::new(
                    BigUint::from_bytes_le(&cert.public_key.modulus[..length - 8]),
                    BigUint::from(cert.public_key.public_exponent),
                )?
            }
            CertificateType::X509(_) => {
                RsaPublicKey::from_pkcs1_der(&certificate.get_public_key()?)?
            }
        };
        ensure!(
            (64..=512).contains(&key.size()),
            "RDP RSA 公钥须为512至4096位"
        );
        let mut encrypted = BigUint::from_bytes_le(&client)
            .modpow(key.e(), key.n())
            .to_bytes_le();
        encrypted.resize(key.size() + 8, 0);
        let mut exchange = Vec::with_capacity(encrypted.len() + 8);
        exchange.extend_from_slice(&1u32.to_le_bytes());
        exchange.extend_from_slice(&(encrypted.len() as u32).to_le_bytes());
        exchange.extend_from_slice(&encrypted);
        let (send, receive) = crypto::keys(&client, &random, strength);
        self.send = Some(send);
        self.receive = Some(receive);
        self.low = server.encryption_level == EncryptionLevel::Low;
        self.message_channel = message_channel;
        self.tags.clear();
        Ok(Some(exchange))
    }
    /// The connector knows which PDUs already include a Basic Security header;
    /// do not guess based on arbitrary application/virtual-channel payload bytes.
    pub fn tag(&mut self, bytes: &[u8], kind: Outgoing) -> Result<()> {
        if self.send.is_none() {
            return Ok(());
        }
        let mut bytes = bytes;
        while !bytes.is_empty() {
            let length = packet::length(bytes)?.context("RDP 输出帧不完整")?;
            ensure!(length <= bytes.len(), "RDP 输出帧不完整");
            if packet::is_send_request(&bytes[..length]) {
                ensure!(self.tags.len() < 64, "RDP 输出标记队列超过限制");
                self.tags.push_back(kind);
            }
            bytes = &bytes[length..];
        }
        Ok(())
    }
}
