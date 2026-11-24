use super::*;
mod ai_progress;
#[cfg(test)]
mod alert_tests;
mod alerts;
mod batch_input;
mod completion;
mod copy_drag;
mod designer;
mod editor_menu;
#[cfg(test)]
mod editor_menu_tests;
#[cfg(test)]
mod format_tests;
mod highlight;
mod information;
mod layout;
#[cfg(test)]
mod layout_tests;
mod objects;
mod query_edit;
pub(super) mod results;
#[cfg(test)]
mod results_projection_tests;
#[cfg(test)]
mod results_tests;
mod script;
mod table;
#[cfg(test)]
mod temporal_cell_tests;
#[cfg(test)]
mod temporal_tests;
mod toolbar;
pub(super) fn render_results(
    ui: &mut Ui,
    result: &tomato_services::QueryResult,
    state: &mut crate::tabs::query_results::State,
    id: egui::Id,
) {
    results::render(ui, result, state, id);
}
pub(super) fn render_results_footer(
    ui: &mut Ui,
    result: &tomato_services::QueryResult,
    state: &mut crate::tabs::query_results::State,
    connection: &tomato_core::Connection,
    id: egui::Id,
) {
    let _ = query_edit::render(
        ui,
        result,
        state,
        query_edit::Context {
            connection,
            schema: "",
            index: 0,
            busy: false,
            id,
        },
    );
}

#[cfg(test)]
pub(super) fn render(ui: &mut Ui, tab: &mut Tab) -> Option<Intent> {
    render_with_connections(ui, tab, &mut crate::icons::Icons::default(), &[])
}
pub(super) fn render_with_connections(
    ui: &mut Ui,
    tab: &mut Tab,
    icons: &mut crate::icons::Icons,
    catalog: &[tomato_core::Connection],
) -> Option<Intent> {
    if let Panel::Sql { workbench, .. } = &mut tab.panel
        && workbench.browser.is_none()
        && workbench.designer.is_none()
    {
        if let Some(error) = tab.error.take() {
            workbench.failure = Some(crate::tabs::workbench::Failure {
                text: error,
                focus: true,
            });
        }
        alerts::show(ui.ctx(), &tab.id, workbench, icons);
    }
    if matches!(&tab.panel, Panel::Sql {workbench,..} if workbench.browser.is_some()) {
        let Panel::Sql { workbench, .. } = &mut tab.panel else {
            unreachable!()
        };
        let state = workbench.browser.as_mut().unwrap();
        return objects::render(
            ui,
            state,
            icons,
            objects::Context {
                id: &tab.id,
                busy: tab.busy,
                error: tab.error.as_deref(),
            },
        )
        .map(Intent::Objects);
    }
    if matches!(&tab.panel,Panel::Sql{workbench,..}if workbench.designer.is_some()) {
        let id = tab.id.clone();
        return ui
            .push_id(("table-designer", id), |ui| {
                designer::render(ui, tab, icons)
            })
            .inner;
    }
    if matches!(&tab.panel, Panel::Sql { workbench, .. } if workbench.table.is_some()) {
        return table::render(ui, tab);
    }
    let Panel::Sql {
        connection,
        sql,
        results,
        selected,
        workbench,
        ..
    } = &mut tab.panel
    else {
        return None;
    };
    ui.spacing_mut().item_spacing.y = 0.;
    let mut intent = None;
    let mut formatted = false;
    let editable = !workbench
        .document
        .as_ref()
        .is_some_and(|document| document.loading || document.close_after_save);
    let database_names = workbench.selectors.databases.clone();
    let databases = if database_names.is_empty() {
        workbench.completion.catalog().databases.as_slice()
    } else {
        database_names.as_slice()
    };
    if let Some(action) = toolbar::render(
        ui,
        &mut workbench.selectors,
        icons,
        toolbar::Context {
            connection,
            namespace: &workbench.schema,
            catalog,
            databases,
            ai_pending: workbench.ai.pending(),
            editable,
        },
    ) {
        use toolbar::Action;
        match action {
            Action::Save => intent = Some(Intent::SaveQuery),
            Action::Ai => {
                intent = Some(Intent::AiSql {
                    selected_prompt: false,
                })
            }
            Action::Settings => intent = Some(Intent::AiSqlSettings),
            Action::Connection(name) => intent = Some(Intent::SqlConnection(name)),
            Action::Database(name) => intent = Some(Intent::SqlDatabase(name)),
            Action::Format if editable && !sql.trim_matches(|ch: char| ch <= ' ').is_empty() => {
                match tomato_core::sql_format::format(sql) {
                    Ok(value) => {
                        formatted |= workbench.completion.editor.replace(ui.ctx(), sql, value);
                        workbench.completion.invalidate();
                    }
                    Err(error) => workbench.completion.error = error.to_string(),
                }
            }
            Action::Run | Action::Explain => {
                let range = workbench.completion.selection;
                let effective = tomato_core::sql_edit::effective(
                    sql,
                    range.secondary.index.into(),
                    range.primary.index.into(),
                );
                if !effective.is_empty() {
                    intent = Some(Intent::command(Command::SqlScript {
                        connection: crate::tabs::workbench::query_connection(
                            connection,
                            &workbench.schema,
                        ),
                        sql: effective.into(),
                        mode: if matches!(action, Action::Explain) {
                            tomato_services::database::script::Mode::Explain
                        } else {
                            tomato_services::database::script::Mode::Run
                        },
                    }));
                }
            }
            _ => {}
        }
    }
    let body = ui.available_rect_before_wrap();
    let (editor_rect, results_rect) =
        layout::split(ui, body, &mut workbench.editor_split, ui.id().with(&tab.id));
    let loading = workbench
        .document
        .as_ref()
        .is_some_and(|document| document.loading || document.close_after_save);
    ui.add_enabled_ui(!loading, |ui| {
        layout::editor(
            ui,
            editor_rect,
            sql,
            &mut workbench.completion,
            &tab.id,
            !tab.busy,
        );
    });
    if workbench.ai.pending() {
        ai_progress::show(ui, editor_rect, &workbench.completion);
    }
    if ui.input(|input| {
        input
            .pointer
            .hover_pos()
            .is_some_and(|point| editor_rect.contains(point))
    }) && let Some(path) = ui.input(|input| {
        input
            .raw
            .dropped_files
            .first()
            .map(|file| file.path().to_path_buf())
    }) {
        intent = Some(Intent::LoadSql(path));
    }
    if formatted || workbench.completion.changed {
        let document = workbench.document.get_or_insert_with(|| {
            let mut document = crate::tabs::queries::Document::new(
                tomato_core::queries::Scope {
                    connection: connection.name.clone(),
                    database: crate::tabs::workbench::query_database(connection, &workbench.schema)
                        .into(),
                    path: String::new(),
                },
                sql,
            );
            document.namespace = workbench.schema.clone();
            document
        });
        document.changed(sql);
    }
    if std::mem::take(&mut workbench.completion.save_requested) {
        intent = Some(Intent::SaveQuery);
    }
    if let Some(selected_prompt) = workbench.completion.ai_requested.take() {
        intent = Some(Intent::AiSql { selected_prompt });
    }
    if let Some(statement) = workbench.completion.run_sql.take() {
        intent = Some(Intent::command(Command::SqlScript {
            connection: crate::tabs::workbench::query_connection(connection, &workbench.schema),
            sql: statement,
            mode: tomato_services::database::script::Mode::Run,
        }));
    }
    ui.scope_builder(
        egui::UiBuilder::new()
            .id_salt((&tab.id, "sql-results-area"))
            .max_rect(results_rect),
        |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(results_rect));
            render_body(
                ui,
                results,
                selected,
                workbench,
                Body {
                    tab: &tab.id,
                    connection,
                    busy: tab.busy,
                    error: tab.error.as_deref(),
                },
            )
        },
    )
    .inner
    .or(intent)
}
struct Body<'a> {
    tab: &'a str,
    connection: &'a tomato_core::Connection,
    busy: bool,
    error: Option<&'a str>,
}
fn render_body(
    ui: &mut Ui,
    results: &mut [tomato_services::QueryResult],
    selected: &mut usize,
    workbench: &mut crate::tabs::workbench::Workbench,
    context: Body<'_>,
) -> Option<Intent> {
    let Body {
        tab,
        connection,
        busy,
        error,
    } = context;
    if let Some(mode) = workbench.execution {
        let explain = matches!(mode, tomato_services::database::script::Mode::Explain);
        information::render(
            ui,
            if explain { "剖析" } else { "信息" },
            if explain {
                "执行解释..."
            } else {
                "执行中..."
            },
            false,
        );
        return None;
    }
    if let Some(error) = error {
        information::render(ui, "信息", error, true);
        return None;
    }
    let mut intent = None;
    if !workbench.completion.error.is_empty() {
        ui.label(RichText::new(&workbench.completion.error).color(Color32::from_rgb(180, 70, 40)));
    }
    if workbench.script.is_some() {
        return script::render(ui, tab, results, workbench, connection, busy).or(intent);
    }
    if results.is_empty() {
        information::render(ui, "信息", "执行查询以查看结果", false);
        return None;
    }
    crate::native_tabs::show(
        ui,
        (tab, "query-result-tabs"),
        selected,
        crate::native_tabs::Style::Compact,
        results
            .iter()
            .enumerate()
            .map(|(index, _)| (index, format!("结果 {}", index + 1))),
    );
    if let Some(result) = results.get(*selected) {
        ui.horizontal(|ui| {
            ui.label(format!(
                "{} 行 · 影响 {} 行{}",
                result.rows.len(),
                result.affected,
                if result.truncated {
                    " · 显示已截断"
                } else {
                    ""
                }
            ));
            if ui.button("导出").clicked() {
                intent = Some(Intent::ExportData(Some(*selected)));
            }
            if ui.button("复制 JSON").clicked() {
                let rows: Vec<_> = result
                    .rows
                    .iter()
                    .enumerate()
                    .map(|(row_index, _)| {
                        result
                            .columns
                            .iter()
                            .enumerate()
                            .map(|(column_index, name)| {
                                (name, result.value(row_index, column_index))
                            })
                            .collect::<std::collections::BTreeMap<_, _>>()
                    })
                    .collect();
                if let Ok(json) = serde_json::to_string_pretty(&rows) {
                    ui.ctx().copy_text(json);
                }
            }
        });
        workbench
            .query_results
            .resize_with(results.len(), Default::default);
        results::render(
            ui,
            result,
            &mut workbench.query_results[*selected],
            ui.make_persistent_id(("query-results", tab, *selected)),
        );
        intent =
            results::take_intent(&mut workbench.query_results[*selected], *selected).or(intent);
    }

    intent
}

pub(super) fn result_dialog(
    ctx: &egui::Context,
    result: &tomato_services::QueryResult,
    state: &mut crate::tabs::query_results::State,
    id: egui::Id,
) {
    results::dialog(ctx, result, state, id);
}

pub(super) fn preview_layout(
    ui: &Ui,
    buffer: &dyn egui::TextBuffer,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    highlight::layout(ui, buffer, width)
}

pub(crate) fn export_table_snapshot(
    ctx: &egui::Context,
    state: &mut crate::tabs::workbench::Workbench,
    dialect: &str,
) -> anyhow::Result<tomato_core::data_export::Snapshot> {
    table::export_snapshot(ctx, state, dialect)
}
