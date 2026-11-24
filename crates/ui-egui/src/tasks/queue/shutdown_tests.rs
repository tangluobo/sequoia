use super::*;
use anyhow::{Result, ensure};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;

fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?)
}
#[test]
fn shutdown_before_first_dispatch_prevents_queued_file_mutation() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let directory = tempfile::tempdir()?;
    let file = directory.path().join("must-not-start.sqlite");
    let mut queue = TaskQueue::new(&runtime, egui::Context::default());
    let mut connection = tomato_core::Connection::new("SQLITE");
    connection.set("database", file.to_string_lossy().as_ref());
    queue.submit(Command::Sql {
        connection,
        sql: "CREATE TABLE should_not_run(id INTEGER)".into(),
    })?;
    let shutdown = queue.shutdown();
    runtime
        .block_on(async { tokio::time::timeout(Duration::from_secs(3), shutdown.wait()).await })?;
    assert!(!file.exists());
    assert_eq!(queue.slots.available_permits(), 64);
    Ok(())
}
#[test]
fn shutdown_cancels_blocked_deliveries_releases_admission_and_is_idempotent() -> Result<()> {
    let runtime = runtime()?;
    let mut queue = TaskQueue::new(&runtime, egui::Context::default());
    let c = tomato_core::Connection::new("SQLITE");
    for _ in 0..32 {
        queue.submit(Command::Sql {
            connection: c.clone(),
            sql: "SELECT 1".into(),
        })?;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while queue.receiver.len() < 8 {
        ensure!(Instant::now() < deadline, "Completion buffer did not fill");
        std::thread::sleep(Duration::from_millis(5));
    }
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(3), queue.shutdown().wait()).await
    })?;
    assert_eq!(queue.slots.available_permits(), 64);
    assert_eq!(queue.budget.available_permits(), 32 * 1024);
    assert!(
        queue
            .submit(Command::Sql {
                connection: c,
                sql: "SELECT 2".into()
            })
            .unwrap_err()
            .to_string()
            .contains("关闭")
    );
    runtime.block_on(queue.shutdown().wait());
    Ok(())
}
#[test]
fn shutdown_cancels_an_actual_stalled_mysql_handshake_and_closes_its_socket() -> Result<()> {
    let runtime = runtime()?;
    let listener = runtime.block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))?;
    let address = listener.local_addr()?;
    let (ready, accepted) = tokio::sync::oneshot::channel();
    let peer = runtime.spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let _ = ready.send(());
        let result =
            tokio::time::timeout(Duration::from_secs(5), socket.read(&mut [0u8; 1])).await?;
        match result {
            Ok(0) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::BrokenPipe
                ) => {}
            other => anyhow::bail!("Stalled handshake socket was not closed: {other:?}"),
        }
        Ok::<_, anyhow::Error>(())
    });
    let mut queue = TaskQueue::new(&runtime, egui::Context::default());
    let mut c = tomato_core::Connection::new("MYSQL");
    c.set("host", "127.0.0.1");
    c.set("port", address.port());
    c.set("username", "owned");
    c.set("database", "owned");
    queue.submit(Command::Sql {
        connection: c,
        sql: "SELECT 1".into(),
    })?;
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), accepted).await??;
        let started = Instant::now();
        tokio::time::timeout(Duration::from_secs(3), queue.shutdown().wait()).await?;
        ensure!(
            started.elapsed() < Duration::from_millis(2500),
            "Shutdown waited for the normal connection timeout"
        );
        peer.await??;
        Ok::<_, anyhow::Error>(())
    })?;
    Ok(())
}
#[test]
fn embedded_markdown_image_reserves_its_payload_in_the_queue() {
    let document = std::path::PathBuf::from("input.md");
    let source = String::with_capacity(6 * 1024 * 1024);
    let expected = (document.capacity() + source.capacity()).div_ceil(1024) as u32;
    assert_eq!(
        queued_units(&Command::MarkdownImage { document, source }),
        expected
    );
}
