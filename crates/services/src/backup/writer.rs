//! Atomic nb3 writer. Gzip pages spill to disk before their TAR headers are emitted.
use anyhow::{Result, ensure};
use sha1::{Digest, Sha1};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use tomato_core::backup::Entry;
const MAX_JSON: usize = 8 * 1024 * 1024;
struct Limited<W> {
    inner: W,
    remaining: usize,
}
impl<W: Write> Write for Limited<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(std::io::Error::other("nb3 元数据超过 8 MiB"));
        }
        let count = self.inner.write(bytes)?;
        self.remaining -= count;
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
struct Hashed<W> {
    inner: W,
    checksum: Sha1,
    size: u64,
}
impl<W: Write> Write for Hashed<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let count = self.inner.write(bytes)?;
        self.checksum.update(&bytes[..count]);
        self.size += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
pub struct Page {
    temporary: tempfile::NamedTempFile,
    encoder: flate2::write::GzEncoder<Hashed<File>>,
    records: usize,
    failed: bool,
}
impl Page {
    pub fn new(directory: &Path) -> Result<Self> {
        let temporary = tempfile::NamedTempFile::new_in(directory)?;
        let encoder = flate2::write::GzEncoder::new(
            Hashed {
                inner: temporary.reopen()?,
                checksum: Sha1::new(),
                size: 0,
            },
            flate2::Compression::default(),
        );
        Ok(Self {
            temporary,
            encoder,
            records: 0,
            failed: false,
        })
    }
}
impl Page {
    pub fn record(&mut self, write: impl FnOnce(&mut dyn Write) -> Result<()>) -> Result<()> {
        ensure!(!self.failed, "备份数据页已失败");
        let result = (|| {
            if self.records > 0 {
                self.encoder
                    .write_all(tomato_core::backup::RECORD_SEPARATOR.as_bytes())?;
            }
            write(&mut Limited {
                inner: &mut self.encoder,
                remaining: 32 * 1024 * 1024,
            })
        })();
        if result.is_ok() {
            self.records += 1;
        } else {
            self.failed = true;
        }
        result
    }
}
impl Write for Page {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.encoder.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.encoder.flush()
    }
}
pub struct Writer {
    temporary: tempfile::NamedTempFile,
    archive: tar::Builder<File>,
    directory: PathBuf,
}
impl Writer {
    pub fn new(directory: &Path) -> Result<Self> {
        let temporary = tempfile::NamedTempFile::new_in(directory)?;
        let archive = tar::Builder::new(temporary.reopen()?);
        Ok(Self {
            temporary,
            archive,
            directory: directory.to_owned(),
        })
    }
    fn append(&mut self, name: &str, size: u64, reader: impl Read) -> Result<()> {
        let mut header = tar::Header::new_ustar();
        header.set_size(size);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();
        self.archive.append_data(&mut header, name, reader)?;
        Ok(())
    }
    pub fn page(&mut self, name: &str, page: Page) -> Result<Entry> {
        let Page {
            mut temporary,
            encoder,
            failed,
            ..
        } = page;
        ensure!(!failed, "备份数据页未完整写入");
        let result = encoder.finish()?;
        let size = result.size;
        let checksum = format!("{:x}", result.checksum.finalize());
        drop(result.inner);
        temporary.as_file_mut().seek(SeekFrom::Start(0))?;
        self.append(name, size, temporary.as_file_mut())?;
        Ok(Entry {
            filename: name.into(),
            checksum,
        })
    }
    pub fn json<T: serde::Serialize>(&mut self, name: &str, value: &T) -> Result<Entry> {
        let mut page = Page::new(&self.directory)?;
        serde_json::to_writer_pretty(
            Limited {
                inner: &mut page,
                remaining: MAX_JSON,
            },
            value,
        )?;
        self.page(name, page)
    }
    pub fn finish(
        mut self,
        destination: &Path,
        metadata: &tomato_core::backup::Metadata,
    ) -> Result<()> {
        ensure!(
            destination.parent() == Some(self.directory.as_path()),
            "备份目标与临时目录不同"
        );
        let mut output = Limited {
            inner: Vec::new(),
            remaining: MAX_JSON,
        };
        serde_json::to_writer_pretty(&mut output, metadata)?;
        let json = output.inner;
        self.append("meta.json", json.len() as u64, json.as_slice())?;
        let output = self.archive.into_inner()?;
        output.sync_all()?;
        drop(output);
        self.temporary
            .persist(destination)
            .map_err(|error| error.error)?;
        Ok(())
    }
}

mod definition;
pub use definition::DefinitionPage;
