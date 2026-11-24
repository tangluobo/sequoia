use super::*;
use std::sync::Arc;
use tomato_core::workbench::{TableId, TableInfo};
use tomato_services::mongo::{Action, Output as MongoOutput, Target};

pub(super) struct Host {
    pub connection: Arc<Connection>,
    pub databases: Vec<String>,
    pub collections: std::collections::BTreeMap<String, Vec<TableInfo>>,
    pub generation: String,
    pub ready: bool,
    pub busy: bool,
    pub loading: Option<String>,
}
impl Host {
    fn new(connection: Connection) -> Self {
        Self {
            connection: Arc::new(connection),
            databases: Vec::new(),
            collections: Default::default(),
            generation: Connection::new("TOOL").id,
            ready: false,
            busy: false,
            loading: None,
        }
    }
}
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Database,
    Create,
    Rename,
    Drop,
}
pub(super) struct Dialog {
    id: String,
    connection: String,
    target: Target,
    kind: Kind,
    name: String,
    initial: String,
    initial_step: bool,
    focus: bool,
}
impl TomatoApp {
    pub(super) fn prune_mongo_hosts(&mut self) {
        let obsolete: Vec<_> = self
            .mongo_hosts
            .keys()
            .filter(|id| {
                !self
                    .store
                    .connections
                    .iter()
                    .any(|c| &c.id == *id && c.kind() == "MONGODB")
            })
            .cloned()
            .collect();
        for id in obsolete {
            self.mongo_disconnect(&id);
        }
    }
    pub(super) fn mongo_open(&mut self, connection: Connection, refresh: bool) {
        let id = connection.id.clone();
        if !refresh && self.mongo_hosts.get(&id).is_some_and(|h| h.ready) {
            if !self.collapsed.remove(&id) {
                self.collapsed.insert(id);
            }
            return;
        }
        self.mongo_hosts
            .entry(id.clone())
            .or_insert_with(|| Host::new(connection));
        self.collapsed.remove(&id);
        self.mongo_request(&id, Action::Databases, None);
    }
    fn mongo_request(&mut self, id: &str, action: Action, dialog: Option<String>) {
        let Some(host) = self.mongo_hosts.get_mut(id) else {
            return;
        };
        if host.busy {
            return;
        }
        let target = action.target().cloned();
        host.loading = match &action {
            Action::Collections { database } => Some(database.clone()),
            _ => target.as_ref().map(|t| t.database.clone()),
        };
        match self.queue.submit(Command::Mongo {
            connection: (*host.connection).clone(),
            action,
        }) {
            Ok(request) => {
                host.busy = true;
                self.pending.insert(
                    request,
                    Pending {
                        tab_id: String::new(),
                        purpose: Purpose::MongoTree {
                            connection: id.into(),
                            generation: host.generation.clone(),
                            target,
                            dialog,
                        },
                    },
                );
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }
    pub(super) fn mongo_reply(
        &mut self,
        id: &str,
        generation: &str,
        target: Option<&Target>,
        dialog: Option<&str>,
        result: anyhow::Result<tomato_services::commands::Output>,
    ) {
        let Some(host) = self
            .mongo_hosts
            .get_mut(id)
            .filter(|h| h.generation == generation)
        else {
            return;
        };
        host.busy = false;
        let mut followup = None;
        match result {
            Err(error) => {
                self.error = Some(format!("MongoDB 操作失败：{error:#}"));
                return;
            }
            Ok(tomato_services::commands::Output::Mongo(output)) => match output {
                MongoOutput::Databases(names) => {
                    host.ready = true;
                    host.databases = names;
                    host.collections.clear();
                    if std::env::var_os("TOMATO_UI_CAPTURE").is_some()
                        && std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("mongo")
                        && host.connection.name == "MongoDB 验收"
                        && let Some(target) = std::env::var("TOMATO_UI_MONGO_TARGET")
                            .ok()
                            .and_then(|s| serde_json::from_str::<Target>(&s).ok())
                        && target.database.starts_with("tomato_fixture")
                        && host.databases.contains(&target.database)
                    {
                        followup = Some(Action::Collections {
                            database: target.database,
                        });
                    }
                }
                MongoOutput::Collections { database, names } => {
                    host.collections.insert(
                        database.clone(),
                        names
                            .into_iter()
                            .map(|name| TableInfo {
                                table: TableId {
                                    schema: database.clone(),
                                    name,
                                },
                                view: false,
                            })
                            .collect(),
                    );
                    self.collapsed.remove(&format!("schema:{id}:{database}"));
                }
                MongoOutput::Changed { target: new, kind } => match kind.as_str() {
                    "collections" => {
                        if host.databases.contains(&new.database) {
                            followup = Some(Action::Collections {
                                database: new.database,
                            });
                        } else {
                            followup = Some(Action::Databases);
                        }
                    }
                    "renamed" => {
                        if let Some(old) = target
                            && let Some(entries) = host.collections.get_mut(&old.database)
                            && let Some(entry) =
                                entries.iter_mut().find(|e| e.table.name == old.collection)
                        {
                            entry.table.name = new.collection;
                        }
                    }
                    "dropped" => {
                        if let Some(entries) = host.collections.get_mut(&new.database) {
                            entries.retain(|e| e.table.name != new.collection);
                        }
                    }
                    _ => {}
                },
                _ => {}
            },
            Ok(_) => {}
        }
        if dialog.is_some_and(|token| self.mongo_dialog.as_ref().is_some_and(|d| d.id == token)) {
            self.mongo_dialog = None;
        }
        if let Some(action) = followup {
            self.mongo_request(id, action, None);
        }
    }
    pub(super) fn mongo_collections(&mut self, id: &str, database: String, refresh: bool) {
        let key = format!("schema:{id}:{database}");
        if !refresh
            && self
                .mongo_hosts
                .get(id)
                .is_some_and(|h| h.collections.contains_key(&database))
        {
            if !self.collapsed.remove(&key) {
                self.collapsed.insert(key);
            }
            return;
        }
        self.mongo_request(id, Action::Collections { database }, None);
    }
    pub(super) fn mongo_collection(&mut self, connection: Connection, target: Target) {
        let source = format!(
            "mongo:{}:{}",
            connection.id,
            serde_json::to_string(&target).unwrap_or_default()
        );
        if let Some(tab) = self.tabs.iter().find(|t| t.source_id == source) {
            self.active = Some(tab.id.clone());
            return;
        }
        let connection = self
            .mongo_hosts
            .get(&connection.id)
            .map(|h| h.connection.clone())
            .unwrap_or_else(|| Arc::new(connection));
        let id = Connection::new("TOOL").id;
        self.tabs.push(Tab {
            id: id.clone(),
            source_id: source,
            title: format!("{}@{}", target.collection, target.database),
            panel: Panel::Mongo {
                connection,
                view: Box::new(crate::tabs::mongo::State::new(target)),
            },
            busy: false,
            error: None,
        });
        self.active = Some(id);
    }
    pub(super) fn mongo_disconnect(&mut self, id: &str) {
        if let Err(error) = self.queue.submit(Command::Disconnect { id: id.into() }) {
            self.error = Some(format!("关闭 MongoDB 失败：{error:#}"));
            return;
        }
        let tabs: Vec<_> = self
            .tabs
            .iter()
            .filter_map(|t| {
                matches!(&t.panel,Panel::Mongo {connection,..} if connection.id==id)
                    .then_some(t.id.clone())
            })
            .collect();
        for tab in tabs {
            self.close(&tab);
        }
        self.mongo_hosts.remove(id);
        self.mongo_dialog = None;
    }
    pub(super) fn mongo_prompt(&mut self, connection: Connection, target: Target, kind: Kind) {
        let id = connection.id.clone();
        self.mongo_hosts
            .entry(id.clone())
            .or_insert_with(|| Host::new(connection));
        self.mongo_dialog = Some(Dialog {
            id: Connection::new("TOOL").id,
            connection: id,
            name: if matches!(kind, Kind::Rename) {
                target.collection.clone()
            } else {
                String::new()
            },
            initial: "items".into(),
            initial_step: false,
            target,
            kind,
            focus: true,
        });
    }
    pub(super) fn mongo_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.mongo_dialog else {
            return;
        };
        let busy = self
            .mongo_hosts
            .get(&dialog.connection)
            .is_some_and(|h| h.busy);
        let mut accept = false;
        let mut cancel = false;
        egui::Window::new(match dialog.kind {
            Kind::Database if dialog.initial_step => "初始集合",
            Kind::Database => "新建 MongoDB 数据库",
            Kind::Create => "新建集合",
            Kind::Rename => "重命名集合",
            Kind::Drop => "删除集合",
        })
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            match dialog.kind {
                Kind::Drop => {
                    ui.label(format!("确定删除集合 “{}” 吗？", dialog.target.collection));
                    ui.label("集合内所有文档和索引都会被删除，此操作不可撤销。");
                }
                _ => {
                    ui.label(if matches!(dialog.kind, Kind::Database) {
                        "MongoDB 数据库将在创建首个集合时生成"
                    } else {
                        &dialog.target.database
                    });
                    ui.label(
                        if matches!(dialog.kind, Kind::Database) && !dialog.initial_step {
                            "数据库名："
                        } else {
                            "集合名："
                        },
                    );
                    let field = ui.text_edit_singleline(if dialog.initial_step {
                        &mut dialog.initial
                    } else {
                        &mut dialog.name
                    });
                    if dialog.focus {
                        field.request_focus();
                        dialog.focus = false;
                    }
                }
            }
            ui.horizontal(|ui| {
                accept = ui.add_enabled(!busy, egui::Button::new("确定")).clicked();
                cancel = ui.button("取消").clicked();
            });
        });
        if cancel {
            self.mongo_dialog = None;
            return;
        }
        if accept {
            if !matches!(dialog.kind, Kind::Drop) {
                let name = if dialog.initial_step {
                    &dialog.initial
                } else {
                    &dialog.name
                };
                if name.trim().is_empty()
                    || matches!(dialog.kind, Kind::Rename)
                        && name.trim() == dialog.target.collection
                {
                    self.mongo_dialog = None;
                    return;
                }
            }
            if matches!(dialog.kind, Kind::Database) && !dialog.initial_step {
                dialog.initial_step = true;
                dialog.focus = true;
                return;
            }
            let action = match dialog.kind {
                Kind::Database => Action::CreateCollection {
                    target: Target {
                        database: dialog.name.trim().into(),
                        collection: dialog.initial.trim().into(),
                    },
                },
                Kind::Create => Action::CreateCollection {
                    target: Target {
                        collection: dialog.name.trim().into(),
                        ..dialog.target.clone()
                    },
                },
                Kind::Rename => Action::RenameCollection {
                    target: dialog.target.clone(),
                    name: dialog.name.trim().into(),
                },
                Kind::Drop => Action::DropCollection {
                    target: dialog.target.clone(),
                },
            };
            let (id, token) = (dialog.connection.clone(), dialog.id.clone());
            self.mongo_request(&id, action, Some(token));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    fn app(connection: &Connection, path: &std::path::Path) -> Result<TomatoApp> {
        Store {
            connections: vec![connection.clone()],
            ..Default::default()
        }
        .save(path)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        Ok(TomatoApp::with_repository(
            &ctx,
            runtime,
            ConnectionFileRepository::new(path),
        ))
    }
    fn rows<'a>(app: &'a TomatoApp, filter: &str) -> Vec<db_tree::Row<'a>> {
        db_tree::rows(
            &app.store,
            &app.tabs,
            &app.collapsed,
            db_tree::Hosts {
                remote: &app.remote_hosts,
                backups: &app.backups,
                sql: &app.sql_hosts,
                queries: &app.queries,
                ssh: &app.ssh_hosts,
                local: &app.local_trees,
                mongo: &app.mongo_hosts,
                redis: &app.redis_hosts,
            },
            filter,
        )
    }
    #[test]
    fn mongo_tree_keeps_runtime_credentials_and_opened_icons_and_rejects_stale_replies()
    -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("connections.json");
        let c = Connection::new("MONGODB");
        let mut app = app(&c, &path)?;
        let bytes = std::fs::read(&path)?;
        let mut runtime = c.clone();
        runtime.set("password", "runtime-only");
        runtime.set("future", serde_json::json!({"retained":true}));
        let mut host = Host::new(runtime);
        host.ready = true;
        host.databases = vec!["fixture".into()];
        let generation = host.generation.clone();
        app.mongo_hosts.insert(c.id.clone(), host);
        app.collapsed.remove(&c.id);
        assert_eq!(rows(&app, "").len(), 2);
        assert_eq!(rows(&app, "")[1].icon(), "images/connect/database_gray.png");
        app.mongo_reply(
            &c.id,
            &generation,
            None,
            None,
            Ok(tomato_services::commands::Output::Mongo(
                MongoOutput::Collections {
                    database: "fixture".into(),
                    names: vec!["items".into()],
                },
            )),
        );
        assert_eq!(rows(&app, "").len(), 3);
        assert_eq!(rows(&app, "")[1].icon(), "images/connect/database.png");
        app.collapsed.insert(format!("schema:{}:fixture", c.id));
        assert_eq!(rows(&app, "").len(), 2);
        assert_eq!(rows(&app, "")[1].icon(), "images/connect/database.png");
        assert_eq!(
            rows(&app, "items").len(),
            3,
            "search must retain host and database ancestors"
        );
        let target = Target {
            database: "fixture".into(),
            collection: "items".into(),
        };
        app.mongo_collection(c.clone(), target.clone());
        app.mongo_collection(c.clone(), target.clone());
        assert_eq!(app.tabs.len(), 1);
        let Panel::Mongo { connection, view } = &mut app.tabs[0].panel else {
            unreachable!()
        };
        assert!(Arc::ptr_eq(connection, &app.mongo_hosts[&c.id].connection));
        assert_eq!(connection.text("password"), "runtime-only");
        view.pending.clear();
        view.accept(MongoOutput::Documents {
            target: target.clone(),
            documents: vec!["{\"_id\":1,\"value\":\"中文\"}".into()],
            truncated: false,
        });
        let ctx = app.context.clone();
        for page in 0..4 {
            if let Panel::Mongo { view, .. } = &mut app.tabs[0].panel {
                view.page = page;
                view.pending.clear();
            }
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280., 900.),
                    )),
                    ..Default::default()
                },
                |ui| app.ui(ui),
            );
            output.textures_delta.clear();
            assert!(!output.shapes.is_empty());
        }
        app.mongo_reply(
            &c.id,
            "stale",
            None,
            None,
            Ok(tomato_services::commands::Output::Mongo(
                MongoOutput::Databases(vec!["wrong".into()]),
            )),
        );
        assert_eq!(app.mongo_hosts[&c.id].databases, ["fixture"]);
        app.mongo_prompt(c.clone(), target, Kind::Rename);
        let token = app.mongo_dialog.as_ref().unwrap().id.clone();
        app.mongo_dialog.as_mut().unwrap().name = "draft collection".into();
        app.mongo_reply(
            &c.id,
            &generation,
            None,
            Some(&token),
            Err(anyhow::anyhow!("fixture rename denied")),
        );
        assert_eq!(app.mongo_dialog.as_ref().unwrap().name, "draft collection");
        assert_eq!(std::fs::read(&path)?, bytes);
        app.mongo_disconnect(&c.id);
        assert!(app.mongo_hosts.is_empty());
        assert!(app.tabs.is_empty());
        Ok(())
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    use anyhow::{Result, ensure};
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn pump(app: &mut TomatoApp) -> Result<()> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let ctx = app.context.clone();
        loop {
            app.logic(&ctx);
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280., 900.),
                    )),
                    ..Default::default()
                },
                |ui| app.ui(ui),
            );
            output.textures_delta.clear();
            let pending = !app.pending.is_empty()
                || app.tabs.iter().any(|t| {
                    t.busy || matches!(&t.panel,Panel::Mongo {view,..} if !view.pending.is_empty())
                });
            if !pending {
                return Ok(());
            }
            ensure!(
                std::time::Instant::now() < deadline,
                "MongoDB GUI command pipeline timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    fn submit(app: &mut TomatoApp, id: &str, c: &Connection, action: Action) -> Result<()> {
        app.handle_intent(
            id,
            Intent::Command(Box::new(Command::Mongo {
                connection: c.clone(),
                action,
            })),
        );
        pump(app)
    }
    #[test]
    #[ignore = "Set TOMATO_TEST_MONGOD; isolated MongoDB plus real GUI command controller, no saved hosts"]
    fn real_mongo_tree_and_workbench_keep_failed_drafts_and_release_tabs() -> Result<()> {
        let executable =
            std::env::var_os("TOMATO_TEST_MONGOD").expect("TOMATO_TEST_MONGOD required");
        let root = tempfile::tempdir()?;
        let db = root.path().join("db");
        std::fs::create_dir(&db)?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        let mut command = std::process::Command::new(executable);
        command
            .args([
                "--bind_ip",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--wiredTigerCacheSizeGB",
                "0.25",
                "--setParameter",
                "diagnosticDataCollectionEnabled=false",
                "--dbpath",
            ])
            .arg(&db)
            .arg("--logpath")
            .arg(root.path().join("mongod.log"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let _child = Child(command.spawn()?);
        for _ in 0..200 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let mut c = Connection::new("MONGODB");
        c.set("host", "127.0.0.1");
        c.set("port", port);
        c.set("password", "");
        let path = root.path().join("connections.json");
        Store {
            connections: vec![c.clone()],
            ..Default::default()
        }
        .save(&path)?;
        let before = std::fs::read(&path)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app =
            TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(&path));
        app.open_connection(c.clone());
        pump(&mut app)?;
        assert!(app.mongo_hosts[&c.id].ready);
        assert!(app.tabs.is_empty());
        let target = Target {
            database: "tomato_fixture_ui".into(),
            collection: "items".into(),
        };
        app.mongo_request(
            &c.id,
            Action::CreateCollection {
                target: target.clone(),
            },
            None,
        );
        pump(&mut app)?;
        assert!(app.mongo_hosts[&c.id].databases.contains(&target.database));
        app.mongo_collections(&c.id, target.database.clone(), false);
        pump(&mut app)?;
        assert_eq!(
            app.mongo_hosts[&c.id].collections[&target.database][0]
                .table
                .name,
            "items"
        );
        app.mongo_collection(c.clone(), target.clone());
        pump(&mut app)?;
        let id = app.tabs[0].id.clone();
        submit(
            &mut app,
            &id,
            &c,
            Action::Insert {
                target: target.clone(),
                json: r#"{"name":"old"}"#.into(),
            },
        )?;
        let original = match &app.tabs[0].panel {
            Panel::Mongo { view, .. } => {
                assert_eq!(view.documents.len(), 1);
                view.original().unwrap().to_owned()
            }
            _ => unreachable!(),
        };
        let draft = r#"{"_id":1,"name":"invalid identity"}"#;
        if let Panel::Mongo { view, .. } = &mut app.tabs[0].panel {
            view.text = draft.into();
        }
        submit(
            &mut app,
            &id,
            &c,
            Action::Replace {
                target: target.clone(),
                original: original.clone(),
                json: draft.into(),
            },
        )?;
        assert!(app.tabs[0].error.is_some());
        assert!(matches!(&app.tabs[0].panel,Panel::Mongo {view,..} if view.text==draft));
        submit(
            &mut app,
            &id,
            &c,
            Action::Replace {
                target: target.clone(),
                original,
                json: r#"{"name":"new"}"#.into(),
            },
        )?;
        assert!(app.tabs[0].error.is_none());
        assert!(matches!(&app.tabs[0].panel,Panel::Mongo {view,..} if view.text.contains("new")));
        submit(
            &mut app,
            &id,
            &c,
            Action::CreateIndex {
                target: target.clone(),
                keys: r#"{"name":1}"#.into(),
                name: "ui_name".into(),
                unique: false,
            },
        )?;
        assert!(
            matches!(&app.tabs[0].panel,Panel::Mongo {view,..} if view.indexes.iter().any(|s|s.contains("ui_name")))
        );
        submit(
            &mut app,
            &id,
            &c,
            Action::UpdateValidator {
                target: target.clone(),
                json: r#"{"name":{"$type":"string"}}"#.into(),
            },
        )?;
        assert!(
            matches!(&app.tabs[0].panel,Panel::Mongo {view,..} if view.validator.contains("$type"))
        );
        let invalid = r#"{"name":17}"#;
        if let Panel::Mongo { view, .. } = &mut app.tabs[0].panel {
            view.text = invalid.into();
        }
        submit(
            &mut app,
            &id,
            &c,
            Action::Insert {
                target,
                json: invalid.into(),
            },
        )?;
        assert!(app.tabs[0].error.is_some());
        assert!(matches!(&app.tabs[0].panel,Panel::Mongo {view,..} if view.text==invalid));
        assert_eq!(std::fs::read(&path)?, before);
        app.mongo_disconnect(&c.id);
        assert!(app.mongo_hosts.is_empty());
        assert!(app.tabs.is_empty());
        Ok(())
    }
}
