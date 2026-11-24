//! SQL editor edits expressed in Unicode scalar cursor positions for the native UI.
use anyhow::{Result, ensure};
pub struct Edit {
    pub text: String,
    pub anchor: usize,
    pub caret: usize,
}
fn byte(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(at, _)| at)
}
/// Java String.trim() removes U+0000..U+0020, rather than Unicode whitespace.
pub fn effective(text: &str, start: usize, end: usize) -> &str {
    fn trim(value: &str) -> &str {
        value.trim_matches(|ch| ch <= '\u{20}')
    }
    let all = trim(text);
    if all.is_empty() {
        return all;
    }
    let selected = trim(&text[byte(text, start.min(end))..byte(text, start.max(end))]);
    if selected.is_empty() { all } else { selected }
}
pub fn duplicate(text: &str, start: usize, end: usize, caret: usize) -> Result<Edit> {
    let chars = text.chars().count();
    let (start, end) = (start.min(end).min(chars), start.max(end).min(chars));
    duplicate_ordered(text, start, end, caret.min(chars))
}
fn duplicate_ordered(text: &str, start: usize, end: usize, caret: usize) -> Result<Edit> {
    let (insertion, insert, anchor, cursor) = if start < end {
        let selected = &text[byte(text, start)..byte(text, end)];
        (
            byte(text, end),
            selected.to_owned(),
            end,
            end + selected.chars().count(),
        )
    } else {
        let at = byte(text, caret);
        let line_start = text[..at].rfind('\n').map_or(0, |index| index + 1);
        let feed = text[at..].find('\n').map(|index| index + at);
        let line_end = feed.unwrap_or(text.len());
        let content_end = if line_end > line_start && text.as_bytes()[line_end - 1] == b'\r' {
            line_end - 1
        } else {
            line_end
        };
        let line = &text[line_start..content_end];
        let column = text[line_start..at]
            .chars()
            .count()
            .min(line.chars().count());
        if let Some(feed) = feed {
            let separator = if content_end < line_end { "\r\n" } else { "\n" };
            let insert = format!("{line}{separator}");
            let cursor = text[..feed + 1].chars().count() + column;
            (feed + 1, insert, cursor, cursor)
        } else {
            let separator = if text.contains("\r\n") { "\r\n" } else { "\n" };
            let cursor = text.chars().count() + separator.chars().count() + column;
            (text.len(), format!("{separator}{line}"), cursor, cursor)
        }
    };
    ensure!(
        text.len().saturating_add(insert.len()) <= 8 * 1024 * 1024,
        "SQL 正文超过 8 MiB，未复制"
    );
    let mut result = String::with_capacity(text.len() + insert.len());
    result.push_str(&text[..insertion]);
    result.push_str(&insert);
    result.push_str(&text[insertion..]);
    Ok(Edit {
        text: result,
        anchor,
        caret: cursor,
    })
}
