//! Original two-line query footer; protocol writes go through the connection actor.
use super::*;
use crate::tabs::query_results::State;
mod status;
mod toolbar;
pub(super) struct Context<'a> {
    pub connection: &'a tomato_core::Connection,
    pub schema: &'a str,
    pub index: usize,
    pub busy: bool,
    pub id: egui::Id,
}
pub(super) fn render(
    ui: &mut Ui,
    result: &tomato_services::QueryResult,
    state: &mut State,
    context: Context<'_>,
) -> Option<Intent> {
    let Context {
        connection,
        schema,
        index,
        busy,
        id,
    } = context;
    if let Some(editing) = &mut state.editing {
        editing.enabled = !busy && !state.saving;
    }
    let outer = ui.available_rect_before_wrap();
    let grid = egui::Rect::from_min_max(
        outer.min,
        egui::pos2(outer.right(), (outer.bottom() - 48.0).max(outer.top())),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(grid), |ui| {
        super::results::render(ui, result, state, id)
    });
    let footer = egui::Rect::from_min_max(grid.left_bottom(), outer.max);
    let mut deleting = false;
    let action = egui::Rect::from_min_size(footer.min, egui::vec2(footer.width(), 27.0));
    let save = toolbar::controls(ui, state, busy, action);
    status::paint(
        ui,
        result,
        state,
        egui::Rect::from_min_max(action.left_bottom(), footer.max),
    );
    if state
        .editing
        .as_ref()
        .is_some_and(|editing| editing.confirm_delete)
    {
        egui::Window::new("删除行")
            .id(id.with("delete-confirm"))
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                let target = &state.editing.as_ref().unwrap().target.table;
                let count = (0..state.visible_rows())
                    .filter(|row| state.row_selected(*row))
                    .count();
                ui.label(format!(
                    "确定要从表 {} 中删除选中的 {count} 行吗？此操作不可撤销！",
                    target.name
                ));
                ui.horizontal(|ui| {
                    if ui.button("确定").clicked() {
                        state.mark_delete();
                        deleting = true;
                    }
                    if ui.button("取消").clicked()
                        && let Some(editing) = &mut state.editing
                    {
                        editing.confirm_delete = false;
                    }
                });
            });
    }
    ui.advance_cursor_after_rect(outer);
    if save || deleting {
        let editing = state.editing.as_mut()?;
        editing.release(ui.ctx());
        if let Err(error) = editing.finish(result) {
            state.error = error.to_string();
            return None;
        }
        match editing.changes(result, deleting) {
            Ok(changes) if !changes.is_empty() => {
                return Some(Intent::command(Command::SaveQueryRows {
                    connection: crate::tabs::workbench::query_connection(connection, schema),
                    result: index,
                    editing: editing.target.clone(),
                    changes,
                }));
            }
            Ok(_) => {}
            Err(error) => state.error = error.to_string(),
        }
    }
    super::results::take_intent(state, index)
}
