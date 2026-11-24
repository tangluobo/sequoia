use super::*;
use base64::Engine as _;
pub fn column_type(column: &Column) -> DataType {
    if column.binary {
        return DataType::Binary;
    }
    let t = column.data_type.to_ascii_lowercase();
    let name = t.split(['(', ' ']).next().unwrap_or_default();
    match name {
        "bool" | "boolean" => DataType::Boolean,
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint" | "int2" | "int4"
        | "int8" | "serial" | "bigserial" | "bit" => DataType::Integer,
        "float" | "double" | "real" | "binary_float" | "binary_double" => DataType::Real,
        "decimal" | "numeric" | "number" => DataType::Decimal {
            precision: 38,
            scale: 18,
        },
        "date" => DataType::Date,
        "timestamp" | "datetime" | "datetime2" | "timestamptz" => DataType::DateTime,
        _ => DataType::Text,
    }
}
pub(super) fn value(
    typ: &DataType,
    value: Cell,
    source: &Source,
    options: &Options,
) -> Result<Cell> {
    if value == Cell::Null {
        return Ok(Cell::Null);
    }
    if *typ == DataType::Binary && matches!(value, Cell::Binary(_)) {
        return Ok(value);
    }
    let text = value.display();
    Ok(match typ {
        DataType::Text => Cell::Text(text),
        DataType::Binary => match value {
            Cell::Binary(b) => Cell::Binary(b),
            _ => {
                let text = text.trim();
                let b = if options.binary_base64 {
                    base64::engine::general_purpose::STANDARD.decode(text)?
                } else {
                    let text = text
                        .strip_prefix("0x")
                        .or_else(|| text.strip_prefix("\\x"))
                        .unwrap_or(text);
                    ensure!(
                        text.len().is_multiple_of(2),
                        "二进制字段需要偶数长度的十六进制"
                    );
                    text.as_bytes()
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| Ok(u8::from_str_radix(std::str::from_utf8(b)?, 16)?))
                        .collect::<Result<Vec<_>>>()?
                };
                Cell::Binary(b)
            }
        },
        DataType::Integer => match value {
            Cell::Integer(n) => Cell::Integer(n),
            Cell::Boolean(b) => Cell::Integer(i64::from(b)),
            Cell::Real(n) => {
                ensure!(
                    n.is_finite()
                        && n.fract() == 0.
                        && (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&n),
                    "整数超出范围或包含小数"
                );
                Cell::Integer(n as i64)
            }
            _ => {
                let s = text.trim();
                if let Ok(n) = s.parse::<i64>() {
                    Cell::Integer(n)
                } else {
                    ensure!(s.parse::<u64>().is_ok(), "整数格式或范围无效");
                    Cell::Text(s.into())
                }
            }
        },
        DataType::Decimal { .. } => {
            let s = text.trim();
            let body = s.trim_start_matches(['+', '-']);
            let mut pieces = body.split(['e', 'E']);
            let mantissa = pieces.next().unwrap_or_default();
            let exponent = pieces.next();
            ensure!(
                !mantissa.is_empty()
                    && mantissa.bytes().any(|b| b.is_ascii_digit())
                    && mantissa.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                    && mantissa.matches('.').count() <= 1
                    && pieces.next().is_none()
                    && exponent.is_none_or(|e| e.parse::<i32>().is_ok()),
                "数值格式无效"
            );
            Cell::Text(s.into())
        }
        DataType::Real => {
            let n = text.parse::<f64>()?;
            ensure!(n.is_finite(), "浮点数必须为有限数");
            Cell::Real(n)
        }
        DataType::Boolean => Cell::Boolean(match text.trim().to_lowercase().as_str() {
            "true" | "t" | "yes" | "y" | "1" | "是" => true,
            "false" | "f" | "no" | "n" | "0" | "否" => false,
            _ => anyhow::bail!("布尔值格式无效"),
        }),
        DataType::Date => Cell::Text(date(&text, source)?.format("%Y-%m-%d").to_string()),
        DataType::DateTime => {
            let text = text.replace('T', " ");
            if let Ok(d) = chrono::NaiveDateTime::parse_from_str(&text, "%Y-%m-%d %H:%M:%S%.f") {
                Cell::Text(d.format("%Y-%m-%d %H:%M:%S%.f").to_string())
            } else if let Some((d, t)) = text.split_once(' ') {
                let d = date(d, source)?;
                let t = chrono::NaiveTime::parse_from_str(t, "%H:%M:%S%.f")?;
                Cell::Text(d.and_time(t).format("%Y-%m-%d %H:%M:%S%.f").to_string())
            } else {
                Cell::Text(
                    date(&text, source)?
                        .and_hms_opt(0, 0, 0)
                        .unwrap()
                        .to_string(),
                )
            }
        }
    })
}
fn date(text: &str, source: &Source) -> Result<chrono::NaiveDate> {
    if let Ok(d) = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return Ok(d);
    }
    let order = match source.date_order.as_str() {
        "DMY" => ["%d", "%m", "%Y"],
        "MDY" => ["%m", "%d", "%Y"],
        _ => ["%Y", "%m", "%d"],
    };
    Ok(chrono::NaiveDate::parse_from_str(
        text,
        &order.join(&source.date_separator),
    )?)
}
