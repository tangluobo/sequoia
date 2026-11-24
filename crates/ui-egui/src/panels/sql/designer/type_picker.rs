//! Java DataTypeComboBoxTableCell: open on press, focus candidates, commit explicitly.
use super::*;
use crate::tabs::table_design::Edit;

pub(super) struct Output {
    pub finish: Option<bool>,
    pub popup: Option<Rect>,
}

pub(super) fn arrow(ui: &Ui, rect: Rect, hovered: bool) {
    if hovered {
        let p = pos2(rect.right() - 12., rect.center().y);
        ui.painter().add(egui::Shape::convex_polygon(
            vec![p + vec2(-4., -2.), p + vec2(4., -2.), p + vec2(0., 2.)],
            Color32::from_gray(102),
            Stroke::NONE,
        ));
    }
}

pub(super) fn render(ui: &mut Ui, rect: Rect, edit: &mut Edit, types: &[&str]) -> Output {
    let id = edit
        .editor
        .prepare_id(ui.ctx(), ui.id().with("field-type-text"));
    let popup_id = id.with("popup");
    let picker = edit.type_picker.as_mut().expect("type editor");
    picker.popup = Some(popup_id);
    let initial = std::mem::take(&mut edit.focus);
    if initial {
        // Java gives the list focus and leaves the editor text deselected.
        edit.editor.select(
            ui.ctx(),
            edit.text.chars().count()..edit.text.chars().count(),
        );
        ui.memory_mut(|memory| memory.request_focus(popup_id));
    }
    let escape = ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    let enter = ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    if picker.open {
        for (key, down) in [(egui::Key::ArrowUp, false), (egui::Key::ArrowDown, true)] {
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key)) {
                picker.step(types, down);
            }
        }
    }
    ui.painter().rect(
        rect,
        0,
        Color32::WHITE,
        Stroke::new(1., Color32::from_rgb(53, 146, 203)),
        StrokeKind::Inside,
    );
    let button = Rect::from_min_max(pos2(rect.right() - 23., rect.top()), rect.max);
    let response = ui.interact(button, id.with("arrow"), Sense::click());
    arrow(ui, rect, ui.rect_contains_pointer(rect));
    if response.clicked() {
        picker.open = !picker.open;
        if picker.open {
            picker.scroll = true;
            ui.memory_mut(|memory| memory.request_focus(popup_id));
        }
    }
    let text_rect =
        Rect::from_min_max(rect.min + vec2(4., 1.), button.left_bottom() - vec2(1., 1.));
    let text = ui.place(
        text_rect,
        TextEdit::singleline(&mut edit.text)
            .id(id)
            .frame(egui::Frame::NONE)
            .margin(vec2(0., 0.))
            .min_size(text_rect.size())
            .vertical_align(egui::Align::Center)
            .font(crate::table_style::field_font(ui.ctx()))
            .char_limit(8192),
    );
    if text.changed() {
        picker.filter(types, &edit.text);
    }
    let mut chosen = None;
    let visible = types.iter().filter(|value| picker.matches(value)).count();
    let focused_row = picker.focused.and_then(|index| {
        types
            .iter()
            .take(index + 1)
            .filter(|value| picker.matches(value))
            .count()
            .checked_sub(1)
    });
    let scroll = std::mem::take(&mut picker.scroll);
    let item_height = (crate::table_style::field_font(ui.ctx()).size + 8.).max(25.);
    // Java's skin adds the scrollbar to the 120px cell and displays twenty 25px rows.
    let label_width = *picker.label_width.get_or_insert_with(|| {
        types
            .iter()
            .map(|value| {
                ui.painter()
                    .layout_no_wrap(
                        (*value).to_owned(),
                        crate::table_style::field_font(ui.ctx()),
                        Color32::BLACK,
                    )
                    .size()
                    .x
            })
            .fold(0., f32::max)
    });
    let available = ui.ctx().content_rect();
    let popup_width = (rect.width() + 17.)
        .max(label_width + 35.)
        .min((available.right() - rect.left()).max(4.));
    let popup_height = (available.bottom() - rect.bottom() - 5.).clamp(0., 500.);
    let popup = egui::Popup::new(popup_id, ui.ctx().clone(), rect, ui.layer_id())
        .align(egui::emath::RectAlign::BOTTOM_START)
        .align_alternatives(&[])
        .open_memory(Some(egui::SetOpenCommand::Bool(picker.open && !escape)))
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        .width(popup_width)
        .frame(
            egui::Frame::NONE
                .inner_margin(egui::Margin {
                    left: 1,
                    right: 1,
                    top: 1,
                    bottom: 2,
                })
                .fill(Color32::WHITE)
                .stroke(Stroke::new(1., Color32::from_gray(204))),
        )
        .show(|ui| {
            ui.set_width((popup_width - 4.).max(1.));
            ui.set_max_height(popup_height);
            ui.spacing_mut().item_spacing = vec2(0., 0.);
            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            ui.spacing_mut().scroll.bar_width = 15.;
            let salt = popup_id.with("scroll");
            let mut area = egui::ScrollArea::vertical()
                .id_salt(salt)
                .max_height(popup_height)
                .min_scrolled_height(0.)
                .auto_shrink([false, true]);
            if scroll && let Some(row) = focused_row {
                let scroll_id = ui.make_persistent_id(egui::IdSalt::new(salt));
                let old = egui::scroll_area::State::load(ui.ctx(), scroll_id)
                    .map_or(0., |state| state.offset.y);
                let top = row as f32 * item_height;
                let height = ui.available_height().min(popup_height);
                area = area
                    .vertical_scroll_offset(old.clamp((top + item_height - height).max(0.), top));
            }
            area.show_rows(ui, item_height, visible, |ui, rows| {
                for (index, value) in types
                    .iter()
                    .enumerate()
                    .filter(|(_, value)| picker.matches(value))
                    .skip(rows.start)
                    .take(rows.len())
                {
                    let (item_rect, response) = ui.allocate_exact_size(
                        vec2(ui.available_width(), item_height),
                        Sense::click(),
                    );
                    let focused = picker.focused == Some(index);
                    let fill = if focused {
                        Color32::from_rgb(53, 146, 203)
                    } else if response.hovered() {
                        Color32::from_rgb(216, 230, 242)
                    } else if edit.text == *value {
                        Color32::from_rgb(192, 220, 243)
                    } else {
                        Color32::WHITE
                    };
                    ui.painter().rect_filled(item_rect, 0, fill);
                    ui.painter().text(
                        item_rect.left_center() + vec2(8., 0.),
                        Align2::LEFT_CENTER,
                        value,
                        crate::table_style::field_font(ui.ctx()),
                        if focused {
                            Color32::WHITE
                        } else {
                            Color32::from_gray(51)
                        },
                    );
                    if response.clicked() {
                        chosen = Some(*value);
                    }
                }
            });
        })
        .map(|output| output.response.rect);
    if enter && picker.open {
        chosen = picker.focused.and_then(|index| types.get(index).copied());
    }
    if let Some(value) = chosen {
        value.clone_into(&mut edit.text);
    }
    let outside_press = !initial
        && ui.input(|input| {
            input.pointer.primary_pressed()
                && input.pointer.interact_pos().is_some_and(|point| {
                    !rect.contains(point) && popup.is_none_or(|popup| !popup.contains(point))
                })
        });
    let lost = !initial && !picker.open && text.lost_focus() && !response.clicked();
    Output {
        finish: if escape {
            Some(false)
        } else if chosen.is_some() || enter || outside_press || lost {
            Some(true)
        } else {
            None
        },
        popup,
    }
}
