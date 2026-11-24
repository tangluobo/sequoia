//! Seekable nb3 reader: retain entry offsets, not the entire compressed/decompressed backup.
use anyhow::{Context, Result, ensure};
use serde::de::DeserializeOwned;
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
const MAX_INDEX: usize = 8 * 1024 * 1024;
const MAX_JSON: usize = 8 * 1024 * 1024;
mod index;
#[derive(Clone, Copy)]
struct Entry {
    offset: u64,
    size: u64,
}
pub struct Archive {
    file: File,
    entries: HashMap<String, Entry>,
}
impl Archive {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let entries = index::read(&mut file)?;
        Ok(Self { file, entries })
    }
    fn read<T>(&mut self, name: &str, read: impl FnOnce(&mut dyn Read) -> Result<T>) -> Result<T> {
        let entry = self
            .entries
            .get(name)
            .copied()
            .context("nb3 引用的条目不存在")?;
        self.file.seek(SeekFrom::Start(entry.offset))?;
        read(&mut (&mut self.file).take(entry.size))
    }
    pub fn json<T: DeserializeOwned>(&mut self, name: &str, compressed: bool) -> Result<T> {
        self.read(name, |input| {
            let mut reader: Box<dyn Read + '_> = if compressed {
                Box::new(flate2::read::GzDecoder::new(input))
            } else {
                Box::new(input)
            };
            let mut bytes = Vec::new();
            reader
                .by_ref()
                .take(MAX_JSON as u64 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= MAX_JSON, "nb3 元数据超过 8 MiB");
            Ok(serde_json::from_slice(&bytes)?)
        })
    }
    pub fn verify(&mut self, entry: &tomato_core::backup::Entry) -> Result<()> {
        self.verify_with(entry, || Ok(()))
    }
    pub fn verify_with(
        &mut self,
        entry: &tomato_core::backup::Entry,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<()> {
        use sha1::{Digest, Sha1};
        self.read(&entry.filename, |input| {
            let mut digest = Sha1::new();
            let mut buffer = [0; 64 * 1024];
            loop {
                check()?;
                let count = input.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                digest.update(&buffer[..count]);
            }
            ensure!(
                format!("{:x}", digest.finalize()).eq_ignore_ascii_case(&entry.checksum),
                "nb3 条目校验和不匹配"
            );
            Ok(())
        })
    }
    pub fn records(&mut self, name: &str) -> Result<super::Records<'_>> {
        self.records_with_strings(name, false)
    }
    pub fn records_with_strings(
        &mut self,
        name: &str,
        standard: bool,
    ) -> Result<super::Records<'_>> {
        let entry = self
            .entries
            .get(name)
            .copied()
            .context("nb3 引用的条目不存在")?;
        self.file.seek(SeekFrom::Start(entry.offset))?;
        let input = (&mut self.file).take(entry.size);
        Ok(if standard {
            super::Records::standard(input)
        } else {
            super::Records::new(input)
        })
    }
    pub fn rows(&mut self, name: &str, mut record: impl FnMut(&str) -> Result<()>) -> Result<()> {
        let mut rows = self.records(name)?;
        while let Some(row) = rows.next_record()? {
            record(row)?;
        }
        Ok(())
    }
}
