//! Oracle TableStructureView projection. Owner identifiers never become a SID.
use super::super::oracle::catalog;
use super::*;
use anyhow::Context as _;
use tomato_core::table_design::{
    Column, Create, Options,
    alter::{InfoPage, InfoTable, Snapshot},
};

pub(super) async fn read(
    session: &mut AnyConnection,
    table: &TableId,
    include_ddl: bool,
) -> Result<Snapshot> {
    let bindings = catalog::binds(table);
    let (metadata, primary_name) = catalog::read_columns(session, table).await?;
    ensure!(
        !metadata.is_empty() && metadata.len() <= 4096,
        "表没有可见字段或字段超过 4,096 项"
    );
    let view = catalog::is_view(session, table).await?;
    let mut columns = Vec::with_capacity(metadata.len());
    let mut raw_columns = Vec::with_capacity(metadata.len());
    for row in metadata {
        let values = row.into_values();
        let value = |i: usize| values[i].clone();
        let data_type = value(1);
        let character = matches!(
            data_type.as_str(),
            "CHAR" | "VARCHAR2" | "NCHAR" | "NVARCHAR2"
        );
        let numeric = data_type == "NUMBER" || data_type == "FLOAT";
        let length = if character {
            value(13)
        } else if numeric {
            value(11)
        } else if data_type == "RAW" {
            value(10)
        } else {
            String::new()
        };
        let scale = if data_type == "NUMBER" || data_type.starts_with("TIMESTAMP") {
            value(12)
        } else {
            String::new()
        };
        // Preserve signatures of attributes which the field grid does not edit:
        // virtual/identity generation, type owner, byte/character semantics.
        raw_columns.push(serde_json::to_string(&values)?);
        columns.push(Column {
            name: value(0),
            computed: if value(5) == "YES" {
                Some(tomato_core::table_design::Computed {
                    expression: value(3),
                    stored: false,
                })
            } else {
                None
            },
            generation: match value(8).as_str() {
                "ALWAYS" => tomato_core::table_design::Generation::Always,
                "BY DEFAULT" => tomato_core::table_design::Generation::ByDefault,
                "BY DEFAULT ON NULL" => tomato_core::table_design::Generation::ByDefaultOnNull,
                _ => Default::default(),
            },
            default_expression: !value(3).trim().is_empty(),
            data_type: data_type.clone(),
            length,
            scale,
            not_null: value(2) == "N",
            primary_key: value(4).parse::<u32>()? > 0,
            auto_increment: value(6) == "1",
            default_value: if value(5) == "YES" {
                String::new()
            } else {
                value(3).trim().into()
            },
            comment: value(7),
            charset: if matches!(data_type.as_str(), "CHAR" | "VARCHAR2") {
                match value(14).as_str() {
                    "C" => "CHAR".into(),
                    "B" => "BYTE".into(),
                    _ => String::new(),
                }
            } else {
                String::new()
            },
            ..Default::default()
        });
    }
    let extra = workbench::metadata(session, "SELECT comments FROM all_tab_comments WHERE owner=:1 AND table_name=:2 AND table_type IN ('TABLE','VIEW')", &bindings).await?;
    let comment = extra
        .first()
        .map(|r| workbench::optional(r, 0))
        .transpose()?
        .flatten()
        .unwrap_or_default();
    // A baseline must describe the same object in both lightweight and full
    // reads. Keep identity/version guards even while the DDL itself is deferred.
    let versions = workbench::metadata(
        session,
        "SELECT object_id,TO_CHAR(last_ddl_time,'YYYYMMDDHH24MISS') FROM all_objects WHERE owner=:1 AND object_name=:2 AND object_type IN ('TABLE','VIEW') AND subobject_name IS NULL",
        &bindings,
    )
    .await?;
    let version = versions.first().context("表或视图已不存在")?;
    raw_columns.push(serde_json::to_string(&(
        workbench::text(version, 0)?,
        workbench::text(version, 1)?,
    ))?);
    let sql = if include_ddl {
        ddl(session, table, view).await?
    } else {
        String::new()
    };
    let snapshot = Snapshot {
        definition: Create {
            auxiliary: Vec::new(),
            indexes: if view {
                Vec::new()
            } else {
                super::designer_indexes::read(session, "ORACLE", table).await?
            },
            table: table.clone(),
            columns,
            options: Options::default(),
            comment,
        },
        extra_options: Default::default(),
        backslash_escapes: false,
        raw_columns,
        indexes: InfoTable::default(),
        foreign_keys: InfoTable::default(),
        triggers: InfoTable::default(),
        sql,
        view,
        primary_name,
    };
    struct Bounded(usize);
    impl std::io::Write for Bounded {
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
    serde_json::to_writer(Bounded(0), &snapshot)?;
    Ok(snapshot)
}

pub(super) async fn read_sql(session: &mut AnyConnection, table: &TableId) -> Result<String> {
    let view = catalog::is_view(session, table).await?;
    ddl(session, table, view).await
}
async fn ddl(session: &mut AnyConnection, table: &TableId, view: bool) -> Result<String> {
    let object = if view { "VIEW" } else { "TABLE" };
    let rows = workbench::metadata(
        session,
        &format!("SELECT DBMS_METADATA.GET_DDL('{object}',:1,:2) FROM dual"),
        &[
            tomato_core::workbench::Cell::Text(table.name.clone()),
            tomato_core::workbench::Cell::Text(table.schema.clone()),
        ],
    )
    .await?;
    let row = rows.first().context("服务器未返回原始对象定义")?;
    let sql = workbench::text(row, 0)?;
    ensure!(!sql.trim().is_empty(), "服务器返回了空的对象定义");
    ensure!(sql.len() <= BUDGET, "表定义预览超过 8 MiB");
    Ok(sql)
}
pub(super) async fn read_info(
    session: &mut AnyConnection,
    table: &TableId,
    page: InfoPage,
) -> Result<InfoTable> {
    if matches!(page, InfoPage::Indexes) {
        return Ok(super::designer_indexes::information(
            super::designer_indexes::read(session, "ORACLE", table).await?,
        ));
    }
    if page == InfoPage::ForeignKeys {
        return Ok(InfoTable {
            headers: tomato_core::table_design::auxiliary::headers(page),
            rows: catalog::read_foreign_keys(session, &table.schema, Some(&table.name))
                .await?
                .into_iter()
                .map(|key| key.into_row())
                .collect(),
            ..Default::default()
        });
    }
    let (sql, headers): (&str, &[&str]) = match page {
        InfoPage::Indexes => unreachable!(),
        InfoPage::ForeignKeys => unreachable!(),
        InfoPage::Triggers => (
            "SELECT trigger_name,CASE WHEN trigger_type LIKE '%BEFORE%' THEN 'BEFORE' WHEN trigger_type LIKE '%INSTEAD OF%' THEN 'INSTEAD OF' ELSE 'AFTER' END,triggering_event,trigger_body FROM all_triggers WHERE owner=:1 AND table_name=:2 ORDER BY trigger_name",
            &["名称", "时机", "事件", "语句"],
        ),
    };
    let rows = workbench::metadata(session, sql, &catalog::binds(table)).await?;
    Ok(InfoTable {
        signature: String::new(),
        index_definitions: Vec::new(),
        headers: headers.iter().map(|s| (*s).into()).collect(),
        rows: rows
            .iter()
            .map(|r| {
                (0..headers.len())
                    .map(|i| workbench::optional(r, i).map(|v| v.unwrap_or_default()))
                    .collect()
            })
            .collect::<Result<_>>()?,
    })
}
