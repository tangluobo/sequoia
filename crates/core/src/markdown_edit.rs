//! Editing rules from the original MarkdownEditorPane, independent of the widget.
use anyhow::{Result, bail};
use regex::Regex;
use std::{ops::Range, sync::LazyLock};

const MAX_EDIT: usize = 8 * 1024 * 1024;
pub struct Edit {
    pub text: String,
    /// Unicode scalar indices, matching egui's cursor model.
    pub selection: Range<usize>,
}
fn offset(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(i, _)| i)
}
fn replace(text: &str, range: Range<usize>, value: &str, selection: Range<usize>) -> Result<Edit> {
    if text
        .len()
        .saturating_sub(range.len())
        .saturating_add(value.len())
        > MAX_EDIT
    {
        bail!("编辑结果超过 8 MiB");
    }
    let mut result = String::with_capacity(text.len() - range.len() + value.len());
    result.push_str(&text[..range.start]);
    result.push_str(value);
    result.push_str(&text[range.end..]);
    Ok(Edit {
        text: result,
        selection,
    })
}
pub fn indent(text: &str, selection: Range<usize>, outdent: bool) -> Result<Edit> {
    let (start, end) = (offset(text, selection.start), offset(text, selection.end));
    let line = text[..start].rfind('\n').map_or(0, |i| i + 1);
    if start == end {
        if outdent {
            let count = text[line..]
                .bytes()
                .take(4)
                .take_while(|b| *b == b' ')
                .count();
            let caret = selection.start.saturating_sub(count);
            return replace(text, line..line + count, "", caret..caret);
        }
        let caret = selection.start + 4;
        return replace(text, start..start, "    ", caret..caret);
    }
    let end = if end < text.len() && text.as_bytes()[end] != b'\n' {
        text[end..].find('\n').map_or(text.len(), |i| end + i)
    } else {
        end
    };
    let mut block = String::new();
    // Preserve Java's split(-1) and StringBuilder separator behavior, including empty lines.
    for part in text[line..end].split('\n') {
        if !block.is_empty() {
            block.push('\n');
        }
        if outdent {
            let count = part.bytes().take(4).take_while(|b| *b == b' ').count();
            block.push_str(&part[count..]);
        } else {
            block.push_str("    ");
            block.push_str(part);
        }
        if block.len() > MAX_EDIT {
            bail!("编辑结果超过 8 MiB");
        }
    }
    let left = text[..line].chars().count();
    let right = left + block.chars().count();
    replace(text, line..end, &block, left..right)
}
pub fn continue_line(text: &str, caret: usize) -> Result<Option<Edit>> {
    static PREFIX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?P<prefix>[ \t\x0B\x0C\r]*(?:[-*+]|[0-9]+\.)[ \t\x0B\x0C\r]+(?:\[[ xX]\][ \t\x0B\x0C\r]+)?|[ \t\x0B\x0C\r]*(?:>[ \t\x0B\x0C\r]*)+)(?P<content>.*)$").unwrap()
    });
    static ORDERED: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^([ \t\x0B\x0C\r]*)([0-9]+)\.(.*)$").unwrap());
    let byte = offset(text, caret);
    let start = text[..byte].rfind('\n').map_or(0, |i| i + 1);
    let end = text[byte..].find('\n').map_or(text.len(), |i| byte + i);
    let Some(captures) = PREFIX.captures(&text[start..end]) else {
        return Ok(None);
    };
    if captures["content"].is_empty() {
        let caret = text[..start].chars().count();
        return replace(text, start..end, "", caret..caret).map(Some);
    }
    let prefix = &captures["prefix"];
    let prefix = if let Some(captures) = ORDERED.captures(prefix) {
        // Java Integer arithmetic wraps at MAX_VALUE; invalid oversized input is reported.
        let number = captures[2].parse::<i32>()?.wrapping_add(1);
        format!("{}{number}.{}", &captures[1], &captures[3])
    } else {
        prefix.to_owned()
    };
    let value = format!("\n{prefix}");
    let caret = caret + value.chars().count();
    replace(text, byte..byte, &value, caret..caret).map(Some)
}
#[derive(Clone, Copy, Default, Debug)]
pub struct Position {
    pub row: usize,
    pub column: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct Rectangle {
    pub start: Position,
    pub end: Position,
}
impl Rectangle {
    pub fn active(self) -> bool {
        self.start.row != self.end.row || self.start.column != self.end.column
    }
    pub fn rows(self) -> Range<usize> {
        self.start.row.min(self.end.row)..self.start.row.max(self.end.row).saturating_add(1)
    }
    pub fn columns(self) -> Range<usize> {
        self.start.column.min(self.end.column)..self.start.column.max(self.end.column)
    }
    /// Java columns count UTF-16 units. Reject a boundary inside a surrogate pair.
    pub fn edit(self, text: &str, value: &str) -> Result<Edit> {
        let mut output = String::with_capacity(text.len().min(MAX_EDIT));
        let mut caret = None;
        let rows = self.rows();
        let columns = self.columns();
        for (row, line) in text.split('\n').enumerate() {
            if row > 0 {
                output.push('\n');
            }
            if rows.contains(&row) {
                let left = utf16_offset(line, columns.start)?;
                let right = utf16_offset(line, columns.end)?;
                output.push_str(&line[..left]);
                output.push_str(value);
                if caret.is_none() && (left < right || !value.is_empty()) {
                    caret = Some(output.chars().count());
                }
                output.push_str(&line[right..]);
            } else {
                output.push_str(line);
            }
            if output.len() > MAX_EDIT {
                bail!("编辑结果超过 8 MiB");
            }
        }
        let caret = caret.unwrap_or(0);
        Ok(Edit {
            text: output,
            selection: caret..caret,
        })
    }
}
fn utf16_offset(text: &str, column: usize) -> Result<usize> {
    let mut units = 0;
    for (index, ch) in text.char_indices() {
        if units == column {
            return Ok(index);
        }
        units += ch.len_utf16();
        if units > column {
            bail!("列选择边界位于 UTF-16 代理项内部，请调整选区");
        }
    }
    Ok(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markdown_indent_and_continuation_preserve_original_rules() {
        assert_eq!(
            indent("中文\nnext", 0..3, false).unwrap().text,
            "    中文\n    next"
        );
        assert_eq!(indent("    中文", 5..5, true).unwrap().selection, 1..1);
        for (input, expected) in [
            ("- item", "- item\n- "),
            ("  09. [x] ok", "  09. [x] ok\n  10. [x] "),
            ("> > text", "> > text\n> > "),
            ("- [ ] ", ""),
            ("2147483647. x", "2147483647. x\n-2147483648. "),
        ] {
            assert_eq!(
                continue_line(input, input.chars().count())
                    .unwrap()
                    .unwrap()
                    .text,
                expected
            );
        }
        assert!(continue_line("ordinary", 8).unwrap().is_none());
        assert!(continue_line("999999999999. x", 15).is_err());
    }
    #[test]
    fn rectangular_edits_clamp_short_lines_keep_trailing_newlines_and_utf16_columns() {
        let rect = Rectangle {
            start: Position { row: 3, column: 3 },
            end: Position { row: 0, column: 1 },
        };
        assert_eq!(
            rect.edit("abcd\nx\n\n最后一行\n", "中").unwrap().text,
            "a中d\nx中\n中\n最中行\n"
        );
        let rect = Rectangle {
            start: Position { row: 0, column: 2 },
            end: Position { row: 1, column: 3 },
        };
        assert_eq!(rect.edit("😀a!\n中文b!", "").unwrap().text, "😀!\n中文!");
        assert!(rect.edit("😀a\na😀", "").is_err());
    }
}
