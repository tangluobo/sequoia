//! Compact SQL preview, with normal text selection and full-statement copying.
use super::*;
#[cfg(test)]
mod tests;

pub(super) fn render(ui: &mut Ui, state: &Workbench, _connection: &tomato_core::Connection) {
    let available = ui.available_rect_before_wrap();
    let rect = egui::Rect::from_min_size(available.min, egui::vec2(available.width(), 21.));
    ui.painter().rect_filled(rect, 0, Color32::from_gray(250));
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(1., Color32::from_gray(215)),
    );
    let info = state.page.as_ref().map_or_else(
        || "无数据".into(),
        |page| {
            if page.total == 0 {
                "无数据".into()
            } else {
                let row = state.table_view.focus.map_or(0, |cell| cell.0) as u64;
                format!(
                    "第 {} 条记录（共 {} 条）于第 {} 页",
                    (page.offset + row + 1).min(page.total),
                    page.total,
                    page.offset / u64::from(page.limit).max(1) + 1
                )
            }
        },
    );
    let galley = ui.painter().layout_no_wrap(
        info,
        egui::FontId::proportional(11.),
        Color32::from_gray(34),
    );
    let right = egui::pos2(
        rect.right() - 5. - galley.size().x,
        rect.center().y - galley.size().y / 2.,
    );
    ui.painter().galley(right, galley, Color32::from_gray(34));
    let sql = state
        .page
        .as_ref()
        .map(|page| page.sql.as_str())
        .filter(|sql| !sql.is_empty());
    let text = sql.unwrap_or(&state.status);
    let sql_rect = egui::Rect::from_min_max(
        rect.min + egui::vec2(5., 0.),
        egui::pos2((right.x - 8.).max(rect.left() + 5.), rect.bottom()),
    );
    if sql_rect.width() > 1. {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(sql_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.set_clip_rect(sql_rect.intersect(ui.clip_rect()));
        // &str is a read-only TextBuffer: selection works, editing cannot alter the SQL.
        let mut readonly = text;
        let response = child.add(
            egui::TextEdit::singleline(&mut readonly)
                .id(ui.id().with("table-page-sql-preview"))
                .font(egui::FontId::monospace(11.))
                .text_color(Color32::from_gray(17))
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .vertical_align(egui::Align::Center)
                .min_size(egui::vec2(0., 21.))
                .desired_width(sql_rect.width())
                .cursor_at_end(false),
        );
        response.clone().on_hover_text(if state.status.is_empty() {
            text.to_owned()
        } else {
            format!("{text}\n{}", state.status)
        });
        egui::Popup::context_menu(&response).width(110.).show(|ui| {
            if ui
                .add_enabled(sql.is_some(), crate::compact_menu::Item::new("复制 SQL"))
                .clicked()
            {
                if let Some(sql) = sql {
                    ui.ctx().copy_text(sql.to_owned());
                }
                ui.close();
            }
        });
    }
    ui.allocate_rect(rect, egui::Sense::hover());
}
