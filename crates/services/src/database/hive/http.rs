//! One-origin HS2 HTTP transport; only an explicit 401 permits replaying a request.
use super::{config::Config, transport::Socket};
use anyhow::{Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bytes::Bytes;
use http::{HeaderMap, HeaderValue, StatusCode};
use http_body_util::{BodyExt as _, Full};
use std::time::{Duration, SystemTime};

const MAX_BODY: usize = 8 * 1024 * 1024;
enum Authentication {
    Basic(HeaderValue),
    Kerberos(String),
}
impl Authentication {
    async fn header(&self) -> Result<HeaderValue> {
        let mut value = match self {
            Self::Basic(value) => value.clone(),
            Self::Kerberos(principal) => HeaderValue::from_str(&format!(
                "Negotiate {}",
                STANDARD.encode(super::sasl::http_ticket(principal.clone()).await?)
            ))?,
        };
        value.set_sensitive(true);
        Ok(value)
    }
}
struct Cookie {
    value: String,
    expires: Option<SystemTime>,
}
impl Cookie {
    fn name(&self) -> &str {
        self.value.split('=').next().unwrap_or_default()
    }
}
struct Driver(tokio::task::JoinHandle<()>);
impl Drop for Driver {
    fn drop(&mut self) {
        self.0.abort();
    }
}
pub(super) struct Transport {
    sender: hyper::client::conn::http1::SendRequest<Full<Bytes>>,
    driver: Driver,
    path: String,
    host: String,
    ssl: bool,
    auth: Authentication,
    cookies: Vec<Cookie>,
    cookie_names: Vec<String>,
    cookies_enabled: bool,
}
impl Transport {
    pub async fn connect(socket: Socket, config: &Config) -> Result<Self> {
        let auth = if config.kerberos() {
            Authentication::Kerberos(super::sasl::principal(config)?)
        } else {
            Authentication::Basic(HeaderValue::from_str(&format!(
                "Basic {}",
                STANDARD.encode(format!("{}:{}", config.username, config.password))
            ))?)
        };
        let cookie_names = if config.option("cookiename").is_empty() {
            "hive.server2.auth"
        } else {
            config.option("cookiename")
        }
        .split(',')
        .map(|name| name.trim().to_owned())
        .collect::<Vec<_>>();
        ensure!(
            cookie_names.len() <= 32
                && cookie_names
                    .iter()
                    .all(|name| !name.is_empty() && name.len() <= 256),
            "Hive HTTP Cookie 名称无效"
        );
        let path = config.http_path()?;
        let host = if config.host.contains(':') {
            format!("[{}]:{}", config.host, config.port)
        } else {
            format!("{}:{}", config.host, config.port)
        };
        let (sender, connection) =
            hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(socket)).await?;
        Ok(Self {
            sender,
            driver: Driver(tokio::spawn(async move {
                let _ = connection.await;
            })),
            path,
            host,
            ssl: config.ssl(),
            auth,
            cookies: Vec::new(),
            cookie_names,
            cookies_enabled: !config.option("cookieauth").eq_ignore_ascii_case("false"),
        })
    }
    fn auth_cookie(&mut self) -> bool {
        let now = SystemTime::now();
        self.cookies
            .retain(|c| c.expires.is_none_or(|end| end > now));
        self.cookies
            .iter()
            .any(|cookie| self.cookie_names.iter().any(|name| name == cookie.name()))
    }
    fn remember(&mut self, headers: &HeaderMap) -> Result<()> {
        if !self.cookies_enabled {
            return Ok(());
        }
        for value in headers.get_all("Set-Cookie") {
            let value = value.to_str()?;
            ensure!(value.len() <= 8192, "Hive HTTP Cookie 超过8192字节");
            let mut parts = value.split(';');
            let pair = parts.next().unwrap_or_default().trim();
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let mut expires = None;
            let mut max_age = None;
            let mut skip = false;
            for part in parts.map(str::trim) {
                let (key, value) = part.split_once('=').unwrap_or((part, ""));
                if key.eq_ignore_ascii_case("secure") && !self.ssl {
                    skip = true;
                } else if key.eq_ignore_ascii_case("path") {
                    let path = self.path.split('?').next().unwrap_or(&self.path);
                    skip |= !path.starts_with(value)
                        || (!value.ends_with('/')
                            && path.len() > value.len()
                            && path.as_bytes().get(value.len()) != Some(&b'/'));
                } else if key.eq_ignore_ascii_case("max-age") {
                    max_age = value.parse::<i64>().ok();
                } else if key.eq_ignore_ascii_case("expires") {
                    expires = chrono::DateTime::parse_from_rfc2822(value)
                        .ok()
                        .map(SystemTime::from);
                }
            }
            if skip {
                continue;
            }
            if let Some(seconds) = max_age {
                expires = Some(if seconds <= 0 {
                    SystemTime::UNIX_EPOCH
                } else {
                    SystemTime::now()
                        .checked_add(Duration::from_secs(seconds as u64))
                        .unwrap_or(SystemTime::UNIX_EPOCH)
                });
            }
            self.cookies.retain(|old| old.name() != name);
            if value.is_empty() || expires.is_some_and(|end| end <= SystemTime::now()) {
                continue;
            }
            ensure!(
                self.cookies.len() < 32
                    && self.cookies.iter().map(|c| c.value.len()).sum::<usize>() + pair.len()
                        <= 64 * 1024,
                "Hive HTTP Cookie 总量超过限制"
            );
            self.cookies.push(Cookie {
                value: pair.into(),
                expires,
            });
        }
        Ok(())
    }
    pub async fn request(&mut self, bytes: Vec<u8>) -> Result<Vec<u8>> {
        ensure!(!self.driver.0.is_finished(), "Hive HTTP 连接已关闭");
        let bytes = Bytes::from(bytes);
        for attempt in 0..2 {
            let has_cookie = self.auth_cookie();
            let mut request = http::Request::builder()
                .method("POST")
                .uri(&self.path)
                .header("Host", &self.host)
                .header("Content-Type", "application/x-thrift")
                .header("Accept", "application/x-thrift")
                .header("User-Agent", "Tomato");
            // A fresh AP-REQ is essential: reusing a service token fails replay
            // protection on Hive servers with cookie authentication disabled.
            if !has_cookie || matches!(self.auth, Authentication::Basic(_)) {
                request = request.header("Authorization", self.auth.header().await?);
            }
            if !self.cookies.is_empty() {
                let mut value = HeaderValue::from_str(
                    &self
                        .cookies
                        .iter()
                        .map(|c| c.value.as_str())
                        .collect::<Vec<_>>()
                        .join("; "),
                )?;
                value.set_sensitive(true);
                request = request.header("Cookie", value);
            }
            let mut response = self
                .sender
                .send_request(request.body(Full::new(bytes.clone()))?)
                .await?;
            let status = response.status();
            if status.is_success() {
                self.remember(response.headers())?;
            }
            let mut body = Vec::new();
            while let Some(frame) = response.body_mut().frame().await {
                if let Ok(data) = frame?.into_data() {
                    ensure!(
                        body.len() + data.len() <= MAX_BODY,
                        "Hive HTTP 响应超过8 MiB"
                    );
                    body.extend_from_slice(&data);
                }
            }
            if status == StatusCode::UNAUTHORIZED && has_cookie && attempt == 0 {
                // Authentication is rejected before Thrift execution. Never
                // replay a network failure or an ambiguous successful write.
                self.cookies
                    .retain(|cookie| !self.cookie_names.iter().any(|name| name == cookie.name()));
                continue;
            }
            ensure!(status.is_success(), "Hive HTTP 请求失败：{status}");
            return Ok(body);
        }
        unreachable!("bounded HTTP authentication retry")
    }
}
