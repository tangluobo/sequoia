//! Compact field selection: natural-width checkboxes and virtual, scrollable rows.
use super::*;
use crate::native_dialog::button;
use egui::{Align, Color32, Layout, Rect, ScrollArea, Stroke, UiBuilder, vec2};
use tomato_core::data_export::Format;

pub(super) fn render(ui: &mut Ui, draft: &mut Draft, rect: Rect) {
    let at = |x, y, w, h| Rect::from_min_size(rect.min + vec2(x, y), vec2(w, h));
    if button(ui, at(20., 85., 46., 23.), "全选", false).clicked() {
        draft.fields.fill(true);
    }
    if button(ui, at(76., 85., 72., 23.), "取消全选", false).clicked() {
        draft.fields.fill(false);
    }
    ui.painter().text(
        at(164., 85., rect.width() - 184., 23.).left_center(),
        egui::Align2::LEFT_CENTER,
        format!(
            "已选择 {} / {} 个字段",
            draft.fields.iter().filter(|selected| **selected).count(),
            draft.fields.len()
        ),
        crate::native_dialog::font(ui.ctx(), 13.),
        Color32::from_gray(90),
    );
    let bottom = if draft.error.is_some() {
        385.
    } else {
        rect.height() - 52.
    };
    let list = at(20., 120., rect.width() - 40., (bottom - 120.).max(22.));
    ui.painter().rect_stroke(
        list,
        0.,
        Stroke::new(1., Color32::from_gray(210)),
        egui::StrokeKind::Inside,
    );
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(list.shrink(4.))
            .layout(Layout::top_down(Align::Min)),
        |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(list));
            ui.spacing_mut().interact_size.y = 22.;
            ui.spacing_mut().item_spacing.y = 0.;
            ui.spacing_mut().button_padding.y = 2.;
            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            ScrollArea::both()
                .id_salt("export-fields")
                .auto_shrink([false, false])
                .max_height(ui.available_height())
                .show_rows(ui, 22., draft.fields.len(), |ui, rows| {
                    let Some(snapshot) = &draft.current else {
                        return;
                    };
                    for index in rows {
                        ui.push_id(index, |ui| {
                            ui.horizontal(|ui| {
                                if draft.options.format == Format::Dbase {
                                    ui.allocate_ui_with_layout(
                                        vec2(280., 22.),
                                        Layout::left_to_right(Align::Center),
                                        |ui| {
                                            ui.set_width(280.);
                                            ui.style_mut().wrap_mode =
                                                Some(egui::TextWrapMode::Truncate);
                                            ui.checkbox(
                                                &mut draft.fields[index],
                                                &snapshot.schema.columns[index].name,
                                            )
                                            .on_hover_text(&snapshot.schema.columns[index].name);
                                        },
                                    );
                                    ui.add(
                                        TextEdit::singleline(&mut draft.aliases[index])
                                            .desired_width(240.)
                                            .hint_text("DBF 字段名（最多 10 字节）"),
                                    );
                                } else {
                                    ui.checkbox(
                                        &mut draft.fields[index],
                                        &snapshot.schema.columns[index].name,
                                    )
                                    .on_hover_text(&snapshot.schema.columns[index].name);
                                }
                            });
                        });
                    }
                });
        },
    );
}
