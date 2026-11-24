//! Bounded owner/table-scoped catalog reads, matching JDBC's separate metadata calls.
use super::{binds, metadata, optional, text};
use anyhow::{Result, ensure};
use sqlx::AnyConnection;
use tomato_core::workbench::{Cell, TableId};

pub(in crate::database) struct ColumnMetadata {
    pub name: String,
    pub data_type: String,
    pub nullable: String,
    pub default: Option<String>,
    pub primary_key: u32,
    pub virtual_column: String,
    pub identity: String,
    pub comment: String,
    pub type_owner: String,
    pub length: String,
    pub precision: String,
    pub scale: String,
    pub char_length: String,
    pub char_used: String,
}
impl ColumnMetadata {
    fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + [
                &self.name,
                &self.data_type,
                &self.nullable,
                &self.virtual_column,
                &self.identity,
                &self.comment,
                &self.type_owner,
                &self.length,
                &self.precision,
                &self.scale,
                &self.char_length,
                &self.char_used,
            ]
            .iter()
            .map(|s| s.capacity())
            .sum::<usize>()
            + self.default.as_ref().map_or(0, String::capacity)
    }
    pub(in crate::database) fn into_values(self) -> [String; 15] {
        let identity_flag = if self.identity.is_empty() { "0" } else { "1" }.into();
        [
            self.name,
            self.data_type,
            self.nullable,
            self.default.unwrap_or_default(),
            self.primary_key.to_string(),
            self.virtual_column,
            identity_flag,
            self.comment,
            self.identity,
            self.type_owner,
            self.length,
            self.precision,
            self.scale,
            self.char_length,
            self.char_used,
        ]
    }
}
const BASE: &str = "SELECT column_name,data_type,nullable,data_default,virtual_column,data_type_owner,data_length,data_precision,data_scale,char_length,char_used,CASE WHEN owner=SYS_CONTEXT('USERENV','SESSION_USER') THEN 'Y' ELSE 'N' END FROM all_tab_cols WHERE owner=:1 AND table_name=:2 AND hidden_column='NO' ORDER BY column_id";
const COMMENTS: &str =
    "SELECT column_name,comments FROM all_col_comments WHERE owner=:1 AND table_name=:2";
const PRIMARY_NAME: &str = "SELECT constraint_name FROM all_constraints WHERE owner=:1 AND table_name=:2 AND constraint_type='P'";
const PRIMARY_COLUMNS: &str = "SELECT column_name,position FROM all_cons_columns WHERE owner=:1 AND table_name=:2 AND constraint_name=:3";
const OWN_PRIMARY_NAME: &str =
    "SELECT constraint_name FROM user_constraints WHERE table_name=:1 AND constraint_type='P'";
const OWN_PRIMARY_COLUMNS: &str =
    "SELECT column_name,position FROM user_cons_columns WHERE table_name=:1 AND constraint_name=:2";
const IDENTITY: &str = "SELECT column_name,generation_type FROM all_tab_identity_cols WHERE owner=:1 AND table_name=:2";
#[derive(Clone, Copy)]
enum Attribute {
    Comment,
    Primary,
    Identity,
}

pub(in crate::database) async fn read_columns(
    session: &mut AnyConnection,
    table: &TableId,
) -> Result<(Vec<ColumnMetadata>, Option<String>)> {
    let bindings = binds(table);
    let rows = metadata(session, BASE, &bindings).await?;
    ensure!(
        !rows.is_empty() && rows.len() <= 4096,
        "表不存在、没有可见列或字段超过 4,096 项"
    );
    let own_table = text(&rows[0], 11)? == "Y";
    let mut columns = Vec::with_capacity(rows.len());
    for row in rows {
        columns.push(ColumnMetadata {
            name: text(&row, 0)?,
            data_type: text(&row, 1)?,
            nullable: text(&row, 2)?,
            default: optional(&row, 3)?,
            virtual_column: text(&row, 4)?,
            type_owner: text(&row, 5)?,
            length: text(&row, 6)?,
            precision: text(&row, 7)?,
            scale: text(&row, 8)?,
            char_length: text(&row, 9)?,
            char_used: text(&row, 10)?,
            primary_key: 0,
            identity: String::new(),
            comment: String::new(),
        });
    }
    let mut names: Vec<usize> = (0..columns.len()).collect();
    names.sort_unstable_by(|a, b| columns[*a].name.cmp(&columns[*b].name));
    let mut held = columns
        .iter()
        .map(ColumnMetadata::retained_bytes)
        .sum::<usize>()
        + names.capacity() * std::mem::size_of::<usize>();
    ensure!(
        held <= crate::database::limits::QUERY_BYTES,
        "结构元数据超过 8 MiB，请缩小数据库范围"
    );
    // Restrict each dictionary view before combining the results. Joining the
    // two expanded ALL_* views led to prolonged optimizer work on XE and Free.
    // A table owner can avoid Oracle's expanded grant checks. This decision
    // uses the authenticated session identity even after ALTER SESSION changes
    // CURRENT_SCHEMA; another owner's tables still use the authorized catalog.
    let (primary_name_sql, primary_columns_sql) = if own_table {
        (OWN_PRIMARY_NAME, OWN_PRIMARY_COLUMNS)
    } else {
        (PRIMARY_NAME, PRIMARY_COLUMNS)
    };
    let owner_bind = usize::from(own_table);
    let keys = metadata(session, primary_name_sql, &bindings[owner_bind..]).await?;
    let primary_name = keys.first().map(|row| text(row, 0)).transpose()?;
    for (kind, sql) in [
        (Attribute::Comment, COMMENTS),
        (Attribute::Primary, primary_columns_sql),
        (Attribute::Identity, IDENTITY),
    ] {
        let rows = if matches!(kind, Attribute::Primary) {
            let Some(name) = &primary_name else { continue };
            let key = [
                bindings[0].clone(),
                bindings[1].clone(),
                Cell::Text(name.clone()),
            ];
            metadata(session, sql, &key[owner_bind..]).await?
        } else {
            metadata(session, sql, &bindings).await?
        };
        for row in rows {
            let name = text(&row, 0)?;
            let Ok(position) = names.binary_search_by(|index| columns[*index].name.cmp(&name))
            else {
                continue;
            };
            let column = &mut columns[names[position]];
            let previous = column.retained_bytes();
            match kind {
                Attribute::Comment => column.comment = text(&row, 1)?,
                Attribute::Primary => column.primary_key = text(&row, 1)?.parse()?,
                Attribute::Identity => column.identity = text(&row, 1)?,
            }
            held = held - previous + column.retained_bytes();
            ensure!(
                held <= crate::database::limits::QUERY_BYTES,
                "结构元数据超过 8 MiB，请缩小数据库范围"
            );
        }
    }
    Ok((columns, primary_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_projection_keeps_existing_designer_signatures_and_full_long_defaults() {
        let expression = "'中文 O''Reilly' || ".repeat(500);
        let column = ColumnMetadata {
            name: "字段".into(),
            data_type: "NUMBER".into(),
            nullable: "N".into(),
            default: Some(expression.clone()),
            primary_key: 2,
            virtual_column: "NO".into(),
            identity: "BY DEFAULT ON NULL".into(),
            comment: "完整注释".into(),
            type_owner: "OWNER".into(),
            length: "22".into(),
            precision: "38".into(),
            scale: "-3".into(),
            char_length: "0".into(),
            char_used: String::new(),
        };
        let values = column.into_values();
        assert_eq!(
            values,
            [
                "字段",
                "NUMBER",
                "N",
                &expression,
                "2",
                "NO",
                "1",
                "完整注释",
                "BY DEFAULT ON NULL",
                "OWNER",
                "22",
                "38",
                "-3",
                "0",
                ""
            ]
        );
        assert!(
            values[3].len() > 4000,
            "must preserve the full LONG instead of DATA_DEFAULT_VC"
        );
    }
}
