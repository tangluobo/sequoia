//! Read legacy client keystores off the async executor, without exporting keys.
use super::config::Config;
use anyhow::{Context, Result, ensure};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::io::Read as _;
pub(super) type Identity = (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>);
pub(super) fn is_pkcs12(bytes: &[u8]) -> bool {
    if bytes.first() != Some(&0x30) || bytes.len() < 2 {
        return false;
    }
    let header = if bytes[1] & 0x80 == 0 {
        2
    } else {
        let count = usize::from(bytes[1] & 0x7f);
        if count == 0 || count > 4 {
            return false;
        }
        2 + count
    };
    bytes.get(header..header + 3) == Some(&[0x02, 0x01, 0x03])
}
pub(super) fn validate_jks(mut data: &[u8]) -> Result<()> {
    fn take<'a>(data: &mut &'a [u8], count: usize) -> Result<&'a [u8]> {
        ensure!(data.len() >= count, "Hive JKS 数据不完整");
        let (head, tail) = data.split_at(count);
        *data = tail;
        Ok(head)
    }
    fn u32be(data: &mut &[u8]) -> Result<u32> {
        Ok(u32::from_be_bytes(take(data, 4)?.try_into()?))
    }
    fn string(data: &mut &[u8]) -> Result<()> {
        let count = u16::from_be_bytes(take(data, 2)?.try_into()?) as usize;
        take(data, count)?;
        Ok(())
    }
    fn blob(data: &mut &[u8]) -> Result<()> {
        let count = u32be(data)? as usize;
        take(data, count)?;
        Ok(())
    }
    ensure!(u32be(&mut data)? == 0xfeedfeed, "Hive JKS 文件头无效");
    let version = u32be(&mut data)?;
    ensure!(matches!(version, 1 | 2), "Hive JKS 版本无效");
    let count = u32be(&mut data)?;
    ensure!(count <= 4096, "Hive JKS 项目超过4096项");
    let mut certificates = 0;
    for _ in 0..count {
        let kind = u32be(&mut data)?;
        ensure!(matches!(kind, 1 | 2), "Hive JKS 条目类型无效");
        string(&mut data)?;
        take(&mut data, 8)?;
        if kind == 1 {
            blob(&mut data)?;
            let count = u32be(&mut data)?;
            ensure!(count <= 4096, "Hive JKS 证书超过4096项");
            certificates += count;
            ensure!(certificates <= 4096, "Hive JKS 证书超过4096项");
            for _ in 0..count {
                if version == 2 {
                    string(&mut data)?;
                }
                blob(&mut data)?;
            }
        } else {
            certificates += 1;
            ensure!(certificates <= 4096, "Hive JKS 证书超过4096项");
            if version == 2 {
                string(&mut data)?;
            }
            blob(&mut data)?;
        }
    }
    ensure!(data.len() == 20, "Hive JKS 校验数据长度无效");
    Ok(())
}
fn read(path: &str, password: &str, format: &str, alias: &str) -> Result<Identity> {
    ensure!(
        !path.is_empty(),
        "Hive twoWay=true 需要 sslKeyStore 客户端密钥库"
    );
    let file = std::fs::File::open(path).context("无法读取 Hive 客户端密钥库")?;
    ensure!(
        file.metadata()?.len() <= 8 * 1024 * 1024,
        "Hive 客户端密钥库超过8 MiB"
    );
    let mut bytes = Vec::new();
    file.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 8 * 1024 * 1024, "Hive 客户端密钥库超过8 MiB");
    if bytes.starts_with(&[0xfe, 0xed, 0xfe, 0xed]) {
        validate_jks(&bytes)?;
        let mut store = jks::KeyStore::new();
        store
            .load(bytes.as_slice(), password.as_bytes())
            .context("Hive JKS 客户端密钥库或密码无效")?;
        let mut aliases = store.aliases();
        aliases.sort();
        let name = aliases
            .into_iter()
            .find(|name| (alias.is_empty() || alias == name) && store.is_private_key_entry(name))
            .context("Hive JKS 客户端密钥库缺少私钥")?;
        let entry = store
            .get_private_key_entry(&name, password.as_bytes())
            .context("Hive JKS 私钥密码无效")?;
        let certificates = entry
            .certificate_chain
            .into_iter()
            .map(|certificate| CertificateDer::from(certificate.content))
            .collect::<Vec<_>>();
        ensure!(!certificates.is_empty(), "Hive 客户端私钥缺少证书链");
        return Ok((
            certificates,
            PrivateKeyDer::try_from(entry.private_key).map_err(anyhow::Error::msg)?,
        ));
    }
    if is_pkcs12(&bytes)
        || format.eq_ignore_ascii_case("PKCS12")
        || std::path::Path::new(path)
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("p12") || s.eq_ignore_ascii_case("pfx"))
    {
        let store = p12_keystore::KeyStore::from_pkcs12(
            &bytes,
            password,
            p12_keystore::Pkcs12ImportPolicy::Strict,
        )
        .context("Hive PKCS12 客户端密钥库或密码无效")?;
        ensure!(store.entries_len() <= 4096, "Hive 客户端密钥库超过4096项");
        let chain = store
            .entries()
            .find_map(|(name, entry)| {
                if (alias.is_empty() || alias == name)
                    && let p12_keystore::KeyStoreEntry::PrivateKeyChain(chain) = entry
                {
                    Some(chain)
                } else {
                    None
                }
            })
            .context("Hive PKCS12 客户端密钥库缺少私钥")?;
        let certificates = chain
            .certs()
            .iter()
            .map(|cert| CertificateDer::from(cert.as_der().to_owned()))
            .collect::<Vec<_>>();
        ensure!(!certificates.is_empty(), "Hive 客户端私钥缺少证书链");
        return Ok((
            certificates,
            PrivateKeyDer::try_from(chain.key().as_der().to_owned()).map_err(anyhow::Error::msg)?,
        ));
    }
    let mut certificates = Vec::new();
    let mut key = None;
    for entry in pem::parse_many(&bytes)? {
        match entry.tag() {
            "CERTIFICATE" => certificates.push(CertificateDer::from(entry.into_contents())),
            "PRIVATE KEY" | "RSA PRIVATE KEY" | "EC PRIVATE KEY" => {
                ensure!(
                    key.is_none(),
                    "Hive PEM 包含多个私钥，请使用带别名的 JKS/PKCS12 密钥库"
                );
                key = Some(
                    PrivateKeyDer::try_from(entry.into_contents()).map_err(anyhow::Error::msg)?,
                );
            }
            _ => {}
        }
    }
    ensure!(
        !certificates.is_empty() && certificates.len() <= 4096,
        "Hive 客户端证书链缺失或超过4096项"
    );
    Ok((
        certificates,
        key.context("Hive PEM 缺少可用私钥；加密私钥请放入 JKS/PKCS12 密钥库")?,
    ))
}
pub(super) async fn load(config: &Config) -> Result<Option<Identity>> {
    if !config.option("twoway").eq_ignore_ascii_case("true") {
        return Ok(None);
    }
    let path = config.option("sslkeystore").to_owned();
    let password = config.option("keystorepassword").to_owned();
    let format = config.option("keystoretype").to_owned();
    let alias = config.option("sslkeystorealias").to_owned();
    Ok(Some(
        tokio::task::spawn_blocking(move || read(&path, &password, &format, &alias)).await??,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn oversized_declared_jks_lengths_are_rejected_before_the_keystore_allocator()
    -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("bounded.jks");
        let mut data = Vec::new();
        for n in [0xfeedfeedu32, 2, 1, 1] {
            data.extend_from_slice(&n.to_be_bytes());
        }
        data.extend_from_slice(&[0; 10]);
        data.extend_from_slice(&u32::MAX.to_be_bytes());
        data.extend_from_slice(&[0; 20]);
        std::fs::write(&path, &data)?;
        let mut connection = tomato_core::Connection::new("HIVE");
        connection.set("host", "127.0.0.1");
        connection.set("port", 10000);
        connection.set(
            "database",
            format!("default;ssl=true;sslTrustStore={}", path.display()),
        );
        let error = Config::new(&connection, "", None)?.tls().await.unwrap_err();
        assert!(format!("{error:#}").contains("不完整"));
        assert_eq!(std::fs::read(path)?, data);
        Ok(())
    }
}
