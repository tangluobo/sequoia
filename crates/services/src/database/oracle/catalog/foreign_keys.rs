//! Read constrained dictionary projections instead of joining expanded grant views.
use super::{metadata, text};
use anyhow::{Context, Result, ensure};
use sqlx::AnyConnection;
use std::collections::BTreeMap;
use tomato_core::{
    database_objects::ForeignKey,
    table_design::auxiliary::fields_label,
    workbench::{Cell, TableId},
};

const BUDGET: usize = 8 * 1024 * 1024;
pub(in crate::database) struct Key {
    pub value: ForeignKey,
    delete_rule: String,
}
impl Key {
    pub(in crate::database) fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.value.name.capacity()
            + self.value.table.schema.capacity()
            + self.value.table.name.capacity()
            + self.value.referenced.schema.capacity()
            + self.value.referenced.name.capacity()
            + self.delete_rule.capacity()
            + field_bytes(&self.value.columns)
            + field_bytes(&self.value.referenced_columns)
    }
    pub(in crate::database) fn into_row(self) -> Vec<String> {
        vec![
            self.value.name,
            fields_label(&self.value.columns),
            self.value.referenced.schema,
            self.value.referenced.name,
            fields_label(&self.value.referenced_columns),
            self.delete_rule,
            "NO ACTION".into(),
        ]
    }
}
fn field_bytes(fields: &Vec<String>) -> usize {
    fields.capacity() * std::mem::size_of::<String>()
        + fields.iter().map(String::capacity).sum::<usize>()
}
struct Columns {
    table: String,
    names: Vec<String>,
}
impl Columns {
    fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.table.capacity() + field_bytes(&self.names)
    }
}
async fn columns(
    session: &mut AnyConnection,
    login: &str,
    owner: &str,
    name: &str,
) -> Result<Columns> {
    let own = owner == login;
    let (sql, bindings) = if own {
        (
            "SELECT table_name,column_name,position FROM user_cons_columns WHERE constraint_name=:1 ORDER BY position",
            vec![Cell::Text(name.into())],
        )
    } else {
        (
            "SELECT table_name,column_name,position FROM all_cons_columns WHERE owner=:1 AND constraint_name=:2 ORDER BY position",
            vec![Cell::Text(owner.into()), Cell::Text(name.into())],
        )
    };
    let rows = metadata(session, sql, &bindings).await?;
    ensure!(
        !rows.is_empty() && rows.len() <= 64,
        "外键约束字段不存在或超过 64 项"
    );
    let table = text(&rows[0], 0)?;
    let mut names = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        ensure!(
            text(row, 0)? == table && text(row, 2)?.parse::<usize>()? == index + 1,
            "外键约束字段顺序或所属表不一致"
        );
        names.push(text(row, 1)?);
    }
    Ok(Columns { table, names })
}

pub(in crate::database) async fn read(
    session: &mut AnyConnection,
    owner: &str,
    table: Option<&str>,
) -> Result<Vec<Key>> {
    // CURRENT_SCHEMA can change independently of the authenticated owner.
    let identity = metadata(
        session,
        "SELECT SYS_CONTEXT('USERENV','SESSION_USER') FROM dual",
        &[],
    )
    .await?;
    let login = text(identity.first().context("无法读取 Oracle 会话身份")?, 0)?;
    let own = owner == login;
    let mut bindings = Vec::new();
    let mut sql = if own {
        "SELECT table_name,constraint_name,r_owner,r_constraint_name,delete_rule FROM user_constraints WHERE constraint_type='R'".to_owned()
    } else {
        bindings.push(Cell::Text(owner.into()));
        "SELECT table_name,constraint_name,r_owner,r_constraint_name,delete_rule FROM all_constraints WHERE owner=:1 AND constraint_type='R'".to_owned()
    };
    if let Some(table) = table {
        bindings.push(Cell::Text(table.into()));
        sql.push_str(&format!(" AND table_name=:{}", bindings.len()));
    }
    sql.push_str(" ORDER BY table_name,constraint_name");
    let rows = metadata(session, &sql, &bindings).await?;
    let mut parents = BTreeMap::<(String, String), Columns>::new();
    let (mut cached, mut retained, mut count) = (0usize, 0usize, 0usize);
    let mut keys = Vec::new();
    for row in rows {
        let table_name = text(&row, 0)?;
        let name = text(&row, 1)?;
        let referenced_owner = text(&row, 2)?;
        let referenced_key = text(&row, 3)?;
        let local = columns(session, &login, owner, &name).await?;
        ensure!(local.table == table_name, "外键字段所属表已变化，请刷新");
        let parent = (referenced_owner.clone(), referenced_key);
        let reference = match parents.entry(parent) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let parent = entry.key();
                let columns = columns(session, &login, &parent.0, &parent.1).await?;
                cached +=
                    columns.retained_bytes() + parent.0.capacity() + parent.1.capacity() + 128;
                ensure!(cached <= BUDGET, "外键引用字段缓存超过 8 MiB");
                entry.insert(columns)
            }
        };
        ensure!(
            local.names.len() == reference.names.len(),
            "外键与引用字段数量不一致"
        );
        count += local.names.len();
        ensure!(count <= 10_000, "外键字段超过 10,000 项");
        let key = Key {
            value: ForeignKey {
                name,
                table: TableId {
                    schema: owner.into(),
                    name: table_name,
                },
                columns: local.names,
                referenced: TableId {
                    schema: referenced_owner,
                    name: reference.table.clone(),
                },
                referenced_columns: reference.names.clone(),
            },
            delete_rule: text(&row, 4)?,
        };
        retained += key.retained_bytes();
        let capacity = keys.capacity();
        keys.push(key);
        retained += (keys.capacity() - capacity) * std::mem::size_of::<Key>();
        ensure!(retained <= BUDGET, "外键目录超过 8 MiB，请缩小数据库范围");
    }
    Ok(keys)
}
