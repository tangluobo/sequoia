//! Java GlobalConfig's tableFontName/tableFontSize without discarding future fields.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableFont {
    pub name: String,
    pub size: u8,
}
impl Default for TableFont {
    fn default() -> Self {
        Self {
            name: "Sans Serif".into(),
            size: 10,
        }
    }
}
impl Preferences {
    pub fn table_font(&self) -> TableFont {
        let default = TableFont::default();
        let name = self
            .extra
            .get("tableFontName")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&default.name)
            .to_owned();
        let size = self
            .extra
            .get("tableFontSize")
            .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
            .filter(|v| v.is_finite())
            .unwrap_or(f64::from(default.size))
            .round()
            .clamp(8., 24.) as u8;
        TableFont { name, size }
    }
    pub fn set_table_font(&mut self, font: &TableFont) -> Result<()> {
        ensure!(
            !font.name.trim().is_empty() && font.name.len() <= 1024 && !font.name.contains('\0'),
            "表格字体名称无效"
        );
        ensure!((8..=24).contains(&font.size), "表格字号必须在 8–24 之间");
        self.extra
            .insert("tableFontName".into(), font.name.clone().into());
        self.extra.insert("tableFontSize".into(), font.size.into());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_table_font_uses_original_global_file_and_preserves_other_settings() -> Result<()> {
        let root = tempfile::tempdir()?;
        let file = root.path().join("global.json");
        std::fs::write(&file, br#"{"tableFontName":"Arial","tableFontSize":18,"future":{"keep":1},"sshTerminalFontSize":15}"#)?;
        let repo = PreferencesRepository::new(file.clone());
        let mut settings = repo.load()?;
        assert_eq!(
            settings.table_font(),
            TableFont {
                name: "Arial".into(),
                size: 18
            }
        );
        settings.set_table_font(&TableFont {
            name: "Microsoft YaHei".into(),
            size: 12,
        })?;
        repo.save(&settings)?;
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&file)?)?;
        assert_eq!(saved["tableFontSize"], 12);
        assert_eq!(saved["future"]["keep"], 1);
        assert_eq!(saved["sshTerminalFontSize"].as_f64(), Some(15.));
        assert!(root.path().join("global.json.before-rust.bak").is_file());
        std::fs::write(&file, b"{\"tableFontSize\":20}")?;
        assert!(repo.save(&settings).is_err());
        Ok(())
    }
}
