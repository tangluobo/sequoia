//! Synthetic objects only; the preview never dispatches database or filesystem commands.
use super::*;
pub(super) fn active() -> bool {
    matches!(
        std::env::var("TOMATO_UI_PREVIEW").as_deref(),
        Ok("backup" | "restore" | "restore_confirm" | "backup_complete" | "backup_error")
    ) && std::env::var_os("TOMATO_UI_CAPTURE").is_some()
}
impl TomatoApp {
    pub(crate) fn preview_backup(&mut self) -> anyhow::Result<()> {
        let directory = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("备份夹具需要隔离目录"))?;
        anyhow::ensure!(
            directory
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("tomato-backup-fixture-"))
                && self.store.connections.is_empty(),
            "备份夹具需要独立空配置"
        );
        let mut connection = Connection::new("MYSQL");
        connection.name = "本地数据库".into();
        let scope = tomato_core::backup::Scope {
            connection: connection.name.clone(),
            database: "sample_db".into(),
            path: String::new(),
        };
        let mut draft = Draft::new(connection, scope, &self.context);
        draft.loading = false;
        draft.tab = std::env::var("TOMATO_UI_BACKUP_TAB")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0)
            .min(3);
        use tomato_core::backup::{Kind, Selection};
        draft.groups = selection::groups(
            [
                (Kind::Table, "users"),
                (Kind::Table, "orders"),
                (Kind::View, "active_users"),
                (Kind::Function, "total_orders"),
                (Kind::Event, "daily_cleanup"),
            ]
            .map(|(kind, name)| Selection {
                kind,
                table: tomato_core::workbench::TableId {
                    schema: "sample_db".into(),
                    name: name.into(),
                },
            })
            .into(),
        );
        if matches!(
            std::env::var("TOMATO_UI_PREVIEW").as_deref(),
            Ok("restore" | "restore_confirm")
        ) {
            draft.restore = Some("20261018120000".into());
            draft.tab = draft.tab.min(1);
            if std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("restore_confirm") {
                draft.confirm_restore = true;
                draft.confirm_focus = true;
            }
        }
        match std::env::var("TOMATO_UI_PREVIEW").as_deref() {
            Ok("backup_complete") => {
                draft.complete = Some("备份文件已保存:\nC:\\backup\\sample.nb3".into())
            }
            Ok("backup_error") => {
                draft.error = Some("错误: 对象名称已存在".into());
                draft.notice = draft.error.clone();
            }
            _ => {}
        }
        self.backup_dialog = Some(draft);
        Ok(())
    }
}
pub(super) fn capture(ui: &egui::Ui, viewport: egui::ViewportId, draft: &Draft) {
    if !active() {
        return;
    }
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-viewport"), viewport));
    if let Some(image) = ui.input(|input| {
        input.events.iter().find_map(|event| {
            if let egui::Event::Screenshot {
                viewport_id, image, ..
            } = event
                && *viewport_id == viewport
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
    if let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from) {
        let bounds = ui
            .ctx()
            .data(|data| {
                data.get_temp::<[egui::Rect; 2]>(egui::Id::new("tomato-backup-button-bounds"))
            })
            .map(|rects| rects.map(|rect| [rect.left(), rect.top(), rect.width(), rect.height()]));
        let _=std::fs::write(path.with_extension("state.json"),serde_json::json!({"ready":true,"clientWidth":ui.max_rect().width(),"clientHeight":ui.max_rect().height(),"tab":draft.tab,"objectCount":draft.total(),"selectedCount":draft.objects().len(),"buttons":bounds,"databaseRequested":false,"systemClipboardWritten":false}).to_string());
    }
}
pub(super) fn capture_question(
    ui: &egui::Ui,
    viewport: egui::ViewportId,
    bounds: [egui::Rect; 2],
    object_count: usize,
) {
    if std::env::var("TOMATO_UI_PREVIEW").as_deref() != Ok("restore_confirm") || !active() {
        return;
    }
    capture_alert(ui, viewport, &bounds, object_count);
}
pub(super) fn capture_alert(
    ui: &egui::Ui,
    viewport: egui::ViewportId,
    bounds: &[egui::Rect],
    object_count: usize,
) {
    if !matches!(
        std::env::var("TOMATO_UI_PREVIEW").as_deref(),
        Ok("restore_confirm" | "backup_complete" | "backup_error")
    ) || !active()
    {
        return;
    }
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-viewport"), viewport));
    if let Some(image) = ui.input(|input| {
        input.events.iter().find_map(|event| {
            if let egui::Event::Screenshot {
                viewport_id, image, ..
            } = event
                && *viewport_id == viewport
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
    if let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from) {
        let origin = ui.max_rect().min;
        let buttons: Vec<_> = bounds
            .iter()
            .map(|rect| {
                [
                    rect.left() - origin.x,
                    rect.top() - origin.y,
                    rect.width(),
                    rect.height(),
                ]
            })
            .collect();
        let _=std::fs::write(path.with_extension("state.json"),serde_json::json!({"ready":true,"clientWidth":ui.max_rect().width(),"clientHeight":ui.max_rect().height(),"tab":0,"objectCount":object_count,"selectedCount":0,"buttons":buttons,"databaseRequested":false,"systemClipboardWritten":false}).to_string());
    }
}
