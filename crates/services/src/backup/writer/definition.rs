//! Append the Data array as pages complete; no vector of all page descriptors is retained.
use super::{Limited, MAX_JSON, Page, Writer};
use anyhow::{Result, ensure};
use std::{io::Write, path::Path};
use tomato_core::backup::{Definition, Entry};
struct WithoutBrace<W> {
    inner: W,
    last: Option<u8>,
}
impl<W: Write> Write for WithoutBrace<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if let Some((&last, prefix)) = bytes.split_last() {
            if let Some(previous) = self.last {
                self.inner.write_all(&[previous])?;
            }
            self.inner.write_all(prefix)?;
            self.last = Some(last);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
pub struct DefinitionPage {
    output: Limited<Page>,
    first: bool,
}
impl DefinitionPage {
    pub fn new(directory: &Path, definition: &Definition) -> Result<Self> {
        ensure!(
            definition.data.is_none(),
            "流式备份元数据不能包含已有数据页"
        );
        let mut output = Limited {
            inner: Page::new(directory)?,
            remaining: MAX_JSON,
        };
        {
            let mut header = WithoutBrace {
                inner: &mut output,
                last: None,
            };
            serde_json::to_writer_pretty(&mut header, definition)?;
            ensure!(header.last == Some(b'}'), "备份元数据不是 JSON 对象");
        }
        output.write_all(b",\"Data\":[")?;
        Ok(Self {
            output,
            first: true,
        })
    }
    pub fn data(&mut self, entry: &Entry) -> Result<()> {
        if !self.first {
            self.output.write_all(b",")?;
        }
        serde_json::to_writer(&mut self.output, entry)?;
        self.first = false;
        Ok(())
    }
    pub fn finish(mut self, writer: &mut Writer, name: &str) -> Result<Entry> {
        self.output.write_all(b"]}")?;
        writer.page(name, self.output.inner)
    }
}
