use super::*;
#[derive(Clone)]
pub(super) enum Resume {
    Open,
    SshTree {
        operation: tomato_services::terminal::ssh::tools::Operation,
    },
    StorageCommand {
        tab: String,
        command: Box<Command>,
    },
    SqlConnection {
        tab: String,
    },
    DatabaseAdmin {
        database: String,
        mode: super::database_admin::Mode,
    },
    Transfer {
        id: String,
        generation: u64,
        source: bool,
    },
    PasteTables(Box<tomato_core::table_transfer::Request>),
}
pub(super) struct Prompt {
    pub connection: Connection,
    password: String,
    remember: bool,
    focus: bool,
    resume: Resume,
    editor: crate::tabs::text_state::Editor,
}
impl Prompt {
    pub fn new(connection: Connection) -> Self {
        Self {
            connection,
            password: String::new(),
            remember: false,
            focus: true,
            resume: Resume::Open,
            editor: Default::default(),
        }
    }
    pub fn then(connection: Connection, resume: Resume) -> Self {
        Self {
            resume,
            ..Self::new(connection)
        }
    }
}
impl TomatoApp {
    pub(super) fn credential_dialog(&mut self, ctx: &egui::Context) {
        let Some(prompt) = &mut self.credential else {
            return;
        };
        let object = matches!(
            prompt.connection.kind(),
            "S3" | "ALIYUN_OSS" | "LOCAL_DIRECTORY" | "ALIYUN" | "TENCENT_CLOUD"
        );
        let (mut accept, mut cancel) = (false, false);
        egui::Modal::new(egui::Id::new("connection-credential-prompt"))
            .backdrop_color(Color32::TRANSPARENT)
            .show(ctx, |ui| {
                ui.set_width(380.);
                ui.heading(if object {
                    "输入Secret Key"
                } else {
                    "输入密码"
                });
                let host = if object {
                    let endpoint = prompt.connection.text("endpoint");
                    if endpoint.is_empty() {
                        prompt.connection.text("region")
                    } else {
                        endpoint
                    }
                } else {
                    prompt.connection.text("host")
                };
                ui.label(format!(
                    "{} ({}@{})",
                    prompt.connection.name,
                    prompt.connection.text("username"),
                    host
                ));
                ui.add_space(16.);
                egui::Grid::new("connection-credential-fields")
                    .spacing([10., 10.])
                    .show(ui, |ui| {
                        ui.label(if object { "Secret Key：" } else { "密码：" });
                        let id = prompt
                            .editor
                            .prepare_id(ctx, ui.id().with("credential-password"));
                        let field = ui.add_sized(
                            [250., 28.],
                            TextEdit::singleline(&mut prompt.password)
                                .id(id)
                                .password(true),
                        );
                        prompt.editor.trim(ctx);
                        if prompt.focus {
                            field.request_focus();
                            prompt.focus = false;
                        }
                        if (field.has_focus() || field.lost_focus())
                            && !prompt.password.is_empty()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            accept = true;
                        }
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(
                            &mut prompt.remember,
                            if object {
                                "保存密钥"
                            } else {
                                "保存密码"
                            },
                        );
                        ui.end_row();
                    });
                ui.add_space(16.);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_sized([80., 28.], egui::Button::new("取消"))
                        .clicked()
                    {
                        cancel = true;
                    }
                    if ui
                        .add_enabled(
                            !prompt.password.is_empty(),
                            crate::theme::green_button("确定").min_size(egui::vec2(80., 28.)),
                        )
                        .clicked()
                    {
                        accept = true;
                    }
                });
                cancel |= ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            });
        if cancel {
            if let Some(prompt) = self.credential.take() {
                prompt.editor.release(ctx);
                if let Resume::StorageCommand { tab, command } = prompt.resume
                    && matches!(*command, Command::Certificate { .. })
                    && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab)
                    && let Panel::Cloud { view, .. } = &mut tab.panel
                {
                    view.control = None;
                }
            }
        } else if accept && let Some(prompt) = self.credential.take() {
            match self.supplied_credentials(&prompt.connection, &prompt.password, prompt.remember) {
                Ok(connection) => {
                    prompt.editor.release(ctx);
                    self.resume_credentials(connection, prompt.resume.clone());
                }
                Err(error) => {
                    self.error = Some(format!("{error:#}"));
                    self.credential = Some(prompt);
                }
            }
        }
    }
    pub(super) fn sql_password_missing(&self, connection: &Connection) -> bool {
        tomato_services::database::authentication_requires_password(connection)
            && !self.sql_hosts.contains_key(&connection.id)
            && connection
                .options
                .get("password")
                .is_none_or(serde_json::Value::is_null)
    }
    fn resume_credentials(&mut self, connection: Connection, resume: Resume) {
        match resume {
            Resume::StorageCommand { tab, mut command } => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab)
                    && let Panel::Cloud { view, .. } = &mut tab.panel
                    && let Some(dialog) = &mut view.certificate
                {
                    dialog.request.storage = connection.clone();
                }
                match command.as_mut() {
                    Command::CloudBuckets {
                        connection: storage,
                    } => *storage = connection,
                    Command::Certificate { request, .. } => request.storage = connection,
                    _ => return,
                }
                self.submit(&tab, *command, Purpose::Normal);
            }
            Resume::Open => self.open_connection(connection),
            Resume::SshTree { operation } => self.ssh_tree_request(connection, operation),
            Resume::SqlConnection { tab } => self.apply_sql_connection(&tab, connection),
            Resume::DatabaseAdmin { database, mode } => {
                self.open_database_admin(connection, database, mode)
            }
            Resume::Transfer {
                id,
                generation,
                source,
            } => self.resume_transfer_credentials(&id, generation, connection, source),
            Resume::PasteTables(mut request) => {
                request.source.set("password", connection.text("password"));
                request.target.set("password", connection.text("password"));
                self.start_direct_paste(*request);
            }
        }
    }
    pub(super) fn supplied_credentials(
        &mut self,
        connection: &Connection,
        password: &str,
        remember: bool,
    ) -> anyhow::Result<Connection> {
        anyhow::ensure!(!password.is_empty(), "密码不能为空");
        let mut session = connection.clone();
        session.set("password", password);
        if remember {
            anyhow::ensure!(!self.load_failed, "配置加载失败，保存已禁用");
            let mut store = self.store.clone();
            let saved = store
                .connections
                .iter_mut()
                .find(|c| c.id == connection.id)
                .ok_or_else(|| anyhow::anyhow!("连接已被删除"))?;
            saved.set("password", password);
            saved.set("savePassword", true);
            self.repository.save(&store)?;
            self.store = store;
            session.set("savePassword", true);
        }
        Ok(session)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn certificate_bucket_loading_resumes_with_a_session_key_without_modifying_legacy_config()
    -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("connections.json");
        Store::default().save(&path)?;
        let before = std::fs::read(&path)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let listener = runtime.block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))?;
        let address = listener.local_addr()?;
        let server = runtime.spawn(async move {
            let app = axum::Router::new().fallback(|request: axum::extract::Request| async move {
                assert!(request.headers()["authorization"].to_str().unwrap().contains("fixture-key"));
                "<ListAllMyBucketsResult><Buckets><Bucket><Name>certificate-bucket</Name></Bucket></Buckets></ListAllMyBucketsResult>"
            });
            axum::serve(listener, app).await.unwrap();
        });
        let ctx = egui::Context::default();
        let mut app =
            TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(&path));
        let mut storage = Connection::new("S3");
        storage.set("endpoint", address.to_string());
        storage.set("username", "fixture-key");
        app.store.connections.push(storage.clone());
        let mut tab = tabs::create_tab(Connection::new("ALIYUN"), None)?;
        let id = tab.id.clone();
        if let Panel::Cloud { view, .. } = &mut tab.panel {
            view.loaded = true;
            view.certificate("owned.test", "owned.test", "owner@owned.test");
            let dialog = view.certificate.as_mut().unwrap();
            dialog.selected_storage = storage.id.clone();
            dialog.buckets_for = storage.id.clone();
        }
        app.tabs.push(tab);
        app.submit(
            &id,
            Command::CloudBuckets {
                connection: storage.clone(),
            },
            Purpose::Normal,
        );
        let prompt = app
            .credential
            .take()
            .expect("unsaved key must prompt before network I/O");
        assert!(!app.tabs[0].busy);
        let session = app.supplied_credentials(&prompt.connection, "fixture-secret", false)?;
        app.resume_credentials(session, prompt.resume);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while matches!(&app.tabs[0].panel, Panel::Cloud { view, .. }
            if view.certificate.as_ref().unwrap().bucket_task.is_some())
        {
            anyhow::ensure!(
                std::time::Instant::now() < deadline,
                "Bucket fixture timed out"
            );
            app.logic(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(app.tabs[0].error.is_none(), "{:?}", app.tabs[0].error);
        let Panel::Cloud { view, .. } = &app.tabs[0].panel else {
            unreachable!()
        };
        let dialog = view.certificate.as_ref().unwrap();
        assert_eq!(dialog.buckets, ["certificate-bucket"]);
        assert_eq!(dialog.request.bucket, "certificate-bucket");
        assert_eq!(dialog.request.storage.text("password"), "fixture-secret");
        assert!(dialog.bucket_task.is_none());
        assert!(app.store.connections[0].text("password").is_empty());
        assert_eq!(std::fs::read(path)?, before);
        server.abort();
        Ok(())
    }
    #[test]
    fn temporary_passwords_stay_in_the_session_and_remember_uses_the_encrypted_repository()
    -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("connections.json");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let mut app = TomatoApp::with_repository(
            &egui::Context::default(),
            runtime,
            ConnectionFileRepository::new(&path),
        );
        let mut connection = Connection::new("S3");
        connection.set("username", "fixture-key");
        connection.set("savePassword", false);
        app.store.connections.push(connection.clone());
        app.open_connection(connection.clone());
        assert!(app.credential.is_some());
        assert!(app.tabs.is_empty());
        let session = app.supplied_credentials(&connection, "fixture-secret", false)?;
        assert_eq!(session.text("password"), "fixture-secret");
        assert!(app.store.connections[0].text("password").is_empty());
        assert!(!path.exists());
        app.supplied_credentials(&connection, "fixture-secret", true)?;
        let encrypted = std::fs::read_to_string(&path)?;
        assert!(encrypted.starts_with(tomato_core::legacy::MARKER));
        assert!(!encrypted.contains("fixture-secret"));
        let reloaded = Store::load(&path)?;
        assert_eq!(reloaded.connections[0].text("password"), "fixture-secret");
        assert!(reloaded.connections[0].flag("savePassword"));
        std::fs::write(&path, "external update")?;
        assert!(
            app.supplied_credentials(&connection, "must-not-save", true)
                .is_err()
        );
        assert_eq!(std::fs::read_to_string(path)?, "external update");
        Ok(())
    }
}
