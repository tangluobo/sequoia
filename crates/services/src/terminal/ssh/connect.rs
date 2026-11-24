use super::*;
use std::collections::HashSet;
pub(super) struct Connected {
    pub session: client::Handle<Client>,
    pub _parents: Vec<client::Handle<Client>>,
    pub _socket_guard: SocketGuard,
}
pub(super) async fn authenticated(
    connection: &Connection,
    known_hosts: PathBuf,
    input: &mut mpsc::Receiver<Input>,
    events: &Events,
) -> Result<Connected> {
    authenticated_forward(connection, known_hosts, input, events, None).await
}
pub(super) async fn authenticated_forward(
    connection: &Connection,
    known_hosts: PathBuf,
    input: &mut mpsc::Receiver<Input>,
    events: &Events,
    forwards: Option<mpsc::Sender<russh::Channel<client::Msg>>>,
) -> Result<Connected> {
    let mut chain = vec![connection.clone()];
    if connection.flag("useSshTunnel") {
        let path = known_hosts.with_file_name("connections.json");
        let store = tokio::task::spawn_blocking(move || tomato_core::Store::load(&path)).await??;
        let mut seen = HashSet::from([connection.id.clone()]);
        while chain.last().is_some_and(|c| c.flag("useSshTunnel")) {
            ensure!(chain.len() < 16, "SSH 跳板层级超过 16 层");
            let id = chain
                .last()
                .context("缺少 SSH 目标")?
                .text("sshTunnelHostId");
            if id.is_empty()
                && let Some(jump) =
                    super::tunnel::Tunnel::inline_jump(chain.last().context("缺少 SSH 目标")?)
            {
                ensure!(seen.insert(jump.id.clone()), "SSH 跳板配置存在循环引用");
                chain.push(jump);
                continue;
            }
            ensure!(
                !id.is_empty() && seen.insert(id.to_string()),
                "SSH 跳板配置为空或存在循环引用"
            );
            let next = store
                .connections
                .iter()
                .find(|c| c.id == id)
                .context("找不到引用的 SSH 跳板连接")?;
            ensure!(
                matches!(next.kind(), "SSH" | "SFTP"),
                "跳板连接必须为 SSH/SFTP 类型"
            );
            chain.push(next.clone());
        }
    }
    chain.reverse();
    let config = Arc::new(client::Config {
        keepalive_interval: Some(Duration::from_secs(10)),
        keepalive_max: 3,
        ..Default::default()
    });
    let first = chain.first().context("缺少 SSH 连接")?;
    let host = first.text("host").to_string();
    let port = u16::try_from(first.number("port"))?;
    let (mut session, guard) = tokio::time::timeout(Duration::from_secs(30), async {
        let socket = tokio::net::TcpStream::connect((host.as_str(), port))
            .await?
            .into_std()?;
        let guard = SocketGuard(socket.try_clone()?);
        let socket = tokio::net::TcpStream::from_std(socket)?;
        let session = client::connect_stream(
            config.clone(),
            socket,
            Client {
                host,
                port,
                known_hosts: known_hosts.clone(),
                forwards: if chain.len() == 1 {
                    forwards.clone()
                } else {
                    None
                },
            },
        )
        .await?;
        Ok::<_, anyhow::Error>((session, guard))
    })
    .await
    .context("SSH 跳板或目标连接超时")??;
    auth::authenticate(&mut session, first, events, input).await?;
    let mut parents = Vec::new();
    for next in chain.iter().skip(1) {
        let host = next.text("host").to_string();
        let port = u16::try_from(next.number("port"))?;
        let channel =
            timed(session.channel_open_direct_tcpip(&host, u32::from(port), "127.0.0.1", 0))
                .await
                .context("跳板拒绝转发目标端口")?;
        let mut target = tokio::time::timeout(
            Duration::from_secs(30),
            client::connect_stream(
                config.clone(),
                channel.into_stream(),
                Client {
                    host,
                    port,
                    known_hosts: known_hosts.clone(),
                    forwards: if next.id == connection.id {
                        forwards.clone()
                    } else {
                        None
                    },
                },
            ),
        )
        .await
        .context("SSH 跳板后的目标连接超时")??;
        auth::authenticate(&mut target, next, events, input).await?;
        parents.push(session);
        session = target;
    }
    Ok(Connected {
        session,
        _parents: parents,
        _socket_guard: guard,
    })
}
