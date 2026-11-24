use super::*;
use anyhow::Context as _;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Moved,
    Ask,
    Steady,
}
struct Peer {
    ports: [u16; 2],
    mode: Mode,
    moved: AtomicBool,
    commands: std::sync::Mutex<Vec<(usize, Vec<String>)>>,
}
fn bulk(value: &str) -> String {
    format!("${}\r\n{value}\r\n", value.len())
}
fn slot(key: &str) -> u16 {
    let mut crc = 0u16;
    for byte in key.bytes() {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc % 16384
}
impl Peer {
    fn topology(&self) -> String {
        let row = |start, end, node: usize| {
            format!(
                "*3\r\n:{start}\r\n:{end}\r\n*3\r\n{}:{}\r\n{}",
                bulk("redis.internal.test"),
                self.ports[node],
                bulk(&format!("node-{node}"))
            )
        };
        if self.mode == Mode::Steady {
            format!("*2\r\n{}{}", row(0, 8191, 0), row(8192, 16383, 1))
        } else {
            format!(
                "*1\r\n{}",
                row(0, 16383, usize::from(self.moved.load(Ordering::Acquire)))
            )
        }
    }
}
async fn serve<S: AsyncRead + AsyncWrite + Unpin>(
    socket: S,
    peer: Arc<Peer>,
    node: usize,
) -> Result<()> {
    let mut socket = BufReader::new(socket);
    let mut asking = false;
    loop {
        let mut line = String::new();
        match socket.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                ) =>
            {
                break;
            }
            Err(error) => return Err(error.into()),
        }
        let count: usize = line
            .trim()
            .strip_prefix('*')
            .context("RESP array missing")?
            .parse()?;
        ensure!(count <= 32, "Fixture command bound");
        let mut args = Vec::new();
        for _ in 0..count {
            line.clear();
            socket.read_line(&mut line).await?;
            let size: usize = line
                .trim()
                .strip_prefix('$')
                .context("RESP argument missing")?
                .parse()?;
            ensure!(size <= 4096, "Fixture argument bound");
            let mut bytes = vec![0; size + 2];
            socket.read_exact(&mut bytes).await?;
            args.push(String::from_utf8(bytes[..size].to_vec())?);
        }
        peer.commands.lock().unwrap().push((node, args.clone()));
        let response = match args.first().map(String::as_str) {
            Some("PING") => "+PONG\r\n".into(),
            Some("CLUSTER") => peer.topology(),
            Some("GET") if node == 0 && peer.mode != Mode::Steady => {
                if peer.mode == Mode::Moved {
                    peer.moved.store(true, Ordering::Release);
                }
                format!(
                    "-{} {} redis.internal.test:{}\r\n",
                    if peer.mode == Mode::Moved {
                        "MOVED"
                    } else {
                        "ASK"
                    },
                    slot(&args[1]),
                    peer.ports[1]
                )
            }
            Some("GET") => {
                ensure!(
                    peer.mode != Mode::Ask || asking,
                    "ASKING must precede the redirected request"
                );
                bulk("owned cluster value")
            }
            Some("ASKING") => {
                asking = true;
                "+OK\r\n".into()
            }
            Some("SCAN") => format!("*2\r\n{}*1\r\n{}", bulk("0"), bulk(&format!("key-{node}"))),
            _ => "+OK\r\n".into(),
        };
        socket.get_mut().write_all(response.as_bytes()).await?;
        socket.get_mut().flush().await?;
    }
    Ok(())
}
struct Jobs(Vec<tokio::task::JoinHandle<()>>);
impl Drop for Jobs {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

#[tokio::test]
async fn cluster_discovery_redirect_auth_tls_and_primary_scan_use_scoped_encrypted_routes()
-> Result<()> {
    crate::init_crypto();
    for tls in [false, true] {
        for mode in [Mode::Moved, Mode::Ask, Mode::Steady] {
            let jump = crate::ssh_fixture::Jump::start().await?;
            let listeners = [
                tokio::net::TcpListener::bind("127.0.0.1:0").await?,
                tokio::net::TcpListener::bind("127.0.0.1:0").await?,
            ];
            let ports = [
                listeners[0].local_addr()?.port(),
                listeners[1].local_addr()?.port(),
            ];
            let peer = Arc::new(Peer {
                ports,
                mode,
                moved: AtomicBool::new(false),
                commands: Default::default(),
            });
            let certificate =
                rcgen::generate_simple_self_signed(vec!["redis.internal.test".into()])?;
            let cert = certificate.cert.der().clone();
            let key = rustls::pki_types::PrivatePkcs8KeyDer::from(
                certificate.signing_key.serialize_der(),
            );
            let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(
                rustls::ServerConfig::builder()
                    .with_no_client_auth()
                    .with_single_cert(vec![cert], key.into())?,
            ));
            let mut jobs = Jobs(Vec::new());
            for (node, listener) in listeners.into_iter().enumerate() {
                let peer = peer.clone();
                let acceptor = acceptor.clone();
                jobs.0.push(tokio::spawn(async move {
                    let mut children=tokio::task::JoinSet::new();
                    loop {tokio::select! {
                        accepted=listener.accept()=>{
                            let Ok((socket,_))=accepted else {break};let peer=peer.clone();let acceptor=acceptor.clone();
                            children.spawn(async move {
                                if tls {if let Ok(socket)=acceptor.accept(socket).await {
                                    assert_eq!(socket.get_ref().1.server_name(),Some("redis.internal.test"));
                                    serve(socket,peer,node).await.unwrap();
                                }} else {serve(socket,peer,node).await.unwrap();}
                            });
                        },
                        _=children.join_next(),if !children.is_empty()=>{},
                    }}
                }));
            }
            let profile = jump.root.path().join("connections.json");
            tomato_core::Store {
                connections: vec![jump.connection.clone()],
                ..Default::default()
            }
            .save(&profile)?;
            let bytes = std::fs::read(&profile)?;
            let mut connection = Connection::new("REDIS");
            connection.set("redisCluster", true);
            connection.set("redisTls", tls);
            connection.set(
                "redisClusterNodes",
                format!("redis.internal.test:{}", ports[0]),
            );
            connection.set("username", "fixture");
            connection.set("password", "fixture-only");
            connection.set("useSshTunnel", true);
            connection.set("sshTunnelHostId", jump.connection.id.clone());
            let before = serde_json::to_value(&connection)?;
            let certificates = tls.then(|| redis::TlsCertificates {
                client_tls: None,
                root_cert: Some(certificate.cert.pem().into_bytes()),
            });
            let mut session = tokio::time::timeout(
                Duration::from_secs(15),
                session_at(&connection, &profile, certificates),
            )
            .await??;
            let value: String = tokio::time::timeout(
                Duration::from_secs(10),
                query(&mut session, redis::cmd("GET").arg("fixture-key")),
            )
            .await??;
            assert_eq!(value, "owned cluster value");
            if !tls && mode == Mode::Steady {
                let Output::Keys { keys, .. } =
                    super::super::cluster_scan(&mut session, &connection, "*".into()).await?
                else {
                    anyhow::bail!("Missing cluster keys")
                };
                assert_eq!(keys, ["key-0", "key-1"]);
            }
            let Session::Cluster {
                routes: Some(lease),
                ..
            } = &session
            else {
                anyhow::bail!("Missing route owner")
            };
            let resolver = lease.resolver();
            assert_eq!(resolver.0.routes.lock().len(), 2);
            assert_eq!(
                jump.authentications.load(Ordering::Relaxed),
                1,
                "Cluster nodes must share one authenticated SSH transport"
            );
            assert!(
                jump.targets
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(host, port)| host == "redis.internal.test"
                        && *port == u32::from(ports[1]))
            );
            {
                let commands = peer.commands.lock().unwrap();
                assert!(
                    commands
                        .iter()
                        .any(|(_, args)| args == &["AUTH", "fixture", "fixture-only"])
                );
                if mode == Mode::Ask {
                    assert!(
                        commands
                            .iter()
                            .any(|(node, args)| *node == 1 && args == &["ASKING"])
                    );
                }
            }
            drop(session);
            assert!(resolver.0.closed.is_cancelled());
            assert!(resolver.0.routes.lock().is_empty());
            assert!(resolver.0.transport.lock().is_none());
            assert!(resolver.address("127.0.0.1", ports[1]).await.is_err());
            assert_eq!(std::fs::read(&profile)?, bytes);
            assert_eq!(serde_json::to_value(connection)?, before);
        }
    }
    Ok(())
}

#[tokio::test]
async fn resolver_bounds_nodes_reuses_ip_routes_and_cancels_owned_sockets() -> Result<()> {
    use redis::io::AsyncDNSResolver as _;
    let jump = crate::ssh_fixture::Jump::start().await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let lease = Resolver::via(
        jump.connection.clone(),
        jump.root.path().join("known_hosts"),
    );
    let resolver = lease.resolver();
    let address = resolver.address("127.0.0.1", port).await?;
    assert_eq!(resolver.address("127.0.0.1", port).await?, address);
    let mut socket = tokio::net::TcpStream::connect(address).await?;
    let (mut peer, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept()).await??;
    socket.write_all(b"owned").await?;
    let mut bytes = [0; 5];
    peer.read_exact(&mut bytes).await?;
    assert_eq!(&bytes, b"owned");
    assert!(resolver.resolve("", port).await.is_err());
    for i in 1..128 {
        resolver.0.routes.lock().insert(
            (format!("reserved-{i}.invalid"), port),
            Arc::new(OnceCell::new()),
        );
    }
    assert!(resolver.address("extra.invalid", port).await.is_err());
    drop(lease);
    let mut byte = [0];
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), peer.read(&mut byte)).await??,
        0
    );
    assert!(resolver.address("127.0.0.1", port).await.is_err());
    Ok(())
}
