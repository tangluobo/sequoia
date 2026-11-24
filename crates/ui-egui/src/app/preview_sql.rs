//! Opt-in framebuffer fixture, backed by the real connection actor and an in-memory DB.
use super::*;
impl TomatoApp {
    pub(super) fn preview_sql_results(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("结果夹具必须使用隔离目录"))?;
        anyhow::ensure!(
            profile.file_name().is_some_and(|name| name
                .to_string_lossy()
                .starts_with("tomato-sql-results-fixture-"))
                && self.store.connections.is_empty(),
            "结果夹具必须使用独立空配置"
        );
        let mut connection = Connection::new("SQLITE");
        connection.name = "SQL 结果验收".into();
        connection.set("queryRowLimit", 2000);
        self.store.connections.push(connection.clone());
        self.open_connection(connection.clone());
        let id = self
            .active
            .clone()
            .ok_or_else(|| anyhow::anyhow!("没有查询标签"))?;
        self.submit(&id,Command::Sql {connection,sql:"WITH RECURSIVE seq(n) AS (SELECT 0 UNION ALL SELECT n+1 FROM seq WHERE n<1999) SELECT NULL AS missing, '' AS empty, 'NULL' AS literal, '第 '||n||' 行' AS 中文字段 FROM seq".into()},Purpose::Normal);
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-sql-results-fixture"), true));
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("tomato-sql-results-footer"),
                std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("sql_results_footer"),
            )
        });
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("tomato-sql-filter-fixture"),
                std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("sql_results_filter"),
            )
        });
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("tomato-sql-temporal-fixture"),
                matches!(
                    std::env::var("TOMATO_UI_PREVIEW").as_deref(),
                    Ok("sql_results_date" | "sql_results_time" | "sql_results_datetime")
                ),
            )
        });
        Ok(())
    }
    pub(super) fn sql_results_preview(&mut self, ctx: &egui::Context) {
        if !ctx
            .data(|data| data.get_temp::<bool>(egui::Id::new("tomato-sql-results-fixture")))
            .unwrap_or(false)
        {
            return;
        }
        let Some(Panel::Sql {
            results,
            workbench,
            connection,
            ..
        }) = self
            .tabs
            .iter_mut()
            .find(|tab| Some(&tab.id) == self.active.as_ref())
            .map(|tab| &mut tab.panel)
        else {
            return;
        };
        let id = egui::ViewportId::from_hash_of("sql-result-fixture");
        let footer = ctx
            .data(|data| data.get_temp::<bool>(egui::Id::new("tomato-sql-results-footer")))
            .unwrap_or(false);
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-viewport"), id));
        ctx.show_viewport_immediate(id,egui::ViewportBuilder::default().with_title("查询结果验收").with_inner_size(egui::vec2(700.,450.)).with_resizable(false).with_active(false),|ui,_| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            if let Some(result) = results.first() {
                workbench.query_results.resize_with(results.len(),Default::default);
                if result.rows.len()==2000 && ctx.data(|data|data.get_temp::<bool>(egui::Id::new("tomato-sql-filter-fixture"))).unwrap_or(false) && !ctx.data(|data|data.get_temp::<bool>(egui::Id::new("tomato-sql-filter-fixture-opened"))).unwrap_or(false) {
                    workbench.query_results[0].initialize(result);
                    workbench.query_results[0].view.dialog=Some(crate::tabs::query_results::view::Dialog {column:3,input:"原字段值".into(),editor:Default::default(),focus:true});
                    ctx.data_mut(|data|data.insert_temp(egui::Id::new("tomato-sql-filter-fixture-opened"),true));
                }
                if result.rows.len()==2000 && ctx.data(|data|data.get_temp::<bool>(egui::Id::new("tomato-sql-temporal-fixture"))).unwrap_or(false) && !ctx.data(|data|data.get_temp::<bool>(egui::Id::new("tomato-sql-temporal-fixture-opened"))).unwrap_or(false) {
                    use crate::tabs::query_results::{editing::Editing,temporal::Picker,temporal_model::Kind,Cell};
                    let kind=match std::env::var("TOMATO_UI_PREVIEW").as_deref(){Ok("sql_results_date")=>Kind::Date,Ok("sql_results_time")=>Kind::Time,_=>Kind::DateTime};
                    let state=&mut workbench.query_results[0];state.initialize(result);
                    state.editing=Some(Box::new(Editing::new(tomato_services::database::query_rows::Editing {table:tomato_core::workbench::TableId {schema:String::new(),name:"isolated_ui_fixture".into()},plan:tomato_core::query_edit::Plan {database_column_names:result.columns.clone(),editable_column_indexes:vec![3],primary_key_columns:vec![result.columns[3].clone()],uses_original_row_match:false,status_text:"隔离编辑器夹具".into()},comments:Default::default()})));
                    state.start_edit(result,Cell {row:0,column:3});let editor=state.editing.as_mut().unwrap().editor.as_mut().unwrap();
                    editor.text=match kind{Kind::Date=>"2024-02-29",Kind::Time=>"12:34:56.75",Kind::DateTime=>"2024-02-29 12:34:56.75"}.into();
                    editor.temporal=Some(Box::new(Picker::new(kind,&editor.text,egui::pos2(100.,100.))));
                    ctx.data_mut(|data|data.insert_temp(egui::Id::new("tomato-sql-temporal-fixture-opened"),true));
                }
                if footer {crate::panels::render_query_results_footer(ui,result,&mut workbench.query_results[0],connection,egui::Id::new("sql-result-native-fixture"));}else{crate::panels::render_query_results(ui,result,&mut workbench.query_results[0],egui::Id::new("sql-result-native-fixture"));}
                if let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from) {
                    let _ = std::fs::write(path.with_extension("state.json"),serde_json::json!({"ready":result.rows.len()==2000,"footer":footer,"clientWidth":ui.max_rect().width(),"clientHeight":ui.max_rect().height(),"rows":result.rows.len(),"columns":result.columns,"nullFirst":result.is_null(0,0),"emptySecond":result.value(0,1),"literalThird":result.value(0,2),"lastValue":result.value(1999,3),"columnWidths":workbench.query_results[0].widths}).to_string());
                }
            } else {ui.spinner();ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));}
            if let Some(image) = ui.input(|input|input.events.iter().find_map(|event|if let egui::Event::Screenshot {viewport_id,image,..} = event && *viewport_id==id {Some(image.clone())} else {None})) {
                ui.ctx().data_mut(|data|data.insert_temp(egui::Id::new("tomato-capture-image"),image));
            }
        });
    }
    pub(super) fn preview_query_dialog(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("查询夹具必须使用隔离目录"))?;
        anyhow::ensure!(
            profile
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("tomato-query-fixture-"))
                && self.store.connections.is_empty(),
            "查询夹具必须使用独立空配置"
        );
        let mut connection = Connection::new("SQLITE");
        connection.name = "查询文件验收".into();
        self.store.connections.push(connection.clone());
        self.open_connection(connection);
        let id = self
            .active
            .clone()
            .ok_or_else(|| anyhow::anyhow!("没有查询标签"))?;
        self.queries.dialog = Some(query_tree::Dialog::save(id));
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-query-dialog-fixture"), true));
        Ok(())
    }
    pub(super) fn preview_sql_completion(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("SQL fixture requires isolated profile"))?;
        anyhow::ensure!(
            profile.file_name().is_some_and(|name| name
                .to_string_lossy()
                .starts_with("tomato-sql-completion-fixture-")),
            "SQL fixture profile name mismatch"
        );
        anyhow::ensure!(
            self.store.connections.is_empty(),
            "SQL fixture requires an empty profile"
        );
        let mut connection = Connection::new("SQLITE");
        connection.name = "SQL 补全验收".into();
        // First job in this connection's queue seeds its real session. Its result
        // is intentionally unrouted; the fixture asserts the subsequent live catalog.
        self.queue.submit(Command::Sql {
            connection: connection.clone(),
            sql: "CREATE TABLE Users(id INTEGER, name TEXT)".into(),
        })?;
        self.store.connections.push(connection.clone());
        self.open_connection(connection);
        if let Some(tab) = self.tabs.last_mut()
            && let Panel::Sql { sql, .. } = &mut tab.panel
        {
            sql.clear();
        }
        self.section = 0;
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-sql-completion-fixture"), true));
        Ok(())
    }
}
