use super::*;
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    time::Duration,
};
static BINARY: &[u8] = include_bytes!(env!("TOMATO_ODBC32_READER"));
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn text(out: &mut impl Write, value: &str) -> Result<()> {
    out.write_all(&u32::try_from(value.len())?.to_be_bytes())?;
    out.write_all(value.as_bytes())?;
    Ok(())
}
fn number(input: &mut impl Read) -> Result<u32> {
    let mut b = [0; 4];
    input.read_exact(&mut b)?;
    Ok(u32::from_be_bytes(b))
}
fn byte(input: &mut impl Read) -> Result<u8> {
    let mut b = [0];
    input.read_exact(&mut b)?;
    Ok(b[0])
}
fn string(input: &mut impl Read) -> Result<String> {
    let n = number(input)? as usize;
    ensure!(n <= MAX_ROW_BYTES, "ODBC 字段过大");
    let mut b = vec![0; n];
    input.read_exact(&mut b)?;
    Ok(String::from_utf8(b)?)
}
struct Output {
    reader: BufReader<File>,
    _directory: tempfile::TempDir,
}
fn run(
    parent: &Path,
    op: u32,
    source: Option<&Odbc>,
    query: Option<&str>,
    table: Option<&OdbcTable>,
    control: &Control,
) -> Result<Output> {
    let directory = tempfile::Builder::new()
        .prefix("tomato-odbc32-")
        .tempdir_in(parent)?;
    let helper = directory.path().join("tomato-odbc.exe");
    std::fs::write(&helper, BINARY)?;
    let output = directory.path().join("result.bin");
    let mut command = Command::new(helper);
    use std::os::windows::process::CommandExt;
    command
        .creation_flags(0x0800_0000)
        .stdin(Stdio::piped())
        .stdout(File::create_new(&output)?)
        .stderr(Stdio::null());
    let mut child = Process(command.spawn()?);
    {
        let mut input = child.0.stdin.take().context("ODBC 桥接输入不可用")?;
        input.write_all(&0x544f4431u32.to_be_bytes())?;
        input.write_all(&op.to_be_bytes())?;
        if let Some(s) = source {
            for value in [&s.dsn, &s.user, &s.password, &s.connection_string] {
                text(&mut input, value)?
            }
        }
        if let Some(query) = query {
            text(&mut input, query)?
        }
        if let Some(table) = table {
            for value in [&table.catalog, &table.schema, &table.name] {
                text(&mut input, value)?
            }
        }
    }
    let status = loop {
        control.check()?;
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    ensure!(
        status.success(),
        "32 位 ODBC 读取失败，请检查驱动、源数据、登录信息或源查询"
    );
    let reader = BufReader::with_capacity(64 * 1024, File::open(output)?);
    Ok(Output {
        _directory: directory,
        reader,
    })
}
pub(super) fn sources(directory: &Path) -> Result<Catalog> {
    let mut out = run(directory, 1, None, None, None, &Control::default())?;
    ensure!(number(&mut out.reader)? == 0x544f5331, "ODBC 桥接协议无效");
    let mut catalog = Catalog::default();
    loop {
        let tag = byte(&mut out.reader)?;
        if tag == 0 {
            break;
        }
        let name = string(&mut out.reader)?;
        match tag {
            1 => catalog.dsns.push(name),
            2 => catalog.drivers.push(name),
            _ => anyhow::bail!("ODBC 来源标记无效"),
        };
        ensure!(
            catalog.dsns.len() + catalog.drivers.len() <= 20000,
            "ODBC 来源过多"
        );
    }
    Ok(catalog)
}
pub(super) fn catalog(source: &Odbc, directory: &Path) -> Result<Catalog> {
    let mut out = run(directory, 2, Some(source), None, None, &Control::default())?;
    ensure!(number(&mut out.reader)? == 0x544f5431, "ODBC 桥接协议无效");
    let mut catalog = Catalog::default();
    while byte(&mut out.reader)? == 1 {
        ensure!(catalog.odbc_tables.len() < 10000, "ODBC 源表过多");
        catalog.odbc_tables.push(OdbcTable {
            catalog: string(&mut out.reader)?,
            schema: string(&mut out.reader)?,
            name: string(&mut out.reader)?,
        });
    }
    Ok(catalog)
}
pub(super) fn read(
    source: &Source,
    directory: &Path,
    output: &mut Catalog,
    control: &Control,
) -> Result<()> {
    let selections = if source.odbc.query.trim().is_empty() {
        ensure!(
            !source.odbc.tables.is_empty() && source.odbc.tables.len() <= 100,
            "请选择 1 至 100 张源表"
        );
        source
            .odbc
            .tables
            .iter()
            .map(|t| (t.name.clone(), Some(t), None))
            .collect::<Vec<_>>()
    } else {
        ensure!(
            tomato_core::sql_script::is_select(Some(&source.odbc.query))
                && tomato_core::sql_script::split_for("SQLITE", Some(&source.odbc.query)).count()
                    == 1,
            "ODBC 源查询必须是单条 SELECT"
        );
        vec![("query".into(), None, Some(source.odbc.query.as_str()))]
    };
    for (name, table, query) in selections {
        let mut out = run(
            directory,
            if table.is_some() { 4 } else { 3 },
            Some(&source.odbc),
            query,
            table,
            control,
        )?;
        let input = &mut out.reader;
        ensure!(number(input)? == 0x544f5231, "ODBC 数据协议无效");
        let count = number(input)? as usize;
        ensure!(count > 0 && count <= MAX_COLUMNS, "ODBC 字段数无效");
        let mut fields = Vec::new();
        for _ in 0..count {
            let name = string(input)?;
            let typ = number(input)? as i32;
            let precision = number(input)?;
            let scale = number(input)? as i32;
            let data_type = match typ {
                -2..=-1 => {
                    if typ == -1 {
                        DataType::Text
                    } else {
                        DataType::Binary
                    }
                }
                -3 | -4 => DataType::Binary,
                -7 => DataType::Boolean,
                -6 | -5 | 4 | 5 => DataType::Integer,
                6..=8 => DataType::Real,
                91 => DataType::Date,
                93 => DataType::DateTime,
                2 | 3
                    if precision > 0
                        && precision <= 38
                        && scale >= 0
                        && scale <= precision as i32 =>
                {
                    DataType::Decimal {
                        precision: precision as u16,
                        scale: scale as u16,
                    }
                }
                _ => DataType::Text,
            };
            fields.push(Field {
                name,
                data_type,
                nullable: true,
            });
        }
        let mut sink = Sink::new(directory, output, name, "ODBC（32 位）".into(), fields)?.typed();
        let mut row = 0;
        loop {
            let tag = byte(input)?;
            if tag == 0 {
                break;
            }
            ensure!(tag == 1, "ODBC 记录标记无效");
            let mut values = Vec::new();
            let mut held = 0;
            for _ in 0..count {
                let value = match byte(input)? {
                    0 => Cell::Null,
                    1 => Cell::Text(string(input)?),
                    5 => {
                        let n = number(input)? as usize;
                        ensure!(n <= MAX_ROW_BYTES, "ODBC 字段过大");
                        let mut bytes = vec![0; n];
                        input.read_exact(&mut bytes)?;
                        Cell::Binary(bytes)
                    }
                    _ => anyhow::bail!("ODBC 单元格类型无效"),
                };
                held += match &value {
                    Cell::Text(s) => s.len(),
                    Cell::Binary(b) => b.len(),
                    _ => 8,
                };
                ensure!(held <= MAX_ROW_BYTES, "ODBC 记录超过 16 MiB");
                values.push(value);
            }
            row += 1;
            if included(source, row) {
                sink.push(values, control)?
            }
        }
        sink.finish(output)?;
    }
    Ok(())
}
