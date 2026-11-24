use anyhow::Result;
mod json_reader;

pub fn json_transform(input: &str, action: &str) -> Result<String> {
    // Java String.trim(), rather than Unicode whitespace normalization.
    let input = input.trim_matches(|c: char| c <= ' ');
    anyhow::ensure!(!input.is_empty(), "请输入JSON数据或要转换的文本");
    Ok(match action {
        "format" => {
            let mut value = json_reader::parse(input)?;
            gson_nulls(&mut value);
            gson_html(json_write(&value, true)?)?
        }
        "minify" => {
            let mut value = json_reader::parse(input)?;
            gson_nulls(&mut value);
            gson_html(json_write(&value, false)?)?
        }
        "escape" => input
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t"),
        "unescape" => input
            .replace("\\n", "\n")
            .replace("\\r", "\r")
            .replace("\\t", "\t")
            .replace("\\\"", "\"")
            .replace("\\\\", "\\"),
        "unicode" => input
            .encode_utf16()
            .map(|unit| {
                if unit < 128 {
                    char::from_u32(unit as u32).unwrap_or_default().to_string()
                } else {
                    format!("\\u{unit:04x}")
                }
            })
            .collect(),
        "decode_unicode" => {
            let mut units = Vec::new();
            let mut index = 0;
            while index < input.len() {
                let tail = &input[index..];
                if tail.starts_with("\\u")
                    && tail
                        .as_bytes()
                        .get(2..6)
                        .is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit))
                {
                    units.push(u16::from_str_radix(&tail[2..6], 16)?);
                    index += 6;
                } else {
                    let c = tail.chars().next().unwrap_or_default();
                    let mut encoded = [0; 2];
                    units.extend_from_slice(c.encode_utf16(&mut encoded));
                    index += c.len_utf8();
                }
            }
            String::from_utf16_lossy(&units)
        }
        _ => anyhow::bail!("未知 JSON 操作: {action}"),
    })
}
fn gson_nulls(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.retain(|_, v| !v.is_null());
            for value in map.values_mut() {
                gson_nulls(value);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                gson_nulls(value);
            }
        }
        _ => {}
    }
}
/// Gson's default HTML-safe JSON writer also escapes JavaScript line separators.
const MAX_JSON_OUTPUT: usize = 32 * 1024 * 1024;
/// The original Gson grammar, retaining object nulls and numeric spelling.
/// Configuration repositories deliberately keep their separate strict parser.
pub fn json_parse(input: &str) -> Result<serde_json::Value> {
    json_reader::parse(input)
}
/// Bounded JSON serialization; callers choose whether to preserve object nulls.
pub fn json_write(value: &serde_json::Value, pretty: bool) -> Result<String> {
    struct Bounded(Vec<u8>);
    impl std::io::Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.0.len().saturating_add(bytes.len()) > MAX_JSON_OUTPUT {
                return Err(std::io::Error::other("JSON输出超过32 MiB"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Bounded(Vec::new());
    if pretty {
        serde_json::to_writer_pretty(&mut output, value)?;
    } else {
        serde_json::to_writer(&mut output, value)?;
    }
    Ok(String::from_utf8(output.0)?)
}
fn gson_html(text: String) -> Result<String> {
    if !text
        .chars()
        .any(|c| matches!(c, '<' | '>' | '&' | '=' | '\'' | '\u{2028}' | '\u{2029}'))
    {
        return Ok(text);
    }
    let extra: usize = text
        .chars()
        .map(|c| match c {
            '<' | '>' | '&' | '=' | '\'' => 5,
            '\u{2028}' | '\u{2029}' => 3,
            _ => 0,
        })
        .sum();
    anyhow::ensure!(
        text.len().saturating_add(extra) <= MAX_JSON_OUTPUT,
        "JSON转义输出超过32 MiB"
    );
    let mut escaped = String::with_capacity(text.len() + extra);
    for c in text.chars() {
        match c {
            '<' => escaped.push_str("\\u003c"),
            '>' => escaped.push_str("\\u003e"),
            '&' => escaped.push_str("\\u0026"),
            '=' => escaped.push_str("\\u003d"),
            '\'' => escaped.push_str("\\u0027"),
            '\u{2028}' => escaped.push_str("\\u2028"),
            '\u{2029}' => escaped.push_str("\\u2029"),
            _ => escaped.push(c),
        }
    }
    Ok(escaped)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_roundtrip_including_emoji() -> Result<()> {
        let text = "中文🍅 \" \\ test\nend";
        assert_eq!(
            json_transform(&json_transform(text, "unicode")?, "decode_unicode")?,
            text
        );
        assert_eq!(
            json_transform(&json_transform(text, "escape")?, "unescape")?,
            text
        );
        assert!(json_transform("{broken", "format").is_err());
        Ok(())
    }
    #[test]
    fn json_format_and_minify_preserve_original_member_order() -> Result<()> {
        let text = r#"{"z":1,"a":{"末":2,"初":3}}"#;
        let formatted = json_transform(text, "format")?;
        assert!(formatted.find("\"z\"") < formatted.find("\"a\""));
        assert_eq!(json_transform(&formatted, "minify")?, text);
        Ok(())
    }
    #[test]
    fn legacy_gson_and_text_actions_keep_html_numbers_and_manual_escape_semantics() -> Result<()> {
        let text = r#"{"z":"<>&='","n":123456789012345678901234567890,"e":1.00e+02}"#;
        assert_eq!(
            json_transform(text, "minify")?,
            r#"{"z":"\u003c\u003e\u0026\u003d\u0027","n":123456789012345678901234567890,"e":1.00e+02}"#
        );
        assert_eq!(
            json_transform("  \"a\"\n\tb  ", "escape")?,
            "\\\"a\\\"\\n\\tb"
        );
        assert_eq!(
            json_transform(r"\u4e2d\u6587 \uD83C\uDF45 \uXXXX", "decode_unicode")?,
            "中文 🍅 \\uXXXX"
        );
        assert!(json_transform(" \n\t", "format").is_err());
        assert_eq!(
            json_transform(
                r#"{"a":1E3,"a":2E4,"b":[-0,1e-00,{"x":3e2,"x":1.200E-02}]}"#,
                "minify"
            )?,
            r#"{"a":2E4,"b":[-0,1e-00,{"x":1.200E-02}]}"#
        );
        assert_eq!(
            json_transform(r#"{"a":null,"b":[null,{"c":null}]}"#, "minify")?,
            r#"{"b":[null,{}]}"#
        );
        Ok(())
    }
}
