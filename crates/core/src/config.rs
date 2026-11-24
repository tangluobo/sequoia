use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;
pub mod export;
mod import;
pub mod retained;
pub use import::{Baseline as ImportBaseline, ImportPlan, Row as ImportRow};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    #[serde(default = "new_id")]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    // Keep every legacy field, including nested server config and future fields.
    #[serde(flatten)]
    pub options: BTreeMap<String, Value>,
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

impl Connection {
    pub fn new(kind: &str) -> Self {
        let mut result = Self {
            id: new_id(),
            name: String::new(),
            parent_id: None,
            kind: Some(kind.into()),
            options: BTreeMap::new(),
        };
        if let Some(info) = crate::catalog::find_kind(kind) {
            result.name = info.label.into();
            result.set("port", info.port);
        }
        result.set("host", "127.0.0.1");
        result
    }
    pub fn kind(&self) -> &str {
        self.kind.as_deref().unwrap_or("FOLDER")
    }
    pub fn text(&self, key: &str) -> &str {
        self.options
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
    }
    pub fn number(&self, key: &str) -> u64 {
        self.options
            .get(key)
            .and_then(Value::as_u64)
            .unwrap_or_default()
    }
    pub fn flag(&self, key: &str) -> bool {
        self.options
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or_default()
    }
    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        self.options.insert(key.into(), value.into());
    }
    pub fn redacted(&self) -> Self {
        let mut result = self.clone();
        for (key, value) in &mut result.options {
            redact(key, value);
        }
        result
    }
}

fn redact(key: &str, value: &mut Value) {
    let name = key.to_ascii_lowercase();
    if ["password", "secret", "token", "accesskey"]
        .iter()
        .any(|word| name.contains(word))
        && !name.contains("savepassword")
        && !name.contains("usepassword")
    {
        *value = Value::Null;
    } else {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    redact(key, value);
                }
            }
            Value::Array(array) => {
                for value in array {
                    redact("", value);
                }
            }
            _ => {}
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default = "schema_version")]
    pub version: u32,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub dark_mode: bool,
}
fn schema_version() -> u32 {
    1
}

impl Store {
    pub fn default_path() -> PathBuf {
        if let Some(path) = std::env::var_os("TOMATO_DATA_DIR") {
            return PathBuf::from(path).join("connections.json");
        }
        directories::UserDirs::new()
            .map(|dirs| dirs.home_dir().join(".tomato/connections.json"))
            .unwrap_or_else(|| PathBuf::from(".tomato/connections.json"))
    }
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                version: 1,
                ..Self::default()
            });
        }
        Self::decode(&std::fs::read(path)?)
    }
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        let content = std::str::from_utf8(bytes).context("连接配置不是 UTF-8")?;
        let text = content.trim_start_matches('\u{feff}').trim();
        let store: Self = if text.is_empty()
            || text.starts_with('[')
            || text.starts_with(crate::legacy::MARKER)
            || text == "null"
        {
            Self {
                version: 1,
                connections: parse_legacy(text)?,
                ..Self::default()
            }
        } else {
            let value: Value = serde_json::from_str(text).context("无法解析配置，原文件未修改")?;
            if !value.get("connections").is_some_and(Value::is_array) {
                bail!("配置对象缺少有效的 connections 数组，原文件未修改");
            }
            serde_json::from_value(value).context("无法解析工作区配置，原文件未修改")?
        };
        if store.version != 1 {
            bail!("不支持的配置版本 {}", store.version);
        }
        validate(&store.connections)?;
        Ok(store)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        validate(&self.connections)?;
        atomic_write(path, crate::legacy::encode(&self.connections)?.as_bytes())
    }
    pub fn import(&mut self, content: &str) -> Result<usize> {
        let plan = ImportPlan::parse(content, self)?;
        let selected = (0..plan.connections.len())
            .filter(|i| !plan.duplicate(*i))
            .collect();
        plan.apply(self, &selected)
    }
    pub fn export(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(
            &self
                .connections
                .iter()
                .map(Connection::redacted)
                .collect::<Vec<_>>(),
        )?)
    }
    pub fn remove_subtree(&mut self, id: &str) {
        let removed = crate::tree::ConnectionTree::new(&self.connections).subtree_ids(id);
        self.connections.retain(|item| !removed.contains(&item.id));
    }
}

fn duplicate_key(connection: &Connection) -> String {
    fn canonical(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.retain(|key, value| {
                    !matches!(
                        key.as_str(),
                        "id" | "parentId" | "password" | "sshTunnelPassword" | "sshTunnelHostId"
                    ) && !value.is_null()
                });
                for value in map.values_mut() {
                    canonical(value);
                }
                map.sort_keys();
            }
            Value::Array(array) => {
                for value in array {
                    canonical(value);
                }
            }
            _ => {}
        }
    }
    let mut value = serde_json::to_value(connection).expect("connection is JSON");
    // Gson initializes absent primitive fields before analyzing duplicates. Older
    // exports can omit them; normalize for comparison without changing persisted options.
    if let Some(map) = value.as_object_mut() {
        for (key, default) in [
            ("port", 0),
            ("screenWidth", 1024),
            ("screenHeight", 768),
            ("colorDepth", 24),
            ("remoteDesktopTlsPort", 0),
            ("redisDatabase", 0),
            ("sshTunnelPort", 22),
        ] {
            if map.get(key).is_none_or(Value::is_null) {
                map.insert(key.into(), default.into());
            }
        }
        for (key, default) in [
            ("usePassword", true),
            ("savePassword", true),
            ("useKey", false),
            ("fullscreen", false),
            ("useSsl", true),
            ("mapClipboard", true),
            ("enableSound", true),
            ("pathStyleAccess", false),
            ("redisCluster", false),
            ("useSshTunnel", false),
            ("sshTunnelUsePassword", true),
            ("sshTunnelSavePassword", true),
            ("sshTunnelUseKey", false),
        ] {
            if map.get(key).is_none_or(Value::is_null) {
                map.insert(key.into(), default.into());
            }
        }
        for key in ["privateKeyPaths", "sshTunnelPrivateKeyPaths"] {
            map.entry(key).or_insert_with(|| Value::Array(Vec::new()));
        }
        map.entry("directoryType")
            .or_insert_with(|| Value::String("LOCAL".into()));
    }
    canonical(&mut value);
    value.to_string()
}

pub fn parse_legacy(content: &str) -> Result<Vec<Connection>> {
    crate::legacy::decode(content)
}

pub fn validate(connections: &[Connection]) -> Result<()> {
    validate_relations(connections, false)
}
fn validate_relations(connections: &[Connection], allow_external: bool) -> Result<()> {
    validate_refs(connections.iter(), allow_external)
}
fn validate_refs<'a>(
    connections: impl IntoIterator<Item = &'a Connection>,
    allow_external: bool,
) -> Result<()> {
    let mut by_id = HashMap::new();
    for connection in connections {
        if connection.id.is_empty() || by_id.insert(connection.id.as_str(), connection).is_some() {
            bail!("连接 ID 为空或重复");
        }
    }
    let mut states = HashMap::<&str, u8>::new();
    for connection in by_id.values() {
        let mut path = Vec::new();
        let mut next = Some(connection.id.as_str());
        while let Some(id) = next.filter(|id| !id.is_empty()) {
            let Some(node) = by_id.get(id) else {
                if allow_external {
                    break;
                }
                bail!("连接引用了不存在的目录");
            };
            match states.get(id).copied().unwrap_or(0) {
                0 => {
                    states.insert(id, 1);
                    path.push(id);
                    next = node.parent_id.as_deref();
                }
                1 => bail!("连接目录存在循环引用: {}", connection.name),
                _ => break,
            }
        }
        for id in path {
            states.insert(id, 2);
        }
    }
    Ok(())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::{BlockEncryptMut, KeyInit, block_padding::Pkcs7};
    use base64::{Engine, engine::general_purpose::STANDARD};

    #[test]
    fn imports_java_aes_and_preserves_unknown_options() -> Result<()> {
        let json = r#"[{"id":"a","name":"SSH","type":"SSH","password":"secret","screenWidth":1920,"serverConfig":{"future":42}}]"#;
        let encrypted = ecb::Encryptor::<aes::Aes128>::new(b"tomato_connect_k".into())
            .encrypt_padded_vec_mut::<Pkcs7>(json.as_bytes());
        let data = format!("TOMATO_ENCRYPTED{}", STANDARD.encode(encrypted));
        let mut store = Store::default();
        assert_eq!(store.import(&data)?, 1);
        assert_eq!(store.connections[0].text("password"), "secret");
        assert_eq!(store.connections[0].number("screenWidth"), 1920);
        assert!(store.export()?.contains("future"));
        assert!(!store.export()?.contains("secret"));
        assert_eq!(store.import(&data)?, 0);
        Ok(())
    }

    #[test]
    fn import_remaps_folders_and_tunnels() -> Result<()> {
        let mut store = Store::default();
        store.import(r#"[{"id":"folder","name":"组"},{"id":"ssh","name":"主机","type":"SSH","parentId":"folder"},{"id":"db","name":"数据库","type":"MYSQL","parentId":"folder","sshTunnelHostId":"ssh"}]"#)?;
        assert_eq!(
            store.connections[2].parent_id.as_deref(),
            Some(store.connections[0].id.as_str())
        );
        assert_eq!(
            store.connections[2].text("sshTunnelHostId"),
            store.connections[1].id
        );
        let folder = store.connections[0].id.clone();
        store.remove_subtree(&folder);
        assert!(store.connections.is_empty());
        Ok(())
    }

    #[test]
    fn corrupt_import_does_not_replace_current_data() -> Result<()> {
        let mut store = Store::default();
        store.connections.push(Connection::new("SSH"));
        assert!(
            store
                .import(r#"[{"id":"a","parentId":"b"},{"id":"b","parentId":"a"}]"#)
                .is_err()
        );
        assert_eq!(store.connections.len(), 1);
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("workspace.json");
        store.connections[0].set("password", "never-write-this");
        store.save(&path)?;
        assert!(!std::fs::read_to_string(&path)?.contains("never-write-this"));
        assert_eq!(Store::load(&path)?.connections.len(), 1);
        Ok(())
    }
}
