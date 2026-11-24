//! Anchored compound-field dropdown. Checkbox edits apply live; Escape restores the opening value.
use super::*;
pub(super) fn render(ui: &mut Ui, draft: &mut Draft) {
    let Some(mut picker) = draft.indexes.fields.take() else {
        return;
    };
    let Some(index) = draft.request.indexes.get(picker.row) else {
        return;
    };
    let ctx = ui.ctx().clone();
    let id = ui.id().with("designer-index-fields");
    let anchor = picker.anchor.unwrap_or_else(|| {
        Rect::from_min_size(
            pos2(
                ui.max_rect().left() + 202.,
                ui.max_rect().top() + 65. + picker.row as f32 * 30.,
            ),
            vec2(260., 30.),
        )
    });
    let width = (ctx.content_rect().right() - anchor.left() - 8.).clamp(220., 440.);
    let height = (ctx.content_rect().bottom() - anchor.bottom() - 88.).clamp(0., 240.);
    let mut proof = std::env::var_os("TOMATO_DESIGNER_INDEX").map(|_| Vec::new());
    let mut accept = false;
    let prefix = matches!(draft.dialect, tomato_core::table_design::Dialect::Mysql)
        && matches!(index.kind, Kind::Normal | Kind::Unique);
    let order = if matches!(draft.dialect, tomato_core::table_design::Dialect::Postgres) {
        index.method == "btree"
    } else {
        index.method == "BTREE"
    };
    let mut open = picker.open;
    // A direction menu may extend outside this dropdown. Its selection must close
    // only that menu; a click outside both menus closes the field dropdown too.
    let child = picker
        .child
        .filter(|id| egui::Popup::is_id_open(&ctx, *id))
        .and_then(|id| ctx.memory(|m| m.area_rect(id)));
    let outside = child.is_some_and(|child| {
        ctx.input(|i| {
            i.pointer.any_click()
                && i.pointer.interact_pos().is_some_and(|p| {
                    !child.contains(p) && picker.popup.is_some_and(|r| !r.contains(p))
                })
        })
    });
    let escape = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    if escape
        && child.is_some()
        && let Some(id) = picker.child.take()
    {
        egui::Popup::close_id(&ctx, id);
    }
    let mut cancel = escape && child.is_none();
    let popup = egui::Popup::new(id, ctx.clone(), anchor, ui.layer_id())
        .align(egui::emath::RectAlign::BOTTOM_START)
        .align_alternatives(&[])
        .open_bool(&mut open)
        .close_behavior(if child.is_some() {
            egui::PopupCloseBehavior::IgnoreClicks
        } else {
            egui::PopupCloseBehavior::CloseOnClickOutside
        })
        .width(width)
        .show(|ui| {
            let inner = (width - 12.).max(1.);
            ui.set_width(inner);
            ui.spacing_mut().item_spacing = vec2(6., 0.);
            ui.horizontal(|ui| {
                ui.label("索引字段");
                for (down, label) in [(false, "上移"), (true, "下移")] {
                    if ui
                        .add_enabled(
                            picker.focus.is_some_and(|n| {
                                if down {
                                    n + 1 < picker.entries.len()
                                } else {
                                    n > 0
                                }
                            }),
                            egui::Button::new(label),
                        )
                        .clicked()
                    {
                        let row = picker.focus.unwrap();
                        let next = if down { row + 1 } else { row - 1 };
                        picker.entries.swap(row, next);
                        picker.focus = Some(next);
                    }
                }
            });
            ui.add_space(3.);
            egui::ScrollArea::vertical()
                .id_salt("index-fields-rows")
                .max_height(height)
                .min_scrolled_height(0.)
                .auto_shrink([false, true])
                .show_rows(ui, 30., picker.entries.len(), |ui, visible| {
                    for row in visible {
                        let (rect, _) = ui.allocate_exact_size(vec2(inner, 30.), Sense::hover());
                        let mut row_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .id_salt(("index-field-row", row))
                                .max_rect(rect)
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        let ui = &mut row_ui;
                        let (selected, field) = &mut picker.entries[row];
                        ui.set_clip_rect(ui.clip_rect().intersect(rect));
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                        let response = ui
                            .allocate_ui_with_layout(
                                vec2((inner - 210.).max(80.), 26.),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.set_min_width((inner - 210.).max(80.));
                                    ui.checkbox(selected, &field.name)
                                },
                            )
                            .inner;
                        if let Some(proof) = &mut proof {
                            proof.push((field.name.clone(), response.rect));
                        }
                        if response.clicked() {
                            picker.focus = Some(row);
                        }
                        response.on_hover_text(&field.name);
                        ui.add_enabled_ui(prefix && *selected, |ui| {
                            ui.label("长度");
                            let mut n = field.prefix.unwrap_or(0);
                            if ui
                                .add_sized(
                                    vec2(58., 26.),
                                    egui::DragValue::new(&mut n)
                                        .range(0..=65535)
                                        .custom_formatter(|n, _| {
                                            if n == 0. {
                                                "全部".into()
                                            } else {
                                                format!("{n:.0}")
                                            }
                                        }),
                                )
                                .changed()
                            {
                                field.prefix = (n > 0).then_some(n);
                            }
                        });
                        ui.add_enabled_ui(order && *selected, |ui| {
                            let combo = egui::ComboBox::from_id_salt(("index-direction", row))
                                .width(65.)
                                .selected_text(if field.descending { "DESC" } else { "ASC" })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut field.descending, false, "ASC");
                                    ui.selectable_value(&mut field.descending, true, "DESC");
                                });
                            if combo.inner.is_some() {
                                picker.child = Some(combo.response.id.with("popup"));
                            }
                        });
                    }
                });
            if !picker.error.is_empty() {
                ui.colored_label(Color32::RED, &picker.error);
            }
            ui.add_space(3.);
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("确定").clicked() {
                    accept = true;
                }
                if ui.button("取消").clicked() {
                    cancel = true;
                }
            });
        });
    if let Some(proof) = proof {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("designer-index-field-rows"), proof));
    }
    if std::env::var_os("TOMATO_DESIGNER_INDEX").is_some()
        && let Some(popup) = &popup
    {
        ctx.data_mut(|d| {
            d.insert_temp(
                egui::Id::new("designer-index-fields-popup"),
                popup.response.rect,
            )
        });
    }
    picker.popup = popup.as_ref().map(|p| p.response.rect);
    picker.open = open;
    let mut selected_fields: Vec<_> = picker
        .entries
        .iter()
        .filter(|(yes, _)| *yes)
        .map(|(_, f)| f.clone())
        .collect();
    if !prefix {
        for f in &mut selected_fields {
            f.prefix = None;
        }
    }
    if !order {
        for f in &mut selected_fields {
            f.descending = false;
        }
    }
    let mut close = popup.is_none() || !open || outside || cancel;
    if accept {
        let mut proposed = index.clone();
        proposed.fields = selected_fields.clone();
        match proposed.validate(draft.dialect, &draft.request.columns) {
            Ok(()) => close = true,
            Err(e) => picker.error = e.to_string(),
        }
    }
    let fields = if cancel {
        picker.original.clone()
    } else {
        selected_fields
    };
    if fields != draft.request.indexes[picker.row].fields {
        draft.request.indexes[picker.row].fields = fields;
        draft.changed();
        draft.status = "索引字段已修改（未保存）".into();
    }
    if close {
        if let Some(child) = picker.child {
            egui::Popup::close_id(&ctx, child);
        }
    } else {
        draft.indexes.fields = Some(picker);
    }
}
