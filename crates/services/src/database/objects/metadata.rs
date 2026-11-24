//! Version/type/engine/charset options for the original table-design form.
use super::*;
use sqlx::Executor;
use tomato_core::table_design::{Charset, Dialect, Metadata, data_types};
const RESERVE: usize = 16 * 1024;

async fn recover(
    session: &mut AnyConnection,
    transaction: bool,
    point: &str,
    label: &str,
    error: anyhow::Error,
    warnings: &mut Vec<String>,
) -> Result<()> {
    if super::super::recovery::transport(&error) {
        return Err(error);
    }
    if transaction {
        session
            .execute(format!("ROLLBACK TO SAVEPOINT {point}").as_str())
            .await?;
    }
    let mut message = format!("{label}: {error}");
    if message.len() > 4096 {
        let mut end = 4096;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
    }
    warnings.push(message);
    Ok(())
}
async fn version(session: &mut AnyConnection, kind: &str) -> Result<String> {
    if kind == "HIVE" {
        return session
            .backend_mut::<super::super::hive::Backend>()
            .ok_or_else(|| anyhow::anyhow!("Hive 会话类型不匹配"))?
            .version()
            .await;
    }
    let sql = if kind == "ORACLE" {
        "SELECT banner FROM v$version WHERE ROWNUM=1"
    } else if kind == "MYSQL" {
        "SELECT VERSION()"
    } else {
        "SHOW server_version"
    };
    let row = sqlx::query(sql).fetch_one(&mut *session).await?;
    let version = workbench::text(&row, 0)?;
    ensure!(version.len() <= 4096, "服务器版本字符串过长");
    Ok(version)
}
async fn engines(session: &mut AnyConnection, limit: usize) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let mut bytes = 0;
    let mut rows=sqlx::query("SELECT ENGINE FROM information_schema.ENGINES WHERE SUPPORT IN ('YES','DEFAULT') ORDER BY ENGINE").fetch(&mut *session);
    while let Some(row) = rows.try_next().await? {
        ensure!(names.len() < 256, "存储引擎超过 256 项");
        let name = workbench::text(&row, 0)?;
        let capacity = names.capacity();
        bytes += name.capacity();
        names.push(name);
        bytes += (names.capacity() - capacity) * std::mem::size_of::<String>();
        ensure!(bytes <= limit, "设计器元数据超过 8 MiB");
    }
    Ok(names)
}
async fn charsets(session: &mut AnyConnection, kind: &str, limit: usize) -> Result<Vec<Charset>> {
    let sql = if kind == "MYSQL" {
        "SELECT CHARACTER_SET_NAME,COLLATION_NAME FROM information_schema.COLLATIONS ORDER BY CHARACTER_SET_NAME,COLLATION_NAME"
    } else {
        // Stock PostgreSQL has pg_encoding_to_char, not the original query's
        // nonstandard pg_encodings view. -1 collations apply to every encoding.
        "SELECT pg_encoding_to_char(CASE WHEN collencoding<0 THEN pg_char_to_encoding(current_setting('server_encoding')) ELSE collencoding END)::text,collname::text FROM pg_catalog.pg_collation WHERE collencoding IN (-1,pg_char_to_encoding(current_setting('server_encoding'))) ORDER BY 1,2"
    };
    let mut names: Vec<Charset> = Vec::new();
    let mut bytes = 0;
    let mut count = 0;
    let mut rows = sqlx::query(sql).fetch(&mut *session);
    while let Some(row) = rows.try_next().await? {
        count += 1;
        ensure!(count <= 10_000, "排序规则超过 10,000 项");
        let name = workbench::text(&row, 0)?;
        let collation = workbench::text(&row, 1)?;
        if names.last().is_none_or(|last| last.name != name) {
            let capacity = names.capacity();
            bytes += name.capacity();
            names.push(Charset {
                name,
                collations: Vec::new(),
            });
            bytes += (names.capacity() - capacity) * std::mem::size_of::<Charset>();
        }
        let group = names.last_mut().unwrap();
        let capacity = group.collations.capacity();
        bytes += collation.capacity();
        group.collations.push(collation);
        bytes += (group.collations.capacity() - capacity) * std::mem::size_of::<String>();
        ensure!(bytes <= limit, "设计器元数据超过 8 MiB");
    }
    Ok(names)
}
pub(in crate::database) async fn read(session: &mut AnyConnection, kind: &str) -> Result<Metadata> {
    ensure!(
        matches!(kind, "MYSQL" | "POSTGRESQL" | "ORACLE" | "HIVE"),
        "当前数据库尚未实现表设计器元数据"
    );
    let dialect = Dialect::resolve(kind)?;
    let mut metadata = Metadata::empty(dialect);
    metadata.backslash_escapes = super::designer::string_mode(session, kind).await?;
    let point = workbench::savepoint_name(kind, "metadata");
    let transaction = workbench::existing_savepoint(session, kind, &point).await?;
    match version(session, kind).await {
        Ok(version) => {
            metadata.version = Some(version);
            metadata.data_types = data_types(dialect, metadata.version.as_deref());
        }
        Err(error) => {
            recover(
                session,
                transaction,
                &point,
                "读取服务器版本失败",
                error,
                &mut metadata.warnings,
            )
            .await?
        }
    }
    if kind == "MYSQL" {
        match engines(session, BUDGET - RESERVE - metadata.retained_bytes()).await {
            Ok(engines) => metadata.engines = engines,
            Err(error) => {
                recover(
                    session,
                    transaction,
                    &point,
                    "读取存储引擎失败",
                    error,
                    &mut metadata.warnings,
                )
                .await?
            }
        }
    }
    if !matches!(kind, "ORACLE" | "HIVE") {
        match charsets(session, kind, BUDGET - RESERVE - metadata.retained_bytes()).await {
            Ok(charsets) => metadata.charsets = charsets,
            Err(error) => {
                recover(
                    session,
                    transaction,
                    &point,
                    "读取字符集失败",
                    error,
                    &mut metadata.warnings,
                )
                .await?
            }
        }
    }
    if transaction {
        workbench::release_savepoint(session, kind, &point).await?;
    }
    ensure!(
        metadata.retained_bytes() <= BUDGET,
        "设计器元数据超过 8 MiB"
    );
    Ok(metadata)
}
