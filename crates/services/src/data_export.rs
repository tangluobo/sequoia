//! Bounded, cancellable export jobs. Sources and format writers never own the GUI.
mod access;
mod dbf;
#[cfg(test)]
mod tests;
mod text;
mod xls;
mod xlsx;
pub use crate::backup::Control;
use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};
use tomato_core::{
    Connection,
    data_export::{Format, Options, Schema, Snapshot},
    workbench::{Cell, TableId},
};

pub enum Source {
    Current(Snapshot),
    Query {
        connection: Connection,
        database: String,
        sql: String,
        schema: Schema,
    },
    Table {
        connection: Connection,
        database: String,
        table: TableId,
        sort: Option<tomato_core::workbench::Sort>,
        query: tomato_core::workbench::TableQuery,
    },
}
pub struct Request {
    pub source: Source,
    pub path: PathBuf,
    pub options: Options,
}
pub struct Report {
    pub path: PathBuf,
    pub records: u64,
}

pub struct Spool {
    file: tempfile::NamedTempFile,
    writer: BufWriter<File>,
    pub schema: Schema,
    pub records: u64,
}
impl Spool {
    pub(crate) fn new(schema: Schema, directory: &Path) -> Result<Self> {
        ensure!(!schema.columns.is_empty(), "没有可导出的字段");
        let file = tempfile::Builder::new()
            .prefix(".tomato-export-rows-")
            .tempfile_in(directory)?;
        let writer = BufWriter::with_capacity(64 * 1024, file.reopen()?);
        Ok(Self {
            file,
            writer,
            schema,
            records: 0,
        })
    }
    pub(crate) fn push(&mut self, row: &[Cell], control: &Control) -> Result<()> {
        control.check()?;
        ensure!(
            row.len() == self.schema.columns.len(),
            "导出记录的字段数与结构不一致"
        );
        let bytes = serde_json::to_vec(row)?;
        ensure!(bytes.len() <= 32 * 1024 * 1024, "单行导出数据超过 32 MiB");
        self.writer.write_all(&bytes)?;
        self.writer.write_all(b"\n")?;
        self.records += 1;
        if self.records.is_multiple_of(256) {
            control.records(256);
        }
        Ok(())
    }
    fn each(
        &self,
        options: &Options,
        control: &Control,
        mut consume: impl FnMut(&[Cell]) -> Result<()>,
    ) -> Result<()> {
        let input = BufReader::new(File::open(self.file.path())?);
        for line in input.lines() {
            control.check()?;
            let row: Vec<Cell> = serde_json::from_str(&line?)?;
            let selected = options
                .fields
                .iter()
                .map(|&index| row.get(index).cloned().context("导出字段已不存在"))
                .collect::<Result<Vec<_>>>()?;
            consume(&selected)?;
        }
        Ok(())
    }
}
fn revision(path: &Path) -> Result<Option<(u64, std::time::SystemTime)>> {
    match path.metadata() {
        Ok(m) => Ok(Some((m.len(), m.modified()?))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub fn run(request: Request, control: Control, runtime: tokio::runtime::Handle) -> Result<Report> {
    control.check()?;
    let parent = request
        .path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let before = revision(&request.path)?;
    control.log("正在读取导出记录…");
    let mut spool = match request.source {
        Source::Current(snapshot) => {
            let mut spool = Spool::new(snapshot.schema, parent)?;
            for row in snapshot.rows {
                spool.push(&row, &control)?;
            }
            spool
        }
        Source::Table {
            connection,
            database,
            table,
            sort,
            query,
        } => runtime.block_on(crate::database::export::table(
            &connection,
            &database,
            &table,
            sort.as_ref(),
            &query,
            parent,
            &control,
        ))?,
        Source::Query {
            connection,
            database,
            sql,
            schema,
        } => runtime.block_on(crate::database::export::query(
            &connection,
            &database,
            &sql,
            schema,
            parent,
            &control,
        ))?,
    };
    spool.writer.flush()?;
    let mut options = request.options;
    ensure!(
        options.expected_columns.is_empty()
            || options.expected_columns
                == spool
                    .schema
                    .columns
                    .iter()
                    .map(|c| c.name.clone())
                    .collect::<Vec<_>>(),
        "表结构在打开向导后发生变化，请刷新表并重新导出"
    );
    if options.fields.is_empty() {
        options.fields = (0..spool.schema.columns.len()).collect();
    }
    ensure!(options.fields.len() <= 16_384, "导出字段超过 16384 列");
    ensure!(
        options
            .fields
            .iter()
            .all(|&i| i < spool.schema.columns.len()),
        "导出字段已不存在"
    );
    let mut columns = options
        .fields
        .iter()
        .map(|&i| spool.schema.columns[i].clone())
        .collect::<Vec<_>>();
    if !options.field_names.is_empty() {
        ensure!(
            options.field_names.len() == columns.len(),
            "导出字段别名数量不一致"
        );
        for (column, name) in columns.iter_mut().zip(&options.field_names) {
            ensure!(!name.trim().is_empty(), "导出字段名不能为空");
            column.name = name.clone();
        }
    }
    let names = columns
        .iter()
        .map(|c| c.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    ensure!(
        names.len() == columns.len(),
        "导出字段名重复，请先为查询字段设置不同的别名"
    );
    let mut output = tempfile::Builder::new()
        .prefix(".tomato-export-output-")
        .tempfile_in(parent)?;
    control.log(&format!(
        "正在写入 {}，共 {} 条记录…",
        options.format.label(),
        spool.records
    ));
    match options.format {
        Format::Xlsx => xlsx::write(&spool, &columns, &options, &control, output.as_file_mut())?,
        Format::Xls => xls::write(&spool, &columns, &options, &control, output.as_file_mut())?,
        Format::Dbase => dbf::write(&spool, &columns, &options, &control, output.as_file_mut())?,
        Format::Access => access::write(&spool, &columns, &options, &control, output.path())?,
        _ => text::write(
            &spool,
            &columns,
            &options,
            &control,
            &mut BufWriter::new(output.as_file_mut()),
        )?,
    }
    control.check()?;
    output.as_file().sync_all()?;
    ensure!(
        revision(&request.path)? == before,
        "目标文件在导出期间已被其他程序修改，未覆盖原文件"
    );
    output.persist(&request.path).map_err(|e| e.error)?;
    control.records(spool.records % 256);
    control.log(&format!("导出完成：{} 条记录", spool.records));
    Ok(Report {
        path: request.path,
        records: spool.records,
    })
}
fn value(cell: &Cell) -> String {
    match cell {
        Cell::Null => String::new(),
        Cell::Binary(bytes) => bytes.iter().map(|b| format!("{b:02X}")).collect(),
        _ => cell.display(),
    }
}
