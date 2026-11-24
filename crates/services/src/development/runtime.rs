//! Filesystem and process validation stay outside the UI frame.
use super::*;
use std::path::Path;
use tomato_core::development::runtime::{self, EntryRole};
pub(super) fn validate(
    role: EntryRole,
    kind: &str,
    name: &str,
    path: &Path,
    settings: &Path,
) -> Option<String> {
    let valid = !name.trim().is_empty()
        && if role.is_build() {
            path.is_dir() && (kind != "MAVEN" || settings.is_file())
        } else if kind == "NODE" {
            path.is_file()
        } else {
            path.is_dir()
        };
    (!valid).then(|| {
        if role.is_build() {
            format!(
                "请填写名称并选择有效的构建工具路径{}",
                if kind == "MAVEN" {
                    "和 settings.xml"
                } else {
                    ""
                }
            )
        } else {
            format!(
                "请填写名称并选择有效的 {} 路径",
                runtime::display_kind(kind)
            )
        }
    })
}
pub(super) async fn detect(kind: &str, path: &Path) -> Result<String> {
    ensure!(
        ["JDK", "NODE", "FLUTTER", "MAVEN", "GRADLE"].contains(&kind),
        "未知 SDK 类型"
    );
    if kind == "JDK"
        && let Ok(bytes) = workspace::read_bytes(&path.join("release"), 64 * 1024)
        && let Ok(text) = std::str::from_utf8(&bytes)
        && let Some(version) = runtime::release_version(text)
    {
        return Ok(version);
    }
    let Ok(plan) = build::version_command(kind, path) else {
        return Ok(String::new());
    };
    let Ok(output) = process::capture(plan, std::time::Duration::from_secs(5)).await else {
        return Ok(String::new());
    };
    Ok(runtime::command_version(kind, &output))
}
