//! Original Markdown code rules and palette, shared by the preview and HTML writer.
use super::{Kind, Token, keywords};
use regex::{Captures, Regex};
use std::{borrow::Cow, ops::Range, sync::LazyLock};

const COMMON: &str = "if else for while do return break continue switch case default true false null class def func function fn fun import package new this super self try catch finally throw throws public private protected static const let var val void int string bool boolean float double";
const BLOCK: &str = r"(?P<BLOCK>/\*[\s\S]*?\*/)|";
const TRIPLE: &str = r#"(?P<TRIPLE>"""[\s\S]*?"""|'''[\s\S]*?''')|"#;
const STRINGS: &str = r###"(?P<STRING>"(?:\\[^\r\n\u0085\u2028\u2029]|[^"\\])*"|'(?:\\[^\r\n\u0085\u2028\u2029]|[^'\\])*'|\x60(?:\\[^\r\n\u0085\u2028\u2029]|[^\x60\\])*\x60)"###;
const WORDS: &str = r"|(?P<NUMBER>0[xX][0-9a-fA-F_]+|0[bB][01_]+|(?-u:\b)[0-9][0-9_]*(?:\.[0-9][0-9_]*)?(?:[eE][+-]?[0-9]+)?[fFdDuUlL]?(?-u:\b))|(?P<ANNOT>@[A-Za-z_][A-Za-z0-9_]*)|(?P<IDENT>[A-Za-z_$][A-Za-z0-9_$]*)";

fn pattern(prefix: &str, comments: &str) -> Regex {
    Regex::new(&format!("{prefix}{STRINGS}{comments}{WORDS}")).expect("static Markdown syntax")
}

fn rules(language: &str) -> &'static Regex {
    static STANDARD: LazyLock<Regex> = LazyLock::new(|| pattern(BLOCK, r"|(?P<LINE>//[^\n]*)"));
    static PHP: LazyLock<Regex> = LazyLock::new(|| pattern(BLOCK, r"|(?P<LINE>(?://|#)[^\n]*)"));
    static NO_LINE: LazyLock<Regex> = LazyLock::new(|| pattern(BLOCK, ""));
    static PYTHON: LazyLock<Regex> = LazyLock::new(|| pattern(TRIPLE, r"|(?P<LINE>#[^\n]*)"));
    static HASH_ONLY: LazyLock<Regex> = LazyLock::new(|| pattern("", r"|(?P<LINE>#[^\n]*)"));
    static MARKDOWN: LazyLock<Regex> = LazyLock::new(|| pattern("", r"|(?P<LINE>//[^\n]*)"));
    static BATCH: LazyLock<Regex> = LazyLock::new(|| {
        pattern(
            BLOCK,
            r"|(?P<LINE>(?im:^[ \t\r\n\x0B\x0C]*(?:rem(?:[ \t\r\n\x0B\x0C]|$)|::)[^\n]*))",
        )
    });
    static MARKUP: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r#"(?P<COMMENT><!--[\s\S]*?-->)|(?P<STRING>"(?:\\[^\r\n\u0085\u2028\u2029]|[^"\\])*"|'(?:\\[^\r\n\u0085\u2028\u2029]|[^'\\])*')"#,
            r"|(?P<TAG></?[A-Za-z][A-Za-z0-9:_-]*|/?>)|(?P<ATTR>[A-Za-z_:][A-Za-z0-9:_.-]*)[ \t\r\n\x0B\x0C]*=|(?P<ENTITY>&(?:#[0-9]+|#x[0-9a-fA-F]+|[A-Za-z][A-Za-z0-9]+);)"
        )).expect("static Markdown markup syntax")
    });
    match language {
        "html" | "xml" => &MARKUP,
        "sql" | "lua" => &super::sql::PATTERN,
        "php" => &PHP,
        "batch" => &BATCH,
        "css" | "scss" | "sass" | "less" | "json" => &NO_LINE,
        "python" => &PYTHON,
        "markdown" => &MARKDOWN,
        "ruby" | "shell" | "powershell" | "yaml" | "toml" | "ini" | "properties" | "conf"
        | "dockerfile" | "makefile" | "r" => &HASH_ONLY,
        _ => &STANDARD,
    }
}

fn language(info: &str) -> Cow<'_, str> {
    let info = info.split_whitespace().next().unwrap_or("");
    let info = info
        .strip_prefix("{.")
        .and_then(|rest| rest.strip_suffix('}'))
        .or_else(|| info.strip_prefix('.'))
        .unwrap_or(info);
    let info = info.strip_prefix("language-").unwrap_or(info);
    let lower = if info.bytes().any(|byte| byte.is_ascii_uppercase()) {
        Cow::Owned(info.to_ascii_lowercase())
    } else {
        Cow::Borrowed(info)
    };
    match lower.as_ref() {
        "js" | "jsx" | "node" => "javascript".into(),
        "ts" | "tsx" => "typescript".into(),
        "py" => "python".into(),
        "kt" | "kts" => "kotlin".into(),
        "rs" => "rust".into(),
        "golang" => "go".into(),
        "c++" | "cc" | "cxx" => "cpp".into(),
        "cs" | "c#" => "csharp".into(),
        "rb" => "ruby".into(),
        "sh" | "bash" | "zsh" | "console" => "shell".into(),
        "ps1" => "powershell".into(),
        "yml" => "yaml".into(),
        "htm" | "xhtml" | "svg" | "mathml" => "html".into(),
        "md" | "mdown" => "markdown".into(),
        "text" | "txt" | "plain" => "plaintext".into(),
        _ => lower,
    }
}

/// Emit contiguous color ranges without retaining all the matches.
pub fn visit<E>(
    source: &str,
    info: &str,
    mut emit: impl FnMut(Token) -> Result<(), E>,
) -> Result<(), E> {
    if source.is_empty() {
        return Ok(());
    }
    let language = language(info);
    let language = language.as_ref();
    if matches!(language, "" | "plaintext") {
        return emit(Token {
            range: 0..source.len(),
            kind: Kind::Base,
        });
    }
    let keywords = keywords::LANGUAGES
        .iter()
        .find(|(name, _)| *name == language)
        .map_or(COMMON, |(_, words)| *words);
    let mut pending: Option<Token> = None;
    let mut push = |range: Range<usize>, kind| {
        if !range.is_empty() {
            if let Some(previous) = &mut pending
                && previous.kind == kind
            {
                previous.range.end = range.end;
            } else if let Some(previous) = pending.replace(Token { range, kind }) {
                emit(previous)?;
            }
        }
        Ok(())
    };
    let mut last = 0;
    for captures in rules(language).captures_iter(source) {
        let matched = captures.get(0).expect("whole match");
        let token = captures.name("ATTR").unwrap_or(matched);
        push(last..token.start(), Kind::Base)?;
        push(
            token.range(),
            classify(
                &captures,
                token.as_str(),
                &source[token.end()..],
                language,
                keywords,
            ),
        )?;
        last = token.end();
    }
    push(last..source.len(), Kind::Base)?;
    if let Some(token) = pending {
        emit(token)?;
    }
    Ok(())
}

fn classify(
    captures: &Captures<'_>,
    text: &str,
    rest: &str,
    language: &str,
    keywords: &str,
) -> Kind {
    let group = |name| captures.name(name).is_some();
    if group("BLOCK") || group("LINE") || group("COMMENT") {
        Kind::Comment
    } else if group("STRING") || group("TRIPLE") {
        Kind::String
    } else if group("NUMBER") {
        Kind::Number
    } else if group("ANNOT") || group("ENTITY") {
        Kind::Annotation
    } else if group("ATTR") {
        Kind::Attribute
    } else if group("TAG")
        || keywords
            .split_whitespace()
            .any(|word| word == text || (language == "sql" && word.eq_ignore_ascii_case(text)))
    {
        Kind::Keyword
    } else if rest.trim_start_matches([' ', '\t']).starts_with('(') {
        Kind::Function
    } else {
        Kind::Base
    }
}
