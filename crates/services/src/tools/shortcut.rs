use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Fields correspond to DesktopShortcutPane, without touching connection settings.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Shortcut {
    pub name: String,
    pub executable: String,
    pub icon: String,
    pub wm_class: String,
    pub generic_name: String,
    pub comment: String,
    pub arguments: String,
    pub working_directory: String,
    pub category: String,
    pub custom_categories: String,
    pub terminal: bool,
    pub notify: bool,
}

pub const CATEGORIES: &[(&str, &str)] = &[
    ("网络应用", "Network"),
    ("办公", "Office"),
    ("开发", "Development"),
    ("游戏", "Game"),
    ("图形", "Graphics"),
    ("多媒体", "AudioVideo"),
    ("系统工具", "System"),
    ("实用工具", "Utility"),
    ("科学", "Science"),
    ("教育", "Education"),
    ("社会", "Social"),
    ("无障碍", "Accessibility"),
    ("收藏", "Favorites"),
];

// Match the Java preview's quoting, including its Launch action.
fn escaped(value: &str) -> String {
    if value.contains([' ', '\t', '\\', '"']) {
        format!(
            "\"{}\"",
            value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\t', "\\t")
        )
    } else {
        value.to_owned()
    }
}

impl Shortcut {
    pub fn file_name(&self) -> String {
        let mut result = String::new();
        for ch in self.name.trim().to_lowercase().chars() {
            if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
                result.push(ch);
            } else if !result.ends_with('-') {
                result.push('-');
            }
        }
        format!("{}.desktop", result.trim_matches('-'))
    }
    pub fn preview(&self) -> Result<String> {
        let values = [
            &self.name,
            &self.executable,
            &self.icon,
            &self.wm_class,
            &self.generic_name,
            &self.comment,
            &self.arguments,
            &self.working_directory,
            &self.category,
            &self.custom_categories,
        ];
        ensure!(
            values.iter().all(|v| !v.contains(['\n', '\r', '\0'])),
            "快捷方式字段不能包含换行或空字符"
        );
        let mut result = String::from("[Desktop Entry]\nType=Application\n");
        for (key, value) in [
            ("Name", &self.name),
            ("GenericName", &self.generic_name),
            ("Comment", &self.comment),
        ] {
            if !value.trim().is_empty() {
                result.push_str(&format!("{key}={}\n", escaped(value.trim())));
            }
        }
        if !self.executable.trim().is_empty() {
            result.push_str(&format!("Exec={}", escaped(self.executable.trim())));
            if !self.arguments.trim().is_empty() {
                result.push_str(&format!(" {}", escaped(self.arguments.trim())));
            }
            result.push('\n');
        }
        for (key, value) in [("Path", &self.working_directory), ("Icon", &self.icon)] {
            if !value.trim().is_empty() {
                result.push_str(&format!(
                    "{key}={}\n",
                    if key == "Icon" {
                        value.trim().to_owned()
                    } else {
                        escaped(value.trim())
                    }
                ));
            }
        }
        result.push_str(&format!("Terminal={}\n", self.terminal));
        let category = self.category.trim();
        let custom = self.custom_categories.trim().trim_end_matches(';');
        if !category.is_empty() || !custom.is_empty() {
            result.push_str("Categories=");
            for value in [category, custom] {
                if !value.is_empty() {
                    result.push_str(value);
                    result.push(';');
                }
            }
            result.push('\n');
        }
        if self.notify {
            result.push_str("StartupNotify=true\n");
        }
        if !self.wm_class.trim().is_empty() {
            result.push_str(&format!(
                "StartupWMClass={}\n",
                escaped(self.wm_class.trim())
            ));
        }
        if !self.name.trim().is_empty() {
            result.push_str(&format!(
                "Actions=Launch;\n\n[Desktop Action Launch]\nName={}\nExec={}\n",
                escaped(&self.name.trim().replace('"', "")),
                escaped(self.executable.trim())
            ));
        }
        Ok(result)
    }
    pub fn save(&self, destination: &Path) -> Result<()> {
        ensure!(
            [&self.name, &self.executable, &self.icon, &self.wm_class]
                .iter()
                .all(|v| !v.trim().is_empty()),
            "请填写应用名称、执行路径、图标文件和窗口类名"
        );
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        tomato_core::config::atomic_write(destination, self.preview()?.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o755))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_fields_saved_and_injection_rejected() -> Result<()> {
        let item = Shortcut {
            name: "My App".into(),
            executable: "/opt/my app/run".into(),
            icon: "app-icon".into(),
            wm_class: "App".into(),
            arguments: "--new-window".into(),
            category: "Development".into(),
            custom_categories: "IDE;Utility;".into(),
            terminal: true,
            notify: true,
            ..Default::default()
        };
        let dir = tempfile::tempdir()?;
        let path = dir.path().join(item.file_name());
        item.save(&path)?;
        let text = std::fs::read_to_string(path)?;
        assert!(text.contains("Exec=\"/opt/my app/run\" --new-window\n"));
        assert!(text.contains(
            "Categories=Development;IDE;Utility;\nStartupNotify=true\nStartupWMClass=App\n"
        ));
        assert!(text.contains("[Desktop Action Launch]"));
        assert!(
            Shortcut {
                comment: "x\nExec=bad".into(),
                ..item
            }
            .preview()
            .is_err()
        );
        Ok(())
    }
}
