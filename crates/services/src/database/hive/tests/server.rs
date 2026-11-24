//! An isolated HS2 peer exercises the public database actor, including SQL and close.
mod tls;
mod ui;
use super::*;
use anyhow::{Context as _, ensure};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    task::JoinHandle,
};
use tomato_core::workbench::{DatabaseAction, DatabaseOutput, PageRequest, TableId};

struct Step {
    sql: String,
    columns: Vec<(&'static str, i32)>,
    rows: Vec<Vec<Cell>>,
    error: bool,
}
impl Step {
    fn text(sql: impl Into<String>, columns: &[&'static str], rows: &[&[&str]]) -> Self {
        Self {
            sql: sql.into(),
            columns: columns.iter().map(|s| (*s, 7)).collect(),
            rows: rows
                .iter()
                .map(|r| r.iter().map(|s| Cell::Text((*s).into())).collect())
                .collect(),
            error: false,
        }
    }
    fn done(sql: impl Into<String>) -> Self {
        Self::text(sql, &[], &[])
    }
}
struct Peer {
    ui: Option<Arc<Mutex<ui::State>>>,
    steps: VecDeque<Step>,
    operation: Option<Step>,
    fetched: bool,
    closes: Arc<AtomicUsize>,
}
fn status() -> Value {
    Value::object([(1, Value::I32(0))])
}
fn handle() -> Value {
    Value::object([
        (1, Value::Bytes(vec![7; 16])),
        (2, Value::Bytes(vec![9; 16])),
    ])
}
impl Peer {
    fn response(&mut self, method: &str, request: Value) -> Result<Value> {
        let mut response = vec![(1, status())];
        match method {
            "OpenSession" => {
                ensure!(request.required(1)?.integer()? == 5, "expected V6");
                ensure!(
                    request.required(2)?.text()? == "fixture-user"
                        && (self.ui.is_some() || request.required(3)?.text()? == "fixture-only"),
                    "wrong fixture credentials"
                );
                response.extend([(2, Value::I32(5)), (3, Value::object([(1, handle())]))]);
            }
            "GetInfo" => response.push((2, Value::object([(1, Value::string("4.2.1"))]))),
            "ExecuteStatement" => {
                ensure!(self.operation.is_none(), "prior operation was not closed");
                let sql = request.required(2)?.text()?;
                let step = if let Some(state) = &self.ui {
                    state.lock().unwrap().reply(sql)?
                } else {
                    self.steps
                        .pop_front()
                        .with_context(|| format!("unexpected SQL: {sql}"))?
                };
                ensure!(
                    step.sql == sql,
                    "expected SQL: {}\nactual SQL: {sql}",
                    step.sql
                );
                if step.error {
                    return Ok(Value::object([(
                        1,
                        Value::object([
                            (1, Value::I32(3)),
                            (5, Value::string("fixture SQL error")),
                        ]),
                    )]));
                }
                response.push((
                    2,
                    Value::object([
                        (1, handle()),
                        (2, Value::I32(0)),
                        (3, Value::Bool(!step.columns.is_empty())),
                    ]),
                ));
                self.operation = Some(step);
                self.fetched = false;
            }
            "GetOperationStatus" => {
                response.extend([
                    (2, Value::I32(2)),
                    (
                        9,
                        Value::Bool(
                            !self
                                .operation
                                .as_ref()
                                .context("no operation")?
                                .columns
                                .is_empty(),
                        ),
                    ),
                    (11, Value::I64(0)),
                ]);
            }
            "GetResultSetMetadata" => {
                let columns = &self.operation.as_ref().context("no operation")?.columns;
                response.push((
                    2,
                    Value::object([(
                        1,
                        Value::List(
                            12,
                            columns
                                .iter()
                                .enumerate()
                                .map(|(index, (name, kind))| {
                                    Value::object([
                                        (1, Value::string(name)),
                                        (
                                            2,
                                            Value::object([(
                                                1,
                                                Value::List(
                                                    12,
                                                    vec![Value::object([(
                                                        1,
                                                        Value::object([(1, Value::I32(*kind))]),
                                                    )])],
                                                ),
                                            )]),
                                        ),
                                        (3, Value::I32(index as i32 + 1)),
                                    ])
                                })
                                .collect(),
                        ),
                    )]),
                ));
            }
            "FetchResults" => {
                ensure!(request.required(3)?.integer()? == 256, "unbounded fetch");
                let step = self.operation.as_ref().context("no operation")?;
                let rows = if self.fetched { &[][..] } else { &step.rows };
                let vectors = step
                    .columns
                    .iter()
                    .enumerate()
                    .map(|(index, (_, kind))| {
                        let mut nulls = vec![0u8; rows.len().div_ceil(8)];
                        let mut values = Vec::new();
                        for (row, cells) in rows.iter().enumerate() {
                            let cell = &cells[index];
                            if *cell == Cell::Null {
                                nulls[row / 8] |= 1 << (row % 8);
                            }
                            values.push(match (kind, cell) {
                                (4, Cell::Integer(n)) => Value::I64(*n),
                                (4, _) => Value::I64(0),
                                (9, Cell::Binary(bytes)) => Value::Bytes(bytes.clone()),
                                (_, Cell::Null) => Value::string(""),
                                (_, cell) => Value::string(cell.display()),
                            });
                        }
                        let (union, element) = match kind {
                            4 => (5, 10),
                            9 => (8, 11),
                            _ => (7, 11),
                        };
                        Value::object([(
                            union,
                            Value::object([
                                (1, Value::List(element, values)),
                                (2, Value::Bytes(nulls)),
                            ]),
                        )])
                    })
                    .collect();
                self.fetched = true;
                // Several Java Hive servers return false despite an additional
                // page: clients must keep fetching until the empty batch.
                response.extend([
                    (2, Value::Bool(false)),
                    (
                        3,
                        Value::object([
                            (1, Value::I64(0)),
                            (2, Value::List(12, vec![])),
                            (3, Value::List(12, vectors)),
                        ]),
                    ),
                ]);
            }
            "CloseOperation" => {
                ensure!(self.operation.take().is_some(), "operation closed twice");
            }
            "CloseSession" => {
                ensure!(
                    self.operation.is_none() && self.steps.is_empty(),
                    "unfinished query/script"
                );
                self.closes.fetch_add(1, Ordering::SeqCst);
            }
            _ => anyhow::bail!("unexpected RPC {method}"),
        }
        Ok(Value::object(response))
    }
}
struct Fixture {
    ui: Option<Arc<Mutex<ui::State>>>,
    connection: Connection,
    peer: JoinHandle<Result<()>>,
    closes: Arc<AtomicUsize>,
    _certificates: tempfile::TempDir,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.peer.abort();
    }
}
impl Fixture {
    async fn start(mode: &str, steps: Vec<Step>) -> Result<Self> {
        crate::init_crypto();
        let certificates = tempfile::tempdir()?;
        let mutual = mode.starts_with("mtls-");
        let tls = mutual || matches!(mode, "tls" | "sasl-tls" | "https");
        let acceptor = if tls {
            let certificate = rcgen::generate_simple_self_signed(vec![
                "127.0.0.1".into(),
                "hive.internal.test".into(),
            ])?;
            std::fs::write(certificates.path().join("root.pem"), certificate.cert.pem())?;
            let builder = rustls::ServerConfig::builder();
            let builder = if mutual {
                builder.with_client_cert_verifier(tls::client(certificates.path(), mode)?)
            } else {
                builder.with_no_client_auth()
            };
            Some(tokio_rustls::TlsAcceptor::from(Arc::new(
                builder.with_single_cert(
                    vec![certificate.cert.der().clone()],
                    rustls::pki_types::PrivatePkcs8KeyDer::from(
                        certificate.signing_key.serialize_der(),
                    )
                    .into(),
                )?,
            )))
        } else {
            None
        };
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let mut connection = Connection::new("HIVE");
        connection.set("host", "127.0.0.1");
        connection.set("port", listener.local_addr()?.port());
        connection.set("username", "fixture-user");
        connection.set("password", "fixture-only");
        connection.set(
            "database",
            match mode {
                "http" | "https" | "mtls-pem" | "mtls-jks" | "mtls-p12" => {
                    "default;transportMode=http;httpPath=fixture;auth=noSasl"
                }
                "sasl" | "sasl-tls" => "default",
                _ => "default;auth=noSasl",
            },
        );
        if tls {
            connection.set(
                "database",
                format!(
                    "{};ssl=true;sslTrustStore={}",
                    connection.text("database"),
                    certificates.path().join("root.pem").display()
                ),
            );
        }
        if mutual {
            connection.set(
                "database",
                format!(
                    "{};twoWay=true;sslKeyStore={};keyStorePassword=fixture-pass",
                    connection.text("database"),
                    certificates.path().join("client-keystore").display()
                ),
            );
        }
        let closes = Arc::new(AtomicUsize::new(0));
        let ui = (mode == "ui").then(|| Arc::new(Mutex::new(ui::State::default())));
        let state = Arc::new(Mutex::new(Peer {
            ui: ui.clone(),
            steps: steps.into(),
            operation: None,
            fetched: false,
            closes: closes.clone(),
        }));
        let http = mutual || matches!(mode, "http" | "https");
        let sasl = matches!(mode, "sasl" | "sasl-tls");
        let interactive = mode == "ui";
        let peer = tokio::spawn(async move {
            if interactive {
                return ui::serve(listener, state).await;
            }
            let (socket, _) = listener.accept().await?;
            socket.set_nodelay(true)?;
            let mut socket: Box<dyn Io> = if let Some(acceptor) = acceptor {
                Box::new(acceptor.accept(socket).await?)
            } else {
                Box::new(socket)
            };
            if http {
                use bytes::Bytes;
                use http_body_util::{BodyExt as _, Full};
                hyper::server::conn::http1::Builder::new()
                    .serve_connection(
                        hyper_util::rt::TokioIo::new(socket),
                        hyper::service::service_fn(
                            move |request: hyper::Request<hyper::body::Incoming>| {
                                let state = state.clone();
                                async move {
                                    ensure!(request.uri().path() == "/fixture", "wrong HTTP path");
                                    ensure!(
                                        request.headers().get("Authorization").is_some(),
                                        "missing HTTP auth"
                                    );
                                    let bytes = request.into_body().collect().await?.to_bytes();
                                    let (method, sequence, body) =
                                        wire::read_request(&mut bytes.as_ref()).await?;
                                    let response = state.lock().unwrap().response(&method, body)?;
                                    Ok::<_, anyhow::Error>(
                                        hyper::Response::builder()
                                            .header(
                                                "Set-Cookie",
                                                "hive.server2.auth=fixture; HttpOnly",
                                            )
                                            .body(Full::new(Bytes::from(wire::reply(
                                                &method, sequence, response,
                                            )?)))?,
                                    )
                                }
                            },
                        ),
                    )
                    .await?;
                return Ok(());
            }
            if sasl {
                authenticate(&mut socket).await?;
            }
            loop {
                let request = if sasl {
                    let length = match socket.read_u32().await {
                        Ok(n) => n as usize,
                        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                        Err(e) => return Err(e.into()),
                    };
                    ensure!(length <= 8 * 1024 * 1024, "fixture frame too large");
                    let mut bytes = vec![0; length];
                    socket.read_exact(&mut bytes).await?;
                    wire::read_request(&mut bytes.as_slice()).await
                } else {
                    wire::read_request(&mut socket).await
                };
                let (method, sequence, body) = match request {
                    Ok(v) => v,
                    Err(e)
                        if e.downcast_ref::<std::io::Error>()
                            .is_some_and(|e| e.kind() == std::io::ErrorKind::UnexpectedEof) =>
                    {
                        break;
                    }
                    Err(e) => return Err(e),
                };
                let response = state.lock().unwrap().response(&method, body)?;
                let bytes = wire::reply(&method, sequence, response)?;
                for fragment in bytes.chunks(97) {
                    if sasl {
                        socket.write_u32(fragment.len() as u32).await?;
                    }
                    socket.write_all(fragment).await?;
                }
            }
            Ok(())
        });
        Ok(Self {
            ui,
            connection,
            peer,
            closes,
            _certificates: certificates,
        })
    }
    async fn closed(&mut self) -> Result<()> {
        tokio::time::timeout(std::time::Duration::from_secs(3), &mut self.peer).await???;
        ensure!(
            self.closes.load(Ordering::SeqCst) == 1,
            "session was not closed"
        );
        Ok(())
    }
}
trait Io: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin> Io for T {}
async fn authenticate(socket: &mut (impl Io + ?Sized)) -> Result<()> {
    for (status, expected) in [
        (1, &b"PLAIN"[..]),
        (2, &b"\0fixture-user\0fixture-only"[..]),
    ] {
        ensure!(socket.read_u8().await? == status, "wrong SASL phase");
        let count = socket.read_u32().await? as usize;
        ensure!(count <= 1024, "fixture auth bound");
        let mut bytes = vec![0; count];
        socket.read_exact(&mut bytes).await?;
        ensure!(bytes == expected, "wrong fixture auth");
    }
    socket.write_all(&[5, 0, 0, 0, 0]).await?;
    Ok(())
}
#[tokio::test]
async fn native_hive_actor_auth_catalog_page_design_save_refresh_and_close() -> Result<()> {
    use tomato_core::table_design::alter::Edit;
    for mode in [
        "plain", "sasl", "http", "tls", "sasl-tls", "https", "mtls-pem", "mtls-jks", "mtls-p12",
    ] {
        let describe = |changed: bool| {
            let mut rows: Vec<&[&str]> = vec![&["id", "int", "编号"], &["label", "string", "标题"]];
            if changed {
                rows.push(&["amount", "decimal(12,2)", "金额"]);
            }
            rows.extend([
                &["# Detailed Table Information", "", ""][..],
                &["Table Type:", "MANAGED_TABLE", ""][..],
                &["Table Parameters:", "", ""][..],
                &["", "comment", "fixture"][..],
            ]);
            Step::text(
                "DESCRIBE FORMATTED `default`.`items`",
                &["col_name", "data_type", "comment"],
                &rows,
            )
        };
        let ddl = || {
            Step::text(
                "SHOW CREATE TABLE `default`.`items`",
                &["createtab_stmt"],
                &[&["CREATE TABLE `default`.`items` (`id` INT, `label` STRING)"]],
            )
        };
        let mut steps = vec![
            Step::text(
                "SHOW DATABASES",
                &["database_name"],
                &[&["default"], &["其他库"]],
            ),
            Step::text(
                "SHOW TABLES IN `default`",
                &["tab_name"],
                &[&["items"], &["view_items"]],
            ),
            Step::text("SHOW VIEWS IN `default`", &["tab_name"], &[&["view_items"]]),
            describe(false),
            Step::text(
                "SELECT COUNT(*) FROM `default`.`items`",
                &["count"],
                &[&["3"]],
            ),
            Step::text(
                "SELECT `id`, `label` FROM `default`.`items` LIMIT 1,1",
                &["id", "label"],
                &[&["2", "中文"]],
            ),
            describe(false),
            // Structure-only open, on-demand SQL, and the legacy full open.
            describe(false),
            ddl(),
            describe(false),
            ddl(),
            // Save checks the same catalog without retrieving DDL first.
            describe(false),
            Step::done(
                "ALTER TABLE `default`.`items` ADD COLUMNS (`amount` DECIMAL(12,2) COMMENT '金额')",
            ),
            describe(true),
            ddl(),
            // A no-op lightweight save returns a lightweight snapshot too.
            describe(true),
        ];
        let mut failure = Step::done("SELECT broken");
        failure.error = true;
        steps.push(failure);
        steps.push(Step::text(
            "SELECT 'still alive'",
            &["value"],
            &[&["still alive"]],
        ));
        let mut fixture = Fixture::start(mode, steps).await?;
        let before = serde_json::to_value(&fixture.connection)?;
        let mut databases = super::super::super::Databases::default();
        let conn = &fixture.connection;
        let DatabaseOutput::Databases(names) =
            databases.workbench(conn, DatabaseAction::Databases).await?
        else {
            anyhow::bail!("expected databases")
        };
        assert_eq!(names, vec!["default", "其他库"]);
        let DatabaseOutput::Tables(tables) = databases
            .workbench(
                conn,
                DatabaseAction::Tables {
                    schema: "default".into(),
                },
            )
            .await?
        else {
            anyhow::bail!("expected tables")
        };
        assert_eq!(tables.len(), 2);
        assert!(!tables[0].view && tables[1].view);
        let table = TableId {
            schema: "default".into(),
            name: "items".into(),
        };
        let request = PageRequest {
            table: table.clone(),
            offset: 1,
            limit: 1,
            sort: None,
            query: Default::default(),
        };
        let DatabaseOutput::Page(page) = databases
            .workbench(conn, DatabaseAction::Page { request })
            .await?
        else {
            anyhow::bail!("expected page")
        };
        assert_eq!(page.total, 3);
        assert_eq!(page.rows[0][1], Cell::Text("中文".into()));
        assert_eq!(
            page.sql,
            "SELECT `id`, `label` FROM `default`.`items` LIMIT 1,1"
        );
        let DatabaseOutput::Design(light) = databases
            .workbench(
                conn,
                DatabaseAction::DesignStructure {
                    table: table.clone(),
                },
            )
            .await?
        else {
            anyhow::bail!("expected lightweight Hive designer")
        };
        assert!(light.sql.is_empty());
        let DatabaseOutput::DesignSql { sql, .. } = databases
            .workbench(
                conn,
                DatabaseAction::DesignSql {
                    table: table.clone(),
                },
            )
            .await?
        else {
            anyhow::bail!("expected on-demand Hive SQL")
        };
        assert!(sql.contains("CREATE TABLE"));
        let DatabaseOutput::Design(snapshot) = databases
            .workbench(
                conn,
                DatabaseAction::Design {
                    table: table.clone(),
                },
            )
            .await?
        else {
            anyhow::bail!("expected designer")
        };
        let mut after = snapshot.definition.clone();
        after.columns.push(tomato_core::table_design::Column {
            name: "amount".into(),
            data_type: "DECIMAL".into(),
            length: "12".into(),
            scale: "2".into(),
            comment: "金额".into(),
            ..Default::default()
        });
        let edit = Edit {
            before: snapshot.baseline(),
            after,
            origins: vec![Some("id".into()), Some("label".into()), None],
        };
        assert_eq!(
            edit.plan(tomato_core::table_design::Dialect::Hive, false)?
                .statements
                .len(),
            1
        );
        let DatabaseOutput::Design(saved) = databases
            .workbench(
                conn,
                DatabaseAction::SaveDesign {
                    edit: Box::new(edit),
                },
            )
            .await?
        else {
            anyhow::bail!("expected saved designer")
        };
        assert_eq!(saved.definition.columns.len(), 3);
        assert_eq!(saved.definition.columns[2].scale, "2");
        assert!(
            !saved.sql.is_empty(),
            "legacy full save must retain its SQL output"
        );
        let unchanged = Edit {
            before: saved.baseline(),
            after: saved.definition.clone(),
            origins: saved
                .definition
                .columns
                .iter()
                .map(|c| Some(c.name.clone()))
                .collect(),
        };
        let DatabaseOutput::Design(light_saved) = databases
            .workbench(
                conn,
                DatabaseAction::SaveStructure {
                    edit: Box::new(unchanged),
                },
            )
            .await?
        else {
            anyhow::bail!("expected lightweight Hive save")
        };
        assert!(light_saved.sql.is_empty());
        assert_eq!(light_saved.definition, saved.definition);
        assert!(databases.query(conn, "SELECT broken").await.is_err());
        let result = databases.query(conn, "SELECT 'still alive'").await?;
        assert_eq!(result[0].rows[0][0], "still alive");
        assert_eq!(serde_json::to_value(conn)?, before);
        databases.disconnect(&conn.id).await?;
        fixture.closed().await?;
    }
    Ok(())
}

use crate::ssh_fixture as ssh_jump;
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hive_tls_sasl_through_ssh_verifies_original_identity_and_releases_its_tunnel() -> Result<()>
{
    use sqlx::Connection as _;
    let mut fixture = Fixture::start(
        "sasl-tls",
        vec![Step::text("SELECT '中文'", &["value"], &[&["中文"]])],
    )
    .await?;
    fixture.connection.set("host", "hive.internal.test");
    let original = serde_json::to_value(&fixture.connection)?;
    let jump = ssh_jump::Jump::start().await?;
    let tunnel = crate::terminal::ssh::tunnel::Tunnel::open_via(
        &fixture.connection,
        &jump.connection,
        jump.root.path().join("known_hosts"),
    )
    .await?;
    let address = tunnel.address;
    let routed = tunnel.routed(&fixture.connection);
    let mut connection =
        super::super::connect(&fixture.connection, "default", Some(&routed)).await?;
    let rows = crate::database::workbench::metadata(&mut connection, "SELECT '中文'", &[]).await?;
    assert_eq!(crate::database::workbench::text(&rows[0], 0)?, "中文");
    connection.close().await?;
    fixture.closed().await?;
    assert_eq!(
        *jump.targets.lock().unwrap(),
        vec![(
            "hive.internal.test".into(),
            fixture.connection.number("port") as u32
        )]
    );
    assert_eq!(serde_json::to_value(&fixture.connection)?, original);
    tunnel.close().await;
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    Ok(())
}
#[tokio::test]
async fn hive_tls_rejects_a_trusted_certificate_for_a_different_host() -> Result<()> {
    let fixture = Fixture::start("tls", vec![]).await?;
    let mut original = fixture.connection.clone();
    original.set("host", "wrong-hive.internal.test");
    let failure = super::super::connect(&original, "default", Some(&fixture.connection))
        .await
        .unwrap_err();
    assert!(
        format!("{failure:#}").contains("certificate not valid for name"),
        "{failure:#}"
    );
    Ok(())
}

#[tokio::test]
async fn hive_client_keystore_wrong_password_alias_and_missing_key_never_fall_back() -> Result<()> {
    for mode in ["mtls-jks", "mtls-p12"] {
        let fixture = Fixture::start(mode, vec![]).await?;
        let saved = serde_json::to_value(&fixture.connection)?;
        let mut connection = fixture.connection.clone();
        let suffix = connection.text("database").replace(
            "keyStorePassword=fixture-pass",
            "keyStorePassword=incorrect-fixture",
        );
        connection.set("database", suffix);
        assert!(
            config::Config::new(&connection, "", None)?
                .tls()
                .await
                .is_err()
        );
        connection.set(
            "database",
            format!(
                "{};sslKeyStoreAlias=missing",
                fixture.connection.text("database")
            ),
        );
        assert!(
            config::Config::new(&connection, "", None)?
                .tls()
                .await
                .is_err()
        );
        connection.set(
            "database",
            format!("{};sslKeyStore=", fixture.connection.text("database")),
        );
        assert!(
            config::Config::new(&connection, "", None)?
                .tls()
                .await
                .is_err()
        );
        assert_eq!(serde_json::to_value(&fixture.connection)?, saved);
    }
    Ok(())
}
