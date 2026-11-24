use super::signature::Source;
use anyhow::{Context, Result, ensure};
use std::io::{Read, Seek};
#[derive(Clone, Debug)]
pub struct Entry {
    pub kind: u32,
    pub kind_name: Option<String>,
    pub id: u32,
    pub name: String,
    pub lang: u32,
    pub offset: u64,
    pub size: u64,
}
struct Section {
    address: u64,
    size: u64,
    raw: u64,
}
fn string<R: Read + Seek>(source: &mut Source<R>, base: u64, id: u32) -> Result<String> {
    let at = base + u64::from(id & 0x7fff_ffff);
    let count = source.u16(at).context("PE 资源名称越界")?;
    ensure!(count <= 4096, "PE 资源名称过长");
    let bytes = source.slice(at + 2, usize::from(count) * 2)?;
    Ok(String::from_utf16_lossy(
        &bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<_>>(),
    ))
}
fn map(sections: &[Section], rva: u64) -> Option<u64> {
    sections
        .iter()
        .find(|s| rva >= s.address && rva - s.address < s.size)
        .map(|s| s.raw + rva - s.address)
}
pub fn parse<R: Read + Seek>(source: &mut Source<R>) -> Result<Vec<Entry>> {
    ensure!(
        source.len >= 64 && source.slice(0, 2)? == b"MZ",
        "不是 PE 文件"
    );
    let pe = u64::from(source.u32(0x3c).context("PE 头越界")?);
    ensure!(source.slice(pe, 4)? == b"PE\0\0", "无效 PE 签名");
    let count = source.u16(pe + 6).context("缺少 PE 节表")?;
    ensure!(count <= 4096, "PE 节表超过上限");
    let optional = pe + 24;
    let size = source.u16(pe + 20).context("缺少 PE 可选头")?;
    let magic = source.u16(optional).context("缺少 PE 类型")?;
    ensure!(matches!(magic, 0x10b | 0x20b), "不支持的 PE 可选头");
    let directory = optional + if magic == 0x20b { 112 } else { 96 };
    if u64::from(size) < directory + 24 - optional {
        return Ok(Vec::new());
    }
    let resource = u64::from(source.u32(directory + 16).context("PE 资源目录越界")?);
    let length = source.u32(directory + 20).unwrap_or(0);
    let mut sections = Vec::new();
    for index in 0..u64::from(count) {
        let at = optional + u64::from(size) + 40 * index;
        let bytes = source.slice(at, 40)?;
        let n = |i| {
            u64::from(u32::from_le_bytes(
                bytes[i..i + 4].try_into().expect("checked section"),
            ))
        };
        sections.push(Section {
            address: n(12),
            size: n(8).max(n(16)),
            raw: n(20),
        });
    }
    if resource == 0 || length == 0 {
        return Ok(Vec::new());
    }
    let Some(base) = map(&sections, resource) else {
        return Ok(Vec::new());
    };
    let mut pending = vec![(base, 0usize, 0u32, None, 0u32, String::new(), 0u32)];
    let mut entries = Vec::new();
    let mut visited = 0;
    while let Some((table, level, kind, kind_name, id, name, lang)) = pending.pop() {
        let header = source.slice(table, 16)?;
        let count = usize::from(u16::from_le_bytes([header[12], header[13]]))
            + usize::from(u16::from_le_bytes([header[14], header[15]]));
        visited += count;
        ensure!(visited <= 100000, "PE 资源目录超过 100000 项");
        let bytes = source.slice(table + 16, count * 8)?;
        for item in bytes.as_chunks::<8>().0.iter().rev() {
            let raw = u32::from_le_bytes(item[..4].try_into()?);
            let child = u32::from_le_bytes(item[4..].try_into()?);
            let number = raw & 0x7fff_ffff;
            let named = raw & 0x8000_0000 != 0;
            let text = if named {
                Some(string(source, base, raw)?)
            } else {
                None
            };
            let kind = if level == 0 { number } else { kind };
            let kind_name = if level == 0 {
                text.clone()
            } else {
                kind_name.clone()
            };
            let id = if level == 1 { number } else { id };
            let name = if level == 1 {
                text.unwrap_or_else(|| number.to_string())
            } else {
                name.clone()
            };
            let lang = if level == 2 { number } else { lang };
            if child & 0x8000_0000 != 0 {
                if level < 2 {
                    pending.push((
                        base + u64::from(child & 0x7fff_ffff),
                        level + 1,
                        kind,
                        kind_name,
                        id,
                        name,
                        lang,
                    ));
                }
            } else {
                let data = source.slice(base + u64::from(child), 16)?;
                let rva = u64::from(u32::from_le_bytes(data[..4].try_into()?));
                let size = u64::from(u32::from_le_bytes(data[4..8].try_into()?));
                if let Some(offset) = map(&sections, rva)
                    && offset > 0
                    && size > 0
                    && offset <= source.len
                    && size <= source.len - offset
                {
                    entries.push(Entry {
                        kind,
                        kind_name,
                        id,
                        name,
                        lang,
                        offset,
                        size,
                    });
                    ensure!(entries.len() <= 10000, "PE 资源超过 10000 项");
                }
            }
        }
    }
    entries.sort_by_key(|e| e.offset);
    Ok(entries)
}
fn raw<R: Read + Seek>(source: &mut Source<R>, entry: &Entry) -> Result<Vec<u8>> {
    ensure!(entry.size <= 64 * 1024 * 1024, "单个 PE 资源超过 64 MiB");
    source.slice(entry.offset, entry.size as usize)
}
fn safe(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "\\/:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim();
    if name.is_empty() {
        "unnamed".into()
    } else {
        name.into()
    }
}
fn put16(data: &mut [u8], at: usize, n: u16) {
    data[at..at + 2].copy_from_slice(&n.to_le_bytes());
}
fn put32(data: &mut [u8], at: usize, n: usize) {
    data[at..at + 4].copy_from_slice(&(n as u32).to_le_bytes());
}
fn u16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}
fn u32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}
fn png(data: &[u8]) -> bool {
    data.starts_with(b"\x89PNG\r\n\x1a\n")
}
fn choose(entries: &[Entry], kind: u32, id: u32, lang: u32) -> Option<&Entry> {
    entries
        .iter()
        .filter(|e| e.kind == kind && e.id == id)
        .min_by_key(|e| {
            if e.lang == lang {
                0
            } else if e.lang == 0 {
                1
            } else {
                2
            }
        })
}
fn group<R: Read + Seek>(
    source: &mut Source<R>,
    entry: &Entry,
    entries: &[Entry],
    cursor: bool,
) -> Result<Option<Vec<u8>>> {
    let data = raw(source, entry)?;
    let count = u16(&data, 4).unwrap_or(0);
    if count == 0 || count > 1024 {
        return Ok(None);
    }
    let mut frames = Vec::new();
    let mut total = 0;
    for index in 0..usize::from(count) {
        let at = 6 + index * 14;
        let Some(header) = data.get(at..at + 14) else {
            return Ok(None);
        };
        let id = u32::from(u16(header, 12).unwrap_or(0));
        let Some(item) = choose(entries, if cursor { 1 } else { 3 }, id, entry.lang) else {
            continue;
        };
        let mut image = raw(source, item)?;
        let hotspot = if cursor {
            if image.len() <= 4 {
                continue;
            }
            let a = [image[0], image[1], image[2], image[3]];
            image.drain(..4);
            a
        } else {
            [0; 4]
        };
        if image.is_empty() {
            continue;
        }
        total += image.len();
        ensure!(total <= 64 * 1024 * 1024, "重建图标超过 64 MiB");
        frames.push((header, hotspot, image));
    }
    if frames.is_empty() {
        return Ok(None);
    }
    let mut result = vec![0; 6 + frames.len() * 16 + total];
    put16(&mut result, 2, if cursor { 2 } else { 1 });
    put16(&mut result, 4, frames.len() as u16);
    let mut offset = 6 + frames.len() * 16;
    for (index, (header, hotspot, image)) in frames.iter().enumerate() {
        let at = 6 + index * 16;
        if cursor {
            let width = u16(header, 0).unwrap_or(0);
            let height = u16(header, 2).unwrap_or(0);
            result[at] = if width >= 256 { 0 } else { width as u8 };
            result[at + 1] = if height >= 256 { 0 } else { height as u8 };
            result[at + 4..at + 8].copy_from_slice(hotspot);
        } else {
            result[at..at + 8].copy_from_slice(&header[..8]);
        }
        put32(&mut result, at + 8, image.len());
        put32(&mut result, at + 12, offset);
        result[offset..offset + image.len()].copy_from_slice(image);
        offset += image.len();
    }
    Ok(Some(result))
}
pub fn extract<R: Read + Seek>(
    source: &mut Source<R>,
    entries: &[Entry],
    mut emit: impl FnMut(String, &str, Vec<u8>) -> Result<()>,
) -> Result<()> {
    for entry in entries
        .iter()
        .filter(|e| !matches!(e.kind, 1 | 3 | 12 | 14))
    {
        let mut data = raw(source, entry)?;
        let (name, extension) = match entry.kind {
            2 => {
                let mut bitmap = vec![0; 14 + data.len()];
                bitmap[..2].copy_from_slice(b"BM");
                let length = bitmap.len();
                put32(&mut bitmap, 2, length);
                put32(&mut bitmap, 10, 14 + u32(&data, 0).unwrap_or(40) as usize);
                bitmap[14..].copy_from_slice(&data);
                data = bitmap;
                (format!("bitmap_{}", entry.name), "bmp")
            }
            24 => ("manifest".into(), "xml"),
            16 => ("version".into(), "bin"),
            23 => (format!("html_{}", entry.id), "html"),
            6 => (format!("strings_{}", entry.id), "txt"),
            _ => (format!("rt{}_{}", entry.kind, entry.name), "bin"),
        };
        emit(safe(&name), extension, data)?;
    }
    for entry in entries.iter().filter(|e| matches!(e.kind, 12 | 14)) {
        let cursor = entry.kind == 12;
        if let Some(data) = group(source, entry, entries, cursor)? {
            let name = if entry.name == entry.id.to_string() {
                format!("{}_{}", if cursor { "cursor" } else { "icon" }, entry.id)
            } else {
                entry.name.clone()
            };
            emit(safe(&name), if cursor { "cur" } else { "ico" }, data)?;
        }
    }
    let grouped = entries.iter().any(|e| e.kind == 14);
    for entry in entries.iter().filter(|e| e.kind == 3) {
        let data = raw(source, entry)?;
        let name = format!("icon_{}_lang{}", safe(&entry.name), entry.lang);
        if png(&data) {
            emit(name.clone(), "png", data.clone())?;
        }
        if !grouped && data.len() >= 16 {
            let (width, height, planes, bits) = if png(&data) && data.len() >= 24 {
                (
                    u32::from_be_bytes(data[16..20].try_into()?),
                    u32::from_be_bytes(data[20..24].try_into()?),
                    1,
                    32,
                )
            } else {
                (
                    u32(&data, 4).unwrap_or(0),
                    u32(&data, 8).unwrap_or(0) / 2,
                    u16(&data, 12).unwrap_or(1),
                    u16(&data, 14).unwrap_or(32),
                )
            };
            if width > 0 && height > 0 {
                let mut icon = vec![0; 22 + data.len()];
                put16(&mut icon, 2, 1);
                put16(&mut icon, 4, 1);
                icon[6] = if width >= 256 { 0 } else { width as u8 };
                icon[7] = if height >= 256 { 0 } else { height as u8 };
                put16(&mut icon, 10, planes);
                put16(&mut icon, 12, bits);
                put32(&mut icon, 14, data.len());
                put32(&mut icon, 18, 22);
                icon[22..].copy_from_slice(&data);
                emit(name, "ico", icon)?;
            }
        }
    }
    Ok(())
}
