//! Original nb3 metadata and legacy backup-directory layout.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scope {
    pub connection: String,
    pub database: String,
    pub path: String,
}
fn sanitize(value: &str) -> String {
    let clean = crate::queries::sanitize(Some(value));
    if crate::java_text::is_blank(&clean) || matches!(clean.as_str(), "." | "..") {
        "unnamed".into()
    } else {
        clean
    }
}
fn push(path: &mut PathBuf, value: &str) -> Result<()> {
    ensure!(
        !value.contains('\0')
            && Path::new(value)
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "备份目录超出允许范围"
    );
    #[cfg(windows)]
    ensure!(
        !value.chars().any(|ch| ch <= '\u{1f}') && !value.ends_with(' '),
        "备份名称包含无效字符"
    );
    path.push(value);
    Ok(())
}
impl Scope {
    pub fn directory(&self) -> Result<PathBuf> {
        let mut path = PathBuf::new();
        push(&mut path, &sanitize(&self.connection))?;
        push(&mut path, &sanitize(&self.database))?;
        path.push("backup");
        for part in self
            .path
            .split('/')
            .filter(|part| !crate::java_text::is_blank(part))
        {
            push(&mut path, &sanitize(part))?;
        }
        Ok(path)
    }
    pub fn create_file(&self, filename: Option<&str>) -> Result<PathBuf> {
        let name = filename
            .map(|name| {
                name.chars()
                    .map(|c| {
                        if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                            '_'
                        } else {
                            c
                        }
                    })
                    .collect::<String>()
            })
            .filter(|name| {
                !crate::java_text::is_blank(name) && !matches!(name.as_str(), "." | "..")
            })
            .unwrap_or_else(|| "unnamed.nb3".into());
        let mut path = self.directory()?;
        push(&mut path, &name)?;
        Ok(path)
    }
    pub fn file(&self, name: &str) -> Result<PathBuf> {
        self.create_file(Some(&format!("{name}.nb3")))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Metadata {
    pub meta_version: String,
    pub database_type: String,
    pub catalog: String,
    pub schema: String,
    pub start_time: String,
    pub end_time: String,
    pub encryption: String,
    pub comment: String,
    pub objects: Vec<Object>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Object {
    #[serde(rename = "UUID")]
    pub uuid: String,
    #[serde(rename = "Type")]
    pub kind: String,
    pub name: String,
    pub rows: String,
    pub metadata: Entry,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Entry {
    pub filename: String,
    pub checksum: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Definition {
    pub meta_version: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub kind: String,
    #[serde(rename = "DDL")]
    pub ddl: String,
    #[serde(rename = "SubDDL", default, skip_serializing_if = "Option::is_none")]
    pub sub_ddl: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_increment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,
    #[serde(
        rename = "TriggerDDL",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub trigger_ddl: Option<Vec<String>>,
    #[serde(rename = "IndexDDL", default, skip_serializing_if = "Option::is_none")]
    pub index_ddl: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Vec<Entry>>,
    /// Optional PostgreSQL identity; old nb3 files and Java readers remain compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postgres: Option<PostgresDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oracle: Option<OracleDefinition>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct OracleDefinition {
    pub schema: String,
    pub standard_strings: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PostgresDefinition {
    pub schema: String,
    pub name: String,
    pub drop_sql: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
}
/// Original pages separate SQL value groups with U+001E followed by LF.
pub const RECORD_SEPARATOR: &str = "\u{1e}\n";

mod files;
pub use files::{FileAction, FileOutput, FileRequest, Item};

mod requests;
pub mod sql;
pub use requests::{Create, Kind, Operation, Report, Restore, Selection};
