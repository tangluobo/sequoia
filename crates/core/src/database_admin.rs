//! Original database create/edit/drop dialog operations and reviewable SQL.
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    Read {
        database: Option<String>,
    },
    Create {
        database: String,
        charset: String,
        collation: String,
    },
    Alter {
        database: String,
        charset: String,
        collation: String,
    },
    Drop {
        database: String,
    },
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Info {
    pub charset: String,
    pub collation: String,
    pub charsets: Vec<(String, Vec<String>)>,
}
#[derive(Clone, Debug, Serialize)]
pub enum Output {
    Info(Info),
    Changed { database: String, dropped: bool },
}
impl Request {
    pub fn database(&self) -> Option<&str> {
        match self {
            Self::Read { database } => database.as_deref(),
            Self::Create { database, .. }
            | Self::Alter { database, .. }
            | Self::Drop { database } => Some(database),
        }
    }
    pub fn sql(&self, kind: &str) -> Result<Option<String>> {
        let (quote, max) = match kind {
            "MYSQL" => ('`', 64),
            "HIVE" => ('`', 128),
            "POSTGRESQL" => ('"', 63),
            "ORACLE" => ('"', 128),
            _ => bail!("此数据库的管理适配器尚未迁移"),
        };
        if matches!(self, Self::Read { .. }) {
            return Ok(None);
        }
        let database = self.database().unwrap_or_default();
        ensure!(
            !database.is_empty()
                && !database.contains('\0')
                && (if kind == "MYSQL" {
                    database.chars().count()
                } else {
                    database.len()
                }) <= max,
            "数据库名称为空或过长"
        );
        let quoted = format!(
            "{quote}{}{quote}",
            database.replace(quote, &format!("{quote}{quote}"))
        );
        if kind == "ORACLE" {
            // Matches DatabaseService: Oracle's database tree lists users.
            // ALTER charset is a no-op in the original implementation.
            return Ok(match self {
                Self::Create { .. } => Some(format!("CREATE USER {quoted} IDENTIFIED BY {quoted}")),
                Self::Drop { .. } => Some(format!("DROP USER {quoted} CASCADE")),
                _ => None,
            });
        }
        if kind == "HIVE" {
            return Ok(match self {
                Self::Create { .. } => Some(format!("CREATE DATABASE {quoted}")),
                Self::Drop { .. } => Some(format!("DROP DATABASE {quoted} CASCADE")),
                _ => None,
            });
        }
        let (create, charset, collation) = match self {
            Self::Drop { .. } => return Ok(Some(format!("DROP DATABASE {quoted}"))),
            Self::Create {
                charset, collation, ..
            } => (true, charset, collation),
            Self::Alter {
                charset, collation, ..
            } => (false, charset, collation),
            _ => unreachable!(),
        };
        let literal = |value: &str| -> Result<String> {
            ensure!(
                !value.contains(['\0', '\\']) && value.len() <= 256,
                "字符集或排序规则无效"
            );
            Ok(format!("'{}'", value.replace('\'', "''")))
        };
        let mut sql = format!(
            "{} DATABASE {quoted}",
            if create { "CREATE" } else { "ALTER" }
        );
        if kind == "MYSQL" {
            for (prefix, value) in [(" CHARACTER SET ", charset), (" COLLATE ", collation)] {
                if !value.is_empty() {
                    ensure!(
                        value
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'_'),
                        "字符集或排序规则标识符无效"
                    );
                    sql.push_str(prefix);
                    sql.push_str(value);
                }
            }
        } else {
            if !charset.is_empty() {
                sql.push_str(if create {
                    " ENCODING "
                } else {
                    " SET encoding = "
                });
                sql.push_str(&literal(charset)?);
            }
            if create && !collation.is_empty() {
                sql.push_str(" LC_COLLATE ");
                sql.push_str(&literal(collation)?);
            }
        }
        Ok(Some(sql))
    }
}
