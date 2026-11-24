//! HS2 has no PrepareStatement RPC. Bind only tokens outside strings/comments.
use anyhow::{Result, bail, ensure};
use tomato_core::workbench::Cell;
fn literal(cell: &Cell) -> Result<String> {
    Ok(match cell {
        Cell::Null => "NULL".into(),
        Cell::Boolean(value) => if *value { "TRUE" } else { "FALSE" }.into(),
        Cell::Integer(value) => value.to_string(),
        Cell::Real(value) => {
            ensure!(value.is_finite(), "Hive 参数不能为非有限数值");
            value.to_string()
        }
        Cell::Binary(bytes) => format!(
            "unhex('{}')",
            bytes.iter().map(|v| format!("{v:02X}")).collect::<String>()
        ),
        Cell::Text(text) => {
            let mut value = String::with_capacity(text.len() + 2);
            value.push('\'');
            for ch in text.chars() {
                match ch {
                    '\\' => value.push_str("\\\\"),
                    '\'' => value.push_str("\\'"),
                    '\0' => value.push_str("\\0"),
                    '\n' => value.push_str("\\n"),
                    '\r' => value.push_str("\\r"),
                    '\t' => value.push_str("\\t"),
                    _ => value.push(ch),
                }
            }
            value.push('\'');
            value
        }
    })
}
pub(super) fn apply(sql: &str, values: Option<Vec<Cell>>) -> Result<String> {
    ensure!(sql.len() <= 8 * 1024 * 1024, "Hive 查询超过 8 MiB");
    let Some(values) = values else {
        return Ok(sql.into());
    };
    ensure!(
        values.len() <= 4096 && sql.len() <= 8 * 1024 * 1024,
        "Hive 绑定请求超过限制"
    );
    let bytes = sql.as_bytes();
    let mut output = String::with_capacity(sql.len());
    let (mut start, mut index, mut parameter) = (0, 0, 0);
    while index < bytes.len() {
        match bytes[index] {
            b'\'' | b'"' | b'`' => {
                let quote = bytes[index];
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                    } else if bytes[index] == quote {
                        index += 1;
                        if bytes.get(index) == Some(&quote) {
                            index += 1;
                        } else {
                            break;
                        }
                    } else {
                        index += 1;
                    }
                }
            }
            b'-' if bytes.get(index + 1) == Some(&b'-') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                let mut depth = 1;
                while index < bytes.len() && depth > 0 {
                    if bytes.get(index..index + 2) == Some(b"/*") {
                        depth += 1;
                        index += 2;
                    } else if bytes.get(index..index + 2) == Some(b"*/") {
                        depth -= 1;
                        index += 2;
                    } else {
                        index += 1;
                    }
                }
            }
            b'?' => {
                output.push_str(&sql[start..index]);
                output.push_str(&literal(
                    values
                        .get(parameter)
                        .ok_or_else(|| anyhow::anyhow!("Hive 绑定参数不足"))?,
                )?);
                parameter += 1;
                index += 1;
                start = index;
                ensure!(
                    output.len() <= 8 * 1024 * 1024,
                    "Hive 绑定后的语句超过 8 MiB"
                );
            }
            _ => index += 1,
        }
    }
    output.push_str(&sql[start..]);
    ensure!(
        output.len() <= 8 * 1024 * 1024,
        "Hive 绑定后的语句超过 8 MiB"
    );
    if parameter != values.len() {
        bail!("Hive 绑定参数过多");
    }
    Ok(output)
}
