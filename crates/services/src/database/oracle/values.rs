//! Preserve Oracle decimals, NULL, binary and temporal values without JDBC/OCI.
use super::*;
use oracledb::{DbType, Metadata, OracleNumber, OracleTimestamp};
use sqlx_core::any::{AnyTypeInfoKind as Kind, AnyValue};
use std::io::Read;

fn jdbc(kind: &DbType) -> i32 {
    if kind == oracledb::DB_TYPE_NUMBER || kind == oracledb::DB_TYPE_BINARY_INTEGER {
        2
    } else if kind == oracledb::DB_TYPE_BINARY_FLOAT {
        6
    } else if kind == oracledb::DB_TYPE_BINARY_DOUBLE {
        8
    } else if kind == oracledb::DB_TYPE_BOOLEAN {
        16
    } else if kind == oracledb::DB_TYPE_DATE
        || kind == oracledb::DB_TYPE_TIMESTAMP
        || kind == oracledb::DB_TYPE_TIMESTAMP_LTZ
    {
        93
    } else if kind == oracledb::DB_TYPE_TIMESTAMP_TZ {
        2014
    } else if kind == oracledb::DB_TYPE_RAW || kind == oracledb::DB_TYPE_LONG_RAW {
        -3
    } else if kind == oracledb::DB_TYPE_BLOB {
        2004
    } else if kind == oracledb::DB_TYPE_CLOB || kind == oracledb::DB_TYPE_NCLOB {
        2005
    } else {
        12
    }
}
pub(super) fn columns(metadata: &[Metadata]) -> Result<Vec<Column>> {
    if metadata.len() > 4096
        || metadata
            .iter()
            .map(|c| c.name().len() + std::mem::size_of::<Column>())
            .sum::<usize>()
            > limits::QUERY_BYTES
    {
        return Err(error("Oracle 查询列信息超过限制"));
    }
    Ok(metadata
        .iter()
        .enumerate()
        .map(|(ordinal, m)| Column {
            column: AnyColumn {
                ordinal,
                name: m.name().to_owned().into(),
                type_info: AnyTypeInfo { kind: Kind::Text },
            },
            nullable: m.nullable(),
            jdbc: jdbc(m.db_type()),
        })
        .collect())
}
fn text(value: String) -> Result<AnyValueKind<'static>> {
    if value.len() > limits::QUERY_BYTES {
        return Err(error("Oracle 单元格超过 8 MiB"));
    }
    Ok(AnyValueKind::Text(Cow::Owned(value)))
}
fn timestamp(value: OracleTimestamp, zone: bool) -> String {
    let mut result = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        value.year(),
        value.month(),
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    );
    let fraction = format!("{:09}", value.nanoseconds());
    let fraction = fraction.trim_end_matches('0');
    result.push('.');
    result.push_str(if fraction.is_empty() { "0" } else { fraction });
    if zone {
        result.push_str(&format!(
            " {}{:02}:{:02}",
            if value.tz_hour_offset() < 0 || value.tz_minute_offset() < 0 {
                '-'
            } else {
                '+'
            },
            value.tz_hour_offset().unsigned_abs(),
            value.tz_minute_offset().unsigned_abs()
        ));
    }
    result
}

fn session_timestamp(value: OracleTimestamp, with_zone: bool) -> Result<String> {
    if !with_zone {
        return Ok(timestamp(value, false));
    }
    use chrono::{Datelike as _, Timelike as _};

    // The thin driver exposes the UTC components and the original offset
    // separately. Convert to the offset's calendar date before displaying
    // or binding this value as Oracle's wall-clock timestamp text.
    let hour = value.tz_hour_offset();
    let minute = value.tz_minute_offset();
    let offset = i64::from(hour) * 60 + i64::from(minute);
    let local = chrono::NaiveDate::from_ymd_opt(
        i32::from(value.year()),
        u32::from(value.month()),
        u32::from(value.day()),
    )
    .and_then(|date| {
        date.and_hms_nano_opt(
            u32::from(value.hour()),
            u32::from(value.minute()),
            u32::from(value.second()),
            value.nanoseconds(),
        )
    })
    .and_then(|utc| utc.checked_add_signed(chrono::TimeDelta::minutes(offset)))
    .ok_or_else(|| error("Oracle 带时区时间无效"))?;
    Ok(timestamp(
        OracleTimestamp::new_timestamp_tz(
            local.year().try_into().map_err(error)?,
            local.month() as u8,
            local.day() as u8,
            local.hour() as u8,
            local.minute() as u8,
            local.second() as u8,
            local.nanosecond(),
            hour,
            minute,
        ),
        true,
    ))
}
fn bytes(value: Vec<u8>) -> Result<AnyValueKind<'static>> {
    if value.len() > limits::QUERY_BYTES {
        return Err(error("Oracle 二进制单元格超过 8 MiB"));
    }
    Ok(AnyValueKind::Blob(Cow::Owned(value)))
}
fn cell(row: &mut oracledb::Row, index: usize, kind: &DbType) -> Result<AnyValueKind<'static>> {
    let null = AnyValueKind::Null(Kind::Text);
    if kind == oracledb::DB_TYPE_NUMBER || kind == oracledb::DB_TYPE_BINARY_INTEGER {
        return match row
            .take::<Option<OracleNumber>>(index)
            .map_err(super::driver_error)?
        {
            None => Ok(null),
            Some(v) => {
                let v = v.to_string();
                if let Ok(n) = v.parse::<i64>() {
                    Ok(AnyValueKind::BigInt(n))
                } else {
                    text(v)
                }
            }
        };
    }
    if kind == oracledb::DB_TYPE_BINARY_FLOAT {
        return Ok(row
            .take::<Option<f32>>(index)
            .map_err(super::driver_error)?
            .map_or(null, AnyValueKind::Real));
    }
    if kind == oracledb::DB_TYPE_BINARY_DOUBLE {
        return Ok(row
            .take::<Option<f64>>(index)
            .map_err(super::driver_error)?
            .map_or(null, AnyValueKind::Double));
    }
    if kind == oracledb::DB_TYPE_BOOLEAN {
        return Ok(row
            .take::<Option<bool>>(index)
            .map_err(super::driver_error)?
            .map_or(null, AnyValueKind::Bool));
    }
    if kind == oracledb::DB_TYPE_DATE
        || kind == oracledb::DB_TYPE_TIMESTAMP
        || kind == oracledb::DB_TYPE_TIMESTAMP_LTZ
        || kind == oracledb::DB_TYPE_TIMESTAMP_TZ
    {
        return row
            .take::<Option<OracleTimestamp>>(index)
            .map_err(super::driver_error)?
            .map_or(Ok(null), |v| {
                text(session_timestamp(
                    v,
                    kind == oracledb::DB_TYPE_TIMESTAMP_TZ,
                )?)
            });
    }
    if kind == oracledb::DB_TYPE_RAW || kind == oracledb::DB_TYPE_LONG_RAW {
        return row
            .take::<Option<Vec<u8>>>(index)
            .map_err(super::driver_error)?
            .map_or(Ok(null), bytes);
    }
    if kind == oracledb::DB_TYPE_BLOB
        || kind == oracledb::DB_TYPE_CLOB
        || kind == oracledb::DB_TYPE_NCLOB
        || kind == oracledb::DB_TYPE_BFILE
    {
        let Some(mut lob) = row
            .take::<Option<oracledb::Lob>>(index)
            .map_err(super::driver_error)?
        else {
            return Ok(null);
        };
        if lob.get_size().map_err(super::driver_error)? > limits::QUERY_BYTES {
            return Err(error("Oracle LOB 超过 8 MiB，请缩小查询范围"));
        }
        let mut value = Vec::new();
        lob.take((limits::QUERY_BYTES + 1) as u64)
            .read_to_end(&mut value)
            .map_err(super::failure::lob_error)?;
        return if kind == oracledb::DB_TYPE_BLOB || kind == oracledb::DB_TYPE_BFILE {
            bytes(value)
        } else {
            text(String::from_utf8(value).map_err(error)?)
        };
    }
    if kind == oracledb::DB_TYPE_INTERVAL_DS {
        return row
            .take::<Option<oracledb::OracleIntervalDS>>(index)
            .map_err(super::driver_error)?
            .map_or(Ok(null), |v| text(v.to_string()));
    }
    if kind == oracledb::DB_TYPE_INTERVAL_YM {
        return row
            .take::<Option<oracledb::OracleIntervalYM>>(index)
            .map_err(super::driver_error)?
            .map_or(Ok(null), |v| text(v.to_string()));
    }
    if kind == oracledb::DB_TYPE_JSON {
        return row
            .take::<Option<oracledb::JsonValue>>(index)
            .map_err(super::driver_error)?
            .map_or(Ok(null), |v| text(json(v, 0)?.to_string()));
    }
    if kind == oracledb::DB_TYPE_VECTOR {
        return row
            .take::<Option<oracledb::Vector>>(index)
            .map_err(super::driver_error)?
            .map_or(Ok(null), |v| text(vector(v)?.to_string()));
    }
    row.take::<Option<String>>(index)
        .map_err(super::driver_error)?
        .map_or(Ok(null), text)
}
pub(super) struct Shape(AnyRow);
impl Shape {
    pub(super) fn new(columns: &[Column]) -> Self {
        Self(AnyRow {
            column_names: Arc::new(
                columns
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (c.column.name.clone(), i))
                    .collect(),
            ),
            columns: columns.iter().map(|c| c.column.clone()).collect(),
            values: Vec::new(),
        })
    }
}
pub(super) fn row(mut row: oracledb::Row, shape: &Shape) -> Result<AnyRow> {
    let mut values = Vec::with_capacity(row.columns().len());
    for index in 0..row.columns().len() {
        let kind = row.columns()[index].db_type();
        values.push(AnyValue {
            kind: cell(&mut row, index, kind)?,
        });
    }
    Ok(AnyRow {
        column_names: shape.0.column_names.clone(),
        columns: shape.0.columns.clone(),
        values,
    })
}
fn vector_data(value: oracledb::VectorData) -> Result<serde_json::Value> {
    use oracledb::VectorData::*;
    match value {
        Float32(v) => {
            if v.iter().any(|n| !n.is_finite()) {
                return Err(error("Oracle 向量含非有限数值"));
            }
            serde_json::to_value(v)
        }
        Float64(v) => {
            if v.iter().any(|n| !n.is_finite()) {
                return Err(error("Oracle 向量含非有限数值"));
            }
            serde_json::to_value(v)
        }
        Int8(v) => serde_json::to_value(v),
        Binary(v) => serde_json::to_value(v),
    }
    .map_err(error)
}
fn vector(value: oracledb::Vector) -> Result<serde_json::Value> {
    match value {
        oracledb::Vector::Dense(v) => vector_data(v),
        oracledb::Vector::Sparse(v) => Ok(
            serde_json::json!({"dimensions":v.num_dimensions(),"indices":v.indices(),"values":vector_data(v.values().clone())?}),
        ),
    }
}
fn json(value: oracledb::JsonValue, depth: usize) -> Result<serde_json::Value> {
    use oracledb::JsonValue as J;
    if depth > 64 {
        return Err(error("Oracle JSON 嵌套超过 64 层"));
    }
    Ok(match value {
        J::Null => serde_json::Value::Null,
        J::Boolean(v) => v.into(),
        J::String(v) => v.into(),
        J::Number(v) => serde_json::Value::Number(v.to_string().parse().map_err(error)?),
        J::BinaryDouble(v) => serde_json::Number::from_f64(v)
            .ok_or_else(|| error("Oracle JSON 非有限数值"))?
            .into(),
        J::BinaryFloat(v) => serde_json::Number::from_f64(f64::from(v))
            .ok_or_else(|| error("Oracle JSON 非有限数值"))?
            .into(),
        J::JsonArray(v) => serde_json::Value::Array(
            v.into_iter()
                .map(|v| json(v, depth + 1))
                .collect::<Result<_>>()?,
        ),
        J::JsonObject(v) => serde_json::Value::Object(
            v.into_iter()
                .map(|(k, v)| Ok((k, json(v, depth + 1)?)))
                .collect::<Result<_>>()?,
        ),
        J::Timestamp(v) => timestamp(v, true).into(),
        J::IntervalDS(v) => v.to_string().into(),
        J::IntervalYM(v) => v.to_string().into(),
        J::Raw(v) | J::JsonId(v) => v
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
            .into(),
        J::Vector(v) => vector(v)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_timestamp_preserves_local_date_offset_and_full_precision() -> anyhow::Result<()> {
        for (wire, expected) in [
            (
                OracleTimestamp::new_timestamp_tz(2024, 2, 28, 20, 34, 56, 123_456_789, 8, 0),
                "2024-02-29 04:34:56.123456789 +08:00",
            ),
            (
                OracleTimestamp::new_timestamp_tz(2024, 3, 1, 0, 15, 50, 1, 0, -30),
                "2024-02-29 23:45:50.000000001 -00:30",
            ),
        ] {
            assert_eq!(session_timestamp(wire, true)?, expected);
        }
        Ok(())
    }
    #[test]
    fn decimals_timezone_and_nonfinite_vectors_are_not_silently_changed() -> anyhow::Result<()> {
        let decimal = "12345678901234567890.123456789012345678";
        assert_eq!(
            json(
                oracledb::JsonValue::Number(decimal.parse::<OracleNumber>().map_err(error)?),
                0
            )?
            .to_string(),
            decimal
        );
        assert_eq!(
            timestamp(
                OracleTimestamp::new_timestamp_tz(2026, 10, 21, 12, 34, 56, 123_000_000, 0, -30),
                true
            ),
            "2026-10-21 12:34:56.123 -00:30"
        );
        assert_eq!(
            timestamp(
                OracleTimestamp::new_timestamp(2026, 10, 21, 12, 34, 56, 0),
                false
            ),
            "2026-10-21 12:34:56.0"
        );
        assert!(vector_data(oracledb::VectorData::Float32(vec![f32::NAN])).is_err());
        assert!(vector_data(oracledb::VectorData::Float64(vec![f64::INFINITY])).is_err());
        let mut nested = oracledb::JsonValue::Null;
        for _ in 0..65 {
            nested = oracledb::JsonValue::JsonArray(vec![nested]);
        }
        assert!(json(nested, 0).is_err());
        Ok(())
    }
}
