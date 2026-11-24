//! OpenDAL's FTP adapter has no rename capability; issue native RNFR/RNTO.
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
type Client = suppaftp::tokio::AsyncFtpStream;
async fn connect(connection: &Connection) -> Result<Client> {
    tokio::time::timeout(Duration::from_secs(30), async {
        let port = u16::try_from(connection.number("port"))?;
        ensure!(port > 0, "FTP端口必须在1–65535之间");
        let mut client = Client::connect((connection.text("host"), port)).await?;
        let user = if connection.text("username").is_empty() {
            "anonymous"
        } else {
            connection.text("username")
        };
        client.login(user, connection.text("password")).await?;
        client
            .transfer_type(suppaftp::types::FileType::Binary)
            .await?;
        Ok::<_, anyhow::Error>(client)
    })
    .await
    .context("FTP连接超时")?
}
pub(super) async fn download(
    connection: &Connection,
    path: &str,
    output: &mut (impl tokio::io::AsyncWrite + Unpin),
) -> Result<u64> {
    read_into(connection, path, output, u64::MAX).await
}
pub(super) async fn read_text(connection: &Connection, path: &str) -> Result<String> {
    let mut bytes = Vec::new();
    read_into(connection, path, &mut bytes, 8 * 1024 * 1024).await?;
    String::from_utf8(bytes).context("文件不是 UTF-8 文本，请使用外部应用打开")
}
async fn read_into(
    connection: &Connection,
    path: &str,
    output: &mut (impl tokio::io::AsyncWrite + Unpin),
    limit: u64,
) -> Result<u64> {
    let mut client = connect(connection).await?;
    let mut stream = tokio::time::timeout(
        Duration::from_secs(30),
        client.retr_as_stream(format!("/{path}")),
    )
    .await
    .context("FTP下载启动超时")??;
    let mut buffer = vec![0; 64 * 1024];
    let mut total = 0;
    loop {
        let n = tokio::time::timeout(Duration::from_secs(30), stream.read(&mut buffer))
            .await
            .context("FTP下载读取超时")??;
        if n == 0 {
            break;
        }
        ensure!(total + n as u64 <= limit, "文本文件超过 8 MiB");
        output.write_all(&buffer[..n]).await?;
        total += n as u64;
    }
    tokio::time::timeout(Duration::from_secs(30), client.finalize_retr_stream(stream))
        .await
        .context("FTP下载确认超时")??;
    let _ = tokio::time::timeout(Duration::from_secs(1), client.quit()).await;
    Ok(total)
}
pub(super) async fn rename(connection: &Connection, from: &str, to: &str) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(30), async {
        let mut client = connect(connection).await?;
        client.rename(format!("/{from}"), format!("/{to}")).await?;
        let _ = tokio::time::timeout(Duration::from_secs(1), client.quit()).await;
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("FTP重命名超时")?
}
// A 550 reply can mean either missing or forbidden. Preserve that error rather
// than treating every 550 as an idempotent success, as OpenDAL's deleter does.
pub(super) async fn remove(connection: &Connection, entries: &[(String, bool)]) -> Result<()> {
    let mut client = connect(connection).await?;
    for (path, directory) in entries {
        ensure!(!path.contains(['\r', '\n']), "FTP路径含有无效换行符");
        tokio::time::timeout(Duration::from_secs(30), async {
            if *directory {
                client.rmdir(format!("/{path}")).await
            } else {
                client.rm(format!("/{path}")).await
            }
        })
        .await
        .context("FTP删除超时")??;
    }
    let _ = tokio::time::timeout(Duration::from_secs(1), client.quit()).await;
    Ok(())
}
