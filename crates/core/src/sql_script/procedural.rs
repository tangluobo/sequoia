//! Borrowed dialect lexer for PostgreSQL dollar strings and MySQL stored programs.
//! https://www.postgresql.org/docs/current/sql-syntax-lexical.html
//! https://dev.mysql.com/doc/refman/8.4/en/stored-programs-defining.html
pub(super) struct Statements<'a> {
    remaining: &'a str,
    delimiter: &'a str,
    mysql: bool,
    backslash: bool,
}
impl<'a> Statements<'a> {
    #[cfg(test)]
    pub fn new(sql: &'a str, mysql: bool) -> Self {
        Self::with_escape(sql, mysql, mysql)
    }
    pub fn with_escape(sql: &'a str, mysql: bool, backslash: bool) -> Self {
        Self {
            remaining: sql,
            delimiter: ";",
            mysql,
            backslash,
        }
    }
    pub fn set_backslash(&mut self, value: bool) {
        self.backslash = value;
    }
}

fn word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 128
}
fn following(text: &str) -> &str {
    let text = super::strip_leading(text);
    &text[..text.bytes().take_while(|b| word(*b)).count()]
}
impl<'a> Iterator for Statements<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.remaining.trim().is_empty() {
                self.remaining = "";
                return None;
            }
            if self.mysql {
                let mut head = super::strip_leading(self.remaining);
                while let Some(comment) = head.strip_prefix('#') {
                    head =
                        super::strip_leading(comment.split_once('\n').map_or("", |(_, rest)| rest));
                }
                if head
                    .get(..9)
                    .is_some_and(|s| s.eq_ignore_ascii_case("DELIMITER"))
                    && head.as_bytes().get(9).is_some_and(u8::is_ascii_whitespace)
                {
                    let (line, rest) = head.split_once('\n').unwrap_or((head, ""));
                    let delimiter = line[9..].trim();
                    if !delimiter.is_empty()
                        && delimiter.len() <= 32
                        && !delimiter.contains(char::is_whitespace)
                    {
                        self.delimiter = delimiter;
                        self.remaining = rest;
                        continue;
                    }
                }
            }
            let text = self.remaining;
            let bytes = text.as_bytes();
            let mut i = 0;
            let mut create = false;
            let mut routine = false;
            let mut blocks = 0usize;
            let mut skip_suffix = false;
            let mut end = bytes.len();
            while i < bytes.len() {
                let byte = bytes[i];
                if bytes[i..].starts_with(self.delimiter.as_bytes())
                    && (self.delimiter != ";" || blocks == 0)
                {
                    end = i;
                    break;
                }
                if bytes[i..].starts_with(b"--") || self.mysql && byte == b'#' {
                    i = text[i..].find('\n').map_or(bytes.len(), |n| i + n + 1);
                    continue;
                }
                if bytes[i..].starts_with(b"/*") {
                    i += 2;
                    let mut depth = 1usize;
                    while i < bytes.len() && depth > 0 {
                        if !self.mysql && bytes[i..].starts_with(b"/*") {
                            depth += 1;
                            i += 2;
                        } else if bytes[i..].starts_with(b"*/") {
                            depth -= 1;
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                    continue;
                }
                if matches!(byte, b'\'' | b'"') || self.mysql && byte == b'`' {
                    let escape = if self.mysql {
                        self.backslash && byte != b'`'
                    } else {
                        byte == b'\''
                            && (self.backslash
                                || i > 0
                                    && matches!(bytes[i - 1], b'e' | b'E')
                                    && (i == 1 || !word(bytes[i - 2])))
                    };
                    let quote = byte;
                    i += 1;
                    while i < bytes.len() {
                        if escape && bytes[i] == b'\\' {
                            i = (i + 2).min(bytes.len());
                        } else if bytes[i] == quote {
                            i += 1;
                            if bytes.get(i) == Some(&quote) {
                                i += 1;
                            } else {
                                break;
                            }
                        } else {
                            i += 1;
                        }
                    }
                    continue;
                }
                if !self.mysql
                    && byte == b'$'
                    && (i == 0 || !word(bytes[i - 1]) && bytes[i - 1] != b'$')
                {
                    let tail = &text[i + 1..];
                    if let Some(length) = tail.find('$') {
                        let tag = &tail[..length];
                        if tag.chars().enumerate().all(|(n, c)| {
                            c == '_' || c.is_alphabetic() || n > 0 && c.is_ascii_digit()
                        }) {
                            let mark = &text[i..i + length + 2];
                            let begin = i + mark.len();
                            i = text[begin..]
                                .find(mark)
                                .map_or(bytes.len(), |n| begin + n + mark.len());
                            continue;
                        }
                    }
                }
                if self.mysql && (byte.is_ascii_alphabetic() || byte == b'_' || byte >= 128) {
                    let start = i;
                    i += 1;
                    while i < bytes.len() && word(bytes[i]) {
                        i += 1;
                    }
                    let token = &text[start..i];
                    if bytes[..start]
                        .iter()
                        .rev()
                        .find(|b| !b.is_ascii_whitespace())
                        .copied()
                        == Some(b'.')
                    {
                        continue;
                    }
                    if skip_suffix {
                        skip_suffix = false;
                        continue;
                    }
                    if token.eq_ignore_ascii_case("CREATE") {
                        create = true;
                    }
                    if create
                        && ["TRIGGER", "PROCEDURE", "FUNCTION", "EVENT"]
                            .iter()
                            .any(|s| token.eq_ignore_ascii_case(s))
                    {
                        routine = true;
                    }
                    if routine {
                        if token.eq_ignore_ascii_case("BEGIN")
                            || blocks > 0 && token.eq_ignore_ascii_case("CASE")
                        {
                            blocks = blocks.saturating_add(1);
                        } else if token.eq_ignore_ascii_case("END") {
                            let suffix = following(&text[i..]);
                            if ["IF", "LOOP", "WHILE", "REPEAT"]
                                .iter()
                                .any(|s| suffix.eq_ignore_ascii_case(s))
                            {
                                skip_suffix = true;
                            } else {
                                blocks = blocks.saturating_sub(1);
                                skip_suffix = suffix.eq_ignore_ascii_case("CASE");
                            }
                        }
                    }
                    continue;
                }
                i += 1;
            }
            self.remaining = if end < bytes.len() {
                &text[end + self.delimiter.len()..]
            } else {
                ""
            };
            let statement = text[..end].trim();
            if !statement.is_empty() {
                return Some(statement);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_mode_can_change_without_copying_the_remaining_script() {
        let source = r"SELECT 'C:\'; SELECT 'it\'s;ok';";
        let mut rows = super::super::split_for_session("MYSQL", Some(source), false);
        let first = rows.next().unwrap();
        assert_eq!(first, r"SELECT 'C:\'");
        rows.set_backslash(true);
        let second = rows.next().unwrap();
        assert_eq!(second, r"SELECT 'it\'s;ok'");
        assert!(
            second.as_ptr() as usize >= source.as_ptr() as usize
                && second.as_ptr() as usize + second.len()
                    <= source.as_ptr() as usize + source.len()
        );
        assert!(rows.next().is_none());
    }
    #[test]
    fn postgres_body_tags_nested_comments_and_escaped_quotes_do_not_split_functions() {
        let sql = "/* outer /* ; */ ; */ CREATE FUNCTION f() RETURNS text AS $中文_tag$ BEGIN RETURN $$a;b$$; END; $中文_tag$ LANGUAGE plpgsql; SELECT E'a\\\';b', $1, foo$tag$;";
        let statements = Statements::new(sql, false).collect::<Vec<_>>();
        assert_eq!(statements.len(), 2);
        assert!(statements[0].ends_with("LANGUAGE plpgsql"));
        assert!(statements[1].starts_with("SELECT"));
        assert_eq!(
            Statements::new("SELECT $bad$unclosed; SELECT 2;", false).count(),
            1
        );
    }
    #[test]
    fn mysql_directives_and_nested_program_blocks_are_single_commands() {
        let sql = "# client directive\n-- client directive\nDELIMITER //\nCREATE TRIGGER t BEFORE INSERT ON `a;b` FOR EACH ROW BEGIN IF NEW.x=1 THEN SET NEW.x=CASE WHEN NEW.x=1 THEN 2 ELSE 3 END; END IF; BEGIN SET NEW.x=4; END; END//\nDELIMITER ;\nSELECT 'a;\\\'b'; SELECT 2;";
        let rows = Statements::new(sql, true).collect::<Vec<_>>();
        assert_eq!(rows.len(), 3);
        assert!(rows[0].ends_with("END"));
        assert!(!rows.iter().any(|s| s.starts_with("DELIMITER")));
        assert_eq!(Statements::new("CREATE TRIGGER t BEFORE INSERT ON x FOR EACH ROW BEGIN SET NEW.a=1; SET NEW.b=2; END; SELECT 1;",true).count(),2);
        assert_eq!(
            Statements::new("BEGIN; UPDATE t SET x=1; COMMIT;", true).count(),
            3
        );
        assert_eq!(Statements::new("CREATE TRIGGER t BEFORE INSERT ON x FOR EACH ROW BEGIN SET NEW. end=1; SET 中文BEGIN=2; END; SELECT 2;",true).count(),2);
    }
}
