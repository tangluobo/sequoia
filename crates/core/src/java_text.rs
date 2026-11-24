//! Java String.isBlank / Character.isWhitespace semantics used by legacy policies.
pub fn is_whitespace(ch: char) -> bool {
    matches!(ch,'\t'..='\r'|'\u{1c}'..=' '|'\u{1680}'|'\u{2000}'..='\u{2006}'|'\u{2008}'..='\u{200a}'|'\u{2028}'|'\u{2029}'|'\u{205f}'|'\u{3000}')
}
pub fn is_blank(text: &str) -> bool {
    text.chars().all(is_whitespace)
}
