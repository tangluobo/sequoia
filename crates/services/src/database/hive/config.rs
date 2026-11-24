//! Parse the legacy Hive database/session/configuration URL suffix in place.
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, sync::Arc};
use tomato_core::Connection;
pub(super) struct Config {
    pub host: String,
    pub port: u16,
    pub endpoint: (String, u16),
    pub username: String,
    pub password: String,
    pub options: BTreeMap<String, String>,
    pub configuration: BTreeMap<String, String>,
}
impl Config {
    pub fn new(
        connection: &Connection,
        database: &str,
        routed: Option<&Connection>,
    ) -> Result<Self> {
        let host = connection.text("host").trim().trim_matches(['[', ']']);
        ensure!(!host.is_empty(), "Hive 主机不能为空");
        let configured = connection.text("database");
        let (before_variables, variables) = configured.split_once('#').unwrap_or((configured, ""));
        let (before_config, configuration) = before_variables
            .split_once('?')
            .unwrap_or((before_variables, ""));
        let mut parts = before_config.split(';');
        let configured_database = parts.next().unwrap_or_default().trim();
        let mut options = BTreeMap::new();
        for part in parts.filter(|s| !s.is_empty()) {
            let (key, value) = part
                .split_once('=')
                .context("Hive 会话参数需要使用 key=value")?;
            ensure!(!key.trim().is_empty(), "Hive 会话参数名不能为空");
            options.insert(
                key.trim().to_ascii_lowercase(),
                percent_encoding::percent_decode_str(value)
                    .decode_utf8()?
                    .into_owned(),
            );
        }
        let mut overlay = BTreeMap::new();
        for (prefix, values) in [
            ("set:hiveconf:", configuration),
            ("set:hivevar:", variables),
        ] {
            for part in values.split(';').filter(|s| !s.is_empty()) {
                let (key, value) = part
                    .split_once('=')
                    .context("Hive 配置参数需要使用 key=value")?;
                ensure!(!key.trim().is_empty(), "Hive 配置参数名不能为空");
                overlay.insert(
                    format!("{prefix}{}", key.trim()),
                    percent_encoding::percent_decode_str(value)
                        .decode_utf8()?
                        .into_owned(),
                );
            }
        }
        let database = if database.is_empty() {
            configured_database
        } else {
            database.split([';', '?', '#']).next().unwrap_or_default()
        };
        let database = if database.is_empty() {
            "default"
        } else {
            database
        };
        ensure!(
            !database.chars().any(char::is_control),
            "Hive 数据库名称包含控制字符"
        );
        overlay.insert("use:database".into(), database.to_owned());
        let endpoint = routed.unwrap_or(connection);
        let port = u16::try_from(endpoint.number("port"))?;
        ensure!(port > 0, "Hive 端口不能为空");
        Ok(Self {
            host: host.into(),
            port: u16::try_from(connection.number("port"))?,
            endpoint: (
                endpoint.text("host").trim().trim_matches(['[', ']']).into(),
                port,
            ),
            username: connection.text("username").into(),
            password: connection.text("password").into(),
            options,
            configuration: overlay,
        })
    }
    pub fn option(&self, key: &str) -> &str {
        self.options
            .get(key)
            .map(String::as_str)
            .unwrap_or_default()
    }
    pub fn http(&self) -> bool {
        self.option("transportmode").eq_ignore_ascii_case("http")
    }
    pub fn ssl(&self) -> bool {
        self.option("ssl").eq_ignore_ascii_case("true")
    }
    pub fn sasl(&self) -> Result<bool> {
        let auth = self.option("auth").to_ascii_lowercase();
        ensure!(
            matches!(
                auth.as_str(),
                "" | "none" | "nosasl" | "ldap" | "pam" | "custom" | "kerberos"
            ),
            "Hive 认证参数无效"
        );
        ensure!(
            !self.kerberos() || auth != "nosasl",
            "Hive Kerberos 不能使用 noSasl"
        );
        Ok(auth != "nosasl")
    }
    pub fn kerberos(&self) -> bool {
        !self.option("principal").is_empty() || self.option("auth").eq_ignore_ascii_case("kerberos")
    }
    pub fn http_path(&self) -> Result<String> {
        let path = self.option("httppath").trim_start_matches('/');
        let path = if path.is_empty() { "cliservice" } else { path };
        ensure!(
            !path.chars().any(char::is_control) && !path.starts_with('/'),
            "Hive HTTP 路径无效"
        );
        Ok(format!("/{path}"))
    }
    pub async fn tls(&self) -> Result<Arc<rustls::ClientConfig>> {
        let path = self.option("ssltruststore").to_owned();
        let password = self.option("truststorepassword").to_owned();
        let format = self.option("truststoretype").to_owned();
        let roots = tokio::task::spawn_blocking(move || -> Result<_> {
            let mut roots =
                rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            if !path.is_empty() {
                use std::io::Read as _;
                let file = std::fs::File::open(&path).context("无法读取 Hive TLS 信任库")?;
                ensure!(
                    file.metadata()?.len() <= 8 * 1024 * 1024,
                    "Hive TLS 信任库超过 8 MiB"
                );
                let mut bytes = Vec::new();
                file.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
                ensure!(bytes.len() <= 8 * 1024 * 1024, "Hive TLS 信任库超过 8 MiB");
                if bytes.starts_with(&[0xfe, 0xed, 0xfe, 0xed]) {
                    super::tls_identity::validate_jks(&bytes)?;
                    let mut store = jks::KeyStore::new();
                    store
                        .load(bytes.as_slice(), password.as_bytes())
                        .context("Hive JKS 信任库或密码无效")?;
                    let aliases = store.aliases();
                    ensure!(aliases.len() <= 4096, "Hive 信任库证书超过 4096 项");
                    for alias in aliases {
                        if store.is_trusted_certificate_entry(&alias) {
                            roots.add(
                                store
                                    .get_trusted_certificate_entry(&alias)?
                                    .certificate
                                    .content
                                    .into(),
                            )?;
                        } else if store.is_private_key_entry(&alias) {
                            for cert in store.get_private_key_entry_certificate_chain(&alias)? {
                                roots.add(cert.content.into())?;
                            }
                        }
                    }
                } else if super::tls_identity::is_pkcs12(&bytes)
                    || format.eq_ignore_ascii_case("PKCS12")
                    || std::path::Path::new(&path).extension().is_some_and(|s| {
                        s.eq_ignore_ascii_case("p12") || s.eq_ignore_ascii_case("pfx")
                    })
                {
                    let store = p12_keystore::KeyStore::from_pkcs12(
                        &bytes,
                        &password,
                        p12_keystore::Pkcs12ImportPolicy::Raw,
                    )
                    .context("Hive PKCS12 信任库或密码无效")?;
                    ensure!(store.entries_len() <= 4096, "Hive 信任库证书超过 4096 项");
                    for (_, entry) in store.entries() {
                        match entry {
                            p12_keystore::KeyStoreEntry::Certificate(cert) => {
                                roots.add(cert.as_der().to_vec().into())?
                            }
                            p12_keystore::KeyStoreEntry::PrivateKeyChain(chain) => {
                                for cert in chain.certs() {
                                    roots.add(cert.as_der().to_vec().into())?;
                                }
                            }
                            p12_keystore::KeyStoreEntry::Secret(_) => {}
                        }
                    }
                } else if bytes.starts_with(b"-----BEGIN") {
                    for cert in pem::parse_many(&bytes)?
                        .into_iter()
                        .filter(|p| p.tag() == "CERTIFICATE")
                    {
                        roots.add(cert.into_contents().into())?;
                    }
                } else {
                    roots.add(bytes.into())?;
                }
            }
            Ok(roots)
        })
        .await??;
        let builder = rustls::ClientConfig::builder().with_root_certificates(roots);
        let client = if let Some((certificates, key)) = super::tls_identity::load(self).await? {
            builder
                .with_client_auth_cert(certificates, key)
                .context("Hive 客户端私钥与证书不匹配")?
        } else {
            builder.with_no_client_auth()
        };
        Ok(Arc::new(client))
    }
}
