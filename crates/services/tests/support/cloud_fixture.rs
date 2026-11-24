//! Owned loopback cloud, ACME, DNS and S3 fixture. It verifies the actual JWS signatures.
use anyhow::{Context, Result, ensure};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::State as AxumState,
    http::{Request, StatusCode},
    response::Response,
    routing::any,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tomato_services::cloud::{Record, certificate};
pub struct Fixture {
    pub endpoint: String,
    pub dns: SocketAddr,
    pub state: Arc<Shared>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
pub struct Shared {
    pub inner: Mutex<Data>,
    pub issuer: rcgen::Issuer<'static, rcgen::KeyPair>,
}
#[derive(Default)]
pub struct Data {
    pub base: String,
    pub records: BTreeMap<String, Record>,
    pub writes: BTreeMap<String, Vec<u8>>,
    pub calls: Vec<String>,
    pub next_id: u64,
    pub keys: Option<Value>,
    pub auth_ready: usize,
    pub names: Vec<String>,
    pub chain: String,
    pub cache_txt_answers: bool,
    pub first_txt_query_nxdomain: bool,
    pub cancel_before_ready: bool,
    pub reject_save: bool,
    pub public_ip: String,
    pub required_eab: bool,
    pub reject_delete: bool,
    pub reject_bucket_list: bool,
    pub bucket_list_delay: std::time::Duration,
    pub deny_authorization: bool,
    pub order_retry_after: Option<u64>,
    pub certificate_expiry: Option<(i32, u8, u8)>,
}
impl Fixture {
    pub async fn start() -> Result<Self> {
        let params = rcgen::CertificateParams::new(vec!["owned.test".into()])?;
        let issuer = rcgen::Issuer::new(params, rcgen::KeyPair::generate()?);
        let state = Arc::new(Shared {
            inner: Mutex::new(Data::default()),
            issuer,
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let endpoint = format!("http://{addr}");
        state.inner.lock().unwrap().base = endpoint.clone();
        let app = Router::new()
            .fallback(any(handler))
            .with_state(state.clone());
        let http = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let dns = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
        let dns_addr = dns.local_addr()?;
        let dns_state = state.clone();
        let dns = tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            let mut cached = BTreeMap::<Vec<u8>, Vec<String>>::new();
            while let Ok((n, peer)) = dns.recv_from(&mut buf).await {
                if n < 12 {
                    continue;
                }
                let mut end = 12;
                let mut labels = Vec::new();
                while end < n && buf[end] != 0 {
                    let next = end + usize::from(buf[end]) + 1;
                    if next >= n {
                        break;
                    }
                    labels.push(String::from_utf8_lossy(&buf[end + 1..next]));
                    end = next;
                }
                if end + 5 > n || buf[end] != 0 {
                    continue;
                }
                end += 5;
                let name = labels.join(".");
                let (nxdomain, values) = {
                    let mut state = dns_state.inner.lock().unwrap();
                    let lookup = || {
                        state
                            .records
                            .values()
                            .filter(|r| r.kind == "TXT" && name == format!("{}.owned.test", r.rr))
                            .map(|r| r.value.clone())
                            .collect()
                    };
                    if state.first_txt_query_nxdomain {
                        state.first_txt_query_nxdomain = false;
                        (true, Vec::new())
                    } else if state.cache_txt_answers {
                        // Keep the first answer for longer than the test deadline.
                        (
                            false,
                            cached
                                .entry(buf[12..end].to_vec())
                                .or_insert_with(lookup)
                                .clone(),
                        )
                    } else {
                        (false, lookup())
                    }
                };
                let mut out = buf[..end].to_vec();
                out[2] = 0x81;
                out[3] = if nxdomain { 0x83 } else { 0x80 };
                out[6..8].copy_from_slice(&(values.len() as u16).to_be_bytes());
                for s in values {
                    out.extend_from_slice(&[0xc0, 12, 0, 16, 0, 1, 0, 0, 0, 30]);
                    out.extend_from_slice(&((s.len() + 1) as u16).to_be_bytes());
                    out.push(s.len() as u8);
                    out.extend_from_slice(s.as_bytes());
                }
                let _ = dns.send_to(&out, peer).await;
            }
        });
        Ok(Self {
            endpoint,
            dns: dns_addr,
            state,
            tasks: vec![http, dns],
        })
    }
    pub fn connection(&self, kind: &str) -> tomato_core::Connection {
        let mut c = tomato_core::Connection::new(kind);
        c.id = format!("owned-{kind}");
        c.set("username", "fixture-id");
        c.set("password", "fixture-secret");
        c.set("cloudApiEndpoint", format!("{}/cloud", self.endpoint));
        c.set("acmeDirectory", format!("{}/acme/directory", self.endpoint));
        c.set("certificateDnsServer", self.dns.to_string());
        c
    }
    pub fn storage(&self) -> tomato_core::Connection {
        let mut c = tomato_core::Connection::new("S3");
        c.name = "隔离证书存储".into();
        c.set("endpoint", self.endpoint.as_str());
        c.set("region", "us-east-1");
        c.set("pathStyleAccess", true);
        c.set("username", "fixture-storage");
        c.set("password", "fixture-storage-secret");
        c
    }
    pub fn request(&self) -> certificate::Request {
        certificate::Request {
            domain: "owned.test".into(),
            dns_zone: "owned.test".into(),
            email: "owner@owned.test".into(),
            wildcard: true,
            storage: self.storage(),
            bucket: "cert-bucket".into(),
            prefix: "certificates/owned.test/".into(),
            server_types: vec![
                "Nginx".into(),
                "Apache".into(),
                "Tomcat (PFX)".into(),
                "IIS (PFX)".into(),
                "Java JKS".into(),
                "通用 PEM".into(),
            ],
            zip_package: false,
            keystore_password: "fixture-password".into(),
            certificate_authority: "Let's Encrypt（90 天）".into(),
            eab_kid: String::new(),
            eab_hmac: String::new(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
async fn handler(AxumState(state): AxumState<Arc<Shared>>, request: Request<Body>) -> Response {
    let result = handle(state, request).await;
    match result {
        Ok(v) => v,
        Err(e) => Response::builder()
            .status(500)
            .body(Body::from(format!("fixture: {e:#}")))
            .unwrap(),
    }
}
async fn handle(state: Arc<Shared>, request: Request<Body>) -> Result<Response> {
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, 4 * 1024 * 1024).await?;
    let path = parts.uri.path();
    if path == "/" && parts.method == axum::http::Method::GET {
        let delay = state.inner.lock().unwrap().bucket_list_delay;
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
    }
    let mut s = state.inner.lock().unwrap();
    let base = s.base.clone();
    let mut location = None;
    let mut status = StatusCode::OK;
    let mut retry_after = None;
    let value = if path == "/cloud" {
        let tc = parts.method == axum::http::Method::POST;
        let (action, params) = if tc {
            ensure!(
                parts.headers.get("authorization").is_some_and(|v| v
                    .as_bytes()
                    .starts_with(b"TC3-HMAC-SHA256 Credential=fixture-id/")),
                "missing TC3 signature"
            );
            (
                parts.headers["x-tc-action"].to_str()?.to_owned(),
                serde_json::from_slice::<Value>(&body)?,
            )
        } else {
            let params: BTreeMap<String, String> =
                url::form_urlencoded::parse(parts.uri.query().unwrap_or_default().as_bytes())
                    .into_owned()
                    .collect();
            ensure!(
                params.get("AccessKeyId").is_some_and(|v| v == "fixture-id")
                    && params.contains_key("Signature"),
                "missing Aliyun signature"
            );
            let action = params["Action"].clone();
            (action, serde_json::to_value(params)?)
        };
        s.calls.push(action.clone());
        let t = |name: &str| tomato_services::cloud::text(&params, name);
        let v = match action.as_str() {
            "GetCallerIdentity" => json!({"AccountId":"fixture"}),
            "DescribeRegions" => {
                if tc {
                    json!({"RegionSet":[{"Region":"ap-guangzhou","RegionState":"AVAILABLE","RegionName":"广州"}]})
                } else {
                    json!({"Regions":{"Region":[{"RegionId":"cn-hangzhou","LocalName":"杭州"}]}})
                }
            }
            "DescribeInstances" => {
                let item = json!({"InstanceId":"owned-instance","InstanceName":"隔离实例","InstanceState":"RUNNING","Status":"Running","PublicIpAddresses":["127.0.0.1"],"PublicIpAddress":{"IpAddress":["127.0.0.1"]}});
                if tc {
                    json!({"InstanceSet":[item]})
                } else {
                    json!({"Instances":{"Instance":[item]}})
                }
            }
            "DescribeDomainList" => {
                json!({"DomainList":[{"Name":"owned.test","DomainId":42,"Status":"ENABLE"}]})
            }
            "QueryDomainList" => {
                json!({"Data":{"Domain":[{"DomainName":"owned.test","InstanceId":"42","DomainStatus":"1"}]}})
            }
            "DescribeRecordList" | "DescribeDomainRecords" => {
                let rows=s.records.values().map(|r|if tc{json!({"RecordId":r.record_id.parse::<u64>().unwrap(),"Name":r.rr,"Type":r.kind,"Value":r.value,"TTL":r.ttl,"Line":r.line,"Status":"ENABLE"})}else{json!({"RecordId":r.record_id,"RR":r.rr,"Type":r.kind,"Value":r.value,"TTL":r.ttl,"Line":r.line,"Status":"ENABLE"})}).collect::<Vec<_>>();
                if tc {
                    json!({"RecordList":rows})
                } else {
                    json!({"DomainRecords":{"Record":rows}})
                }
            }
            "CreateRecord" | "AddDomainRecord" | "ModifyRecord" | "UpdateDomainRecord" => {
                ensure!(!s.reject_save, "owned save failure");
                if tc {
                    ensure!(t("Domain") == "owned.test", "incorrect DNSPod zone");
                    if matches!(params["RecordType"].as_str(), Some("MX" | "HTTPS" | "SVCB")) {
                        ensure!(
                            params["MX"].as_u64().is_some(),
                            "missing DNSPod MX priority"
                        );
                    } else if params.get("MX").is_some() {
                        return Ok(Response::builder()
                            .header("Content-Type", "application/json")
                            .body(Body::from(r#"{"Response":{"Error":{"Code":"InvalidParameter.MxInvalid","Message":"Unexpected MX priority for this record type"}}}"#))?);
                    }
                }
                let id = if matches!(action.as_str(), "ModifyRecord" | "UpdateDomainRecord") {
                    t("RecordId")
                } else {
                    s.next_id += 1;
                    (1000 + s.next_id).to_string()
                };
                let r = Record {
                    record_id: id.clone(),
                    rr: t(if tc { "SubDomain" } else { "RR" }),
                    kind: t(if tc { "RecordType" } else { "Type" }),
                    value: t("Value"),
                    ttl: t("TTL").parse().unwrap_or(600),
                    line: t(if tc { "RecordLine" } else { "Line" }),
                    status: "ENABLE".into(),
                    ..Default::default()
                };
                s.records.insert(id.clone(), r);
                if tc {
                    json!({"RecordId":id.parse::<u64>()?})
                } else {
                    json!({"RecordId":id})
                }
            }
            "DeleteRecord" | "DeleteDomainRecord" => {
                ensure!(!s.reject_delete, "owned deletion failure");
                if tc {
                    ensure!(t("Domain") == "owned.test", "incorrect DNSPod zone");
                }
                s.records.remove(&t("RecordId"));
                json!({})
            }
            _ => anyhow::bail!("unexpected cloud action {action}"),
        };
        if tc { json!({"Response":v}) } else { v }
    } else if path == "/acme/directory" {
        json!({"newNonce":format!("{base}/acme/nonce"),"newAccount":format!("{base}/acme/account"),"newOrder":format!("{base}/acme/new-order")})
    } else if path == "/acme/nonce" {
        Value::Null
    } else if path.starts_with("/acme/") {
        let jws: Value = serde_json::from_slice(&body)?;
        let protected: Value =
            serde_json::from_slice(&B64.decode(jws["protected"].as_str().context("protected")?)?)?;
        let jwk = if let Some(jwk) = protected.get("jwk") {
            jwk.clone()
        } else {
            s.keys.clone().context("account key not set")?
        };
        let mut pubkey = vec![4];
        pubkey.extend(B64.decode(jwk["x"].as_str().context("x")?)?);
        pubkey.extend(B64.decode(jwk["y"].as_str().context("y")?)?);
        let signed = format!(
            "{}.{}",
            jws["protected"].as_str().context("protected")?,
            jws["payload"].as_str().context("payload")?
        );
        ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, &pubkey)
            .verify(
                signed.as_bytes(),
                &B64.decode(jws["signature"].as_str().context("signature")?)?,
            )
            .map_err(|_| anyhow::anyhow!("invalid ACME JWS"))?;
        ensure!(
            protected["url"] == format!("{base}{path}"),
            "incorrect ACME URL binding"
        );
        let payload = B64.decode(jws["payload"].as_str().context("payload")?)?;
        let payload = if payload.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice::<Value>(&payload)?
        };
        s.calls.push(path.into());
        if path == "/acme/account" {
            if s.required_eab {
                let binding = &payload["externalAccountBinding"];
                let p = binding["protected"].as_str().context("EAB protected")?;
                let b = binding["payload"].as_str().context("EAB payload")?;
                let protected: Value = serde_json::from_slice(&B64.decode(p)?)?;
                ensure!(
                    protected["kid"] == "fixture-eab"
                        && protected["url"] == format!("{base}{path}"),
                    "EAB binding invalid"
                );
                ring::hmac::verify(
                    &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &[7u8; 32]),
                    format!("{p}.{b}").as_bytes(),
                    &B64.decode(binding["signature"].as_str().context("EAB signature")?)?,
                )
                .map_err(|_| anyhow::anyhow!("EAB HMAC invalid"))?;
                let key: Value = serde_json::from_slice(&B64.decode(b)?)?;
                ensure!(key == jwk, "EAB bound another account key");
            }
            s.keys = Some(jwk);
            status = StatusCode::CREATED;
            location = Some(format!("{base}/acme/account/1"));
            json!({"status":"valid"})
        } else if path == "/acme/new-order" {
            s.names = payload["identifiers"]
                .as_array()
                .context("identifiers")?
                .iter()
                .map(|x| x["value"].as_str().unwrap_or_default().to_owned())
                .collect();
            s.auth_ready = 0;
            s.chain.clear();
            status = StatusCode::CREATED;
            location = Some(format!("{base}/acme/order"));
            order(&s)
        } else if path == "/acme/order" {
            if let Some(delay) = s.order_retry_after.take() {
                retry_after = Some(delay);
                let mut waiting = order(&s);
                waiting["status"] = json!("pending");
                waiting
            } else {
                order(&s)
            }
        } else if let Some(index) = path.strip_prefix("/acme/auth/") {
            let i = index.parse::<usize>()?;
            let denied = s.deny_authorization && i < s.auth_ready;
            let authorization = if denied {
                "invalid"
            } else if i < s.auth_ready {
                "valid"
            } else {
                "pending"
            };
            let error = denied.then(|| {
                json!({
                    "type": "urn:ietf:params:acme:error:dns",
                    "detail": "DNS problem: NXDOMAIN looking up TXT for _acme-challenge.owned.test",
                    "status": 400
                })
            });
            json!({
                "identifier": {"type":"dns","value":s.names[i].trim_start_matches("*.")},
                "wildcard": s.names[i].starts_with("*."),
                "status": authorization,
                "challenges": [{
                    "type": "dns-01", "url": format!("{base}/acme/challenge/{i}"),
                    "token": format!("owned-token-{i}"),
                    "status": if denied { "invalid" } else { authorization },
                    "error": error
                }]
            })
        } else if let Some(index) = path.strip_prefix("/acme/challenge/") {
            let i = index.parse::<usize>()?;
            let jwk = s.keys.as_ref().unwrap();
            let canonical = format!(
                "{{\"crv\":\"P-256\",\"kty\":\"EC\",\"x\":\"{}\",\"y\":\"{}\"}}",
                jwk["x"].as_str().unwrap(),
                jwk["y"].as_str().unwrap()
            );
            let thumb = B64.encode(ring::digest::digest(
                &ring::digest::SHA256,
                canonical.as_bytes(),
            ));
            let digest = B64.encode(ring::digest::digest(
                &ring::digest::SHA256,
                format!("owned-token-{i}.{thumb}").as_bytes(),
            ));
            ensure!(
                s.records
                    .values()
                    .any(|r| r.kind == "TXT" && r.value == digest),
                "DNS-01 proof incorrect"
            );
            if !s.cancel_before_ready {
                s.auth_ready = s.auth_ready.max(i + 1);
            }
            json!({"type":"dns-01","url":format!("{base}{path}"),"status":"valid","token":format!("owned-token-{i}")})
        } else if path == "/acme/finalize" {
            let der = B64.decode(payload["csr"].as_str().context("csr")?)?;
            let mut csr = rcgen::CertificateSigningRequestParams::from_der(
                &rustls::pki_types::CertificateSigningRequestDer::from(der),
            )?;
            ensure!(
                csr.params.distinguished_name.iter().next().is_none(),
                "expected an empty CSR subject"
            );
            let csr_names = csr
                .params
                .subject_alt_names
                .iter()
                .map(|name| match name {
                    rcgen::SanType::DnsName(dns) => Ok(dns.as_str()),
                    _ => anyhow::bail!("unexpected non-DNS identifier in CSR"),
                })
                .collect::<Result<BTreeSet<_>>>()?;
            let order_names = s.names.iter().map(String::as_str).collect::<BTreeSet<_>>();
            ensure!(
                csr_names == order_names,
                "CSR identifiers differ from order"
            );
            if let Some((year, month, day)) = s.certificate_expiry {
                csr.params.not_after = rcgen::date_time_ymd(year, month, day);
            }
            s.chain = csr.signed_by(&state.issuer)?.pem();
            order(&s)
        } else if path == "/acme/certificate" {
            return Ok(Response::builder()
                .header("Replay-Nonce", nonce())
                .header("Content-Type", "application/pem-certificate-chain")
                .body(Body::from(s.chain.clone()))?);
        } else {
            anyhow::bail!("unexpected ACME path {path}")
        }
    } else if path == "/ip" {
        s.calls.push("owned-ip-query".into());
        return Ok(Response::builder().body(Body::from(s.public_ip.clone()))?);
    } else if parts.method == axum::http::Method::PUT {
        ensure!(
            parts
                .headers
                .get("authorization")
                .is_some_and(|h| h.as_bytes().starts_with(b"AWS4-HMAC-SHA256")),
            "missing storage signature"
        );
        s.writes.insert(path.into(), body.to_vec());
        Value::Null
    } else if parts.method == axum::http::Method::GET && path == "/" {
        s.calls.push("ListBuckets".into());
        if s.reject_bucket_list {
            return Ok(Response::builder()
                .status(StatusCode::FORBIDDEN)
                .header("Content-Type", "application/xml")
                .body(Body::from(
                    "<Error><Code>AccessDenied</Code><Message>ListBuckets denied</Message></Error>",
                ))?);
        }
        return Ok(Response::builder().header("content-type","application/xml").body(Body::from("<ListAllMyBucketsResult><Buckets><Bucket><Name>cert-bucket</Name></Bucket></Buckets></ListAllMyBucketsResult>"))?);
    } else {
        anyhow::bail!("unexpected fixture path {path}")
    };
    let mut response = Response::builder()
        .status(status)
        .header("Replay-Nonce", nonce())
        .header("Content-Type", "application/json");
    if let Some(delay) = retry_after {
        response = response.header("Retry-After", delay.to_string());
    }
    if let Some(location) = location {
        response = response.header("Location", location);
    }
    Ok(response.body(Body::from(if value.is_null() {
        Vec::new()
    } else {
        serde_json::to_vec(&value)?
    }))?)
}
fn nonce() -> String {
    B64.encode(uuid::Uuid::new_v4().as_bytes())
}
fn order(s: &Data) -> Value {
    json!({"status":if !s.chain.is_empty(){"valid"}else if s.deny_authorization && s.auth_ready>0{"invalid"}else if s.auth_ready>=s.names.len(){"ready"}else{"pending"},"identifiers":s.names.iter().map(|s|json!({"type":"dns","value":s})).collect::<Vec<_>>(),"authorizations":(0..s.names.len()).map(|i|format!("{}/acme/auth/{i}",s.base)).collect::<Vec<_>>(),"finalize":format!("{}/acme/finalize",s.base),"certificate":if s.chain.is_empty(){Value::Null}else{json!(format!("{}/acme/certificate",s.base))}})
}
