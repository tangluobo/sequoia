//! Stateful HS2 fixture for the actual sidebar/designer/query actor workflow.
use super::*;
mod background;
#[derive(Default)]
pub(super) struct State {
    changed: bool,
    statements: usize,
    field_gate: Option<background::Gate>,
}
impl State {
    pub(super) fn reply(&mut self, sql: &str) -> Result<Step> {
        self.statements += 1;
        ensure!(self.statements <= 2048, "unexpected query loop");
        let rows: Vec<&[&str]> = match sql {
            "USE `default`" => return Ok(Step::done(sql)),
            "SHOW DATABASES" => {
                return Ok(Step::text(
                    sql,
                    &["database_name"],
                    &[&["default"], &["未打开库"]],
                ));
            }
            "SHOW TABLES IN `default`" => {
                return Ok(Step::text(
                    sql,
                    &["tab_name"],
                    &[&["items"], &["view_items"]],
                ));
            }
            "SHOW VIEWS IN `default`" => {
                return Ok(Step::text(sql, &["tab_name"], &[&["view_items"]]));
            }
            "DESCRIBE FORMATTED `default`.`view_items`" => {
                return Ok(Step::text(
                    sql,
                    &["col_name", "data_type", "comment"],
                    &[
                        &["id", "int", "编号"],
                        &["label", "string", "标题"],
                        &["# Detailed Table Information", "", ""],
                        &["Table Type:", "VIRTUAL_VIEW", ""],
                    ],
                ));
            }
            "DESCRIBE FORMATTED `default`.`items`" => {
                let label = [
                    "label",
                    "string",
                    if self.field_gate.is_some() {
                        "后台字段备注"
                    } else {
                        "标题"
                    },
                ];
                let mut rows: Vec<&[&str]> = vec![&["id", "int", "编号"], &label];
                if self.changed {
                    rows.push(&["amount", "decimal(12,2)", "金额"]);
                }
                rows.extend([
                    &["# Detailed Table Information", "", ""][..],
                    &["Table Type:", "MANAGED_TABLE", ""][..],
                    &["Table Parameters:", "", ""][..],
                    &["", "comment", "fixture"][..],
                ]);
                return Ok(Step::text(
                    sql,
                    &["col_name", "data_type", "comment"],
                    &rows,
                ));
            }
            "SHOW CREATE TABLE `default`.`items`" => {
                return Ok(Step::text(
                    sql,
                    &["createtab_stmt"],
                    &[&[if self.changed {
                        "CREATE TABLE `default`.`items` (`id` INT, `label` STRING, `amount` DECIMAL(12,2))"
                    } else {
                        "CREATE TABLE `default`.`items` (`id` INT, `label` STRING)"
                    }]],
                ));
            }
            "ALTER TABLE `default`.`items` ADD COLUMNS (`amount` DECIMAL(12,2) COMMENT '金额')" =>
            {
                ensure!(!self.changed, "DDL was replayed");
                self.changed = true;
                return Ok(Step::done(sql));
            }
            "SELECT COUNT(*) FROM `default`.`items`" => {
                return Ok(Step::text(sql, &["count"], &[&["2"]]));
            }
            "SELECT `id`, `label` FROM `default`.`items` LIMIT 0,1000" => {
                ensure!(!self.changed, "stale projection after DDL");
                vec![&["1", "第一行"], &["2", "第二行"]]
            }
            "SELECT `id`, `label`, `amount` FROM `default`.`items` LIMIT 0,1000" => {
                ensure!(self.changed, "new projection before DDL");
                vec![&["1", "第一行", "12.34"], &["2", "第二行", "56.78"]]
            }
            "SELECT 'hive input' AS value" => {
                return Ok(Step::text(sql, &["value"], &[&["hive input"]]));
            }
            "EXPLAIN SELECT 'hive input' AS value" => {
                return Ok(Step::text(
                    sql,
                    &["Explain"],
                    &[&["STAGE DEPENDENCIES: Stage-0 is a root stage"]],
                ));
            }
            _ => anyhow::bail!("Unexpected native UI SQL: {sql}"),
        };
        Ok(Step::text(
            sql,
            if self.changed {
                &["id", "label", "amount"]
            } else {
                &["id", "label"]
            },
            &rows,
        ))
    }
}
pub(super) async fn serve(listener: TcpListener, template: Arc<Mutex<Peer>>) -> Result<()> {
    let mut peers = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            accepted=listener.accept()=> {
                let (mut socket,_)=accepted?;socket.set_nodelay(true)?;
                let mut peer={let template=template.lock().unwrap();Peer {ui:template.ui.clone(),steps:VecDeque::new(),operation:None,fetched:false,closes:template.closes.clone()}};
                peers.spawn(async move {
                    loop {
                        let (method,sequence,body)=match wire::read_request(&mut socket).await {
                            Ok(v)=>v,
                            Err(e) if e.downcast_ref::<std::io::Error>().is_some_and(|e|e.kind()==std::io::ErrorKind::UnexpectedEof)=>return Ok::<_,anyhow::Error>(()),
                            Err(e)=>return Err(e),
                        };
                        if method == "ExecuteStatement" {
                            let sql = body.required(2)?.text()?;
                            let held = match &peer.ui {
                                Some(state) => state.lock().unwrap().field_gate.as_mut().map(|gate| gate.before(sql)).transpose()?.flatten(),
                                None => None,
                            };
                            if let Some(root) = held {
                                background::wait(&root).await?;
                                peer.ui.as_ref().unwrap().lock().unwrap().field_gate.as_mut().unwrap().released = true;
                            }
                        }
                        let response=peer.response(&method,body)?;
                        socket.write_all(&wire::reply(&method,sequence,response)?).await?;
                    }
                });
            },
            result=peers.join_next(),if !peers.is_empty()=>{result.unwrap()??;},
        }
    }
}

#[tokio::test]
#[ignore = "owned native Hive UI fixture host"]
async fn host_hive_ui_fixture() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("TOMATO_HIVE_UI_FIXTURE")?);
    ensure!(
        root.is_absolute()
            && root
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("tomato-hive-ui-fixture-")),
        "requires owned profile"
    );
    std::fs::create_dir_all(&root)?;
    let mut fixture = Fixture::start("ui", vec![]).await?;
    let mut c = fixture.connection.clone();
    c.name = "owned HIVE".into();
    c.set("password", serde_json::Value::Null);
    c.set("futureFixtureField", serde_json::json!({"keep":true}));
    tomato_core::Store {
        connections: vec![c],
        ..Default::default()
    }
    .save(&root.join("connections.json"))?;
    if std::env::var("TOMATO_HIVE_FIELD_GATE").as_deref() == Ok("owned-fixture") {
        fixture.ui.as_ref().unwrap().lock().unwrap().field_gate =
            Some(background::Gate::new(root.clone()));
    }
    std::fs::write(root.join("ready"), "ready")?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(180);
    while !root.join("stop").exists() {
        if fixture.peer.is_finished() {
            (&mut fixture.peer).await??;
            anyhow::bail!("HS2 fixture closed unexpectedly");
        }
        ensure!(
            tokio::time::Instant::now() < deadline,
            "owned UI fixture timed out"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    ensure!(
        fixture.closes.load(Ordering::SeqCst)
            >= if std::env::var("TOMATO_HIVE_UI_CAPTURE").as_deref() == Ok("owned-fixture")
                || std::env::var("TOMATO_HIVE_FIELD_GATE").as_deref() == Ok("owned-fixture")
            {
                1
            } else {
                2
            },
        "client did not close its HS2 sessions through disconnect and application shutdown"
    );
    let state = fixture.ui.as_ref().unwrap().lock().unwrap();
    let field_gate_verified = state
        .field_gate
        .as_ref()
        .is_some_and(|gate| gate.held && gate.released);
    ensure!(
        field_gate_verified
            || (state.changed && state.statements > 10)
            || (std::env::var("TOMATO_HIVE_UI_CAPTURE").as_deref() == Ok("owned-fixture")
                && state.statements >= 3),
        "UI did not finish its native workflow"
    );
    std::fs::write(
        root.join("server-validation.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"passed":true,"nativeHs2Statements":state.statements,"ddlExecutedOnce":state.changed,"heldFieldResponseReleased":field_gate_verified,"closedSessions":fixture.closes.load(Ordering::SeqCst),"savedHostsContacted":false}),
        )?,
    )?;
    Ok(())
}
