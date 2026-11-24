//! Import source adapters and owned disk snapshots. Database writes live in database::import.
mod access;
mod delimited;
mod documents;
mod odbc;
#[cfg(windows)]
mod odbc32;
mod paradox;
mod tabular;
#[cfg(test)]
mod tests;
mod worker;
pub use crate::backup::Control;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};
use tomato_core::{data_import::*, workbench::Cell};
pub use worker::{WorkerRequest, WorkerResponse, worker_entry};

pub struct Prepared {
    pub tables: Vec<SourceTable>,
    pub directory: tempfile::TempDir,
    pub source: Source,
}
impl Prepared {
    pub fn each(
        &self,
        index: usize,
        control: &Control,
        mut consume: impl FnMut(u64, Vec<Cell>) -> Result<()>,
    ) -> Result<()> {
        let table = self.tables.get(index).context("源表已不存在，请重新分析")?;
        let path = self.directory.path().join(&table.spool);
        ensure!(
            Path::new(&table.spool).components().count() == 1,
            "源缓存路径无效"
        );
        let mut reader = BufReader::with_capacity(64 * 1024, File::open(path)?);
        let mut line = Vec::new();
        let mut row = 0;
        loop {
            control.check()?;
            line.clear();
            let n = reader
                .by_ref()
                .take((MAX_ROW_BYTES + 2) as u64)
                .read_until(b'\n', &mut line)?;
            if n == 0 {
                break;
            }
            ensure!(
                n <= MAX_ROW_BYTES + 1 && line.last() == Some(&b'\n'),
                "源缓存行过大或不完整"
            );
            let mut values: Vec<Cell> = serde_json::from_slice(&line)?;
            ensure!(values.len() <= table.fields.len(), "源缓存字段数无效");
            values.resize(table.fields.len(), Cell::Null);
            row += 1;
            consume(row, values)?;
        }
        ensure!(row == table.rows, "源缓存记录数不一致");
        Ok(())
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub tables: Vec<SourceTable>,
    pub odbc_tables: Vec<OdbcTable>,
    pub dsns: Vec<String>,
    pub drivers: Vec<String>,
}
pub fn prepare(source: Source, control: &Control) -> Result<Prepared> {
    source.validate()?;
    let directory = tempfile::Builder::new()
        .prefix("tomato-import-source-")
        .tempdir()?;
    let response = worker::run(
        WorkerRequest::Read(Box::new(source.clone())),
        directory.path(),
        control,
    )?;
    let WorkerResponse::Catalog(catalog) = response;
    ensure!(!catalog.tables.is_empty(), "源文件中没有可导入的数据表");
    Ok(Prepared {
        tables: catalog.tables,
        directory,
        source: source.without_secrets(),
    })
}
pub fn discover_odbc(source: Odbc, control: &Control) -> Result<Catalog> {
    let directory = tempfile::Builder::new()
        .prefix("tomato-import-catalog-")
        .tempdir()?;
    let WorkerResponse::Catalog(catalog) = worker::run(
        WorkerRequest::OdbcCatalog(source),
        directory.path(),
        control,
    )?;
    Ok(catalog)
}
pub fn odbc_sources(use_32bit: bool, control: &Control) -> Result<Catalog> {
    let directory = tempfile::Builder::new()
        .prefix("tomato-import-drivers-")
        .tempdir()?;
    let WorkerResponse::Catalog(catalog) = worker::run(
        WorkerRequest::OdbcSources(use_32bit),
        directory.path(),
        control,
    )?;
    Ok(catalog)
}
pub(crate) fn materialize(source: &Source, directory: &Path, control: &Control) -> Result<Catalog> {
    source.validate()?;
    let mut output = Catalog::default();
    if source.format == Format::Odbc {
        odbc::read(source, directory, &mut output, control)?;
        return Ok(output);
    }
    for path in &source.files {
        control.check()?;
        let before = revision(path)?;
        ensure!(before.0 > 0, "源文件为空：{}", path.display());
        match source.format {
            Format::Text | Format::Csv => {
                delimited::read(source, path, directory, &mut output, control)?
            }
            Format::Excel => tabular::excel(source, path, directory, &mut output, control)?,
            Format::Dbase => tabular::dbase(source, path, directory, &mut output, control)?,
            Format::Json => documents::json(source, path, directory, &mut output, control)?,
            Format::Xml => documents::xml(source, path, directory, &mut output, control)?,
            Format::Access => access::read(source, path, directory, &mut output, control)?,
            Format::Paradox => paradox::read(source, path, directory, &mut output, control)?,
            Format::Odbc => unreachable!(),
        }
        ensure!(
            revision(path)? == before,
            "读取期间源文件发生变化，请重新分析：{}",
            path.display()
        );
    }
    ensure!(
        output.tables.len() <= 100,
        "源数据表超过 100 张，请分批导入"
    );
    Ok(output)
}
fn revision(path: &Path) -> Result<(u64, std::time::SystemTime)> {
    let m = path.metadata()?;
    ensure!(m.is_file(), "请选择普通文件");
    Ok((m.len(), m.modified()?))
}
fn name(path: &Path) -> String {
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
// The parser may buffer one chunk ahead. Reset at record boundaries, before an
// untrusted CSV field, JSON value or XML token can allocate an unbounded string.
struct Budget<R> {
    input: R,
    bytes: std::rc::Rc<std::cell::Cell<usize>>,
}
impl<R: Read> Budget<R> {
    fn new(input: R) -> (Self, std::rc::Rc<std::cell::Cell<usize>>) {
        let bytes = std::rc::Rc::new(std::cell::Cell::new(0));
        (
            Self {
                input,
                bytes: bytes.clone(),
            },
            bytes,
        )
    }
}
impl<R: Read> Read for Budget<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let left = (MAX_ROW_BYTES + 64 * 1024).saturating_sub(self.bytes.get());
        if left == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "源记录超过 16 MiB",
            ));
        }
        let size = out.len().min(left);
        let n = self.input.read(&mut out[..size])?;
        self.bytes.set(self.bytes.get() + n);
        Ok(n)
    }
}
fn cell_bytes(cell: &Cell) -> usize {
    match cell {
        Cell::Text(s) => s.len(),
        Cell::Binary(b) => b.len(),
        _ => 16,
    }
}
struct Sink {
    writer: BufWriter<File>,
    table: SourceTable,
    inferred: Vec<Option<DataType>>,
    preview_bytes: usize,
    types_known: bool,
}
impl Sink {
    fn new(
        directory: &Path,
        output: &Catalog,
        name: String,
        origin: String,
        fields: Vec<Field>,
    ) -> Result<Self> {
        ensure!(output.tables.len() < 100, "源数据表超过 100 张");
        ensure!(fields.len() <= MAX_COLUMNS, "源字段超过 {MAX_COLUMNS} 列");
        ensure!(
            fields
                .iter()
                .all(|f| !f.name.is_empty() && f.name.len() <= 1024 && !f.name.contains('\0')),
            "源字段名为空、过长或包含无效字符"
        );
        ensure!(
            fields
                .iter()
                .map(|f| &f.name)
                .collect::<std::collections::HashSet<_>>()
                .len()
                == fields.len(),
            "源字段名重复，请调整字段名行或源查询别名"
        );
        let spool = format!("source-{:04}.rows", output.tables.len());
        let file = File::create_new(directory.join(&spool))?;
        let used = output
            .tables
            .iter()
            .flat_map(|t| t.preview.iter())
            .map(|r| serde_json::to_vec(r).map_or(0, |b| b.len()))
            .sum::<usize>();
        Ok(Self {
            writer: BufWriter::with_capacity(64 * 1024, file),
            inferred: vec![None; fields.len()],
            table: SourceTable {
                name,
                origin,
                fields,
                rows: 0,
                preview: Vec::new(),
                spool,
            },
            preview_bytes: used,
            types_known: false,
        })
    }
    fn typed(mut self) -> Self {
        self.types_known = true;
        self
    }
    fn push(&mut self, values: Vec<Cell>, control: &Control) -> Result<()> {
        control.check()?;
        ensure!(
            values.len() <= self.table.fields.len(),
            "源记录的字段数超过表结构"
        );
        let bytes = serde_json::to_vec(&values)?;
        ensure!(bytes.len() <= MAX_ROW_BYTES, "源记录超过 16 MiB");
        self.writer.write_all(&bytes)?;
        self.writer.write_all(b"\n")?;
        for (index, value) in values.iter().enumerate() {
            if matches!(value, Cell::Null) {
                self.table.fields[index].nullable = true;
                continue;
            }
            if !self.types_known {
                let typ = infer(value);
                self.inferred[index] = Some(match self.inferred[index].take() {
                    None => typ,
                    Some(old) => merge(old, typ),
                });
            }
        }
        if self.table.preview.len() < PREVIEW_ROWS
            && self.preview_bytes + bytes.len() <= 8 * 1024 * 1024
        {
            self.preview_bytes += bytes.len();
            self.table.preview.push(values)
        }
        self.table.rows += 1;
        Ok(())
    }
    fn field(&mut self, name: &str) -> Result<usize> {
        if let Some(i) = self.table.fields.iter().position(|f| f.name == name) {
            return Ok(i);
        }
        ensure!(
            self.table.fields.len() < MAX_COLUMNS,
            "源字段超过 {MAX_COLUMNS} 列"
        );
        ensure!(
            !name.is_empty() && name.len() <= 1024 && !name.contains('\0'),
            "源字段名无效"
        );
        self.table.fields.push(Field {
            name: name.into(),
            data_type: DataType::Text,
            nullable: self.table.rows > 0,
        });
        self.inferred.push(None);
        Ok(self.table.fields.len() - 1)
    }
    fn finish(mut self, output: &mut Catalog) -> Result<()> {
        self.writer.flush()?;
        for (f, t) in self.table.fields.iter_mut().zip(self.inferred) {
            if let Some(t) = t {
                f.data_type = t
            }
        }
        for row in &mut self.table.preview {
            row.resize(self.table.fields.len(), Cell::Null)
        }
        ensure!(!self.table.fields.is_empty(), "源数据没有字段");
        output.tables.push(self.table);
        Ok(())
    }
}
fn merge(a: DataType, b: DataType) -> DataType {
    if a == b {
        return a;
    }
    match (a, b) {
        (DataType::Integer, DataType::Decimal { precision, scale })
        | (DataType::Decimal { precision, scale }, DataType::Integer) => {
            let p = (precision - scale).max(19) + scale;
            if p <= 38 {
                DataType::Decimal {
                    precision: p,
                    scale,
                }
            } else {
                DataType::Text
            }
        }
        (
            DataType::Decimal {
                precision: p,
                scale: s,
            },
            DataType::Decimal {
                precision: q,
                scale: t,
            },
        ) => {
            let scale = s.max(t);
            let precision = (p - s).max(q - t) + scale;
            if precision <= 38 {
                DataType::Decimal { precision, scale }
            } else {
                DataType::Text
            }
        }
        (DataType::Integer, DataType::Real) | (DataType::Real, DataType::Integer) => DataType::Real,
        (DataType::Date, DataType::DateTime) | (DataType::DateTime, DataType::Date) => {
            DataType::DateTime
        }
        _ => DataType::Text,
    }
}
fn infer(value: &Cell) -> DataType {
    match value {
        Cell::Null => DataType::Text,
        Cell::Integer(_) => DataType::Integer,
        Cell::Real(_) => DataType::Real,
        Cell::Boolean(_) => DataType::Boolean,
        Cell::Binary(_) => DataType::Binary,
        Cell::Text(s) => {
            let n = s.trim_start_matches(['+', '-']);
            if s.trim() != s
                || n.is_empty()
                || (n.starts_with('0') && n.len() > 1 && !n.starts_with("0."))
            {
                return DataType::Text;
            }
            if s.parse::<i64>().is_ok() {
                return DataType::Integer;
            }
            if n.bytes().all(|c| c.is_ascii_digit() || c == b'.') && n.matches('.').count() <= 1 {
                let (whole, fraction) = n.split_once('.').unwrap_or((n, ""));
                let precision = (whole.trim_start_matches('0').len() + fraction.len()).max(1);
                if precision <= 38 {
                    return DataType::Decimal {
                        precision: precision as u16,
                        scale: fraction.len() as u16,
                    };
                }
            }
            if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
                return DataType::Date;
            }
            if chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f").is_ok()
                || chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").is_ok()
            {
                return DataType::DateTime;
            }
            DataType::Text
        }
    }
}
fn field(name: String) -> Field {
    Field {
        name,
        data_type: DataType::Text,
        nullable: false,
    }
}
fn included(source: &Source, row: u64) -> bool {
    row >= source.first_row && source.last_row.is_none_or(|last| row <= last)
}
fn text_cell(source: &Source, text: String, quoted: bool) -> Cell {
    let value = if source.trim {
        text.trim().into()
    } else {
        text
    };
    if !quoted && source.null_text.as_ref() == Some(&value) {
        Cell::Null
    } else {
        Cell::Text(value)
    }
}
