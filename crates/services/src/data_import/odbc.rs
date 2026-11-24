//! ODBC reads run only in the cancellable source process, with no saved credentials.
use super::*;
use odbc_api::{
    ConnectionOptions, Cursor, Environment, ResultSetMetadata,
    buffers::Indicator,
    parameter::{VarBinarySliceMut, VarCell, WideText},
};
fn connect<'a>(env: &'a Environment, source: &Odbc) -> Result<odbc_api::Connection<'a>> {
    let options = ConnectionOptions {
        login_timeout_sec: Some(10),
        ..Default::default()
    };
    let result = if source.connection_string.trim().is_empty() {
        env.connect(&source.dsn, &source.user, &source.password, options)
    } else {
        env.connect_with_connection_string(&source.connection_string, options)
    };
    // Some drivers echo their complete connection string in diagnostics.
    result.map_err(|_| anyhow::anyhow!("ODBC 连接失败，请检查数据源、驱动位数和登录信息"))
}
pub(super) fn sources(use_32bit: bool, directory: &Path) -> Result<Catalog> {
    #[cfg(not(windows))]
    let _ = directory;
    if use_32bit {
        #[cfg(windows)]
        return super::odbc32::sources(directory);
        #[cfg(not(windows))]
        anyhow::bail!("32 位 ODBC 桥接仅在 Windows 上可用");
    }
    let env = Environment::new()?;
    Ok(Catalog {
        dsns: env
            .data_sources()?
            .into_iter()
            .map(|s| s.server_name)
            .collect(),
        drivers: env.drivers()?.into_iter().map(|d| d.description).collect(),
        ..Default::default()
    })
}
pub(super) fn catalog(source: &Odbc, directory: &Path) -> Result<Catalog> {
    #[cfg(not(windows))]
    let _ = directory;
    if source.use_32bit {
        #[cfg(windows)]
        return super::odbc32::catalog(source, directory);
        #[cfg(not(windows))]
        anyhow::bail!("32 位 ODBC 桥接仅在 Windows 上可用");
    }
    let env = Environment::new()?;
    let connection = connect(&env, source)?;
    let mut out = Catalog::default();
    for row in connection.tables("", "", "%", "TABLE,VIEW")? {
        let row = row?;
        ensure!(
            out.odbc_tables.len() < 10_000,
            "源表超过 10000 张，请使用查询缩小范围"
        );
        out.odbc_tables.push(OdbcTable {
            catalog: row.catalog.as_str()?.unwrap_or_default().into(),
            schema: row.schema.as_str()?.unwrap_or_default().into(),
            name: row.table.as_str()?.unwrap_or_default().into(),
        });
    }
    Ok(out)
}
fn kind(t: &odbc_api::DataType) -> DataType {
    use odbc_api::DataType as T;
    match t {
        T::Binary { .. } | T::Varbinary { .. } | T::LongVarbinary { .. } => DataType::Binary,
        T::Bit => DataType::Boolean,
        T::TinyInt | T::SmallInt | T::Integer | T::BigInt => DataType::Integer,
        T::Real | T::Double | T::Float { .. } => DataType::Real,
        T::Numeric { precision, scale } | T::Decimal { precision, scale }
            if *precision <= 38 && *scale >= 0 && (*scale as usize) <= *precision =>
        {
            DataType::Decimal {
                precision: *precision as u16,
                scale: *scale as u16,
            }
        }
        T::Date => DataType::Date,
        T::Timestamp { .. } => DataType::DateTime,
        _ => DataType::Text,
    }
}
fn read_cell(row: &mut odbc_api::CursorRow<'_>, column: u16, binary: bool) -> Result<Cell> {
    if binary {
        let mut result = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let mut target =
                VarBinarySliceMut::from_buffer(buffer.as_mut_slice(), Indicator::NoTotal);
            row.get_data(column, &mut target)?;
            let Some(part) = target.as_slice() else {
                return Ok(Cell::Null);
            };
            ensure!(
                result.len() + part.len() <= MAX_ROW_BYTES,
                "ODBC 字段超过 16 MiB"
            );
            result.extend_from_slice(part);
            if target.is_complete() {
                break;
            }
            ensure!(!part.is_empty(), "ODBC 驱动返回了无效分块");
        }
        Ok(Cell::Binary(result))
    } else {
        let mut result = Vec::new();
        let mut buffer = [0u16; 4096];
        loop {
            let mut target =
                VarCell::<_, WideText>::from_buffer(buffer.as_mut_slice(), Indicator::NoTotal);
            row.get_data(column, &mut target)?;
            let Some(part) = target.as_slice() else {
                return Ok(Cell::Null);
            };
            ensure!(
                result.len() + part.len() <= MAX_ROW_BYTES / 2,
                "ODBC 字段超过 16 MiB"
            );
            result.extend_from_slice(part);
            if target.is_complete() {
                break;
            }
            ensure!(!part.is_empty(), "ODBC 驱动返回了无效分块");
        }
        Ok(Cell::Text(String::from_utf16(&result)?))
    }
}
pub(super) fn read(
    source: &Source,
    directory: &Path,
    output: &mut Catalog,
    control: &Control,
) -> Result<()> {
    if source.odbc.use_32bit {
        #[cfg(windows)]
        return super::odbc32::read(source, directory, output, control);
        #[cfg(not(windows))]
        anyhow::bail!("32 位 ODBC 桥接仅在 Windows 上可用");
    }
    let env = Environment::new()?;
    let connection = connect(&env, &source.odbc)?;
    let quote = connection.identifier_quote_char()?.unwrap_or('"');
    let quoted = |s: &str| -> Result<String> {
        ensure!(!s.is_empty() && !s.contains('\0'), "ODBC 对象名无效");
        let end = if quote == '[' { ']' } else { quote };
        Ok(format!(
            "{quote}{}{end}",
            s.replace(end, &format!("{end}{end}"))
        ))
    };
    let queries = if !source.odbc.query.trim().is_empty() {
        let sql = &source.odbc.query;
        ensure!(
            tomato_core::sql_script::is_select(Some(sql))
                && tomato_core::sql_script::split_for("SQLITE", Some(sql)).count() == 1,
            "ODBC 源查询必须是单条 SELECT"
        );
        vec![("query".to_owned(), sql.clone())]
    } else {
        ensure!(
            !source.odbc.tables.is_empty() && source.odbc.tables.len() <= 100,
            "请选择 1 至 100 张 ODBC 源表"
        );
        source
            .odbc
            .tables
            .iter()
            .map(|t| {
                let catalog = if t.catalog.contains([':', '\\', '/']) {
                    ""
                } else {
                    t.catalog.as_str()
                };
                let parts = [catalog, t.schema.as_str(), t.name.as_str()]
                    .into_iter()
                    .filter(|v| !v.is_empty())
                    .map(&quoted)
                    .collect::<Result<Vec<_>>>()?;
                Ok((t.name.clone(), format!("SELECT * FROM {}", parts.join("."))))
            })
            .collect::<Result<Vec<_>>>()?
    };
    for (name, sql) in queries {
        control.check()?;
        let mut cursor = connection
            .execute(&sql, (), Some(30))?
            .context("ODBC 源查询没有结果集")?;
        let count = cursor.num_result_cols()? as usize;
        ensure!(count > 0 && count <= MAX_COLUMNS, "ODBC 字段数无效");
        let names = cursor
            .column_names()?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut fields = Vec::new();
        for (i, name) in names.into_iter().enumerate() {
            let mut desc = odbc_api::ColumnDescription::default();
            cursor.describe_col((i + 1) as u16, &mut desc)?;
            fields.push(Field {
                name,
                data_type: kind(&desc.data_type),
                nullable: true,
            });
        }
        let binary = fields
            .iter()
            .map(|f| f.data_type == DataType::Binary)
            .collect::<Vec<_>>();
        let mut sink = Sink::new(directory, output, name, "ODBC".into(), fields)?.typed();
        let mut number = 0;
        while let Some(mut row) = cursor.next_row()? {
            control.check()?;
            number += 1;
            if !included(source, number) {
                continue;
            }
            let mut values = Vec::new();
            let mut size = 0usize;
            for (i, binary) in binary.iter().enumerate() {
                let cell = read_cell(&mut row, (i + 1) as u16, *binary)?;
                size = size.saturating_add(cell_bytes(&cell));
                ensure!(size <= MAX_ROW_BYTES, "ODBC 记录超过 16 MiB");
                values.push(cell);
            }
            sink.push(values, control)?;
        }
        sink.finish(output)?;
    }
    Ok(())
}
