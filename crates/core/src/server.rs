//! Legacy serverConfig objects live inside the shared encrypted connection file.
use crate::Connection;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ServerType {
    #[default]
    Http,
    Ftp,
    Smb,
}
impl ServerType {
    pub fn port(self) -> u16 {
        match self {
            Self::Http => 8080,
            Self::Ftp => 2121,
            Self::Smb => 445,
        }
    }
    pub fn scheme(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Ftp => "ftp",
            Self::Smb => "smb",
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ServerConfig {
    #[serde(rename = "type")]
    pub kind: ServerType,
    pub port: u16,
    pub bind_address: String,
    pub anonymous_access: bool,
    pub root_directory: String,
    pub shared_directories: Vec<SharedDirectory>,
    pub accounts: Vec<ServerAccount>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            kind: ServerType::Http,
            port: 8080,
            bind_address: "0.0.0.0".into(),
            anonymous_access: false,
            root_directory: String::new(),
            shared_directories: Vec::new(),
            accounts: Vec::new(),
            extra: Default::default(),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SharedDirectory {
    pub alias: String,
    pub path: String,
    pub read_only: bool,
    pub allowed_users: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ServerAccount {
    pub username: String,
    pub password: String,
    pub enabled: bool,
    pub home_directory: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
impl Default for ServerAccount {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            enabled: true,
            home_directory: String::new(),
            extra: Default::default(),
        }
    }
}
impl fmt::Debug for ServerAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerAccount")
            .field("username", &self.username)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}
impl fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerConfig")
            .field("kind", &self.kind)
            .field("port", &self.port)
            .field("accounts", &self.accounts.len())
            .field("shares", &self.shared_directories.len())
            .finish_non_exhaustive()
    }
}
impl ServerConfig {
    pub fn from_connection(c: &Connection) -> Result<Self> {
        if let Some(value) = c.options.get("serverConfig").filter(|v| !v.is_null()) {
            let mut value = value.clone();
            if let Some(object) = value.as_object_mut() {
                for key in [
                    "type",
                    "port",
                    "bindAddress",
                    "rootDirectory",
                    "accounts",
                    "sharedDirectories",
                ] {
                    if object.get(key).is_some_and(Value::is_null) {
                        object.remove(key);
                    }
                }
                object.entry("type").or_insert_with(|| {
                    Value::String(
                        match c.kind() {
                            "FTP_SERVER" => "FTP",
                            "SMB_SERVER" => "SMB",
                            _ => "HTTP",
                        }
                        .into(),
                    )
                });
                for (group, keys) in [
                    ("accounts", &["username", "password", "homeDirectory"][..]),
                    ("sharedDirectories", &["alias", "path", "allowedUsers"][..]),
                ] {
                    if let Some(entries) = object.get_mut(group).and_then(Value::as_array_mut) {
                        for entry in entries {
                            if let Some(fields) = entry.as_object_mut() {
                                for key in keys {
                                    if fields.get(*key).is_some_and(Value::is_null) {
                                        fields.remove(*key);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            return Ok(serde_json::from_value(value)?);
        }
        let kind = match c.kind() {
            "FTP_SERVER" => ServerType::Ftp,
            "SMB_SERVER" => ServerType::Smb,
            _ => ServerType::Http,
        };
        Ok(Self {
            kind,
            port: if c.number("port") == 0 {
                kind.port()
            } else {
                u16::try_from(c.number("port"))?
            },
            root_directory: c.text("localDirectoryPath").into(),
            ..Default::default()
        })
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.bind_address.trim().is_empty(), "监听地址不能为空");
        ensure!(
            self.shared_directories.len() <= 1000 && self.accounts.len() <= 1000,
            "共享目录或账号超过 1000 项"
        );
        let mut aliases = HashSet::new();
        for share in &self.shared_directories {
            ensure!(
                !share.alias.is_empty()
                    && share.alias != "."
                    && share.alias != ".."
                    && !share.alias.contains(['/', '\\', '\0']),
                "共享别名无效"
            );
            ensure!(aliases.insert(&share.alias), "共享别名重复");
            ensure!(!share.path.is_empty(), "共享目录不能为空");
        }
        let mut users = HashSet::new();
        for account in &self.accounts {
            ensure!(!account.username.is_empty(), "账号名不能为空");
            ensure!(users.insert(&account.username), "账号名重复");
        }
        Ok(())
    }
    pub fn authenticate(&self, user: &str, password: &str) -> bool {
        self.anonymous_access && user.eq_ignore_ascii_case("anonymous")
            || self
                .accounts
                .iter()
                .any(|a| a.enabled && a.username == user && a.password == password)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_server_config_keeps_permissions_credentials_and_unknown_fields() -> Result<()> {
        let mut c = Connection::new("FTP_SERVER");
        c.set("serverConfig",serde_json::json!({"type":"FTP","port":2121,"bindAddress":"127.0.0.1","rootDirectory":null,
            "accounts":[{"username":"u","password":"fixture-only","homeDirectory":null,"futureFlag":7}],
            "sharedDirectories":[{"alias":"a","path":"fixture","readOnly":true,"allowedUsers":["u"]}],"futureSetting":{"enabled":true}}));
        let config = ServerConfig::from_connection(&c)?;
        assert_eq!(config.kind, ServerType::Ftp);
        assert!(config.accounts[0].enabled);
        assert!(config.shared_directories[0].read_only);
        assert!(config.authenticate("u", "fixture-only"));
        assert!(!config.authenticate("u", "wrong"));
        assert_eq!(config.accounts[0].home_directory, "");
        let output = serde_json::to_value(&config)?;
        assert_eq!(output["futureSetting"]["enabled"], true);
        assert_eq!(output["accounts"][0]["futureFlag"], 7);
        assert!(!format!("{config:?} {:?}", config.accounts[0]).contains("fixture-only"));
        let mut disabled = config;
        disabled.accounts[0].enabled = false;
        assert!(!disabled.authenticate("u", "fixture-only"));
        assert!(disabled.validate().is_ok());
        Ok(())
    }
}
