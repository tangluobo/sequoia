//! SQL token matching follows the original Java highlighter's ordered alternatives.
use super::{Kind, Token};
use std::sync::LazyLock;
pub(super) static PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(concat!(
    r###"(?P<BLOCK>/\*[\s\S]*?\*/)|(?P<STRING>"(?:\\[^\r\n\u0085\u2028\u2029]|[^"\\])*"|'(?:\\[^\r\n\u0085\u2028\u2029]|[^'\\])*'|`(?:\\[^\r\n\u0085\u2028\u2029]|[^`\\])*`)|(?P<LINE>--[^\n]*)|"###,
    r"(?P<NUMBER>0[xX][0-9a-fA-F_]+|0[bB][01_]+|(?-u:\b)[0-9][0-9_]*(?:\.[0-9][0-9_]*)?(?:[eE][+-]?[0-9]+)?[fFdDuUlL]?(?-u:\b))|(?P<ANNOT>@[A-Za-z_][A-Za-z0-9_]*)|(?P<IDENT>[A-Za-z_$][A-Za-z0-9_$]*)"
)).unwrap()
});
fn push(output: &mut Vec<Token>, range: std::ops::Range<usize>, kind: Kind) {
    if range.is_empty() {
        return;
    }
    if let Some(previous) = output.last_mut()
        && previous.kind == kind
    {
        previous.range.end = range.end;
    } else {
        output.push(Token { range, kind });
    }
}
pub(super) fn tokens(source: &str, keywords: &str) -> Vec<Token> {
    let mut output = Vec::new();
    let mut last = 0;
    for captures in PATTERN.captures_iter(source) {
        let matched = captures.get(0).unwrap();
        push(&mut output, last..matched.start(), Kind::Base);
        let kind = if captures.name("BLOCK").is_some() || captures.name("LINE").is_some() {
            Kind::Comment
        } else if captures.name("STRING").is_some() {
            Kind::String
        } else if captures.name("NUMBER").is_some() {
            Kind::Number
        } else if captures.name("ANNOT").is_some() {
            Kind::Annotation
        } else if keywords
            .split_whitespace()
            .any(|word| word.eq_ignore_ascii_case(matched.as_str()))
        {
            Kind::Keyword
        } else if source[matched.end()..]
            .trim_start_matches([' ', '\t'])
            .starts_with('(')
        {
            Kind::Function
        } else {
            Kind::Base
        };
        push(&mut output, matched.range(), kind);
        last = matched.end();
    }
    push(&mut output, last..source.len(), Kind::Base);
    output
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(serde::Deserialize)]
    struct Case {
        input: String,
        tokens: Vec<Expected>,
    }
    #[derive(serde::Deserialize)]
    struct Expected {
        text: String,
        kind: String,
    }
    #[test]
    fn sql_highlighting_matches_actual_java_token_ranges_and_palette_categories()
    -> anyhow::Result<()> {
        let cases: Vec<Case> = serde_json::from_str(include_str!(
            "../../tests/fixtures/sql-highlight/original.json"
        ))?;
        assert_eq!(cases.len(), 32);
        let keywords = super::super::keywords::LANGUAGES
            .iter()
            .find(|(language, _)| *language == "sql")
            .unwrap()
            .1;
        for (index, case) in cases.into_iter().enumerate() {
            let actual = tokens(&case.input, keywords)
                .into_iter()
                .map(|token| {
                    (
                        case.input[token.range].to_owned(),
                        format!("{:?}", token.kind).to_uppercase(),
                    )
                })
                .collect::<Vec<_>>();
            let mut expected: Vec<(String, String)> = Vec::new();
            for token in case.tokens {
                if let Some(previous) = expected.last_mut()
                    && previous.1 == token.kind
                {
                    previous.0.push_str(&token.text);
                } else {
                    expected.push((token.text, token.kind));
                }
            }
            assert_eq!(actual, expected, "Java SQL highlighting case {index}");
        }
        Ok(())
    }
}
