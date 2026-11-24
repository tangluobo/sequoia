//! Small VT control side channel: display parsing remains owned by vt100.
#[derive(Default)]
pub struct Control {
    sequence: Vec<u8>,
    osc: bool,
}
#[derive(Default)]
pub struct Actions {
    pub replies: Vec<String>,
    pub directory: Option<String>,
}
impl Control {
    pub fn process(&mut self, bytes: &[u8], parser: &mut vt100::Parser) -> Actions {
        let mut actions = Actions::default();
        for &byte in bytes {
            parser.process(&[byte]);
            if self.sequence.is_empty() {
                if byte == 27 {
                    self.sequence.push(byte);
                }
                continue;
            }
            self.sequence.push(byte);
            if self.sequence.len() > 4096 {
                self.sequence.clear();
                self.osc = false;
                continue;
            }
            if self.sequence.len() == 2 {
                match byte {
                    b']' => self.osc = true,
                    b'[' => {}
                    b'Z' => {
                        actions.replies.push("\x1b[?62;4c".into());
                        self.sequence.clear();
                    }
                    _ => self.sequence.clear(),
                };
                continue;
            }
            if self.osc {
                if byte == 7 || self.sequence.ends_with(b"\x1b\\") {
                    let end = self.sequence.len() - if byte == 7 { 1 } else { 2 };
                    if let Some(path) = directory(&self.sequence[2..end]) {
                        actions.directory = Some(path);
                    }
                    self.sequence.clear();
                    self.osc = false;
                }
            } else if (0x40..=0x7e).contains(&byte) {
                let reply = match self.sequence.as_slice() {
                    b"\x1b[c" | b"\x1b[0c" => Some("\x1b[?62;4c".into()),
                    b"\x1b[>c" | b"\x1b[>0c" => Some("\x1b[>0;136;0c".into()),
                    b"\x1b[5n" => Some("\x1b[0n".into()),
                    b"\x1b[6n" | b"\x1b[?6n" => {
                        let (row, col) = parser.screen().cursor_position();
                        Some(format!(
                            "\x1b[{}{};{}R",
                            if self.sequence[2] == b'?' { "?" } else { "" },
                            row + 1,
                            col + 1
                        ))
                    }
                    _ => None,
                };
                if let Some(reply) = reply {
                    actions.replies.push(reply);
                }
                self.sequence.clear();
            }
        }
        actions
    }
}
fn directory(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?.strip_prefix("7;file://")?;
    let path = &text[text.find('/')?..];
    let mut decoded = Vec::new();
    let mut iter = path.bytes();
    while let Some(byte) = iter.next() {
        if byte == b'%' {
            let hi = (iter.next()? as char).to_digit(16)?;
            let lo = (iter.next()? as char).to_digit(16)?;
            decoded.push((hi * 16 + lo) as u8);
        } else {
            decoded.push(byte);
        }
    }
    let path = String::from_utf8(decoded).ok()?;
    (!path.contains(['\0', '\r', '\n'])).then_some(path)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_controls_reply_using_current_cursor_and_decode_cwd() {
        let mut c = Control::default();
        let mut p = vt100::Parser::new(24, 80, 0);
        assert!(c.process(b"hello\x1b[", &mut p).replies.is_empty());
        assert_eq!(c.process(b"6n", &mut p).replies, ["\x1b[1;6R"]);
        assert_eq!(
            c.process(b"\x1b]7;file://host/home/%E4%B8%AD\x1b\\", &mut p)
                .directory
                .as_deref(),
            Some("/home/中")
        );
        assert_eq!(p.screen().contents(), "hello");
    }
    #[test]
    fn malformed_or_large_osc_does_not_keep_growing() {
        let mut c = Control::default();
        let mut p = vt100::Parser::default();
        c.process(b"\x1b]7;", &mut p);
        c.process(&vec![b'x'; 9000], &mut p);
        assert!(c.sequence.len() <= 4096);
        assert_eq!(directory(b"7;file:///tmp/%00"), None);
    }
}
