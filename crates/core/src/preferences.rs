//! Java-compatible global.json, preserving settings unknown to this build.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::BTreeMap, path::PathBuf};
mod table;
pub use table::TableFont;
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    pub scrollback_lines: usize,
    pub ssh_terminal_font_name: String,
    pub ssh_terminal_font_size: f32,
    pub ssh_service_management_enabled: bool,
    pub ssh_default_file_view_mode: String,
    pub sidebar_visible: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            scrollback_lines: 1000,
            ssh_terminal_font_name: "monospace".into(),
            ssh_terminal_font_size: 13.,
            ssh_service_management_enabled: true,
            ssh_default_file_view_mode: "LIST".into(),
            sidebar_visible: true,
            extra: BTreeMap::new(),
        }
    }
}
impl Preferences {
    fn protected_bytes(&self) -> Result<Vec<u8>> {
        let mut copy = self.clone();
        if let Some(serde_json::Value::String(key)) = copy.extra.get_mut("aiApiKey")
            && !key.is_empty()
            && !key.starts_with("TOMATO_ENCRYPTED:")
        {
            *key = format!(
                "TOMATO_ENCRYPTED:{}",
                crate::legacy::encrypt_text(key.as_bytes())
            );
        }
        Ok(serde_json::to_vec_pretty(&copy)?)
    }
}
pub struct PreferencesRepository {
    path: PathBuf,
    revision: RefCell<Option<Option<Vec<u8>>>>,
}
impl PreferencesRepository {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            revision: RefCell::new(None),
        }
    }
    fn read(&self) -> Result<Option<Vec<u8>>> {
        match std::fs::metadata(&self.path) {
            Ok(meta) => ensure!(meta.len() <= 1024 * 1024, "全局配置超过 1 MiB"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        Ok(Some(std::fs::read(&self.path)?))
    }
    pub fn load(&self) -> Result<Preferences> {
        let bytes = self.read()?;
        let preferences = match &bytes {
            Some(bytes) => {
                serde_json::from_slice(bytes).context("无法解析原版 global.json，原文件未修改")?
            }
            None => Preferences::default(),
        };
        *self.revision.borrow_mut() = Some(bytes);
        Ok(preferences)
    }
    pub fn save(&self, preferences: &Preferences) -> Result<()> {
        let original = self.read()?;
        ensure!(
            self.revision.borrow().as_ref() == Some(&original),
            "全局配置未加载或已被其他程序修改，未覆盖原文件"
        );
        ensure!(
            (6.0..=72.).contains(&preferences.ssh_terminal_font_size)
                && preferences.scrollback_lines <= i32::MAX as usize,
            "终端字号或回滚行数超出范围"
        );
        ensure!(
            ["ICON", "LIST", "COLUMN"].contains(&preferences.ssh_default_file_view_mode.as_str()),
            "未知文件视图模式"
        );
        let bytes = preferences.protected_bytes()?;
        if let Some(data) = &original {
            let mut backup = self.path.as_os_str().to_os_string();
            backup.push(".before-rust.bak");
            let backup = PathBuf::from(backup);
            if !backup.exists() {
                let previous: Preferences = serde_json::from_slice(data)?;
                let mut temporary = tempfile::NamedTempFile::new_in(
                    self.path.parent().context("全局配置缺少目录")?,
                )?;
                use std::io::Write;
                temporary.write_all(&previous.protected_bytes()?)?;
                temporary.as_file().sync_all()?;
                temporary.persist_noclobber(backup).map_err(|e| e.error)?;
            }
        }
        ensure!(
            self.read()? == original,
            "全局配置已被其他程序修改，保存已取消"
        );
        crate::config::atomic_write(&self.path, &bytes)?;
        *self.revision.borrow_mut() = Some(Some(bytes));
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_adb_path_is_preserved_as_an_uninterpreted_setting() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("global.json");
        let old = serde_json::json!({"adbExecutablePath":"C:/old/adb.exe","future":{"keep":42}});
        std::fs::write(&path, serde_json::to_vec(&old)?)?;
        let repository = PreferencesRepository::new(path.clone());
        let mut p = repository.load()?;
        p.sidebar_visible = false;
        repository.save(&p)?;
        let saved = repository.load()?;
        assert_eq!(saved.extra["adbExecutablePath"], old["adbExecutablePath"]);
        assert_eq!(saved.extra["future"], old["future"]);
        let backup: serde_json::Value = serde_json::from_slice(&std::fs::read(
            dir.path().join("global.json.before-rust.bak"),
        )?)?;
        assert_eq!(backup["adbExecutablePath"], old["adbExecutablePath"]);
        assert_eq!(backup["future"], old["future"]);
        assert_eq!(backup["sidebarVisible"], true);
        Ok(())
    }
    #[test]
    fn preserves_unknown_fields_encrypts_api_key_and_rejects_external_changes() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("global.json");
        std::fs::write(
            &path,
            r#"{"sshTerminalFontSize":15,"future":{"x":42},"aiApiKey":"fixture-secret"}"#,
        )?;
        let repo = PreferencesRepository::new(path.clone());
        let mut p = repo.load()?;
        assert_eq!(p.ssh_terminal_font_size, 15.);
        p.scrollback_lines = 200;
        repo.save(&p)?;
        let text = std::fs::read_to_string(&path)?;
        assert!(!text.contains("fixture-secret"));
        assert!(text.contains("TOMATO_ENCRYPTED:"));
        assert_eq!(repo.load()?.extra["future"]["x"], 42);
        std::fs::write(&path, b"{}")?;
        assert!(repo.save(&p).is_err());
        assert_eq!(std::fs::read(&path)?, b"{}");
        Ok(())
    }
}
