//! Own inactive designer viewport, matching the unchanged Java scene fixture.
use super::*;
mod index_input;
mod menu_input;
mod selection_input;
mod type_input;
impl TomatoApp {
    pub(super) fn preview_table_designer(&mut self) -> anyhow::Result<()> {
        use crate::tabs::table_design::{Cell, Draft, Page};
        use tomato_core::table_design::{Dialect, Metadata};
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("设计器夹具需要隔离目录"))?;
        anyhow::ensure!(
            profile
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("tomato-designer-fixture-"))
                && self.store.connections.is_empty(),
            "设计器夹具需要独立空配置"
        );
        let mut tab = tabs::create_tab(Connection::new("MYSQL"), None)?;
        let Panel::Sql { workbench, .. } = &mut tab.panel else {
            unreachable!()
        };
        let index = std::env::var("TOMATO_DESIGNER_PAGE")
            .unwrap_or_else(|_| "0".into())
            .parse::<usize>()?;
        if (7..=9).contains(&index) {
            use crate::tabs::database_objects::window::{Draft as Browser, Mode};
            use tomato_core::{
                database_objects::{Catalog, ForeignKey, Object},
                workbench::TableId,
            };
            let table = |name: &str| TableId {
                schema: "sample_db".into(),
                name: name.into(),
            };
            let objects = [
                (
                    "orders",
                    false,
                    "订单",
                    "3",
                    "2024-02-29 12:34:56",
                    "16.00 KB",
                    "2",
                ),
                ("users", false, "用户", "2", "", "16.00 KB", "1"),
                ("active_users", true, "用户视图", "", "", "", ""),
            ]
            .into_iter()
            .map(
                |(name, view, comment, auto_increment, update_time, data_length, rows)| Object {
                    table: table(name),
                    view,
                    comment: comment.into(),
                    engine: if view { "" } else { "InnoDB" }.into(),
                    auto_increment: auto_increment.into(),
                    update_time: update_time.into(),
                    data_length: data_length.into(),
                    rows: rows.into(),
                    fields: Vec::new(),
                },
            )
            .collect();
            let catalog = std::sync::Arc::new(Catalog {
                objects,
                foreign_keys: vec![ForeignKey {
                    name: "fk_user".into(),
                    table: table("orders"),
                    columns: vec!["user_id".into()],
                    referenced: table("users"),
                    referenced_columns: vec!["id".into()],
                }],
            });
            let mut browser = Browser::default();
            browser.receive_shared(catalog.clone());
            browser.mode = [Mode::Icon, Mode::Detail, Mode::Er][index - 7];
            workbench.objects = Some(catalog);
            workbench.browser = Some(Box::new(browser));
            self.active = Some(tab.id.clone());
            self.tabs.push(tab);
            return Ok(());
        }
        let mut draft = Draft::new("MYSQL", "sample_db".into())?;
        let mut metadata = Metadata::empty(Dialect::Mysql);
        metadata.version = Some("8.4.11".into());
        draft.receive_metadata(std::sync::Arc::new(metadata));
        draft.request.columns[0].name = "id".into();
        draft.request.columns[0].data_type = "BIGINT".into();
        for bit in [0, 1, 2] {
            draft.set_flag(0, bit, true);
        }
        if matches!(
            std::env::var("TOMATO_DESIGNER_GRID").as_deref(),
            Ok("multi" | "large")
        ) {
            let status = draft.status.clone();
            for (name, data_type, length) in
                [("name", "VARCHAR", "255"), ("created_at", "TIMESTAMP", "")]
            {
                draft.perform(&self.context, crate::tabs::table_design::Action::Add);
                draft.commit_edit(&self.context);
                let column = draft.request.columns.last_mut().unwrap();
                column.name = name.into();
                column.data_type = data_type.into();
                column.length = length.into();
            }
            if std::env::var("TOMATO_DESIGNER_GRID").as_deref() == Ok("large") {
                for row in 3..512 {
                    draft.append();
                    let column = draft.request.columns.last_mut().unwrap();
                    column.name = format!("column_{row}");
                    column.data_type = "INT".into();
                }
            }
            draft.status = status;
        }
        draft.select(Cell { row: 0, column: 0 }, false, false);
        let page = *[
            Page::Fields,
            Page::Indexes,
            Page::ForeignKeys,
            Page::Triggers,
            Page::Options,
            Page::Comment,
            Page::Sql,
        ]
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("设计器夹具页无效"))?;
        if page == Page::Indexes {
            use tomato_core::table_design::indexes::{Field, Index, Kind};
            for name in ["name", "created_at"] {
                draft.append();
                let column = draft.request.columns.last_mut().unwrap();
                column.name = name.into();
                column.data_type = "VARCHAR".into();
                column.length = "100".into();
            }
            draft.request.indexes = vec![
                Index {
                    name: "ix_name".into(),
                    fields: vec![Field {
                        name: "name".into(),
                        ..Default::default()
                    }],
                    method: "BTREE".into(),
                    ..Default::default()
                },
                Index {
                    name: "uq_created".into(),
                    fields: vec![Field {
                        name: "created_at".into(),
                        ..Default::default()
                    }],
                    method: "BTREE".into(),
                    kind: Kind::Unique,
                    ..Default::default()
                },
            ];
        }
        if matches!(
            std::env::var("TOMATO_DESIGNER_AUX_FORM").as_deref(),
            Ok("1" | "saved")
        ) && matches!(page, Page::ForeignKeys | Page::Triggers)
        {
            use tomato_core::table_design::alter::InfoPage;
            let (i, p) = if page == Page::ForeignKeys {
                (1, InfoPage::ForeignKeys)
            } else {
                (2, InfoPage::Triggers)
            };
            draft.info[i].add(&[], p, "sample_db");
            draft.info[i].rows.as_mut().unwrap()[0] = if i == 1 {
                vec![
                    "fk_orders_customer".into(),
                    "id".into(),
                    "sample_db".into(),
                    "customers".into(),
                    "id".into(),
                    "CASCADE".into(),
                    "NO ACTION".into(),
                ]
            } else {
                vec![
                    "tr_orders_insert".into(),
                    "BEFORE".into(),
                    "INSERT".into(),
                    "BEGIN\n    SET NEW.created_at = NOW();\nEND".into(),
                ]
            };
        }
        if std::env::var("TOMATO_DESIGNER_AUX_FORM").as_deref() == Ok("saved")
            && matches!(page, Page::ForeignKeys | Page::Triggers)
        {
            use tomato_core::table_design::alter::{InfoPage, InfoTable, Snapshot};
            let (index, info_page) = if page == Page::ForeignKeys {
                (1, InfoPage::ForeignKeys)
            } else {
                (2, InfoPage::Triggers)
            };
            let info = InfoTable {
                headers: tomato_core::table_design::auxiliary::headers(info_page),
                rows: draft.info[index].rows.take().unwrap_or_default(),
                ..Default::default()
            };
            draft.info[index].added.clear();
            draft.info_loaded[index] = true;
            draft.request.table.name = "sample_table".into();
            draft.saved = Some(Box::new(Snapshot {
                definition: draft.request.clone(),
                extra_options: Default::default(),
                backslash_escapes: true,
                raw_columns: Vec::new(),
                indexes: InfoTable::default(),
                foreign_keys: if index == 1 {
                    info.clone()
                } else {
                    InfoTable::default()
                },
                triggers: if index == 2 {
                    info
                } else {
                    InfoTable::default()
                },
                sql: String::new(),
                view: false,
                primary_name: Some("PRIMARY".into()),
            }));
        }
        draft.switch_page(&self.context, page);
        draft.dirty = false;
        workbench.designer = Some(Box::new(draft));
        self.active = Some(tab.id.clone());
        self.tabs.push(tab);
        Ok(())
    }
    pub(super) fn table_designer_preview(&mut self, ctx: &egui::Context) {
        if std::env::var("TOMATO_UI_PREVIEW").as_deref() != Ok("table_designer")
            || std::env::var_os("TOMATO_UI_CAPTURE").is_none()
        {
            return;
        }
        let Some(index) = self
            .tabs
            .iter()
            .position(|tab| Some(&tab.id) == self.active.as_ref())
        else {
            return;
        };
        menu_input::install(ctx, &self.tabs[index].id);
        if type_input::install(ctx) || selection_input::install(ctx) || index_input::install(ctx) {
            // Only this opt-in, empty-profile fixture may discard synthetic edits on capture exit.
            self.quit_approved = true;
        }
        let id = egui::ViewportId::from_hash_of("table-designer-fixture");
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-viewport"), id));
        ctx.show_viewport_immediate(id,egui::ViewportBuilder::default().with_title("表设计器验收").with_inner_size(egui::vec2(896.,650.)).with_resizable(false).with_active(false),|ui,_| {
            crate::panels::render_with_connections(ui,&mut self.tabs[index],&mut self.icons,&self.store.connections);
            type_input::record(ctx, &self.tabs[index]);
            index_input::record(ctx, &self.tabs[index]);
            selection_input::record(ui, &self.tabs[index]);
            if let Some(path)=std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from) {
                let _=std::fs::write(path.with_extension("state.json"),serde_json::json!({"ready":true,"clientWidth":ui.max_rect().width(),"clientHeight":ui.max_rect().height(),"databaseRequested":false,"systemClipboardWritten":false}).to_string());
            }
            if let Some(image)=ui.input(|input|input.events.iter().find_map(|event|if let egui::Event::Screenshot {viewport_id,image,..}=event&&*viewport_id==id {Some(image.clone())}else {None})) {ctx.data_mut(|data|data.insert_temp(egui::Id::new("tomato-capture-image"),image));}
        });
    }
}
