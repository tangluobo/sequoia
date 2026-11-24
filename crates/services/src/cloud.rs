//! Cloud use cases use the legacy username/password credential fields.
pub mod api;
pub mod certificate;
pub mod ddns;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tomato_core::Connection;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    Verify,
    Regions,
    Instances {
        region: String,
    },
    Domains,
    Records {
        domain: String,
    },
    SaveRecord {
        domain: String,
        record: Record,
    },
    DeleteRecord {
        domain: String,
        id: String,
    },
    SetDdns {
        domain: String,
        record: Record,
        enabled: bool,
    },
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Record {
    pub record_id: String,
    pub rr: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub value: String,
    pub ttl: u64,
    pub line: String,
    pub priority: u64,
    pub status: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Output {
    pub rows: Vec<Value>,
    pub message: String,
}
pub async fn execute(c: &Connection, action: Action) -> Result<Output> {
    execute_at(c, action, ddns::path()).await
}
pub async fn execute_at(
    c: &Connection,
    action: Action,
    path: std::path::PathBuf,
) -> Result<Output> {
    if let Action::SetDdns {
        domain,
        record,
        enabled,
    } = action
    {
        let c = c.clone();
        tokio::task::spawn_blocking(move || ddns::toggle(&path, &c, &domain, &record, enabled))
            .await??;
        return Ok(Output {
            rows: Vec::new(),
            message: if enabled {
                "已开启 DDNS"
            } else {
                "已关闭 DDNS"
            }
            .into(),
        });
    }
    let zone = match &action {
        Action::Records { domain } => Some(domain.clone()),
        _ => None,
    };
    let certificate_rows = zone.is_some() || matches!(action, Action::Domains);
    let mut output = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        api::Client::new(c)?.execute(action),
    )
    .await
    .map_err(|_| anyhow::anyhow!("云平台查询超时，请按地域或域名缩小范围"))??;
    if certificate_rows {
        let id = c.id.clone();
        output = tokio::task::spawn_blocking(move || -> Result<_> {
            if zone.is_some() {
                let entries = ddns::load(&path)?;
                for row in &mut output.rows {
                    row["ddns"] = json!(
                        entries
                            .iter()
                            .any(|e| e.connection_id == id && e.record_id == text(row, "recordId"))
                    );
                }
            }
            certificate::renewal::annotate(&path, &id, zone.as_deref(), &mut output.rows);
            Ok(output)
        })
        .await??;
    }
    Ok(output)
}
pub fn certificate_domain(rr: &str, zone: &str) -> String {
    let rr = rr.trim().trim_end_matches('.');
    let zone = zone.trim_end_matches('.');
    if rr.is_empty() || rr == "@" {
        return zone.into();
    }
    if rr.eq_ignore_ascii_case(zone)
        || rr
            .len()
            .checked_sub(zone.len())
            .and_then(|index| rr.split_at_checked(index))
            .is_some_and(|(prefix, suffix)| {
                prefix.ends_with('.') && suffix.eq_ignore_ascii_case(zone)
            })
    {
        rr.into()
    } else {
        format!("{rr}.{zone}")
    }
}
pub fn text(v: &Value, name: &str) -> String {
    match &v[name] {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        x => x.to_string(),
    }
}
pub fn products(kind: &str) -> &'static [(&'static str, &'static str)] {
    if kind == "TENCENT_CLOUD" {
        &[("cvm", "云服务器 CVM"), ("dnspod", "DNSPod 域名解析")]
    } else {
        &[
            ("ecs", "云服务器ECS"),
            ("rds", "云数据库RDS"),
            ("oss", "对象存储OSS"),
            ("vpc", "专有网络VPC"),
            ("slb", "负载均衡SLB"),
            ("redis", "云数据库Redis"),
            ("mq", "消息队列MQ"),
            ("ack", "容器服务ACK"),
            ("domain", "域名管理"),
        ]
    }
}
