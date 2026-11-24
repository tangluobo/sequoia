use super::*;
use calamine::{Data, Reader};
use std::io::{Read, Seek, SeekFrom};
pub(super) fn excel(
    source: &Source,
    path: &Path,
    directory: &Path,
    output: &mut Catalog,
    control: &Control,
) -> Result<()> {
    ensure!(
        path.metadata()?.len() <= 64 * 1024 * 1024,
        "Excel 文件超过 64 MiB，请分成较小的工作簿或转为 CSV 流式导入"
    );
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("xlsx"))
    {
        let zip = zip::ZipArchive::new(File::open(path)?)?;
        ensure!(
            zip.len() <= 20_000
                && zip
                    .decompressed_size()
                    .is_some_and(|n| n <= 256 * 1024 * 1024),
            "Excel 解压数据超过 256 MiB"
        );
    }
    let mut book = calamine::open_workbook_auto(path)?;
    let sheets = book.sheet_names().to_vec();
    for name in &source.tables {
        ensure!(sheets.contains(name), "工作表不存在：{name}");
    }
    for name in sheets {
        control.check()?;
        if !source.tables.is_empty() && !source.tables.contains(&name) {
            continue;
        }
        let range = book.worksheet_range(&name)?;
        if range.is_empty() {
            continue;
        }
        ensure!(
            range.width() <= MAX_COLUMNS,
            "工作表字段超过 {MAX_COLUMNS} 列"
        );
        ensure!(
            range.get_size().0.saturating_mul(range.get_size().1) <= 4_000_000,
            "工作表超过 400 万个单元格，请转为 CSV 流式导入"
        );
        let start = range.start().unwrap_or_default();
        let mut names = Vec::new();
        let mut sink = None;
        for (i, row) in range.rows().enumerate() {
            control.check()?;
            let number = start.0 as u64 + i as u64 + 1;
            if number == source.header_row as u64 {
                names = row
                    .iter()
                    .enumerate()
                    .map(|(c, v)| {
                        field(if v.to_string().is_empty() {
                            format!("f{}", start.1 as usize + c + 1)
                        } else {
                            v.to_string()
                        })
                    })
                    .collect();
                continue;
            }
            if !included(source, number)
                || source.skip_empty && row.iter().all(|v| matches!(v, Data::Empty))
            {
                continue;
            }
            if sink.is_none() {
                if names.is_empty() {
                    names = (0..range.width())
                        .map(|c| field(format!("f{}", start.1 as usize + c + 1)))
                        .collect()
                }
                ensure!(
                    names
                        .iter()
                        .map(|f| &f.name)
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == names.len(),
                    "工作表 {name} 的字段名重复"
                );
                sink = Some(Sink::new(
                    directory,
                    output,
                    name.clone(),
                    path.display().to_string(),
                    std::mem::take(&mut names),
                )?);
            }
            let values = row
                .iter()
                .map(|v| match v {
                    Data::Empty => Cell::Null,
                    Data::String(s) => Cell::Text(if source.trim {
                        s.trim().into()
                    } else {
                        s.clone()
                    }),
                    Data::Int(n) => Cell::Integer(*n),
                    Data::Float(n) => Cell::Real(*n),
                    Data::Bool(b) => Cell::Boolean(*b),
                    Data::DateTime(v) => Cell::Text(v.as_datetime().map_or_else(
                        || v.to_string(),
                        |d| d.format("%Y-%m-%d %H:%M:%S%.f").to_string(),
                    )),
                    Data::DateTimeIso(s) | Data::DurationIso(s) => Cell::Text(s.clone()),
                    Data::Error(e) => Cell::Text(e.to_string()),
                })
                .collect();
            sink.as_mut().unwrap().push(values, control)?;
        }
        if let Some(sink) = sink {
            sink.finish(output)?
        } else if !names.is_empty() {
            Sink::new(directory, output, name, path.display().to_string(), names)?.finish(output)?
        }
    }
    Ok(())
}
pub(super) fn dbase(
    source: &Source,
    path: &Path,
    directory: &Path,
    output: &mut Catalog,
    control: &Control,
) -> Result<()> {
    let mut reader = if source.encoding == "自动" {
        dbase::Reader::from_path(path)?
    } else {
        let encoding = encoding_rs::Encoding::for_label(source.encoding.as_bytes())
            .context("DBF 字符编码无效")?;
        dbase::Reader::from_path_with_encoding(path, dbase::encoding::EncodingRs::from(encoding))?
    };
    let mut raw = File::open(path)?;
    let mut header = [0u8; 32];
    raw.read_exact(&mut header)?;
    let length = u16::from_le_bytes([header[10], header[11]]) as usize;
    ensure!(length > 0, "DBF 记录长度无效");
    let mut descriptors = Vec::new();
    loop {
        let mut first = [0u8; 1];
        raw.read_exact(&mut first)?;
        if first[0] == 0x0d {
            break;
        }
        let mut desc = [0u8; 32];
        desc[0] = first[0];
        raw.read_exact(&mut desc[1..])?;
        descriptors.push((desc[11], desc[16] as usize));
        ensure!(descriptors.len() <= MAX_COLUMNS, "DBF 字段过多");
    }
    let fields = reader
        .fields()
        .iter()
        .map(|f| field(f.name().into()))
        .collect::<Vec<_>>();
    ensure!(fields.len() == descriptors.len(), "DBF 字段描述不一致");
    let names = fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
    let mut sink = Sink::new(
        directory,
        output,
        name(path),
        path.display().to_string(),
        fields,
    )?;
    raw.seek(SeekFrom::Start(
        reader.header().offset_to_first_record as u64,
    ))?;
    let count = reader.header().num_records;
    let mut records = reader.iter_records();
    let mut bytes = vec![0; length];
    let mut number = 0;
    for _ in 0..count {
        control.check()?;
        raw.read_exact(&mut bytes)?;
        if bytes[0] == b'*' {
            continue;
        }
        ensure!(bytes[0] == b' ', "DBF 记录标记无效");
        let record = records.next().context("DBF 记录数与文件头不一致")??;
        number += 1;
        if !included(source, number) {
            continue;
        }
        let mut values = Vec::new();
        let mut offset = 1;
        for ((typ, len), name) in descriptors.iter().zip(&names) {
            ensure!(offset + len <= bytes.len(), "DBF 字段越过记录边界");
            let raw = &bytes[offset..offset + len];
            offset += len;
            let v = record.get(name).context("DBF 字段不存在")?;
            use dbase::FieldValue as V;
            let value = match *typ {
                b'N' | b'F' => {
                    let s = std::str::from_utf8(raw)?.trim();
                    if s.is_empty() || s.bytes().all(|b| b == b'*') {
                        Cell::Null
                    } else {
                        Cell::Text(s.into())
                    }
                }
                b'Y' if raw.len() == 8 => {
                    let n = i64::from_le_bytes(raw.try_into()?);
                    let sign = if n < 0 { "-" } else { "" };
                    let n = n.unsigned_abs();
                    Cell::Text(format!("{sign}{}.{:04}", n / 10000, n % 10000))
                }
                b'T' if raw.len() == 8 => {
                    let day = u32::from_le_bytes(raw[..4].try_into()?) as i64;
                    let millis = u32::from_le_bytes(raw[4..].try_into()?) as i64;
                    if day == 0 {
                        Cell::Null
                    } else {
                        let d = chrono::DateTime::from_timestamp_millis(
                            (day - 2_440_588) * 86_400_000 + millis,
                        )
                        .context("DBF 日期时间无效")?;
                        Cell::Text(d.naive_utc().format("%Y-%m-%d %H:%M:%S%.f").to_string())
                    }
                }
                _ => match v {
                    V::Character(s) => s.as_ref().map_or(Cell::Null, |s| Cell::Text(s.clone())),
                    V::Memo(s) => Cell::Text(s.clone()),
                    V::Logical(b) => b.map_or(Cell::Null, Cell::Boolean),
                    V::Date(d) => d.map_or(Cell::Null, |d| {
                        Cell::Text(format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day()))
                    }),
                    V::Integer(n) => Cell::Integer(*n as i64),
                    V::Double(n) | V::Currency(n) => Cell::Real(*n),
                    V::Numeric(n) => n.map_or(Cell::Null, Cell::Real),
                    V::Float(n) => n.map_or(Cell::Null, |n| Cell::Real(n as f64)),
                    V::DateTime(d) => Cell::Text(
                        chrono::DateTime::from_timestamp(d.to_unix_timestamp(), 0)
                            .context("DBF 日期时间无效")?
                            .naive_utc()
                            .to_string(),
                    ),
                },
            };
            values.push(value);
        }
        sink.push(values, control)?;
    }
    sink.finish(output)
}
