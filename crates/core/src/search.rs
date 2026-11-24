//! Bounded UTF-8 search and Java-style replacement references, shared by editors.
use anyhow::{Result, bail, ensure};
use fancy_regex::{Error, Regex, RegexBuilder, RuntimeError};
use std::{
    ops::Range,
    time::{Duration, Instant},
};

pub const MAX_HITS: usize = 100_000;
pub const MAX_TEXT: usize = 8 * 1024 * 1024;
pub const MAX_REPLACEMENT: usize = 256 * 1024;
const MAX_PATTERN: usize = 16 * 1024;
const BACKTRACK_LIMIT: usize = 100_000;
const TIME_LIMIT: Duration = Duration::from_secs(2);

pub struct Search {
    matcher: Regex,
    pub hits: Vec<Range<usize>>,
    pub truncated: bool,
    regex: bool,
}
impl Search {
    pub fn new(text: &str, needle: &str, match_case: bool, regex: bool) -> Result<Self> {
        Self::scan(text, needle, match_case, regex, || false)
    }

    pub fn scan(
        text: &str,
        needle: &str,
        match_case: bool,
        regex: bool,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        ensure!(text.len() <= MAX_TEXT, "查找正文超过 8 MiB");
        ensure!(needle.len() <= MAX_PATTERN, "查找表达式超过 16 KiB");
        let check = budget(&cancelled);
        check()?;
        let pattern = if regex {
            quoted_literals(needle)
        } else {
            regex::escape(needle)
        };
        let matcher = RegexBuilder::new(&pattern)
            .case_insensitive(!match_case)
            .backtrack_limit(BACKTRACK_LIMIT)
            .delegate_size_limit(2 * 1024 * 1024)
            .delegate_dfa_size_limit(2 * 1024 * 1024)
            .build()?;
        let mut hits = Vec::new();
        if !needle.is_empty() {
            let mut start = 0;
            while start <= text.len() && hits.len() <= MAX_HITS {
                check()?;
                let Some(hit) = matcher.find_from_pos(text, start).map_err(runtime_error)? else {
                    break;
                };
                // Java keeps a zero-width match immediately following a
                // consuming match. Step to the next valid UTF-8 boundary.
                start = if hit.start() == hit.end() {
                    hit.end() + text[hit.end()..].chars().next().map_or(1, char::len_utf8)
                } else {
                    hit.end()
                };
                hits.push(hit.range());
            }
        }
        check()?;
        let truncated = hits.len() > MAX_HITS;
        hits.truncate(MAX_HITS);
        Ok(Self {
            matcher,
            hits,
            truncated,
            regex,
        })
    }
    /// Compute the complete result before committing any edit; errors leave input intact.
    pub fn replace(
        &self,
        text: &str,
        index: Option<usize>,
        replacement: &str,
        limit: usize,
    ) -> Result<String> {
        self.replace_checked(text, index, replacement, limit, || false)
    }

    pub fn replace_checked(
        &self,
        text: &str,
        index: Option<usize>,
        replacement: &str,
        limit: usize,
        cancelled: impl Fn() -> bool,
    ) -> Result<String> {
        let check = budget(&cancelled);
        check()?;
        if self.hits.is_empty() {
            return Ok(text.to_owned());
        }
        ensure!(
            index.is_some() || !self.truncated,
            "匹配超过 100,000 项，请缩小范围后替换"
        );
        ensure!(replacement.len() <= limit, "替换内容超出大小限制");
        ensure!(
            !self.regex || replacement.len() <= MAX_REPLACEMENT,
            "正则替换模板超过 256 KiB"
        );
        let template = self.regex.then(|| self.template(replacement)).transpose()?;
        let hits = match index {
            Some(index) => std::slice::from_ref(
                self.hits
                    .get(index)
                    .ok_or_else(|| anyhow::anyhow!("匹配位置已失效"))?,
            ),
            None => self.hits.as_slice(),
        };
        let mut out = String::new();
        let mut end = 0;
        for hit in hits {
            check()?;
            let prefix = text
                .get(end..hit.start)
                .ok_or_else(|| anyhow::anyhow!("匹配位置已失效"))?;
            append(&mut out, prefix, limit)?;
            if let Some(template) = &template {
                let captures = self
                    .matcher
                    .captures_from_pos(text, hit.start)
                    .map_err(runtime_error)?
                    .filter(|captures| captures.get(0).is_some_and(|found| found.range() == *hit))
                    .ok_or_else(|| anyhow::anyhow!("匹配位置已失效"))?;
                for part in template {
                    let value = match part {
                        Part::Literal(value) => value.as_str(),
                        Part::Group(number) => captures.get(*number).map_or("", |m| m.as_str()),
                    };
                    append(&mut out, value, limit)?;
                }
            } else {
                ensure!(text.is_char_boundary(hit.end), "匹配位置已失效");
                append(&mut out, replacement, limit)?;
            }
            end = hit.end;
        }
        append(&mut out, &text[end..], limit)?;
        check()?;
        Ok(out)
    }
    fn template(&self, input: &str) -> Result<Vec<Part>> {
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut chars = input.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\\' => literal.push(
                    chars
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("替换内容末尾缺少转义字符"))?,
                ),
                '$' => {
                    if !literal.is_empty() {
                        parts.push(Part::Literal(std::mem::take(&mut literal)));
                    }
                    let next = chars
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("替换内容缺少捕获组编号"))?;
                    let number = if next == '{' {
                        let mut name = String::new();
                        loop {
                            match chars.next() {
                                Some('}') => break,
                                Some(ch) => name.push(ch),
                                None => bail!("捕获组名称缺少右花括号"),
                            }
                        }
                        self.matcher
                            .capture_names()
                            .position(|value| value == Some(name.as_str()))
                            .ok_or_else(|| anyhow::anyhow!("不存在捕获组：{name}"))?
                    } else {
                        let mut number = next
                            .to_digit(10)
                            .filter(|_| next.is_ascii_digit())
                            .ok_or_else(|| anyhow::anyhow!("替换内容的 $ 后应为编号或组名"))?
                            as usize;
                        ensure!(
                            number < self.matcher.captures_len(),
                            "不存在捕获组：{number}"
                        );
                        while let Some(digit) = chars
                            .peek()
                            .and_then(|ch| ch.to_digit(10).filter(|_| ch.is_ascii_digit()))
                        {
                            let candidate =
                                number.saturating_mul(10).saturating_add(digit as usize);
                            if candidate >= self.matcher.captures_len() {
                                break;
                            }
                            number = candidate;
                            chars.next();
                        }
                        number
                    };
                    parts.push(Part::Group(number));
                }
                _ => literal.push(ch),
            }
        }
        if !literal.is_empty() {
            parts.push(Part::Literal(literal));
        }
        Ok(parts)
    }
}

fn budget(cancelled: &impl Fn() -> bool) -> impl Fn() -> Result<()> {
    let deadline = Instant::now() + TIME_LIMIT;
    move || {
        ensure!(!cancelled(), "已取消查找或替换");
        ensure!(
            Instant::now() < deadline,
            "正则计算超时，请缩小范围或简化表达式"
        );
        Ok(())
    }
}

fn runtime_error(error: Error) -> anyhow::Error {
    match error {
        Error::RuntimeError(RuntimeError::BacktrackLimitExceeded | RuntimeError::StackOverflow) => {
            anyhow::anyhow!("正则回溯超过上限，请简化表达式")
        }
        other => other.into(),
    }
}

/// Java quotes all characters between an unescaped \Q and the next \E.
fn quoted_literals(mut pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len());
    while let Some(slash) = pattern.find('\\') {
        result.push_str(&pattern[..slash]);
        let escaped = &pattern[slash + 1..];
        if let Some(literal) = escaped.strip_prefix('Q') {
            let (literal, tail) = literal.split_once(r"\E").unwrap_or((literal, ""));
            for ch in literal.chars() {
                // Hex escaping also keeps whitespace and # literal inside (?x).
                use std::fmt::Write as _;
                let _ = write!(result, r"\x{{{:x}}}", u32::from(ch));
            }
            pattern = tail;
        } else {
            let end = slash + 1 + escaped.chars().next().map_or(0, char::len_utf8);
            result.push_str(&pattern[slash..end]);
            pattern = &pattern[end..];
        }
    }
    result.push_str(pattern);
    result
}
enum Part {
    Literal(String),
    Group(usize),
}
fn append(out: &mut String, text: &str, limit: usize) -> Result<()> {
    ensure!(
        out.len().saturating_add(text.len()) <= limit,
        "替换结果超过 {} MiB",
        limit / 1024 / 1024
    );
    out.push_str(text);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_search_case_and_literal_metacharacters() {
        let search = Search::new("中文 Ab.C ab.c", "ab.c", false, false).unwrap();
        assert_eq!(search.hits, [7..11, 12..16]);
        assert_eq!(
            search.replace("中文 Ab.C ab.c", None, "$1", 100).unwrap(),
            "中文 $1 $1"
        );
        assert!(
            Search::new("ABC", "abc", true, false)
                .unwrap()
                .hits
                .is_empty()
        );
    }
    #[test]
    fn java_group_replacement_escaping_and_atomic_limits() {
        let text = "苹果12 梨34";
        let search = Search::new(text, r"(?P<fruit>\p{Han}+)(\d+)", true, true).unwrap();
        assert_eq!(
            search.replace(text, None, r"${fruit}:$2:\$", 100).unwrap(),
            "苹果:12:$ 梨:34:$"
        );
        assert_eq!(
            search.replace(text, Some(1), "$20", 100).unwrap(),
            "苹果12 340"
        );
        assert!(search.replace(text, None, "$9", 100).is_err());
        assert!(search.replace(text, None, "longlong", 5).is_err());
        assert_eq!(text, "苹果12 梨34");
    }
    #[test]
    fn zero_width_matches_advance_and_invalid_patterns_are_errors() {
        let search = Search::new("中文", "^|$", true, true).unwrap();
        assert_eq!(search.hits, [0..0, 6..6]);
        assert_eq!(search.replace("中文", None, "_", 100).unwrap(), "_中文_");
        assert!(Search::new("text", "[", false, true).is_err());
    }

    #[test]
    fn java_regex_lookarounds_backreferences_and_quoted_text() -> Result<()> {
        for (text, expression, replacement, expected) in [
            (
                "总计:苹果12, 梨34;",
                r"(?<=[: ])(?<fruit>\p{IsHan}+)(\d+)(?=[,;])",
                r"${fruit}=$2",
                "总计:苹果=12, 梨=34;",
            ),
            (
                "red red blue green 苹果 苹果",
                r"(?<word>[\p{L}]+)\s+\k<word>",
                r"<${word}>",
                "<red> blue green <苹果>",
            ),
            ("AA AB BB", r"([A-Z])\1", "$1", "A AB B"),
            (
                "中.a + # [x] 😀",
                r"(?x)\Q.a + # [x]\E",
                r"\$0:$0",
                "中$0:.a + # [x] 😀",
            ),
            ("a😀", r"a|(?=😀)|$", "_", "__😀_"),
            (
                "前置:值 额外前置:值",
                r"(?<=前置{1,2}:)值",
                "X",
                "前置:X 额外前置:X",
            ),
        ] {
            let search = Search::new(text, expression, true, true)?;
            assert_eq!(
                search.replace(text, None, replacement, MAX_TEXT)?,
                expected,
                "{expression}"
            );
        }
        Ok(())
    }

    #[test]
    fn complex_regex_limits_and_cancellation_leave_no_partial_result() -> Result<()> {
        let pattern = r"^(a|aa)+\1$";
        let text = format!("{}b", "a".repeat(40));
        let error = Search::new(&text, pattern, true, true)
            .err()
            .expect("pathological backtracking should be bounded");
        assert!(error.to_string().contains("回溯"), "{error}");
        let text = "AA BB";
        let search = Search::new(text, r"([A-Z])\1", true, true)?;
        let checks = std::cell::Cell::new(0);
        let error = search
            .replace_checked(text, None, "$1", MAX_TEXT, || {
                checks.set(checks.get() + 1);
                checks.get() > 2
            })
            .unwrap_err();
        assert!(error.to_string().contains("取消"));
        assert_eq!(search.replace(text, Some(1), "$1", MAX_TEXT)?, "AA B");
        Ok(())
    }
}
