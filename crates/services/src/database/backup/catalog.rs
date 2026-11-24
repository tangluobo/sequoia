//! Fetch actual backup objects on the database actor, never on the rendering thread.
use super::super::*;
use tomato_core::{
    backup::{Kind, Selection},
    workbench::TableId,
};

impl Databases {
    pub async fn backup_objects(
        &mut self,
        connection: &Connection,
        namespace: &str,
    ) -> Result<Vec<Selection>> {
        self.backup_objects_in(connection, namespace, None).await
    }
    pub async fn backup_objects_in(
        &mut self,
        connection: &Connection,
        namespace: &str,
        schema: Option<&str>,
    ) -> Result<Vec<Selection>> {
        anyhow::ensure!(namespace.len() <= 256 * 1024, "数据库名称过长");
        anyhow::ensure!(
            schema.is_none_or(|schema| schema.len() <= 256 * 1024),
            "schema名称过长"
        );
        anyhow::ensure!(
            matches!(connection.kind(), "MYSQL" | "POSTGRESQL" | "ORACLE"),
            "此数据库的备份适配器尚未迁移"
        );
        let target = tomato_core::database_scope::select(connection, namespace);
        let conn = self.session(&target).await?;
        if connection.kind() == "ORACLE" {
            return super::oracle::list(conn, namespace).await;
        }
        if connection.kind() == "POSTGRESQL" {
            return super::postgres::list(conn, schema).await;
        }
        let mut objects = Vec::new();
        let mut bytes = 0usize;
        for (kind, sql) in [
            (
                Kind::Table,
                "SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA=? AND TABLE_TYPE='BASE TABLE' ORDER BY TABLE_NAME",
            ),
            (
                Kind::View,
                "SELECT TABLE_NAME FROM information_schema.VIEWS WHERE TABLE_SCHEMA=? ORDER BY TABLE_NAME",
            ),
            (
                Kind::Function,
                "SELECT ROUTINE_NAME FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA=? AND ROUTINE_TYPE='FUNCTION' ORDER BY ROUTINE_NAME",
            ),
            (
                Kind::Event,
                "SELECT EVENT_NAME FROM information_schema.EVENTS WHERE EVENT_SCHEMA=? ORDER BY EVENT_NAME",
            ),
        ] {
            let mut rows = sqlx::query(sql).bind(namespace).fetch(&mut *conn);
            while let Some(row) = rows.try_next().await? {
                anyhow::ensure!(objects.len() < 10_000, "备份对象超过 10,000 项");
                let name = workbench::text(&row, 0)?;
                bytes = bytes
                    .saturating_add(name.capacity())
                    .saturating_add(namespace.len());
                let before = objects.capacity();
                objects.push(Selection {
                    kind,
                    table: TableId {
                        schema: namespace.into(),
                        name,
                    },
                });
                bytes = bytes.saturating_add(
                    (objects.capacity() - before) * std::mem::size_of::<Selection>(),
                );
                anyhow::ensure!(bytes <= 8 * 1024 * 1024, "备份对象超过 8 MiB");
            }
        }
        Ok(objects)
    }
}
