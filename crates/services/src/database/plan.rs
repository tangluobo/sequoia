//! Parameterized row-write planning. No user values become executable SQL text.
use anyhow::{Context, Result, bail};
use tomato_core::workbench::{Cell, Column, RowChange, TableDefinition, TableId};

pub(in crate::database) struct Statement {
    pub sql: String,
    pub params: Vec<Cell>,
}

pub(in crate::database) fn qualified(kind: &str, table: &TableId) -> Result<String> {
    let dialect = crate::dialect::resolve(kind)?;
    if table.schema.is_empty() {
        dialect.quote(&table.name)
    } else {
        Ok(format!(
            "{}.{}",
            dialect.quote(&table.schema)?,
            dialect.quote(&table.name)?
        ))
    }
}

pub(in crate::database) fn placeholder(kind: &str, index: usize) -> String {
    if kind == "POSTGRESQL" {
        format!("${index}")
    } else if kind == "ORACLE" {
        format!(":{index}")
    } else {
        "?".into()
    }
}

pub(in crate::database) fn argument(
    kind: &str,
    column: &Column,
    cell: &Cell,
    params: &mut Vec<Cell>,
) -> Result<String> {
    if matches!(cell, Cell::Real(value) if !value.is_finite()) {
        bail!("数值必须为有限数");
    }
    if *cell == Cell::Null {
        return Ok("NULL".into());
    }
    if kind == "ORACLE"
        && column.data_type == "BLOB"
        && matches!(cell,Cell::Binary(bytes) if bytes.is_empty())
    {
        return Ok("EMPTY_BLOB()".into());
    }
    if kind == "ORACLE"
        && matches!(column.data_type.as_str(), "CLOB" | "NCLOB")
        && matches!(cell,Cell::Text(text) if text.is_empty())
    {
        return Ok("EMPTY_CLOB()".into());
    }
    if kind == "MYSQL" && mysql_bit(column) {
        params.push(mysql_bit_value(column, cell)?);
    } else {
        params.push(cell.clone());
    }
    let parameter = placeholder(kind, params.len());
    if kind == "MYSQL" && mysql_bit(column) {
        // Force integer comparison as well as integer assignment, so adjacent
        // BIT(64) values above 2^53 never compare through floating point.
        // Fix the prepared parameter's type as text first. Otherwise MySQL can
        // infer a decimal parameter that rejects valid 20-digit u64 values.
        Ok(format!("CAST(CAST({parameter} AS CHAR) AS UNSIGNED)"))
    } else if kind == "POSTGRESQL" && matches!(cell, Cell::Text(_)) {
        let cast = qualified(
            kind,
            &TableId {
                schema: column.type_schema.clone(),
                name: column.type_name.clone(),
            },
        )?;
        Ok(format!("CAST({parameter} AS {cast})"))
    } else if kind == "ORACLE" && matches!(cell, Cell::Text(_)) {
        Ok(oracle_parameter(column, &parameter))
    } else {
        Ok(parameter)
    }
}
pub(in crate::database) fn oracle_parameter(column: &Column, parameter: &str) -> String {
    let kind = column.data_type.to_ascii_uppercase();
    let base = kind.split('(').next().unwrap_or(&kind).trim();
    if base == "DATE" || base.starts_with("TIMESTAMP") {
        let zone = kind.contains("WITH TIME ZONE") && !kind.contains("LOCAL");
        format!(
            "{}({parameter},'YYYY-MM-DD HH24:MI:SS.FF{}')",
            if zone {
                "TO_TIMESTAMP_TZ"
            } else {
                "TO_TIMESTAMP"
            },
            if zone { " TZH:TZM" } else { "" }
        )
    } else {
        parameter.into()
    }
}

pub(in crate::database) fn mysql_bit(column: &Column) -> bool {
    column
        .data_type
        .trim()
        .split(['(', ' '])
        .next()
        .is_some_and(|kind| kind.eq_ignore_ascii_case("bit"))
}

/// MySQL binds BIT as an unsigned number, not as a BLOB parameter. Decimal text
/// also preserves values above i64::MAX without floating-point conversion.
pub(in crate::database) fn mysql_bit_value(column: &Column, cell: &Cell) -> Result<Cell> {
    let value = match cell {
        Cell::Null => return Ok(Cell::Null),
        Cell::Binary(bytes) => {
            anyhow::ensure!(
                !bytes.is_empty() && bytes.len() <= 8,
                "BIT 字段需要 1 至 8 个字节"
            );
            bytes
                .iter()
                .fold(0u64, |value, byte| (value << 8) | u64::from(*byte))
        }
        Cell::Text(text) => text
            .parse::<u64>()
            .context("BIT 值必须为无符号十进制整数")?,
        Cell::Integer(value) => u64::try_from(*value).context("BIT 值不能为负数")?,
        Cell::Boolean(value) => u64::from(*value),
        Cell::Real(_) => bail!("BIT 字段只接受无符号整数"),
    };
    mysql_bit_number(column, value)?;
    Ok(Cell::Text(value.to_string()))
}

fn mysql_bit_number(column: &Column, value: u64) -> Result<()> {
    let width = match column.data_type.split_once('(') {
        Some((_, width)) => width
            .trim_end()
            .strip_suffix(')')
            .and_then(|width| width.trim().parse::<u32>().ok()),
        None => Some(1),
    }
    .filter(|width| (1..=64).contains(width))
    .ok_or_else(|| anyhow::anyhow!("BIT 字段宽度无效"))?;
    anyhow::ensure!(
        width == 64 || value < (1u64 << width),
        "BIT 字段 {} 的值超过 {width} 位",
        column.name
    );
    Ok(())
}

fn predicate(
    kind: &str,
    definition: &TableDefinition,
    original: &[Cell],
    params: &mut Vec<Cell>,
    primary_only: bool,
) -> Result<String> {
    if original.len() != definition.columns.len() {
        bail!("原始行的列数与表结构不一致，请刷新后重试");
    }
    let dialect = crate::dialect::resolve(kind)?;
    let mut conditions = Vec::new();
    for (column, value) in definition.columns.iter().zip(original) {
        if primary_only && column.primary_key == 0 {
            continue;
        }
        let name = dialect.quote(&column.name)?;
        if *value == Cell::Null {
            conditions.push(format!("{name} IS NULL"));
        } else if kind == "ORACLE" && matches!(column.data_type.as_str(), "CLOB" | "NCLOB" | "BLOB")
        {
            let parameter = argument(kind, column, value, params)?;
            let conversion = match column.data_type.as_str() {
                "BLOB" => "TO_BLOB",
                "NCLOB" => "TO_NCLOB",
                _ => "TO_CLOB",
            };
            conditions.push(format!(
                "DBMS_LOB.COMPARE({name},{conversion}({parameter}))=0"
            ));
        } else {
            // PostgreSQL json/custom types may not define equality; their text
            // representation also preserves dates/numerics unsupported by SQLx Any.
            let mysql_text = matches!(
                column
                    .data_type
                    .to_ascii_lowercase()
                    .split(['(', ' '])
                    .next()
                    .unwrap_or_default(),
                "char"
                    | "varchar"
                    | "tinytext"
                    | "text"
                    | "mediumtext"
                    | "longtext"
                    | "enum"
                    | "set"
                    | "json"
            );
            let (name, parameter) =
                if kind == "MYSQL" && matches!(value, Cell::Text(_)) && mysql_text {
                    params.push(value.clone());
                    (
                        format!("CAST({name} AS BINARY)"),
                        format!("CAST({} AS BINARY)", placeholder(kind, params.len())),
                    )
                } else if kind == "POSTGRESQL" && matches!(value, Cell::Text(_)) {
                    params.push(value.clone());
                    (
                        format!("CAST({name} AS TEXT)"),
                        placeholder(kind, params.len()),
                    )
                } else {
                    (name, argument(kind, column, value, params)?)
                };
            conditions.push(format!("{name} = {parameter}"));
        }
    }
    if conditions.is_empty() {
        bail!("无法定位没有列的表");
    }
    Ok(conditions.join(" AND "))
}

fn one_row(
    kind: &str,
    definition: &TableDefinition,
    table: &str,
    condition: &str,
) -> Result<String> {
    Ok(match kind {
        "MYSQL" => format!("{condition} LIMIT 1"),
        "POSTGRESQL" => format!(
            "(tableoid, ctid) IN (SELECT tableoid, ctid FROM {table} WHERE {condition} LIMIT 1)"
        ),
        "ORACLE" => format!("ROWID IN (SELECT ROWID FROM {table} WHERE {condition} AND ROWNUM=1)"),
        "SQLITE" => {
            // Rowid aliases can be shadowed by real columns. Never guess if all
            // aliases are shadowed; PK predicates remain safe only when non-null.
            let alias = ["rowid", "_rowid_", "oid"].into_iter().find(|candidate| {
                !definition
                    .columns
                    .iter()
                    .any(|c| c.name.eq_ignore_ascii_case(candidate))
            });
            if let Some(alias) = alias {
                format!("{alias} IN (SELECT {alias} FROM {table} WHERE {condition} LIMIT 1)")
            } else {
                bail!("表覆盖了全部 rowid 标识符，无法保证只修改一行");
            }
        }
        _ => bail!("不支持此数据库的安全行定位"),
    })
}

fn oracle_default_insert(definition: &TableDefinition, table: &str) -> Result<String> {
    let column = definition
        .columns
        .iter()
        .find(|c| !c.generated || c.auto_increment)
        .ok_or_else(|| anyhow::anyhow!("表没有可使用 DEFAULT 的字段"))?;
    let name = crate::dialect::resolve("ORACLE")?.quote(&column.name)?;
    Ok(format!("INSERT INTO {table} ({name}) VALUES (DEFAULT)"))
}

pub(in crate::database) fn build(
    kind: &str,
    definition: &TableDefinition,
    change: &RowChange,
) -> Result<Option<Statement>> {
    build_inner(kind, definition, change, false)
}
pub(in crate::database) fn build_query(
    kind: &str,
    definition: &TableDefinition,
    change: &RowChange,
) -> Result<Option<Statement>> {
    build_inner(kind, definition, change, true)
}
fn build_inner(
    kind: &str,
    definition: &TableDefinition,
    change: &RowChange,
    query_result: bool,
) -> Result<Option<Statement>> {
    if definition.view {
        bail!("视图当前为只读");
    }
    if !definition.transactional {
        bail!("该表不支持事务，无法保证批量修改失败时回滚");
    }
    let table = qualified(kind, &definition.table)?;
    let dialect = crate::dialect::resolve(kind)?;
    let mut params = Vec::new();
    let sql = match change {
        RowChange::Insert { values } => {
            if values.len() != definition.columns.len() {
                bail!("新行列数不匹配，请刷新表结构");
            }
            let mut names = Vec::new();
            let mut arguments = Vec::new();
            for (column, value) in definition.columns.iter().zip(values) {
                if let Some(value) = value {
                    if column.generated {
                        bail!("生成列不可写入：{}", column.name);
                    }
                    names.push(dialect.quote(&column.name)?);
                    arguments.push(argument(kind, column, value, &mut params)?);
                }
            }
            if names.is_empty() {
                if kind == "MYSQL" {
                    format!("INSERT INTO {table} () VALUES ()")
                } else if kind == "ORACLE" {
                    oracle_default_insert(definition, &table)?
                } else {
                    format!("INSERT INTO {table} DEFAULT VALUES")
                }
            } else {
                format!(
                    "INSERT INTO {table} ({}) VALUES ({})",
                    names.join(", "),
                    arguments.join(", ")
                )
            }
        }
        RowChange::Update { original, values } => {
            if values.len() != definition.columns.len() || original.len() != values.len() {
                bail!("更新列数不匹配");
            }
            let mut assignments = Vec::new();
            for ((column, before), value) in definition.columns.iter().zip(original).zip(values) {
                if before != value {
                    if column.generated {
                        bail!("生成列不可修改：{}", column.name);
                    }
                    assignments.push(format!(
                        "{} = {}",
                        dialect.quote(&column.name)?,
                        argument(kind, column, value, &mut params)?
                    ));
                }
            }
            if assignments.is_empty() {
                return Ok(None);
            }
            let primary_only = query_result
                && definition
                    .columns
                    .iter()
                    .any(|column| column.primary_key > 0);
            let condition = predicate(kind, definition, original, &mut params, primary_only)?;
            let has_primary_key = definition
                .columns
                .iter()
                .any(|column| column.primary_key > 0)
                && definition
                    .columns
                    .iter()
                    .zip(original)
                    .filter(|(column, _)| column.primary_key > 0)
                    .all(|(_, cell)| *cell != Cell::Null);
            // Full original-row matching also detects concurrent changes for PK tables.
            let condition = if has_primary_key {
                condition
            } else {
                one_row(kind, definition, &table, &condition)?
            };
            format!(
                "UPDATE {table} SET {} WHERE {condition}",
                assignments.join(", ")
            )
        }
        RowChange::Delete { original } => {
            if !definition
                .columns
                .iter()
                .any(|column| column.primary_key > 0)
            {
                bail!("无主键表不提供删除操作");
            }
            let condition = predicate(kind, definition, original, &mut params, query_result)?;
            if definition
                .columns
                .iter()
                .zip(original)
                .any(|(column, cell)| column.primary_key > 0 && *cell == Cell::Null)
            {
                bail!("主键包含 NULL，不能安全删除");
            }
            format!("DELETE FROM {table} WHERE {condition}")
        }
    };
    Ok(Some(Statement { sql, params }))
}

pub fn inserts_sql(kind: &str, definition: &TableDefinition, rows: &[Vec<Cell>]) -> Result<String> {
    let dialect = crate::dialect::resolve(kind)?;
    let columns: Vec<_> = definition
        .columns
        .iter()
        .enumerate()
        .filter(|(_, column)| !column.generated)
        .collect();
    let names = columns
        .iter()
        .map(|(_, column)| dialect.quote(&column.name))
        .collect::<Result<Vec<_>>>()?
        .join(", ");
    let table = qualified(kind, &definition.table)?;
    let mut result = String::new();
    for row in rows {
        if row.len() != definition.columns.len() {
            bail!("行列数不匹配");
        }
        if columns.is_empty() {
            result.push_str(&if kind == "MYSQL" {
                format!("INSERT INTO {table} () VALUES ();\n")
            } else if kind == "ORACLE" {
                format!("{};\n", oracle_default_insert(definition, &table)?)
            } else {
                format!("INSERT INTO {table} DEFAULT VALUES;\n")
            });
            continue;
        }
        if row
            .iter()
            .any(|cell| matches!(cell, Cell::Real(value) if !value.is_finite()))
        {
            bail!("无法导出非有限数值");
        }
        let values = columns
            .iter()
            .map(|(index, column)| match &row[*index] {
                Cell::Null => "NULL".into(),
                Cell::Text(text)
                    if kind == "ORACLE"
                        && (column.data_type == "DATE"
                            || column.data_type.starts_with("TIMESTAMP")) =>
                {
                    let zone = column.data_type.contains("WITH TIME ZONE")
                        && !column.data_type.contains("LOCAL");
                    format!(
                        "{}('{}','YYYY-MM-DD HH24:MI:SS.FF{}')",
                        if zone {
                            "TO_TIMESTAMP_TZ"
                        } else {
                            "TO_TIMESTAMP"
                        },
                        text.replace('\'', "''"),
                        if zone { " TZH:TZM" } else { "" }
                    )
                }
                Cell::Text(text) => {
                    // MySQL backslash escapes are mode-dependent. Hex literals avoid
                    // SQL modes changing the meaning of a quote/backslash sequence.
                    if kind == "HIVE" {
                        format!(
                            "'{}'",
                            text.replace('\\', "\\\\")
                                .replace('\'', "\\'")
                                .replace('\0', "\\0")
                                .replace('\n', "\\n")
                                .replace('\r', "\\r")
                        )
                    } else if kind == "MYSQL" {
                        format!("CONVERT(X'{}' USING utf8mb4)", hex(text.as_bytes()))
                    } else if kind == "POSTGRESQL" {
                        format!("E'{}'", text.replace('\\', "\\\\").replace('\'', "''"))
                    } else if kind == "ORACLE" && text.contains('\0') {
                        text.split('\0')
                            .map(|part| format!("'{}'", part.replace('\'', "''")))
                            .collect::<Vec<_>>()
                            .join(" || CHR(0) || ")
                    } else if text.contains('\0') {
                        format!("CAST(X'{}' AS TEXT)", hex(text.as_bytes()))
                    } else {
                        format!("'{}'", text.replace('\'', "''"))
                    }
                }
                Cell::Binary(bytes) if kind == "ORACLE" => {
                    format!("HEXTORAW('{}')", hex(bytes))
                }
                Cell::Binary(bytes) if kind == "HIVE" => format!("unhex('{}')", hex(bytes)),
                Cell::Binary(bytes) if kind == "POSTGRESQL" => {
                    format!("decode('{}','hex')", hex(bytes))
                }
                Cell::Binary(bytes) => format!("X'{}'", hex(bytes)),
                other => other.display(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        result.push_str(&format!(
            "INSERT INTO {table} ({names}) VALUES ({values});\n"
        ));
    }
    Ok(result)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
