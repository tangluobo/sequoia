//! Native bounded Kafka and RocketMQ adapters. Each command owns its sockets/tunnels.
mod batch;
mod kafka;
mod rocket;
mod transport;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tomato_core::Connection;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    Topics,
    Stats {
        topic: String,
    },
    Route {
        topic: String,
    },
    CreateTopic {
        topic: String,
        partitions: i32,
        replicas: i16,
    },
    DeleteTopic {
        topic: String,
    },
    Groups,
    Group {
        group: String,
    },
    DeleteGroup {
        group: String,
    },
    DeleteGroups {
        groups: Vec<String>,
    },
    Cluster,
    Broker {
        address: String,
    },
    Messages {
        topic: String,
        query: Query,
    },
    ConsumeStatus {
        topic: String,
    },
    Send {
        topic: String,
        partition: Option<i32>,
        key: String,
        body: String,
        headers: BTreeMap<String, String>,
        tags: String,
        delay: i32,
    },
    Track {
        topic: String,
        id: String,
    },
    Reconsume {
        topic: String,
        group: String,
        id: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Query {
    Offset {
        partition: i32,
        offset: i64,
        limit: usize,
    },
    Time {
        begin: i64,
        end: i64,
    },
    Key(String),
    Id(String),
    Unconsumed {
        group: String,
        limit: usize,
    },
}
#[derive(Clone, Debug, Serialize)]
pub struct Output {
    pub rows: Vec<Value>,
    pub detail: Value,
    pub message: String,
}
impl Output {
    fn rows(rows: Vec<Value>) -> Self {
        Self {
            rows,
            detail: Value::Null,
            message: String::new(),
        }
    }
    fn detail(detail: Value) -> Self {
        Self {
            rows: Vec::new(),
            detail,
            message: String::new(),
        }
    }
    fn changed(message: &str) -> Self {
        Self {
            rows: Vec::new(),
            detail: Value::Null,
            message: message.into(),
        }
    }
}
pub async fn execute(c: &Connection, action: Action) -> Result<Output> {
    let serialized = serde_json::to_vec(&action)?;
    ensure!(
        serialized.len() <= 2 * 1024 * 1024,
        "消息操作载荷超过 2 MiB"
    );
    let result = tokio::time::timeout(std::time::Duration::from_secs(90), async {
        execute_single(c, action).await
    })
    .await??;
    ensure!(
        result.rows.len() <= 10000 && serde_json::to_vec(&result)?.len() <= 16 * 1024 * 1024,
        "消息结果超过内存上限，请缩小查询范围"
    );
    Ok(result)
}
async fn execute_single(c: &Connection, action: Action) -> Result<Output> {
    match c.kind() {
        "KAFKA" => kafka::execute(c, action).await,
        "ROCKETMQ" => rocket::execute(c, action).await,
        _ => anyhow::bail!("不是消息队列连接"),
    }
}
fn text(v: &Value, name: &str) -> String {
    crate::cloud::text(v, name)
}

fn append_rows(new: impl IntoIterator<Item = Value>, rows: &mut Vec<Value>) -> Result<()> {
    fn size(v: &Value) -> usize {
        match v {
            Value::String(s) => s.len().saturating_add(32),
            Value::Array(a) => a.iter().fold(32usize, |n, v| n.saturating_add(size(v))),
            Value::Object(m) => m.iter().fold(32usize, |n, (k, v)| {
                n.saturating_add(k.len())
                    .saturating_add(size(v))
                    .saturating_add(32)
            }),
            _ => 32,
        }
    }
    let mut bytes = rows.iter().fold(0usize, |n, v| n.saturating_add(size(v)));
    for row in new {
        bytes = bytes.saturating_add(size(&row));
        ensure!(
            bytes <= 16 * 1024 * 1024 && rows.len() < 10000,
            "消息结果超过 16 MiB，请缩小查询范围"
        );
        rows.push(row);
    }
    Ok(())
}
