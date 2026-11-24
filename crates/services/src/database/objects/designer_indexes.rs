//! Bounded index catalogs, with lossless signatures for unsupported server attributes.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use tomato_core::table_design::{
    Dialect,
    alter::InfoTable,
    indexes::{Field, Index, Kind},
};

pub(super) fn information(indexes: Vec<Index>) -> InfoTable {
    InfoTable {
        signature: String::new(),
        headers: ["名称", "字段", "类型", "方法", "注释"]
            .map(String::from)
            .to_vec(),
        rows: indexes
            .iter()
            .map(|i| {
                vec![
                    i.name.clone(),
                    i.fields_label(),
                    i.kind.label().into(),
                    i.method.clone(),
                    i.comment.clone(),
                ]
            })
            .collect(),
        index_definitions: indexes,
    }
}
pub(super) async fn read(
    session: &mut AnyConnection,
    kind: &str,
    table: &TableId,
) -> Result<Vec<Index>> {
    let rows = if kind == "ORACLE" {
        oracle_rows(session, table).await?
    } else {
        let sql = match kind {
            "MYSQL" => {
                "SELECT INDEX_NAME,COALESCE(COLUMN_NAME,''),CASE WHEN NON_UNIQUE=0 THEN 'UNIQUE' ELSE 'NORMAL' END,INDEX_TYPE,COALESCE(CAST(SUB_PART AS CHAR),''),COALESCE(COLLATION,''),COALESCE(INDEX_COMMENT,''),'' FROM information_schema.STATISTICS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? AND INDEX_NAME<>'PRIMARY' ORDER BY INDEX_NAME,SEQ_IN_INDEX"
            }
            "POSTGRESQL" => {
                "SELECT ic.relname::text,COALESCE(a.attname::text,''),CASE WHEN i.indisunique THEN 'UNIQUE' ELSE 'NORMAL' END,am.amname::text,'',CASE WHEN (i.indoption[k.pos-1]&1)=1 THEN 'D' ELSE 'A' END,COALESCE(obj_description(ic.oid),''),pg_get_indexdef(i.indexrelid),CASE WHEN EXISTS(SELECT 1 FROM pg_constraint x WHERE x.conindid=i.indexrelid) THEN '1' ELSE '0' END,CASE WHEN i.indpred IS NOT NULL OR i.indexprs IS NOT NULL OR i.indnatts<>COALESCE((to_jsonb(i)->>'indnkeyatts')::int,i.indnatts) OR ic.reloptions IS NOT NULL OR ic.reltablespace<>0 OR NOT i.indisvalid OR NOT i.indisready OR ic.relkind<>'i' OR ic.relispartition OR i.indisclustered OR i.indisreplident OR pg_get_indexdef(i.indexrelid,k.pos,true) <> quote_ident(a.attname) OR NOT op.opcdefault OR i.indcollation[k.pos-1]<>a.attcollation OR ((i.indoption[k.pos-1]&2)<>CASE WHEN (i.indoption[k.pos-1]&1)=1 THEN 2 ELSE 0 END) OR ia.attoptions IS NOT NULL THEN '1' ELSE '0' END FROM pg_index i JOIN pg_class c ON c.oid=i.indrelid JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_class ic ON ic.oid=i.indexrelid JOIN pg_am am ON am.oid=ic.relam CROSS JOIN LATERAL generate_series(1,i.indnatts) k(pos) LEFT JOIN pg_attribute a ON a.attrelid=c.oid AND a.attnum=i.indkey[k.pos-1] LEFT JOIN pg_opclass op ON op.oid=i.indclass[k.pos-1] LEFT JOIN pg_attribute ia ON ia.attrelid=ic.oid AND ia.attnum=k.pos WHERE n.nspname=$1 AND c.relname=$2 AND NOT i.indisprimary ORDER BY ic.relname,k.pos"
            }
            _ => bail!("当前数据库尚未实现索引设计"),
        };
        super::designer::rows(session, sql, table).await?
    };
    let mut indexes: Vec<Index> = Vec::new();
    let mut signatures: Vec<Vec<String>> = Vec::new();
    for row in rows {
        if indexes.last().is_none_or(|i| i.name != row[0]) {
            ensure!(indexes.len() < 1024, "索引超过 1,024 项");
            if let Some(previous) = indexes.last_mut() {
                previous.signature = serde_json::to_string(&signatures)?;
                signatures.clear();
            }
            indexes.push(Index {
                name: row[0].clone(),
                origin: Some(row[0].clone()),
                kind: if row[2] == "UNIQUE" {
                    Kind::Unique
                } else {
                    Kind::Normal
                },
                method: row[3].clone(),
                comment: row[6].clone(),
                constraint: row.get(8).is_some_and(|v| v == "1"),
                ..Default::default()
            });
        }
        let index = indexes.last_mut().unwrap();
        if kind == "MYSQL" {
            match index.method.as_str() {
                "FULLTEXT" => {
                    index.kind = Kind::Fulltext;
                    index.method.clear();
                }
                "SPATIAL" | "RTREE" => {
                    index.kind = Kind::Spatial;
                    index.method.clear();
                }
                _ => {}
            }
        } else if kind == "ORACLE" {
            index.method = match row[3].as_str() {
                "NORMAL" => "BTREE",
                "NORMAL/REV" => "REVERSE",
                "BITMAP" => "BITMAP",
                other => other,
            }
            .into();
        }
        if row[1].is_empty()
            || row.get(9).is_some_and(|v| v == "1")
            || row[7].contains("NULLS NOT DISTINCT")
        {
            index.opaque =
                "此索引含表达式、条件或其他特殊属性，可重命名；结构请通过 SQL 修改".into();
        }
        index.fields.push(Field {
            name: row[1].clone(),
            prefix: if row[4].is_empty() {
                None
            } else {
                Some(row[4].parse()?)
            },
            descending: row[5] == "D",
        });
        signatures.push(row);
    }
    if let Some(last) = indexes.last_mut() {
        last.signature = serde_json::to_string(&signatures)?;
    }
    if kind == "MYSQL" && !indexes.is_empty() {
        use sqlx::Row;
        let dialect = crate::dialect::resolve(kind)?;
        let raw = sqlx::query(&format!(
            "SHOW CREATE TABLE {}.{}",
            dialect.quote(&table.schema)?,
            dialect.quote(&table.name)?
        ))
        .fetch_one(&mut *session)
        .await?;
        let ddl: String = raw.try_get(1)?;
        let backslash = super::designer::string_mode(session, kind).await?;
        let tokens = tomato_core::table_design::mysql_definition::tokens(&ddl, backslash)?;
        let body = tokens
            .iter()
            .map(|t| &ddl[t.range.clone()])
            .find(|s| s.starts_with('('))
            .ok_or_else(|| anyhow::anyhow!("表定义缺少字段"))?;
        let body = &body[1..body.len() - 1];
        let tokens = tomato_core::table_design::mysql_definition::tokens(body, backslash)?;
        let mut start = 0;
        for end in tokens
            .iter()
            .filter(|t| &body[t.range.clone()] == ",")
            .map(|t| t.range.start)
            .chain(std::iter::once(body.len()))
        {
            let definition = body[start..end].trim();
            start = end + 1;
            let ts = tomato_core::table_design::mysql_definition::tokens(definition, backslash)?;
            let Some(key) = ts
                .iter()
                .position(|t| &definition[t.range.clone()] == "KEY")
            else {
                continue;
            };
            let Some(name) = ts.get(key + 1).map(|t| &definition[t.range.clone()]) else {
                continue;
            };
            let name = name
                .strip_prefix('`')
                .and_then(|s| s.strip_suffix('`'))
                .map(|s| s.replace("``", "`"));
            if let Some(index) = indexes.iter_mut().find(|i| Some(&i.name) == name.as_ref()) {
                index.signature.push_str(definition);
                if ts.iter().any(|t| {
                    let token = &definition[t.range.clone()];
                    !token.starts_with(['`', '\'', '('])
                        && ![
                            "UNIQUE", "FULLTEXT", "SPATIAL", "KEY", "USING", "BTREE", "HASH",
                            "COMMENT",
                        ]
                        .contains(&token)
                }) {
                    index.opaque =
                        "此索引含解析器、可见性或其他特殊属性，可重命名；结构请通过 SQL 修改"
                            .into();
                }
            }
        }
    }
    // Catalog methods unknown to this client are never silently rewritten.
    let dialect = Dialect::resolve(kind)?;
    for index in &mut indexes {
        if !tomato_core::table_design::indexes::methods(dialect, index.kind)
            .contains(&index.method.as_str())
        {
            index.opaque = "此索引使用自定义方法，可重命名；结构请通过 SQL 修改".into();
        }
    }
    Ok(indexes)
}

async fn oracle_rows(session: &mut AnyConnection, table: &TableId) -> Result<Vec<Vec<String>>> {
    let bindings = super::super::oracle::catalog::binds(table);
    let mut primary = BTreeSet::new();
    let mut constrained = BTreeSet::new();
    for row in workbench::metadata(
        session,
        "SELECT index_owner,index_name,constraint_type FROM all_constraints WHERE owner=:1 AND table_name=:2 AND constraint_type IN ('P','U') AND index_name IS NOT NULL",
        &bindings,
    ).await? {
        let key = (workbench::text(&row, 0)?, workbench::text(&row, 1)?);
        if workbench::text(&row, 2)? == "P" {
            primary.insert(key);
        } else {
            constrained.insert(key);
        }
    }
    let mut indexes = BTreeMap::new();
    let mut names = BTreeSet::new();
    for row in workbench::metadata(
        session,
        "SELECT owner,index_name,CASE WHEN uniqueness='UNIQUE' THEN 'UNIQUE' ELSE 'NORMAL' END,index_type,index_type || ':' || status || ':' || tablespace_name,CASE WHEN index_type NOT IN ('NORMAL','NORMAL/REV','BITMAP') OR partitioned='YES' OR compression<>'DISABLED' THEN '1' ELSE '0' END FROM all_indexes WHERE table_owner=:1 AND table_name=:2",
        &bindings,
    ).await? {
        let name = workbench::text(&row, 1)?;
        let key = (workbench::text(&row, 0)?, name.clone());
        if primary.contains(&key) {
            continue;
        }
        ensure!(indexes.len() < 1024, "索引超过 1,024 项");
        ensure!(names.insert(name), "Oracle 表包含来自不同所有者的同名索引");
        let values = [
            workbench::text(&row, 2)?,
            workbench::text(&row, 3)?,
            workbench::text(&row, 4)?,
            if constrained.contains(&key) { "1" } else { "0" }.into(),
            workbench::text(&row, 5)?,
        ];
        indexes.insert(key, values);
    }
    if indexes.is_empty() {
        return Ok(Vec::new());
    }
    let mut output = Vec::new();
    let mut bytes = 0usize;
    for row in workbench::metadata(
        session,
        "SELECT index_owner,index_name,column_name,descend FROM all_ind_columns WHERE table_owner=:1 AND table_name=:2 ORDER BY index_name,index_owner,column_position",
        &bindings,
    ).await? {
        let name = workbench::text(&row, 1)?;
        let key = (workbench::text(&row, 0)?, name.clone());
        let Some([kind, method, signature, constraint, opaque]) = indexes.get(&key) else {
            continue;
        };
        let values = vec![
            name,
            workbench::text(&row, 2)?,
            kind.clone(),
            method.clone(),
            String::new(),
            if workbench::text(&row, 3)? == "DESC" { "D" } else { "A" }.into(),
            String::new(),
            signature.clone(),
            constraint.clone(),
            opaque.clone(),
        ];
        bytes += std::mem::size_of::<Vec<String>>()
            + values.capacity() * std::mem::size_of::<String>()
            + values.iter().map(String::capacity).sum::<usize>();
        ensure!(bytes <= BUDGET, "Oracle 索引结构元数据超过 8 MiB");
        output.push(values);
    }
    Ok(output)
}
