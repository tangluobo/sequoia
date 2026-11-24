//! Per-object transforms avoid changing DBMS_METADATA's session defaults.
//! https://docs.oracle.com/en/database/oracle/oracle-database/19/arpls/DBMS_METADATA.html
use super::*;

pub(super) fn same_connection(source: &Connection, target: &Connection) -> bool {
    // GUI copies of the same Oracle connection can select different owners.
    // All actual connection settings must match before using one session.
    let session_option = |key: &str| key != "__tomatoOracleOwner";
    source.id == target.id
        && source
            .options
            .iter()
            .filter(|(key, _)| session_option(key))
            .eq(target.options.iter().filter(|(key, _)| session_option(key)))
}

pub(super) async fn copy_in_database(
    session: &mut AnyConnection,
    sql: &str,
    control: &Control,
) -> Result<u64> {
    let mut transaction = session.begin().await?;
    let result = tokio::select! {
        biased;
        () = control.wait_cancelled() => anyhow::bail!("操作已取消"),
        result = transaction.execute(sql) => result?,
    };
    // Once the insert finishes, wait for a definite commit result before
    // reporting its rows. An interrupted statement cannot commit implicitly.
    transaction
        .commit()
        .await
        .context("提交 Oracle 表复制失败，请核对目标表")?;
    let records = result.rows_affected();
    control.records(records);
    Ok(records)
}
pub(super) fn adapt(
    column: &mut tomato_core::table_design::Column,
    source: &str,
    target: &str,
) -> Result<()> {
    let mut kind = column.data_type.to_ascii_lowercase();
    if source == "ORACLE" && kind.starts_with("timestamp(") && kind.ends_with(')') {
        kind = "timestamp".into();
    }
    if source == "ORACLE" && kind == "timestamp" {
        ensure!(
            column.scale.parse::<u32>().unwrap_or(0) <= 6,
            "字段 {} 的时间精度超过目标数据库，不能自动截断",
            column.name
        );
    }
    let data_type = match (source, target, kind.as_str()) {
        (_, "ORACLE", "tinyint" | "smallint" | "int2") => {
            column.length = if column.unsigned { "5" } else { "6" }.into();
            column.scale = "0".into();
            "NUMBER"
        }
        (_, "ORACLE", "mediumint" | "int" | "integer" | "int4") => {
            column.length = "10".into();
            column.scale = "0".into();
            "NUMBER"
        }
        (_, "ORACLE", "bigint" | "int8") => {
            column.length = if column.unsigned { "20" } else { "19" }.into();
            column.scale = "0".into();
            "NUMBER"
        }
        (_, "ORACLE", "bool" | "boolean") => {
            column.length = "1".into();
            column.scale = "0".into();
            "NUMBER"
        }
        (_, "ORACLE", "decimal" | "numeric") => {
            ensure!(
                column
                    .length
                    .parse::<u32>()
                    .is_ok_and(|n| (1..=38).contains(&n)),
                "字段 {} 的数值精度不能无损映射到 Oracle NUMBER",
                column.name
            );
            "NUMBER"
        }
        (_, "ORACLE", "varchar" | "character varying") => {
            ensure!(
                column
                    .length
                    .parse::<u32>()
                    .is_ok_and(|n| (1..=4000).contains(&n)),
                "字段 {} 超过 Oracle VARCHAR2 的可移植长度",
                column.name
            );
            "VARCHAR2"
        }
        (_, "ORACLE", "char" | "bpchar") => {
            ensure!(
                column
                    .length
                    .parse::<u32>()
                    .is_ok_and(|n| (1..=2000).contains(&n)),
                "字段 {} 超过 Oracle CHAR 的可移植长度",
                column.name
            );
            "CHAR"
        }
        (_, "ORACLE", "text" | "tinytext" | "mediumtext" | "longtext" | "json" | "jsonb") => "CLOB",
        (
            _,
            "ORACLE",
            "bytea" | "blob" | "tinyblob" | "mediumblob" | "longblob" | "binary" | "varbinary",
        ) => "BLOB",
        (_, "ORACLE", "float4" | "real") => "BINARY_FLOAT",
        (_, "ORACLE", "float8" | "double precision" | "double" | "float") => "BINARY_DOUBLE",
        (_, "ORACLE", "date") => "DATE",
        (_, "ORACLE", "datetime" | "timestamp" | "timestamp without time zone") => "TIMESTAMP",
        ("ORACLE", _, "number") => "DECIMAL",
        ("ORACLE", "MYSQL", "varchar2" | "nvarchar2") => "VARCHAR",
        ("ORACLE", "POSTGRESQL", "varchar2" | "nvarchar2") => "varchar",
        ("ORACLE", _, "char" | "nchar") => "CHAR",
        ("ORACLE", "MYSQL", "clob" | "nclob" | "long") => "LONGTEXT",
        ("ORACLE", "POSTGRESQL", "clob" | "nclob" | "long") => "text",
        ("ORACLE", "MYSQL", "blob" | "raw" | "long raw") => "LONGBLOB",
        ("ORACLE", "POSTGRESQL", "blob" | "raw" | "long raw") => "bytea",
        ("ORACLE", "MYSQL", "binary_float") => "FLOAT",
        ("ORACLE", "MYSQL", "binary_double" | "float") => "DOUBLE",
        ("ORACLE", "POSTGRESQL", "binary_float") => "real",
        ("ORACLE", "POSTGRESQL", "binary_double" | "float") => "double precision",
        ("ORACLE", "MYSQL", "date" | "timestamp") => "DATETIME",
        ("ORACLE", "POSTGRESQL", "date" | "timestamp") => "timestamp",
        _ => anyhow::bail!(
            "字段 {} 的类型 {} 无法无损映射到目标数据库",
            column.name,
            column.data_type
        ),
    };
    if source == "ORACLE" && kind == "number" {
        ensure!(
            target == "POSTGRESQL" || !column.length.is_empty(),
            "字段 {} 未限定 NUMBER 精度，请先在 SQL 中显式转换",
            column.name
        );
        let scale = column.scale.parse::<i32>().unwrap_or(0);
        if scale < 0 {
            let precision = column
                .length
                .parse::<i32>()
                .context("Oracle NUMBER 精度无效")?;
            column.length = (precision - scale).to_string();
            column.scale = "0".into();
        }
        if target == "POSTGRESQL" {
            column.data_type = "numeric".into();
        } else {
            column.data_type = data_type.into();
        }
    } else {
        column.data_type = data_type.into();
    }
    if !matches!(
        data_type,
        "NUMBER" | "DECIMAL" | "VARCHAR2" | "VARCHAR" | "varchar" | "CHAR"
    ) {
        column.length.clear();
    }
    if !matches!(
        data_type,
        "NUMBER" | "DECIMAL" | "TIMESTAMP" | "timestamp" | "DATETIME"
    ) {
        column.scale.clear();
    }
    column.charset.clear();
    column.collation.clear();
    column.unsigned = false;
    column.zerofill = false;
    if column.auto_increment {
        column.default_value.clear();
    }
    Ok(())
}
pub(super) async fn create(
    session: &mut AnyConnection,
    source: &TableId,
    target: &TableId,
) -> Result<String> {
    super::super::oracle::metadata::ddl(session, "TABLE", source, target, true).await
}
