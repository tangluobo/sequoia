//! Strategy: SQL syntax varies independently of connection/session transport.
use anyhow::{Result, bail};

pub trait SqlDialect: Send + Sync {
    fn quote(&self, identifier: &str) -> Result<String>;
    fn tables(&self) -> &'static str;
    fn page(&self, schema: &str, table: &str, limit: u32, offset: u64) -> Result<String> {
        let table = if schema.is_empty() {
            self.quote(table)?
        } else {
            format!("{}.{}", self.quote(schema)?, self.quote(table)?)
        };
        Ok(format!(
            "SELECT * FROM {table} LIMIT {} OFFSET {offset}",
            limit.clamp(1, 10000)
        ))
    }
}
struct MySql;
struct Postgres;
struct Sqlite;
struct Oracle;
struct Hive;

fn quoted(identifier: &str, delimiter: char) -> Result<String> {
    if identifier.is_empty() || identifier.contains('\0') {
        bail!("表名和字段名不能为空或包含 NUL");
    }
    Ok(format!(
        "{delimiter}{}{delimiter}",
        identifier.replace(delimiter, &format!("{delimiter}{delimiter}"))
    ))
}
impl SqlDialect for MySql {
    fn quote(&self, identifier: &str) -> Result<String> {
        quoted(identifier, '`')
    }
    fn tables(&self) -> &'static str {
        "SHOW TABLES"
    }
}
impl SqlDialect for Hive {
    fn quote(&self, identifier: &str) -> Result<String> {
        quoted(identifier, '`')
    }
    fn tables(&self) -> &'static str {
        "SHOW TABLES"
    }
    fn page(&self, schema: &str, table: &str, limit: u32, offset: u64) -> Result<String> {
        let table = if schema.is_empty() {
            self.quote(table)?
        } else {
            format!("{}.{}", self.quote(schema)?, self.quote(table)?)
        };
        Ok(format!(
            "SELECT * FROM {table} LIMIT {offset},{}",
            limit.clamp(1, 10000)
        ))
    }
}
impl SqlDialect for Postgres {
    fn quote(&self, identifier: &str) -> Result<String> {
        quoted(identifier, '"')
    }
    fn tables(&self) -> &'static str {
        "SELECT table_schema, table_name FROM information_schema.tables WHERE table_schema NOT IN ('pg_catalog','information_schema') ORDER BY 1,2"
    }
}
impl SqlDialect for Sqlite {
    fn quote(&self, identifier: &str) -> Result<String> {
        quoted(identifier, '"')
    }
    fn tables(&self) -> &'static str {
        "SELECT name, sql FROM sqlite_master WHERE type='table' ORDER BY name"
    }
}
impl SqlDialect for Oracle {
    fn quote(&self, identifier: &str) -> Result<String> {
        quoted(identifier, '"')
    }
    fn tables(&self) -> &'static str {
        "SELECT owner,table_name FROM all_tables ORDER BY owner,table_name"
    }
    fn page(&self, schema: &str, table: &str, limit: u32, offset: u64) -> Result<String> {
        let table = if schema.is_empty() {
            self.quote(table)?
        } else {
            format!("{}.{}", self.quote(schema)?, self.quote(table)?)
        };
        Ok(format!(
            "SELECT * FROM {table} OFFSET {offset} ROWS FETCH NEXT {} ROWS ONLY",
            limit.clamp(1, 10000)
        ))
    }
}
pub fn resolve(kind: &str) -> Result<&'static dyn SqlDialect> {
    match kind {
        "MYSQL" => Ok(&MySql),
        "POSTGRESQL" => Ok(&Postgres),
        "SQLITE" => Ok(&Sqlite),
        "ORACLE" => Ok(&Oracle),
        "HIVE" => Ok(&Hive),
        _ => bail!("{kind} 尚无 SQL 方言适配器"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quotes_identifiers_without_allowing_statement_injection() -> Result<()> {
        assert_eq!(
            resolve("MYSQL")?.page("", "table`; DROP TABLE x; --", 20, 0)?,
            "SELECT * FROM `table``; DROP TABLE x; --` LIMIT 20 OFFSET 0"
        );
        assert_eq!(
            resolve("POSTGRESQL")?.page("a\"b", "t", 50, 100)?,
            "SELECT * FROM \"a\"\"b\".\"t\" LIMIT 50 OFFSET 100"
        );
        assert!(resolve("SQLITE")?.page("", "", 1, 0).is_err());
        Ok(())
    }
}
