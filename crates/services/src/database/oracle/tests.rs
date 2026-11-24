use super::*;
#[test]
fn native_transport_types_remain_recoverable_without_replaying_sql_errors() {
    for kind in [
        std::io::ErrorKind::TimedOut,
        std::io::ErrorKind::ConnectionReset,
        std::io::ErrorKind::ConnectionAborted,
        std::io::ErrorKind::BrokenPipe,
    ] {
        let error = driver_error(oracledb::Error::from(std::io::Error::new(
            kind,
            "owned transport fixture",
        )));
        assert!(super::super::recovery::transport(&error.into()));
        let lob = failure::lob_error(std::io::Error::other(oracledb::Error::from(
            std::io::Error::new(kind, "owned LOB fixture"),
        )));
        assert!(super::super::recovery::transport(&lob.into()));
    }
    assert!(super::super::recovery::transport(&closed().into()));
    let invalid: oracledb::Error = "invalid-number"
        .parse::<oracledb::OracleNumber>()
        .unwrap_err();
    assert!(!super::super::recovery::transport(
        &driver_error(invalid).into()
    ));
}

#[tokio::test]
async fn oracle_peer_eof_retries_only_catalog_reads_and_reports_final_failure() -> anyhow::Result<()>
{
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let count = Arc::new(AtomicUsize::new(0));
    let accepts = count.clone();
    let server = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            accepts.fetch_add(1, Ordering::SeqCst);
            drop(stream);
        }
    });
    let mut c = Connection::new("ORACLE");
    c.set("host", "127.0.0.1");
    c.set("port", port);
    c.set("database", "/owned-fixture");
    c.set("username", "fixture");
    c.set("password", "fixture");
    let mut db = super::super::Databases::default();
    let result = tokio::time::timeout(
        Duration::from_secs(8),
        db.workbench(&c, tomato_core::workbench::DatabaseAction::Schemas),
    )
    .await?;
    server.abort();
    let error = result.unwrap_err();
    let failure = error
        .downcast_ref::<super::super::recovery::ConnectionFailure>()
        .expect("typed read failure after three attempts");
    assert_eq!(failure.attempts, 3);
    assert!(failure.read_only);
    assert_eq!(count.load(Ordering::SeqCst), 3);
    Ok(())
}
#[test]
fn native_oracle_prepared_execution_removes_sql_terminators_but_preserves_plsql_units()
-> anyhow::Result<()> {
    assert_eq!(
        one_statement("CREATE TABLE \"样例\" (\"id\" NUMBER);")?,
        "CREATE TABLE \"样例\" (\"id\" NUMBER)"
    );
    assert_eq!(
        one_statement("SELECT ';' FROM dual;")?,
        "SELECT ';' FROM dual"
    );
    let block = "BEGIN DBMS_METADATA.CLOSE(:1); END;";
    assert_eq!(one_statement(block)?, block);
    assert!(one_statement("SELECT 1 FROM dual; DROP TABLE items").is_err());
    Ok(())
}
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

#[test]
fn descriptor_keeps_physical_service_and_rejects_descriptor_injection() -> anyhow::Result<()> {
    let mut c = Connection::new("ORACLE");
    c.set("host", "127.0.0.1");
    c.set("port", 1521);
    c.set("database", "/销售.example");
    c.set("username", "secret-user");
    c.set("password", "secret-password");
    c.set("__tomatoOracleOwner", "OTHER_OWNER");
    let value = descriptor(&c)?;
    assert!(value.contains("(SERVICE_NAME=销售.example)"));
    assert!(!value.contains("OTHER_OWNER") && !value.contains("secret-"));
    c.set("database", "ORCL");
    assert!(descriptor(&c)?.contains("(SID=ORCL)"));
    for invalid in ["", "/", "ORCL)(HOST=evil)", "ORCL\n"] {
        c.set("database", invalid);
        assert!(descriptor(&c).is_err());
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actor_rejects_malformed_tns_header_and_closes_owned_socket() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let server = std::thread::spawn(move || -> std::io::Result<()> {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let (mut socket, _) = loop {
            match listener.accept() {
                Ok(pair) => break pair,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Err(e) => return Err(e),
            }
        };
        socket.set_nonblocking(false)?;
        socket.set_read_timeout(Some(Duration::from_secs(3)))?;
        let mut buffer = [0; 8192];
        socket.read_exact(&mut buffer[..8])?;
        let length = u16::from_be_bytes([buffer[0], buffer[1]]) as usize;
        assert!((8..=8192).contains(&length));
        socket.read_exact(&mut buffer[8..length])?;
        socket.write_all(&[0, 0, 0, 0, 6, 0, 0, 0])?;
        // Long connect data may already be pipelined in a second TNS packet.
        // Drain that bounded initial request before checking peer shutdown.
        let mut trailing = 0usize;
        loop {
            match socket.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    trailing += n;
                    assert!(trailing <= 16384);
                }
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    });
    let mut c = Connection::new("ORACLE");
    c.set("host", "127.0.0.1");
    c.set("port", port);
    c.set("database", "ORCL");
    c.set("username", "fixture_user");
    c.set("password", "fixture_password");
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        super::super::Databases::default().query(&c, "SELECT 1 FROM dual"),
    )
    .await?;
    server
        .join()
        .map_err(|_| anyhow::anyhow!("fixture panicked"))??;
    let failure = result
        .err()
        .ok_or_else(|| anyhow::anyhow!("malformed handshake unexpectedly succeeded"))?;
    anyhow::ensure!(
        failure.to_string().contains("invalid Oracle packet length"),
        "{failure:#}"
    );
    Ok(())
}
