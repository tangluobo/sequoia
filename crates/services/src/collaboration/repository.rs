use super::*;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{path::Path, str::FromStr};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cached {
    pub object: Object,
    pub dirty: bool,
    pub conflict: bool,
    pub local_id: Option<String>,
    pub error: Option<String>,
}
pub struct Repository {
    pool: SqlitePool,
}
impl Repository {
    pub async fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let options = SqliteConnectOptions::from_str("sqlite:")?
            .filename(path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .foreign_keys(true)
            .pragma("cache_size", "-2048");
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        sqlx::raw_sql("CREATE TABLE IF NOT EXISTS cache(id TEXT PRIMARY KEY, project TEXT NOT NULL, meta TEXT NOT NULL, body TEXT NOT NULL, revision INTEGER NOT NULL, deleted INTEGER NOT NULL, pending TEXT, remote TEXT, local_id TEXT,error TEXT,parent TEXT NOT NULL DEFAULT '',origin INTEGER NOT NULL DEFAULT 0); CREATE INDEX IF NOT EXISTS by_project ON cache(project,deleted); CREATE TABLE IF NOT EXISTS state(key TEXT PRIMARY KEY,value TEXT NOT NULL);").execute(&pool).await?;
        for name in ["error", "parent", "origin", "baseline"] {
            let count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM pragma_table_info('cache') WHERE name=?")
                    .bind(name)
                    .fetch_one(&pool)
                    .await?;
            if count == 0 {
                let statement = if name == "baseline" {
                    "ALTER TABLE cache ADD COLUMN baseline TEXT NOT NULL DEFAULT ''"
                } else if name == "origin" {
                    "ALTER TABLE cache ADD COLUMN origin INTEGER NOT NULL DEFAULT 0"
                } else if name == "error" {
                    "ALTER TABLE cache ADD COLUMN error TEXT"
                } else {
                    "ALTER TABLE cache ADD COLUMN parent TEXT NOT NULL DEFAULT ''"
                };
                sqlx::query(statement).execute(&pool).await?;
            }
        }
        Ok(Self { pool })
    }
    pub async fn projects(&self) -> Result<Vec<Project>> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT value FROM state WHERE key='projects'")
                .fetch_optional(&self.pool)
                .await?;
        value
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map(Option::unwrap_or_default)
            .map_err(Into::into)
    }
    pub async fn set_projects(&self, projects: &[Project]) -> Result<()> {
        sqlx::query("INSERT INTO state VALUES('projects',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").bind(serde_json::to_string(projects)?).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn cursor(&self) -> Result<i64> {
        Ok(
            sqlx::query_scalar::<_, String>("SELECT value FROM state WHERE key='cursor'")
                .fetch_optional(&self.pool)
                .await?
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
        )
    }
    pub async fn set_cursor(&self, cursor: i64) -> Result<()> {
        sqlx::query("INSERT INTO state VALUES('cursor',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").bind(cursor.to_string()).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn list(&self, project: &str) -> Result<Vec<Cached>> {
        let rows=sqlx::query("SELECT meta,pending IS NOT NULL AS dirty,remote IS NOT NULL AS conflict,local_id,error FROM cache WHERE project=? ORDER BY id LIMIT 10000").bind(project).fetch_all(&self.pool).await?;
        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            result.push(Cached {
                object: serde_json::from_str(row.try_get("meta")?)?,
                dirty: row.try_get("dirty")?,
                conflict: row.try_get("conflict")?,
                local_id: row.try_get("local_id")?,
                error: row.try_get("error")?,
            });
        }
        result.sort_by(|a, b| a.object.name.cmp(&b.object.name));
        Ok(result)
    }
    pub async fn object(&self, id: &str) -> Result<Object> {
        let row = sqlx::query("SELECT meta,body FROM cache WHERE id=?")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        let mut object: Object = serde_json::from_str(row.try_get("meta")?)?;
        object.body = serde_json::from_str(row.try_get("body")?)?;
        Ok(object)
    }
    pub async fn remote(&self, id: &str) -> Result<Option<Object>> {
        let text: Option<String> = sqlx::query_scalar("SELECT remote FROM cache WHERE id=?")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        text.map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(Into::into)
    }
    pub async fn revision(&self, id: &str) -> Result<Option<i64>> {
        Ok(sqlx::query_scalar("SELECT revision FROM cache WHERE id=?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }
    pub async fn pending(&self) -> Result<Vec<(String, String, Mutation)>> {
        let rows=sqlx::query("SELECT c.id,c.project,c.pending FROM cache c WHERE c.pending IS NOT NULL AND c.remote IS NULL AND c.error IS NULL AND (c.deleted=0 OR NOT EXISTS(SELECT 1 FROM cache child WHERE child.parent=c.id AND (child.deleted=0 OR child.pending IS NOT NULL))) ORDER BY c.rowid LIMIT 1").fetch_all(&self.pool).await?;
        let mut out = Vec::new();
        for row in rows {
            out.push((
                row.try_get("id")?,
                row.try_get("project")?,
                serde_json::from_str(row.try_get("pending")?)?,
            ))
        }
        Ok(out)
    }
    pub async fn enqueue(&self, project: &str, id: &str, mut mutation: Mutation) -> Result<()> {
        self.enqueue_origin(project, id, &mut mutation, false).await
    }
    pub async fn enqueue_local(
        &self,
        project: &str,
        id: &str,
        mut mutation: Mutation,
    ) -> Result<()> {
        self.enqueue_origin(project, id, &mut mutation, true).await
    }
    async fn enqueue_origin(
        &self,
        project: &str,
        id: &str,
        mutation: &mut Mutation,
        origin: bool,
    ) -> Result<()> {
        mutation.validate()?;
        let old: Option<(i64, Option<String>)> =
            sqlx::query_as("SELECT revision,pending FROM cache WHERE id=?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        let mut remote = None;
        if let Some((revision, pending)) = &old {
            if let Some(pending) = pending {
                let old: Mutation = serde_json::from_str(pending)?;
                mutation.base_revision = old.base_revision;
            } else if *revision != mutation.base_revision {
                let current = self.object(id).await?;
                if current.name == mutation.name
                    && current.parent_id == mutation.parent_id
                    && current.kind == mutation.kind
                    && current.deleted == mutation.deleted
                    && current.body == mutation.body
                {
                    // An unchanged save may arrive before the UI observes its previous ACK.
                    mutation.base_revision = *revision;
                } else {
                    remote = Some(serde_json::to_string(&current)?);
                }
            }
        }
        let o = Object {
            id: id.into(),
            project_id: project.into(),
            parent_id: mutation.parent_id.clone(),
            kind: mutation.kind,
            name: mutation.name.clone(),
            body: serde_json::Value::Null,
            bytes: serde_json::to_vec(&mutation.body)?.len(),
            revision: mutation.base_revision,
            deleted: mutation.deleted,
            modified: 0,
            modified_by: String::new(),
            mutation_id: mutation.mutation_id.clone(),
        };
        sqlx::query("INSERT INTO cache(id,project,meta,body,revision,deleted,pending,remote,parent,origin) VALUES(?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET meta=excluded.meta,body=excluded.body,pending=excluded.pending,deleted=excluded.deleted,remote=COALESCE(excluded.remote,cache.remote),error=NULL,parent=excluded.parent,origin=excluded.origin").bind(id).bind(project).bind(serde_json::to_string(&o)?).bind(serde_json::to_string(&mutation.body)?).bind(mutation.base_revision).bind(mutation.deleted).bind(serde_json::to_string(&mutation)?).bind(remote).bind(&mutation.parent_id).bind(origin).execute(&self.pool).await?;
        Ok(())
    }
    /// Remote changes never replace a pending local edit. Both versions remain on disk.
    pub async fn receive(&self, object: Object) -> Result<()> {
        let pending: Option<Option<String>> =
            sqlx::query_scalar("SELECT pending FROM cache WHERE id=?")
                .bind(&object.id)
                .fetch_optional(&self.pool)
                .await?;
        if let Some(Some(pending)) = pending {
            let mutation: Mutation = serde_json::from_str(&pending)?;
            if object.mutation_id != mutation.mutation_id {
                if object.revision != mutation.base_revision {
                    sqlx::query("UPDATE cache SET remote=? WHERE id=?")
                        .bind(serde_json::to_string(&object)?)
                        .bind(&object.id)
                        .execute(&self.pool)
                        .await?;
                }
                return Ok(());
            }
        }
        self.replace(object).await
    }
    async fn replace(&self, mut object: Object) -> Result<()> {
        let body = std::mem::take(&mut object.body);
        sqlx::query("INSERT INTO cache(id,project,meta,body,revision,deleted,parent) VALUES(?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET meta=excluded.meta,body=excluded.body,revision=excluded.revision,deleted=excluded.deleted,pending=NULL,remote=NULL,error=NULL,parent=excluded.parent,origin=excluded.origin").bind(&object.id).bind(&object.project_id).bind(serde_json::to_string(&object)?).bind(serde_json::to_string(&body)?).bind(object.revision).bind(object.deleted).bind(&object.parent_id).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn resolve(&self, id: &str, keep_local: bool) -> Result<()> {
        let remote = self
            .remote(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("没有需要处理的冲突"))?;
        if keep_local {
            let mut local = Mutation::from_object(&self.object(id).await?);
            local.base_revision = remote.revision;
            sqlx::query("UPDATE cache SET revision=?,pending=?,remote=NULL,error=NULL WHERE id=?")
                .bind(remote.revision)
                .bind(serde_json::to_string(&local)?)
                .bind(id)
                .execute(&self.pool)
                .await?;
        } else {
            self.replace(remote).await?;
        }
        Ok(())
    }
    pub async fn bind(&self, id: &str, local: &str) -> Result<()> {
        ensure!(!local.is_empty(), "本地连接标识为空");
        let object = self.object(id).await?;
        let baseline = super::connection_stamp(&object.name, &object.body);
        sqlx::query("UPDATE cache SET local_id=?,baseline=? WHERE id=?")
            .bind(local)
            .bind(baseline)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn bindings(&self) -> Result<Vec<(String, String, bool)>> {
        Ok(sqlx::query_as(
            "SELECT id,local_id,origin FROM cache WHERE local_id IS NOT NULL AND deleted=0",
        )
        .fetch_all(&self.pool)
        .await?)
    }
    pub async fn clear(&self) -> Result<()> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cache WHERE pending IS NOT NULL")
            .fetch_one(&self.pool)
            .await?;
        ensure!(count == 0, "仍有待上传修改，请先同步、解决冲突或下载备份");
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM cache")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM state")
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        sqlx::query("VACUUM").execute(&self.pool).await?;
        Ok(())
    }
    pub async fn block(&self, id: &str, error: &str) -> Result<()> {
        sqlx::query("UPDATE cache SET error=? WHERE id=?")
            .bind(error)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn retry_blocked(&self) -> Result<()> {
        sqlx::query("UPDATE cache SET error=NULL WHERE error IS NOT NULL")
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn baseline(&self, id: &str) -> Result<String> {
        Ok(sqlx::query_scalar("SELECT baseline FROM cache WHERE id=?")
            .bind(id)
            .fetch_one(&self.pool)
            .await?)
    }
    pub async fn set_baseline(&self, id: &str, value: &str) -> Result<()> {
        sqlx::query("UPDATE cache SET baseline=? WHERE id=?")
            .bind(value)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
