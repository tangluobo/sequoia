//! ACME DNS-01 orchestration. Always remove only the challenge records we created.
use super::*;
use anyhow::{Context, bail};
use base64::Engine;
use instant_acme::{
    Account, AuthorizationStatus, ChallengeType, ExternalAccountKey, Identifier, NewAccount,
    NewOrder, OrderStatus, RetryPolicy,
};
use std::{collections::BTreeMap, time::Duration};
pub mod renewal;
pub type Control = crate::backup::Control;
const ACME_TIMEOUT: Duration = Duration::from_secs(180);
static ACTIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static FINISHED: tokio::sync::Notify = tokio::sync::Notify::const_new();
struct Cleanup;
impl Drop for Cleanup {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        FINISHED.notify_waiters();
    }
}
pub async fn wait_cleanup() {
    loop {
        let notify = FINISHED.notified();
        tokio::pin!(notify);
        notify.as_mut().enable();
        if ACTIVE.load(std::sync::atomic::Ordering::Acquire) == 0 {
            return;
        }
        notify.await;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub domain: String,
    pub dns_zone: String,
    pub email: String,
    pub wildcard: bool,
    pub storage: Connection,
    pub bucket: String,
    pub prefix: String,
    pub server_types: Vec<String>,
    pub zip_package: bool,
    pub keystore_password: String,
    pub certificate_authority: String,
    pub eab_kid: String,
    pub eab_hmac: String,
}
fn directory(authority: &str) -> &'static str {
    if authority.starts_with("ZeroSSL") {
        "https://acme.zerossl.com/v2/DV90"
    } else if authority.starts_with("Google") {
        "https://dv.acme-v02.api.pki.goog/directory"
    } else if authority.starts_with("SSL.com") {
        "https://acme.ssl.com/sslcom-dv-rsa"
    } else {
        "https://acme-v02.api.letsencrypt.org/directory"
    }
}
pub async fn issue(c: &Connection, r: Request, control: &Control) -> Result<String> {
    let connection = c.clone();
    run_owned(control, |control| async move {
        let issued = issue_owned(&connection, r, false, &mut None, &control).await?;
        match issued.cleanup_warning {
            Some(error) => Err(error),
            None => Ok(issued.message),
        }
    })
    .await
}

async fn run_owned<T, F>(control: &Control, run: impl FnOnce(Control) -> F) -> Result<T>
where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T>> + Send + 'static,
{
    struct CancelOnDrop(Option<Control>);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            if let Some(control) = &self.0 {
                control.cancel();
            }
        }
    }

    let mut cancel = CancelOnDrop(Some(control.clone()));
    let control = control.clone();
    let task = run(control);
    ACTIVE.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    let cleanup = Cleanup;
    // A closing tab or command queue can drop the caller while a DNS mutation
    // is in flight. Let the owned task receive its record ID and clean it up.
    let result = tokio::spawn(async move {
        let _cleanup = cleanup;
        task.await
    })
    .await;
    cancel.0 = None;
    result?
}

#[derive(Serialize, Deserialize)]
struct Material {
    chain: String,
    key: String,
}

struct Issued {
    message: String,
    expires_at: i64,
    material: Option<Material>,
    cleanup_warning: Option<anyhow::Error>,
}

async fn issue_owned(
    c: &Connection,
    r: Request,
    deploy: bool,
    account: &mut Option<Value>,
    control: &Control,
) -> Result<Issued> {
    control.check()?;
    ensure!(
        r.email.contains('@') && (!r.bucket.trim().is_empty() || deploy),
        "请填写邮箱并选择证书保存方式"
    );
    ensure!(
        !r.prefix.contains(['\\', '\0'])
            && !r.prefix.trim().split('/').any(|p| matches!(p, "." | "..")),
        "证书保存目录无效"
    );
    let domain = normalize_domain(&r.domain)?;
    let zone = normalize_domain(&r.dns_zone)?;
    let base = domain.trim_start_matches("*.");
    ensure!(
        !zone.starts_with("*.") && (base == zone || base.ends_with(&format!(".{zone}"))),
        "证书域名不属于当前 DNS 区域"
    );
    let dns = super::api::Client::new(c)?;
    let storage = if r.bucket.trim().is_empty() {
        None
    } else {
        ensure!(!r.server_types.is_empty(), "请选择服务器类型");
        ensure!(
            matches!(r.storage.kind(), "S3" | "ALIYUN_OSS"),
            "证书存储需要 S3/OSS 连接"
        );
        ensure!(
            !r.storage.text("username").trim().is_empty() && !r.storage.text("password").is_empty(),
            "证书存储需要 Access Key 和 Secret Key"
        );
        let mut storage_connection = r.storage.clone();
        storage_connection.set("bucket", r.bucket.trim());
        storage_connection.set("s3Prefix", "");
        Some(
            checked(
                control,
                crate::files::storage::Storage::connect(&storage_connection),
            )
            .await
            .context("建立证书存储连接失败")?,
        )
    };
    let mut created = Vec::new();
    let order = OrderContext {
        connection: c,
        request: &r,
        dns: &dns,
        zone: &zone,
        control,
    };
    let result = issue_order(order, storage.as_ref(), deploy, account, &mut created).await;
    let mut cleanup_error = None;
    let cleanup = created.iter().map(|id| {
        let (dns, zone) = (&dns, &zone);
        async move {
            dns.delete_record(zone, id)
                .await
                .with_context(|| format!("清理 {zone} 的验证 TXT 记录 {id} 失败"))
        }
    });
    for error in futures_util::future::join_all(cleanup)
        .await
        .into_iter()
        .filter_map(Result::err)
    {
        control.log(&format!("{error:#}"));
        cleanup_error.get_or_insert(error);
    }
    match (result, cleanup_error) {
        (Ok(mut issued), warning) => {
            issued.cleanup_warning = warning.map(|e| e.context("DNS TXT 清理失败"));
            Ok(issued)
        }
        (Err(e), Some(cleanup)) => Err(e.context(format!("验证记录清理失败：{cleanup:#}"))),
        (r, _) => r,
    }
}
async fn checked<T>(
    control: &Control,
    future: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    tokio::select! {
        biased;
        _ = control.wait_cancelled() => bail!("证书申请已取消"),
        result = tokio::time::timeout(ACME_TIMEOUT, future) => result.context("证书申请阶段超时")?,
    }
}
fn directory_url<'a>(connection: &'a Connection, request: &Request) -> &'a str {
    let custom = connection.text("acmeDirectory");
    if custom.is_empty() {
        directory(&request.certificate_authority)
    } else {
        custom
    }
}

struct OrderContext<'a> {
    connection: &'a Connection,
    request: &'a Request,
    dns: &'a api::Client,
    zone: &'a str,
    control: &'a Control,
}

async fn acme_account(
    c: &Connection,
    r: &Request,
    saved: &mut Option<Value>,
    control: &Control,
) -> Result<(Account, String)> {
    if let Some(value) = saved {
        control.log("正在连接已注册的证书账户…");
        let credentials: instant_acme::AccountCredentials = serde_json::from_value(value.clone())?;
        let key = account_key(&credentials);
        let account = checked(control, async {
            Ok(Account::builder_with_http(Box::new(AcmeHttp::new()?))
                .from_credentials(credentials)
                .await?)
        })
        .await
        .context("连接已注册的证书账户失败")?;
        return Ok((account, key));
    }
    control.log("正在注册证书账户…");
    let external = if r.certificate_authority.starts_with("Let's Encrypt") {
        None
    } else {
        ensure!(
            !r.eab_kid.trim().is_empty() && !r.eab_hmac.trim().is_empty(),
            "所选证书颁发机构需要 EAB KID 和 HMAC Key"
        );
        Some(ExternalAccountKey::new(
            r.eab_kid.clone(),
            &base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(r.eab_hmac.trim().trim_end_matches('='))?,
        ))
    };
    let url = directory_url(c, r);
    let contacts = [format!("mailto:{}", r.email.trim())];
    let contact = [contacts[0].as_str()];
    let (account, credentials) = checked(control, async {
        Ok(Account::builder_with_http(Box::new(AcmeHttp::new()?))
            .create(
                &NewAccount {
                    contact: &contact,
                    terms_of_service_agreed: true,
                    only_return_existing: false,
                },
                url.into(),
                external.as_ref(),
            )
            .await?)
    })
    .await
    .context("注册证书账户失败")?;
    *saved = Some(serde_json::to_value(&credentials)?);
    Ok((account, account_key(&credentials)))
}

fn account_key(credentials: &instant_acme::AccountCredentials) -> String {
    pem::encode(&pem::Pem::new(
        "PRIVATE KEY",
        credentials.private_key().secret_pkcs8_der(),
    ))
}

async fn issue_order(
    order: OrderContext<'_>,
    storage: Option<&crate::files::storage::Storage>,
    deploy: bool,
    saved_account: &mut Option<Value>,
    created: &mut Vec<String>,
) -> Result<Issued> {
    let OrderContext {
        connection: c,
        request: r,
        dns,
        zone,
        control,
    } = order;
    let (account, account_key) = acme_account(c, r, saved_account, control).await?;
    let domain = normalize_domain(&r.domain)?;
    let mut names = vec![domain.clone()];
    if r.wildcard && !domain.starts_with("*.") {
        names.push(format!("*.{domain}"));
    }
    let identifiers = names
        .iter()
        .cloned()
        .map(Identifier::Dns)
        .collect::<Vec<_>>();
    control.log("正在创建证书订单…");
    let mut order = checked(control, async {
        Ok(account.new_order(&NewOrder::new(&identifiers)).await?)
    })
    .await
    .context("创建证书订单失败")?;
    let mut dns_records = BTreeMap::<String, Vec<String>>::new();
    let suffix = format!(".{zone}");
    {
        let mut auths = order.authorizations();
        while let Some(auth) = auths.next().await {
            control.check()?;
            let mut auth = auth.context("读取 ACME 域名授权失败")?;
            if auth.status == AuthorizationStatus::Valid {
                continue;
            }
            ensure!(
                auth.status == AuthorizationStatus::Pending,
                "{} 的域名授权状态为 {:?}",
                auth.identifier(),
                auth.status
            );
            let challenge = auth
                .challenge(ChallengeType::Dns01)
                .context("证书机构未提供 DNS-01 验证")?;
            let name = format!(
                "_acme-challenge.{}",
                challenge.identifier().to_string().trim_start_matches("*.")
            );
            let rr = name
                .strip_suffix(&suffix)
                .context("DNS 验证记录不属于当前云域名")?;
            let digest = challenge.key_authorization().dns_value();
            control.log(&format!("正在添加 DNS TXT 记录：{name}"));
            // Do not race cancellation with a mutation: keep the ID so cleanup can always run.
            let id = dns
                .save_record(
                    zone,
                    &Record {
                        rr: rr.into(),
                        kind: "TXT".into(),
                        value: digest.clone(),
                        ttl: 600,
                        ..Default::default()
                    },
                )
                .await
                .with_context(|| format!("创建验证 TXT 记录 {name} 失败"))?;
            control.log(&format!("已创建 DNS TXT 记录 {name}（记录 ID {id}）"));
            created.push(id);
            dns_records.entry(name).or_default().push(digest);
        }
    }
    // A base domain and its wildcard share one TXT record set. Publish both
    // values before a recursive lookup can cache the incomplete set.
    for (name, digests) in dns_records {
        wait_dns(&name, &digests, c.text("certificateDnsServer"), control).await?;
    }
    {
        let mut auths = order.authorizations();
        while let Some(auth) = auths.next().await {
            control.check()?;
            let mut auth = auth.context("读取 ACME 域名授权失败")?;
            if auth.status == AuthorizationStatus::Valid {
                continue;
            }
            let mut challenge = auth
                .challenge(ChallengeType::Dns01)
                .context("证书机构未提供 DNS-01 验证")?;
            let identifier = challenge.identifier().to_string();
            control.log(&format!("正在验证域名所有权：{identifier}"));
            checked(control, async { Ok(challenge.set_ready().await?) })
                .await
                .with_context(|| format!("提交 {identifier} 的 DNS-01 验证失败"))?;
        }
    }
    let retries = RetryPolicy::new()
        .initial_delay(Duration::from_secs(2))
        .backoff(1.0)
        .timeout(ACME_TIMEOUT);
    let status = checked(control, async { Ok(order.poll_ready(&retries).await?) })
        .await
        .context("等待证书机构验证域名失败")?;
    if status == OrderStatus::Invalid {
        let detail = checked(control, async { dns_rejection(&mut order).await })
            .await
            .unwrap_or_else(|error| format!("读取域名验证详情失败：{error:#}"));
        bail!("证书机构拒绝了 DNS-01 验证：{detail}");
    }
    ensure!(status == OrderStatus::Ready, "证书订单未就绪：{status:?}");
    control.log("正在生成 RSA 2048 证书密钥并签发…");
    let key = tokio::task::spawn_blocking(|| -> Result<_> {
        use rsa::pkcs8::EncodePrivateKey;
        let private = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048)?;
        let pem = private.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)?;
        Ok(rcgen::KeyPair::from_pem(pem.as_str())?)
    })
    .await??;
    let mut params = rcgen::CertificateParams::new(names)?;
    // ACME identifiers are in the SAN extension. Clear rcgen's default Common Name.
    params.distinguished_name = rcgen::DistinguishedName::new();
    let csr = params.serialize_request(&key)?;
    checked(control, async { Ok(order.finalize_csr(csr.der()).await?) })
        .await
        .context("提交证书签名请求失败")?;
    let chain = checked(control, async {
        Ok(order.poll_certificate(&retries).await?)
    })
    .await
    .context("等待证书签发失败")?;
    ensure!(chain.len() <= 1024 * 1024, "证书链过大");
    control.check()?;
    let material = Material {
        chain,
        key: key.serialize_pem(),
    };
    let expires_at = certificate_expiry(&material.chain)?;
    if let Some(storage) = storage {
        upload(r, storage, &material, account_key, control).await?;
    }
    control.log("证书申请完成");
    Ok(Issued {
        message: if storage.is_some() {
            format!("证书已保存到 {}/{}", r.bucket, r.prefix)
        } else {
            "证书已签发".into()
        },
        expires_at,
        material: deploy.then_some(material),
        cleanup_warning: None,
    })
}

fn certificate_expiry(chain: &str) -> Result<i64> {
    use x509_parser::prelude::FromDer;
    let leaf = pem::parse(chain).context("无法读取签发的证书")?;
    let (_, certificate) = x509_parser::certificate::X509Certificate::from_der(leaf.contents())
        .map_err(|_| anyhow::anyhow!("无法读取签发证书的有效期"))?;
    Ok(certificate.validity().not_after.timestamp())
}

async fn upload(
    r: &Request,
    storage: &crate::files::storage::Storage,
    material: &Material,
    account_key: String,
    control: &Control,
) -> Result<()> {
    control.log("正在生成服务器格式…");
    let request = r.clone();
    let chain = material.chain.clone();
    let key = material.key.clone();
    let files = tokio::task::spawn_blocking(move || package(&request, &chain, &key, &account_key))
        .await??;
    let prefix = r.prefix.trim().trim_matches('/');
    ensure!(
        !prefix.split('/').any(|x| matches!(x, ".." | ".")),
        "证书保存目录无效"
    );
    for (name, bytes) in files {
        control.check()?;
        control.log(&format!("正在保存 {name}…"));
        let key = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        storage
            .operator
            .write_with(&key, bytes)
            .content_type(if key.ends_with(".zip") {
                "application/zip"
            } else {
                "application/octet-stream"
            })
            .await
            .with_context(|| format!("写入证书存储 {}/{} 失败", r.bucket, key))?;
    }
    Ok(())
}
async fn dns_rejection(order: &mut instant_acme::Order) -> Result<String> {
    let mut auths = order.authorizations();
    let mut failures = Vec::new();
    while let Some(auth) = auths.next().await {
        let mut auth = auth?;
        let state = auth.refresh().await?;
        if state.status == AuthorizationStatus::Valid {
            continue;
        }
        let reason = state
            .challenges
            .iter()
            .find(|challenge| challenge.r#type == ChallengeType::Dns01)
            .and_then(|challenge| challenge.error.as_ref())
            .map_or_else(|| format!("{:?}", state.status), ToString::to_string);
        failures.push(format!("{}：{reason}", state.identifier()));
    }
    if failures.is_empty() {
        failures.push("证书订单状态为 Invalid，未返回域名验证错误详情".into());
    }
    Ok(failures.join("；"))
}

pub fn package(
    r: &Request,
    chain: &str,
    key: &str,
    account_key: &str,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let certs = pem::parse_many(chain)?;
    ensure!(!certs.is_empty(), "证书链为空");
    let first = pem::encode(&certs[0]);
    let private = pem::parse(key)?;
    let name = r
        .domain
        .replace('*', "wildcard")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    let mut files = BTreeMap::new();
    for kind in &r.server_types {
        let mut put = |directory: &str, name: &str, value: &str| {
            files.insert(format!("{directory}/{name}"), value.as_bytes().to_vec());
        };
        if kind.starts_with("Nginx") {
            put("nginx", &format!("{name}.pem"), chain);
            put("nginx", &format!("{name}.key"), key);
        } else if kind.starts_with("Apache") {
            put("apache", &format!("{name}.crt"), &first);
            put("apache", &format!("{name}-chain.crt"), chain);
            put("apache", &format!("{name}.key"), key);
        } else if kind.starts_with("Tomcat") || kind.starts_with("IIS") {
            let mut ks = p12_keystore::KeyStore::new();
            let chain = certs
                .iter()
                .map(|c| p12_keystore::Certificate::from_der(c.contents()))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ks.add_entry(
                "certificate",
                p12_keystore::KeyStoreEntry::PrivateKeyChain(p12_keystore::PrivateKeyChain::new(
                    "certificate",
                    p12_keystore::PrivateKey::from_der(private.contents())?,
                    chain,
                )),
            );
            files.insert(
                format!(
                    "{}/{name}.pfx",
                    if kind.starts_with("Tomcat") {
                        "tomcat"
                    } else {
                        "iis"
                    }
                ),
                ks.writer(&r.keystore_password).write()?,
            );
        } else if kind.contains("JKS") {
            let mut ks = jks::KeyStore::with_options(jks::KeyStoreOptions {
                min_password_len: 0,
                ..Default::default()
            });
            let password = r.keystore_password.as_bytes();
            ks.set_private_key_entry(
                "certificate",
                jks::PrivateKeyEntry {
                    creation_time: std::time::SystemTime::now(),
                    private_key: private.contents().into(),
                    certificate_chain: certs
                        .iter()
                        .map(|c| jks::Certificate {
                            cert_type: "X509".into(),
                            content: c.contents().into(),
                        })
                        .collect(),
                },
                password,
            )?;
            let mut bytes = Vec::new();
            ks.store(&mut bytes, password)?;
            files.insert(format!("jks/{name}.jks"), bytes);
        } else {
            put("pem", "fullchain.pem", chain);
            put("pem", "cert.pem", &first);
            put("pem", "private.key", key);
            put("pem", "account.key", account_key);
        }
    }
    if r.zip_package {
        use std::io::Write;
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in files {
            z.start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )?;
            z.write_all(&bytes)?;
        }
        Ok(BTreeMap::from([(
            format!("{name}-certificates.zip"),
            z.finish()?.into_inner(),
        )]))
    } else {
        Ok(files)
    }
}
async fn wait_dns(name: &str, expected: &[String], custom: &str, control: &Control) -> Result<()> {
    if !custom.is_empty() {
        return wait_dns_using(name, expected, &[TxtResolver::Udp(custom)], control).await;
    }
    let https = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let servers = [
        TxtResolver::Udp("119.29.29.29:53"),
        TxtResolver::Udp("1.1.1.1:53"),
        TxtResolver::Https("https://doh.pub/dns-query", https.clone()),
        TxtResolver::Https("https://dns.google/dns-query", https),
    ];
    wait_dns_using(name, expected, &servers, control).await
}

enum TxtResolver<'a> {
    Udp(&'a str),
    Https(&'a str, reqwest::Client),
}

async fn wait_dns_using(
    name: &str,
    expected: &[String],
    servers: &[TxtResolver<'_>],
    control: &Control,
) -> Result<()> {
    // Allow a resolver to expire a 180-second negative cache entry even
    // when it was obtained before the DNS provider published the record.
    const PROPAGATION_TIMEOUT: Duration = Duration::from_secs(300);
    const PROGRESS_INTERVAL: Duration = Duration::from_secs(30);
    control.log(&format!(
        "等待 DNS TXT 记录生效：{name}（最多 {} 秒）",
        PROPAGATION_TIMEOUT.as_secs()
    ));
    let started = tokio::time::Instant::now();
    let mut next_report = started + PROGRESS_INTERVAL;
    let mut observations = vec![String::new(); servers.len()];
    let poll = async {
        loop {
            use futures_util::{StreamExt, stream::FuturesUnordered};
            let mut responses = servers
                .iter()
                .enumerate()
                .map(|(index, server)| async move {
                    let (address, result) = match server {
                        TxtResolver::Udp(address) => (address, txt(address, name).await),
                        TxtResolver::Https(address, client) => {
                            (address, txt_over_https(client, address, name).await)
                        }
                    };
                    (index, address, result)
                })
                .collect::<FuturesUnordered<_>>();
            while let Some((index, address, result)) = responses.next().await {
                observations[index] = match result {
                    Ok(values) if expected.iter().all(|value| values.contains(value)) => {
                        control.log(&format!("已通过 {address} 查到 {name} 的全部验证值"));
                        return;
                    }
                    Ok(values) if values.is_empty() => format!("{address} 未返回 TXT 记录"),
                    Ok(_) => format!("{address} 返回的 TXT 未包含本次申请的全部验证值"),
                    Err(error) => format!("{address} 查询失败（{error}）"),
                };
            }
            let now = tokio::time::Instant::now();
            if now >= next_report {
                control.log(&format!(
                    "{name} 已等待 {} 秒。{}",
                    (now - started).as_secs(),
                    observations.join("；")
                ));
                next_report = now + PROGRESS_INTERVAL;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    };
    let result = tokio::select! {
        biased;
        _ = control.wait_cancelled() => bail!("证书申请已取消"),
        result = tokio::time::timeout_at(started + PROPAGATION_TIMEOUT, poll) => result,
    };
    ensure!(
        result.is_ok(),
        "等待 DNS TXT 记录生效超时：{name}。{}。请检查该名称的公网解析及域名的 NS 委派。",
        observations.join("；")
    );
    Ok(())
}
/// Bounded UDP DNS query; parse TXT answers and skip compressed owner names safely.
pub async fn txt(server: &str, name: &str) -> Result<Vec<String>> {
    let id = (chrono::Utc::now().timestamp_subsec_nanos() & 0xffff) as u16;
    let q = txt_query(name, id)?;
    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(server).await?;
    socket.send(&q).await?;
    let mut b = [0u8; 4096];
    let n = tokio::time::timeout(Duration::from_secs(3), socket.recv(&mut b)).await??;
    parse_txt_answer(&b[..n], id)
}

const DNS_MESSAGE: &str = "application/dns-message";

async fn txt_over_https(client: &reqwest::Client, url: &str, name: &str) -> Result<Vec<String>> {
    // RFC 8484 recommends a zero DNS transaction ID over HTTPS.
    let query = txt_query(name, 0)?;
    let mut response = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, DNS_MESSAGE)
        .header(reqwest::header::ACCEPT, DNS_MESSAGE)
        .body(query)
        .send()
        .await
        .map_err(reqwest::Error::without_url)?;
    ensure!(
        response.status().is_success(),
        "DoH HTTP {}",
        response.status()
    );
    ensure!(
        response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case(DNS_MESSAGE)),
        "DoH 响应类型无效"
    );
    let mut answer = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(reqwest::Error::without_url)?
    {
        ensure!(
            answer.len() + chunk.len() <= u16::MAX as usize,
            "DoH 响应过大"
        );
        answer.extend_from_slice(&chunk);
    }
    parse_txt_answer(&answer, 0)
}

fn txt_query(name: &str, id: u16) -> Result<Vec<u8>> {
    let mut q = Vec::from(id.to_be_bytes());
    q.extend_from_slice(&[1, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.trim_end_matches('.').split('.') {
        ensure!(label.len() <= 63, "DNS 标签过长");
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 16, 0, 1]);
    Ok(q)
}

fn parse_txt_answer(b: &[u8], id: u16) -> Result<Vec<String>> {
    ensure!(
        b.len() >= 12 && b[..2] == id.to_be_bytes() && b[2] & 0x80 != 0,
        "DNS 响应无效"
    );
    ensure!(b[2] & 0x02 == 0, "DNS 响应被截断");
    let rcode = b[3] & 0x0f;
    ensure!(rcode != 3, "DNS 名称不存在（NXDOMAIN）");
    ensure!(rcode == 0, "DNS 返回错误（RCODE {rcode}）");
    fn skip(b: &[u8], p: &mut usize) -> Result<()> {
        loop {
            let n = *b.get(*p).context("DNS 名称截断")? as usize;
            *p += 1;
            if n & 0xc0 == 0xc0 {
                *p += 1;
                break;
            }
            if n == 0 {
                break;
            }
            ensure!(n <= 63, "DNS 名称无效");
            *p += n;
            ensure!(*p <= b.len(), "DNS 名称截断");
        }
        Ok(())
    }
    let mut p = 12;
    for _ in 0..u16::from_be_bytes([b[4], b[5]]) {
        skip(b, &mut p)?;
        p += 4;
    }
    let mut values = Vec::new();
    for _ in 0..u16::from_be_bytes([b[6], b[7]]) {
        skip(b, &mut p)?;
        ensure!(p + 10 <= b.len(), "DNS 答案截断");
        let kind = u16::from_be_bytes([b[p], b[p + 1]]);
        let len = u16::from_be_bytes([b[p + 8], b[p + 9]]) as usize;
        p += 10;
        ensure!(p + len <= b.len(), "DNS TXT 截断");
        let end = p + len;
        if kind == 16 {
            let mut value = Vec::new();
            while p < end {
                let n = b[p] as usize;
                p += 1;
                ensure!(p + n <= end, "DNS TXT 字符串截断");
                value.extend_from_slice(&b[p..p + n]);
                p += n;
            }
            values.push(String::from_utf8(value)?);
        }
        p = end;
    }
    Ok(values)
}

/// Reuse the application's TLS transport while bounding every ACME response.
struct AcmeHttp(reqwest::Client);
impl AcmeHttp {
    fn new() -> Result<Self> {
        Ok(Self(
            reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        ))
    }
}
impl instant_acme::HttpClient for AcmeHttp {
    fn request(
        &self,
        request: http::Request<instant_acme::BodyWrapper<bytes::Bytes>>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = std::result::Result<instant_acme::BytesResponse, instant_acme::Error>,
                > + Send,
        >,
    > {
        let client = self.0.clone();
        Box::pin(async move {
            let result: Result<_> = async {
                use http_body_util::BodyExt;
                let (parts, body) = request.into_parts();
                let url = url::Url::parse(&parts.uri.to_string())?;
                ensure!(
                    url.scheme() == "https"
                        || (url.scheme() == "http"
                            && url
                                .host_str()
                                .and_then(|s| s.parse::<std::net::IpAddr>().ok())
                                .is_some_and(|ip| ip.is_loopback())),
                    "ACME 需要 HTTPS 或隔离回环端点"
                );
                let body = body.collect().await?.to_bytes();
                ensure!(body.len() <= 1024 * 1024, "ACME 请求超过 1 MiB");
                let mut response = client
                    .request(parts.method, url)
                    .headers(parts.headers)
                    .body(body)
                    .send()
                    .await
                    .map_err(reqwest::Error::without_url)?;
                let mut headers = response.headers().clone();
                headers.remove("transfer-encoding");
                let status = response.status();
                let mut b = Vec::new();
                while let Some(chunk) = response
                    .chunk()
                    .await
                    .map_err(reqwest::Error::without_url)?
                {
                    ensure!(
                        b.len() + chunk.len() <= 2 * 1024 * 1024,
                        "ACME 响应超过 2 MiB"
                    );
                    b.extend_from_slice(&chunk);
                }
                let mut response =
                    http::Response::new(http_body_util::Full::new(bytes::Bytes::from(b)));
                *response.status_mut() = status;
                *response.headers_mut() = headers;
                Ok(instant_acme::BytesResponse::from(response))
            }
            .await;
            result.map_err(|e| instant_acme::Error::Other(e.into_boxed_dyn_error()))
        })
    }
}

fn normalize_domain(value: &str) -> Result<String> {
    let value = value.trim().trim_end_matches('.');
    let wildcard = value.starts_with("*.");
    let base = value.trim_start_matches("*.");
    let url::Host::Domain(name) = url::Host::parse(base)? else {
        bail!("证书域名无效")
    };
    ensure!(
        name.len() <= 253
            && name.split('.').all(|part| !part.is_empty()
                && part.len() <= 63
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')),
        "证书域名无效"
    );
    Ok(if wildcard { format!("*.{name}") } else { name })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dns_over_https_continues_when_udp_returns_nxdomain() -> Result<()> {
        const NAME: &str = "_acme-challenge.owned.test";
        const VALUES: [&str; 2] = ["base-validation", "wildcard-validation"];

        let udp = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
        let udp_address = udp.local_addr()?.to_string();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/dns-query", listener.local_addr()?);
        let app = axum::Router::new().route(
            "/dns-query",
            axum::routing::post(
                |headers: axum::http::HeaderMap, body: bytes::Bytes| async move {
                    assert_eq!(headers[axum::http::header::CONTENT_TYPE], DNS_MESSAGE);
                    assert_eq!(headers[axum::http::header::ACCEPT], DNS_MESSAGE);
                    assert_eq!(body.as_ref(), txt_query(NAME, 0).unwrap());
                    let mut reply = body.to_vec();
                    reply[2..4].copy_from_slice(&[0x81, 0x80]);
                    reply[6..8].copy_from_slice(&(VALUES.len() as u16).to_be_bytes());
                    for value in VALUES {
                        reply.extend_from_slice(&[0xc0, 12, 0, 16, 0, 1, 0, 0, 0, 60]);
                        reply.extend_from_slice(&((value.len() + 1) as u16).to_be_bytes());
                        reply.push(value.len() as u8);
                        reply.extend_from_slice(value.as_bytes());
                    }
                    ([(axum::http::header::CONTENT_TYPE, DNS_MESSAGE)], reply)
                },
            ),
        );
        let servers = [
            TxtResolver::Udp(&udp_address),
            TxtResolver::Https(&url, reqwest::Client::builder().no_proxy().build()?),
        ];
        let expected = VALUES.map(str::to_owned);
        let control = Control::default();
        let check = async {
            let reply_nxdomain = async {
                let mut query = [0u8; 512];
                let (len, peer) = udp.recv_from(&mut query).await?;
                query[2..4].copy_from_slice(&[0x81, 0x83]);
                udp.send_to(&query[..len], peer).await
            };
            let (result, udp_sent) = tokio::join!(
                wait_dns_using(NAME, &expected, &servers, &control),
                reply_nxdomain,
            );
            udp_sent?;
            result
        };
        tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(8), check) => result??,
            result = axum::serve(listener, app) => bail!("test DoH server exited: {result:?}"),
        }
        assert!(
            control
                .read(|progress| progress
                    .logs
                    .iter()
                    .any(|line| { line.contains(&url) && line.contains("全部验证值") }))
                .unwrap()
        );
        Ok(())
    }
}
