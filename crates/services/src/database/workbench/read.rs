//! Plain SELECTs use native text protocols, without server-side value conversions.
use super::*;
use futures_util::Stream;
use sqlx::{mysql::MySqlRow, postgres::PgRow};

pub(super) struct Rows {
    pub rows: Vec<Vec<Cell>>,
    pub truncated: bool,
}

fn mysql_cell(row: &MySqlRow, index: usize, column: &Column) -> Result<Cell> {
    let value = row.try_get_raw(index)?;
    if value.is_null() {
        return Ok(Cell::Null);
    }
    let bytes = value.as_bytes().map_err(anyhow::Error::msg)?;
    // Includes BIT: its wire bytes already preserve padding and declared width.
    if column.binary {
        Ok(Cell::Binary(bytes.to_vec()))
    } else {
        Ok(Cell::Text(std::str::from_utf8(bytes)?.to_owned()))
    }
}

fn postgres_boolean(mut ty: &sqlx::postgres::PgTypeInfo) -> bool {
    use sqlx::TypeInfo;
    loop {
        if ty.oid().is_some_and(|oid| oid.0 == 16) {
            return true;
        }
        // Simple-protocol row descriptions can carry unresolved OIDs (XML,
        // extensions and user types). kind() panics on those declarations.
        if ty.oid().is_none() || ty.name() == "?" {
            return false;
        }
        match ty.kind() {
            sqlx::postgres::PgTypeKind::Domain(base) => ty = base,
            _ => return false,
        }
    }
}

fn postgres_cell(row: &PgRow, index: usize, column: &Column) -> Result<Cell> {
    let value = row.try_get_raw(index)?;
    if value.is_null() {
        return Ok(Cell::Null);
    }
    anyhow::ensure!(
        value.format() == sqlx::postgres::PgValueFormat::Text,
        "表读取收到非文本 PostgreSQL 数据帧"
    );
    if column.binary {
        Ok(Cell::Binary(postgres_binary(
            value.as_bytes().map_err(anyhow::Error::msg)?,
        )?))
    } else {
        let text = value.as_str().map_err(anyhow::Error::msg)?;
        // Keep the existing grid/editor and optimistic match representation.
        let text = if postgres_boolean(row.column(index).type_info()) {
            match text {
                "t" => "true",
                "f" => "false",
                _ => text,
            }
        } else {
            text
        };
        Ok(Cell::Text(text.to_owned()))
    }
}

fn postgres_binary(bytes: &[u8]) -> Result<Vec<u8>> {
    if let Some(hex) = bytes.strip_prefix(b"\\x") {
        return unhex(std::str::from_utf8(hex)?);
    }
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'\\' {
            decoded.push(bytes[at]);
            at += 1;
        } else if bytes.get(at + 1) == Some(&b'\\') {
            decoded.push(b'\\');
            at += 2;
        } else {
            let octal = bytes.get(at + 1..at + 4).context("bytea 转义不完整")?;
            anyhow::ensure!(
                matches!(octal[0], b'0'..=b'3')
                    && octal[1..].iter().all(|b| matches!(b, b'0'..=b'7')),
                "bytea 转义无效"
            );
            decoded.push((octal[0] - b'0') * 64 + (octal[1] - b'0') * 8 + (octal[2] - b'0'));
            at += 4;
        }
    }
    Ok(decoded)
}

pub(super) async fn page(
    conn: &mut AnyConnection,
    kind: &str,
    sql: &str,
    columns: &[Column],
) -> Result<Rows> {
    match kind {
        "MYSQL" => {
            let native = conn
                .backend_mut::<sqlx::MySqlConnection>()
                .context("MySQL 原生会话类型不匹配")?;
            collect(sqlx::raw_sql(sql).fetch(native), columns, mysql_cell).await
        }
        "POSTGRESQL" => {
            let native = conn
                .backend_mut::<sqlx::PgConnection>()
                .context("PostgreSQL 原生会话类型不匹配")?;
            collect(sqlx::raw_sql(sql).fetch(native), columns, postgres_cell).await
        }
        _ => {
            collect(
                query(sql, &[]).fetch(conn),
                columns,
                |row, index, column| {
                    if kind == "SQLITE" || column.binary {
                        native_cell(row, index)
                    } else {
                        strings(row, index)
                    }
                },
            )
            .await
        }
    }
}

async fn collect<R, S, F>(mut stream: S, columns: &[Column], cell: F) -> Result<Rows>
where
    R: Row,
    S: Stream<Item = std::result::Result<R, sqlx::Error>> + Unpin,
    F: Fn(&R, usize, &Column) -> Result<Cell>,
{
    let mut result = Rows {
        rows: Vec::new(),
        truncated: false,
    };
    let mut held = 0usize;
    while let Some(row) = stream.try_next().await? {
        // Drain the result even after reaching the cap, keeping this session usable.
        if result.truncated {
            continue;
        }
        anyhow::ensure!(row.len() == columns.len(), "表结构已变化，请刷新后重新打开");
        let values = columns
            .iter()
            .enumerate()
            .map(|(index, column)| cell(&row, index, column))
            .collect::<Result<Vec<_>>>()?;
        let size = std::mem::size_of::<Vec<Cell>>()
            + values.capacity() * std::mem::size_of::<Cell>()
            + values
                .iter()
                .map(|cell| match cell {
                    Cell::Text(text) => text.capacity(),
                    Cell::Binary(bytes) => bytes.capacity(),
                    _ => 0,
                })
                .sum::<usize>();
        if held.saturating_add(size) > limits::QUERY_BYTES {
            result.truncated = true;
        } else {
            held += size;
            result.rows.push(values);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unresolved_postgres_type_declarations_do_not_panic_or_change_text_to_boolean() {
        use sqlx::postgres::{PgTypeInfo, types::Oid};
        assert!(postgres_boolean(&PgTypeInfo::with_oid(Oid(16))));
        for oid in [142, 16384, 100000] {
            assert!(!postgres_boolean(&PgTypeInfo::with_oid(Oid(oid))));
        }
        assert!(!postgres_boolean(&PgTypeInfo::with_name(
            "unresolved_domain"
        )));
    }
    #[test]
    fn bytea_hex_and_legacy_escape_preserve_null_bytes_empty_bytes_and_backslashes() -> Result<()> {
        for (input, expected) in [
            (r"\x00ff5c2722", &[0, 255, 92, 39, 34][..]),
            (r"\000\377\134'\042", &[0, 255, 92, 39, 34][..]),
            (r"\\abc", &b"\\abc"[..]),
            (r"\x", &b""[..]),
            ("", &b""[..]),
        ] {
            assert_eq!(postgres_binary(input.as_bytes())?, expected);
        }
        for malformed in [r"\x0", r"\xzz", "\\", r"\400", r"\099"] {
            assert!(postgres_binary(malformed.as_bytes()).is_err());
        }
        Ok(())
    }
}
