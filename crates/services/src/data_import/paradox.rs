//! Independent reader of the documented Paradox table/block/MB structures.
//! Format references: Randy Beck, PxFORMAT; Kevin Mitchell, PARADOX4 (1996).
use super::*;
use std::io::{Read, Seek, SeekFrom};
fn short(b: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        b.get(at..at + 2)
            .context("Paradox 文件头不完整")?
            .try_into()?,
    ))
}
fn long(b: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        b.get(at..at + 4)
            .context("Paradox 文件不完整")?
            .try_into()?,
    ))
}
fn decode(source: &Source, cp: u16, bytes: &[u8]) -> Result<String> {
    if source.encoding == "自动" {
        use yore::code_pages::*;
        let value = match cp {
            437 => Some(CP437.decode(bytes)),
            850 => Some(CP850.decode(bytes)),
            852 => Some(CP852.decode(bytes)),
            865 => Some(CP865.decode(bytes)),
            866 => Some(CP866.decode(bytes)),
            861 => Some(CP861.decode(bytes)),
            _ => None,
        };
        if let Some(v) = value {
            return Ok(v.into_owned());
        }
    }
    let label = if source.encoding == "自动" {
        match cp {
            936 => "GBK".into(),
            932 => "Shift_JIS".into(),
            950 => "Big5".into(),
            65001 => "UTF-8".into(),
            0 => "windows-1252".into(),
            n => format!("windows-{n}"),
        }
    } else {
        source.encoding.clone()
    };
    let encoding = encoding_rs::Encoding::for_label(label.as_bytes())
        .context("Paradox 字符编码无法自动识别，请手动选择编码")?;
    let (text, _, errors) = encoding.decode(bytes);
    ensure!(!errors, "Paradox 字符无效，请检查字符编码");
    Ok(text.into_owned())
}
fn float(bytes: &[u8]) -> Result<f64> {
    ensure!(bytes.len() == 8, "Paradox 浮点字段长度无效");
    let mut b: [u8; 8] = bytes.try_into()?;
    if b[0] & 0x80 != 0 {
        b[0] ^= 0x80
    } else {
        for c in &mut b {
            *c = !*c
        }
    }
    let n = f64::from_be_bytes(b);
    ensure!(n.is_finite(), "Paradox 浮点数无效");
    Ok(n)
}
fn signed(bytes: &[u8]) -> Result<i64> {
    Ok(match bytes.len() {
        1 => (bytes[0] ^ 0x80) as i8 as i64,
        2 => {
            let mut b: [u8; 2] = bytes.try_into()?;
            b[0] ^= 0x80;
            i16::from_be_bytes(b) as i64
        }
        4 => {
            let mut b: [u8; 4] = bytes.try_into()?;
            b[0] ^= 0x80;
            i32::from_be_bytes(b) as i64
        }
        _ => anyhow::bail!("Paradox 整数字段长度无效"),
    })
}
fn memo(path: &Path, bytes: &[u8]) -> Result<Vec<u8>> {
    ensure!(bytes.len() >= 10, "Paradox BLOB 字段过短");
    let leader = bytes.len() - 10;
    let pointer = long(bytes, leader)?;
    let length = long(bytes, leader + 4)? as usize;
    ensure!(length <= MAX_ROW_BYTES, "Paradox BLOB 超过 16 MiB");
    if pointer == 0 {
        ensure!(length <= leader, "Paradox 内联 BLOB 长度无效");
        return Ok(bytes[..length].to_vec());
    }
    let upper = path.with_extension("MB");
    let lower = path.with_extension("mb");
    let file = if upper.exists() { upper } else { lower };
    let mut file = File::open(file).context("Paradox BLOB 需要同名的 .MB 文件")?;
    let offset = (pointer & 0xffff_ff00) as u64;
    let index = (pointer & 255) as usize;
    file.seek(SeekFrom::Start(offset))?;
    let mut head = [0; 12];
    file.read_exact(&mut head)?;
    let (start, available, modification) = if index == 255 {
        ensure!(head[0] == 2, "Paradox BLOB 块类型不匹配");
        (offset + 9, long(&head, 3)? as usize, short(&head, 7)?)
    } else {
        ensure!(head[0] == 3 && index < 64, "Paradox BLOB 指针无效");
        file.seek(SeekFrom::Start(offset + 12 + 5 * index as u64))?;
        let mut entry = [0; 5];
        file.read_exact(&mut entry)?;
        ensure!(
            entry[0] != 0 && entry[1] != 0 && entry[4] > 0 && entry[4] <= 16,
            "Paradox BLOB 指向已删除的数据"
        );
        let available = (entry[1] as usize - 1) * 16 + entry[4] as usize;
        (offset + entry[0] as u64 * 16, available, short(&entry, 2)?)
    };
    let expected = short(bytes, leader + 8)?;
    ensure!(
        expected == 0 || modification == expected,
        "Paradox BLOB 的修改序号不匹配"
    );
    ensure!(length <= available, "Paradox BLOB 长度超过存储块");
    ensure!(
        start
            .checked_add(length as u64)
            .is_some_and(|end| end <= file.metadata().map_or(0, |m| m.len())),
        "Paradox BLOB 超过文件边界"
    );
    let mut result = vec![0; length];
    file.seek(SeekFrom::Start(start))?;
    file.read_exact(&mut result)?;
    Ok(result)
}
fn cell(source: &Source, path: &Path, cp: u16, typ: u8, scale: u8, bytes: &[u8]) -> Result<Cell> {
    if bytes.iter().all(|b| *b == 0) {
        return Ok(Cell::Null);
    }
    Ok(match typ {
        1 => Cell::Text(decode(
            source,
            cp,
            &bytes[..bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len())],
        )?),
        3 | 4 | 0x16 => Cell::Integer(signed(bytes)?),
        5 | 6 => Cell::Real(float(bytes)?),
        9 => Cell::Boolean(signed(bytes)? != 0),
        2 => Cell::Text(
            chrono::NaiveDate::from_num_days_from_ce_opt(i32::try_from(signed(bytes)?)?)
                .context("Paradox 日期无效")?
                .format("%Y-%m-%d")
                .to_string(),
        ),
        0x14 => {
            let n = signed(bytes)?;
            ensure!((0..86_400_000).contains(&n), "Paradox 时间无效");
            Cell::Text(format!(
                "{:02}:{:02}:{:02}.{:03}",
                n / 3_600_000,
                n / 60_000 % 60,
                n / 1000 % 60,
                n % 1000
            ))
        }
        0x15 => {
            let millis = float(bytes)?.round() as i64 - 62_135_683_200_000;
            Cell::Text(
                chrono::DateTime::from_timestamp_millis(millis)
                    .context("Paradox 时间戳无效")?
                    .naive_utc()
                    .format("%Y-%m-%d %H:%M:%S%.f")
                    .to_string(),
            )
        }
        0x17 => {
            ensure!(bytes.len() == 17 && scale <= 32, "Paradox BCD 精度无效");
            let negative = bytes[0] & 128 == 0;
            let mut digits = String::new();
            for &byte in &bytes[1..] {
                let b = if negative { !byte } else { byte };
                let (hi, lo) = (b >> 4, b & 15);
                ensure!(hi <= 9 && lo <= 9, "Paradox BCD 中存在无效数字");
                digits.push((b'0' + hi) as char);
                digits.push((b'0' + lo) as char)
            }
            let integer = 32 - scale as usize;
            let mut s: String = if integer == 0 {
                "0".into()
            } else {
                let whole = digits[..integer].trim_start_matches('0');
                if whole.is_empty() {
                    "0".into()
                } else {
                    whole.into()
                }
            };
            if scale > 0 {
                s.push('.');
                s.push_str(&digits[integer..])
            }
            if negative {
                s.insert(0, '-')
            }
            Cell::Text(s)
        }
        0x0c => Cell::Text(decode(source, cp, &memo(path, bytes)?)?),
        0x0d..=0x10 => Cell::Binary(memo(path, bytes)?),
        0x18 => Cell::Binary(bytes.to_vec()),
        _ => anyhow::bail!("Paradox 字段类型 0x{typ:02x} 尚无法读取"),
    })
}
pub(super) fn read(
    source: &Source,
    path: &Path,
    directory: &Path,
    output: &mut Catalog,
    control: &Control,
) -> Result<()> {
    let mut input = File::open(path)?;
    let mut prefix = [0; 120];
    input.read_exact(&mut prefix)?;
    let record = short(&prefix, 0)? as usize;
    let header_size = short(&prefix, 2)? as usize;
    let count = long(&prefix, 6)? as u64;
    let fields = short(&prefix, 0x21)? as usize;
    let version = prefix[0x39];
    ensure!(
        matches!(prefix[4], 0 | 2) && (3..=12).contains(&version),
        "请选择 Paradox 数据表文件（不支持索引文件）"
    );
    ensure!(
        if version < 5 {
            long(&prefix, 0x25)? == 0
        } else {
            matches!(long(&prefix, 0x25)?, 0 | 0xff00_ff00) && long(&prefix, 0x5c)? == 0
        },
        "此 Paradox 表已加密，请通过具有相应驱动的 ODBC 数据源导入"
    );
    ensure!(
        header_size >= 120 && record > 0 && fields > 0 && fields <= MAX_COLUMNS,
        "Paradox 表头无效"
    );
    let block_size = prefix[5] as usize * 1024;
    ensure!((1024..=32768).contains(&block_size), "Paradox 块大小无效");
    let mut header = vec![0; header_size];
    input.seek(SeekFrom::Start(0))?;
    input.read_exact(&mut header)?;
    let base = if version >= 5 { 120 } else { 88 };
    let mut descriptors = Vec::new();
    for i in 0..fields {
        let at = base + 2 * i;
        let typ = *header.get(at).context("Paradox 字段描述缺失")?;
        let size = *header.get(at + 1).context("Paradox 字段长度缺失")?;
        descriptors.push((typ, size, if typ == 0x17 { 17 } else { size as usize }));
    }
    ensure!(
        descriptors.iter().map(|f| f.2).sum::<usize>() == record,
        "Paradox 字段长度与记录长度不符"
    );
    let cp = if version >= 5 {
        short(&header, 0x6a)?
    } else {
        437
    };
    let mut at = base + 6 * fields + 4 + if version >= 12 { 261 } else { 79 };
    let mut columns = Vec::new();
    for _ in 0..fields {
        let tail = header.get(at..).context("Paradox 字段名缺失")?;
        let n = tail
            .iter()
            .position(|b| *b == 0)
            .context("Paradox 字段名不完整")?;
        ensure!(n > 0, "Paradox 字段名为空");
        columns.push(field(decode(source, cp, &tail[..n])?));
        at += n + 1;
    }
    let mut sink = Sink::new(
        directory,
        output,
        name(path),
        path.display().to_string(),
        columns,
    )?;
    let mut block = short(&header, 0x0e)?;
    let mut visited = std::collections::HashSet::new();
    let mut total = 0;
    let mut data = vec![0; block_size];
    while block != 0 {
        control.check()?;
        ensure!(visited.insert(block), "Paradox 数据块形成循环");
        let offset = header_size as u64 + (block as u64 - 1) * block_size as u64;
        input.seek(SeekFrom::Start(offset))?;
        input.read_exact(&mut data)?;
        block = short(&data, 0)?;
        let used = short(&data, 4)? as i16;
        let rows = if used < 0 {
            0
        } else {
            used as usize / record + 1
        };
        ensure!(6 + rows * record <= data.len(), "Paradox 记录越过块边界");
        for bytes in data[6..6 + rows * record].chunks_exact(record) {
            total += 1;
            if !included(source, total) {
                continue;
            }
            let mut values = Vec::new();
            let mut offset = 0;
            for &(typ, scale, length) in &descriptors {
                values.push(cell(
                    source,
                    path,
                    cp,
                    typ,
                    scale,
                    &bytes[offset..offset + length],
                )?);
                offset += length;
            }
            sink.push(values, control)?;
        }
    }
    ensure!(
        total == count,
        "Paradox 文件头记录数 {count} 与实际记录数 {total} 不符"
    );
    sink.finish(output)
}
