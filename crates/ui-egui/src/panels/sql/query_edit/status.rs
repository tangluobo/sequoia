use super::*;
pub(super) fn paint(
    ui: &Ui,
    _result: &tomato_services::QueryResult,
    state: &State,
    rect: egui::Rect,
) {
    ui.painter().rect_filled(rect, 0, Color32::from_gray(250));
    ui.painter().line_segment(
        [
            egui::pos2(rect.left(), rect.bottom() - 0.5),
            egui::pos2(rect.right(), rect.bottom() - 0.5),
        ],
        egui::Stroke::new(1.0, Color32::from_gray(224)),
    );
    let family = if ui.ctx().fonts(|fonts| {
        fonts
            .definitions()
            .families
            .contains_key(&egui::FontFamily::Name("table".into()))
    }) {
        egui::FontFamily::Name("table".into())
    } else {
        egui::FontFamily::Proportional
    };
    let font = egui::FontId::new(11.0, family);
    let status = if state.saving {
        if state
            .editing
            .as_ref()
            .is_some_and(|editing| !editing.deleted.is_empty())
        {
            "正在删除数据..."
        } else {
            "保存中..."
        }
    } else if state
        .editing
        .as_ref()
        .is_some_and(|editing| editing.dirty())
    {
        "有未保存的修改"
    } else {
        state.edit_status.as_deref().unwrap_or_else(|| {
            state.editing.as_ref().map_or(
                "只读结果 · 仅安全的单表查询可编辑",
                |editing| editing.target.plan.status_text.as_str(),
            )
        })
    };
    let record = if state.visible_rows() == 0 {
        "无数据".into()
    } else {
        format!(
            "第 {} 条记录（共 {} 条）",
            state
                .focus
                .map_or(1, |cell| cell.row + 1)
                .min(state.visible_rows()),
            state.visible_rows()
        )
    };
    let galley = ui
        .painter()
        .layout_no_wrap(record, font.clone(), Color32::from_gray(34));
    let x = (rect.right() - 5.0 - galley.size().x.ceil()).max(rect.left() + 5.0);
    ui.painter().galley(
        egui::pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        Color32::from_gray(34),
    );
    let width = (x - rect.left() - 13.0).max(0.0);
    let galley = egui::WidgetText::from(
        RichText::new(status)
            .font(font)
            .color(Color32::from_gray(85)),
    )
    .into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        width,
        egui::FontSelection::Default,
    );
    ui.painter().galley(
        egui::pos2(rect.left() + 5.0, rect.center().y - galley.size().y / 2.0),
        galley,
        Color32::from_gray(85),
    );
}
