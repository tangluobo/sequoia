//! The JSON tool's legacy Gson grammar, without a second normalized input buffer.
//! Configuration files continue to use their own strict, lossless repositories.
use anyhow::{Result, bail, ensure};
use serde_json::{Map, Number, Value};

pub(super) fn parse(input: &str) -> Result<Value> {
    ensure!(input.len() <= 8 * 1024 * 1024, "JSON输入超过8 MiB");
    let mut reader = Reader {
        input,
        at: 0,
        remaining: 250_000,
    };
    if input.starts_with('\u{feff}') {
        reader.at = '\u{feff}'.len_utf8();
    }
    reader.space()?;
    if reader.tail().starts_with(")]}'\n") {
        reader.at += 5;
        reader.space()?;
    }
    if reader.at == input.len() {
        return Ok(Value::Null);
    }
    let value = reader.value(0)?;
    // JsonParser.parseString restores the reader's strictness before checking
    // consumption, and skips that check for a null root. Preserve both details.
    if !value.is_null() {
        reader.whitespace();
        ensure!(
            reader.at == input.len(),
            "JSON末尾存在多余内容（字节{}）",
            reader.at
        );
    }
    Ok(value)
}

struct Reader<'a> {
    input: &'a str,
    at: usize,
    remaining: usize,
}
impl Reader<'_> {
    fn tail(&self) -> &str {
        &self.input[self.at..]
    }
    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.at).copied()
    }
    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.at += 1;
        }
    }
    fn space(&mut self) -> Result<()> {
        loop {
            self.whitespace();
            if self.tail().starts_with("/*") {
                let end = self.tail()[2..].find("*/");
                let Some(end) = end else {
                    bail!("JSON注释未结束（字节{}）", self.at)
                };
                self.at += end + 4;
            } else if self.tail().starts_with("//") || self.peek() == Some(b'#') {
                self.at += self.tail().find(['\r', '\n']).unwrap_or(self.tail().len());
            } else {
                return Ok(());
            }
        }
    }
    fn take(&mut self, byte: u8) -> bool {
        if self.peek() != Some(byte) {
            return false;
        }
        self.at += 1;
        true
    }
    fn value(&mut self, depth: usize) -> Result<Value> {
        ensure!(depth <= 128, "JSON嵌套超过128层");
        self.node()?;
        self.space()?;
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'\'' | b'"') => Ok(Value::String(self.quoted()?)),
            _ => {
                let token = self.literal()?;
                if token.eq_ignore_ascii_case("null") {
                    Ok(Value::Null)
                } else if token.eq_ignore_ascii_case("true") {
                    Ok(Value::Bool(true))
                } else if token.eq_ignore_ascii_case("false") {
                    Ok(Value::Bool(false))
                } else if number(token) {
                    // Validated ASCII numeric token; retain spelling and precision.
                    Ok(Value::Number(Number::from_string_unchecked(
                        token.to_owned(),
                    )))
                } else {
                    Ok(Value::String(token.to_owned()))
                }
            }
        }
    }
    fn object(&mut self, depth: usize) -> Result<Value> {
        self.at += 1;
        let mut object = Map::new();
        self.space()?;
        if self.take(b'}') {
            return Ok(Value::Object(object));
        }
        loop {
            self.space()?;
            let key = match self.peek() {
                Some(b'\'' | b'"') => self.quoted()?,
                _ => self.literal()?.to_owned(),
            };
            self.space()?;
            if self.take(b'=') {
                self.take(b'>');
            } else {
                ensure!(self.take(b':'), "JSON对象缺少冒号（字节{}）", self.at);
            }
            object.insert(key, self.value(depth + 1)?);
            self.space()?;
            if self.take(b'}') {
                return Ok(Value::Object(object));
            }
            ensure!(
                self.take(b',') || self.take(b';'),
                "JSON对象缺少分隔符（字节{}）",
                self.at
            );
        }
    }
    fn array(&mut self, depth: usize) -> Result<Value> {
        self.at += 1;
        let mut array = Vec::new();
        self.space()?;
        if self.take(b']') {
            return Ok(Value::Array(array));
        }
        loop {
            self.space()?;
            array.push(if matches!(self.peek(), Some(b',' | b';' | b']')) {
                self.node()?;
                Value::Null
            } else {
                self.value(depth + 1)?
            });
            self.space()?;
            if self.take(b']') {
                return Ok(Value::Array(array));
            }
            ensure!(
                self.take(b',') || self.take(b';'),
                "JSON数组缺少分隔符（字节{}）",
                self.at
            );
        }
    }
    fn node(&mut self) -> Result<()> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or_else(|| anyhow::anyhow!("JSON节点超过250000个"))?;
        Ok(())
    }
    fn literal(&mut self) -> Result<&str> {
        let start = self.at;
        // Quotes inside an unquoted token are literal, as in the original reader.
        while self
            .peek()
            .is_some_and(|b| !b"{}[]:,;=\\/# \t\r\n\x0c".contains(&b))
        {
            self.at += 1;
        }
        ensure!(start != self.at, "JSON缺少值或名称（字节{}）", self.at);
        Ok(&self.input[start..self.at])
    }
    fn quoted(&mut self) -> Result<String> {
        let quote = self.peek().expect("quoted token");
        self.at += 1;
        let mut text = String::new();
        loop {
            let start = self.at;
            while self.peek().is_some_and(|b| b != quote && b != b'\\') {
                self.at += 1;
            }
            text.push_str(&self.input[start..self.at]);
            if self.take(quote) {
                return Ok(text);
            }
            ensure!(self.take(b'\\'), "JSON字符串未结束（字节{}）", self.at);
            let escape = self.peek();
            self.at += usize::from(escape.is_some());
            text.push(match escape {
                Some(b'"' | b'\'' | b'\\' | b'/') => char::from(escape.unwrap()),
                Some(b'b') => '\x08',
                Some(b'f') => '\x0c',
                Some(b'n' | b'\n') => '\n',
                Some(b'r' | b'\r') => '\r',
                Some(b't') => '\t',
                Some(b'u') => self.unicode()?,
                _ => bail!("JSON无效转义（字节{}）", self.at),
            });
        }
    }
    fn hex(&mut self) -> Result<u16> {
        let hex = self.input.as_bytes().get(self.at..self.at + 4);
        ensure!(
            hex.is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)),
            "JSON无效Unicode转义（字节{}）",
            self.at
        );
        let value = u16::from_str_radix(&self.input[self.at..self.at + 4], 16)?;
        self.at += 4;
        Ok(value)
    }
    fn unicode(&mut self) -> Result<char> {
        let first = self.hex()?;
        if (0xd800..=0xdbff).contains(&first) {
            ensure!(
                self.tail().starts_with("\\u"),
                "JSON包含孤立UTF-16代理项（字节{}）",
                self.at
            );
            self.at += 2;
            let second = self.hex()?;
            ensure!(
                (0xdc00..=0xdfff).contains(&second),
                "JSON包含孤立UTF-16代理项（字节{}）",
                self.at
            );
            return Ok(char::from_u32(
                0x10000 + ((u32::from(first) - 0xd800) << 10) + u32::from(second) - 0xdc00,
            )
            .expect("validated surrogate pair"));
        }
        char::from_u32(u32::from(first))
            .ok_or_else(|| anyhow::anyhow!("JSON包含孤立UTF-16代理项（字节{}）", self.at))
    }
}
fn number(token: &str) -> bool {
    // Gson's numeric lookahead buffer is 1024 UTF-16 code units. A token which
    // fills that buffer is read as an unquoted string, rather than a number.
    if token.len() >= 1024 {
        return false;
    }
    let bytes = token.as_bytes();
    let mut at = usize::from(bytes.first() == Some(&b'-'));
    if bytes.get(at) == Some(&b'0') {
        at += 1;
    } else {
        let start = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == start {
            return false;
        }
    }
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        let start = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if start == at {
            return false;
        }
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(bytes.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let start = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if start == at {
            return false;
        }
    }
    at == bytes.len()
}
