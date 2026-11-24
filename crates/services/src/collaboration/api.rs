use super::*;
use anyhow::{Context, Result, ensure};
use futures_util::StreamExt;
use reqwest::Method;
use serde::{Serialize, de::DeserializeOwned};
use std::{fmt, time::Duration};

#[derive(Debug)]
pub struct HttpError {
    pub status: u16,
    pub code: String,
    pub message: String,
}
impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for HttpError {}
#[derive(Clone)]
pub struct Api {
    base: String,
    client: reqwest::Client,
    token: Option<String>,
}
impl Api {
    pub fn new(base: &str) -> Result<Self> {
        crate::init_crypto();
        let url = url::Url::parse(base.trim()).context("云服务地址无效")?;
        ensure!(
            url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "服务地址不能包含账号、密码或查询参数"
        );
        let loopback = url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        ensure!(
            url.scheme() == "https" || url.scheme() == "http" && loopback,
            "云服务需要 HTTPS；本机回环测试允许 HTTP"
        );
        Ok(Self {
            base: url.as_str().trim_end_matches('/').into(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(35))
                .connect_timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            token: None,
        })
    }
    pub fn authenticated(mut self, token: String) -> Self {
        self.token = Some(token);
        self
    }
    pub fn base(&self) -> &str {
        &self.base
    }
    pub async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<T> {
        ensure!(
            path.starts_with('/') && !path.contains("..") && !path.contains('#'),
            "无效云服务路径"
        );
        let mut request = self
            .client
            .request(method, format!("{}/v1{path}", self.base));
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            let bytes = serde_json::to_vec(body)?;
            ensure!(bytes.len() <= MAX_OBJECT_BYTES + 16384, "请求超过 8 MiB");
            request = request
                .header("content-type", "application/json")
                .body(bytes);
        }
        let response = request.send().await.context("无法连接 Tomato Cloud")?;
        let status = response.status();
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(part) = stream.next().await {
            let part = part?;
            ensure!(
                bytes.len() + part.len() <= MAX_OBJECT_BYTES + 65536,
                "云服务回复超过大小限制"
            );
            bytes.extend_from_slice(&part);
        }
        if !status.is_success() {
            let error: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
            return Err(HttpError {
                status: status.as_u16(),
                code: error["error"].as_str().unwrap_or("http").into(),
                message: error["message"].as_str().unwrap_or("云服务请求失败").into(),
            }
            .into());
        }
        serde_json::from_slice(&bytes).context("云服务回复格式无效")
    }
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.request(Method::GET, path, None::<&()>).await
    }
    pub async fn login(&self, email: String, password: String, code: String) -> Result<Login> {
        self.request(Method::POST,"/login",Some(&serde_json::json!({"email":email,"password":password,"code":code,"device":format!("Tomato {} {}",env!("CARGO_PKG_VERSION"),std::env::consts::OS)}))).await
    }
    pub async fn projects(&self) -> Result<Vec<Project>> {
        let mut result = Vec::new();
        let mut after = String::new();
        loop {
            let page: Vec<Project> = self
                .get(&format!("/projects?limit=200&after={after}"))
                .await?;
            let done = page.len() < 200;
            if let Some(last) = page.last() {
                after = last.id.clone();
            }
            result.extend(page);
            ensure!(result.len() <= 5000, "云端项目过多");
            if done {
                return Ok(result);
            }
        }
    }
    pub async fn object(&self, project: &str, id: &str) -> Result<Object> {
        self.get(&format!("/projects/{project}/objects/{id}")).await
    }
    pub async fn put(&self, project: &str, id: &str, mutation: &Mutation) -> Result<Object> {
        self.request(
            Method::PUT,
            &format!("/projects/{project}/objects/{id}"),
            Some(mutation),
        )
        .await
    }
    pub async fn events(&self, cursor: i64) -> Result<Events> {
        self.get(&format!("/events?cursor={cursor}&wait=25")).await
    }
}
