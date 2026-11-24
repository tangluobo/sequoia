//! dBASE III records, GBK language driver. Format limits fail before publication.
use super::*;
use tomato_core::workbench::Column;
pub(super) fn write(
    spool: &Spool,
    columns: &[Column],
    options: &Options,
    control: &Control,
    out: &mut File,
) -> Result<()> {
    ensure!(columns.len() <= 128, "DBF 最多导出 128 个字段，请减少字段");
    ensure!(spool.records <= u32::MAX as u64, "DBF 记录数超过格式上限");
    let encode = |text: &str| -> Result<Vec<u8>> {
        let (bytes, _, errors) = encoding_rs::GBK.encode(text);
        ensure!(
            !errors,
            "DBF 的 GBK 编码不能表示部分字符，请选择 UTF-8 文本或 Excel"
        );
        Ok(bytes.into_owned())
    };
    let names = columns
        .iter()
        .map(|column| {
            let name = encode(&column.name)?;
            ensure!(
                name.len() <= 10,
                "DBF 字段名 {} 超过 10 字节，请选择其他格式或在查询中设置短别名",
                column.name
            );
            Ok(name)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut widths = vec![1usize; columns.len()];
    spool.each(options, control, |row| {
        for ((cell, width), column) in row.iter().zip(&mut widths).zip(columns) {
            let text = if matches!(cell, Cell::Null) {
                options.null_text.clone()
            } else {
                value(cell)
            };
            let bytes = encode(&text)?;
            ensure!(
                bytes.len() <= 254,
                "DBF 字段 {} 超过 254 字节，请使用 Excel 或文本格式，未截断数据",
                column.name
            );
            *width = (*width).max(bytes.len());
        }
        Ok(())
    })?;
    let record_size = 1 + widths.iter().sum::<usize>();
    ensure!(record_size <= 65535, "DBF 单条记录超过 65535 字节");
    let mut header = [0u8; 32];
    header[0] = 3;
    use chrono::Datelike;
    let today = chrono::Local::now().date_naive();
    header[1] = (today.year() - 1900) as u8;
    header[2] = today.month() as u8;
    header[3] = today.day() as u8;
    header[4..8].copy_from_slice(&(spool.records as u32).to_le_bytes());
    header[8..10].copy_from_slice(&((32 + 32 * columns.len() + 1) as u16).to_le_bytes());
    header[10..12].copy_from_slice(&(record_size as u16).to_le_bytes());
    header[29] = 0x7a;
    out.write_all(&header)?;
    for (name, width) in names.iter().zip(&widths) {
        let mut field = [0u8; 32];
        field[..name.len()].copy_from_slice(name);
        field[11] = b'C';
        field[16] = *width as u8;
        out.write_all(&field)?;
    }
    out.write_all(&[0x0d])?;
    spool.each(options, control, |row| {
        out.write_all(b" ")?;
        for (cell, width) in row.iter().zip(&widths) {
            let text = if matches!(cell, Cell::Null) {
                options.null_text.clone()
            } else {
                value(cell)
            };
            let bytes = encode(&text)?;
            out.write_all(&bytes)?;
            out.write_all(&vec![b' '; width - bytes.len()])?;
        }
        Ok(())
    })?;
    out.write_all(&[0x1a])?;
    Ok(())
}
