//! Isolated native Jackcess writer. No system Java/Office and no startup worker.
use super::*;
use crate::ACCESS_ADAPTER as BINARY;
use std::{
    io::Read,
    process::{Command, Stdio},
    time::Duration,
};
use tomato_core::workbench::Column;
struct Child(std::process::Child);
impl Drop for Child {
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
fn kind(column: &Column) -> u8 {
    let t = column.data_type.to_ascii_lowercase();
    if column.binary {
        4
    } else if matches!(
        t.split([' ', '(']).next().unwrap_or_default(),
        "tinyint"
            | "smallint"
            | "mediumint"
            | "int"
            | "integer"
            | "bigint"
            | "int2"
            | "int4"
            | "int8"
            | "serial"
            | "bigserial"
            | "smallserial"
    ) {
        1
    } else if matches!(t.as_str(), "bool" | "boolean") {
        3
    } else if matches!(
        t.as_str(),
        "float" | "double" | "real" | "binary_float" | "binary_double"
    ) {
        2
    } else {
        0
    }
}
pub(super) fn write(
    spool: &Spool,
    columns: &[Column],
    options: &Options,
    control: &Control,
    path: &Path,
) -> Result<()> {
    ensure!(columns.len() <= 255, "Access 最多 255 个字段");
    let directory = tempfile::tempdir()?;
    let helper = directory.path().join(if cfg!(windows) {
        "tomato-access.exe"
    } else {
        "tomato-access"
    });
    std::fs::write(&helper, BINARY)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700))?;
    }
    let input = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    let mut writer = BufWriter::new(input.as_file());
    writer.write_all(&0x54414331u32.to_be_bytes())?;
    string(&mut writer, &spool.schema.table.name)?;
    writer.write_all(&(columns.len() as u32).to_be_bytes())?;
    for column in columns {
        string(&mut writer, &column.name)?;
        writer.write_all(&[kind(column)])?;
    }
    spool.each(options, control, |row| {
        writer.write_all(&[1])?;
        for (cell, column) in row.iter().zip(columns) {
            match cell {
                Cell::Null => writer.write_all(&[0])?,
                Cell::Binary(bytes) => {
                    writer.write_all(&[5])?;
                    writer.write_all(&u32::try_from(bytes.len())?.to_be_bytes())?;
                    writer.write_all(bytes)?;
                }
                _ => {
                    let text = value(cell);
                    match kind(column) {
                        1 => {
                            writer.write_all(&[2])?;
                            string(&mut writer, &text)?;
                        }
                        2 => {
                            let n = text.parse::<f64>()?;
                            ensure!(n.is_finite(), "Access 不支持非有限数值");
                            writer.write_all(&[3])?;
                            writer.write_all(&n.to_be_bytes())?;
                        }
                        3 => {
                            let boolean = match text.to_ascii_lowercase().as_str() {
                                "1" | "t" | "true" => true,
                                "0" | "f" | "false" => false,
                                _ => anyhow::bail!("布尔字段值无效：{}", column.name),
                            };
                            writer.write_all(&[4, u8::from(boolean)])?;
                        }
                        _ => {
                            writer.write_all(&[1])?;
                            string(&mut writer, &text)?;
                        }
                    }
                }
            }
        }
        Ok(())
    })?;
    writer.write_all(&[0])?;
    writer.flush()?;
    drop(writer);
    let stderr = tempfile::NamedTempFile::new()?;
    let mut command = Command::new(&helper);
    command
        .arg("-Xmx128m")
        .arg(path)
        .stdin(File::open(input.path())?)
        .stdout(Stdio::null())
        .stderr(stderr.reopen()?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = Child(command.spawn().context("启动内置 Access 写入器失败")?);
    let status = loop {
        if control.cancelled() {
            anyhow::bail!("操作已取消");
        }
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if !status.success() {
        let mut error = String::new();
        File::open(stderr.path())?
            .take(8192)
            .read_to_string(&mut error)?;
        anyhow::bail!("Access 导出失败：{error}");
    }
    Ok(())
}
