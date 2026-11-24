//! Editable index grid; fixed cell geometry, visible rows, anchored dropdowns.
use super::*;
use tomato_core::table_design::indexes::{self as model, Kind};
mod fields;

fn choice<T: PartialEq + Copy>(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    value: &mut T,
    items: &[(T, &str)],
) -> (bool, bool) {
    let response = ui.interact(rect, id, Sense::click());
    type_picker::arrow(ui, rect, true);
    let mut changed = false;
    let popup = egui::Popup::menu(&response)
        .anchor(rect)
        .align(egui::emath::RectAlign::BOTTOM_START)
        .align_alternatives(&[])
        .width(rect.width().max(120.))
        .show(|ui| {
            ui.set_min_width((rect.width() - 16.).max(104.));
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            ui.spacing_mut().item_spacing = vec2(0., 0.);
            for (item, label) in items {
                if ui
                    .add_sized(
                        vec2(ui.available_width(), 25.),
                        egui::Button::selectable(*value == *item, *label),
                    )
                    .clicked()
                {
                    changed = *value != *item;
                    *value = *item;
                    ui.close();
                }
            }
        });
    if std::env::var_os("TOMATO_DESIGNER_INDEX").is_some()
        && let Some(popup) = popup
    {
        ui.ctx().data_mut(|d| {
            d.insert_temp(egui::Id::new("designer-index-popup"), popup.response.rect)
        });
    }
    (changed, response.clicked())
}
pub(super) fn render(
    ui: &mut Ui,
    draft: &mut Draft,
    icons: &mut crate::icons::Icons,
    writable: bool,
) -> Option<Intent> {
    let bounds = ui.max_rect();
    let toolbar = Rect::from_min_size(bounds.min, vec2(bounds.width(), 35.));
    ui.painter()
        .rect_filled(toolbar, 0, Color32::from_gray(248));
    ui.painter().line_segment(
        [toolbar.left_bottom(), toolbar.right_bottom()],
        Stroke::new(1., Color32::from_gray(221)),
    );
    let mut x = toolbar.left() + 8.;
    let mut intent = None;
    for (n, (label, asset, width)) in [
        ("添加索引", "col_add.png", 86.),
        ("删除", "col_del.png", 62.),
        ("刷新", "refresh.png", 61.),
    ]
    .into_iter()
    .enumerate()
    {
        let enabled = writable
            && (n != 2 || draft.saved.is_some() && !draft.index_changes())
            && !model::kinds(draft.dialect).is_empty();
        if button(
            ui,
            icons,
            Rect::from_min_size(pos2(x, toolbar.top() + 4.), vec2(width, 26.)),
            label,
            asset,
            enabled,
        )
        .clicked()
        {
            match n {
                0 => {
                    draft.indexes.close_fields(ui.ctx());
                    draft.add_index();
                }
                1 => {
                    draft.indexes.close_fields(ui.ctx());
                    draft.delete_indexes();
                }
                _ => {
                    intent = Some(Intent::RefreshDesignInfo(
                        tomato_core::table_design::alter::InfoPage::Indexes,
                    ))
                }
            }
        }
        x += width + 2.;
    }
    let content = Rect::from_min_max(pos2(bounds.left(), toolbar.bottom()), bounds.max);
    ui.painter()
        .rect_filled(content, 0, Color32::from_gray(244));
    let mut changed = false;
    let mut select = None;
    let mut edit = None;
    let mut picker = None;
    let mut finish = false;
    let mut cancel = false;
    ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(content));
        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        ui.spacing_mut().scroll.bar_width = 12.;
        egui::ScrollArea::horizontal()
            .id_salt("index-horizontal")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let width = draft
                    .indexes
                    .widths
                    .iter()
                    .sum::<f32>()
                    .max(ui.available_width());
                ui.set_min_width(width);
                let (header, _) = ui.allocate_exact_size(vec2(width, 30.), Sense::hover());
                let mut x = header.left();
                for (col, label) in ["", "名称", "字段", "类型", "方法", "注释"]
                    .into_iter()
                    .enumerate()
                {
                    let rect = Rect::from_min_size(
                        pos2(x, header.top()),
                        vec2(draft.indexes.widths[col], 30.),
                    );
                    grid::cell(ui, rect, Color32::from_gray(245), true, col == 0);
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        label,
                        crate::native_dialog::font(ui.ctx(), 12.),
                        Color32::BLACK,
                    );
                    let resize = ui.interact(
                        Rect::from_center_size(rect.right_center(), vec2(5., 30.)),
                        ui.id().with(("index-resize", col)),
                        Sense::drag(),
                    );
                    if resize.dragged() {
                        draft.indexes.widths[col] = (draft.indexes.widths[col]
                            + ui.input(|i| i.pointer.delta().x))
                        .max(if col == 0 { 22. } else { 60. });
                    }
                    x += rect.width();
                }
                let count = draft.request.indexes.len();
                egui::ScrollArea::vertical()
                    .id_salt("index-vertical")
                    .auto_shrink([false, false])
                    .scroll_source(egui::scroll_area::ScrollSource {
                        drag: egui::scroll_area::DragScroll::Never,
                        ..Default::default()
                    })
                    .show_rows(ui, 30., count, |ui, visible| {
                        for row in visible {
                            let (line, _) =
                                ui.allocate_exact_size(vec2(width, 30.), Sense::hover());
                            let selected = draft.indexes.selected.get(row) == Some(&true);
                            let index = &mut draft.request.indexes[row];
                            let mut x = line.left();
                            for col in 0..6 {
                                let rect = Rect::from_min_size(
                                    pos2(x, line.top()),
                                    vec2(draft.indexes.widths[col], 30.),
                                );
                                x += rect.width();
                                let fill = if selected {
                                    Color32::from_rgb(53, 146, 203)
                                } else {
                                    Color32::WHITE
                                };
                                grid::cell(
                                    ui,
                                    rect,
                                    if col == 0 {
                                        Color32::from_gray(245)
                                    } else {
                                        fill
                                    },
                                    false,
                                    col == 0,
                                );
                                let id = ui.id().with(("index-cell", row, col));
                                let active = draft
                                    .indexes
                                    .editor
                                    .as_mut()
                                    .filter(|e| e.cell == Cell { row, column: col });
                                if let Some(editor) = active {
                                    let text = if col == 1 {
                                        &mut index.name
                                    } else {
                                        &mut index.comment
                                    };
                                    let initial = std::mem::take(&mut editor.focus);
                                    if initial {
                                        draft.indexes.text.release(ui.ctx());
                                        draft.indexes.text.reset();
                                    }
                                    let id = draft.indexes.text.prepare_id(ui.ctx(), id);
                                    if initial {
                                        draft
                                            .indexes
                                            .text
                                            .select(ui.ctx(), 0..text.chars().count());
                                    }
                                    let response = ui.place(
                                        rect.shrink2(vec2(4., 1.)),
                                        TextEdit::singleline(text)
                                            .id(id)
                                            .font(crate::native_dialog::font(ui.ctx(), 12.))
                                            .margin(vec2(2., 0.))
                                            .min_size(vec2(0., 28.))
                                            .vertical_align(egui::Align::Center)
                                            .char_limit(if col == 1 { 1024 } else { 65536 }),
                                    );
                                    if initial {
                                        response.request_focus();
                                    }
                                    changed |= response.changed();
                                    if ui.input_mut(|i| {
                                        i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                                    }) {
                                        *text = editor.original.clone();
                                        cancel = true;
                                        changed = true;
                                    }
                                    finish |= response.lost_focus()
                                        || ui.input_mut(|i| {
                                            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                                        });
                                    continue;
                                }
                                let label = match col {
                                    0 => {
                                        if selected {
                                            "▶".into()
                                        } else {
                                            String::new()
                                        }
                                    }
                                    1 => index.name.clone(),
                                    2 => index.fields_label(),
                                    3 => index.kind.label().into(),
                                    4 => {
                                        if index.method.is_empty() {
                                            "默认".into()
                                        } else {
                                            index.method.clone()
                                        }
                                    }
                                    _ => index.comment.clone(),
                                };
                                let field_dropdown = col == 2
                                    && writable
                                    && !index.constraint
                                    && index.opaque.is_empty();
                                if field_dropdown {
                                    type_picker::arrow(ui, rect, true);
                                    if let Some(fields) = &mut draft.indexes.fields
                                        && fields.row == row
                                    {
                                        fields.anchor = Some(rect);
                                    }
                                }
                                ui.painter()
                                    .with_clip_rect(ui.clip_rect().intersect(Rect::from_min_max(
                                        rect.left_top() + vec2(5., 0.),
                                        rect.right_bottom()
                                            - vec2(if field_dropdown { 20. } else { 5. }, 0.),
                                    )))
                                    .text(
                                        pos2(rect.left() + 6., rect.center().y),
                                        Align2::LEFT_CENTER,
                                        label,
                                        crate::native_dialog::font(ui.ctx(), 12.),
                                        if selected && col != 0 {
                                            Color32::WHITE
                                        } else {
                                            Color32::BLACK
                                        },
                                    );
                                let mutable = writable && !index.constraint;
                                if [3, 4].contains(&col) && mutable && index.opaque.is_empty() {
                                    let (change, clicked) = if col == 3 {
                                        let kinds: Vec<_> = model::kinds(draft.dialect)
                                            .iter()
                                            .map(|k| (*k, k.label()))
                                            .collect();
                                        let (change, clicked) =
                                            choice(ui, rect, id, &mut index.kind, &kinds);
                                        if change
                                            && !model::methods(draft.dialect, index.kind)
                                                .contains(&index.method.as_str())
                                        {
                                            index.method =
                                                model::methods(draft.dialect, index.kind)[0].into();
                                        }
                                        (change, clicked)
                                    } else {
                                        let methods = model::methods(draft.dialect, index.kind);
                                        let current = methods
                                            .iter()
                                            .position(|s| *s == index.method)
                                            .unwrap_or(0);
                                        let mut selected = current;
                                        let items: Vec<_> = methods
                                            .iter()
                                            .enumerate()
                                            .map(|(n, s)| {
                                                (n, if s.is_empty() { "默认" } else { s })
                                            })
                                            .collect();
                                        let (change, clicked) =
                                            choice(ui, rect, id, &mut selected, &items);
                                        if change {
                                            index.method = methods[selected].into();
                                        }
                                        (change, clicked)
                                    };
                                    if clicked {
                                        select = Some((row, egui::Modifiers::NONE));
                                    }
                                    changed |= change;
                                } else {
                                    let response = ui.interact(rect, id, Sense::click_and_drag());
                                    if response.clicked() {
                                        select = Some((
                                            row,
                                            ui.input(|i| {
                                                i.events
                                                    .iter()
                                                    .rev()
                                                    .find_map(|e| {
                                                        if let egui::Event::PointerButton {
                                                            modifiers,
                                                            ..
                                                        } = e
                                                        {
                                                            Some(*modifiers)
                                                        } else {
                                                            None
                                                        }
                                                    })
                                                    .unwrap_or(i.modifiers)
                                            }),
                                        ));
                                        if col == 2 && mutable {
                                            picker = Some((row, rect));
                                        }
                                    }
                                    if response.double_clicked()
                                        && writable
                                        && (col == 1 || mutable && col == 5)
                                    {
                                        edit = Some(Cell { row, column: col });
                                    }
                                    if col == 0 && response.drag_started() {
                                        draft.indexes.drag = Some(row);
                                    }
                                    if response.hovered() && !index.opaque.is_empty() {
                                        response.on_hover_text(&index.opaque);
                                    }
                                }
                            }
                            if draft.indexes.drag.is_some()
                                && ui.input(|i| {
                                    i.pointer.primary_down()
                                        && i.pointer.hover_pos().is_some_and(|p| line.contains(p))
                                })
                            {
                                select = Some((row, egui::Modifiers::SHIFT));
                            }
                        }
                    });
            });
    });
    if !ui.input(|i| i.pointer.primary_down()) {
        draft.indexes.drag = None;
    }
    if finish || cancel {
        draft.indexes.text.release(ui.ctx());
        draft.indexes.editor = None;
    }
    if let Some((row, modifiers)) = select {
        draft.select_index(row, modifiers);
    }
    if let Some(cell) = edit {
        draft.edit_index(cell);
    }
    if let Some((row, rect)) = picker {
        let toggle = draft.indexes.fields.as_ref().is_some_and(|p| p.row == row);
        draft.indexes.close_fields(ui.ctx());
        if !toggle {
            draft.index_fields(row);
            if let Some(fields) = &mut draft.indexes.fields {
                fields.anchor = Some(rect);
            }
        }
    }
    if changed {
        draft.changed();
        draft.status = "索引已修改（未保存）".into();
    }
    if writable
        && draft.indexes.editor.is_none()
        && draft.indexes.fields.is_none()
        && !egui::Popup::is_any_open(ui.ctx())
        && !ui.ctx().text_edit_focused()
    {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
            draft.delete_indexes();
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F2))
            && let Some(row) = draft.indexes.selected.iter().position(|b| *b)
        {
            draft.edit_index(Cell { row, column: 1 });
        }
    }
    fields::render(ui, draft);
    intent
}
