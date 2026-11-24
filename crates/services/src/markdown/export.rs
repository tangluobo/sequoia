//! Java-compatible HTML document, table and code rendering.
use super::{
    MAX_SOURCE_BYTES,
    resource::java_trim,
    tables::{self, MAX_COLUMNS, Segment, cells},
};
use anyhow::{Result, ensure};
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd, html::write_html_io};
use std::{
    io::{self, BufWriter, Write},
    path::Path,
};

const CSS: &str = concat!(
    "body{font-family:'Segoe UI','Microsoft YaHei',-apple-system,sans-serif;color:#24292e;",
    "font-size:15px;line-height:1.6;max-width:780px;margin:16px auto;padding:0 16px;}",
    "h1{font-size:2em;margin:0.6em 0 0.3em;}h2{font-size:1.5em;}h3{font-size:1.25em;}",
    "h4{font-size:1em;}h5{font-size:0.875em;}h6{font-size:0.85em;color:#666;}",
    "h1,h2,h3,h4,h5,h6{color:#1163a6;font-weight:600;}",
    "a{color:#1a73e8;text-decoration:none;}a:hover{text-decoration:underline;}",
    "code{background:#f6f8fa;padding:2px 6px;border-radius:3px;font-family:Consolas,'Courier New',monospace;color:#c725e9;font-size:90%;}",
    "pre{background:#f6f8fa;border:1px solid #e0e0e0;border-radius:4px;padding:10px;overflow:auto;}",
    "pre code{background:none;color:#24292e;padding:0;font-size:12px;}",
    "blockquote{border-left:3px solid #1a73e8;margin:0.4em 0;padding:0.2em 0 0.2em 12px;color:#666;}",
    "table{border-collapse:collapse;margin:0.5em 0;}",
    "th,td{border:1px solid #dfe2e5;padding:6px 10px;}",
    "th{background:#f6f8fa;font-weight:bold;}",
    "img{max-width:100%;}hr{border:none;border-top:1px solid #e0e0e0;margin:1em 0;}",
);
const MAX_OUTPUT: usize = 64 * 1024 * 1024;
const MAX_CODE_HTML: usize = 16 * 1024 * 1024;

pub fn html_filename(title: &str) -> String {
    let title = java_trim(title);
    let title = if title.trim().is_empty() {
        "导出"
    } else {
        title
    };
    let mut name = title
        .chars()
        .map(|ch| {
            if "\\/:*?\"<>|".contains(ch) || ch.is_control() {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();
    name.push_str(".html");
    name
}

/// Publish only after rendering and flushing a complete UTF-8 document.
pub fn export_html(source: &str, title: &str, path: &Path) -> Result<()> {
    ensure!(source.len() <= MAX_SOURCE_BYTES, "Markdown 最多 8 MiB");
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut buffer = BufWriter::new(temporary.as_file_mut());
        render(source, title, &mut buffer)?;
        buffer.flush()?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

struct Limit<W> {
    writer: W,
    left: usize,
    message: &'static str,
}
impl<W: Write> Write for Limit<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.left {
            return Err(io::Error::other(self.message));
        }
        let written = self.writer.write(bytes)?;
        self.left -= written;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

pub(super) fn render(source: &str, title: &str, output: impl Write) -> Result<()> {
    ensure!(source.len() <= MAX_SOURCE_BYTES, "Markdown 最多 8 MiB");
    let mut out = Limit {
        writer: output,
        left: MAX_OUTPUT,
        message: "HTML 输出超过 64 MiB",
    };
    out.write_all(b"<!DOCTYPE html>\n<html lang=\"zh\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>")?;
    escape(&mut out, title)?;
    write!(out, "</title>\n<style>{CSS}</style>\n</head>\n<body>\n")?;

    for segment in tables::segments(source) {
        match segment {
            Segment::Text(prose) => write_prose(&mut out, prose)?,
            Segment::Table(segment) => table(
                &mut out,
                segment.header,
                segment.divider,
                segment.body.split_inclusive('\n'),
            )?,
        }
    }
    out.write_all(b"</body>\n</html>\n")?;
    Ok(())
}

fn alignment(value: &str) -> &'static str {
    match (value.starts_with(':'), value.ends_with(':')) {
        (true, true) => "center",
        (_, true) => "right",
        _ => "left",
    }
}

fn table<'a>(
    out: &mut impl Write,
    header: &str,
    divider: &str,
    rows: impl Iterator<Item = &'a str>,
) -> Result<()> {
    let mut alignments = Vec::new();
    let mut spec = cells(divider);
    out.write_all(b"<table>\n<thead>\n<tr>")?;
    for label in cells(header) {
        ensure!(alignments.len() < MAX_COLUMNS, "Markdown 表格超过 4096 列");
        let align = spec.next().as_deref().map_or("left", alignment);
        alignments.push(align);
        cell(out, "th", align, &label)?;
    }
    out.write_all(b"</tr>\n</thead>\n<tbody>\n")?;
    for row in rows {
        out.write_all(b"<tr>")?;
        let mut row = cells(row);
        for align in &alignments {
            cell(out, "td", align, row.next().as_deref().unwrap_or(""))?;
        }
        out.write_all(b"</tr>\n")?;
    }
    out.write_all(b"</tbody>\n</table>\n")?;
    Ok(())
}

fn cell(out: &mut impl Write, tag: &str, align: &str, value: &str) -> io::Result<()> {
    write!(out, "<{tag} style=\"text-align:{align}\">")?;
    escape(out, value)?;
    write!(out, "</{tag}>")
}

fn escape(out: &mut impl Write, text: &str) -> io::Result<()> {
    let mut last = 0;
    for (index, byte) in text.bytes().enumerate() {
        let entity = match byte {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            _ => continue,
        };
        out.write_all(&text.as_bytes()[last..index])?;
        out.write_all(entity.as_bytes())?;
        last = index + 1;
    }
    out.write_all(&text.as_bytes()[last..])
}

fn write_prose(out: &mut impl Write, prose: &str) -> Result<()> {
    let mut parser = Parser::new(prose);
    let mut failure = None;
    let events = std::iter::from_fn(|| {
        if failure.is_some() {
            return None;
        }
        let event = parser.next()?;
        let Event::Start(Tag::CodeBlock(kind)) = event else {
            return Some(event);
        };
        let language = match kind {
            CodeBlockKind::Fenced(info) => info,
            CodeBlockKind::Indented => "".into(),
        };
        let mut text = String::new();
        for event in parser.by_ref() {
            match event {
                Event::Text(line) => text.push_str(&line),
                Event::End(TagEnd::CodeBlock) => break,
                _ => {}
            }
        }
        match code(&text, &language) {
            Ok(html) => Some(Event::Html(html.into())),
            Err(error) => {
                failure = Some(error);
                None
            }
        }
    });
    write_html_io(out, events)?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(())
}

fn code(text: &str, language: &str) -> Result<String> {
    let mut out = Limit {
        writer: Vec::new(),
        left: MAX_CODE_HTML,
        message: "单个 Markdown 代码块的 HTML 超过 16 MiB",
    };
    out.write_all(b"<pre><code>")?;
    tomato_core::syntax::markdown::visit(text, language, |token| {
        let [r, g, b] = token.kind.rgb();
        write!(out, "<span style=\"color:#{r:02X}{g:02X}{b:02X}\">")?;
        escape(&mut out, &text[token.range])?;
        out.write_all(b"</span>")
    })?;
    out.write_all(b"</code></pre>\n")?;
    Ok(String::from_utf8(out.writer)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Original {
        name: String,
        title: String,
        source: String,
        filename: String,
        html: String,
    }

    #[test]
    fn original_markdown_html_files_and_default_names_match() -> Result<()> {
        let cases: Vec<Original> = serde_json::from_str(include_str!(
            "../../tests/fixtures/markdown-html-original.json"
        ))?;
        let directory = tempfile::tempdir()?;
        assert_eq!(cases.len(), 6);
        for case in cases {
            assert_eq!(html_filename(&case.title), case.filename, "{}", case.name);
            let path = directory.path().join(format!("{}.html", case.name));
            export_html(&case.source, &case.title, &path)?;
            let actual = std::fs::read_to_string(path)?;
            if actual != case.html {
                let offset = actual
                    .bytes()
                    .zip(case.html.bytes())
                    .take_while(|(left, right)| left == right)
                    .count();
                let sample = |source: &str| {
                    String::from_utf8_lossy(
                        &source.as_bytes()[offset..source.len().min(offset + 180)],
                    )
                    .into_owned()
                };
                anyhow::bail!(
                    "{} differs at byte {offset}: actual {:?}, Java {:?}",
                    case.name,
                    sample(&actual),
                    sample(&case.html)
                );
            }
        }
        Ok(())
    }

    #[test]
    fn incomplete_render_does_not_replace_an_existing_file() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("报告.html");
        std::fs::write(&path, b"previous content")?;
        let bad = format!("before\n\n|{}\n---\n", "x|".repeat(MAX_COLUMNS + 1));
        let error = export_html(&bad, "报告", &path).unwrap_err();
        assert!(error.to_string().contains("4096"));
        assert_eq!(std::fs::read(&path)?, b"previous content");
        assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
        export_html("# 成功", "报告", &path)?;
        assert!(std::fs::read_to_string(path)?.contains("<h1>成功</h1>"));
        Ok(())
    }
}
