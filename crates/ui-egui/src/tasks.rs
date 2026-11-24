//! Observer-style bounded delivery keeps protocol and disk work outside UI frames.
mod queue;
pub use queue::{Completion, TaskQueue};
#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use tomato_services::commands::{Command, Output};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_and_repeated_disconnects_complete_and_reopened_actor_keeps_its_new_session()
    -> anyhow::Result<()> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let mut queue = TaskQueue::new(&runtime, egui::Context::default());
        let connection = tomato_core::Connection::new("SQLITE");
        let absent = queue.submit(Command::Disconnect {
            id: connection.id.clone(),
        })?;
        let create = queue.submit(Command::Sql {
            connection: connection.clone(),
            sql: "CREATE TABLE old_session(id INTEGER)".into(),
        })?;
        let first = queue.submit(Command::Disconnect {
            id: connection.id.clone(),
        })?;
        let repeated = queue.submit(Command::Disconnect {
            id: connection.id.clone(),
        })?;
        let reopen = queue.submit(Command::Sql {
            connection: connection.clone(),
            sql: "CREATE TABLE new_session(value TEXT); INSERT INTO new_session VALUES('fresh')"
                .into(),
        })?;
        let read=queue.submit(Command::Sql{connection,sql:"SELECT value,(SELECT count(*) FROM sqlite_master WHERE name='old_session') FROM new_session".into()})?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut results = HashMap::new();
        while results.len() < 6 && std::time::Instant::now() < deadline {
            for item in queue.poll() {
                results.insert(item.id, item.result?);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        for id in [absent, create, first, repeated, reopen, read] {
            assert!(
                results.contains_key(&id),
                "admitted job {id} did not complete"
            );
        }
        assert!(
            matches!(results.get(&read),Some(Output::Queries(values))if values[0].rows[0]==["fresh","0"])
        );
        Ok(())
    }
    #[test]
    fn admission_bounds_the_total_number_of_dispatched_tasks() -> anyhow::Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let mut queue = TaskQueue::new(&runtime, egui::Context::default());
        for _ in 0..64 {
            queue.submit(Command::Json {
                text: "{}".into(),
                action: "minify".into(),
            })?;
        }
        assert!(
            queue
                .submit(Command::Json {
                    text: "{}".into(),
                    action: "minify".into()
                })
                .is_err()
        );
        Ok(())
    }
    #[test]
    fn admission_bounds_total_payload_and_rejection_keeps_the_queue_usable() -> anyhow::Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let mut queue = TaskQueue::new(&runtime, egui::Context::default());
        assert!(
            queue
                .submit(Command::Json {
                    text: String::with_capacity(32 * 1024 * 1024 + 1),
                    action: "format".into()
                })
                .is_err()
        );
        queue.submit(Command::Json {
            text: String::with_capacity(32 * 1024 * 1024),
            action: "format".into(),
        })?;
        assert!(
            queue
                .submit(Command::Json {
                    text: "{}".into(),
                    action: "format".into()
                })
                .is_err()
        );
        Ok(())
    }
    #[test]
    fn connection_metadata_and_remote_write_payload_share_the_queue_budget() -> anyhow::Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let mut queue = TaskQueue::new(&runtime, egui::Context::default());
        let mut connection = tomato_core::Connection::new("LOCAL_DIRECTORY");
        connection.set(
            "futurePayload",
            serde_json::json!({"nested":"x".repeat(32*1024*1024)}),
        );
        assert!(
            queue
                .submit(Command::ObjectTree {
                    connection,
                    action: tomato_services::files::object_tree::Request::List("/".into())
                })
                .is_err()
        );
        assert!(
            queue
                .submit(Command::ObjectTree {
                    connection: tomato_core::Connection::new("LOCAL_DIRECTORY"),
                    action: tomato_services::files::object_tree::Request::Write {
                        path: "/a.md".into(),
                        text: String::with_capacity(32 * 1024 * 1024 + 1)
                    }
                })
                .is_err()
        );
        queue.submit(Command::Json {
            text: "{}".into(),
            action: "format".into(),
        })?;
        Ok(())
    }
    #[test]
    fn database_actor_keeps_transaction_state_and_delivers_other_jobs() -> anyhow::Result<()> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let mut queue = TaskQueue::new(&runtime, egui::Context::default());
        let connection = tomato_core::Connection::new("SQLITE");
        queue.submit(Command::Sql {
            connection: connection.clone(),
            sql: "CREATE TABLE t(value TEXT); INSERT INTO t VALUES('same-session');".into(),
        })?;
        let select = queue.submit(Command::Sql {
            connection,
            sql: "SELECT value FROM t".into(),
        })?;
        let json = queue.submit(Command::Json {
            text: "{\"v\": 1}".into(),
            action: "minify".into(),
        })?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut results = HashMap::new();
        while results.len() < 3 && std::time::Instant::now() < deadline {
            for item in queue.poll() {
                results.insert(item.id, item.result?);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            matches!(results.get(&select),Some(Output::Queries(results)) if results[0].rows[0][0] == "same-session")
        );
        assert!(matches!(results.get(&json),Some(Output::Text(text)) if text == "{\"v\":1}"));
        Ok(())
    }
}
