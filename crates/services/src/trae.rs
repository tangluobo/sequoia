//! Legacy Trae session repository; filesystem moves are journaled and reversible until commit.
mod history;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};
const LIMIT: usize = 8 * 1024 * 1024;
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    pub phone: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub active: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(rename = "traeUserDir", default = "default_user")]
    pub user_dir: PathBuf,
    #[serde(default)]
    pub sessions: Vec<Session>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            user_dir: default_user(),
            sessions: Vec::new(),
            extra: Default::default(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub config: Config,
    pub stamp: Option<String>,
    pub user_exists: bool,
    pub exists: Vec<bool>,
    pub warning: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Mutation {
    Archive { phone: String, name: String },
    Activate { phone: String },
    Close { phone: String },
    Rename { phone: String, name: String },
    Delete { phone: String, delete_data: bool },
    Reorder { phones: Vec<String> },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    Load,
    Mutate {
        previous: Box<Snapshot>,
        mutation: Mutation,
    },
    Tasks {
        config: Config,
        phone: String,
        generation: u64,
    },
}
#[derive(Debug, Serialize, Deserialize)]
pub enum Output {
    Snapshot(Snapshot),
    Tasks {
        phone: String,
        generation: u64,
        tasks: Vec<String>,
        status: String,
        truncated: bool,
    },
}
fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}
fn default_user() -> PathBuf {
    home().join(if cfg!(windows) {
        "AppData/Roaming/Trae CN/User"
    } else {
        ".config/Trae CN/User"
    })
}
pub fn legacy_path() -> PathBuf {
    home().join(".tomata/trae_sessions.json")
}
fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.take((LIMIT + 1) as u64).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= LIMIT, "Trae 配置超过 8 MiB");
    Ok(Some(bytes))
}
fn digest(bytes: Option<&[u8]>) -> Option<String> {
    use base64::Engine;
    bytes.map(|b| {
        base64::engine::general_purpose::STANDARD
            .encode(ring::digest::digest(&ring::digest::SHA256, b))
    })
}
fn component(value: &str) -> Result<()> {
    ensure!(
        !value.trim().is_empty()
            && !value.contains(['/', '\\', ':', '\0'])
            && value != "."
            && value != ".."
            && !value.ends_with([' ', '.']),
        "手机号或工作区标识不能含路径分隔符"
    );
    Ok(())
}
fn validate(config: &Config) -> Result<()> {
    ensure!(
        config.user_dir.is_absolute()
            && config.user_dir.parent().is_some()
            && config.user_dir.file_name().is_some(),
        "无效的 User 目录路径"
    );
    ensure!(config.sessions.len() <= 10000, "Trae 会话超过 10000 项");
    let mut phones = std::collections::HashSet::new();
    for session in &config.sessions {
        component(&session.phone)?;
        ensure!(phones.insert(&session.phone), "Trae 会话手机号重复");
    }
    Ok(())
}
fn archive(config: &Config, phone: &str) -> Result<PathBuf> {
    component(phone)?;
    Ok(config
        .user_dir
        .parent()
        .context("User 目录没有父目录")?
        .join(format!("User{phone}")))
}
pub fn directory(config: &Config, phone: &str) -> Result<PathBuf> {
    let session = config
        .sessions
        .iter()
        .find(|s| s.phone == phone)
        .context("找不到 Trae 会话")?;
    if session.active {
        Ok(config.user_dir.clone())
    } else {
        archive(config, phone)
    }
}
fn snapshot(mut config: Config, stamp: Option<String>) -> Result<Snapshot> {
    validate(&config)?;
    let mut active = false;
    for index in 0..config.sessions.len() {
        if config.sessions[index].active {
            let backup = archive(&config, &config.sessions[index].phone)?;
            if active || backup.exists() {
                config.sessions[index].active = false;
            } else {
                active = true;
            }
        }
    }
    let exists = config
        .sessions
        .iter()
        .map(|s| directory(&config, &s.phone).is_ok_and(|p| p.is_dir()))
        .collect();
    Ok(Snapshot {
        user_exists: config.user_dir.is_dir(),
        config,
        stamp,
        exists,
        warning: String::new(),
    })
}
pub fn load(path: &Path) -> Result<Snapshot> {
    let bytes = read(path)?;
    let config = bytes
        .as_deref()
        .map(serde_json::from_slice)
        .transpose()?
        .unwrap_or_default();
    snapshot(config, digest(bytes.as_deref()))
}
struct Moves {
    entries: Vec<(PathBuf, PathBuf)>,
    committed: bool,
}
impl Moves {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            committed: false,
        }
    }
    fn rename(&mut self, from: &Path, to: &Path) -> Result<()> {
        ensure!(!to.try_exists()?, "目标目录已存在：{}", to.display());
        ensure!(
            fs::symlink_metadata(from)?.is_dir() && !is_link(from)?,
            "会话目录不能是链接或重解析点"
        );
        // Resolve both parents in this shell-independent adapter before moving a directory.
        ensure!(
            from.parent().map(fs::canonicalize).transpose()?
                == to.parent().map(fs::canonicalize).transpose()?,
            "会话目录必须在同一父目录内移动"
        );
        fs::rename(from, to).context("无法重命名 User 目录，请确认 Trae CN 已完全关闭")?;
        self.entries.push((from.to_owned(), to.to_owned()));
        Ok(())
    }
    fn rollback(&mut self) -> Result<()> {
        while let Some((from, to)) = self.entries.last() {
            ensure!(!from.try_exists()?, "回滚目标已存在：{}", from.display());
            fs::rename(to, from).context("Trae 目录回滚失败")?;
            self.entries.pop();
        }
        Ok(())
    }
}
impl Drop for Moves {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.rollback();
        }
    }
}
fn is_link(path: &Path) -> Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        Ok(metadata.file_attributes() & 0x400 != 0)
    }
    #[cfg(not(windows))]
    {
        Ok(metadata.file_type().is_symlink())
    }
}
fn vacant(parent: &Path, base: &str) -> Result<PathBuf> {
    for index in 1..=100000 {
        let p = parent.join(format!("{base}_{index}"));
        if !p.try_exists()? {
            return Ok(p);
        }
    }
    anyhow::bail!("没有可用的 Trae 归档目录")
}
fn mutate(path: &Path, previous: &Snapshot, mutation: Mutation) -> Result<Snapshot> {
    let current = load(path)?;
    ensure!(
        current.stamp == previous.stamp,
        "Trae 会话配置已被其他程序修改，请重新加载；草稿已保留"
    );
    let mut config = current.config;
    let mut moves = Moves::new();
    let mut trash = None;
    let result = (|| -> Result<()> {
        match mutation {
            Mutation::Archive { phone, name } => {
                component(&phone)?;
                ensure!(!name.trim().is_empty(), "名称不能为空");
                let target = archive(&config, &phone)?;
                moves.rename(&config.user_dir, &target)?;
                for s in &mut config.sessions {
                    s.active = false;
                }
                if let Some(s) = config.sessions.iter_mut().find(|s| s.phone == phone) {
                    s.name = name.trim().into();
                } else {
                    config.sessions.push(Session {
                        phone,
                        name: name.trim().into(),
                        ..Default::default()
                    });
                }
            }
            Mutation::Activate { phone } => {
                let target = directory(&config, &phone)?;
                if !config.sessions.iter().any(|s| s.phone == phone && s.active) {
                    ensure!(target.is_dir(), "目标会话目录不存在");
                    let parent = config.user_dir.parent().context("User 目录没有父目录")?;
                    if config.user_dir.try_exists()? {
                        let current = config.sessions.iter().find(|s| s.active);
                        let archive = if let Some(s) = current {
                            let p = archive(&config, &s.phone)?;
                            if p.try_exists()? {
                                vacant(parent, &format!("User_archive_{}", s.phone))?
                            } else {
                                p
                            }
                        } else {
                            vacant(parent, "User_unknown_archive")?
                        };
                        moves.rename(&config.user_dir, &archive)?;
                    }
                    moves.rename(&target, &config.user_dir)?;
                    for s in &mut config.sessions {
                        s.active = s.phone == phone;
                    }
                }
            }
            Mutation::Close { phone } => {
                ensure!(
                    config.sessions.iter().any(|s| s.phone == phone && s.active),
                    "会话未激活"
                );
                let target = archive(&config, &phone)?;
                moves.rename(&config.user_dir, &target)?;
                for s in &mut config.sessions {
                    s.active = false;
                }
            }
            Mutation::Rename { phone, name } => {
                ensure!(!name.trim().is_empty(), "名称不能为空");
                config
                    .sessions
                    .iter_mut()
                    .find(|s| s.phone == phone)
                    .context("找不到 Trae 会话")?
                    .name = name.trim().into();
            }
            Mutation::Delete { phone, delete_data } => {
                let s = config
                    .sessions
                    .iter()
                    .find(|s| s.phone == phone)
                    .context("找不到 Trae 会话")?;
                if !s.active && delete_data {
                    let dir = directory(&config, &phone)?;
                    if dir.try_exists()? {
                        let staged = dir.with_file_name(format!(
                            ".tomato-trae-delete-{}",
                            uuid::Uuid::new_v4()
                        ));
                        moves.rename(&dir, &staged)?;
                        trash = Some(staged);
                    }
                }
                config.sessions.retain(|s| s.phone != phone);
            }
            Mutation::Reorder { phones } => {
                ensure!(phones.len() == config.sessions.len(), "会话排序不完整");
                let mut ordered = Vec::with_capacity(phones.len());
                for phone in phones {
                    let i = config
                        .sessions
                        .iter()
                        .position(|s| s.phone == phone)
                        .context("会话排序重复或已失效")?;
                    ordered.push(config.sessions.remove(i));
                }
                config.sessions = ordered;
            }
        }
        validate(&config)?;
        // Keep forward-slash spelling and the exact legacy field names.
        let mut json = serde_json::to_value(&config)?;
        json["traeUserDir"] = config.user_dir.to_string_lossy().replace('\\', "/").into();
        let bytes = serde_json::to_vec(&json)?;
        ensure!(bytes.len() <= LIMIT, "Trae 会话配置超过 8 MiB");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let backup = path.with_extension("json.bak");
        if path.exists() && !backup.exists() {
            fs::copy(path, &backup)?;
        }
        ensure!(
            digest(read(path)?.as_deref()) == previous.stamp,
            "Trae 配置在操作期间被修改"
        );
        tomato_core::config::atomic_write(path, &bytes)?;
        Ok(())
    })();
    if let Err(error) = result {
        moves.rollback().context(format!("原操作失败：{error:#}"))?;
        return Err(error);
    }
    moves.committed = true;
    let mut result = load(path)?;
    if let Some(trash) = trash
        && let Err(error) = fs::remove_dir_all(&trash)
    {
        result.warning = format!("会话已移除，归档清理失败：{}：{error}", trash.display());
    }
    Ok(result)
}
pub async fn execute(path: PathBuf, action: Action) -> Result<Output> {
    match action {
        Action::Tasks {
            config,
            phone,
            generation,
        } => history::tasks(config, phone, generation).await,
        action => {
            tokio::task::spawn_blocking(move || match action {
                Action::Load => load(&path).map(Output::Snapshot),
                Action::Mutate { previous, mutation } => {
                    mutate(&path, &previous, mutation).map(Output::Snapshot)
                }
                Action::Tasks { .. } => unreachable!(),
            })
            .await?
        }
    }
}
impl Action {
    pub fn queued_bytes(&self) -> usize {
        // Serializing here is only done once per command, never per frame.
        serde_json::to_vec(self)
            .map(|v| v.len())
            .unwrap_or(LIMIT + 1)
    }
}
