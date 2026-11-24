//! Original `.tomata/hosts_groups.json` repository and managed Hosts blocks.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const LIMIT: u64 = 8 * 1024 * 1024;
mod elevation;
pub use elevation::{Authorization, Password};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Entry {
    pub ip: String,
    pub domain: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Group {
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub entries: Vec<Entry>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub config: Config,
    pub system: String,
    pub config_stamp: Option<String>,
    pub system_stamp: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SaveRequest {
    pub previous: Box<Snapshot>,
    pub config: Config,
    pub system: Option<String>,
    pub apply: bool,
}
#[derive(Debug)]
pub struct ElevationRequired(pub Box<SaveRequest>);
impl std::fmt::Display for ElevationRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("写入系统 hosts 文件需要管理员权限")
    }
}
impl std::error::Error for ElevationRequired {}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    Load,
    Save {
        previous: Box<Snapshot>,
        config: Config,
        system: Option<String>,
        apply: bool,
    },
    SaveElevated {
        request: Box<SaveRequest>,
        #[serde(skip)]
        authorization: Option<Authorization>,
    },
}
pub fn legacy_path() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".tomata/hosts_groups.json")
}
pub fn system_path() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
            .join("System32/drivers/etc/hosts")
    } else {
        PathBuf::from("/etc/hosts")
    }
}
fn read(path: &Path) -> Result<Option<String>> {
    match fs::File::open(path) {
        Ok(file) => {
            let mut text = String::new();
            file.take(LIMIT + 1).read_to_string(&mut text)?;
            ensure!(text.len() as u64 <= LIMIT, "Hosts 文件超过 8 MiB");
            Ok(Some(text))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn stamp(text: Option<&str>) -> Option<String> {
    text.map(|s| {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .encode(ring::digest::digest(&ring::digest::SHA256, s.as_bytes()))
    })
}
pub fn parse_entries(text: &str) -> Vec<Entry> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let i = l.find(char::is_whitespace)?;
            Some(Entry {
                ip: l[..i].into(),
                domain: l[i..].trim().into(),
                ..Default::default()
            })
        })
        .collect()
}
pub fn group_text(group: &Group) -> String {
    let mut text = format!("# {}\n", group.name);
    for entry in &group.entries {
        text.push_str(&format!("{} {}\n", entry.ip, entry.domain));
    }
    text.push_str(&format!("# End {}", group.name));
    text
}
/// Match whole marker lines, so `dev` cannot consume a `development` block.
pub fn compose(system: &str, previous: &Config, next: &Config) -> String {
    let names: std::collections::BTreeSet<_> = previous
        .groups
        .iter()
        .chain(&next.groups)
        .map(|g| g.name.as_str())
        .collect();
    let lines: Vec<_> = system.split_inclusive('\n').collect();
    let mut base = String::new();
    let mut i = 0;
    while i < lines.len() {
        let marker = lines[i].trim_end_matches(['\r', '\n']);
        let matched = names.iter().find_map(|name| {
            if marker != format!("# {name}") {
                return None;
            }
            let end = format!("# End {name}");
            (i + 1..lines.len()).find(|&j| lines[j].trim_end_matches(['\r', '\n']) == end)
        });
        if let Some(end) = matched {
            i = end + 1;
        } else {
            base.push_str(lines[i]);
            i += 1;
        }
    }
    let mut result = base.trim().to_owned();
    for group in &next.groups {
        if group.enabled && !group.entries.is_empty() {
            result.push('\n');
            result.push_str(&group_text(group));
            result.push('\n');
        }
    }
    result
}
fn defaults() -> Config {
    Config {
        groups: vec![
            Group {
                name: "开发环境".into(),
                entries: parse_entries("127.0.0.1 localhost\n127.0.0.1 dev.example.com"),
                ..Default::default()
            },
            Group {
                name: "测试环境".into(),
                entries: parse_entries(
                    "192.168.1.100 test.example.com\n192.168.1.101 test-api.example.com",
                ),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}
fn load(config_path: &Path, system_path: &Path) -> Result<Snapshot> {
    let raw = read(config_path)?;
    let system = read(system_path)?;
    let config = raw
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .context("Hosts 分组配置无效，保留原文件")?
        .unwrap_or_else(defaults);
    Ok(Snapshot {
        config,
        config_stamp: stamp(raw.as_deref()),
        system_stamp: stamp(system.as_deref()),
        system: system.unwrap_or_default(),
    })
}
fn write_system(path: &Path, text: &str) -> Result<()> {
    // Keep the existing inode/ACL; replacing this protected system file loses permissions.
    ensure!(text.len() as u64 <= LIMIT, "Hosts 文件超过 8 MiB");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .context("写入 Hosts 失败，可能需要管理员权限")?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    Ok(())
}
pub fn execute(config_path: &Path, system_path: &Path, action: Action) -> Result<Snapshot> {
    let (request, authorization) = match action {
        Action::Load => return load(config_path, system_path),
        Action::Save {
            previous,
            config,
            system,
            apply,
        } => (
            SaveRequest {
                previous,
                config,
                system,
                apply,
            },
            None,
        ),
        Action::SaveElevated {
            request,
            authorization,
        } => {
            ensure!(cfg!(unix), "此平台不使用 sudo 写入 Hosts");
            let authorization = authorization.context("sudo 授权已失效，请重新输入密码")?;
            authorization.control.check()?;
            (*request, Some(authorization))
        }
    };
    save(config_path, system_path, request, authorization.as_ref())
}
fn save(
    config_path: &Path,
    system_path: &Path,
    request: SaveRequest,
    authorization: Option<&Authorization>,
) -> Result<Snapshot> {
    save_using(
        config_path,
        system_path,
        request,
        cfg!(unix) && authorization.is_none(),
        |text| match authorization {
            Some(authorization) => elevation::write(system_path, text, authorization),
            None => write_system(system_path, text),
        },
        |text| match authorization {
            Some(authorization) => elevation::restore(system_path, text, authorization),
            None => write_system(system_path, text),
        },
    )
}
fn save_using(
    config_path: &Path,
    system_path: &Path,
    request: SaveRequest,
    challenge_on_denied: bool,
    mut write: impl FnMut(&str) -> Result<()>,
    mut restore: impl FnMut(&str) -> Result<()>,
) -> Result<Snapshot> {
    let SaveRequest {
        previous,
        config,
        system,
        apply,
    } = &request;
    let current = load(config_path, system_path)?;
    ensure!(
        current.config_stamp == previous.config_stamp,
        "Hosts 分组已被其他程序修改，请重新加载；草稿已保留"
    );
    for group in &config.groups {
        ensure!(
            !group.name.trim().is_empty() && !group.name.contains(['\n', '\r']),
            "分组名称不能为空或含换行"
        );
    }
    // The original parser searches literal `"groups":[`, `"entries":[` and
    // `"name":"` tokens; whitespace after ':' makes its saved groups disappear.
    let data = serde_json::to_vec(config)?;
    ensure!(data.len() as u64 <= LIMIT, "Hosts 分组配置超过 8 MiB");
    let updated = system
        .clone()
        .or_else(|| apply.then(|| compose(&current.system, &previous.config, config)));
    if updated.is_some() {
        ensure!(
            current.system_stamp == previous.system_stamp,
            "系统 Hosts 已被其他程序修改，请重新加载；草稿已保留"
        );
    }
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let backup = config_path.with_extension("json.bak");
    if config_path.exists() && !backup.exists() {
        fs::copy(config_path, &backup)?;
    }
    if let Some(text) = &updated
        && let Err(error) = write(text)
    {
        if read(system_path).ok().flatten().as_deref() != Some(&current.system) {
            restore(&current.system)
                .context(format!("Hosts 写入失败且回滚失败，原错误：{error:#}"))?;
        }
        if challenge_on_denied
            && error.chain().any(|e| {
                e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied)
            })
        {
            return Err(ElevationRequired(Box::new(request)).into());
        }
        return Err(error);
    }
    let commit = || -> Result<()> {
        ensure!(
            stamp(read(config_path)?.as_deref()) == previous.config_stamp,
            "Hosts 分组已被其他程序修改，请重新加载；草稿已保留"
        );
        tomato_core::config::atomic_write(config_path, &data)
    };
    if let Err(error) = commit() {
        if updated.is_some() {
            restore(&current.system).context("分组保存失败，系统 Hosts 回滚也失败")?;
        }
        return Err(error);
    }
    load(config_path, system_path)
}

impl Action {
    pub fn queued_bytes(&self) -> usize {
        match self {
            Self::Load => 0,
            Self::SaveElevated {
                request,
                authorization,
            } => {
                request.queued_bytes()
                    + authorization
                        .as_ref()
                        .map_or(0, Authorization::queued_bytes)
            }
            Self::Save {
                previous,
                config,
                system,
                ..
            } => save_bytes(previous, config, system),
        }
    }
}
impl SaveRequest {
    pub fn queued_bytes(&self) -> usize {
        save_bytes(&self.previous, &self.config, &self.system)
    }
}
fn save_bytes(previous: &Snapshot, config: &Config, system: &Option<String>) -> usize {
    fn size(v: &serde_json::Value) -> usize {
        match v {
            serde_json::Value::String(s) => s.capacity(),
            serde_json::Value::Array(a) => a.iter().map(size).sum(),
            serde_json::Value::Object(o) => o.iter().map(|(k, v)| k.capacity() + size(v)).sum(),
            _ => 16,
        }
    }
    fn extras(e: &BTreeMap<String, serde_json::Value>) -> usize {
        e.iter().map(|(k, v)| k.capacity() + size(v)).sum()
    }
    fn config_size(c: &Config) -> usize {
        c.groups
            .iter()
            .map(|g| {
                g.name.capacity()
                    + extras(&g.extra)
                    + g.entries
                        .iter()
                        .map(|e| e.ip.capacity() + e.domain.capacity() + extras(&e.extra))
                        .sum::<usize>()
            })
            .sum::<usize>()
            + extras(&c.extra)
    }
    previous.system.capacity()
        + config_size(&previous.config)
        + config_size(config)
        + system.as_ref().map_or(0, String::capacity)
}

#[cfg(test)]
mod tests;
