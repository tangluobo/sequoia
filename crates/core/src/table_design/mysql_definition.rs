//! Lossless SHOW CREATE column slices; expression groups and quoted commas stay opaque.
use anyhow::{Result, bail, ensure};
use std::ops::Range;
#[derive(Clone, Debug)]
pub struct Token {
    pub range: Range<usize>,
}
pub fn tokens(source: &str, backslash: bool) -> Result<Vec<Token>> {
    ensure!(
        source.len() <= 8 * 1024 * 1024 && !source.contains('\0'),
        "表定义超过限制或含空字符"
    );
    let bytes = source.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        ensure!(result.len() < 100_000, "表定义标记过多");
        let start = i;
        if b"'\"`".contains(&bytes[i]) {
            i = quoted(bytes, i, backslash)?;
        } else if bytes[i] == b'(' {
            let mut depth = 1;
            i += 1;
            while i < bytes.len() && depth > 0 {
                match bytes[i] {
                    b'\'' | b'"' | b'`' => i = quoted(bytes, i, backslash)?,
                    b'(' => {
                        depth += 1;
                        ensure!(depth <= 128, "表定义嵌套过深");
                        i += 1;
                    }
                    b')' => {
                        depth -= 1;
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
            ensure!(depth == 0, "表定义括号未闭合");
        } else if b",);".contains(&bytes[i]) {
            i += 1;
        } else {
            while i < bytes.len()
                && !bytes[i].is_ascii_whitespace()
                && !b"'\"`(),;".contains(&bytes[i])
            {
                i += 1;
            }
        }
        result.push(Token { range: start..i });
    }
    Ok(result)
}
fn quoted(bytes: &[u8], mut i: usize, backslash: bool) -> Result<usize> {
    let quote = bytes[i];
    i += 1;
    while i < bytes.len() {
        if bytes[i] == quote {
            if bytes.get(i + 1) == Some(&quote) {
                i += 2;
            } else {
                return Ok(i + 1);
            }
        } else if backslash && quote != b'`' && bytes[i] == b'\\' {
            i += 2;
        } else {
            i += 1;
        }
    }
    bail!("表定义引号未闭合")
}
pub fn columns(create: &str, backslash: bool) -> Result<Vec<(String, String)>> {
    let outer = tokens(create, backslash)?;
    let body = outer
        .iter()
        .map(|t| &create[t.range.clone()])
        .find(|s| s.starts_with('('))
        .ok_or_else(|| anyhow::anyhow!("CREATE TABLE 缺少字段"))?;
    let body = &body[1..body.len() - 1];
    let parts = tokens(body, backslash)?;
    let mut start = 0;
    let mut result = Vec::new();
    for end in parts
        .iter()
        .filter(|t| &body[t.range.clone()] == ",")
        .map(|t| t.range.start)
        .chain(std::iter::once(body.len()))
    {
        let definition = body[start..end].trim();
        start = end + 1;
        if definition.starts_with(['`', '"']) {
            let ts = tokens(definition, backslash)?;
            let name = &definition[ts[0].range.clone()];
            result.push((
                name[1..name.len() - 1].replace(&name[..1].repeat(2), &name[..1]),
                definition.to_owned(),
            ));
            ensure!(result.len() <= 4096, "字段超过 4,096 项");
        }
    }
    Ok(result)
}
/// SHOW CREATE reads the live counter; INFORMATION_SCHEMA may return cached statistics.
pub fn auto_increment(create: &str, backslash: bool) -> Result<Option<String>> {
    let tokens = tokens(create, backslash)?;
    let mut body = false;
    for token in tokens {
        let text = &create[token.range];
        if text.starts_with('(') {
            body = true;
            continue;
        }
        if body
            && let Some((key, value)) = text.split_once('=')
            && key.eq_ignore_ascii_case("AUTO_INCREMENT")
        {
            ensure!(
                !value.is_empty()
                    && value.bytes().all(|b| b.is_ascii_digit())
                    && value.parse::<u64>().is_ok(),
                "表的自动递增值无效"
            );
            return Ok(Some(value.into()));
        }
    }
    Ok(None)
}
/// Replace one attribute, retaining all other clauses byte for byte.
pub fn replace_attribute(
    definition: &str,
    attribute: &str,
    replacement: &str,
    backslash: bool,
) -> Result<String> {
    let ts = tokens(definition, backslash)?;
    ensure!(ts.len() >= 2, "列定义不完整");
    let mut segments = Vec::new();
    let mut i = 2;
    while i < ts.len() {
        let word = &definition[ts[i].range.clone()];
        let upper = word.to_ascii_uppercase();
        let category = match upper.as_str() {
            "CHARACTER" => "CHARACTER",
            "COLLATE" => "COLLATE",
            "NOT" | "NULL" => "NULL",
            "DEFAULT" => "DEFAULT",
            "AUTO_INCREMENT" => "AUTO_INCREMENT",
            "COMMENT" => "COMMENT",
            "ON" => "ON",
            "GENERATED" | "AS" => "GENERATED",
            "VIRTUAL" | "STORED" => "STORAGE_KIND",
            "COLUMN_FORMAT"
            | "STORAGE"
            | "SRID"
            | "VISIBLE"
            | "INVISIBLE"
            | "ENGINE_ATTRIBUTE"
            | "SECONDARY_ENGINE_ATTRIBUTE" => upper.as_str(),
            _ => {
                i += 1;
                continue;
            }
        }
        .to_owned();
        let begin = ts[i].range.start;
        let mut next = i + 1;
        ensure!(
            i + 1 < ts.len()
                || ![
                    "DEFAULT",
                    "COMMENT",
                    "COLLATE",
                    "CHARACTER",
                    "NOT",
                    "ON",
                    "GENERATED",
                    "AS"
                ]
                .contains(&upper.as_str()),
            "列属性不完整"
        );
        if upper == "NOT" {
            next = (i + 2).min(ts.len());
        } else if upper == "CHARACTER" {
            next = (i + 3).min(ts.len());
        } else if [
            "DEFAULT",
            "COMMENT",
            "COLLATE",
            "COLUMN_FORMAT",
            "STORAGE",
            "SRID",
            "ENGINE_ATTRIBUTE",
            "SECONDARY_ENGINE_ATTRIBUTE",
        ]
        .contains(&upper.as_str())
        {
            next = (i + 2).min(ts.len());
            if let Some(t) = ts.get(next)
                && definition[t.range.clone()].starts_with('(')
            {
                next += 1;
            }
            // MySQL character introducers: DEFAULT _utf8mb4'text'.
            if upper == "DEFAULT"
                && (definition[ts[i + 1].range.clone()].starts_with('_')
                    || ["b", "x", "n"]
                        .iter()
                        .any(|s| definition[ts[i + 1].range.clone()].eq_ignore_ascii_case(s)))
                && let Some(t) = ts.get(next)
                && definition[t.range.clone()].starts_with(['\'', '"'])
            {
                next += 1;
            }
        } else if upper == "ON" {
            next = (i + 3).min(ts.len());
            if let Some(t) = ts.get(next)
                && definition[t.range.clone()].starts_with('(')
            {
                next += 1;
            }
        } else if upper == "GENERATED" || upper == "AS" {
            while next < ts.len() && !definition[ts[next].range.clone()].starts_with('(') {
                next += 1;
            }
            next = (next + 1).min(ts.len());
        }
        let end = ts[next - 1].range.end;
        segments.push((category, begin..end));
        i = next;
    }
    let range = segments
        .iter()
        .find(|(name, _)| name == attribute)
        .map(|(_, range)| range.clone());
    if let Some(range) = range {
        Ok(format!(
            "{}{}{}",
            &definition[..range.start],
            replacement,
            &definition[range.end..]
        ))
    } else if replacement.is_empty() {
        Ok(definition.into())
    } else {
        Ok(format!("{definition} {replacement}"))
    }
}
pub fn replace_type(definition: &str, replacement: &str, backslash: bool) -> Result<String> {
    let ts = tokens(definition, backslash)?;
    ensure!(ts.len() >= 2, "列定义不完整");
    let mut end = 2;
    while end < ts.len() {
        let s = &definition[ts[end].range.clone()];
        if s.starts_with('(')
            || ["UNSIGNED", "ZEROFILL", "PRECISION", "VARYING"]
                .iter()
                .any(|w| s.eq_ignore_ascii_case(w))
        {
            end += 1;
        } else {
            break;
        }
    }
    Ok(format!(
        "{}{}{}",
        &definition[..ts[1].range.start],
        replacement,
        &definition[ts[end - 1].range.end..]
    ))
}

#[cfg(test)]
mod live_counter_tests {
    use super::*;
    #[test]
    fn counter_only_comes_from_table_options_not_columns_or_quoted_comments() -> Result<()> {
        let ddl = "CREATE TABLE `items` (`id` bigint AUTO_INCREMENT, `note` varchar(50) COMMENT 'AUTO_INCREMENT=666') ENGINE=InnoDB AUTO_INCREMENT=503 COMMENT='AUTO_INCREMENT=777'";
        assert_eq!(auto_increment(ddl, true)?, Some("503".into()));
        assert_eq!(
            auto_increment(
                "CREATE TABLE `items` (`id` bigint AUTO_INCREMENT) ENGINE=InnoDB",
                true
            )?,
            None
        );
        Ok(())
    }
}
