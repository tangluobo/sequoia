use super::*;
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    time::Duration,
};
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn string(out: &mut impl Write, text: &str) -> Result<()> {
    out.write_all(&u32::try_from(text.len())?.to_be_bytes())?;
    out.write_all(text.as_bytes())?;
    Ok(())
}
fn u32(reader: &mut impl Read) -> Result<u32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_be_bytes(bytes))
}
fn byte(reader: &mut impl Read) -> Result<u8> {
    let mut bytes = [0; 1];
    reader.read_exact(&mut bytes)?;
    Ok(bytes[0])
}
fn text(reader: &mut impl Read) -> Result<String> {
    let length = u32(reader)? as usize;
    ensure!(length <= MAX_ROW_BYTES, "Access 字段超过 16 MiB");
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    Ok(String::from_utf8(bytes)?)
}
fn run(
    helper: &Path,
    source: &Source,
    path: &Path,
    output: &Path,
    table: Option<&str>,
    control: &Control,
) -> Result<()> {
    // Credentials travel only through stdin, never through argv, files or diagnostics.
    let mut command = Command::new(helper);
    command
        .arg("-Xmx128m")
        .arg(if table.is_some() { "--read" } else { "--list" })
        .arg(path)
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = Process(command.spawn()?);
    {
        let mut stdin = child.0.stdin.take().context("Access 读取器输入不可用")?;
        string(&mut stdin, &source.access_password)?;
        if let Some(name) = table {
            string(&mut stdin, name)?;
        }
    }
    let status = loop {
        control.check()?;
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if !status.success() {
        // Do not echo codec diagnostics which can contain supplied passwords.
        anyhow::bail!("Access 读取失败，请检查文件完整性和数据库密码");
    }
    Ok(())
}
pub(super) fn read(
    source: &Source,
    path: &Path,
    directory: &Path,
    output: &mut Catalog,
    control: &Control,
) -> Result<()> {
    let helper_dir = tempfile::Builder::new()
        .prefix("tomato-access-import-")
        .tempdir_in(directory)?;
    let helper = helper_dir.path().join(if cfg!(windows) {
        "tomato-access.exe"
    } else {
        "tomato-access"
    });
    std::fs::write(&helper, crate::ACCESS_ADAPTER)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700))?;
    }
    let list = helper_dir.path().join("tables.bin");
    run(&helper, source, path, &list, None, control)?;
    let mut reader = BufReader::new(File::open(list)?);
    ensure!(u32(&mut reader)? == 0x54414c31, "Access 数据表协议无效");
    let count = u32(&mut reader)?;
    ensure!(count <= 10_000, "Access 源表过多");
    let mut names = Vec::new();
    for _ in 0..count {
        names.push(text(&mut reader)?)
    }
    for name in &source.tables {
        ensure!(names.contains(name), "Access 源表不存在：{name}");
    }
    for name in names {
        control.check()?;
        if !source.tables.is_empty() && !source.tables.contains(&name) {
            continue;
        }
        let data = helper_dir.path().join("rows.bin");
        run(&helper, source, path, &data, Some(&name), control)?;
        let mut reader = BufReader::with_capacity(64 * 1024, File::open(data)?);
        ensure!(u32(&mut reader)? == 0x54414931, "Access 数据协议无效");
        let columns = u32(&mut reader)? as usize;
        ensure!(columns > 0 && columns <= MAX_COLUMNS, "Access 字段数无效");
        let mut fields = Vec::new();
        for _ in 0..columns {
            let name = text(&mut reader)?;
            let _typ = text(&mut reader)?;
            fields.push(field(name));
        }
        let mut sink = Sink::new(directory, output, name, path.display().to_string(), fields)?;
        let mut number = 0;
        loop {
            control.check()?;
            match byte(&mut reader)? {
                0 => break,
                1 => {}
                _ => anyhow::bail!("Access 记录标记无效"),
            };
            let mut row = Vec::new();
            let mut size = 0usize;
            for _ in 0..columns {
                row.push(match byte(&mut reader)? {
                    0 => Cell::Null,
                    1 | 2 => Cell::Text(text(&mut reader)?),
                    3 => {
                        let mut b = [0; 8];
                        reader.read_exact(&mut b)?;
                        Cell::Real(f64::from_be_bytes(b))
                    }
                    4 => Cell::Boolean(byte(&mut reader)? != 0),
                    5 => {
                        let n = u32(&mut reader)? as usize;
                        ensure!(n <= MAX_ROW_BYTES, "Access 二进制字段过大");
                        let mut b = vec![0; n];
                        reader.read_exact(&mut b)?;
                        Cell::Binary(b)
                    }
                    _ => anyhow::bail!("Access 单元格类型无效"),
                });
                size = size.saturating_add(cell_bytes(row.last().unwrap()));
                ensure!(size <= MAX_ROW_BYTES, "Access 记录超过 16 MiB");
            }
            number += 1;
            if included(source, number) {
                sink.push(row, control)?;
            }
        }
        sink.finish(output)?;
    }
    Ok(())
}
