//! Sidebar identity includes PostgreSQL's actual database, independently of SQL schema.
//! NUL cannot occur in a database identifier, so it cannot collide with a legacy
//! literal schema. Keys are transient UI/cache values, never filesystem paths.
use tomato_core::Connection;
const PREFIX: &str = "\0pg:";
pub(super) struct Scope<'a> {
    pub database: &'a str,
    pub namespace: &'a str,
}
pub(in crate::app) fn key(connection: &Connection, namespace: &str) -> String {
    if !namespace.is_empty()
        && connection.kind() == "POSTGRESQL"
        && !connection.text("database").is_empty()
    {
        let database = connection.text("database");
        format!("{PREFIX}{}:{database}{namespace}", database.len())
    } else {
        namespace.into()
    }
}
pub(super) fn decode(key: &str) -> anyhow::Result<Option<Scope<'_>>> {
    let Some(value) = key.strip_prefix(PREFIX) else {
        anyhow::ensure!(!key.contains('\0'), "查询目录标识无效");
        return Ok(None);
    };
    let (length, value) = value
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("查询目录标识无效"))?;
    let length: usize = length
        .parse()
        .map_err(|_| anyhow::anyhow!("查询目录标识长度无效"))?;
    let (database, namespace) = value
        .split_at_checked(length)
        .ok_or_else(|| anyhow::anyhow!("查询目录标识长度无效"))?;
    anyhow::ensure!(
        !database.is_empty() && !namespace.contains('\0') && !database.contains('\0'),
        "查询目录标识无效"
    );
    Ok(Some(Scope {
        database,
        namespace,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_schema_in_different_actual_databases_has_different_identity_without_sql_rewriting() {
        let mut connection = Connection::new("POSTGRESQL");
        connection.set("database", "数据库:甲");
        let first = key(&connection, "public:资料");
        connection.set("database", "数据库:乙");
        let second = key(&connection, "public:资料");
        assert_ne!(first, second);
        assert_eq!(decode(&first).unwrap().unwrap().database, "数据库:甲");
        assert_eq!(decode(&second).unwrap().unwrap().namespace, "public:资料");
        assert!(decode("public").unwrap().is_none());
        assert!(decode("\0pg:1:数据库").is_err());
        assert!(decode("\0pg:999:x").is_err());
        assert!(decode("\0pg:0:public").is_err());
        let mut mysql = Connection::new("MYSQL");
        mysql.set("database", "库");
        assert_eq!(key(&mysql, "库"), "库");
    }
}
