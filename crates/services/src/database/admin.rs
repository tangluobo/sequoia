//! Database management stays on the connection actor; identifiers are quoted centrally.
use super::*;
use anyhow::{Context, ensure};
use sqlx::Executor;
use tomato_core::{
    database_admin::{Info, Output, Request},
    workbench::Cell,
};
impl Databases {
    pub async fn admin(&mut self, connection: &Connection, request: Request) -> Result<Output> {
        let kind = connection.kind();
        ensure!(
            matches!(kind, "MYSQL" | "POSTGRESQL" | "ORACLE" | "HIVE"),
            "此数据库的管理适配器尚未迁移"
        );
        let mut routed = connection.clone();
        if let Request::Drop { database } = &request {
            if kind != "ORACLE" {
                self.disconnect_database(&connection.id, database).await?;
            }
            if kind == "POSTGRESQL" && routed.text("database") == database {
                routed.set(
                    "database",
                    if database == "postgres" {
                        "template1"
                    } else {
                        "postgres"
                    },
                );
            }
            if kind == "MYSQL" && routed.text("database") == database {
                routed.set("database", "mysql");
            }
            if kind == "HIVE" {
                let configured = routed.text("database");
                let suffix = configured
                    .find([';', '?', '#'])
                    .map_or("", |i| &configured[i..]);
                routed.set("database", format!("default{suffix}"));
            }
        }
        let session = self.session(&routed).await?;
        if let Request::Alter {
            database,
            charset,
            collation,
        } = &request
            && kind == "POSTGRESQL"
        {
            let rows=workbench::metadata(session,"SELECT pg_encoding_to_char(encoding)::text,datcollate::text FROM pg_database WHERE datname=$1",&[Cell::Text(database.clone())]).await?;
            let row = rows.first().context("数据库不存在或无权访问")?;
            if (charset.is_empty() || charset.eq_ignore_ascii_case(&workbench::text(row, 0)?))
                && (collation.is_empty() || collation == &workbench::text(row, 1)?)
            {
                return Ok(Output::Changed {
                    database: database.clone(),
                    dropped: false,
                });
            }
            anyhow::bail!("PostgreSQL 不支持在线修改数据库编码或排序规则");
        }
        if let Some(sql) = request.sql(kind)? {
            // PostgreSQL rejects changing database encoding in place, as in Java.
            // Surface that server error and retain the draft rather than claiming success.
            if matches!(kind, "MYSQL" | "ORACLE") {
                let name = workbench::savepoint_name(kind, "admin");
                if workbench::existing_savepoint(session, kind, &name).await? {
                    workbench::release_savepoint(session, kind, &name).await?;
                    anyhow::bail!("当前连接存在未完成的手动事务，请先提交或回滚");
                }
            }
            session.execute(sql.as_str()).await?;
            return Ok(Output::Changed {
                database: request.database().unwrap_or_default().into(),
                dropped: matches!(request, Request::Drop { .. }),
            });
        }
        let mut info = Info::default();
        if matches!(kind, "ORACLE" | "HIVE") {
            return Ok(if matches!(&request, Request::Alter { .. }) {
                Output::Changed {
                    database: request.database().unwrap_or_default().into(),
                    dropped: false,
                }
            } else {
                Output::Info(info)
            });
        }
        let mut bytes = 0usize;
        if let Some(database) = request.database() {
            let sql = if kind == "MYSQL" {
                "SELECT DEFAULT_CHARACTER_SET_NAME,DEFAULT_COLLATION_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME=?"
            } else {
                "SELECT pg_encoding_to_char(encoding)::text,datcollate::text FROM pg_database WHERE datname=$1"
            };
            let rows = workbench::metadata(session, sql, &[Cell::Text(database.into())]).await?;
            let row = rows.first().context("数据库不存在或无权访问")?;
            info.charset = workbench::text(row, 0)?;
            info.collation = workbench::text(row, 1)?;
        }
        let sql = if kind == "MYSQL" {
            "SELECT CHARACTER_SET_NAME,COLLATION_NAME FROM information_schema.COLLATIONS ORDER BY CHARACTER_SET_NAME,COLLATION_NAME"
        } else {
            "SELECT DISTINCT CASE WHEN collencoding=-1 THEN pg_encoding_to_char(pg_char_to_encoding(current_setting('server_encoding'))) ELSE pg_encoding_to_char(collencoding) END::text,collcollate::text FROM pg_collation WHERE collprovider='c' AND collcollate IS NOT NULL ORDER BY 1,2"
        };
        let mut stream = sqlx::query(sql).fetch(session);
        let mut count = 0usize;
        while let Some(row) = stream.try_next().await? {
            let name = workbench::text(&row, 0)?;
            let collation = workbench::text(&row, 1)?;
            bytes += name.len() + collation.len() + 64;
            count += 1;
            ensure!(
                bytes <= 1024 * 1024 && count <= 10000,
                "数据库字符集目录超过限制"
            );
            if let Some((_, collations)) = info.charsets.last_mut().filter(|(key, _)| key == &name)
            {
                collations.push(collation);
            } else {
                info.charsets.push((name, vec![collation]));
            }
        }
        Ok(Output::Info(info))
    }
}
