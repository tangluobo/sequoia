//! A scoped loopback SOCKS bridge preserves TLS/SNI even for IP-address endpoints.
use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
pub(super) struct Proxy {
    pub address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Proxy {
    pub async fn start(forward: SocketAddr, host: String, port: u16) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let task = tokio::spawn(async move {
            let mut jobs = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept(), if jobs.len() < 16 => {
                        let Ok((client, _)) = accepted else { break; };
                        let host = host.clone();
                        jobs.spawn(async move {
                            let handshake = tokio::time::timeout(Duration::from_secs(10), accept(client, forward, &host, port)).await;
                            if let Ok(Ok((mut client, mut remote))) = handshake {
                                let _ = tokio::io::copy_bidirectional(&mut client, &mut remote).await;
                            }
                        });
                    }
                    _ = jobs.join_next(), if !jobs.is_empty() => {},
                }
            }
        });
        Ok(Self { address, task })
    }
}
async fn accept(
    mut client: TcpStream,
    forward: SocketAddr,
    host: &str,
    port: u16,
) -> Result<(TcpStream, TcpStream)> {
    let version = client.read_u8().await?;
    let methods = client.read_u8().await?;
    ensure!(version == 5 && methods > 0, "存储代理协议无效");
    let mut methods = vec![0; usize::from(methods)];
    client.read_exact(&mut methods).await?;
    ensure!(methods.contains(&0), "存储代理认证方式无效");
    client.write_all(&[5, 0]).await?;
    let mut header = [0; 4];
    client.read_exact(&mut header).await?;
    ensure!(header[..3] == [5, 1, 0], "存储代理只支持 TCP 连接");
    let requested = match header[3] {
        1 => {
            let mut ip = [0; 4];
            client.read_exact(&mut ip).await?;
            std::net::Ipv4Addr::from(ip).to_string()
        }
        4 => {
            let mut ip = [0; 16];
            client.read_exact(&mut ip).await?;
            std::net::Ipv6Addr::from(ip).to_string()
        }
        3 => {
            let length = client.read_u8().await?;
            let mut name = vec![0; usize::from(length)];
            client.read_exact(&mut name).await?;
            String::from_utf8(name)?.to_ascii_lowercase()
        }
        _ => bail!("存储代理地址无效"),
    };
    let requested_port = client.read_u16().await?;
    let host = host.trim_matches(['[', ']']);
    ensure!(
        (requested == host || requested.ends_with(&format!(".{host}"))) && requested_port == port,
        "存储代理目标超出连接端点"
    );
    let remote = TcpStream::connect(forward).await?;
    client.set_nodelay(true)?;
    remote.set_nodelay(true)?;
    client.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0]).await?;
    Ok((client, remote))
}
