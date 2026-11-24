//! OpenAI-compatible adapter; returns text only, with no database access.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tomato_core::ai_sql::{self as policy, settings::Settings};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub settings: Settings,
    pub dialect: String,
    pub database: String,
    pub schema: String,
    pub structure: String,
    pub prompt: String,
    pub selected: String,
}
impl Request {
    pub fn retained_bytes(&self) -> usize {
        [
            &self.settings.base,
            &self.settings.model,
            &self.settings.key,
            &self.dialect,
            &self.database,
            &self.schema,
            &self.structure,
            &self.prompt,
            &self.selected,
        ]
        .iter()
        .fold(std::mem::size_of::<Self>(), |bytes, text| {
            bytes.saturating_add(text.capacity())
        })
    }
}
pub async fn generate(request: Request) -> Result<String> {
    ensure!(
        request.settings.configured(),
        "请先配置 AI SQL 的接口地址和模型"
    );
    ensure!(
        request.retained_bytes() <= policy::MAX_TEXT,
        "AI SQL 请求超过 8 MiB"
    );
    let timeout = Duration::from_secs(request.settings.timeout.max(1));
    let client = reqwest::Client::builder()
        .connect_timeout(timeout.min(Duration::from_secs(30)))
        .timeout(timeout)
        .pool_max_idle_per_host(0)
        .build()?;
    let body = serde_json::json!({"model":request.settings.model,"stream":false,"messages":[
        {"role":"system","content":policy::system(Some(&request.dialect),Some(&request.database),Some(&request.schema),Some(&request.structure))},
        {"role":"user","content":policy::user(Some(&request.prompt),Some(&request.selected))}
    ]});
    let mut encoded = LimitedBody(Vec::new());
    serde_json::to_writer(&mut encoded, &body)
        .map_err(|_| anyhow::anyhow!("AI SQL JSON 请求超过 8 MiB"))?;
    drop(body);
    let mut outgoing = client
        .post(policy::endpoint(Some(&request.settings.base)))
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(encoded.0);
    if !request.settings.key.is_empty() {
        outgoing = outgoing.bearer_auth(&request.settings.key);
    }
    drop(request);
    // Do not propagate reqwest's URL-bearing error chain: endpoints can contain credentials.
    let mut response = outgoing
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("AI SQL 接口连接失败或请求超时"))?;
    ensure!(
        response
            .content_length()
            .is_none_or(|length| length <= policy::MAX_TEXT as u64),
        "模型响应超过 8 MiB"
    );
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("AI SQL 响应读取失败或请求超时"))?
    {
        ensure!(
            bytes.len().saturating_add(chunk.len()) <= policy::MAX_TEXT,
            "模型响应超过 8 MiB"
        );
        bytes.extend_from_slice(&chunk);
    }
    let text = String::from_utf8(bytes)?;
    ensure!(
        status.is_success(),
        "模型接口返回 HTTP {}：{}",
        status.as_u16(),
        policy::failure(Some(&text))
    );
    let content = policy::content(&text)?;
    let sql = policy::clean(Some(&content));
    ensure!(!sql.is_empty(), "模型没有返回 SQL");
    ensure!(
        policy::looks_sql(sql),
        "模型返回的内容不像 SQL，请调整提示后重试"
    );
    Ok(sql.into())
}
struct LimitedBody(Vec<u8>);
impl std::io::Write for LimitedBody {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > policy::MAX_TEXT {
            return Err(std::io::Error::other("AI SQL JSON request limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn loopback_protocol_cleans_sql_and_rejects_errors_without_a_database() -> Result<()> {
        for (status, body, expected) in [
            (
                200,
                r#"{"choices":[{"message":{"content":"```sql\nSELECT 1;\n```"}}]}"#,
                "SELECT 1;",
            ),
            (
                401,
                r#"{"error":{"message":"fixture rejected"}}"#,
                "模型接口返回 HTTP 401：fixture rejected",
            ),
            (
                200,
                r#"{"choices":[{"message":{"content":"explanation only"}}]}"#,
                "模型返回的内容不像 SQL，请调整提示后重试",
            ),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let address = listener.local_addr()?;
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await?;
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                loop {
                    let length = socket.read(&mut buffer).await?;
                    ensure!(length > 0, "request closed");
                    bytes.extend_from_slice(&buffer[..length]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let header = std::str::from_utf8(&bytes[..end])?;
                        let size = header
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().ok())
                                    .flatten()
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + size {
                            assert!(header.starts_with("POST /v1/chat/completions HTTP/1.1"));
                            assert!(
                                header
                                    .to_ascii_lowercase()
                                    .contains("authorization: bearer fixture-key")
                            );
                            let request: serde_json::Value =
                                serde_json::from_slice(&bytes[end + 4..end + 4 + size])?;
                            assert_eq!(request["model"], "fixture-model");
                            assert_eq!(request["stream"], false);
                            assert_eq!(request["messages"][1]["content"], "需求：人数");
                            break;
                        }
                    }
                    ensure!(bytes.len() < 64 * 1024, "request too large");
                }
                socket.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await?;
                Ok::<_, anyhow::Error>(())
            });
            let result = generate(Request {
                settings: Settings {
                    base: format!("http://{address}/v1/"),
                    model: "fixture-model".into(),
                    key: "fixture-key".into(),
                    include_schema: true,
                    timeout: 5,
                },
                dialect: "MySQL".into(),
                database: "fixture".into(),
                schema: String::new(),
                structure: String::new(),
                prompt: "人数".into(),
                selected: String::new(),
            })
            .await;
            server.await??;
            assert_eq!(result.unwrap_or_else(|error| error.to_string()), expected);
        }
        Ok(())
    }
}
