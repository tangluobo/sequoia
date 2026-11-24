//! Disconnect replies are barriers even while another close is already queued.
use super::*;
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, time::Duration};

#[test]
fn repeated_disconnect_waits_for_its_actor_without_blocking_other_connections() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let (accepted, ready) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let peer = tokio::spawn(async move {
            let (socket, _) = listener.accept().await?;
            let _ = accepted.send(());
            let _ = released.await;
            drop(socket);
            Ok::<_, anyhow::Error>(())
        });
        let mut queue = TaskQueue::new(&runtime, egui::Context::default());
        let mut blocked = tomato_core::Connection::new("MYSQL");
        blocked.set("host", "127.0.0.1");
        blocked.set("port", port);
        blocked.set("username", "owned-fixture");
        blocked.set("database", "owned-fixture");
        let request = queue.submit(Command::Sql {
            connection: blocked.clone(),
            sql: "SELECT 1".into(),
        })?;
        tokio::time::timeout(Duration::from_secs(5), ready).await??;
        let first = queue.submit(Command::Disconnect {
            id: blocked.id.clone(),
        })?;
        let second = queue.submit(Command::Disconnect {
            id: blocked.id.clone(),
        })?;
        let unrelated = queue.submit(Command::Sql {
            connection: tomato_core::Connection::new("SQLITE"),
            sql: "SELECT 7".into(),
        })?;
        let mut replies = BTreeMap::new();
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while !replies.contains_key(&unrelated) {
            for reply in queue.poll() {
                ensure!(
                    reply.id != first && reply.id != second,
                    "disconnect acknowledged while its actor still owns the blocked socket"
                );
                replies.insert(reply.id, reply.result);
            }
            ensure!(
                std::time::Instant::now() < until,
                "a connection close blocked an unrelated connection"
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        replies.remove(&unrelated).unwrap()?;
        let _ = release.send(());
        peer.await??;
        let until = std::time::Instant::now() + Duration::from_secs(5);
        let mut order = Vec::new();
        while replies.len() < 3 {
            for reply in queue.poll() {
                order.push(reply.id);
                replies.insert(reply.id, reply.result);
            }
            ensure!(
                std::time::Instant::now() < until,
                "disconnect replies were lost"
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(order, [request, first, second]);
        assert!(replies.remove(&request).unwrap().is_err());
        replies.remove(&first).unwrap()?;
        replies.remove(&second).unwrap()?;
        tokio::time::timeout(Duration::from_secs(3), queue.shutdown().wait()).await?;
        assert_eq!(queue.slots.available_permits(), 64);
        assert_eq!(queue.budget.available_permits(), 32 * 1024);
        Ok::<_, anyhow::Error>(())
    })
}

#[test]
fn queued_sql_after_disconnect_keeps_its_new_session_and_statement_order() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let mut queue = TaskQueue::new(&runtime, egui::Context::default());
        let connection = tomato_core::Connection::new("SQLITE");
        for _ in 0..8 {
            let before = queue.submit(Command::Sql {
                connection: connection.clone(),
                sql: "CREATE TABLE old_session(id INTEGER)".into(),
            })?;
            let close = queue.submit(Command::Disconnect {
                id: connection.id.clone(),
            })?;
            let empty = queue.submit(Command::Sql {
                connection: connection.clone(),
                sql: "SELECT COUNT(*) FROM sqlite_master WHERE name='old_session'".into(),
            })?;
            let create = queue.submit(Command::Sql {
                connection: connection.clone(),
                sql: "CREATE TABLE live(id INTEGER); INSERT INTO live VALUES(42)".into(),
            })?;
            let read = queue.submit(Command::Sql {
                connection: connection.clone(),
                sql: "SELECT id FROM live".into(),
            })?;
            let until = std::time::Instant::now() + Duration::from_secs(5);
            let mut replies = BTreeMap::new();
            let mut order = Vec::new();
            while replies.len() < 5 {
                for reply in queue.poll() {
                    order.push(reply.id);
                    replies.insert(reply.id, reply.result);
                }
                ensure!(
                    std::time::Instant::now() < until,
                    "reopened session timed out"
                );
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            assert_eq!(order, [before, close, empty, create, read]);
            replies.remove(&before).unwrap()?;
            replies.remove(&close).unwrap()?;
            let Output::Queries(rows) = replies.remove(&empty).unwrap()? else {
                anyhow::bail!("expected the reopened session query");
            };
            assert_eq!(rows[0].rows[0][0], "0");
            replies.remove(&create).unwrap()?;
            let Output::Queries(rows) = replies.remove(&read).unwrap()? else {
                anyhow::bail!("expected the new session readback");
            };
            assert_eq!(rows[0].rows[0][0], "42");
        }
        tokio::time::timeout(Duration::from_secs(3), queue.shutdown().wait()).await?;
        assert_eq!(queue.slots.available_permits(), 64);
        Ok::<_, anyhow::Error>(())
    })
}
