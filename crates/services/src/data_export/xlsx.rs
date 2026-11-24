use super::*;
use tomato_core::workbench::Column;
use zip::{ZipWriter, write::SimpleFileOptions};
fn escaped(text: &str) -> Result<String> {
    let mut encoded = String::with_capacity(text.len());
    for (index, ch) in text.char_indices() {
        let rest = &text[index..];
        if ch == '_'
            && rest.len() >= 7
            && rest.as_bytes()[1] == b'x'
            && rest.as_bytes()[2..6].iter().all(u8::is_ascii_hexdigit)
            && rest.as_bytes()[6] == b'_'
        {
            encoded.push_str("_x005F_");
        } else if (ch < ' ' && !matches!(ch, '\t' | '\n')) || matches!(ch, '\u{fffe}' | '\u{ffff}')
        {
            encoded.push_str(&format!("_x{:04X}_", ch as u32));
        } else {
            encoded.push(ch);
        }
    }
    super::text::escape(&encoded)
}
fn name(mut index: usize) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (index % 26) as u8) as char);
        if index < 26 {
            return s;
        }
        index = index / 26 - 1;
    }
}
pub(super) fn write(
    spool: &Spool,
    columns: &[Column],
    options: &Options,
    control: &Control,
    out: &mut File,
) -> Result<()> {
    ensure!(
        spool.records + u64::from(options.header) <= 1_048_576,
        "XLSX 每个工作表最多 1,048,576 行，请缩小范围或使用 CSV"
    );
    let mut zip = ZipWriter::new(out);
    let settings =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (path, data) in [
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="导出数据" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        ),
    ] {
        zip.start_file(path, settings)?;
        zip.write_all(data.as_bytes())?;
    }
    zip.start_file("xl/worksheets/sheet1.xml", settings)?;
    zip.write_all(b"<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>")?;
    let mut index = 1u64;
    let mut row = |cells: &[Cell]| -> Result<()> {
        write!(zip, "<row r=\"{index}\">")?;
        for (column, cell) in cells.iter().enumerate() {
            let id = format!("{}{index}", name(column));
            match cell {
                Cell::Null => {}
                Cell::Boolean(v) => {
                    write!(zip, "<c r=\"{id}\" t=\"b\"><v>{}</v></c>", u8::from(*v))?
                }
                Cell::Integer(v) if v.unsigned_abs() < 1_000_000_000_000_000 => {
                    write!(zip, "<c r=\"{id}\"><v>{v}</v></c>")?
                }
                Cell::Real(v) if v.is_finite() => write!(zip, "<c r=\"{id}\"><v>{v}</v></c>")?,
                _ => {
                    let text = value(cell);
                    ensure!(
                        text.encode_utf16().count() <= 32767,
                        "Excel 单元格超过 32767 个字符，请选择文本或 JSON"
                    );
                    write!(
                        zip,
                        "<c r=\"{id}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                        escaped(&text)?
                    )?;
                }
            }
        }
        zip.write_all(b"</row>")?;
        index += 1;
        Ok(())
    };
    if options.header {
        row(&columns
            .iter()
            .map(|c| Cell::Text(c.name.clone()))
            .collect::<Vec<_>>())?;
    }
    spool.each(options, control, &mut row)?;
    zip.write_all(b"</sheetData></worksheet>")?;
    zip.finish()?;
    Ok(())
}
