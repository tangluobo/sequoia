//! Fixed-height cells never lay out an entire stored program in the grid.
use std::borrow::Cow;
pub(super) fn single_line(text: &str) -> Cow<'_, str> {
    let end = text.char_indices().nth(256).map_or(text.len(), |(i, _)| i);
    let prefix = &text[..end];
    if end == text.len() && !prefix.contains(['\r', '\n', '\t']) {
        return Cow::Borrowed(text);
    }
    let mut result = String::with_capacity(prefix.len() + 3);
    result.extend(prefix.chars().map(|ch| {
        if matches!(ch, '\r' | '\n' | '\t') {
            ' '
        } else {
            ch
        }
    }));
    if end < text.len() {
        result.push('…');
    }
    Cow::Owned(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multiline_and_very_large_definitions_keep_bounded_single_line_cells() {
        assert!(matches!(single_line("fk_name"), Cow::Borrowed(_)));
        assert_eq!(
            single_line("BEGIN\n SET NEW.x=1;\nEND"),
            "BEGIN  SET NEW.x=1; END"
        );
        let text = "汉".repeat(2 * 1024 * 1024);
        let preview = single_line(&text);
        assert_eq!(preview.chars().count(), 257);
        assert!(preview.len() <= 771);
        assert!(preview.ends_with('…'));
    }
}
