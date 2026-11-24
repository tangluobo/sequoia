//! Dependency-free code tokenization with the original IDEA light palette.
#[path = "syntax_keywords.rs"]
mod keywords;
pub mod markdown;
mod sql;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Base,
    Keyword,
    String,
    Comment,
    Number,
    Annotation,
    Function,
    Attribute,
}
impl Kind {
    /// Original IdeaCodeHighlighter light palette, shared by all native editors.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Self::Base => [8, 8, 8],
            Self::Keyword => [0, 51, 179],
            Self::String => [6, 125, 23],
            Self::Comment => [140, 140, 140],
            Self::Number => [23, 80, 235],
            Self::Annotation => [158, 136, 13],
            Self::Function => [0, 98, 122],
            Self::Attribute => [23, 74, 212],
        }
    }
}
pub struct Token {
    pub range: std::ops::Range<usize>,
    pub kind: Kind,
}
pub fn language(info: &str) -> &str {
    let first = info.split_whitespace().next().unwrap_or("");
    if first.eq_ignore_ascii_case("sql") || first.eq_ignore_ascii_case("psql") {
        return "sql";
    }
    match info.split_whitespace().next().unwrap_or("") {
        "js" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "py" => "python",
        "golang" => "go",
        "rs" => "rust",
        "c++" | "cc" | "cxx" => "cpp",
        "cs" | "c#" => "csharp",
        "sh" | "bash" | "zsh" => "shell",
        "ps1" | "pwsh" => "powershell",
        "rb" => "ruby",
        "yml" => "yaml",
        other => other,
    }
}
pub fn tokens(source: &str, info: &str) -> Vec<Token> {
    let lang = language(info);
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut output: Vec<Token> = Vec::new();
    let keywords=keywords::LANGUAGES.iter().find(|(name,_)|*name==lang).map_or("if else for while do return break continue switch case default true false null class def func function fn fun import package new this super self try catch finally throw throws public private protected static const let var val void int string bool boolean float double",|(_,words)|*words);
    if lang == "sql" {
        return sql::tokens(source, keywords);
    }
    while index < bytes.len() {
        let start = index;
        let mut kind = Kind::Base;
        if source[index..].starts_with("//")
            || (lang == "sql" && source[index..].starts_with("--"))
            || (matches!(lang, "python" | "shell" | "powershell" | "yaml" | "toml")
                && bytes[index] == b'#')
        {
            index += source[index..].find('\n').unwrap_or(bytes.len() - index);
            kind = Kind::Comment;
        } else if source[index..].starts_with("/*") {
            index += source[index + 2..]
                .find("*/")
                .map_or(bytes.len() - index, |end| end + 4);
            kind = Kind::Comment;
        } else if matches!(bytes[index], b'"' | b'\'' | b'`') {
            let quote = bytes[index];
            index += 1;
            while index < bytes.len() {
                let next = bytes[index];
                index += 1;
                if next == b'\\' && index < bytes.len() {
                    index += 1;
                } else if next == quote {
                    break;
                }
            }
            while !source.is_char_boundary(index) {
                index += 1;
            }
            kind = Kind::String;
        } else if bytes[index].is_ascii_digit() {
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'.' | b'_'))
            {
                index += 1;
            }
            kind = Kind::Number;
        } else if bytes[index].is_ascii_alphabetic() || matches!(bytes[index], b'_' | b'$' | b'@') {
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'_' | b'$'))
            {
                index += 1;
            }
            let word = &source[start..index];
            let sql = if lang == "sql" {
                word.to_ascii_lowercase()
            } else {
                String::new()
            };
            let compare = if lang == "sql" { sql.as_str() } else { word };
            let next = source[index..].trim_start().as_bytes().first().copied();
            kind = if word.starts_with('@') {
                Kind::Annotation
            } else if keywords.split_whitespace().any(|word| word == compare) {
                Kind::Keyword
            } else if next == Some(b'(') {
                Kind::Function
            } else if lang == "json" && next == Some(b':') {
                Kind::Attribute
            } else {
                Kind::Base
            };
        } else {
            index += source[index..].chars().next().unwrap().len_utf8();
        }
        if let Some(previous) = output.last_mut()
            && previous.kind == kind
        {
            previous.range.end = index;
        } else {
            output.push(Token {
                range: start..index,
                kind,
            });
        }
    }
    output
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tokens_cover_unicode_exactly_and_keep_original_palette_categories() {
        let text = "fn main() { println!(\"中文\"); // comment\n}";
        let tokens = tokens(text, "rs");
        assert_eq!(
            tokens
                .iter()
                .map(|token| &text[token.range.clone()])
                .collect::<String>(),
            text
        );
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == Kind::Keyword && &text[token.range.clone()] == "fn")
        );
        assert!(tokens.iter().any(|token| token.kind == Kind::String));
        assert!(tokens.iter().any(|token| token.kind == Kind::Comment));
    }
}
