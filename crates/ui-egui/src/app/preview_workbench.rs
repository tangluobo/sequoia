//! Own inactive viewport; no database connection, persisted config or clipboard access.
use super::*;
mod menu_input;
impl TomatoApp {
    pub(super) fn preview_sql_workbench(&mut self) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("工作台夹具需要隔离目录"))?;
        anyhow::ensure!(
            profile.file_name().is_some_and(|name| name
                .to_string_lossy()
                .starts_with("tomato-sql-workbench-fixture-"))
                && self.store.connections.is_empty(),
            "工作台夹具需要独立空配置"
        );
        let mut connection = Connection::new("MYSQL");
        connection.name.clear();
        let mut tab = tabs::create_tab(connection, None)?;
        if let Panel::Sql { sql, workbench, .. } = &mut tab.panel {
            *sql = "SELECT id, name\nFROM Users\nWHERE id > 0;\n-- 中文查询\n".into();
            workbench.schema.clear();
            if std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("table_data") {
                use tomato_core::workbench::*;
                let definition: TableDefinition = serde_json::from_value(serde_json::json!({
                    "table":{"schema":"","name":"items"}, "view":false,"transactional":true,
                    "columns":(["missing","empty","literal","中文字段"].map(|name| serde_json::json!({
                        "name":name,"data_type":"TEXT","nullable":true,"default":null,
                        "primary_key":0,"generated":false,"auto_increment":false,"binary":false,
                        "type_schema":"","type_name":"TEXT"})))
                }))?;
                workbench.table = Some(definition.table.clone());
                workbench.definition = Some(definition.clone());
                workbench.page = Some(TablePage {
                    definition,
                    rows: (0..1000)
                        .map(|i| {
                            vec![
                                Cell::Null,
                                Cell::Text(String::new()),
                                Cell::Text("NULL".into()),
                                Cell::Text(format!("第 {i} 行")),
                            ]
                        })
                        .collect(),
                    total: 2000,
                    offset: 0,
                    limit: 1000,
                    truncated: false,
                    query: Default::default(),
                    sql: tomato_services::database::page_sql(
                        "MYSQL",
                        workbench.definition.as_ref().unwrap(),
                        &workbench.request().unwrap(),
                    )?,
                });
                if matches!(
                    std::env::var("TOMATO_TABLE_STATE").as_deref(),
                    Ok("filter" | "sort")
                ) {
                    use tomato_core::result_filter::{Condition, Operator};
                    workbench.table_view.query_draft = Some(TableQuery {
                        filters: vec![ColumnFilter {
                            column: "中文字段".into(),
                            condition: Condition {
                                operator: Operator::Like,
                                expected: Some("第 1500 行".into()),
                            },
                        }],
                        sorts: vec![
                            Sort {
                                column: "中文字段".into(),
                                descending: true,
                            },
                            Sort {
                                column: "literal".into(),
                                descending: false,
                            },
                        ],
                        ..Default::default()
                    });
                }
                if std::env::var("TOMATO_TABLE_STATE").as_deref() == Ok("editing") {
                    workbench.table_view.widths = vec![80., 80., 80., 160.];
                    workbench.table_view.cells.insert((1, 3));
                    workbench.table_view.focus = Some((1, 3));
                    workbench.editor = Some(crate::tabs::workbench::CellEditor::new(
                        1,
                        3,
                        Some(&Cell::Text("第 1 行".into())),
                        false,
                    ));
                }
                if std::env::var("TOMATO_TABLE_STATE").as_deref() == Ok("selected") {
                    workbench.table_view.cells.insert((1, 2));
                    workbench.table_view.anchor = Some((1, 2));
                    workbench.table_view.focus = Some((1, 2));
                    workbench.selected.insert(1);
                }
            }
        }
        self.active = Some(tab.id.clone());
        if std::env::var("TOMATO_WORKBENCH_STATE").is_ok_and(|state| state.starts_with("script")) {
            use tomato_services::database::script::{Script, Statement};
            if let Panel::Sql {
                sql,
                results,
                workbench,
                ..
            } = &mut tab.panel
            {
                *sql = "SELECT 1 AS id, '订单' AS name;\nSELECT 2 AS id, '用户' AS name;".into();
                let mut script = Script {
                    source: sql.clone(),
                    success_count: 2,
                    ..Default::default()
                };
                for index in 0..2 {
                    results.push(tomato_services::QueryResult {
                        columns: vec!["id".into(), "name".into()],
                        rows: vec![vec![
                            (index + 1).to_string(),
                            ["订单", "用户"][index].into(),
                        ]],
                        has_result_set: true,
                        total_rows: 1,
                        ..Default::default()
                    });
                    script.statements.push(Statement {
                        range: 0..sql.len(),
                        select: true,
                        source_table: None,
                        elapsed_ms: 1,
                        success: true,
                        error: None,
                        result: Some(index),
                        has_result_set: true,
                        row_count: 1,
                        affected: None,
                        truncated: false,
                        editing: None,
                        edit_error: None,
                    });
                }
                workbench.script = Some(Box::new(script));
                workbench.script_view =
                    if std::env::var("TOMATO_WORKBENCH_STATE").as_deref() == Ok("script_info") {
                        0
                    } else {
                        1
                    };
            }
        }
        self.tabs.push(tab);
        Ok(())
    }
    pub(super) fn sql_workbench_preview(&mut self, ctx: &egui::Context) {
        if !matches!(
            std::env::var("TOMATO_UI_PREVIEW").as_deref(),
            Ok("sql_workbench" | "table_data")
        ) || std::env::var_os("TOMATO_UI_CAPTURE").is_none()
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
        menu_input::install(ctx);
        let id = egui::ViewportId::from_hash_of("sql-workbench-fixture");
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-viewport"), id));
        ctx.show_viewport_immediate(id,egui::ViewportBuilder::default().with_title("SQL 工作台验收").with_inner_size(egui::vec2(896.,if std::env::var("TOMATO_UI_PREVIEW").as_deref()==Ok("table_data"){650.}else{700.})).with_resizable(false).with_active(false),|ui,_| {
            ui.spacing_mut().item_spacing=egui::Vec2::ZERO;
            // The same editor must not render twice in one egui context/frame.
            crate::panels::render_with_connections(ui,&mut self.tabs[index],&mut self.icons,&self.store.connections);
            if let Panel::Sql {workbench,sql,..}=&self.tabs[index].panel&&let Some(path)=std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from) {
                let _=std::fs::write(path.with_extension("state.json"),serde_json::json!({"clientWidth":ui.max_rect().width(),"clientHeight":ui.max_rect().height(),"ready":true,"rowCount":workbench.completion.galley.as_ref().map(|g|g.rows.len()),"textX":workbench.completion.galley_pos.x,"textY":workbench.completion.galley_pos.y,"divider":workbench.editor_split,"documentUnchanged":sql=="SELECT id, name\nFROM Users\nWHERE id > 0;\n-- 中文查询\n","databaseRequested":false,"systemClipboardWritten":false,"selectedRows":workbench.selected.iter().copied().collect::<Vec<_>>(),"selectedCells":workbench.table_view.cells.iter().collect::<Vec<_>>(),"rowMode":workbench.table_view.row_mode,"contextMenuRect":ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new("table-context-menu-geometry"))).map(|rect|[rect.left(),rect.top(),rect.width(),rect.height()])}).to_string());
            }
            if let Some(image)=ui.input(|input|input.events.iter().find_map(|event|if let egui::Event::Screenshot {viewport_id,image,..}=event&&*viewport_id==id {Some(image.clone())}else{None})){ctx.data_mut(|data|data.insert_temp(egui::Id::new("tomato-capture-image"),image));}
        });
    }
}
