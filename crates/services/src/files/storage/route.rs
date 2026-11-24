//! Keep the signed authority and TLS server name while routing TCP through SSH.
mod proxy;
use super::*;
use crate::terminal::ssh::tunnel::Tunnel;
use opendal::{Buffer, HttpBody, HttpTransport, HttpTransporter};
use std::{net::SocketAddr, sync::Arc};

pub(super) struct Route {
    _tunnel: Tunnel,
    pub client: reqwest::Client,
    host: String,
    _proxy: proxy::Proxy,
    remote_port: u16,
    scheme: String,
}
impl Route {
    fn target(connection: &Connection) -> Result<(Connection, url::Url)> {
        let url = url::Url::parse(&endpoint(connection, connection.kind() == "ALIYUN_OSS"))?;
        let mut target = connection.clone();
        target.set(
            "host",
            url.host_str()
                .context("存储端点缺少主机")?
                .trim_matches(['[', ']']),
        );
        target.set(
            "port",
            url.port_or_known_default().context("存储端点协议无效")?,
        );
        Ok((target, url))
    }
    async fn open(
        connection: &Connection,
        jump: Option<(&Connection, PathBuf)>,
    ) -> Result<Arc<Self>> {
        let (target, url) = Self::target(connection)?;
        let tunnel = if let Some((jump, known)) = jump {
            Tunnel::open_via(&target, jump, known).await?
        } else {
            Tunnel::open(&target).await?
        };
        let proxy = proxy::Proxy::start(
            tunnel.address,
            url.host_str().context("存储端点缺少主机")?.into(),
            url.port_or_known_default().context("存储端点协议无效")?,
        )
        .await?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .proxy(reqwest::Proxy::all(format!("socks5h://{}", proxy.address))?)
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(4)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Arc::new(Self {
            host: url.host_str().context("存储端点缺少主机")?.into(),
            _proxy: proxy,
            remote_port: url.port_or_known_default().context("存储端点协议无效")?,
            scheme: url.scheme().into(),
            client,
            _tunnel: tunnel,
        }))
    }
    pub fn checked_url(&self, uri: &str) -> Result<url::Url> {
        let url = url::Url::parse(uri)?;
        let host = url.host_str().context("存储请求缺少主机")?;
        ensure!(
            (host == self.host || host.ends_with(&format!(".{}", self.host)))
                && url.port_or_known_default() == Some(self.remote_port)
                && url.scheme() == self.scheme,
            "存储请求超出当前 SSH 隧道端点"
        );
        // The SOCKS bridge changes only TCP routing; Host, URL, and TLS SNI stay original.
        Ok(url)
    }
}
struct Transport(Arc<Route>);
fn error(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> opendal::Error {
    opendal::Error::new(opendal::ErrorKind::Unexpected, "SSH 存储请求失败")
        .set_source(anyhow::Error::from_boxed(error.into()))
}
impl HttpTransport for Transport {
    async fn fetch(
        &self,
        request: http::Request<Buffer>,
    ) -> opendal::Result<http::Response<HttpBody>> {
        let (mut parts, body) = request.into_parts();
        let url = self
            .0
            .checked_url(&parts.uri.to_string())
            .map_err(|e| error(e.into_boxed_dyn_error()))?;
        if !parts.headers.contains_key(http::header::HOST) {
            let host = parts
                .uri
                .authority()
                .ok_or_else(|| error(std::io::Error::other("缺少存储主机")))?;
            parts.headers.insert(
                http::header::HOST,
                http::HeaderValue::from_str(host.as_str()).map_err(error)?,
            );
        }
        let request = self
            .0
            .client
            .request(parts.method.clone(), url)
            .headers(parts.headers);
        let request = if body.is_empty() {
            request
        } else {
            request.body(reqwest::Body::wrap_stream(body))
        };
        let response = request.send().await.map_err(error)?;
        let length = if parts.method == http::Method::HEAD {
            None
        } else {
            response.content_length()
        };
        let mut output = http::Response::builder()
            .status(response.status())
            .version(response.version());
        *output.headers_mut().expect("response builder") = response.headers().clone();
        let stream = futures_util::stream::try_unfold(response, |mut response| async move {
            match response.chunk().await.map_err(error)? {
                Some(bytes) => Ok(Some((Buffer::from(bytes), response))),
                None => Ok(None),
            }
        });
        output
            .body(HttpBody::new(Box::pin(stream), length))
            .map_err(error)
    }
}
impl Storage {
    pub async fn connect(connection: &Connection) -> Result<Self> {
        Self::connect_inner(connection, None).await
    }
    /// Explicit jump credentials/known-host path for isolated clients and fixtures.
    pub async fn connect_via(
        connection: &Connection,
        jump: &Connection,
        known: PathBuf,
    ) -> Result<Self> {
        Self::connect_inner(connection, Some((jump, known))).await
    }
    async fn connect_inner(
        connection: &Connection,
        jump: Option<(&Connection, PathBuf)>,
    ) -> Result<Self> {
        let mut direct = connection.clone();
        direct.set("useSshTunnel", false);
        let mut storage = Self::new(&direct)?;
        if connection.flag("useSshTunnel") || jump.is_some() {
            ensure!(
                super::super::object_copy::is_object(connection),
                "此存储类型不支持 SSH 隧道"
            );
            storage.route = Some(Route::open(connection, jump).await?);
            storage.apply_route();
        }
        storage.connection = connection.clone();
        storage.session_credentials = super::super::object_copy::sessions::Lease::new(connection);
        Ok(storage)
    }
    pub(super) fn apply_route(&mut self) {
        if let Some(route) = &self.route {
            let transport = HttpTransporter::new(Transport(route.clone()));
            self.operator = self
                .operator
                .clone()
                .with_context(self.operator.base_context().with_http_transport(transport));
        }
    }
}
