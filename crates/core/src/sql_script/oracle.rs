//! Borrowed Oracle lexer: SQL*Plus units and alternative-quoted literals.
pub(super) struct Statements<'a> {
    remaining: &'a str,
}
impl<'a> Statements<'a> {
    pub(super) fn new(text: &'a str) -> Self {
        Self { remaining: text }
    }
}
pub(super) fn procedural(text: &str) -> bool {
    let text = super::strip_leading(text);
    let mut words = text
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|s| !s.is_empty());
    let Some(first) = words.next() else {
        return false;
    };
    if first.eq_ignore_ascii_case("DECLARE") || first.eq_ignore_ascii_case("BEGIN") {
        return true;
    }
    if !first.eq_ignore_ascii_case("CREATE") {
        return false;
    }
    for word in words.take(5) {
        if ["OR", "REPLACE", "EDITIONABLE", "NONEDITIONABLE"]
            .iter()
            .any(|s| word.eq_ignore_ascii_case(s))
        {
            continue;
        }
        return ["PROCEDURE", "FUNCTION", "PACKAGE", "TRIGGER", "TYPE"]
            .iter()
            .any(|s| word.eq_ignore_ascii_case(s));
    }
    false
}
impl<'a> Iterator for Statements<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let text = self.remaining.trim_start_matches(|c: char| c <= ' ');
            if text.is_empty() {
                self.remaining = "";
                return None;
            }
            let block = procedural(text);
            let bytes = text.as_bytes();
            let mut index = 0;
            let mut quote = 0;
            let mut alternative = 0;
            let mut comment = 0;
            let mut line_start = 0;
            let mut end = bytes.len();
            let mut consumed = end;
            while index < bytes.len() {
                let c = bytes[index];
                let n = bytes.get(index + 1).copied().unwrap_or(0);
                if comment == 1 {
                    if c == b'\n' {
                        comment = 0;
                        line_start = index + 1;
                    }
                } else if comment == 2 {
                    if c == b'*' && n == b'/' {
                        comment = 0;
                        index += 1;
                    }
                } else if alternative != 0 {
                    if c == alternative && n == b'\'' {
                        alternative = 0;
                        index += 1;
                    }
                } else if quote != 0 {
                    if c == quote {
                        if n == quote {
                            index += 1;
                        } else {
                            quote = 0;
                        }
                    }
                } else if c == b'-' && n == b'-' {
                    comment = 1;
                    index += 1;
                } else if c == b'/' && n == b'*' {
                    comment = 2;
                    index += 1;
                } else if (c == b'q' || c == b'Q') && n == b'\'' && index + 2 < bytes.len() {
                    let delimiter = bytes[index + 2];
                    alternative = match delimiter {
                        b'[' => b']',
                        b'{' => b'}',
                        b'(' => b')',
                        b'<' => b'>',
                        other => other,
                    };
                    index += 2;
                } else if c == b'\'' || c == b'"' {
                    quote = c;
                } else if c == b';' && !block {
                    end = index;
                    consumed = index + 1;
                    break;
                } else if c == b'/' && text[line_start..index].trim().is_empty() {
                    let after = text[index + 1..].split('\n').next().unwrap_or_default();
                    if after.trim().is_empty() {
                        end = line_start;
                        consumed = index + 1 + after.len();
                        break;
                    }
                }
                if c == b'\n' {
                    line_start = index + 1;
                }
                index += 1;
            }
            self.remaining = &text[consumed..];
            let statement = super::trim(&text[..end]);
            if !statement.is_empty() {
                return Some(statement);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_procedural_units_q_literals_and_unicode_ranges() {
        let text = "-- header\nDECLARE x VARCHAR2(100):=q'[a';b]'; BEGIN x:=q'!雪;!'; END;\n/\nCREATE OR REPLACE PROCEDURE p AS BEGIN NULL; END;\n / \nSELECT q'{x';y}' FROM dual; SELECT '尾' FROM dual;";
        let parts: Vec<_> = super::Statements::new(text).collect();
        assert_eq!(parts.len(), 4);
        assert!(parts[0].ends_with("END;"));
        assert!(parts[1].ends_with("END;"));
        assert_eq!(parts[2], "SELECT q'{x';y}' FROM dual");
        assert_eq!(parts[3], "SELECT '尾' FROM dual");
        assert_eq!(
            super::Statements::new("BEGIN NULL; END;").next(),
            Some("BEGIN NULL; END;")
        );
    }
}
