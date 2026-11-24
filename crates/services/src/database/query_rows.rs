//! Query-result writeback stays on the original query session and rechecks metadata.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use tomato_core::{
    query_edit::Plan,
    workbench::{Cell, RowChange, TableId},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Editing {
    pub table: TableId,
    pub plan: Plan,
    #[serde(default)]
    pub comments: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Change {
    pub original: Vec<Option<String>>,
    pub modified: BTreeMap<usize, Option<String>>,
    pub delete: bool,
}
impl Editing {
    pub fn retained_bytes(&self) -> usize {
        let base = std::mem::size_of::<Self>()
            + self.table.schema.capacity()
            + self.table.name.capacity()
            + self.plan.status_text.capacity()
            + self.plan.editable_column_indexes.capacity() * std::mem::size_of::<usize>();
        let strings = self
            .plan
            .database_column_names
            .iter()
            .chain(&self.plan.primary_key_columns)
            .fold(base, |bytes, value| {
                bytes.saturating_add(value.capacity() + std::mem::size_of::<String>())
            });
        self.comments.iter().fold(strings, |bytes, (key, value)| {
            bytes.saturating_add(key.capacity() + value.capacity() + 128)
        })
    }
}
impl Change {
    pub fn queued_bytes(&self) -> usize {
        self.original.iter().chain(self.modified.values()).fold(
            self.original.capacity() * std::mem::size_of::<Option<String>>()
                + self.modified.len() * 64,
            |bytes, value| bytes.saturating_add(value.as_ref().map_or(0, String::capacity)),
        )
    }
}
impl Databases {
    pub async fn query_editing(
        &mut self,
        connection: &Connection,
        target: &str,
        result: &QueryResult,
    ) -> Result<Option<Editing>> {
        match self.query_editing_once(connection, target, result).await {
            Ok(output) => Ok(output),
            Err(error) => Err(self.recover_operation_error(connection, error).await),
        }
    }
    async fn query_editing_once(
        &mut self,
        connection: &Connection,
        target: &str,
        result: &QueryResult,
    ) -> Result<Option<Editing>> {
        let kind = connection.kind();
        if !matches!(kind, "MYSQL" | "POSTGRESQL") || result.origin_error.is_some() {
            return Ok(None);
        }
        let clean = target
            .trim_matches(|ch: char| ch <= ' ')
            .replace(['`', '"', '[', ']'], "");
        let parts: Vec<_> = clean.split('.').map(str::trim).collect();
        let Some(name) = parts.last().filter(|name| !name.is_empty()) else {
            return Ok(None);
        };
        let names: BTreeSet<_> = result
            .origins
            .iter()
            .filter(|origin| !origin.table.is_empty())
            .map(|origin| origin.table.as_str())
            .collect();
        let namespaces: BTreeSet<_> = result
            .origins
            .iter()
            .filter(|origin| !origin.table.is_empty())
            .map(|origin| {
                if kind == "MYSQL" {
                    origin.catalog.as_str()
                } else {
                    origin.schema.as_str()
                }
            })
            .filter(|name| !name.is_empty())
            .collect();
        if names.len() != 1 || namespaces.len() > 1 {
            return Ok(None);
        }
        let actual = *names.first().unwrap();
        if !tomato_core::query_edit::identifier_eq(actual, name) {
            return Ok(None);
        }
        let namespace = namespaces
            .first()
            .copied()
            .or_else(|| parts.len().checked_sub(2).map(|index| parts[index]));
        let Some(namespace) = namespace.filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        let table = TableId {
            schema: namespace.into(),
            name: actual.into(),
        };
        let definition = workbench::describe(self.session(connection).await?, kind, &table).await?;
        let sources: Vec<_> = result
            .origins
            .iter()
            .map(|origin| origin.column.clone())
            .collect();
        let tables: Vec<_> = result
            .origins
            .iter()
            .map(|origin| origin.table.clone())
            .collect();
        let plan = Plan::create(
            Some(kind),
            actual,
            &result.columns,
            Some(&sources),
            Some(&tables),
            &definition.columns,
        );
        let sql = if kind == "MYSQL" {
            "SELECT COLUMN_NAME,COLUMN_COMMENT FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY ORDINAL_POSITION"
        } else {
            "SELECT a.attname::text,COALESCE(col_description(c.oid,a.attnum),'')::text FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid=a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum"
        };
        let rows = workbench::metadata(
            self.session(connection).await?,
            sql,
            &[
                Cell::Text(table.schema.clone()),
                Cell::Text(table.name.clone()),
            ],
        )
        .await?;
        let mut comments = BTreeMap::new();
        let mut bytes = 0usize;
        for row in rows {
            let name = workbench::text(&row, 0)?;
            let comment = workbench::text(&row, 1)?;
            if !comment.is_empty() {
                bytes = bytes.saturating_add(name.capacity() + comment.capacity() + 128);
                anyhow::ensure!(bytes <= 256 * 1024, "列注释超过 256 KiB，请缩小查询范围");
                comments.insert(name, comment);
            }
        }
        Ok(Some(Editing {
            table,
            plan,
            comments,
        }))
    }
    pub async fn save_query_rows(
        &mut self,
        connection: &Connection,
        editing: Editing,
        changes: Vec<Change>,
    ) -> Result<u64> {
        match self
            .save_query_rows_once(connection, editing, changes)
            .await
        {
            Ok(output) => Ok(output),
            Err(error) => Err(self.recover_operation_error(connection, error).await),
        }
    }
    async fn save_query_rows_once(
        &mut self,
        connection: &Connection,
        editing: Editing,
        changes: Vec<Change>,
    ) -> Result<u64> {
        anyhow::ensure!(changes.len() <= 1000, "每批最多保存 1,000 行修改");
        anyhow::ensure!(
            changes
                .first()
                .is_none_or(|first| changes.iter().all(|change| change.delete == first.delete)),
            "保存和删除必须分开提交"
        );
        let kind = connection.kind();
        anyhow::ensure!(
            matches!(kind, "MYSQL" | "POSTGRESQL"),
            "当前数据库不支持查询结果保存"
        );
        // A qualified target may be in a different namespace; never select a new
        // session based on that namespace or merge into an unrelated transaction.
        let conn = self.session(connection).await?;
        let fresh = workbench::describe(conn, kind, &editing.table).await?;
        let mapping = &editing.plan.database_column_names;
        let sources: Vec<_> = mapping
            .iter()
            .map(|name| {
                if name.starts_with('\0') {
                    String::new()
                } else {
                    name.clone()
                }
            })
            .collect();
        let tables: Vec<_> = sources
            .iter()
            .map(|name| {
                if name.is_empty() {
                    String::new()
                } else {
                    editing.table.name.clone()
                }
            })
            .collect();
        let plan = Plan::create(
            Some(kind),
            &editing.table.name,
            mapping,
            Some(&sources),
            Some(&tables),
            &fresh.columns,
        );
        anyhow::ensure!(
            plan.is_editable()
                && plan.primary_key_columns == editing.plan.primary_key_columns
                && plan.uses_original_row_match == editing.plan.uses_original_row_match,
            "表结构已变化或结果不能安全回写，请重新查询"
        );
        let mut subset = fresh.clone();
        let mut indexes = Vec::new();
        subset.columns.clear();
        for index in &plan.editable_column_indexes {
            let column = fresh
                .columns
                .iter()
                .find(|column| column.name == mapping[*index])
                .ok_or_else(|| anyhow::anyhow!("源列已变化，请重新查询"))?;
            indexes.push(*index);
            subset.columns.push(column.clone());
        }
        let convert = |value: &Option<String>,
                       column: &tomato_core::workbench::Column|
         -> Result<Cell> {
            let Some(value) = value else {
                return Ok(Cell::Null);
            };
            if kind == "MYSQL" && workbench::plan::mysql_bit(column) {
                return workbench::plan::mysql_bit_value(column, &Cell::Text(value.clone()));
            }
            if column.binary {
                if kind == "POSTGRESQL" {
                    let hex = value.strip_prefix("\\x").unwrap_or(value);
                    anyhow::ensure!(hex.len().is_multiple_of(2), "二进制值必须为十六进制");
                    let bytes = (0..hex.len())
                        .step_by(2)
                        .map(|index| {
                            hex.get(index..index + 2)
                                .ok_or_else(|| anyhow::anyhow!("二进制值必须为十六进制"))
                                .and_then(|value| u8::from_str_radix(value, 16).map_err(Into::into))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    Ok(Cell::Binary(bytes))
                } else {
                    Ok(Cell::Binary(value.as_bytes().to_vec()))
                }
            } else {
                Ok(Cell::Text(value.clone()))
            }
        };
        let mut statements = Vec::new();
        for change in changes {
            anyhow::ensure!(change.original.len() == mapping.len(), "原始结果列数不一致");
            anyhow::ensure!(
                change
                    .modified
                    .keys()
                    .all(|index| plan.editable_column_indexes.contains(index)),
                "表达式或未映射列不可修改"
            );
            let original = indexes
                .iter()
                .zip(&subset.columns)
                .map(|(index, column)| convert(&change.original[*index], column))
                .collect::<Result<Vec<_>>>()?;
            let write = if change.delete {
                anyhow::ensure!(plan.can_delete(), "无完整主键结果不提供删除操作");
                RowChange::Delete { original }
            } else {
                let values = indexes
                    .iter()
                    .zip(&subset.columns)
                    .map(|(index, column)| {
                        convert(
                            change
                                .modified
                                .get(index)
                                .unwrap_or(&change.original[*index]),
                            column,
                        )
                    })
                    .collect::<Result<Vec<_>>>()?;
                RowChange::Update { original, values }
            };
            if let Some(statement) = workbench::plan::build_query(kind, &subset, &write)? {
                statements.push(statement);
            }
        }
        if statements.is_empty() {
            return Ok(0);
        }
        let name = workbench::savepoint_name(kind, "query");
        if workbench::existing_savepoint(conn, kind, &name).await? {
            workbench::release_savepoint(conn, kind, &name).await?;
            anyhow::bail!("当前连接存在未完成的手动事务，拒绝合并查询结果更新");
        }
        let mut transaction = conn.begin().await?;
        let affected = workbench::writes(&mut transaction, &statements).await?;
        transaction.commit().await?;
        Ok(affected)
    }
}
