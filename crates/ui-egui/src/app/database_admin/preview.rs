use super::*;
pub(super) fn active() -> bool {
    std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("database_admin")
        && std::env::var_os("TOMATO_UI_CAPTURE").is_some()
}
pub(super) fn capture(ui: &Ui) {
    if !active() {
        return;
    }
    let id = ui.ctx().viewport_id();
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-viewport"), id));
    if let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from) {
        let _=std::fs::write(path.with_extension("state.json"),serde_json::json!({"ready":true,"clientWidth":ui.max_rect().width(),"clientHeight":ui.max_rect().height(),"databaseRequested":false,"systemClipboardWritten":false}).to_string());
    }
    if let Some(image) = ui.input(|i| {
        i.events.iter().find_map(|event| {
            if let egui::Event::Screenshot {
                viewport_id, image, ..
            } = event
                && *viewport_id == id
            {
                Some(image.clone())
            } else {
                None
            }
        })
    }) {
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-image"), image));
    }
}
impl TomatoApp {
    pub(in crate::app) fn preview_database_admin(&mut self) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("需要隔离配置"))?;
        anyhow::ensure!(
            profile
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("tomato-admin-fixture-"))
                && self.store.connections.is_empty(),
            "数据库对话框夹具需要独立空配置"
        );
        let mode = if std::env::var("TOMATO_ADMIN_MODE").as_deref() == Ok("edit") {
            Mode::Edit
        } else {
            Mode::Create
        };
        let page = std::env::var("TOMATO_ADMIN_PAGE")
            .unwrap_or_else(|_| "0".into())
            .parse::<usize>()?;
        anyhow::ensure!(page <= 1);
        let connection = Connection::new("MYSQL");
        self.database_admin = Some(Draft {
            id: Connection::new("TOOL").id,
            connection,
            mode,
            database: "sample_db".into(),
            charset: if mode == Mode::Edit {
                "utf8mb4".into()
            } else {
                String::new()
            },
            collation: if mode == Mode::Edit {
                "utf8mb4_0900_ai_ci".into()
            } else {
                String::new()
            },
            info: Info {
                charsets: vec![(
                    "utf8mb4".into(),
                    vec!["utf8mb4_0900_ai_ci".into(), "utf8mb4_general_ci".into()],
                )],
                ..Default::default()
            },
            page,
            loading: false,
            running: false,
            focus: false,
            error: None,
            owner: None,
            editors: Default::default(),
        });
        Ok(())
    }
}
