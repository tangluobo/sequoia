//! Keep only quote flags while csv handles decoding; no duplicate row payload is retained.
use super::*;
use std::{cell::RefCell, collections::VecDeque, rc::Rc};
pub(super) type Flags = Rc<RefCell<VecDeque<Vec<bool>>>>;
pub(super) struct Quoted<R> {
    input: R,
    delimiter: u8,
    quote: Option<u8>,
    inside: bool,
    after_quote: bool,
    start: bool,
    content: bool,
    fields: Vec<bool>,
    flags: Flags,
    eof: bool,
}
impl<R: Read> Quoted<R> {
    pub(super) fn new(input: R, source: &Source) -> (Self, Flags) {
        let flags = Flags::default();
        (
            Self {
                input,
                delimiter: source.delimiter as u8,
                quote: source.quote.map(|q| q as u8),
                inside: false,
                after_quote: false,
                start: true,
                content: false,
                fields: Vec::new(),
                flags: flags.clone(),
                eof: false,
            },
            flags,
        )
    }
    fn finish(&mut self) {
        if self.content {
            if self.start {
                self.fields.push(false);
            }
            self.flags
                .borrow_mut()
                .push_back(std::mem::take(&mut self.fields));
        }
        self.start = true;
        self.content = false;
        self.inside = false;
        self.after_quote = false;
    }
    fn byte(&mut self, b: u8) -> io::Result<()> {
        if self.inside {
            if Some(b) == self.quote {
                self.after_quote = !self.after_quote;
                return Ok(());
            }
            if !self.after_quote {
                return Ok(());
            }
            self.inside = false;
            self.after_quote = false;
        }
        if b == b'\r' || b == b'\n' {
            self.finish();
            return Ok(());
        }
        if b == self.delimiter {
            if self.start {
                self.fields.push(false);
            }
            self.start = true;
            self.content = true;
        } else {
            if self.start {
                self.fields.push(Some(b) == self.quote);
                self.inside = Some(b) == self.quote;
            }
            self.start = false;
            self.content = true;
        }
        if self.fields.len() > MAX_COLUMNS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "源字段超过 16384 列",
            ));
        }
        Ok(())
    }
}
impl<R: Read> Read for Quoted<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let n = self.input.read(out)?;
        for &b in &out[..n] {
            self.byte(b)?;
        }
        if n == 0 && !self.eof {
            self.finish();
            self.eof = true;
        }
        Ok(n)
    }
}
