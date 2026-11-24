use super::*;
#[test]
fn oracle_anonymous_units_do_not_create_phantom_transactions() {
    for sql in [
        "BEGIN NULL; END;",
        "/*unit*/ BEGIN DBMS_SESSION.SLEEP(35); END;",
        "BEGIN /* body */ WORK; END;",
        "DECLARE x NUMBER; BEGIN x:=1; END;",
    ] {
        assert_eq!(transaction_change_for("ORACLE", sql), None);
    }
    for sql in ["BEGIN", "BEGIN;", "/*control*/ BEGIN --end\n"] {
        assert_eq!(transaction_change_for("ORACLE", sql), Some(true));
    }
    assert_eq!(
        transaction_change_for("POSTGRESQL", "BEGIN ISOLATION LEVEL REPEATABLE READ"),
        Some(true)
    );
    assert_eq!(
        transaction_change_for("MYSQL", "START TRANSACTION"),
        Some(true)
    );
    assert_eq!(transaction_change_for("ORACLE", "COMMIT"), Some(false));
}
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
#[test]
fn typed_transport_failures_include_windows_aborted_socket_and_preserve_sql_errors() {
    for kind in [
        io::ErrorKind::ConnectionAborted,
        io::ErrorKind::ConnectionReset,
        io::ErrorKind::BrokenPipe,
        io::ErrorKind::UnexpectedEof,
        io::ErrorKind::TimedOut,
    ] {
        let error = anyhow::Error::new(sqlx::Error::Io(io::Error::new(kind, "fixture")))
            .context("database request");
        assert!(transport(&error));
    }
    assert!(transport(&anyhow::Error::new(
        io::Error::from_raw_os_error(10053)
    )));
    assert!(!transport(&anyhow::Error::new(sqlx::Error::RowNotFound)));
    assert!(!transport(&anyhow::anyhow!(
        "SQL contains 10053 but no socket error"
    )));
    assert!(!transport(&anyhow::Error::new(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "fixture"
    ))));
}
#[test]
fn only_typed_reads_are_replayed_and_transaction_heads_ignore_leading_comments() {
    let table = tomato_core::workbench::TableId {
        schema: "fixture".into(),
        name: "items".into(),
    };
    assert!(read_only(&DatabaseAction::DesignStructure {
        table: table.clone()
    }));
    assert!(read_only(&DatabaseAction::DesignSql { table }));
    assert!(read_only(&DatabaseAction::Schemas));
    assert!(read_only(&DatabaseAction::Tables {
        schema: "fixture".into()
    }));
    assert!(!read_only(&DatabaseAction::Transaction {
        action: tomato_core::workbench::TransactionAction::Commit
    }));
    assert!(!read_only(&DatabaseAction::DeleteRows {
        table: tomato_core::workbench::TableId {
            schema: "fixture".into(),
            name: "fixture".into()
        },
        rows: vec![]
    }));
    assert_eq!(
        transaction_change("/* fixture */ -- test\n START TRANSACTION"),
        Some(true)
    );
    assert_eq!(transaction_change("SELECT 'BEGIN'"), None);
    assert_eq!(
        transaction_change("ROLLBACK\n/* fixture */ TO SAVEPOINT original"),
        None
    );
    assert_eq!(
        transaction_change("COMMIT AND CHAIN /* no comment */"),
        Some(true)
    );
    assert_eq!(transaction_change("COMMIT AND NO CHAIN"), Some(false));
    assert_eq!(transaction_change("START REPLICA"), None);
    assert_eq!(
        transaction_change("/* outer /* inner */ remains */ BEGIN"),
        Some(true)
    );
}
#[tokio::test]
async fn connection_abort_retries_reads_three_times_but_never_writes_or_lost_transactions()
-> Result<()> {
    let timeout = tokio::time::timeout(Duration::ZERO, std::future::pending::<()>())
        .await
        .unwrap_err();
    assert!(transport(&anyhow::Error::new(timeout)));
    for (write, transaction) in [(false, false), (true, false), (false, true)] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let count = Arc::new(AtomicUsize::new(0));
        let accepted = count.clone();
        let server = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                accepted.fetch_add(1, Ordering::SeqCst);
                drop(stream);
            }
        });
        let mut connection = Connection::new("MYSQL");
        connection.set("host", "127.0.0.1");
        connection.set("port", port);
        connection.set("username", "fixture");
        let mut db = Databases::default();
        if transaction {
            db.transactions.insert(SessionKey {
                connection: connection.id.clone(),
                database: String::new(),
            });
        }
        let action = if write {
            DatabaseAction::Transaction {
                action: tomato_core::workbench::TransactionAction::Begin,
            }
        } else {
            DatabaseAction::Schemas
        };
        let result =
            tokio::time::timeout(Duration::from_secs(5), db.workbench(&connection, action)).await?;
        server.abort();
        let error = result.unwrap_err();
        let failure = error
            .downcast_ref::<ConnectionFailure>()
            .expect("typed final connection failure");
        let expected = if write || transaction { 1 } else { 3 };
        assert_eq!(failure.attempts, expected);
        assert_eq!(count.load(Ordering::SeqCst), expected);
        assert_eq!(failure.transaction_lost, transaction);
        assert!(db.sessions.is_empty() && db.tunnels.is_empty() && db.transactions.is_empty());
    }
    Ok(())
}

#[test]
fn ssh_transport_events_retry_but_key_and_protocol_failures_do_not() {
    for error in [
        russh::Error::Disconnect,
        russh::Error::HUP,
        russh::Error::ConnectionTimeout,
        russh::Error::KeepaliveTimeout,
        russh::Error::InactivityTimeout,
        russh::Error::SendError,
        russh::Error::IO(io::Error::new(io::ErrorKind::ConnectionReset, "fixture")),
    ] {
        assert!(transport(&anyhow::Error::new(error)));
    }
    for error in [
        russh::Error::UnknownKey,
        russh::Error::Version,
        russh::Error::IO(io::Error::new(io::ErrorKind::PermissionDenied, "fixture")),
    ] {
        assert!(!transport(&anyhow::Error::new(error)));
    }
}
