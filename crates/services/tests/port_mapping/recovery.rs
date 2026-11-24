use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tomato_services::terminal::ssh::forwarding::ForwardState;

struct Socket(std::net::TcpStream);
impl Drop for Socket {
    fn drop(&mut self) {
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}
async fn ssh_server(port: u16, accepted: Arc<AtomicUsize>) -> Result<Task> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    let config = Arc::new(server::Config {
        keys: vec![PrivateKey::from(Ed25519Keypair::from_seed(&[122; 32]))],
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    Ok(Task(tokio::spawn(async move {
        let mut jobs = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                socket = listener.accept() => {
                    let Ok((socket,_)) = socket else { break; };
                    accepted.fetch_add(1, Ordering::SeqCst);
                    let config = config.clone();
                    jobs.spawn(async move {
                        let socket = socket.into_std()?;
                        let _close = Socket(socket.try_clone()?);
                        let socket = TcpStream::from_std(socket)?;
                        let session = server::run_stream(config, socket, Handler::default()).await?;
                        session.await?;
                        Ok::<_, anyhow::Error>(())
                    });
                }
                _ = jobs.join_next(), if !jobs.is_empty() => {}
            }
        }
    })))
}
async fn echo_server() -> Result<(u16, Task)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let task = Task(tokio::spawn(async move {
        let mut jobs = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                socket = listener.accept() => {
                    let Ok((mut socket,_)) = socket else { break; };
                    jobs.spawn(async move {
                        let mut bytes = [0;8192];
                        while let Ok(n) = socket.read(&mut bytes).await {
                            if n == 0 || socket.write_all(&bytes[..n]).await.is_err() { break; }
                        }
                    });
                }
                _ = jobs.join_next(), if !jobs.is_empty() => {}
            }
        }
    }));
    Ok((port, task))
}
async fn wait(forward: &Forward, running: bool) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if if running {
                forward.is_running()
            } else {
                matches!(forward.state(), ForwardState::Retrying { .. })
            } {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    ensure!(forward.is_active(), "supervisor exited unexpectedly");
    Ok(())
}
async fn payload(port: u16) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut socket = TcpStream::connect(("127.0.0.1", port)).await?;
        let bytes: Vec<_> = (0..16384).map(|i| (i % 251) as u8).collect();
        socket.write_all(&bytes).await?;
        let mut actual = vec![0; bytes.len()];
        socket.read_exact(&mut actual).await?;
        ensure!(actual == bytes, "reconnected tunnel corrupted bytes");
        Ok::<_, anyhow::Error>(())
    })
    .await?
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn both_directions_recover_initial_failure_and_server_restart_and_stop_cancels_retry()
-> Result<()> {
    tokio::time::timeout(Duration::from_secs(65), async {
        let directory = tempfile::tempdir()?;
        let (echo, _echo) = echo_server().await?;
        for direction in ["L", "R"] {
            let port = free_port().await?;
            let accepted = Arc::new(AtomicUsize::new(0));
            let mut host = tomato_core::Connection::new("SSH");
            host.set("host", "127.0.0.1");
            host.set("port", port);
            host.set("username", "fixture");
            host.set("password", "fixture-only");
            host.set("usePassword", true);
            let forwarded = free_port().await?;
            let mapping = Mapping {
                ssh_host_id: host.id.clone(),
                direction: direction.into(),
                local_port: if direction == "L" { forwarded } else { echo },
                remote_port: if direction == "L" { echo } else { forwarded },
                ..Default::default()
            };
            let wake = Arc::new(AtomicUsize::new(0));
            let notify = wake.clone();
            let forward = Forward::start(
                mapping,
                host,
                directory.path().join(format!("known-{direction}")),
                move || {
                    notify.fetch_add(1, Ordering::SeqCst);
                },
            )?;
            wait(&forward, false).await?;
            let server = ssh_server(port, accepted.clone()).await?;
            wait(&forward, true).await?;
            payload(forwarded).await?;
            drop(server);
            wait(&forward, false).await?;
            let server = ssh_server(port, accepted.clone()).await?;
            wait(&forward, true).await?;
            payload(forwarded).await?;
            ensure!(
                accepted.load(Ordering::SeqCst) >= 2 && wake.load(Ordering::SeqCst) >= 5,
                "recovery did not establish a new SSH session or notify the UI"
            );
            drop(server);
            wait(&forward, false).await?;
            let ForwardState::Retrying { delay, .. } = forward.state() else {
                anyhow::bail!("mapping did not remain in retry backoff")
            };
            drop(forward);
            let count = accepted.load(Ordering::SeqCst);
            let _server = ssh_server(port, accepted.clone()).await?;
            tokio::time::sleep(delay + Duration::from_millis(250)).await;
            ensure!(
                accepted.load(Ordering::SeqCst) == count,
                "manual stop resurrected the mapping"
            );
            let _released = TcpListener::bind(("127.0.0.1", forwarded)).await?;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await?
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn occupied_local_port_recovers_after_release_without_restarting_the_application()
-> Result<()> {
    let directory = tempfile::tempdir()?;
    let accepted = Arc::new(AtomicUsize::new(0));
    let port = free_port().await?;
    let _server = ssh_server(port, accepted.clone()).await?;
    let (echo, _echo) = echo_server().await?;
    let blocker = TcpListener::bind("127.0.0.1:0").await?;
    let forwarded = blocker.local_addr()?.port();
    let mut host = tomato_core::Connection::new("SSH");
    host.set("host", "127.0.0.1");
    host.set("port", port);
    host.set("username", "fixture");
    host.set("password", "fixture-only");
    host.set("usePassword", true);
    let mapping = Mapping {
        ssh_host_id: host.id.clone(),
        local_port: forwarded,
        remote_port: echo,
        ..Default::default()
    };
    let forward = Forward::start(mapping, host, directory.path().join("known"), || {})?;
    wait(&forward, false).await?;
    ensure!(
        accepted.load(Ordering::SeqCst) == 0,
        "busy local port should fail before SSH authentication"
    );
    drop(blocker);
    wait(&forward, true).await?;
    payload(forwarded).await?;
    drop(forward);
    Ok(())
}
