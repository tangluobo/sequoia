//! HighlightedLogPane rules. Emit borrowed ranges, without duplicating log payloads.
use std::{ops::Range, sync::OnceLock};
pub const LIMIT: usize = 2_000_000;
/// Keep complete UTF-8 at the tail, reserving before append to avoid doubling a full buffer.
pub fn append(text: &mut String, mut incoming: &str) -> usize {
    let removed;
    if incoming.len() >= LIMIT {
        let mut cut = incoming.len() - LIMIT;
        while !incoming.is_char_boundary(cut) {
            cut += 1;
        }
        removed = text.len() + cut;
        text.clear();
        incoming = &incoming[cut..];
    } else {
        let mut cut = text
            .len()
            .saturating_add(incoming.len())
            .saturating_sub(LIMIT);
        while !text.is_char_boundary(cut) {
            cut += 1;
        }
        removed = cut;
        if cut > 0 {
            text.drain(..cut);
        }
    }
    let length = text.len() + incoming.len();
    if text.capacity() < length {
        let capacity = length.next_power_of_two().min(LIMIT + 65536);
        text.reserve_exact(capacity - text.len());
    }
    text.push_str(incoming);
    removed
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub color: [u8; 3],
    pub bold: bool,
}
pub fn style(line: &str) -> Style {
    let upper = line.to_uppercase();
    let (color, bold) = if upper.contains("[ERROR]")
        || upper.contains("EXCEPTION")
        || upper.contains("CAUSED BY:")
        || line.contains("失败")
    {
        ([198, 40, 40], true)
    } else if upper.contains("[WARNING]")
        || upper.contains("[WARN]")
        || upper.starts_with("WARNING:")
        || line.contains("警告")
    {
        ([217, 119, 6], false)
    } else if upper.contains("BUILD SUCCESS")
        || line.contains("启动成功")
        || line.contains("编译成功")
        || line.contains("运行中")
    {
        ([21, 128, 61], true)
    } else if upper.contains("命令：")
        || upper.starts_with("CMD.EXE ")
        || ["JAVA.EXE ", "MVN.CMD ", "MVNW.CMD ", "GRADLE.BAT "]
            .iter()
            .any(|s| upper.contains(s))
    {
        ([15, 118, 110], true)
    } else if ["[MAVEN]", "[GRADLE]", "[JAVA]", "[NPM]"]
        .iter()
        .any(|s| upper.starts_with(s))
    {
        ([37, 99, 235], true)
    } else if upper.contains(" DEBUG ") || upper.starts_with("[DEBUG]") {
        ([148, 163, 184], false)
    } else {
        ([55, 65, 81], false)
    };
    Style { color, bold }
}
pub fn fragments(text: &str, mut emit: impl FnMut(Range<usize>, Style, bool)) {
    static URL: OnceLock<regex::Regex> = OnceLock::new();
    let pattern = URL.get_or_init(|| {
        regex::Regex::new(r"(?i-u:\bhttps?)://[^\t\n\x0B\x0C\r ]+").expect("static log URL regex")
    });
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let base = style(line);
        let mut previous = 0;
        for found in pattern.find_iter(line) {
            let mut end = found.end();
            while end > found.start() && b"\"'.,;!?)]}>".contains(&line.as_bytes()[end - 1]) {
                end -= 1;
            }
            if previous < found.start() {
                emit(offset + previous..offset + found.start(), base, false);
            }
            if end > found.start() {
                emit(offset + found.start()..offset + end, base, true);
            }
            previous = end;
        }
        if previous < line.len() {
            emit(offset + previous..offset + line.len(), base, false);
        }
        offset += line.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_java_log_colors_and_unicode_url_boundaries() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/development/log-rules.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let text = case["text"].as_str().unwrap();
            let value = style(text);
            let [r, g, b] = value.color;
            let expected = format!(
                "-fx-fill: #{r:02x}{g:02x}{b:02x};{}",
                if value.bold {
                    " -fx-font-weight: bold;"
                } else {
                    ""
                }
            );
            assert_eq!(expected, case["style"].as_str().unwrap());
            let mut links = Vec::new();
            let mut chars = 0;
            let mut copied = String::new();
            fragments(text, |range, _, url| {
                let piece = &text[range];
                let count = piece.chars().count();
                if url {
                    links.push(serde_json::json!({"start":chars,"end":chars+count,"url":piece}));
                }
                chars += count;
                copied.push_str(piece);
            });
            assert_eq!(copied, text);
            assert_eq!(serde_json::json!(links), case["links"]);
        }
        let mut links = 0;
        fragments("httpſ://a.invalid", |_, _, link| {
            if link {
                links += 1
            }
        });
        assert_eq!(links, 0);
    }
}
