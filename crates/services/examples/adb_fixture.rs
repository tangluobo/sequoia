//! Owned ADB device on an ephemeral loopback port for integration and GUI capture tests.
#[path = "adb_fixture/scrcpy.rs"]
mod scrcpy;
#[path = "adb_fixture/wire.rs"]
mod wire;

use anyhow::{Context, Result, ensure};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream},
    net::TcpListener,
};

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    let files = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .context("Usage: adb_fixture <marker base>")?,
    );
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let temporary = files.with_extension("port.tmp");
    std::fs::write(&temporary, listener.local_addr()?.port().to_string())?;
    std::fs::rename(temporary, files.with_extension("port"))?;
    loop {
        let (socket, _) = listener.accept().await?;
        let files = files.clone();
        tokio::spawn(async move {
            if let Err(error) = wire::serve(socket, files).await {
                eprintln!("ADB fixture: {error:#}");
            }
        });
    }
}

fn log(files: &Path, values: &[&str]) -> Result<()> {
    let mut line = serde_json::to_vec(values)?;
    line.push(b'\n');
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(files.with_extension("commands.jsonl"))?
        .write_all(&line)?;
    Ok(())
}

async fn png(files: PathBuf, mut stream: DuplexStream) -> Result<()> {
    let image = image::RgbImage::from_fn(96, 64, |x, y| image::Rgb([x as u8, y as u8, 127]));
    let mut encoded = std::io::Cursor::new(Vec::new());
    image.write_to(&mut encoded, image::ImageFormat::Png)?;
    for count in 0u64.. {
        stream.write_all(encoded.get_ref()).await?;
        std::fs::write(files.with_extension("heartbeat"), count.to_string())?;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}

async fn shell(files: PathBuf, stream: DuplexStream) -> Result<()> {
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read).lines();
    while let Some(line) = lines.next_line().await? {
        log(&files, &[&line])?;
        write.write_all(b"\n__TOMATO_ADB_ACK__:0\n").await?;
    }
    Ok(())
}

async fn sync(files: PathBuf, mut stream: DuplexStream) -> Result<()> {
    const SERVER: &[u8] = include_bytes!("../assets/adb/scrcpy-server-v3.3.4.jar");
    let mut received = Vec::new();
    let mut remote = String::new();
    loop {
        let mut id = [0; 4];
        stream.read_exact(&mut id).await?;
        let size = stream.read_u32_le().await? as usize;
        match &id {
            b"SEND" => {
                ensure!(size < 1024, "fixture path too long");
                let mut name = vec![0; size];
                stream.read_exact(&mut name).await?;
                let name = std::str::from_utf8(&name)?;
                remote = name.rsplit_once(',').context("missing sync mode")?.0.into();
                ensure!(
                    remote.starts_with("/data/local/tmp/tomato-") && remote.ends_with(".jar"),
                    "unexpected remote path"
                );
            }
            b"DATA" => {
                ensure!(
                    received.len() + size <= SERVER.len(),
                    "unexpected jar length"
                );
                let offset = received.len();
                received.resize(offset + size, 0);
                stream.read_exact(&mut received[offset..]).await?;
            }
            b"DONE" => {
                ensure!(received == SERVER, "invalid scrcpy upload");
                log(&files, &["push", &remote])?;
                stream.write_all(b"OKAY\0\0\0\0").await?;
            }
            b"QUIT" => return Ok(()),
            _ => anyhow::bail!("unexpected sync request"),
        }
    }
}
