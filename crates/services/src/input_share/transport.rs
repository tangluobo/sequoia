//! Versioned, authenticated, encrypted ordered TCP records with per-connection keys.
use anyhow::{Result, anyhow, ensure};
use ring::{
    aead, hkdf,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Serialize, de::DeserializeOwned};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    time::{Duration, timeout},
};
const LIMIT: usize = 16 * 1024;
pub(super) const DISCOVER: &[u8] = b"TOMATO-INPUT-SHARE/1";
pub(super) struct Cipher {
    key: aead::LessSafeKey,
    seq: u64,
}
impl Cipher {
    fn new(secret: &[u8; 16], first: &[u8; 32], second: &[u8; 32]) -> Result<Self> {
        let mut salt = [0; 64];
        salt[..32].copy_from_slice(first);
        salt[32..].copy_from_slice(second);
        let extracted = hkdf::Salt::new(hkdf::HKDF_SHA256, &salt).extract(secret);
        let context = [b"tomato-input-share/v1".as_slice()];
        let expanded = extracted
            .expand(&context, hkdf::HKDF_SHA256)
            .map_err(|_| anyhow!("密钥派生失败"))?;
        let mut hash = [0; 32];
        expanded
            .fill(&mut hash)
            .map_err(|_| anyhow!("密钥派生失败"))?;
        Ok(Self {
            key: aead::LessSafeKey::new(
                aead::UnboundKey::new(&aead::CHACHA20_POLY1305, &hash)
                    .map_err(|_| anyhow!("加密初始化失败"))?,
            ),
            seq: 0,
        })
    }
    fn nonce(&mut self) -> Result<aead::Nonce> {
        let mut nonce = [0; 12];
        nonce[4..].copy_from_slice(&self.seq.to_be_bytes());
        self.seq = self
            .seq
            .checked_add(1)
            .ok_or_else(|| anyhow!("加密序号耗尽"))?;
        Ok(aead::Nonce::assume_unique_for_key(nonce))
    }
    pub fn seal<T: Serialize>(&mut self, value: &T) -> Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec(value)?;
        ensure!(bytes.len() + 16 <= LIMIT, "消息过大");
        let nonce = self.nonce()?;
        self.key
            .seal_in_place_append_tag(nonce, aead::Aad::empty(), &mut bytes)
            .map_err(|_| anyhow!("加密失败"))?;
        Ok(bytes)
    }
    pub fn open<T: DeserializeOwned>(&mut self, mut bytes: Vec<u8>) -> Result<T> {
        ensure!(bytes.len() <= LIMIT, "消息过大");
        let nonce = self.nonce()?;
        let plain = self
            .key
            .open_in_place(nonce, aead::Aad::empty(), &mut bytes)
            .map_err(|_| anyhow!("配对认证失败或通信记录损坏"))?;
        Ok(serde_json::from_slice(plain)?)
    }
}
pub(super) fn secret(code: &str) -> Result<[u8; 16]> {
    let id = uuid::Uuid::parse_str(code.trim())
        .map_err(|_| anyhow!("配对码应为服务端显示的完整代码"))?;
    Ok(*id.as_bytes())
}
pub(super) async fn handshake(
    stream: &mut TcpStream,
    secret: &[u8; 16],
) -> Result<(Cipher, Cipher)> {
    let mut local = [0; 32];
    let mut remote = [0; 32];
    SystemRandom::new()
        .fill(&mut local)
        .map_err(|_| anyhow!("系统随机源不可用"))?;
    timeout(Duration::from_secs(3), async {
        stream.write_all(&local).await?;
        stream.read_exact(&mut remote).await?;
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    Ok((
        Cipher::new(secret, &local, &remote)?,
        Cipher::new(secret, &remote, &local)?,
    ))
}
pub(super) async fn write<W: tokio::io::AsyncWrite + Unpin, T: Serialize>(
    w: &mut W,
    cipher: &mut Cipher,
    value: &T,
) -> Result<()> {
    let bytes = cipher.seal(value)?;
    timeout(Duration::from_secs(2), async {
        w.write_u32(bytes.len() as u32).await?;
        w.write_all(&bytes).await
    })
    .await??;
    Ok(())
}
pub(super) async fn read<R: tokio::io::AsyncRead + Unpin, T: DeserializeOwned>(
    r: &mut R,
    cipher: &mut Cipher,
) -> Result<T> {
    let size = r.read_u32().await? as usize;
    ensure!((16..=LIMIT).contains(&size), "无效记录长度");
    let mut bytes = vec![0; size];
    r.read_exact(&mut bytes).await?;
    cipher.open(bytes)
}
pub(super) async fn scan() -> Result<Vec<String>> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.set_broadcast(true)?;
    socket
        .send_to(DISCOVER, (std::net::Ipv4Addr::BROADCAST, super::PORT))
        .await?;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(900);
    let mut found = std::collections::BTreeSet::new();
    let mut bytes = [0; 128];
    while let Ok(Ok((n, addr))) =
        tokio::time::timeout_at(deadline, socket.recv_from(&mut bytes)).await
    {
        if bytes[..n].starts_with(DISCOVER) && n == DISCOVER.len() + 2 {
            let port = u16::from_be_bytes([bytes[n - 2], bytes[n - 1]]);
            found.insert(std::net::SocketAddr::new(addr.ip(), port).to_string());
            if found.len() == 64 {
                break;
            }
        }
    }
    Ok(found.into_iter().collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encryption_rejects_wrong_keys_tampering_and_replay() -> Result<()> {
        let mut a = Cipher::new(&[1; 16], &[2; 32], &[3; 32])?;
        let message = a.seal(&"hello")?;
        let mut b = Cipher::new(&[1; 16], &[2; 32], &[3; 32])?;
        assert_eq!(b.open::<String>(message.clone())?, "hello");
        assert!(b.open::<String>(message.clone()).is_err());
        assert!(
            Cipher::new(&[0; 16], &[2; 32], &[3; 32])?
                .open::<String>(message.clone())
                .is_err()
        );
        let mut damaged = message;
        damaged[0] ^= 1;
        assert!(
            Cipher::new(&[1; 16], &[2; 32], &[3; 32])?
                .open::<String>(damaged)
                .is_err()
        );
        Ok(())
    }
}
