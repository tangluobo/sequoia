//! Lending record reader lets restore await each database write while reusing one row buffer.
use anyhow::{Result, ensure};
use std::{
    fs::File,
    io::{Read, Take},
};
const MAX_ROW: usize = 32 * 1024 * 1024;
pub struct Records<'a> {
    reader: flate2::read::GzDecoder<Take<&'a mut File>>,
    buffer: Box<[u8; 64 * 1024]>,
    position: usize,
    length: usize,
    row: Vec<u8>,
    quoted: bool,
    escaped: bool,
    ended: bool,
    backslash: bool,
}
impl<'a> Records<'a> {
    pub(super) fn new(input: Take<&'a mut File>) -> Self {
        Self {
            reader: flate2::read::GzDecoder::new(input),
            buffer: Box::new([0; 64 * 1024]),
            position: 0,
            length: 0,
            row: Vec::new(),
            quoted: false,
            escaped: false,
            ended: false,
            backslash: true,
        }
    }
    pub(super) fn standard(input: Take<&'a mut File>) -> Self {
        let mut reader = Self::new(input);
        reader.backslash = false;
        reader
    }
    fn fill(&mut self) -> Result<bool> {
        if self.ended {
            return Ok(false);
        }
        loop {
            if self.position == self.length {
                self.length = self.reader.read(self.buffer.as_mut())?;
                self.position = 0;
                if self.length == 0 {
                    self.ended = true;
                    ensure!(!self.quoted, "nb3 记录字符串未闭合");
                    return Ok(!self.row.is_empty());
                }
            }
            let byte = self.buffer[self.position];
            self.position += 1;
            if byte == 0x1e && !self.quoted {
                return Ok(true);
            }
            ensure!(self.row.len() < MAX_ROW, "nb3 单条 SQL 记录超过 32 MiB");
            self.row.push(byte);
            if self.escaped {
                self.escaped = false;
            } else if self.backslash && self.quoted && byte == b'\\' {
                self.escaped = true;
            } else if byte == b'\'' {
                self.quoted = !self.quoted;
            }
        }
    }
    pub fn next_record(&mut self) -> Result<Option<&str>> {
        loop {
            self.row.clear();
            if !self.fill()? {
                return Ok(None);
            }
            if self.row.iter().all(|&byte| byte <= 32) {
                continue;
            }
            return Ok(Some(
                std::str::from_utf8(&self.row)?.trim_matches(|c: char| c <= ' '),
            ));
        }
    }
}
