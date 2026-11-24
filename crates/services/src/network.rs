use anyhow::{Context, Result, bail};
use std::{net::SocketAddr, path::PathBuf, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use tomato_core::Connection;

pub struct RunningServer {
    pub address: SocketAddr,
    task: JoinHandle<()>,
    shutdown: Option<CancellationToken>,
    config_updates: Option<tokio::sync::watch::Sender<tomato_core::server::ServerConfig>>,
}
impl Drop for RunningServer {
    fn drop(&mut self) {
        if let Some(shutdown) = &self.shutdown {
            shutdown.cancel();
        } else {
            self.task.abort();
        }
    }
}
impl RunningServer {
    pub(crate) fn from_task(address: SocketAddr, task: JoinHandle<()>) -> Self {
        Self {
            address,
            task,
            shutdown: None,
            config_updates: None,
        }
    }
    pub(crate) fn with_shutdown(mut self, shutdown: CancellationToken) -> Self {
        self.shutdown = Some(shutdown);
        self
    }
    pub(crate) fn with_updates(
        mut self,
        updates: tokio::sync::watch::Sender<tomato_core::server::ServerConfig>,
    ) -> Self {
        self.config_updates = Some(updates);
        self
    }
    pub fn update_config(&self, config: tomato_core::server::ServerConfig) -> Result<()> {
        if let Some(updates) = &self.config_updates {
            updates.send(config).context("服务器已停止")?;
        }
        Ok(())
    }
    pub fn finished(&self) -> bool {
        self.task.is_finished()
    }
}

pub async fn serve_files(address: SocketAddr, root: PathBuf) -> Result<RunningServer> {
    let root = std::fs::canonicalize(root)?;
    if !root.is_dir() {
        bail!("共享路径必须是目录");
    }
    let listener = TcpListener::bind(address).await?;
    let address = listener.local_addr()?;
    let app = axum::Router::new().fallback_service(tower_http::services::ServeDir::new(root));
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok(RunningServer::from_task(address, task))
}

pub async fn forward_port(address: SocketAddr, target: String) -> Result<RunningServer> {
    let listener = TcpListener::bind(address).await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let mut clients = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let Ok((mut incoming, _)) = accepted else { break; };
                    let target = target.clone();
                    clients.spawn(async move {
                        if let Ok(Ok(mut outgoing)) = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(target)).await {
                            let _ = tokio::io::copy_bidirectional(&mut incoming, &mut outgoing).await;
                        }
                    });
                }
                _ = clients.join_next(), if !clients.is_empty() => {}
            }
        }
    });
    Ok(RunningServer::from_task(address, task))
}

pub async fn redis_command(connection: &Connection, args: &[String]) -> Result<String> {
    crate::redis_data::raw_command(connection, args).await
}

pub(crate) fn redis_uri(connection: &Connection) -> Result<url::Url> {
    // Java serializes useSsl=true on every connection as an RDP default.
    // Its Redis client is plaintext; that unrelated field must not enable TLS.
    let mut uri = url::Url::parse(if connection.flag("redisTls") {
        "rediss://localhost"
    } else {
        "redis://localhost"
    })?;
    uri.set_host(Some(connection.text("host")))?;
    uri.set_port(Some(u16::try_from(connection.number("port"))?))
        .map_err(|_| anyhow::anyhow!("无效端口"))?;
    if !connection.text("username").is_empty() {
        uri.set_username(connection.text("username"))
            .map_err(|_| anyhow::anyhow!("无效用户名"))?;
    }
    if !connection.text("password").is_empty() {
        uri.set_password(Some(connection.text("password")))
            .map_err(|_| anyhow::anyhow!("无效密码"))?;
    }
    uri.set_path(&format!("/{}", connection.number("redisDatabase")));
    Ok(uri)
}

pub fn download(url: &str, destination: &std::path::Path) -> Result<u64> {
    crate::init_crypto();
    let url = url::Url::parse(url)?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("此快捷下载命令仅支持 HTTP/HTTPS；BT 和 eD2k 请在下载器中添加任务");
    }
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(3600))
        .build()?;
    let mut response = client.get(url).send()?.error_for_status()?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    let size = std::io::copy(&mut response, &mut temporary)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(destination)
        .context("下载完成但保存文件失败")?;
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[test]
    fn legacy_rdp_ssl_flag_does_not_enable_redis_tls() -> Result<()> {
        let mut connection = Connection::new("REDIS");
        connection.set("useSsl", true);
        connection.set("redisDatabase", 3);
        let uri = redis_uri(&connection)?;
        assert_eq!(uri.scheme(), "redis");
        assert_eq!(uri.path(), "/3");
        connection.set("redisTls", true);
        assert_eq!(redis_uri(&connection)?.scheme(), "rediss");
        Ok(())
    }
    #[tokio::test]
    async fn forward_roundtrip_and_shutdown() -> Result<()> {
        let echo = TcpListener::bind("127.0.0.1:0").await?;
        let destination = echo.local_addr()?;
        let task = tokio::spawn(async move {
            let (mut client, _) = echo.accept().await?;
            let mut buffer = [0; 4];
            client.read_exact(&mut buffer).await?;
            client.write_all(&buffer).await?;
            Ok::<_, std::io::Error>(())
        });
        let forward = forward_port("127.0.0.1:0".parse()?, destination.to_string()).await?;
        let mut client = TcpStream::connect(forward.address).await?;
        client.write_all(b"test").await?;
        let mut buffer = [0; 4];
        client.read_exact(&mut buffer).await?;
        assert_eq!(&buffer, b"test");
        task.await??;
        drop(forward);
        Ok(())
    }
    #[tokio::test]
    async fn http_serves_files() -> Result<()> {
        let directory = tempfile::tempdir()?;
        std::fs::write(directory.path().join("sample.txt"), "tomato")?;
        let server = serve_files("127.0.0.1:0".parse()?, directory.path().into()).await?;
        let response = reqwest::get(format!("http://{}/sample.txt", server.address)).await?;
        assert_eq!(response.text().await?, "tomato");
        Ok(())
    }
}
