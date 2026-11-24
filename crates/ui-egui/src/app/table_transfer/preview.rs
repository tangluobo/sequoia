use super::*;
pub(super) fn active() -> bool {
    std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("table_transfer")
        && std::env::var_os("TOMATO_UI_CAPTURE").is_some()
}
pub(super) fn capture(ui: &Ui, id: egui::ViewportId) {
    if !active() {
        return;
    }
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
    pub(in crate::app) fn preview_table_transfer(&mut self) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("需要隔离夹具目录"))?;
        anyhow::ensure!(
            profile
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("tomato-transfer-fixture-")),
            "传输夹具目录无效"
        );
        let step = std::env::var("TOMATO_TRANSFER_STEP")
            .unwrap_or_else(|_| "0".into())
            .parse::<usize>()?;
        anyhow::ensure!(step <= 1);
        let mut source = Connection::new("MYSQL");
        source.name = "本地数据库".into();
        source.set("host", "127.0.0.1");
        source.set("port", 3306);
        source.set("database", "sample_db");
        source.set("username", "root");
        let mut target = source.clone();
        target.id = Connection::new("MYSQL").id;
        target.name = "目标数据库".into();
        target.set("database", "sample_target");
        let request = Request {
            source: source.clone(),
            target: target.clone(),
            source_schema: "sample_db".into(),
            target_schema: "sample_target".into(),
            tables: vec!["users".into()],
            structure: true,
            data: true,
            drop_existing: false,
            add: true,
            drop: true,
            modify: true,
        };
        let preview="-- users\nCREATE TABLE `sample_target`.`users` (\n  `id` bigint NOT NULL,\n  `name` varchar(255) DEFAULT NULL,\n  PRIMARY KEY (`id`)\n);\n".into();
        self.table_transfer = Some(Draft {
            id: Connection::new("TOOL").id,
            generation: 0,
            request,
            databases: vec!["sample_db".into(), "sample_target".into()],
            tables: vec![("orders".into(), false), ("users".into(), true)],
            step,
            loading: false,
            running: false,
            completed: false,
            plan: Some(Plan {
                tables: vec![tomato_core::table_transfer::TablePlan {
                    source: tomato_core::workbench::TableId {
                        schema: "sample_db".into(),
                        name: "users".into(),
                    },
                    target: tomato_core::workbench::TableId {
                        schema: "sample_target".into(),
                        name: "users".into(),
                    },
                    sql: vec![String::new()],
                    data: true,
                    data_sql: None,
                    stamp: String::new(),
                }],
            }),
            preview,
            error: None,
            log: String::new(),
            control: Default::default(),
            options: false,
            option_values: [true, true, false],
            editors: Default::default(),
            layout: Default::default(),
            scroll_areas: [None; 2],
        });
        self.store.connections = vec![source, target];
        Ok(())
    }
}
