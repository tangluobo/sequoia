//! Only short regular nb3 entries are needed; reject extensions before tar can buffer them.
use super::{Entry, MAX_INDEX};
use anyhow::{Result, ensure};
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek},
};
pub(super) fn read(file: &mut File) -> Result<HashMap<String, Entry>> {
    let length = file.metadata()?.len();
    scan(file, length)
}
fn scan(input: &mut (impl Read + Seek), length: u64) -> Result<HashMap<String, Entry>> {
    let mut entries = HashMap::new();
    let mut names = 0usize;
    let mut archive = tar::Archive::new(input);
    for entry in archive.entries_with_seek()?.raw(true) {
        let entry = entry?;
        let offset = entry.raw_file_position();
        let size = entry.size();
        ensure!(
            offset.checked_add(size).is_some_and(|end| end <= length),
            "nb3 条目超出文件范围"
        );
        let kind = entry.header().entry_type();
        ensure!(kind.is_file() || kind.is_dir(), "nb3 不支持 TAR 扩展条目");
        if kind.is_dir() {
            ensure!(size == 0, "nb3 目录条目不应包含数据");
            continue;
        }
        let name = entry.path()?.to_string_lossy().into_owned();
        ensure!(!entries.contains_key(&name), "nb3 包含重复条目");
        names = names.saturating_add(name.capacity());
        entries.insert(name, Entry { offset, size });
        let held = names.saturating_add(
            entries
                .capacity()
                .saturating_mul(std::mem::size_of::<(String, Entry)>() + 24),
        );
        ensure!(held <= MAX_INDEX, "nb3 条目索引超过 8 MiB");
    }
    Ok(entries)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, SeekFrom};
    struct HeaderOnly(Cursor<Vec<u8>>);
    impl Read for HeaderOnly {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            assert!(
                self.0.position() < 512,
                "extension payload must never be read or buffered"
            );
            self.0.read(buffer)
        }
    }
    impl Seek for HeaderOnly {
        fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
            self.0.seek(position)
        }
    }
    #[test]
    fn rejects_large_extended_headers_without_reading_their_payload() {
        for kind in *b"xgLKS" {
            let mut header = tar::Header::new_ustar();
            header.set_path("extension").unwrap();
            header.set_entry_type(tar::EntryType::new(kind));
            header.set_size(1024 * 1024 * 1024);
            header.set_cksum();
            let mut input = HeaderOnly(Cursor::new(header.as_bytes().to_vec()));
            let error = scan(&mut input, 1024 * 1024 * 1024 + 512)
                .err()
                .expect("extended header must fail before any payload read");
            assert!(error.to_string().contains("扩展条目"), "{error:#}");
        }
    }
    #[test]
    fn rejects_truncated_and_duplicate_members_before_any_restore() {
        let mut header = tar::Header::new_ustar();
        header.set_path("meta.json").unwrap();
        header.set_size(4096);
        header.set_cksum();
        assert!(
            scan(&mut Cursor::new(header.as_bytes()), 512)
                .err()
                .unwrap()
                .to_string()
                .contains("超出文件范围")
        );
        let mut bytes = Vec::new();
        {
            let mut archive = tar::Builder::new(&mut bytes);
            for _ in 0..2 {
                let mut header = tar::Header::new_ustar();
                header.set_path("meta.json").unwrap();
                header.set_size(2);
                header.set_cksum();
                archive.append(&header, b"{}".as_slice()).unwrap();
            }
            archive.finish().unwrap();
        }
        let length = bytes.len() as u64;
        assert!(
            scan(&mut Cursor::new(bytes), length)
                .err()
                .unwrap()
                .to_string()
                .contains("重复条目")
        );
    }
}
