//! Stable workspace collaboration DTOs. Database records and credentials are not assets.
use crate::Connection;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MAX_OBJECT_BYTES: usize = 8 * 1024 * 1024;
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub const CONNECTION_KEYS: &[&str] = &[
    "type",
    "host",
    "port",
    "username",
    "user",
    "database",
    "schema",
    "charset",
    "encoding",
    "sslMode",
    "ssl",
    "timeout",
    "comment",
    "databaseTls",
    "databaseProfile",
    "connectTimeout",
    "socketTimeout",
    "queryRowLimit",
    "useSshTunnel",
    "sshTunnelHost",
    "sshTunnelPort",
    "sshTunnelUsername",
    "sshTunnelUsePassword",
    "sshTunnelUseKey",
];

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Connection,
    #[default]
    Query,
    Snippet,
    Pipeline,
    Model,
    Bi,
    Group,
    Profile,
}
impl Kind {
    pub const ALL: [Self; 8] = [
        Self::Connection,
        Self::Query,
        Self::Snippet,
        Self::Pipeline,
        Self::Model,
        Self::Bi,
        Self::Group,
        Self::Profile,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Connection => "连接",
            Self::Query => "查询",
            Self::Snippet => "代码片段",
            Self::Pipeline => "聚合管道",
            Self::Model => "模型工作区",
            Self::Bi => "BI 工作区",
            Self::Group => "虚拟分组",
            Self::Profile => "个人配置",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Owner,
    Manager,
    #[default]
    Editor,
    Viewer,
}
impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Self::Owner => "所有者",
            Self::Manager => "管理和编辑",
            Self::Editor => "编辑",
            Self::Viewer => "只读",
        }
    }
    pub fn write(self) -> bool {
        self != Self::Viewer
    }
    pub fn manage(self) -> bool {
        matches!(self, Self::Owner | Self::Manager)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub email: String,
    pub name: String,
    pub admin: bool,
    pub mfa: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub role: Role,
    pub revision: i64,
    pub created: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Object {
    pub id: String,
    pub project_id: String,
    pub parent_id: String,
    pub kind: Kind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub body: Value,
    pub bytes: usize,
    pub revision: i64,
    pub deleted: bool,
    pub modified: i64,
    pub modified_by: String,
    #[serde(default)]
    pub mutation_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mutation {
    pub parent_id: String,
    pub kind: Kind,
    pub name: String,
    pub body: Value,
    pub base_revision: i64,
    pub mutation_id: String,
    pub deleted: bool,
}
impl Mutation {
    pub fn from_object(o: &Object) -> Self {
        Self {
            parent_id: o.parent_id.clone(),
            kind: o.kind,
            name: o.name.clone(),
            body: o.body.clone(),
            base_revision: o.revision,
            mutation_id: uuid::Uuid::new_v4().to_string(),
            deleted: o.deleted,
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty()
                && self.name.len() <= 240
                && !self.name.contains(['\0', '\r', '\n']),
            "对象名称无效"
        );
        ensure!(
            serde_json::to_vec(&self.body)?.len() <= MAX_OBJECT_BYTES,
            "对象超过 8 MiB"
        );
        if self.kind == Kind::Connection {
            validate_connection(&self.body)?;
        }
        Ok(())
    }
}
pub fn connection_body(c: &Connection) -> Value {
    let mut value = serde_json::Map::new();
    value.insert("type".into(), c.kind().into());
    for key in CONNECTION_KEYS.iter().filter(|key| **key != "type") {
        if let Some(v) = c.options.get(*key)
            && valid_scalar(v)
        {
            value.insert((*key).into(), v.clone());
        }
    }
    Value::Object(value)
}
pub fn connection_body_in_store(c: &Connection, store: &crate::Store) -> Value {
    let mut body = connection_body(c);
    if c.flag("useSshTunnel")
        && c.text("sshTunnelHost").is_empty()
        && let Some(jump) = store
            .connections
            .iter()
            .find(|jump| jump.id == c.text("sshTunnelHostId"))
    {
        for (target, key) in [
            ("sshTunnelHost", "host"),
            ("sshTunnelUsername", "username"),
            ("sshTunnelPort", "port"),
        ] {
            if let Some(value) = jump.options.get(key)
                && valid_scalar(value)
            {
                body[target] = value.clone();
            }
        }
        body["sshTunnelUsePassword"] = jump
            .options
            .get("usePassword")
            .and_then(Value::as_bool)
            .unwrap_or(true)
            .into();
        body["sshTunnelUseKey"] = jump.flag("useKey").into();
    }
    body
}
fn valid_scalar(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => true,
        Value::String(v) => {
            v.len() <= 1000 && !v.contains(['\0', '\r', '\n']) && !v.contains("://")
        }
        _ => false,
    }
}
pub fn validate_connection(body: &Value) -> Result<()> {
    let map = body
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("连接格式无效"))?;
    ensure!(
        map.iter()
            .all(|(key, v)| CONNECTION_KEYS.contains(&key.as_str()) && valid_scalar(v)),
        "连接包含不能同步的属性"
    );
    Ok(())
}
/// Apply only shared properties. Unknown Java fields, IDs and all local credentials survive.
pub fn apply_connection(local: &mut Connection, remote: &Object) -> Result<()> {
    if remote.kind == Kind::Group {
        local.kind = None;
        local.name = remote.name.clone();
        return Ok(());
    }
    validate_connection(&remote.body)?;
    let kind = remote
        .body
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("云端连接缺少类型"))?;
    ensure!(
        local.kind.is_none() || local.kind() == kind,
        "云端连接类型变化，请新建本地连接"
    );
    local.kind = Some(kind.into());
    local.name = remote.name.clone();
    for key in CONNECTION_KEYS.iter().filter(|k| **k != "type") {
        if let Some(value) = remote.body.get(*key) {
            local.options.insert((*key).into(), value.clone());
        } else {
            local.options.remove(*key);
        }
    }
    Ok(())
}
pub fn text_body(text: &str) -> Value {
    json!({"text":text})
}

/// Only portable appearance fields; local paths, API tokens and unknown Java
/// settings never enter the personal sync document.
pub const APPEARANCE_KEYS: &[&str] = &[
    "scrollbackLines",
    "sshTerminalFontName",
    "sshTerminalFontSize",
    "sshServiceManagementEnabled",
    "sshDefaultFileViewMode",
    "sidebarVisible",
    "rdpOpenMode",
];
pub const PERSONAL_CONNECTION_KEYS: &[&str] = &[
    "favorite",
    "hidden",
    "connectionColor",
    "starred",
    "color",
    "isHidden",
];
pub fn personal_body(
    preferences: &crate::preferences::Preferences,
    bindings: &[(String, Object)],
    store: &crate::Store,
) -> Value {
    let raw = serde_json::to_value(preferences).unwrap_or_default();
    let mut appearance = serde_json::Map::new();
    for key in APPEARANCE_KEYS {
        if let Some(value) = raw.get(*key) {
            appearance.insert((*key).into(), value.clone());
        }
    }
    let mut connections = serde_json::Map::new();
    for (local, object) in bindings {
        if let Some(c) = store.connections.iter().find(|c| &c.id == local) {
            let mut options = serde_json::Map::new();
            for key in PERSONAL_CONNECTION_KEYS {
                if let Some(value) = c.options.get(*key)
                    && valid_scalar(value)
                {
                    options.insert((*key).into(), value.clone());
                }
            }
            connections.insert(object.id.clone(), Value::Object(options));
        }
    }
    json!({"appearance":appearance,"connections":connections})
}
pub fn apply_personal(
    preferences: &crate::preferences::Preferences,
    body: &Value,
) -> Result<crate::preferences::Preferences> {
    let mut next = serde_json::to_value(preferences)?;
    if let Some(appearance) = body["appearance"].as_object() {
        for (key, value) in appearance {
            ensure!(
                APPEARANCE_KEYS.contains(&key.as_str()),
                "个人配置包含非便携字段"
            );
            next[key] = value.clone();
        }
    }
    Ok(serde_json::from_value(next)?)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_and_future_fields_stay_local() -> Result<()> {
        let mut c = Connection::new("MYSQL");
        c.set("password", "keep-secret");
        c.set("future", json!({"token":"private"}));
        c.set("url", "mysql://u:p@host");
        let body = connection_body(&c);
        assert!(!body.to_string().contains("secret"));
        assert!(validate_connection(&json!({"password":"x"})).is_err());
        let o = Object {
            id: "o".into(),
            project_id: "p".into(),
            parent_id: String::new(),
            kind: Kind::Connection,
            name: "renamed".into(),
            body: json!({"type":"MYSQL","host":"new"}),
            bytes: 0,
            revision: 1,
            deleted: false,
            modified: 0,
            modified_by: String::new(),
            mutation_id: String::new(),
        };
        apply_connection(&mut c, &o)?;
        assert_eq!(c.text("password"), "keep-secret");
        assert_eq!(c.text("host"), "new");
        assert!(c.options.contains_key("future"));
        Ok(())
    }
}
