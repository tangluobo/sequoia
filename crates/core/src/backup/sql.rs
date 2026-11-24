//! Stream scalar literals into nb3 SQL pages without building a second row string.
use anyhow::{Result, bail, ensure};
use std::io::Write;
pub enum Value<'a> {
    Null,
    Text(&'a str),
    Number(&'a str),
    Boolean(bool),
    Binary(&'a [u8]),
}
fn hex(output: &mut dyn Write, bytes: &[u8]) -> Result<()> {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut buffer = [0u8; 8192];
    for chunk in bytes.chunks(buffer.len() / 2) {
        for (i, &byte) in chunk.iter().enumerate() {
            buffer[i * 2] = DIGITS[(byte >> 4) as usize];
            buffer[i * 2 + 1] = DIGITS[(byte & 15) as usize];
        }
        output.write_all(&buffer[..chunk.len() * 2])?;
    }
    Ok(())
}
fn text(output: &mut dyn Write, kind: &str, value: &str) -> Result<()> {
    match kind {
        "ORACLE" => {
            output.write_all(b"'")?;
            let mut previous = 0;
            for (at, _) in value.match_indices('\'') {
                output.write_all(&value.as_bytes()[previous..at])?;
                output.write_all(b"''")?;
                previous = at + 1;
            }
            output.write_all(&value.as_bytes()[previous..])?;
            output.write_all(b"'")?;
        }
        "MYSQL" => {
            output.write_all(b"CONVERT(X'")?;
            hex(output, value.as_bytes())?;
            output.write_all(b"' USING utf8mb4)")?;
        }
        "SQLITE" => {
            output.write_all(b"CAST(X'")?;
            hex(output, value.as_bytes())?;
            output.write_all(b"' AS TEXT)")?;
        }
        "POSTGRESQL" => {
            output.write_all(b"E'")?;
            let bytes = value.as_bytes();
            let mut start = 0;
            for (index, &byte) in bytes.iter().enumerate() {
                let replacement: Option<&[u8]> = match byte {
                    b'\'' => Some(b"\\'"),
                    b'\\' => Some(b"\\\\"),
                    b'\n' => Some(b"\\n"),
                    b'\r' => Some(b"\\r"),
                    b'\t' => Some(b"\\t"),
                    _ => None,
                };
                if let Some(replacement) = replacement {
                    output.write_all(&bytes[start..index])?;
                    output.write_all(replacement)?;
                    start = index + 1;
                } else if byte < 32 || byte == 127 {
                    ensure!(byte != 0, "PostgreSQL 文本不能包含 NUL");
                    output.write_all(&bytes[start..index])?;
                    output.write_all(b"\\x")?;
                    hex(output, &[byte])?;
                    start = index + 1;
                }
            }
            output.write_all(&bytes[start..])?;
            output.write_all(b"'")?;
        }
        _ => bail!("此数据库未实现备份 SQL 值编码"),
    }
    Ok(())
}
pub fn is_number(value: &str) -> bool {
    static NUMBER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\A[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?\z")
            .expect("numeric literal")
    });
    NUMBER.is_match(value)
}
pub fn literal(output: &mut dyn Write, kind: &str, value: Value<'_>) -> Result<()> {
    match value {
        Value::Null => output.write_all(b"NULL")?,
        Value::Boolean(value) => output.write_all(if value { b"true" } else { b"false" })?,
        Value::Text(value) => text(output, kind, value)?,
        Value::Number(value) => {
            if is_number(value) {
                output.write_all(value.as_bytes())?;
            } else {
                ensure!(
                    matches!(value, "NaN" | "Infinity" | "-Infinity"),
                    "备份数值格式无效"
                );
                text(output, kind, value)?;
            }
        }
        Value::Binary(bytes) => {
            match kind {
                "MYSQL" | "SQLITE" => output.write_all(b"X'")?,
                "POSTGRESQL" => output.write_all(b"decode('")?,
                "ORACLE" => output.write_all(b"HEXTORAW('")?,
                _ => bail!("此数据库未实现二进制备份"),
            };
            hex(output, bytes)?;
            output.write_all(if kind == "POSTGRESQL" {
                b"','hex')"
            } else if kind == "ORACLE" {
                b"')"
            } else {
                b"'"
            })?;
        }
    }
    Ok(())
}
