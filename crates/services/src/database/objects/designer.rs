//! Complete designer I/O on the existing actor. No network or SQL work in views.
use super::*;
use anyhow::Context;
use sqlx::{Executor, Row};
use tomato_core::table_design::{
    Column, Create, Dialect, Options,
    alter::{Edit, InfoPage, InfoTable, Snapshot},
    mysql_definition,
};

pub(super) async fn rows(
    session: &mut AnyConnection,
    sql: &str,
    table: &TableId,
) -> Result<Vec<Vec<String>>> {
    let mut stream = sqlx::query(sql)
        .bind(&table.schema)
        .bind(&table.name)
        .fetch(session);
    let mut output = Vec::new();
    let mut bytes = 0usize;
    while let Some(row) = stream.try_next().await? {
        ensure!(output.len() < 10_000, "结构元数据超过 10,000 行");
        let mut cells = Vec::with_capacity(row.len());
        for index in 0..row.len() {
            let value = workbench::optional(&row, index)?.unwrap_or_default();
            bytes = bytes.saturating_add(value.capacity() + std::mem::size_of::<String>());
            ensure!(bytes <= BUDGET, "结构元数据超过 8 MiB");
            cells.push(value);
        }
        output.push(cells);
    }
    Ok(output)
}
async fn info(
    session: &mut AnyConnection,
    sql: &str,
    table: &TableId,
    headers: &[&str],
) -> Result<InfoTable> {
    Ok(InfoTable {
        signature: String::new(),
        index_definitions: Vec::new(),
        headers: headers.iter().map(|s| (*s).into()).collect(),
        rows: rows(session, sql, table).await?,
    })
}
pub(in crate::database) async fn read(
    session: &mut AnyConnection,
    kind: &str,
    table: &TableId,
) -> Result<Snapshot> {
    read_mode(session, kind, table, Detail::WithSql).await
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::database) enum Detail {
    Structure,
    WithSql,
}
pub(in crate::database) async fn read_structure(
    session: &mut AnyConnection,
    kind: &str,
    table: &TableId,
) -> Result<Snapshot> {
    read_mode(session, kind, table, Detail::Structure).await
}
pub(in crate::database) async fn read_sql(
    session: &mut AnyConnection,
    kind: &str,
    table: &TableId,
) -> Result<String> {
    if kind == "ORACLE" {
        return super::oracle_designer::read_sql(session, table).await;
    }
    let sql = read(session, kind, table).await?.sql;
    ensure!(!sql.trim().is_empty(), "服务器返回了空的对象定义");
    Ok(sql)
}
async fn read_mode(
    session: &mut AnyConnection,
    kind: &str,
    table: &TableId,
    detail: Detail,
) -> Result<Snapshot> {
    if kind == "ORACLE" {
        return super::oracle_designer::read(session, table, detail == Detail::WithSql).await;
    }
    if kind == "HIVE" {
        return if detail == Detail::WithSql {
            super::super::hive::designer::read(session, table).await
        } else {
            super::super::hive::designer::read_mode(session, table, false).await
        };
    }
    let dialect = Dialect::resolve(kind)?;
    ensure!(
        matches!(kind, "MYSQL" | "POSTGRESQL"),
        "当前数据库尚未实现表设计器读取"
    );
    let backslash_escapes = string_mode(session, kind).await?;
    let described = workbench::describe(session, kind, table).await?;
    let column_sql = if kind == "MYSQL" {
        "SELECT COLUMN_NAME,UPPER(DATA_TYPE),COALESCE(CAST(CHARACTER_MAXIMUM_LENGTH AS CHAR),CAST(NUMERIC_PRECISION AS CHAR),CAST(DATETIME_PRECISION AS CHAR),''),COALESCE(CAST(NUMERIC_SCALE AS CHAR),CAST(DATETIME_PRECISION AS CHAR),''),IS_NULLABLE,COLUMN_KEY,EXTRA,COALESCE(COLUMN_DEFAULT,''),COLUMN_COMMENT,COLUMN_TYPE,COALESCE(GENERATION_EXPRESSION,'') FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY ORDINAL_POSITION"
    } else {
        "SELECT a.attname::text,CASE WHEN t.typname IN ('varchar','bpchar','numeric','timestamp','timestamptz','time','timetz') THEN t.typname::text ELSE format_type(a.atttypid,a.atttypmod) END,CASE WHEN t.typname IN ('varchar','bpchar') AND a.atttypmod>4 THEN (a.atttypmod-4)::text WHEN t.typname='numeric' AND a.atttypmod>=4 THEN (((a.atttypmod-4)>>16)&65535)::text ELSE '' END,CASE WHEN t.typname='numeric' AND a.atttypmod>=4 THEN ((((a.atttypmod-4)&2047)#1024)-1024)::text WHEN t.typname IN ('timestamp','timestamptz','time','timetz') AND a.atttypmod>=0 THEN a.atttypmod::text ELSE '' END,CASE WHEN a.attnotnull THEN 'NO' ELSE 'YES' END,CASE WHEN EXISTS(SELECT 1 FROM pg_index i WHERE i.indrelid=c.oid AND i.indisprimary AND a.attnum=ANY(i.indkey)) THEN 'PRI' ELSE '' END,a.attidentity::text,COALESCE(pg_get_expr(d.adbin,d.adrelid),''),COALESCE(col_description(c.oid,a.attnum),''),format_type(a.atttypid,a.atttypmod),a.attgenerated::text FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_type t ON t.oid=a.atttypid LEFT JOIN pg_attrdef d ON d.adrelid=c.oid AND d.adnum=a.attnum WHERE n.nspname=$1 AND c.relname=$2 AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum"
    };
    let columns = match rows(session, column_sql, table).await {
        Ok(rows) => rows,
        Err(error)
            if error
                .downcast_ref::<sqlx::Error>()
                .and_then(|e| e.as_database_error())
                .and_then(|e| e.code())
                .is_some_and(|code| matches!(code.as_ref(), "42703" | "1054")) =>
        {
            let fallback = column_sql
                .replace("COALESCE(GENERATION_EXPRESSION,'')", "''")
                .replace("a.attgenerated::text", "''");
            rows(session, &fallback, table).await?
        }
        Err(error) => return Err(error),
    };
    ensure!(columns.len() <= 4096, "字段超过 4,096 项");
    let mut definition = Create {
        auxiliary: Vec::new(),
        indexes: Vec::new(),
        table: table.clone(),
        columns: Vec::with_capacity(columns.len()),
        options: Options::default(),
        comment: String::new(),
    };
    for row in columns {
        let data_type = if row[1] == "bpchar" {
            "char".into()
        } else {
            row[1].clone()
        };
        definition.columns.push(Column {
            name: row[0].clone(),
            computed: if kind == "POSTGRESQL" && !row[10].is_empty() {
                Some(tomato_core::table_design::Computed {
                    expression: row[7].clone(),
                    stored: row[10] == "s",
                })
            } else if kind == "MYSQL" && !row[10].is_empty() {
                Some(tomato_core::table_design::Computed {
                    expression: row[10].clone(),
                    stored: !row[6].contains("VIRTUAL"),
                })
            } else {
                None
            },
            generation: if kind == "POSTGRESQL" {
                match row[6].as_str() {
                    "a" => tomato_core::table_design::Generation::Always,
                    "d" => tomato_core::table_design::Generation::ByDefault,
                    _ if row[7].starts_with("nextval(") => {
                        tomato_core::table_design::Generation::Serial
                    }
                    _ => Default::default(),
                }
            } else {
                Default::default()
            },
            default_expression: if kind == "POSTGRESQL" {
                !row[7].is_empty()
            } else {
                row[6].to_ascii_uppercase().contains("DEFAULT_GENERATED")
            },
            data_type,
            length: row[2].clone(),
            scale: row[3].clone(),
            not_null: row[4] == "NO",
            primary_key: row[5] == "PRI",
            auto_increment: if kind == "MYSQL" {
                row[6].contains("auto_increment")
            } else {
                !row[6].is_empty() || row[7].starts_with("nextval(")
            },
            default_value: if kind == "POSTGRESQL" && !row[10].is_empty() {
                String::new()
            } else {
                row[7].clone()
            },
            comment: row[8].clone(),
            unsigned: kind == "MYSQL" && row[9].contains("unsigned"),
            zerofill: kind == "MYSQL" && row[9].contains("zerofill"),
            ..Default::default()
        });
    }
    let options_sql = if kind == "MYSQL" {
        "SELECT COALESCE(ENGINE,''),COALESCE(SUBSTRING_INDEX(TABLE_COLLATION,'_',1),''),COALESCE(TABLE_COLLATION,''),COALESCE(TABLE_COMMENT,''),COALESCE(CAST(AUTO_INCREMENT AS CHAR),''),COALESCE(ROW_FORMAT,''),COALESCE(CREATE_OPTIONS,'') FROM information_schema.TABLES WHERE TABLE_SCHEMA=? AND TABLE_NAME=?"
    } else {
        "SELECT '','','',COALESCE(obj_description(c.oid),'') FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2"
    };
    let mut extra_options = Default::default();
    if let Some(row) = rows(session, options_sql, table).await?.first() {
        definition.options = Options {
            engine: row[0].clone(),
            charset: row[1].clone(),
            collation: row[2].clone(),
            ..Default::default()
        };
        definition.comment = row[3].clone();
        if kind == "MYSQL" {
            let avg = row[6]
                .split_whitespace()
                .find_map(|s| s.strip_prefix("avg_row_length="))
                .unwrap_or("0");
            definition.options.auto_increment = row[4].clone();
            definition.options.row_format = row[5].clone();
            definition.options.avg_row_length = avg.into();
            extra_options = [row[4].clone(), row[5].clone(), avg.into()];
        }
    }
    if !described.view {
        definition.indexes = super::designer_indexes::read(session, kind, table).await?;
    }
    let mut raw_columns = Vec::new();
    let mut sql;
    if described.view && detail == Detail::Structure {
        sql = String::new();
    } else if kind == "MYSQL" {
        let qualified = format!(
            "{}.{}",
            crate::dialect::resolve(kind)?.quote(&table.schema)?,
            crate::dialect::resolve(kind)?.quote(&table.name)?
        );
        let row = sqlx::query(&format!(
            "SHOW CREATE {} {qualified}",
            if described.view { "VIEW" } else { "TABLE" }
        ))
        .fetch_one(&mut *session)
        .await?;
        sql = workbench::text(&row, 1)?;
        ensure!(sql.len() <= BUDGET, "表定义超过 8 MiB");
        if !described.view {
            definition.options.auto_increment =
                mysql_definition::auto_increment(&sql, backslash_escapes)?.unwrap_or_else(|| {
                    if definition
                        .columns
                        .iter()
                        .any(|column| column.auto_increment)
                    {
                        "1".into()
                    } else {
                        String::new()
                    }
                });
            extra_options[0] = definition.options.auto_increment.clone();
            let parsed = mysql_definition::columns(&sql, backslash_escapes)?;
            for column in &definition.columns {
                raw_columns.push(
                    parsed
                        .iter()
                        .find(|(name, _)| name == &column.name)
                        .ok_or_else(|| anyhow::anyhow!("无法识别原始列 {}", column.name))?
                        .1
                        .clone(),
                );
            }
        }
    } else if detail == Detail::Structure {
        sql = String::new();
    } else if described.view {
        let values=rows(session,"SELECT pg_get_viewdef(c.oid,true) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2",table).await?;
        let body = values
            .first()
            .and_then(|row| row.first())
            .ok_or_else(|| anyhow::anyhow!("视图定义未找到"))?;
        sql = format!(
            "CREATE VIEW {}.{} AS\n{body}",
            crate::dialect::resolve(kind)?.quote(&table.schema)?,
            crate::dialect::resolve(kind)?.quote(&table.name)?
        );
    } else {
        // Snapshot SQL is display-only. Keep unsupported index definitions out of
        // the executable CREATE model, and show the server's canonical DDL here.
        let indexes = std::mem::take(&mut definition.indexes);
        let create = definition.sql_for_session(dialect, backslash_escapes);
        definition.indexes = indexes;
        let mut preview = create?;
        for index in &definition.indexes {
            let signature: Vec<Vec<String>> = serde_json::from_str(&index.signature)?;
            let ddl = signature
                .first()
                .and_then(|row| row.get(7))
                .ok_or_else(|| anyhow::anyhow!("索引预览定义缺失"))?;
            ensure!(
                preview.len() + ddl.len() + index.comment.len() + 128 <= BUDGET,
                "表定义预览超过 8 MiB"
            );
            preview.push('\n');
            preview.push_str(ddl);
            preview.push(';');
            if !index.comment.is_empty() {
                preview.push('\n');
                preview.push_str(&index.comment_sql(dialect, table, backslash_escapes)?);
                preview.push(';');
            }
        }
        sql = preview;
    }
    let primary_name = if kind == "MYSQL" {
        definition
            .columns
            .iter()
            .any(|column| column.primary_key)
            .then(|| "PRIMARY".into())
    } else {
        rows(session,"SELECT con.conname::text FROM pg_constraint con JOIN pg_class c ON c.oid=con.conrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 AND con.contype='p'",table).await?.first().and_then(|row|row.first()).cloned()
    };
    if detail == Detail::Structure {
        // MySQL's canonical definition is still needed for lossless column
        // signatures and counters, but it need not stay resident in the GUI.
        sql = String::new();
    }
    let snapshot = Snapshot {
        definition,
        extra_options,
        backslash_escapes,
        raw_columns,
        indexes: InfoTable::default(),
        foreign_keys: InfoTable::default(),
        triggers: InfoTable::default(),
        sql,
        view: described.view,
        primary_name,
    };
    struct Size(usize);
    impl std::io::Write for Size {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            if self.0 > BUDGET {
                return Err(std::io::Error::other("表设计快照超过 8 MiB"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Size(0), &snapshot)?;
    Ok(snapshot)
}

pub(in crate::database) async fn read_for_transfer(
    session: &mut AnyConnection,
    kind: &str,
    table: &TableId,
) -> Result<Snapshot> {
    // Oracle cross-connection CREATE requests its remapped DDL separately.
    // The other Oracle transfer modes only compare the editable catalog.
    if kind == "ORACLE" {
        return super::oracle_designer::read(session, table, false).await;
    }
    read(session, kind, table).await
}
pub(in crate::database) async fn read_info(
    session: &mut AnyConnection,
    kind: &str,
    table: &TableId,
    page: InfoPage,
) -> Result<InfoTable> {
    if kind == "ORACLE" {
        return super::oracle_designer::read_info(session, table, page).await;
    }
    if kind == "HIVE" {
        return Ok(InfoTable::default());
    }
    ensure!(
        matches!(kind, "MYSQL" | "POSTGRESQL"),
        "当前数据库尚未实现结构元数据读取"
    );
    let mut result = match page {
        InfoPage::Indexes => Ok(super::designer_indexes::information(
            super::designer_indexes::read(session, kind, table).await?,
        )),
        InfoPage::ForeignKeys => super::designer_foreign_keys::read(session, kind, table).await,
        InfoPage::Triggers => {
            let triggers_sql = if kind == "MYSQL" {
                "SELECT TRIGGER_NAME,ACTION_TIMING,EVENT_MANIPULATION,ACTION_STATEMENT FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA=? AND EVENT_OBJECT_TABLE=? ORDER BY TRIGGER_NAME"
            } else {
                "SELECT t.tgname::text,CASE WHEN (t.tgtype & 2)>0 THEN 'BEFORE' WHEN (t.tgtype & 64)>0 THEN 'INSTEAD OF' ELSE 'AFTER' END,concat_ws(' OR ',CASE WHEN (t.tgtype & 4)>0 THEN 'INSERT' END,CASE WHEN (t.tgtype & 8)>0 THEN 'DELETE' END,CASE WHEN (t.tgtype & 16)>0 THEN 'UPDATE' END,CASE WHEN (t.tgtype & 32)>0 THEN 'TRUNCATE' END),pg_get_triggerdef(t.oid) FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 AND NOT t.tgisinternal ORDER BY t.tgname"
            };
            info(
                session,
                triggers_sql,
                table,
                &["名称", "时机", "事件", "语句"],
            )
            .await
        }
    }?;
    if let Some(sql) = match (kind, page) {
        ("POSTGRESQL", InfoPage::ForeignKeys) => Some(
            "SELECT con.oid::text,pg_get_constraintdef(con.oid,true),con.convalidated::text,con.condeferrable::text,con.condeferred::text FROM pg_constraint con JOIN pg_class c ON c.oid=con.conrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 AND con.contype='f' ORDER BY con.conname",
        ),
        ("POSTGRESQL", InfoPage::Triggers) => Some(
            "SELECT t.oid::text,pg_get_triggerdef(t.oid),t.tgenabled::text FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 AND NOT t.tgisinternal ORDER BY t.tgname",
        ),
        ("MYSQL", InfoPage::ForeignKeys) => Some(
            "SELECT r.CONSTRAINT_NAME,r.UNIQUE_CONSTRAINT_SCHEMA,r.UNIQUE_CONSTRAINT_NAME,r.MATCH_OPTION,r.DELETE_RULE,r.UPDATE_RULE,k.REFERENCED_TABLE_SCHEMA,k.REFERENCED_TABLE_NAME,k.COLUMN_NAME,k.REFERENCED_COLUMN_NAME,CAST(k.ORDINAL_POSITION AS CHAR) FROM information_schema.REFERENTIAL_CONSTRAINTS r JOIN information_schema.KEY_COLUMN_USAGE k ON k.CONSTRAINT_SCHEMA=r.CONSTRAINT_SCHEMA AND k.TABLE_NAME=r.TABLE_NAME AND k.CONSTRAINT_NAME=r.CONSTRAINT_NAME WHERE r.CONSTRAINT_SCHEMA=? AND r.TABLE_NAME=? ORDER BY r.CONSTRAINT_NAME,k.ORDINAL_POSITION",
        ),
        ("MYSQL", InfoPage::Triggers) => Some(
            "SELECT TRIGGER_NAME,ACTION_STATEMENT,DEFINER,SQL_MODE,CHARACTER_SET_CLIENT,COLLATION_CONNECTION,DATABASE_COLLATION,CAST(ACTION_ORDER AS CHAR),COALESCE(CAST(CREATED AS CHAR),'') FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA=? AND EVENT_OBJECT_TABLE=? ORDER BY TRIGGER_NAME",
        ),
        _ => None,
    } {
        use sha1::{Digest, Sha1};
        let mut hash = Sha1::new();
        for row in rows(session, sql, table).await? {
            hash.update((row.len() as u64).to_le_bytes());
            for cell in row {
                hash.update((cell.len() as u64).to_le_bytes());
                hash.update(cell.as_bytes());
            }
        }
        result.signature = format!("{:x}", hash.finalize());
    }
    Ok(result)
}
pub(super) async fn string_mode(session: &mut AnyConnection, kind: &str) -> Result<bool> {
    if matches!(kind, "ORACLE" | "HIVE") {
        return Ok(false);
    }
    let sql = if kind == "MYSQL" {
        "SELECT @@SESSION.sql_mode"
    } else {
        "SHOW standard_conforming_strings"
    };
    let row = sqlx::query(sql).fetch_one(session).await?;
    let value = workbench::text(&row, 0)?;
    Ok(if kind == "MYSQL" {
        !value
            .split(',')
            .any(|s| s.eq_ignore_ascii_case("NO_BACKSLASH_ESCAPES"))
    } else {
        value.eq_ignore_ascii_case("off")
    })
}
pub(in crate::database) async fn save(
    session: &mut AnyConnection,
    kind: &str,
    edit: Edit,
    detail: Detail,
) -> Result<Snapshot> {
    let backslash = string_mode(session, kind).await?;
    let plan = edit.plan(Dialect::resolve(kind)?, backslash)?;
    let table = edit.after.table.clone();
    let current = read_structure(session, kind, &table).await?;
    ensure!(
        {
            let mut expected = edit.before.definition.clone();
            let check_counter =
                edit.after.options.auto_increment != expected.options.auto_increment;
            let options_match = current
                .definition
                .options
                .same_schema(&expected.options, check_counter);
            expected.options = current.definition.options.clone();
            options_match && current.definition == expected
        } && current.raw_columns == edit.before.raw_columns
            && current.primary_name == edit.before.primary_name,
        "表结构已被其他操作修改，请刷新后重试；当前草稿已保留"
    );
    for change in &edit.after.auxiliary {
        let current_info = read_info(session, kind, &table, change.page).await?;
        ensure!(
            current_info.rows == change.before
                && (change.before_signature.is_empty()
                    || current_info.signature == change.before_signature),
            "外键或触发器已被其他操作修改，请刷新后重试；当前草稿已保留"
        );
    }
    if plan.statements.is_empty() {
        return if detail == Detail::WithSql {
            read(session, kind, &table).await
        } else {
            Ok(current)
        };
    }
    let point = workbench::savepoint_name(kind, "design");
    let transaction = workbench::existing_savepoint(session, kind, &point).await?;
    if transaction && matches!(kind, "MYSQL" | "ORACLE") {
        workbench::release_savepoint(session, kind, &point).await?;
        bail!("当前存在未完成事务，请先提交或回滚再保存表结构");
    }
    let own = kind == "POSTGRESQL" && !transaction;
    if own {
        session.execute("BEGIN").await?;
    }
    let outcome: Result<()> = async {
        for statement in &plan.statements {
            session.execute(statement.as_str()).await?;
        }
        Ok(())
    }
    .await;
    if transaction {
        if outcome.is_err() {
            session
                .execute(format!("ROLLBACK TO SAVEPOINT {point}").as_str())
                .await?;
        }
        session
            .execute(format!("RELEASE SAVEPOINT {point}").as_str())
            .await?;
    } else if own {
        if outcome.is_ok() {
            if let Err(error) = session.execute("COMMIT").await {
                let _ = session.execute("ROLLBACK").await;
                return Err(error.into());
            }
        } else {
            session.execute("ROLLBACK").await?;
        }
    }
    outcome.with_context(|| {
        if matches!(kind, "ORACLE" | "HIVE" | "MYSQL") {
            "DDL 可能已部分提交，请刷新核对数据库实际结构；当前草稿已保留"
        } else {
            "保存表结构失败"
        }
    })?;
    let mut saved = read_mode(session, kind, &table, detail)
        .await
        .context("表结构已保存，但重新读取失败，请刷新查看数据库实际状态")?;
    for change in &edit.after.auxiliary {
        let info = read_info(session, kind, &table, change.page)
            .await
            .context("结构已保存，但外键或触发器重新读取失败，请刷新核对")?;
        match change.page {
            InfoPage::ForeignKeys => saved.foreign_keys = info,
            InfoPage::Triggers => saved.triggers = info,
            InfoPage::Indexes => {}
        }
    }
    Ok(saved)
}
