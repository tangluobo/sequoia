//! Drive the real connection_list controls; mutations go through the production queue.
use super::*;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tomato_core::workbench::TableId;

pub(in crate::app) struct Driver {
    pub app: TomatoApp,
    pub directory: tempfile::TempDir,
    pub local: Connection,
    pub sql: Connection,
    time: f64,
    _listener: Option<std::net::TcpListener>,
}
impl Driver {
    pub fn new() -> anyhow::Result<Self> {
        let directory = tempfile::tempdir()?;
        let files = directory.path().join("files");
        std::fs::create_dir_all(files.join("folder/nested"))?;
        std::fs::write(files.join("first.md"), b"# actual bytes")?;
        std::fs::write(files.join("second.txt"), b"unrelated")?;
        std::fs::write(files.join("folder/nested/inside.md"), b"# nested bytes")?;
        let mut local = Connection::new("LOCAL_DIRECTORY");
        local.name = "local_fixture".into();
        local.set("localDirectoryPath", files.to_string_lossy().as_ref());
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let mut sql = Connection::new("MYSQL");
        sql.name = "sql_fixture".into();
        sql.set("host", "127.0.0.1");
        sql.set("port", u64::from(listener.local_addr()?.port()));
        sql.set("database", "owned");
        let mut group = Connection::new("SSH");
        group.kind = None;
        group.name = "group_fixture".into();
        group.set("unknownLegacy", serde_json::json!({"kept":true}));
        let config = directory.path().join("connections.json");
        let repo = ConnectionFileRepository::new(&config);
        let mut store = repo.load()?;
        store.connections = vec![group, local.clone(), sql.clone()];
        repo.save(&store)?;
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let mut app = TomatoApp::with_repository(&ctx, runtime, repo);
        app.open_connection(local.clone());
        let mut host = sql_tree::Host::new(sql.clone());
        host.schemas = Arc::new(vec!["owned".into()]);
        host.tables.insert(
            "owned".into(),
            Arc::new(vec![
                TableInfo {
                    table: TableId {
                        schema: "owned".into(),
                        name: "table_first".into(),
                    },
                    view: false,
                },
                TableInfo {
                    table: TableId {
                        schema: "owned".into(),
                        name: "view_first".into(),
                    },
                    view: true,
                },
            ]),
        );
        app.sql_hosts.insert(sql.id.clone(), host);
        app.collapsed.clear();
        let mut driver = Self {
            app,
            directory,
            local,
            sql,
            time: 10.,
            _listener: Some(listener),
        };
        driver.drain()?;
        driver.frame(vec![]);
        driver.drain()?;
        driver.frame(vec![]);
        Ok(driver)
    }
    pub fn wrap(app: TomatoApp, directory: tempfile::TempDir, local: Connection) -> Self {
        Self {
            app,
            directory,
            local,
            sql: Connection::new("SQLITE"),
            time: 10.,
            _listener: None,
        }
    }
    pub fn frame(&mut self, mut events: Vec<Event>) -> egui::FullOutput {
        self.frame_focus(std::mem::take(&mut events), true)
    }
    pub fn frame_focus(&mut self, mut events: Vec<Event>, focused: bool) -> egui::FullOutput {
        self.time += 0.02;
        for event in &mut events {
            if let Event::PointerButton { modifiers, .. } | Event::Key { modifiers, .. } = event {
                modifiers.command |= modifiers.ctrl;
            }
        }
        let modifiers = events
            .iter()
            .find_map(|e| match e {
                Event::PointerButton { modifiers, .. } | Event::Key { modifiers, .. } => {
                    Some(*modifiers)
                }
                _ => None,
            })
            .unwrap_or_default();
        events.insert(0, Event::ModifiersChanged(modifiers));
        let ctx = self.app.context.clone();
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(320., 1050.),
                )),
                time: Some(self.time),
                events,
                focused,
                ..Default::default()
            },
            |ui| self.app.connection_list(ui),
        );
        out.textures_delta.clear();
        out
    }
    pub fn idle(&mut self, seconds: f64) {
        self.time += seconds;
        self.frame(vec![]);
    }
    pub fn point(&mut self, text: &str) -> Pos2 {
        self.frame(vec![])
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == text => {
                    Some(t.pos + t.galley.size() / 2.)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("actual tree label missing: {text}"))
    }
    pub fn click(&mut self, p: Pos2, button: PointerButton, modifiers: Modifiers) {
        for pressed in [true, false] {
            self.frame(vec![
                Event::PointerMoved(p),
                Event::PointerButton {
                    pos: p,
                    button,
                    pressed,
                    modifiers,
                },
            ]);
        }
    }
    pub fn select(&mut self, text: &str) -> Pos2 {
        self.idle(0.7);
        let p = self.point(text);
        self.click(p, PointerButton::Primary, Modifiers::NONE);
        p
    }
    pub fn start(&mut self, text: &str) {
        let p = self.select(text);
        if self
            .app
            .tree_rename
            .as_ref()
            .is_some_and(|d| d.due.is_some())
        {
            self.idle(0.31);
            self.assert_editor();
            return;
        }
        assert!(self.app.tree_rename.is_none(), "first click only selects");
        self.idle(0.7);
        self.click(p, PointerButton::Primary, Modifiers::NONE);
        assert!(
            self.app
                .tree_rename
                .as_ref()
                .is_some_and(|d| d.due.is_some()),
            "second click must schedule {text}"
        );
        self.idle(0.31);
        self.assert_editor();
    }
    pub fn assert_editor(&self) {
        let d = self.app.tree_rename.as_ref().expect("inline name draft");
        assert!(d.due.is_none());
        assert!(!egui::Popup::is_any_open(&self.app.context));
        assert!(
            self.app.queries.dialog.is_none()
                && self.app.backup_edit.is_none()
                && self.app.tree_objects.is_none()
        );
        let rect = self
            .app
            .context
            .data(|v| v.get_temp::<egui::Rect>(egui::Id::new(("tree-name-field", &d.key))))
            .expect("actual field geometry");
        assert!(
            (rect.height() - 26.).abs() < 0.1 && rect.width() > 60.,
            "fixed height, useful width: {rect:?}"
        );
        assert!(
            self.app.context.memory(|m| m.has_focus(d.id())),
            "name field focus"
        );
    }
    pub fn type_name(&mut self, name: &str) {
        self.frame(vec![Event::Text(name.into())]);
        assert_eq!(
            self.app.tree_rename.as_ref().unwrap().name,
            name,
            "old name must be selected"
        );
    }
    pub fn key(&mut self, key: Key) {
        self.frame(vec![Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
    }
    pub fn drain(&mut self) -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.app.pending.is_empty() {
            self.app.logic(&self.app.context.clone());
            anyhow::ensure!(
                Instant::now() < deadline,
                "owned sidebar actor timed out: {:?}",
                self.app.error
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }
}

#[test]
fn sidebar_file_folder_table_view_and_connection_names_use_real_inline_controls_without_moving_rows()
-> anyhow::Result<()> {
    let mut d = Driver::new()?;
    let before = std::fs::read(d.directory.path().join("connections.json"))?;
    for label in [
        "first.md",
        "folder",
        "table_first",
        "view_first",
        "local_fixture",
        "group_fixture",
    ] {
        let other = d.point("second.txt");
        d.start(label);
        assert_eq!(
            d.point("second.txt"),
            other,
            "editing must not change row geometry"
        );
        let id = d.app.tree_rename.as_ref().unwrap().id();
        d.type_name("中文 long replacement name");
        d.key(Key::Escape);
        assert!(d.app.tree_rename.is_none());
        assert!(
            egui::TextEdit::load_state(&d.app.context, id).is_none(),
            "release undo on cancellation"
        );
        d.point(label);
    }
    assert_eq!(
        std::fs::read(d.directory.path().join("connections.json"))?,
        before
    );
    Ok(())
}
#[test]
fn sidebar_context_menu_and_f2_share_inline_edit_for_every_resource_kind() -> anyhow::Result<()> {
    let mut d = Driver::new()?;
    for label in [
        "first.md",
        "folder",
        "table_first",
        "view_first",
        "group_fixture",
    ] {
        let p = d.select(label);
        d.click(p, PointerButton::Secondary, Modifiers::NONE);
        let rename = d.point("重命名");
        d.click(rename, PointerButton::Primary, Modifiers::NONE);
        d.frame(vec![]);
        d.assert_editor();
        d.key(Key::Escape);
        d.select(label);
        d.key(Key::F2);
        d.frame(vec![]);
        d.assert_editor();
        d.key(Key::Escape);
    }
    Ok(())
}
#[test]
fn sidebar_rename_keeps_double_click_disclosure_icons_and_multi_selection_separate()
-> anyhow::Result<()> {
    let mut d = Driver::new()?;
    let p = d.select("first.md");
    d.idle(0.7);
    d.click(
        p - egui::vec2(35., 0.),
        PointerButton::Primary,
        Modifiers::NONE,
    );
    d.idle(0.4);
    assert!(d.app.tree_rename.is_none(), "icon is not a rename label");
    d.select("first.md");
    let second = d.point("second.txt");
    d.click(second, PointerButton::Primary, Modifiers::CTRL);
    d.idle(0.4);
    assert_eq!(d.app.local_selection.len(), 2);
    d.key(Key::F2);
    d.idle(0.4);
    assert!(d.app.tree_rename.is_none());
    let p = d.select("first.md");
    d.idle(0.7);
    d.click(p, PointerButton::Primary, Modifiers::NONE);
    d.click(p, PointerButton::Primary, Modifiers::NONE);
    d.idle(0.4);
    assert!(d.app.tree_rename.is_none());
    assert!(
        d.app
            .tabs
            .iter()
            .any(|t| matches!(t.panel, Panel::Editor { .. })),
        "double click still opens the file"
    );
    Ok(())
}
#[test]
fn sidebar_file_rename_enter_blur_and_batched_clicks_commit_real_bytes_and_preserve_config()
-> anyhow::Result<()> {
    for finish_by in ["enter", "blur", "batched", "window-blur"] {
        let mut d = Driver::new()?;
        let config = d.directory.path().join("connections.json");
        let before = std::fs::read(&config)?;
        d.start("first.md");
        d.type_name("renamed.md");
        match finish_by {
            "enter" => d.key(Key::Enter),
            "blur" => {
                let p = d.point("second.txt");
                d.click(p, PointerButton::Primary, Modifiers::NONE);
            }
            "batched" => {
                let p = d.point("second.txt");
                d.frame(vec![
                    Event::PointerMoved(p),
                    Event::PointerButton {
                        pos: p,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    },
                    Event::PointerButton {
                        pos: p,
                        button: PointerButton::Primary,
                        pressed: false,
                        modifiers: Modifiers::NONE,
                    },
                ]);
            }
            _ => {
                d.frame_focus(vec![], false);
            }
        }
        assert!(d.app.tree_rename.is_none());
        d.drain()?;
        assert_eq!(
            std::fs::read(d.directory.path().join("files/renamed.md"))?,
            b"# actual bytes"
        );
        assert!(!d.directory.path().join("files/first.md").exists());
        d.point("renamed.md");
        assert_eq!(std::fs::read(&config)?, before);
    }
    Ok(())
}
#[test]
fn sidebar_folder_rename_rebases_expanded_descendants_and_open_editor_paths() -> anyhow::Result<()>
{
    let mut d = Driver::new()?;
    let root = d.directory.path().join("files");
    d.app
        .local_tree_open(&d.local, Some(root.join("folder")), false);
    d.drain()?;
    d.app
        .local_tree_open(&d.local, Some(root.join("folder").join("nested")), false);
    d.drain()?;
    d.app.open_tree_editor(
        d.local.clone(),
        root.join("folder").join("nested").join("inside.md"),
    );
    d.drain()?;
    assert!(
        !d.app
            .collapsed
            .contains(&local_tree::key(&d.local.id, &root.join("folder"))),
        "parent expanded before rename"
    );
    assert!(
        !d.app.collapsed.contains(&local_tree::key(
            &d.local.id,
            &root.join("folder").join("nested")
        )),
        "child expanded before rename"
    );
    d.point("inside.md");
    d.start("folder");
    d.type_name("renamed_folder");
    d.key(Key::Enter);
    d.drain()?;
    assert!(
        !d.app
            .collapsed
            .contains(&local_tree::key(&d.local.id, &root.join("renamed_folder"))),
        "parent stays expanded"
    );
    assert!(
        !d.app.collapsed.contains(&local_tree::key(
            &d.local.id,
            &root.join("renamed_folder").join("nested")
        )),
        "child stays expanded: {:?}",
        d.app.collapsed
    );
    d.point("renamed_folder");
    d.point("nested");
    d.point("inside.md");
    assert_eq!(
        std::fs::read(root.join("renamed_folder").join("nested").join("inside.md"))?,
        b"# nested bytes"
    );
    assert!(d.app.tabs.iter().any(|t|matches!(&t.panel,Panel::Editor{path,..}if path==&root.join("renamed_folder").join("nested").join("inside.md"))));
    assert!(
        !d.app.local_trees[&d.local.id]
            .directories
            .keys()
            .any(|p| p.starts_with(root.join("folder")))
    );
    Ok(())
}

#[test]
fn sidebar_query_and_backup_files_and_folders_commit_through_their_real_repositories()
-> anyhow::Result<()> {
    let mut d = Driver::new()?;
    let scope = tomato_core::queries::Scope {
        connection: d.sql.name.clone(),
        database: "owned".into(),
        path: String::new(),
    };
    let root = d.directory.path().join(scope.directory()?);
    std::fs::create_dir_all(root.join("query_folder"))?;
    std::fs::write(
        d.directory.path().join(scope.file("stored_query")?),
        b"SELECT 'kept';",
    )?;
    d.app
        .queries
        .trees
        .entry(d.sql.id.clone())
        .or_default()
        .insert(
            "owned".into(),
            query_tree::Tree {
                epoch: d.app.sql_hosts[&d.sql.id].epoch.clone(),
                root: Some(scope),
                directories: Default::default(),
                error: None,
            },
        );
    d.app.query_action(query_tree::Action::Expand {
        connection: d.sql.id.clone(),
        namespace: "owned".into(),
        path: String::new(),
    });
    let scope = tomato_core::backup::Scope {
        connection: d.sql.name.clone(),
        database: "owned".into(),
        path: String::new(),
    };
    let root = d.directory.path().join(scope.directory()?);
    std::fs::create_dir_all(root.join("backup_folder"))?;
    let archive = include_bytes!("../../../../services/tests/fixtures/backup/original.nb3");
    std::fs::write(root.join("stored_backup.nb3"), archive)?;
    d.app.list_backups(&d.sql.id, scope)?;
    d.app
        .collapsed
        .remove(&query_tree::key(&d.sql.id, "owned", "", None));
    d.app
        .collapsed
        .remove(&backup_tree::Key::new(&d.sql.id, "owned", "").tree_id());
    d.drain()?;
    let before = std::fs::read(d.directory.path().join("connections.json"))?;
    for (old, new) in [
        ("stored_query", "query_renamed"),
        ("query_folder", "query_folder_renamed"),
        ("stored_backup", "backup_renamed"),
        ("backup_folder", "backup_folder_renamed"),
    ] {
        d.start(old);
        d.type_name(new);
        d.key(Key::Enter);
        d.drain()?;
        d.point(new);
        assert!(d.app.queries.dialog.is_none() && d.app.backup_edit.is_none());
    }
    let scope = tomato_core::queries::Scope {
        connection: d.sql.name.clone(),
        database: "owned".into(),
        path: String::new(),
    };
    assert_eq!(
        std::fs::read(d.directory.path().join(scope.file("query_renamed")?))?,
        b"SELECT 'kept';"
    );
    assert!(
        d.directory
            .path()
            .join(scope.directory()?)
            .join("query_folder_renamed")
            .is_dir()
    );
    assert_eq!(std::fs::read(root.join("backup_renamed.nb3"))?, archive);
    assert!(root.join("backup_folder_renamed").is_dir());
    assert_eq!(
        std::fs::read(d.directory.path().join("connections.json"))?,
        before
    );
    Ok(())
}
#[test]
fn sidebar_group_name_save_preserves_legacy_fields_and_rejects_external_config_changes()
-> anyhow::Result<()> {
    let mut d = Driver::new()?;
    let config = d.directory.path().join("connections.json");
    let ids = d
        .app
        .store
        .connections
        .iter()
        .map(|c| c.id.clone())
        .collect::<Vec<_>>();
    d.start("group_fixture");
    d.type_name("group_renamed");
    d.key(Key::Enter);
    let store = ConnectionFileRepository::new(&config).load()?;
    assert_eq!(store.connections[0].name, "group_renamed");
    assert_eq!(
        store.connections[0].options["unknownLegacy"],
        serde_json::json!({"kept":true})
    );
    assert_eq!(
        store
            .connections
            .iter()
            .map(|c| c.id.clone())
            .collect::<Vec<_>>(),
        ids
    );
    d.start("group_renamed");
    d.type_name("conflicting_name");
    std::fs::write(&config, b"external update")?;
    d.key(Key::Enter);
    assert_eq!(std::fs::read(config)?, b"external update");
    assert_eq!(d.app.store.connections[0].name, "group_renamed");
    assert!(d.app.error.is_some());
    Ok(())
}
#[test]
fn sidebar_table_multi_selection_and_stale_filter_cannot_rename_a_different_resource()
-> anyhow::Result<()> {
    let mut d = Driver::new()?;
    d.select("table_first");
    let p = d.point("view_first");
    d.click(p, PointerButton::Primary, Modifiers::CTRL);
    assert_eq!(d.app.sql_selection.len(), 2);
    d.key(Key::F2);
    d.idle(0.4);
    assert!(d.app.tree_rename.is_none());
    d.start("first.md");
    d.type_name("unsent.md");
    let id = d.app.tree_rename.as_ref().unwrap().id();
    d.app.filter = "second".into();
    d.frame(vec![]);
    assert!(d.app.tree_rename.is_none());
    assert!(egui::TextEdit::load_state(&d.app.context, id).is_none());
    assert!(d.directory.path().join("files/first.md").exists());
    assert!(!d.directory.path().join("files/unsent.md").exists());
    Ok(())
}
