//! One native HiveServer2 session with cancel-safe RPC and bounded fetch pages.
use super::{
    config::Config,
    data::{self, Column},
    transport::Transport,
    wire::Value,
};
use anyhow::{Context, Result, bail, ensure};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tomato_core::workbench::Cell;
pub(super) struct Client {
    transport: Option<Transport>,
    session: Value,
    sequence: i32,
    pub operation: Option<Value>,
    pub cancel: CancellationToken,
}
struct Rpc<'a> {
    transport: &'a mut Option<Transport>,
    complete: bool,
}
impl Drop for Rpc<'_> {
    fn drop(&mut self) {
        if !self.complete {
            self.transport.take();
        }
    }
}
fn status(response: &Value) -> Result<()> {
    let status = response.required(1)?;
    let code = status.required(1)?.integer()?;
    if matches!(code, 0..=2) {
        return Ok(());
    }
    bail!(
        "Hive：{}",
        status
            .field(5)
            .and_then(|v| v.text().ok())
            .unwrap_or("服务器拒绝了请求")
    )
}
impl Client {
    pub async fn connect(config: Config) -> Result<Self> {
        let transport = tokio::time::timeout(Duration::from_secs(15), Transport::connect(&config))
            .await
            .context("Hive 认证超时")??;
        let mut client = Self {
            transport: Some(transport),
            session: Value::object([]),
            sequence: 0,
            operation: None,
            cancel: CancellationToken::new(),
        };
        let configuration = Value::Map(
            11,
            11,
            config
                .configuration
                .iter()
                .map(|(key, value)| (Value::string(key), Value::string(value)))
                .collect(),
        );
        let response = client
            .rpc(
                "OpenSession",
                Value::object([
                    (1, Value::I32(5)), // V6: typed column batches, including binary and NULL.
                    (2, Value::string(&config.username)),
                    // Kerberos authenticates with its ticket, not a saved password.
                    (
                        3,
                        Value::string(if config.kerberos() {
                            ""
                        } else {
                            &config.password
                        }),
                    ),
                    (4, configuration),
                ]),
            )
            .await?;
        client.session = response.into_field(3)?;
        Ok(client)
    }
    pub fn disconnect(&mut self) {
        self.operation = None;
        self.transport.take();
    }
    pub async fn rpc(&mut self, method: &str, request: Value) -> Result<Value> {
        self.sequence = self.sequence.checked_add(1).unwrap_or(1);
        let sequence = self.sequence;
        let mut rpc = Rpc {
            transport: &mut self.transport,
            complete: false,
        };
        let transport = rpc
            .transport
            .as_mut()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotConnected))
            .context("Hive 会话已关闭，请重新连接")?;
        let response = tokio::select! {
            biased;
            _ = self.cancel.cancelled() => bail!("Hive 连接已取消"),
            response = tokio::time::timeout(Duration::from_secs(30), transport.rpc(method, sequence, request)) => response.context("Hive 网络响应超时")??,
        };
        rpc.complete = true;
        status(&response)?;
        Ok(response)
    }
    pub async fn close(&mut self) -> Result<()> {
        if self.transport.is_none() {
            return Ok(());
        }
        if self.operation.is_some() {
            self.close_operation().await?;
        }
        let session = self.session.clone();
        let result = self
            .rpc("CloseSession", Value::object([(1, session)]))
            .await;
        self.disconnect();
        result.map(|_| ())
    }
    pub async fn ping(&mut self) -> Result<()> {
        let session = self.session.clone();
        self.rpc(
            "GetInfo",
            Value::object([(1, session), (2, Value::I32(17))]),
        )
        .await?;
        Ok(())
    }
    pub async fn version(&mut self) -> Result<String> {
        let response = self
            .rpc(
                "GetInfo",
                Value::object([(1, self.session.clone()), (2, Value::I32(18))]),
            )
            .await?;
        let version = response.required(2)?.required(1)?.text()?;
        ensure!(version.len() <= 4096, "Hive 服务器版本字符串过长");
        Ok(version.into())
    }
    pub async fn start(&mut self, sql: &str) -> Result<(Vec<Column>, u64)> {
        ensure!(self.operation.is_none(), "Hive 上一个查询尚未关闭");
        let session = self.session.clone();
        let response = self
            .rpc(
                "ExecuteStatement",
                Value::object([
                    (1, session),
                    (2, Value::string(sql)),
                    (3, Value::Map(11, 11, Vec::new())),
                    (4, Value::Bool(true)),
                    (5, Value::I64(0)),
                ]),
            )
            .await?;
        self.operation = Some(response.into_field(2)?);
        let result = self.wait().await;
        if result.is_err() && self.transport.is_some() {
            let _ = self.close_operation().await;
        }
        result
    }
    async fn wait(&mut self) -> Result<(Vec<Column>, u64)> {
        loop {
            let operation = self.operation.clone().context("Hive 操作句柄丢失")?;
            let response = self
                .rpc(
                    "GetOperationStatus",
                    Value::object([(1, operation.clone()), (2, Value::Bool(false))]),
                )
                .await?;
            match response.required(2)?.integer()? {
                0 | 1 | 7 => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                2 => {
                    let results = response
                        .field(9)
                        .map(Value::boolean)
                        .transpose()?
                        .unwrap_or(operation.required(3)?.boolean()?);
                    let affected = response
                        .field(11)
                        .map(Value::integer)
                        .transpose()?
                        .unwrap_or(0)
                        .max(0) as u64;
                    if !results {
                        return Ok((Vec::new(), affected));
                    }
                    let metadata = self
                        .rpc("GetResultSetMetadata", Value::object([(1, operation)]))
                        .await?;
                    return Ok((data::schema(metadata.into_field(2)?)?, affected));
                }
                3 => bail!("Hive 查询已取消"),
                4 => bail!("Hive 查询已关闭"),
                5 => bail!(
                    "Hive：{}",
                    response
                        .field(5)
                        .and_then(|v| v.text().ok())
                        .unwrap_or("查询执行失败")
                ),
                8 => bail!("Hive 查询超时"),
                _ => bail!("Hive 返回了无效的查询状态"),
            }
        }
    }
    pub async fn rows(&mut self, columns: &[Column]) -> Result<Vec<Vec<Cell>>> {
        let operation = self.operation.clone().context("Hive 操作句柄丢失")?;
        let response = self
            .rpc(
                "FetchResults",
                Value::object([
                    (1, operation),
                    (2, Value::I32(0)),
                    (3, Value::I64(256)),
                    (4, Value::I16(0)),
                ]),
            )
            .await?;
        data::rows(response.into_field(3)?, columns)
    }
    pub async fn close_operation(&mut self) -> Result<()> {
        if let Some(operation) = self.operation.take() {
            self.rpc("CloseOperation", Value::object([(1, operation)]))
                .await?;
        }
        Ok(())
    }
}
