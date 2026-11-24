//! Original named-query layout; the file repository owns I/O and conflict checks.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// Matches SqlEditorView.sanitizeFileName, including Java's ASCII-only `\s`.
pub fn sanitize(name: Option<&str>) -> String {
    let Some(name) = name.filter(|name| !name.is_empty()) else {
        return "unnamed".into();
    };
    let mut result = String::with_capacity(name.len());
    let mut underscore = false;
    for ch in name.chars() {
        let ch = if matches!(
            ch,
            '\\' | '/'
                | ':'
                | '*'
                | '?'
                | '"'
                | '<'
                | '>'
                | '|'
                | ' '
                | '\t'
                | '\r'
                | '\n'
                | '\u{b}'
                | '\u{c}'
        ) {
            '_'
        } else {
            ch
        };
        if ch == '_' {
            if !underscore && !result.is_empty() {
                result.push(ch);
            }
            underscore = true;
        } else {
            result.push(ch);
            underscore = false;
        }
    }
    if result.ends_with('_') {
        result.pop();
    }
    result
}
fn push(path: &mut PathBuf, value: &str) -> Result<()> {
    ensure!(!value.contains('\0'), "查询名称包含无效字符");
    if !value.is_empty() {
        ensure!(
            Path::new(value)
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
            "查询名称不能越出保存目录"
        );
        path.push(value);
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Scope {
    pub connection: String,
    pub database: String,
    pub path: String,
}
impl Scope {
    pub fn root(&self) -> Result<PathBuf> {
        let mut path = PathBuf::new();
        push(&mut path, &sanitize(Some(&self.connection)))?;
        push(&mut path, &sanitize(Some(&self.database)))?;
        path.push("query");
        Ok(path)
    }
    pub fn directory(&self) -> Result<PathBuf> {
        let mut path = self.root()?;
        if !self.path.is_empty() {
            let mut parts = self.path.split('/').collect::<Vec<_>>();
            while parts.last() == Some(&"") {
                parts.pop();
            }
            for part in parts {
                push(&mut path, &sanitize(Some(part)))?;
            }
        }
        Ok(path)
    }
    pub fn file(&self, name: &str) -> Result<PathBuf> {
        let mut path = self.directory()?;
        push(&mut path, &format!("{}.sql", sanitize(Some(name))))?;
        Ok(path)
    }
    pub fn child(&self, name: &str) -> Result<Self> {
        let clean = sanitize(Some(name));
        let mut check = PathBuf::new();
        push(&mut check, &clean)?;
        ensure!(!clean.is_empty(), "目录名称不能为空");
        Ok(Self {
            path: if self.path.is_empty() {
                clean
            } else {
                format!("{}/{clean}", self.path)
            },
            ..self.clone()
        })
    }
    fn bytes(&self) -> usize {
        self.connection
            .capacity()
            .saturating_add(self.database.capacity())
            .saturating_add(self.path.capacity())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub directory: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    List(Scope),
    EnsureDirectory(Scope),
    Read {
        scope: Scope,
        name: String,
    },
    Save {
        scope: Scope,
        name: String,
        text: String,
        expected: Option<String>,
    },
    CreateDirectory {
        scope: Scope,
        name: String,
    },
    Rename {
        scope: Scope,
        entry: Entry,
        name: String,
    },
    Delete {
        scope: Scope,
        entries: Vec<Entry>,
    },
}
impl Request {
    pub fn scope(&self) -> &Scope {
        match self {
            Self::List(scope)
            | Self::EnsureDirectory(scope)
            | Self::Read { scope, .. }
            | Self::Save { scope, .. }
            | Self::CreateDirectory { scope, .. }
            | Self::Rename { scope, .. }
            | Self::Delete { scope, .. } => scope,
        }
    }
    pub fn queued_bytes(&self) -> usize {
        let bytes = self.scope().bytes();
        bytes.saturating_add(match self {
            Self::List(_) | Self::EnsureDirectory(_) => 0,
            Self::Read { name, .. } | Self::CreateDirectory { name, .. } => name.capacity(),
            Self::Save {
                name,
                text,
                expected,
                ..
            } => name
                .capacity()
                .saturating_add(text.capacity())
                .saturating_add(expected.as_ref().map_or(0, String::capacity)),
            Self::Rename { entry, name, .. } => {
                entry.name.capacity().saturating_add(name.capacity())
            }
            Self::Delete { entries, .. } => entries.iter().fold(
                entries
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Entry>()),
                |sum, entry| sum.saturating_add(entry.name.capacity()),
            ),
        })
    }
}
#[derive(Debug, Serialize)]
pub enum Output {
    Directory {
        scope: Scope,
        path: PathBuf,
    },
    Listed {
        scope: Scope,
        entries: Vec<Entry>,
    },
    Document {
        scope: Scope,
        name: String,
        text: String,
        stamp: String,
    },
    Saved {
        scope: Scope,
        name: String,
        stamp: String,
    },
    Changed {
        scope: Scope,
        renamed: Option<(PathBuf, PathBuf)>,
        deleted: Vec<PathBuf>,
        errors: Vec<String>,
    },
}
