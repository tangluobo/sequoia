use super::*;
use tomato_core::workbench::{Column, TableDefinition};
pub(super) fn escape(value: &str) -> Result<String> {
    ensure!(
        value
            .chars()
            .all(|c| matches!(c, '\t' | '\r' | '\n')
                || (c >= ' ' && c != '\u{fffe}' && c != '\u{ffff}')),
        "文本包含 XML 不能表示的控制字符，请选择 JSON 或二进制格式"
    );
    Ok(value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;"))
}
fn delimited(
    out: &mut impl Write,
    cells: impl IntoIterator<Item = Option<String>>,
    options: &Options,
) -> Result<()> {
    for (index, cell) in cells.into_iter().enumerate() {
        if index > 0 {
            write!(out, "{}", options.delimiter)?;
        }
        if let Some(text) = cell {
            // Always quote non-NULL values; empty string and literal null marker remain distinct.
            write!(out, "\"{}\"", text.replace('"', "\"\""))?;
        } else {
            write!(out, "{}", options.null_text)?;
        }
    }
    out.write_all(b"\r\n")?;
    Ok(())
}
pub(super) fn write(
    spool: &Spool,
    columns: &[Column],
    options: &Options,
    control: &Control,
    out: &mut impl Write,
) -> Result<()> {
    let format = options.format;
    if matches!(format, Format::Text | Format::Csv) {
        ensure!(
            matches!(options.delimiter, ',' | '\t' | ';' | '|'),
            "分隔符无效"
        );
        ensure!(
            !options
                .null_text
                .contains(['\r', '\n', '"', options.delimiter]),
            "NULL 标记不能包含分隔符、引号或换行"
        );
        if options.bom {
            out.write_all(b"\xEF\xBB\xBF")?;
        }
        if options.header {
            delimited(out, columns.iter().map(|c| Some(c.name.clone())), options)?;
        }
    } else {
        match format {
            Format::Json => out.write_all(b"[\n")?,
            Format::Xml => {
                out.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<records>\n")?
            }
            Format::Html => {
                out.write_all(b"<!DOCTYPE html><html><head><meta charset=\"utf-8\"><style>table{border-collapse:collapse}td,th{border:1px solid #ccc;padding:4px;white-space:pre-wrap}</style></head><body><table>\n")?;
                if options.header {
                    out.write_all(b"<tr>")?;
                    for c in columns {
                        write!(out, "<th>{}</th>", escape(&c.name)?)?;
                    }
                    out.write_all(b"</tr>\n")?;
                }
            }
            Format::Sql => out.write_all(b"-- Tomato data export\n")?,
            _ => unreachable!(),
        }
    }
    let definition = TableDefinition {
        table: spool.schema.table.clone(),
        columns: columns.to_vec(),
        view: false,
        transactional: true,
        comments: Default::default(),
    };
    let mut first = true;
    spool.each(options, control, |row| {
        match format {
            Format::Text | Format::Csv => delimited(
                out,
                row.iter()
                    .map(|v| (!matches!(v, Cell::Null)).then(|| value(v))),
                options,
            )?,
            Format::Json => {
                if !first {
                    out.write_all(b",\n")?;
                }
                out.write_all(b"{")?;
                for (index, (col, cell)) in columns.iter().zip(row).enumerate() {
                    if index > 0 {
                        out.write_all(b",")?;
                    }
                    serde_json::to_writer(&mut *out, &col.name)?;
                    out.write_all(b":")?;
                    match cell {
                        Cell::Null => out.write_all(b"null")?,
                        Cell::Integer(n) => write!(out, "{n}")?,
                        Cell::Real(n) => {
                            ensure!(n.is_finite(), "JSON 不支持非有限数值");
                            write!(out, "{n}")?;
                        }
                        Cell::Boolean(v) => write!(out, "{v}")?,
                        _ => serde_json::to_writer(&mut *out, &value(cell))?,
                    }
                }
                out.write_all(b"}")?;
            }
            Format::Xml => {
                out.write_all(b" <record>")?;
                for (col, cell) in columns.iter().zip(row) {
                    if matches!(cell, Cell::Null) {
                        write!(
                            out,
                            "<field name=\"{}\" null=\"true\"/>",
                            escape(&col.name)?
                        )?;
                    } else {
                        write!(
                            out,
                            "<field name=\"{}\"{}>{}</field>",
                            escape(&col.name)?,
                            if matches!(cell, Cell::Binary(_)) {
                                " encoding=\"hex\""
                            } else {
                                ""
                            },
                            escape(&value(cell))?
                        )?;
                    }
                }
                out.write_all(b"</record>\n")?;
            }
            Format::Html => {
                out.write_all(b"<tr>")?;
                for cell in row {
                    if matches!(cell, Cell::Null) {
                        out.write_all(b"<td data-null=\"true\">NULL</td>")?;
                    } else {
                        write!(out, "<td>{}</td>", escape(&value(cell))?)?;
                    }
                }
                out.write_all(b"</tr>\n")?;
            }
            Format::Sql => {
                let sql = crate::database::inserts_sql(
                    &spool.schema.dialect,
                    &definition,
                    &[row.to_vec()],
                )?;
                out.write_all(sql.as_bytes())?;
                out.write_all(b"\n")?;
            }
            _ => unreachable!(),
        }
        first = false;
        Ok(())
    })?;
    match format {
        Format::Json => out.write_all(b"\n]\n")?,
        Format::Xml => out.write_all(b"</records>\n")?,
        Format::Html => out.write_all(b"</table></body></html>\n")?,
        _ => {}
    }
    out.flush()?;
    Ok(())
}
