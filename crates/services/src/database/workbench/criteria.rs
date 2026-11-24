//! One validated predicate/order plan for paging, counting and streaming exports.
use super::*;
use anyhow::ensure;
use tomato_core::result_filter::Operator;

pub(crate) fn effective(request: &PageRequest) -> TableQuery {
    let mut query = request.query.clone();
    if let Some(sort) = &request.sort {
        query.sorts.retain(|item| item.column != sort.column);
        query.sorts.insert(0, sort.clone());
    }
    query
}

fn literal(kind: &str, value: &str) -> Result<String> {
    ensure!(value.len() <= 64 * 1024, "筛选值超过 64 KiB");
    // Ordinary readable literals are safe in either MySQL string mode when no
    // backslash/NUL is present. Other values use mode-independent encodings.
    if kind == "HIVE" {
        return Ok(format!(
            "'{}'",
            value
                .replace('\\', "\\\\")
                .replace('\'', "\\'")
                .replace('\0', "\\0")
        ));
    }
    if !value.contains(['\\', '\0']) {
        return Ok(format!("'{}'", value.replace('\'', "''")));
    }
    if kind == "MYSQL" {
        // Introducers keep literal coercibility. CONVERT(... USING utf8mb4)
        // instead acquires an implicit collation and conflicts with numeric
        // CASTs in a mixed-column LIKE search on MySQL 8.
        let hex: String = value
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        return Ok(format!("_utf8mb4 X'{hex}'"));
    }
    if kind == "ORACLE" {
        return Ok(value
            .split('\0')
            .map(|part| format!("'{}'", part.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(" || CHR(0) || "));
    }
    let mut output = Vec::new();
    tomato_core::backup::sql::literal(
        &mut output,
        kind,
        tomato_core::backup::sql::Value::Text(value),
    )?;
    Ok(String::from_utf8(output)?)
}

fn binary(kind: &str, value: &str, column: &Column) -> Result<String> {
    let bytes = super::unhex(value)?;
    if kind == "MYSQL" && column.data_type.to_ascii_uppercase().starts_with("BIT") {
        ensure!(
            !bytes.is_empty() && bytes.len() <= 8,
            "BIT 筛选需要 1–8 字节的十六进制值"
        );
        // Comparing a BIT field to a binary-string literal can promote both to
        // DOUBLE, merging adjacent large values. A decimal integer literal
        // retains unsigned 64-bit comparison and leaves the column uncast.
        return Ok(bytes
            .iter()
            .fold(0u64, |value, byte| (value << 8) | u64::from(*byte))
            .to_string());
    }
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(match kind {
        "POSTGRESQL" => format!("decode('{hex}','hex')"),
        "ORACLE" => format!("HEXTORAW('{hex}')"),
        "HIVE" => format!("unhex('{hex}')"),
        _ => format!("X'{hex}'"),
    })
}

fn text_column(kind: &str, name: &str) -> String {
    match kind {
        "MYSQL" => format!("CAST({name} AS CHAR)"),
        "ORACLE" => format!("TO_CHAR({name})"),
        "HIVE" => format!("CAST({name} AS STRING)"),
        _ => format!("CAST({name} AS TEXT)"),
    }
}

fn comparison_column(kind: &str, column: &Column, name: &str) -> String {
    if kind == "POSTGRESQL"
        && [column.type_name.as_str(), column.data_type.as_str()]
            .iter()
            .any(|kind| kind.eq_ignore_ascii_case("json") || kind.eq_ignore_ascii_case("xml"))
    {
        // These PostgreSQL types have no equality/ordering operators. Match
        // the displayed original text; keep the SELECT projection native.
        text_column(kind, name)
    } else {
        name.to_owned()
    }
}

fn contains(kind: &str, name: &str, value: &str, negated: bool) -> Result<String> {
    if kind == "HIVE" {
        let pattern = format!("\\Q{}\\E", value.replace("\\E", "\\E\\\\E\\Q"));
        return Ok(format!(
            "{} {}RLIKE {}",
            text_column(kind, name),
            if negated { "NOT " } else { "" },
            literal(kind, &pattern)?
        ));
    }
    // A cell's '%' and '_' are data. An explicit escape is independent of both
    // MySQL sql_mode and PostgreSQL standard_conforming_strings.
    let escaped = value
        .replace('!', "!!")
        .replace('%', "!%")
        .replace('_', "!_");
    Ok(format!(
        "{} {}LIKE {} ESCAPE '!'",
        text_column(kind, name),
        if negated { "NOT " } else { "" },
        literal(kind, &format!("%{escaped}%"))?
    ))
}

fn numeric(column: &Column) -> bool {
    let data_type = column.data_type.to_ascii_uppercase();
    [
        "INT",
        "TINYINT",
        "SMALLINT",
        "MEDIUMINT",
        "BIGINT",
        "INTEGER",
        "DECIMAL",
        "NUMERIC",
        "NUMBER",
        "FLOAT",
        "DOUBLE",
        "REAL",
        "SERIAL",
        "BIGSERIAL",
        "SMALLSERIAL",
        "INT2",
        "INT4",
        "INT8",
        "FLOAT4",
        "FLOAT8",
    ]
    .iter()
    .any(|name| {
        data_type == *name
            || data_type.starts_with(&format!("{name}("))
            || data_type.starts_with(&format!("{name} "))
    })
}

pub(crate) fn clauses(
    kind: &str,
    definition: &TableDefinition,
    query: &TableQuery,
) -> Result<(String, String)> {
    ensure!(
        query.filters.len() <= 64 && query.sorts.len() <= 64,
        "筛选或排序字段超过 64 个"
    );
    ensure!(query.search.len() <= 4096, "快速筛选超过 4096 字节");
    ensure!(
        query.search.len().saturating_mul(definition.columns.len()) <= 256 * 1024,
        "快速筛选语句过大，请指定字段条件"
    );
    ensure!(
        query
            .filters
            .iter()
            .map(|f| f.condition.expected.as_ref().map_or(0, String::len))
            .sum::<usize>()
            <= 256 * 1024,
        "筛选条件超过 256 KiB"
    );
    let dialect = crate::dialect::resolve(kind)?;
    let column = |name: &str| {
        definition
            .columns
            .iter()
            .find(|column| column.name == name)
            .with_context(|| format!("筛选或排序字段已不存在：{name}"))
    };
    let mut filters = Vec::new();
    for filter in &query.filters {
        let metadata = column(&filter.column)?;
        let name = dialect.quote(&filter.column)?;
        let condition = &filter.condition;
        let Some(value) = &condition.expected else {
            ensure!(
                matches!(condition.operator, Operator::Equal | Operator::NotEqual),
                "NULL 筛选只支持等于或不等于"
            );
            filters.push(format!(
                "{name} IS {}NULL",
                if condition.operator == Operator::NotEqual {
                    "NOT "
                } else {
                    ""
                }
            ));
            continue;
        };
        if matches!(condition.operator, Operator::Like | Operator::NotLike) {
            ensure!(!metadata.binary, "二进制字段请使用十六进制等值筛选");
            filters.push(contains(
                kind,
                &name,
                value,
                condition.operator == Operator::NotLike,
            )?);
            continue;
        }
        let value = if metadata.binary {
            ensure!(value.len() <= 64 * 1024, "二进制筛选值超过 64 KiB");
            binary(kind, value, metadata)?
        } else if numeric(metadata) {
            ensure!(value.len() <= 64 * 1024, "筛选值超过 64 KiB");
            if tomato_core::backup::sql::is_number(value) {
                value.clone()
            } else if kind == "POSTGRESQL"
                && matches!(value.as_str(), "NaN" | "Infinity" | "-Infinity")
            {
                literal(kind, value)?
            } else {
                bail!("字段 {} 的筛选值必须为有效数值", metadata.name);
            }
        } else {
            literal(kind, value)?
        };
        let value = if kind == "MYSQL" && metadata.data_type.eq_ignore_ascii_case("json") {
            format!("CAST({value} AS JSON)")
        } else {
            value
        };
        let name = comparison_column(kind, metadata, &name);
        let operator = match condition.operator {
            Operator::Equal => "=",
            Operator::NotEqual => "<>",
            Operator::Less => "<",
            Operator::Greater => ">",
            Operator::Like | Operator::NotLike => unreachable!(),
        };
        filters.push(format!("{name} {operator} {value}"));
    }
    let mut predicates = Vec::new();
    if !filters.is_empty() {
        predicates.push(format!(
            "({})",
            filters.join(if query.match_any { " OR " } else { " AND " })
        ));
    }
    if !query.search.is_empty() {
        let parts = definition
            .columns
            .iter()
            .filter(|c| !c.binary)
            .map(|c| contains(kind, &dialect.quote(&c.name)?, &query.search, false))
            .collect::<Result<Vec<_>>>()?;
        predicates.push(if parts.is_empty() {
            "1 = 0".into()
        } else {
            format!("({})", parts.join(" OR "))
        });
    }
    let predicate = if predicates.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", predicates.join(" AND "))
    };
    let mut sorts = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for sort in &query.sorts {
        let metadata = column(&sort.column)?;
        ensure!(
            names.insert(sort.column.as_str()),
            "重复排序字段：{}",
            sort.column
        );
        sorts.push(format!(
            "{} {}",
            comparison_column(kind, metadata, &dialect.quote(&sort.column)?),
            if sort.descending { "DESC" } else { "ASC" }
        ));
    }
    let mut primary: Vec<_> = definition
        .columns
        .iter()
        .filter(|c| c.primary_key > 0)
        .collect();
    primary.sort_by_key(|c| c.primary_key);
    for key in primary {
        if names.insert(key.name.as_str()) {
            sorts.push(dialect.quote(&key.name)?);
        }
    }
    let order = if sorts.is_empty() {
        String::new()
    } else {
        format!(" ORDER BY {}", sorts.join(", "))
    };
    Ok((predicate, order))
}
