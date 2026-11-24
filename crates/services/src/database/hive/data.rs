//! Preserve NULL, binary, wide integers and decimal text across both HS2 row formats.
use super::wire::Value;
use anyhow::{Context, Result, bail, ensure};
use tomato_core::workbench::Cell;
#[derive(Clone)]
pub(super) struct Column {
    pub name: String,
    pub kind: i32,
    pub jdbc: i32,
    pub data_type: String,
}
fn type_name(kind: i32) -> (&'static str, i32) {
    match kind {
        0 => ("BOOLEAN", 16),
        1 => ("TINYINT", -6),
        2 => ("SMALLINT", 5),
        3 => ("INT", 4),
        4 => ("BIGINT", -5),
        5 => ("FLOAT", 6),
        6 => ("DOUBLE", 8),
        7 => ("STRING", 12),
        8 => ("TIMESTAMP", 93),
        9 => ("BINARY", -2),
        10 => ("ARRAY", 2003),
        11 => ("MAP", 2000),
        12 => ("STRUCT", 2002),
        13 => ("UNIONTYPE", 2000),
        15 => ("DECIMAL", 3),
        16 => ("NULL", 0),
        17 => ("DATE", 91),
        18 => ("VARCHAR", 12),
        19 => ("CHAR", 1),
        20 => ("INTERVAL_YEAR_MONTH", 1111),
        21 => ("INTERVAL_DAY_TIME", 1111),
        22 => ("TIMESTAMP WITH LOCAL TIME ZONE", 2014),
        _ => ("STRING", 12),
    }
}
pub(super) fn schema(schema: Value) -> Result<Vec<Column>> {
    let columns = schema.required(1)?.list()?;
    ensure!(columns.len() <= 4096, "Hive 查询列超过 4096 项");
    columns
        .iter()
        .map(|column| {
            let name = column.required(1)?.text()?.to_owned();
            let types = column.required(2)?.required(1)?.list()?;
            let first = types.first().context("Hive 列类型为空")?;
            let kind = if let Some(primitive) = first.field(1) {
                i32::try_from(primitive.required(1)?.integer()?)?
            } else if first.field(2).is_some() {
                10
            } else if first.field(3).is_some() {
                11
            } else if first.field(4).is_some() {
                12
            } else {
                13
            };
            let (data_type, jdbc) = type_name(kind);
            Ok(Column {
                name,
                kind,
                jdbc,
                data_type: data_type.into(),
            })
        })
        .collect()
}
fn cell(value: &Value, column: &Column) -> Result<Cell> {
    Ok(match value {
        Value::Bool(v) => Cell::Boolean(*v),
        Value::Byte(v) => Cell::Integer(i64::from(*v)),
        Value::I16(v) => Cell::Integer(i64::from(*v)),
        Value::I32(v) => Cell::Integer(i64::from(*v)),
        Value::I64(v) => Cell::Integer(*v),
        Value::Double(v) if v.is_finite() => Cell::Real(*v),
        Value::Double(v) => Cell::Text(v.to_string()),
        Value::Bytes(v) if column.kind == 9 => Cell::Binary(v.clone()),
        Value::Bytes(v) => {
            Cell::Text(String::from_utf8(v.clone()).context("Hive 单元格不是 UTF-8")?)
        }
        _ => bail!("Hive 返回了无效的单元格类型"),
    })
}
pub(super) fn rows(rows: Value, columns: &[Column]) -> Result<Vec<Vec<Cell>>> {
    if let Some(columnar) = rows.field(3) {
        let vectors = columnar.list()?;
        ensure!(vectors.len() == columns.len(), "Hive 返回的列数不一致");
        let mut output: Vec<Vec<Cell>> = Vec::new();
        for (index, (vector, column)) in vectors.iter().zip(columns).enumerate() {
            let wrapper = (1..=8)
                .find_map(|id| vector.field(id))
                .context("Hive 列向量缺少值")?;
            let values = wrapper.required(1)?.list()?;
            let nulls = wrapper.required(2)?.bytes()?;
            ensure!(values.len() <= 256, "Hive 单批结果超过请求的 256 行");
            if index == 0 {
                output = (0..values.len())
                    .map(|_| Vec::with_capacity(columns.len()))
                    .collect();
            }
            ensure!(values.len() == output.len(), "Hive 列向量行数不一致");
            for (row, value) in values.iter().enumerate() {
                output[row].push(
                    if nulls
                        .get(row / 8)
                        .is_some_and(|v| v & (1 << (row % 8)) != 0)
                    {
                        Cell::Null
                    } else {
                        cell(value, column)?
                    },
                );
            }
        }
        return Ok(output);
    }
    ensure!(
        rows.field(4).is_none(),
        "Hive 未按协商协议返回结果，请关闭服务端二进制结果序列化"
    );
    let rows = rows.required(2)?.list()?;
    ensure!(rows.len() <= 256, "Hive 单批结果超过请求的 256 行");
    rows.iter()
        .map(|row| {
            let values = row.required(1)?.list()?;
            ensure!(values.len() == columns.len(), "Hive 返回的列数不一致");
            values
                .iter()
                .zip(columns)
                .map(|(value, column)| {
                    let value = (1..=7)
                        .find_map(|id| value.field(id))
                        .context("Hive 行单元格缺少类型")?;
                    value
                        .field(1)
                        .map_or(Ok(Cell::Null), |value| cell(value, column))
                })
                .collect()
        })
        .collect()
}
