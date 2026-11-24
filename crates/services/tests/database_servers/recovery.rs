//! Actual MySQL/PostgreSQL sessions over disposable TCP and native SSH transports.
use super::*;
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::watch,
    task::{JoinHandle, JoinSet},
};

struct Proxy {
    address: SocketAddr,
    reset: watch::Sender<u64>,
    count: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Proxy {
    async fn start(target: SocketAddr) -> Result<Self> {
        Self::with_failures(target, 0).await
    }
    async fn with_failures(target: SocketAddr, failures: usize) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (reset, watch) = watch::channel(0u64);
        let count = Arc::new(AtomicUsize::new(0));
        let accepted = count.clone();
        let task = tokio::spawn(async move {
            let mut jobs = JoinSet::new();
            loop {
                tokio::select! {
                    socket=listener.accept()=>{let Ok((mut local,_))=socket else{break};let index=accepted.fetch_add(1,Ordering::SeqCst);if index<failures {drop(local);continue;}let mut stop=watch.clone();stop.borrow_and_update();jobs.spawn(async move {if let Ok(mut remote)=TcpStream::connect(target).await {tokio::select! {_=tokio::io::copy_bidirectional(&mut local,&mut remote)=>{},_=stop.changed()=>{}}}});},
                    _=jobs.join_next(),if !jobs.is_empty()=>{}
                }
            }
        });
        Ok(Self {
            address,
            reset,
            count,
            task,
        })
    }
    fn cut(&self) {
        self.reset.send_modify(|generation| *generation += 1);
    }
}
struct Jump {
    port: u16,
    task: JoinHandle<()>,
}
impl Drop for Jump {
    fn drop(&mut self) {
        self.task.abort();
    }
}
struct Forward {
    target: SocketAddr,
    jobs: JoinSet<()>,
    authentications: Arc<AtomicUsize>,
    auth_limit: usize,
}
impl russh::server::Handler for Forward {
    type Error = anyhow::Error;
    async fn auth_password(&mut self, user: &str, password: &str) -> Result<russh::server::Auth> {
        let allowed = self.authentications.fetch_add(1, Ordering::SeqCst) < self.auth_limit;
        Ok(
            if allowed && user == "fixture" && password == "fixture-only" {
                russh::server::Auth::Accept
            } else {
                russh::server::Auth::reject()
            },
        )
    }
    #[allow(clippy::too_many_arguments)]
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: russh::Channel<russh::server::Msg>,
        host: &str,
        port: u32,
        _origin: &str,
        _origin_port: u32,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut russh::server::Session,
    ) -> Result<()> {
        anyhow::ensure!(
            host == "127.0.0.1" && port == u32::from(self.target.port()),
            "Only the owned database endpoint is permitted"
        );
        let mut remote = TcpStream::connect(self.target).await?;
        reply.accept().await;
        self.jobs.spawn(async move {
            let mut local = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut local, &mut remote).await;
        });
        Ok(())
    }
}
impl Jump {
    async fn start(target: SocketAddr) -> Result<Self> {
        Self::with_auth_limit(target, usize::MAX).await
    }
    async fn with_auth_limit(target: SocketAddr, auth_limit: usize) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let config = Arc::new(russh::server::Config {
            keys: vec![russh::keys::PrivateKey::from(
                russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[104; 32]),
            )],
            auth_rejection_time: std::time::Duration::ZERO,
            auth_rejection_time_initial: Some(std::time::Duration::ZERO),
            ..Default::default()
        });
        let authentications = Arc::new(AtomicUsize::new(0));
        let task = tokio::spawn(async move {
            let mut jobs = JoinSet::new();
            loop {
                tokio::select! {
                    socket=listener.accept()=>{let Ok((socket,_))=socket else{break};let config=config.clone();let authentications=authentications.clone();jobs.spawn(async move {if let Ok(session)=russh::server::run_stream(config,socket,Forward {target,jobs:JoinSet::new(),authentications,auth_limit}).await {let _=session.await;}});},
                    _=jobs.join_next(),if !jobs.is_empty()=>{}
                }
            }
        });
        Ok(Self { port, task })
    }
}
async fn verify(kind: &str, variable: &str) -> Result<()> {
    let uri = url::Url::parse(&std::env::var(variable)?)?;
    anyhow::ensure!(
        uri.host_str() == Some("127.0.0.1"),
        "Owned loopback database required"
    );
    let target = SocketAddr::from(([127, 0, 0, 1], uri.port().context("port")?));
    for ssh in [false, true] {
        let directory = tempfile::tempdir()?;
        let jump = Jump::start(target).await?;
        let proxy = Proxy::start(if ssh {
            SocketAddr::from(([127, 0, 0, 1], jump.port))
        } else {
            target
        })
        .await?;
        let mut connection = Connection::new(kind);
        connection.set("host", "127.0.0.1");
        connection.set(
            "port",
            if ssh {
                target.port()
            } else {
                proxy.address.port()
            },
        );
        connection.set("username", uri.username());
        connection.set("password", uri.password().unwrap_or_default());
        connection.set("database", uri.path().trim_start_matches('/'));
        if ssh {
            connection.set("useSshTunnel", true);
            connection.set("sshTunnelHost", "127.0.0.1");
            connection.set("sshTunnelPort", proxy.address.port());
            connection.set("sshTunnelUsername", "fixture");
            connection.set("sshTunnelPassword", "fixture-only");
        }
        let original = serde_json::to_value(&connection)?;
        let mut db = Databases::with_ssh_configuration(directory.path().join("connections.json"));
        let name = format!("tomato_recovery_{}", uuid::Uuid::new_v4().simple());
        db.query(&connection,&format!("CREATE TABLE {name} (id INTEGER PRIMARY KEY, value VARCHAR(64)); INSERT INTO {name} VALUES (1,'retained')")).await?;
        let action = DatabaseAction::Page {
            request: PageRequest {
                table: TableId {
                    schema: if kind == "MYSQL" {
                        connection.text("database").into()
                    } else {
                        "public".into()
                    },
                    name: name.clone(),
                },
                offset: 0,
                limit: 100,
                sort: None,
                query: Default::default(),
            },
        };
        let first = db.workbench(&connection, action.clone()).await?;
        proxy.cut();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let recovered = db.workbench(&connection, action.clone()).await?;
        let (DatabaseOutput::Page(first), DatabaseOutput::Page(recovered)) = (first, recovered)
        else {
            anyhow::bail!("Expected pages")
        };
        assert_eq!(first.rows, recovered.rows);
        assert_eq!(first.total, recovered.total);
        assert_eq!(first.sql, recovered.sql);
        assert!(
            proxy.count.load(Ordering::SeqCst) >= 2,
            "No new database/SSH transport was opened"
        );
        // Raw metadata and result-save paths must share transport recovery too.
        let result = db
            .query(&connection, &format!("SELECT id,value FROM {name}"))
            .await?
            .remove(0);
        let before = proxy.count.load(Ordering::SeqCst);
        proxy.cut();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let error = db
            .query_editing(&connection, &name, &result)
            .await
            .unwrap_err();
        let failure = error
            .downcast_ref::<tomato_services::database::ConnectionFailure>()
            .context("Editing metadata did not normalize the transport failure")?;
        assert_eq!(failure.is_restored(), ssh);
        assert_eq!(
            proxy.count.load(Ordering::SeqCst),
            before + usize::from(ssh)
        );
        let editing = db
            .query_editing(&connection, &name, &result)
            .await?
            .context("Expected editable result")?;
        let before = proxy.count.load(Ordering::SeqCst);
        proxy.cut();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let error = db
            .save_query_rows(
                &connection,
                editing,
                vec![tomato_services::database::query_rows::Change {
                    original: vec![Some("1".into()), Some("retained".into())],
                    modified: std::collections::BTreeMap::from([(1, Some("must not run".into()))]),
                    delete: false,
                }],
            )
            .await
            .unwrap_err();
        let failure = error
            .downcast_ref::<tomato_services::database::ConnectionFailure>()
            .context("Result save did not normalize the transport failure")?;
        assert_eq!(failure.is_restored(), ssh);
        assert_eq!(
            proxy.count.load(Ordering::SeqCst),
            before + usize::from(ssh)
        );
        let unchanged = db
            .query(&connection, &format!("SELECT value FROM {name}"))
            .await?;
        assert_eq!(
            unchanged[0].rows[0][0], "retained",
            "Result edits were silently replayed"
        );
        db.workbench(
            &connection,
            DatabaseAction::Transaction {
                action: TransactionAction::Begin,
            },
        )
        .await?;
        db.query(
            &connection,
            &format!("INSERT INTO {name} VALUES (2,'uncommitted')"),
        )
        .await?;
        if !ssh {
            // A new namespace can fail without invalidating a healthy sibling transaction.
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let closed_port = listener.local_addr()?.port();
            drop(listener);
            let mut other = connection.clone();
            other.set("database", format!("{name}_missing"));
            other.set("port", closed_port);
            let error = db.query(&other, "SELECT 'not sent'").await.unwrap_err();
            let failure = error
                .downcast_ref::<tomato_services::database::ConnectionFailure>()
                .context("Missing opening failure")?;
            assert_eq!(
                failure.recovery,
                tomato_services::database::RecoveryState::Preserved
            );
            assert!(!failure.transaction_lost);
            let values = db
                .query(&connection, &format!("SELECT COUNT(*) FROM {name}"))
                .await?;
            assert_eq!(
                values[0].rows[0][0], "2",
                "Opening a new namespace discarded the healthy transaction"
            );
        }
        let before = proxy.count.load(Ordering::SeqCst);
        proxy.cut();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let error = db.workbench(&connection, action.clone()).await.unwrap_err();
        let failure = error
            .downcast_ref::<tomato_services::database::ConnectionFailure>()
            .context("Expected transaction loss")?;
        assert!(failure.transaction_lost);
        assert_eq!(failure.attempts, 1);
        assert_eq!(
            proxy.count.load(Ordering::SeqCst),
            before,
            "Transaction was silently retried"
        );
        let DatabaseOutput::Page(page) = db.workbench(&connection, action).await? else {
            anyhow::bail!("Expected recovered page")
        };
        assert_eq!(page.total, 1, "Lost transaction was replayed or committed");
        let terminate = if kind == "MYSQL" {
            let ids = db.query(&connection, "SELECT CONNECTION_ID()").await?;
            format!("KILL {}", ids[0].rows[0][0].parse::<u64>()?)
        } else {
            "SELECT pg_terminate_backend(pg_backend_pid())".into()
        };
        let before = proxy.count.load(Ordering::SeqCst);
        let source = format!(
            "SELECT '保留中文结果'; {terminate}; INSERT INTO {name} VALUES (3,'must not run'); INSERT INTO {name} VALUES (4,'must not reconnect')"
        );
        let error = db
            .script(
                &connection,
                source.clone(),
                tomato_services::database::script::Mode::Run,
            )
            .await
            .unwrap_err();
        let interrupted = error
            .downcast_ref::<tomato_services::database::script::Interrupted>()
            .context("Partial script results were lost")?;
        let retained = &interrupted.script;
        assert_eq!(retained.source, source);
        // MySQL's KILL acknowledgement may race the socket close. Preserve
        // exactly what this client observed, without sending any later INSERT.
        let successful = retained.success_count;
        assert!(if kind == "MYSQL" {
            (1..=2).contains(&successful)
        } else {
            successful == 1
        });
        let failed_sql = retained.sql(&retained.statements.last().unwrap().range);
        assert!(
            failed_sql == terminate
                || (kind == "MYSQL"
                    && failed_sql == format!("INSERT INTO {name} VALUES (3,'must not run')"))
        );
        // A KILL may first report an ordinary query interruption; that does
        // not prove the connection is dead until the next transport operation.
        assert_eq!(
            retained.fail_count,
            retained.statements.len() as u64 - successful
        );
        assert!(
            if kind == "MYSQL" {
                (2..=3).contains(&retained.statements.len())
            } else {
                retained.statements.len() == 2
            },
            "Statements continued after the transport failure"
        );
        assert!(retained.statements[0].success && !retained.statements.last().unwrap().success);
        assert_eq!(retained.results[0].rows[0][0], "保留中文结果");
        let state = retained
            .interruption
            .as_ref()
            .context("Missing interruption state")?
            .connection
            .as_ref()
            .context("Missing transport failure")?;
        assert_eq!(state.is_restored(), ssh);
        assert_eq!(
            proxy.count.load(Ordering::SeqCst),
            before + usize::from(ssh),
            "Only one proactive SSH reconnect is permitted"
        );
        let values = db
            .query(&connection, &format!("SELECT COUNT(*) FROM {name}"))
            .await?;
        assert_eq!(values[0].rows[0][0], "1");
        // No unique key: an unsafe replay would create a second committed row.
        let once = format!("{name}_once");
        db.query(
            &connection,
            &format!("CREATE TABLE {once} (value VARCHAR(64))"),
        )
        .await?;
        let mut observer_connection = connection.clone();
        observer_connection.set("useSshTunnel", false);
        observer_connection.set("port", target.port());
        let mut observer =
            Databases::with_ssh_configuration(directory.path().join("observer.json"));
        let sleep = if kind == "MYSQL" {
            "SELECT SLEEP(20)"
        } else {
            "SELECT pg_sleep(20)"
        };
        let source =
            format!("BEGIN; INSERT INTO {once} VALUES ('committed exactly once'); COMMIT; {sleep}");
        let before = proxy.count.load(Ordering::SeqCst);
        let observe = async {
            let start = std::time::Instant::now();
            loop {
                let values = observer
                    .query(
                        &observer_connection,
                        &format!("SELECT COUNT(*) FROM {once}"),
                    )
                    .await?;
                if values[0].rows[0][0] == "1" {
                    proxy.cut();
                    return Ok::<_, anyhow::Error>(());
                }
                anyhow::ensure!(
                    start.elapsed() < std::time::Duration::from_secs(10),
                    "Commit was not observed before the owned cut"
                );
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        };
        let (result, observed) = tokio::join!(db.query(&connection, &source), observe);
        observed?;
        observer.disconnect(&observer_connection.id).await?;
        let error = result.unwrap_err();
        let failure = error
            .downcast_ref::<tomato_services::database::ConnectionFailure>()
            .context("Expected interrupted write")?;
        assert_eq!(failure.is_restored(), ssh);
        assert_eq!(
            proxy.count.load(Ordering::SeqCst),
            before + usize::from(ssh)
        );
        let values = db
            .query(&connection, &format!("SELECT COUNT(*) FROM {once}"))
            .await?;
        assert_eq!(values[0].rows[0][0], "1", "Committed user SQL was replayed");
        db.query(&connection, &format!("DROP TABLE {once}")).await?;
        db.query(&connection, &format!("DROP TABLE {name}")).await?;
        db.disconnect(&connection.id).await?;
        assert_eq!(serde_json::to_value(&connection)?, original);
        assert!(!directory.path().join("connections.json").exists());
    }
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires isolated loopback MySQL"]
async fn mysql_database_and_ssh_transport_recover_without_replaying_transactions() -> Result<()> {
    verify("MYSQL", "TOMATO_TEST_MYSQL_URL").await
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires isolated loopback PostgreSQL"]
async fn postgres_database_and_ssh_transport_recover_without_replaying_transactions() -> Result<()>
{
    verify("POSTGRESQL", "TOMATO_TEST_POSTGRES_URL").await
}

async fn verify_opening(kind: &str, variable: &str) -> Result<()> {
    use tomato_services::database::{ConnectionFailure, FailureStage};
    let uri = url::Url::parse(&std::env::var(variable)?)?;
    anyhow::ensure!(
        uri.host_str() == Some("127.0.0.1"),
        "Owned loopback database required"
    );
    let target = SocketAddr::from(([127, 0, 0, 1], uri.port().context("port")?));
    let jump = Jump::start(target).await?;
    for (drops, read, denied, script) in [
        (1, false, false, false),
        (6, false, false, false),
        (6, true, false, false),
        (0, false, true, false),
        (6, false, false, true),
    ] {
        let directory = tempfile::tempdir()?;
        let proxy =
            Proxy::with_failures(SocketAddr::from(([127, 0, 0, 1], jump.port)), drops).await?;
        let mut connection = Connection::new(kind);
        connection.set("host", "127.0.0.1");
        connection.set("port", target.port());
        connection.set("username", uri.username());
        connection.set("password", uri.password().unwrap_or_default());
        connection.set("database", uri.path().trim_start_matches('/'));
        connection.set("useSshTunnel", true);
        connection.set("sshTunnelHost", "127.0.0.1");
        connection.set("sshTunnelPort", proxy.address.port());
        connection.set("sshTunnelUsername", "fixture");
        connection.set(
            "sshTunnelPassword",
            if denied {
                "wrong-fixture-password"
            } else {
                "fixture-only"
            },
        );
        let original = serde_json::to_value(&connection)?;
        let mut db = Databases::with_ssh_configuration(directory.path().join("connections.json"));
        let name = format!("tomato_opening_{}", uuid::Uuid::new_v4().simple());
        if drops == 1 {
            // Opening retries happen before sending this user operation.
            db.query(
                &connection,
                &format!(
                    "CREATE TABLE {name} (value VARCHAR(64)); INSERT INTO {name} VALUES ('once')"
                ),
            )
            .await?;
            assert_eq!(proxy.count.load(Ordering::SeqCst), 2);
            let values = db
                .query(&connection, &format!("SELECT COUNT(*) FROM {name}"))
                .await?;
            assert_eq!(values[0].rows[0][0], "1");
            db.query(&connection, &format!("DROP TABLE {name}")).await?;
        } else {
            let error = if script {
                db.script(
                    &connection,
                    "SELECT 'never sent'; SELECT 'not attempted'".into(),
                    tomato_services::database::script::Mode::Run,
                )
                .await
                .unwrap_err()
            } else if read {
                db.workbench(&connection, DatabaseAction::Schemas)
                    .await
                    .unwrap_err()
            } else {
                db.query(&connection, "SELECT 'never sent'")
                    .await
                    .unwrap_err()
            };
            let expected = if denied {
                1
            } else if read {
                3
            } else {
                2
            };
            assert_eq!(
                proxy.count.load(Ordering::SeqCst),
                expected,
                "Opening retries multiplied or authentication was retried"
            );
            if !denied {
                let failure = error
                    .downcast_ref::<ConnectionFailure>()
                    .context("Expected typed opening failure")?;
                assert_eq!(failure.stage, FailureStage::Opening);
                assert_eq!(failure.attempts, expected);
                assert!(!failure.is_restored());
            } else {
                assert!(
                    error.downcast_ref::<ConnectionFailure>().is_none(),
                    "Authentication was classified as a transport failure"
                );
            }
        }
        db.disconnect(&connection.id).await?;
        assert_eq!(serde_json::to_value(&connection)?, original);
        assert!(!directory.path().join("connections.json").exists());
    }
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires isolated loopback MySQL"]
async fn mysql_ssh_initial_opening_is_bounded_without_replaying_sql() -> Result<()> {
    verify_opening("MYSQL", "TOMATO_TEST_MYSQL_URL").await
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires isolated loopback PostgreSQL"]
async fn postgres_ssh_initial_opening_is_bounded_without_replaying_sql() -> Result<()> {
    verify_opening("POSTGRESQL", "TOMATO_TEST_POSTGRES_URL").await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires isolated PostgreSQL and TOMATO_RECOVERY_GUI/CAPTURE_DIR"]
async fn capture_native_recovery_windows() -> Result<()> {
    let gui = std::path::PathBuf::from(
        std::env::var_os("TOMATO_RECOVERY_GUI").context("Missing owned GUI")?,
    );
    let directory = std::path::PathBuf::from(
        std::env::var_os("TOMATO_RECOVERY_CAPTURE_DIR")
            .context("Missing fresh capture directory")?,
    );
    anyhow::ensure!(!directory.exists(), "Use a fresh capture directory");
    std::fs::create_dir_all(&directory)?;
    let uri = url::Url::parse(&std::env::var("TOMATO_TEST_POSTGRES_URL")?)?;
    anyhow::ensure!(
        uri.host_str() == Some("127.0.0.1")
            && uri.username() == "tomato_fixture"
            && uri.path() == "/tomato_fixture",
        "Only owned loopback PostgreSQL is permitted"
    );
    let target = SocketAddr::from(([127, 0, 0, 1], uri.port().context("port")?));
    for case in ["restored", "results", "failed"] {
        let jump =
            Jump::with_auth_limit(target, if case == "failed" { 1 } else { usize::MAX }).await?;
        let profile = tempfile::Builder::new()
            .prefix("tomato-recovery-fixture-")
            .tempdir_in(&directory)?;
        let mut connection = Connection::new("POSTGRESQL");
        connection.name = "数据库断线恢复验收".into();
        connection.set("host", "127.0.0.1");
        connection.set("port", target.port());
        connection.set("database", "tomato_fixture");
        connection.set("username", "tomato_fixture");
        connection.set("useSshTunnel", true);
        connection.set("sshTunnelHost", "127.0.0.1");
        connection.set("sshTunnelPort", jump.port);
        connection.set("sshTunnelUsername", "fixture");
        connection.set("sshTunnelPassword", "fixture-only");
        let config = profile.path().join("connections.json");
        tomato_core::Store {
            connections: vec![connection.clone()],
            ..Default::default()
        }
        .save(&config)?;
        let before = std::fs::read(&config)?;
        let path = directory.join(format!("{case}.png"));
        let mut command = std::process::Command::new(&gui);
        command
            .env("TOMATO_DATA_DIR", profile.path())
            .env("TOMATO_UI_PREVIEW", "database_recovery")
            .env("TOMATO_UI_CAPTURE", &path)
            .env("TOMATO_RECOVERY_CASE", case)
            .env("TOMATO_UI_CAPTURE_FRAMES", "2")
            .env("TOMATO_UI_CAPTURE_DELAY_MS", "1000")
            .stdout(std::fs::File::create(
                directory.join(format!("{case}.stdout.log")),
            )?)
            .stderr(std::fs::File::create(
                directory.join(format!("{case}.stderr.log")),
            )?);
        let mut child = command.spawn()?;
        let status = tokio::task::spawn_blocking(move || -> Result<std::process::ExitStatus> {
            let start = std::time::Instant::now();
            loop {
                if let Some(status) = child.try_wait()? {
                    return Ok(status);
                }
                if start.elapsed() > std::time::Duration::from_secs(100) {
                    let _ = child.kill();
                    let _ = child.wait();
                    anyhow::bail!("Owned recovery GUI timed out");
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        })
        .await??;
        anyhow::ensure!(
            status.success(),
            "Recovery GUI did not exit normally: {case}"
        );
        let proof: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("recovery.json"))?)?;
        assert_eq!(proof["successCount"], 1);
        assert_eq!(proof["failCount"], 1);
        assert_eq!(proof["statements"], 2);
        assert_eq!(proof["results"], 1);
        assert_eq!(proof["retainedValue"], "保留中文结果");
        assert_eq!(proof["resultContext"], connection.id);
        assert_eq!(proof["inlineError"], false);
        assert_eq!(proof["physicalOsInputUsed"], false);
        assert_eq!(proof["modalError"], case == "failed");
        assert_eq!(
            proof["connection"]["recovery"],
            if case == "failed" {
                "failed"
            } else {
                "restored"
            }
        );
        assert_eq!(proof["scriptView"], if case == "results" { 1 } else { 0 });
        anyhow::ensure!(
            path.is_file() && path.with_file_name(format!("{case}-01.png")).is_file(),
            "Both native frames are required"
        );
        assert_eq!(
            std::fs::read(&config)?,
            before,
            "Owned encrypted configuration changed"
        );
    }
    std::fs::write(
        directory.join("validation.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"passed":true,"cases":["restored","results","failed"],"actualPostgres":true,"encryptedSsh":true,"normalExit":true,"savedHostsContacted":false,"configurationUnchanged":true,"physicalOsInputUsed":false}),
        )?,
    )?;
    Ok(())
}
