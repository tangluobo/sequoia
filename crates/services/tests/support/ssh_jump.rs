//! Disposable SSH forwarding peer: no saved connections or user known-host files.
use anyhow::Result;
use russh::{
    Channel,
    keys::{PrivateKey, ssh_key::private::Ed25519Keypair},
    server,
};
use std::{sync::Arc, time::Duration};

pub struct Jump {
    pub connection: tomato_core::Connection,
    pub root: tempfile::TempDir,
    pub targets: Arc<std::sync::Mutex<Vec<(String, u32)>>>,
    #[allow(dead_code)] // Not every consumer checks transport reuse.
    pub authentications: Arc<std::sync::atomic::AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
struct Handler {
    authentications: Arc<std::sync::atomic::AtomicUsize>,
    targets: Arc<std::sync::Mutex<Vec<(String, u32)>>>,
    jobs: tokio::task::JoinSet<()>,
}
impl server::Handler for Handler {
    type Error = anyhow::Error;
    async fn auth_password(&mut self, user: &str, password: &str) -> Result<server::Auth> {
        Ok(
            if user == "jump-fixture" && password == "jump-fixture-only" {
                self.authentications
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                server::Auth::Accept
            } else {
                server::Auth::reject()
            },
        )
    }
    #[allow(clippy::too_many_arguments)]
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<server::Msg>,
        host: &str,
        port: u32,
        _: &str,
        _: u32,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<()> {
        anyhow::ensure!(
            matches!(
                host,
                "rdp.internal.test" | "redis.internal.test" | "hive.internal.test" | "127.0.0.1"
            ),
            "Only fixture targets are allowed"
        );
        self.targets.lock().unwrap().push((host.to_owned(), port));
        let mut socket =
            tokio::net::TcpStream::connect(("127.0.0.1", u16::try_from(port)?)).await?;
        socket.set_nodelay(true)?;
        reply.accept().await;
        self.jobs.spawn(async move {
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut socket).await;
        });
        Ok(())
    }
}
impl Jump {
    pub async fn start() -> Result<Self> {
        let root = tempfile::tempdir()?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let config = Arc::new(server::Config {
            keys: vec![PrivateKey::from(Ed25519Keypair::from_seed(&[119; 32]))],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let targets = Arc::new(std::sync::Mutex::new(Vec::new()));
        let requests = targets.clone();
        let authentications = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let auth_count = authentications.clone();
        let task = tokio::spawn(async move {
            let mut jobs = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    socket = listener.accept() => {
                        let Ok((socket, _)) = socket else { break; };
                        let config = config.clone();
                        let handler = Handler { authentications:auth_count.clone(), targets: requests.clone(), jobs: tokio::task::JoinSet::new() };
                        jobs.spawn(async move {
                            if let Ok(session) = server::run_stream(config, socket, handler).await {
                                let _ = session.await;
                            }
                        });
                    },
                    _ = jobs.join_next(), if !jobs.is_empty() => {},
                }
            }
        });
        let mut connection = tomato_core::Connection::new("SSH");
        connection.set("host", "127.0.0.1");
        connection.set("port", address.port());
        connection.set("username", "jump-fixture");
        connection.set("password", "jump-fixture-only");
        Ok(Self {
            connection,
            root,
            targets,
            authentications,
            task,
        })
    }
}
impl Drop for Jump {
    fn drop(&mut self) {
        self.task.abort();
    }
}
