use super::*;
use std::sync::Arc;
use tomato_services::redis_data::{Action, Output as RedisOutput};
pub(super) struct Host {
    pub connection: Arc<Connection>,
    pub databases: Vec<String>,
    pub ready: bool,
    pub busy: bool,
    pub generation: String,
}

impl TomatoApp {
    pub(super) fn prune_redis_hosts(&mut self) {
        let ids: Vec<_> = self
            .redis_hosts
            .keys()
            .filter(|id| {
                !self
                    .store
                    .connections
                    .iter()
                    .any(|c| &c.id == *id && c.kind() == "REDIS")
            })
            .cloned()
            .collect();
        for id in ids {
            self.redis_disconnect(&id);
        }
    }
    pub(super) fn redis_open(&mut self, connection: Connection, refresh: bool) {
        let id = connection.id.clone();
        let missing_password = connection
            .options
            .get("password")
            .is_none_or(serde_json::Value::is_null);
        let changed_endpoint = self.redis_hosts.get(&id).is_some_and(|h| {
            h.connection.text("host") != connection.text("host")
                || h.connection.number("port") != connection.number("port")
                || h.connection.text("username") != connection.text("username")
        });
        if missing_password && (!self.redis_hosts.contains_key(&id) || refresh && changed_endpoint)
        {
            if let Some(host) = self.redis_hosts.get_mut(&id) {
                host.ready = false;
                host.databases.clear();
                host.busy = false;
                host.generation = Connection::new("TOOL").id;
            }
            self.credential = Some(credentials::Prompt::new(connection));
            return;
        }
        if let Some(host) = self.redis_hosts.get(&id) {
            if host.busy {
                return;
            }
            if host.ready && !refresh {
                if !self.collapsed.remove(&id) {
                    self.collapsed.insert(id);
                }
                return;
            }
        }
        let host = self.redis_hosts.entry(id.clone()).or_insert_with(|| Host {
            connection: Arc::new(connection.clone()),
            databases: Vec::new(),
            ready: false,
            busy: false,
            generation: Connection::new("TOOL").id,
        });
        if refresh || !host.ready {
            let mut connection = connection;
            if connection
                .options
                .get("password")
                .is_none_or(serde_json::Value::is_null)
                && connection.text("host") == host.connection.text("host")
                && connection.text("username") == host.connection.text("username")
                && connection.number("port") == host.connection.number("port")
            {
                connection.set("password", host.connection.text("password"));
            }
            host.connection = Arc::new(connection);
            host.databases.clear();
            host.ready = false;
        }
        host.generation = Connection::new("TOOL").id;
        let generation = host.generation.clone();
        match self.queue.submit(Command::RedisData {
            connection: (*host.connection).clone(),
            action: Action::Databases,
        }) {
            Ok(request) => {
                host.busy = true;
                self.collapsed.remove(&id);
                self.pending.insert(
                    request,
                    Pending {
                        tab_id: String::new(),
                        purpose: Purpose::RedisTree {
                            connection: id,
                            generation,
                        },
                    },
                );
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }
    pub(super) fn redis_reply(
        &mut self,
        id: &str,
        generation: &str,
        result: anyhow::Result<Output>,
    ) {
        let Some(host) = self
            .redis_hosts
            .get_mut(id)
            .filter(|h| h.generation == generation)
        else {
            return;
        };
        host.busy = false;
        match result {
            Ok(Output::RedisData(RedisOutput::Databases(databases))) => {
                host.ready = true;
                host.databases = databases.into_iter().map(|n| format!("db{n}")).collect();
                self.collapsed.remove(id);
            }
            Err(error) => self.error = Some(format!("无法连接到Redis：{error:#}")),
            _ => {}
        }
    }
    pub(super) fn redis_database(&mut self, id: &str, database: u32) {
        let source = format!("redis_{id}_db{database}");
        if let Some(tab) = self.tabs.iter().find(|t| t.source_id == source) {
            self.active = Some(tab.id.clone());
            return;
        }
        let Some(host) = self.redis_hosts.get(id) else {
            return;
        };
        if host.connection.flag("redisCluster") && database != 0 {
            self.error = Some("Redis Cluster 仅支持 db0".into());
            return;
        }
        let connection = host.connection.clone();
        let tab = Connection::new("TOOL").id;
        self.tabs.push(Tab {
            id: tab.clone(),
            source_id: source,
            title: format!(
                "db{database}({}:{})-Redis",
                connection.text("host"),
                connection.number("port")
            ),
            panel: Panel::Redis {
                connection,
                view: Box::new(crate::tabs::redis::State {
                    database,
                    ..Default::default()
                }),
            },
            busy: false,
            error: None,
        });
        self.active = Some(tab);
    }
    pub(super) fn redis_disconnect(&mut self, id: &str) {
        if let Err(error) = self.queue.submit(Command::Disconnect {
            id: format!("redis:{id}"),
        }) {
            self.error = Some(format!("关闭 Redis 失败：{error:#}"));
            return;
        }
        let ids: Vec<_> = self
            .tabs
            .iter()
            .filter(|t| matches!(&t.panel,Panel::Redis {connection,..} if connection.id==id))
            .map(|t| t.id.clone())
            .collect();
        for id in ids {
            self.close(&id);
        }
        self.redis_hosts.remove(id);
        if self
            .credential
            .as_ref()
            .is_some_and(|p| p.connection.id == id)
        {
            self.credential = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn database_tree_uses_original_gray_icons_deduplicates_tabs_and_shares_runtime_credentials()
    -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("connections.json");
        let c = Connection::new("REDIS");
        Store {
            connections: vec![c.clone()],
            ..Default::default()
        }
        .save(&path)?;
        let before = std::fs::read(&path)?;
        let ctx = egui::Context::default();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let mut app =
            TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(&path));
        let mut transient = c.clone();
        transient.set("password", "runtime-only");
        app.redis_hosts.insert(
            c.id.clone(),
            Host {
                connection: Arc::new(transient),
                databases: vec!["db0".into(), "db1".into(), "db2".into()],
                ready: true,
                busy: false,
                generation: "current".into(),
            },
        );
        app.collapsed.remove(&c.id);
        let rows = |app: &TomatoApp, filter: &str| {
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
            .into_iter()
            .map(|r| (r.name().to_owned(), r.icon()))
            .collect::<Vec<_>>()
        };
        assert_eq!(rows(&app, "").len(), 4);
        assert_eq!(
            rows(&app, "db2")
                .iter()
                .map(|r| r.0.as_str())
                .collect::<Vec<_>>(),
            [c.name.as_str(), "db2"]
        );
        assert_eq!(rows(&app, "")[1].1, "images/connect/database_gray.png");
        let mut frame = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200., 900.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.connection_list(ui),
            );
            output.textures_delta.clear();
        };
        frame(Vec::new());
        frame(Vec::new());
        let point = egui::pos2(85., 52.);
        frame(vec![
            egui::Event::PointerMoved(point),
            egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        frame(vec![egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Secondary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert_eq!(
            app.selected_connection.as_deref(),
            Some(format!("schema:{}:db0", c.id).as_str())
        );
        app.redis_open(c.clone(), false);
        assert_eq!(rows(&app, "").len(), 1);
        app.redis_open(c.clone(), false);
        assert_eq!(rows(&app, "").len(), 4);
        app.redis_database(&c.id, 1);
        app.redis_database(&c.id, 2);
        app.redis_database(&c.id, 1);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active.as_deref(), Some(app.tabs[0].id.as_str()));
        match (&app.tabs[0].panel, &app.tabs[1].panel) {
            (
                Panel::Redis {
                    connection: a,
                    view: a_view,
                },
                Panel::Redis {
                    connection: b,
                    view: b_view,
                },
            ) => {
                assert!(Arc::ptr_eq(a, b));
                assert_eq!(a.text("password"), "runtime-only");
                assert_eq!((a_view.database, b_view.database), (1, 2));
            }
            _ => panic!("wrong panels"),
        }
        app.redis_reply(
            &c.id,
            "obsolete",
            Ok(Output::RedisData(RedisOutput::Databases(vec![99]))),
        );
        assert_eq!(app.redis_hosts[&c.id].databases.len(), 3);
        assert_eq!(rows(&app, "")[1].1, "images/connect/database_gray.png");
        assert_eq!(std::fs::read(&path)?, before);
        let mut changed = c.clone();
        changed.set("port", c.number("port") + 1);
        changed.set("password", serde_json::Value::Null);
        app.redis_open(changed, true);
        assert!(app.credential.is_some());
        assert!(!app.redis_hosts[&c.id].ready);
        app.redis_reply(
            &c.id,
            "current",
            Ok(Output::RedisData(RedisOutput::Databases(vec![99]))),
        );
        assert!(app.redis_hosts[&c.id].databases.is_empty());
        app.redis_disconnect(&c.id);
        assert!(app.tabs.is_empty());
        assert!(app.redis_hosts.is_empty());
        assert!(app.credential.is_none());
        Ok(())
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn pump(app: &mut TomatoApp, ctx: &egui::Context) -> anyhow::Result<()> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            app.logic(ctx);
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200., 900.),
                    )),
                    ..Default::default()
                },
                |ui| app.ui(ui),
            );
            output.textures_delta.clear();
            let pending=!app.pending.is_empty() || app.tabs.iter().any(|t|t.busy || matches!(&t.panel,Panel::Redis {view,..} if !view.loaded || view.followup.is_some()));
            if !pending {
                return Ok(());
            }
            anyhow::ensure!(
                std::time::Instant::now() < deadline,
                "Redis GUI command controller timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    #[test]
    #[ignore = "Set TOMATO_TEST_REDIS_SERVER; isolated four-database Redis and real GUI tree/workbench command pipeline"]
    fn real_redis_tree_and_two_database_tabs_keep_values_isolated_and_release_on_disconnect()
    -> anyhow::Result<()> {
        let executable = std::env::var_os("TOMATO_TEST_REDIS_SERVER")
            .expect("Redis fixture executable required");
        let root = tempfile::tempdir()?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        let mut command = std::process::Command::new(executable);
        command
            .current_dir(root.path())
            .args([
                "--bind",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--save",
                "",
                "--appendonly",
                "no",
                "--maxmemory",
                "32mb",
                "--databases",
                "4",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let _child = Child(command.spawn()?);
        for _ in 0..100 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let mut c = Connection::new("REDIS");
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
        pump(&mut app, &ctx)?;
        assert!(app.tabs.is_empty());
        assert_eq!(
            app.redis_hosts[&c.id].databases,
            ["db0", "db1", "db2", "db3"]
        );
        for (database, text) in [(1, "database one 中文"), (2, "database two 中文")] {
            app.redis_database(&c.id, database);
            pump(&mut app, &ctx)?;
            let id = app.active.clone().unwrap();
            let mut connection = c.clone();
            connection.set("redisDatabase", database);
            app.handle_intent(
                &id,
                Intent::Command(Box::new(Command::RedisData {
                    connection,
                    action: Action::Write {
                        key: "same".into(),
                        kind: "string".into(),
                        text: text.into(),
                        create: false,
                    },
                })),
            );
            pump(&mut app, &ctx)?;
            let tab = app.tabs.iter().find(|t| t.id == id).unwrap();
            assert!(tab.error.is_none());
            assert!(
                matches!(&tab.panel,Panel::Redis {view,..} if view.text==text && view.database==database)
            );
        }
        assert_eq!(app.tabs.len(), 2);
        assert!(
            matches!(&app.tabs[0].panel,Panel::Redis {view,..} if view.text=="database one 中文")
        );
        assert!(
            matches!(&app.tabs[1].panel,Panel::Redis {view,..} if view.text=="database two 中文")
        );
        app.redis_database(&c.id, 1);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(std::fs::read(&path)?, before);
        app.redis_disconnect(&c.id);
        assert!(app.tabs.is_empty());
        assert!(app.redis_hosts.is_empty());
        Ok(())
    }
}
