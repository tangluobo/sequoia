//! BIFF8 in a real OLE compound file. Worksheet and SST are disk streams.
use super::*;
use std::io::{Seek, SeekFrom};
use tomato_core::workbench::Column;
fn record(out: &mut impl Write, id: u16, data: &[u8]) -> Result<()> {
    ensure!(data.len() <= 8224, "BIFF8 记录超过格式限制");
    out.write_all(&id.to_le_bytes())?;
    out.write_all(&(data.len() as u16).to_le_bytes())?;
    out.write_all(data)?;
    Ok(())
}
fn bof(out: &mut impl Write, kind: u16) -> Result<()> {
    let mut b = vec![0x00, 0x06];
    b.extend(kind.to_le_bytes());
    b.extend([0xbb, 0x0d, 0xcc, 0x07, 0x41, 0, 0, 0, 0x06, 0, 0, 0]);
    record(out, 0x0809, &b)
}
struct Sst {
    file: tempfile::NamedTempFile,
    bytes: Vec<u8>,
    count: u32,
    first: bool,
}
impl Sst {
    fn flush(&mut self) -> Result<()> {
        record(
            self.file.as_file_mut(),
            if self.first { 0x00fc } else { 0x003c },
            &self.bytes,
        )?;
        self.bytes.clear();
        self.first = false;
        Ok(())
    }
    fn text(&mut self, text: &str) -> Result<u32> {
        let units = text.encode_utf16().collect::<Vec<_>>();
        ensure!(
            units.len() <= 32767,
            "XLS 单元格超过 32767 个字符，请选择文本或 JSON"
        );
        if 8224 - self.bytes.len() < 3 {
            self.flush()?;
        }
        self.bytes.extend((units.len() as u16).to_le_bytes());
        self.bytes.push(1);
        for c in units {
            if self.bytes.len() + 2 > 8224 {
                self.flush()?;
                self.bytes.push(1);
            }
            self.bytes.extend(c.to_le_bytes());
        }
        let index = self.count;
        self.count = self
            .count
            .checked_add(1)
            .context("XLS 字符串数量超过上限")?;
        Ok(index)
    }
    fn finish(&mut self) -> Result<()> {
        self.flush()?;
        self.file.seek(SeekFrom::Start(4))?;
        self.file.write_all(&self.count.to_le_bytes())?;
        self.file.write_all(&self.count.to_le_bytes())?;
        Ok(())
    }
}
pub(super) fn write(
    spool: &Spool,
    columns: &[Column],
    options: &Options,
    control: &Control,
    out: &mut File,
) -> Result<()> {
    ensure!(columns.len() <= 256, "XLS 最多 256 列，请选择 XLSX");
    ensure!(
        spool.records + u64::from(options.header) <= 65536,
        "XLS 每个工作表最多 65536 行，请选择 XLSX 或 CSV"
    );
    let parent = spool.file.path().parent().context("导出临时目录不存在")?;
    let mut sst = Sst {
        file: tempfile::NamedTempFile::new_in(parent)?,
        bytes: vec![0; 8],
        count: 0,
        first: true,
    };
    let mut sheet = tempfile::NamedTempFile::new_in(parent)?;
    bof(sheet.as_file_mut(), 0x0010)?;
    let mut dimensions = vec![0u8; 14];
    dimensions[4..8]
        .copy_from_slice(&((spool.records + u64::from(options.header)) as u32).to_le_bytes());
    dimensions[10..12].copy_from_slice(&(columns.len() as u16).to_le_bytes());
    record(sheet.as_file_mut(), 0x0200, &dimensions)?;
    let mut index = 0u16;
    let mut emit = |cells: &[Cell]| -> Result<()> {
        for (column, cell) in cells.iter().enumerate() {
            if matches!(cell, Cell::Null) {
                continue;
            }
            let mut data = index.to_le_bytes().to_vec();
            data.extend((column as u16).to_le_bytes());
            data.extend(0u16.to_le_bytes());
            match cell {
                Cell::Boolean(v) => {
                    data.extend([u8::from(*v), 0]);
                    record(sheet.as_file_mut(), 0x0205, &data)?;
                }
                Cell::Integer(n) if n.unsigned_abs() < 1_000_000_000_000_000 => {
                    data.extend((*n as f64).to_le_bytes());
                    record(sheet.as_file_mut(), 0x0203, &data)?;
                }
                Cell::Real(n) if n.is_finite() => {
                    data.extend(n.to_le_bytes());
                    record(sheet.as_file_mut(), 0x0203, &data)?;
                }
                _ => {
                    let number = sst.text(&value(cell))?;
                    data.extend(number.to_le_bytes());
                    record(sheet.as_file_mut(), 0x00fd, &data)?;
                }
            }
        }
        index = index.wrapping_add(1);
        Ok(())
    };
    if options.header {
        emit(
            &columns
                .iter()
                .map(|c| Cell::Text(c.name.clone()))
                .collect::<Vec<_>>(),
        )?;
    }
    spool.each(options, control, &mut emit)?;
    record(sheet.as_file_mut(), 0x000a, &[])?;
    sst.finish()?;
    let mut global = Vec::new();
    bof(&mut global, 0x0005)?;
    record(&mut global, 0x0042, &1200u16.to_le_bytes())?;
    record(&mut global, 0x0022, &0u16.to_le_bytes())?;
    let mut font = vec![0u8; 14];
    font[0..2].copy_from_slice(&200u16.to_le_bytes());
    font[4..6].copy_from_slice(&0x7fffu16.to_le_bytes());
    font[6..8].copy_from_slice(&400u16.to_le_bytes());
    font.extend([5, 0]);
    font.extend(b"Arial");
    record(&mut global, 0x0031, &font)?;
    record(&mut global, 0x00e0, &[0; 20])?;
    let mut bound = vec![0; 4];
    bound.extend([0, 0, 4, 1]);
    for c in "导出数据".encode_utf16() {
        bound.extend(c.to_le_bytes());
    }
    let offset =
        global.len() as u64 + 4 + bound.len() as u64 + sst.file.as_file().metadata()?.len() + 4;
    bound[..4].copy_from_slice(&u32::try_from(offset)?.to_le_bytes());
    record(&mut global, 0x0085, &bound)?;
    let mut compound = cfb::CompoundFile::create_with_version(cfb::Version::V3, out)?;
    let mut stream = compound.create_stream("/Workbook")?;
    stream.write_all(&global)?;
    sst.file.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut sst.file, &mut stream)?;
    record(&mut stream, 0x000a, &[])?;
    sheet.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut sheet, &mut stream)?;
    // OLE streams below 4096 bytes use the mini stream; cfb handles both layouts.
    stream.flush()?;
    drop(stream);
    compound.flush()?;
    Ok(())
}
