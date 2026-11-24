use anyhow::Result;
use serde::Deserialize;
use std::{
    io::{Read, Seek, SeekFrom},
    sync::OnceLock,
};
#[derive(Debug, Deserialize)]
pub struct Format {
    pub extension: String,
    pub description: String,
    pub category: String,
    magic: String,
    minimum: u64,
    sizer: String,
    #[serde(skip)]
    bytes: Vec<Option<u8>>,
}
pub fn formats() -> &'static [Format] {
    static FORMATS: OnceLock<Vec<Format>> = OnceLock::new();
    FORMATS.get_or_init(|| {
        let mut formats: Vec<Format> = serde_json::from_str(include_str!("signatures.json"))
            .expect("checked legacy signature registry");
        for format in &mut formats {
            format.bytes = format
                .magic
                .split_whitespace()
                .map(|b| {
                    if b.contains('?') {
                        None
                    } else {
                        Some(u8::from_str_radix(b, 16).expect("checked magic byte"))
                    }
                })
                .collect();
        }
        formats
    })
}
pub struct Source<R> {
    pub reader: R,
    pub len: u64,
}
impl<R: Read + Seek> Source<R> {
    pub fn slice(&mut self, offset: u64, size: usize) -> Result<Vec<u8>> {
        anyhow::ensure!(
            offset <= self.len && size as u64 <= self.len - offset,
            "资源范围越界"
        );
        let mut data = vec![0; size];
        self.reader.seek(SeekFrom::Start(offset))?;
        self.reader.read_exact(&mut data)?;
        Ok(data)
    }
    pub fn u16(&mut self, offset: u64) -> Option<u16> {
        Some(u16::from_le_bytes(
            self.slice(offset, 2).ok()?.try_into().ok()?,
        ))
    }
    pub fn u32(&mut self, offset: u64) -> Option<u32> {
        Some(u32::from_le_bytes(
            self.slice(offset, 4).ok()?.try_into().ok()?,
        ))
    }
    pub fn find(&mut self, needle: &[u8], start: u64, end: u64) -> Option<u64> {
        let end = end.min(self.len);
        let mut offset = start;
        let mut buffer = vec![0; 32768];
        while offset < end {
            let size = ((end - offset) as usize).min(buffer.len());
            self.reader.seek(SeekFrom::Start(offset)).ok()?;
            self.reader.read_exact(&mut buffer[..size]).ok()?;
            if let Some(index) = buffer[..size]
                .windows(needle.len())
                .position(|w| w == needle)
            {
                return Some(offset + index as u64);
            }
            if size < buffer.len() {
                break;
            }
            offset += (size - needle.len() + 1) as u64;
        }
        None
    }
}
impl Format {
    pub fn useful(&self, min: u64) -> bool {
        self.sizer != "none" || self.bytes.len() >= 4 && self.bytes.len() as u64 >= min
    }
    pub fn matches(&self, data: &[u8]) -> bool {
        data.len() >= self.bytes.len()
            && self
                .bytes
                .iter()
                .zip(data)
                .all(|(a, b)| a.is_none_or(|a| a == *b))
    }
    pub fn size<R: Read + Seek>(&self, source: &mut Source<R>, offset: u64) -> Option<u64> {
        let remaining = source.len.checked_sub(offset)?;
        let size = match self.sizer.as_str() {
            "bmp" => source.u32(offset + 2).map(u64::from),
            "riff" => source.u32(offset + 4).map(|v| u64::from(v) + 8),
            "jpeg" => source
                .find(&[0xff, 0xd9], offset + 2, source.len)
                .map(|p| p + 2 - offset),
            "png" => source
                .find(b"IEND\xae\x42\x60\x82", offset + 8, source.len)
                .map(|p| p + 8 - offset),
            "gif" => source
                .find(&[0x3b], offset + 6, source.len)
                .map(|p| p + 1 - offset),
            "mp3id3" => source.slice(offset, 10).ok().map(|b| {
                10 + ((u64::from(b[6] & 127) << 21)
                    | (u64::from(b[7] & 127) << 14)
                    | (u64::from(b[8] & 127) << 7)
                    | u64::from(b[9] & 127))
            }),
            "icon" => {
                let count = source.u16(offset + 4)?;
                if count == 0 || count > 1024 {
                    None
                } else {
                    let entries = source.slice(offset + 6, usize::from(count) * 16).ok()?;
                    let mut end = 0;
                    for entry in entries.as_chunks::<16>().0 {
                        let size = u32::from_le_bytes(entry[8..12].try_into().ok()?);
                        let position = u32::from_le_bytes(entry[12..16].try_into().ok()?);
                        end = end.max(u64::from(size) + u64::from(position));
                    }
                    Some(if end == 0 || end > remaining {
                        6 + u64::from(count) * 16
                    } else {
                        end
                    })
                }
            }
            "pdf" => {
                let mut next = source.find(b"%%EOF", offset + 5, source.len)? + 5;
                while let Some(p) = source.find(
                    b"%%EOF",
                    next.saturating_sub(4),
                    (next + 1028).min(source.len),
                ) {
                    if p + 5 <= next {
                        break;
                    }
                    next = p + 5;
                }
                Some(next - offset)
            }
            "swf" => source
                .u32(offset + 4)
                .map(|n| u64::from(n & 0x3fffffff))
                .filter(|n| *n >= 8 && *n <= remaining),
            _ => None,
        }
        .filter(|n| *n >= self.minimum);
        size.or_else(|| (self.bytes.len() >= 4).then_some(self.bytes.len() as u64))
            .map(|n| n.min(remaining))
    }
}
/// Scan with a reusable 64 KiB window. No whole-file buffer or resource payload is retained.
pub fn scan<R: Read + Seek>(
    source: &mut Source<R>,
    enabled: &[&Format],
    min: u64,
    mut emit: impl FnMut(u64, u64, &Format, &mut Source<R>) -> Result<()>,
) -> Result<()> {
    // Dispatch by the first concrete magic byte. Unknown-length signatures that
    // cannot reach the requested minimum never need testing at every file byte.
    let mut by_first: [Vec<&Format>; 256] = std::array::from_fn(|_| Vec::new());
    let mut wildcard = Vec::new();
    let mut useful = 0;
    for format in enabled.iter().copied().filter(|f| f.useful(min)) {
        useful += 1;
        if let Some(Some(byte)) = format.bytes.first() {
            by_first[usize::from(*byte)].push(format);
        } else {
            wildcard.push(format);
        }
    }
    if useful == 0 {
        return Ok(());
    }
    let mut buffer = vec![0; 65536];
    let mut offset = 0;
    let mut base = u64::MAX;
    let mut count = 0;
    while offset + 2 <= source.len {
        if base == u64::MAX || offset < base || offset + 32 > base + count as u64 {
            base = offset;
            count = ((source.len - offset) as usize).min(buffer.len());
            source.reader.seek(SeekFrom::Start(base))?;
            source.reader.read_exact(&mut buffer[..count])?;
        }
        let index = (offset - base) as usize;
        let mut hit = None;
        for format in by_first[usize::from(buffer[index])].iter().chain(&wildcard) {
            if format.matches(&buffer[index..count])
                && let Some(size) = format
                    .size(source, offset)
                    .filter(|size| *size >= min && *size > 0)
            {
                hit = Some((size, *format));
                break;
            }
        }
        if let Some((size, format)) = hit {
            emit(offset, size, format, source)?;
            offset += size;
        } else {
            offset += 1;
        }
    }
    Ok(())
}
