//! Script presentation owns only view state; rows remain in the tab's result store.
use super::*;
#[cfg(test)]
#[path = "script_tests.rs"]
mod tests;
pub(super) fn render(
    ui: &mut Ui,
    tab: &str,
    results: &[tomato_services::QueryResult],
    workbench: &mut crate::tabs::workbench::Workbench,
    connection: &tomato_core::Connection,
    busy: bool,
) -> Option<Intent> {
    let script = workbench.script.as_ref()?;
    let (connection, namespace) = workbench
        .result_context
        .as_ref()
        .map(|context| (context.connection.as_ref(), context.namespace.as_str()))
        .unwrap_or((connection, workbench.schema.as_str()));
    let explain = matches!(
        script.mode,
        tomato_services::database::script::Mode::Explain
    );
    let count = script.row_results().count();
    let mut intent = None;
    let information = (!explain).then_some((0, "信息".to_owned()));
    let row_tabs = script
        .row_results()
        .enumerate()
        .filter(|_| !explain)
        .map(|(order, _)| {
            (
                order + 1,
                if count == 1 {
                    "结果".into()
                } else {
                    format!("结果{}", order + 1)
                },
            )
        });
    let plan = (!script.plans.is_empty()).then_some((usize::MAX, "剖析".to_owned()));
    let status = script
        .status
        .is_some()
        .then_some((usize::MAX - 1, "状态".to_owned()));
    crate::native_tabs::show(
        ui,
        (tab, "script-tabs"),
        &mut workbench.script_view,
        crate::native_tabs::Style::Compact,
        information
            .into_iter()
            .chain(row_tabs)
            .chain(plan)
            .chain(status),
    );
    if let Some(message) = &script.message {
        ui.label(message);
        return None;
    }
    workbench
        .query_results
        .resize_with(results.len(), Default::default);
    if workbench.script_view == usize::MAX {
        ScrollArea::vertical()
            .id_salt((tab, "script-plans"))
            .show(ui, |ui| {
                for (index, plan) in script.plans.iter().enumerate() {
                    let sql = script.sql(&plan.range);
                    ui.label(RichText::new(sql.chars().take(80).collect::<String>()).monospace())
                        .on_hover_text(sql);
                    if let Some(result) = results.get(plan.result) {
                        ui.allocate_ui(egui::vec2(ui.available_width(), 300.0), |ui| {
                            super::results::render(
                                ui,
                                result,
                                &mut workbench.query_results[plan.result],
                                ui.make_persistent_id((tab, "plan", index)),
                            );
                        });
                        intent = super::results::take_intent(
                            &mut workbench.query_results[plan.result],
                            plan.result,
                        )
                        .or(intent.take());
                    }
                }
            });
    } else if workbench.script_view == 0 {
        let info = workbench.script_info.get_or_insert_with(|| script.info());
        let mut readonly = info.as_str();
        ScrollArea::vertical()
            .id_salt((tab, "script-info"))
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut readonly)
                        .id_salt((tab, "script-info-text"))
                        .font(egui::FontId::new(12.0, egui::FontFamily::Monospace))
                        .margin(egui::vec2(8.0, 8.0))
                        .desired_width(ui.available_width()),
                );
            });
    } else {
        let index = if workbench.script_view == usize::MAX - 1 {
            script.status
        } else {
            script
                .row_results()
                .nth(workbench.script_view - 1)
                .and_then(|(_, statement)| statement.result)
        };
        if let Some(index) = index
            && let Some(result) = results.get(index)
        {
            if workbench.script_view != usize::MAX - 1 {
                return super::query_edit::render(
                    ui,
                    result,
                    &mut workbench.query_results[index],
                    super::query_edit::Context {
                        connection,
                        schema: namespace,
                        index,
                        busy,
                        id: ui.make_persistent_id((tab, "script-result", index)),
                    },
                );
            }
            super::results::render(
                ui,
                result,
                &mut workbench.query_results[index],
                ui.make_persistent_id((tab, "script-result", index)),
            );
            intent =
                super::results::take_intent(&mut workbench.query_results[index], index).or(intent);
        }
    }
    intent
}
