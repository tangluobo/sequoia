use super::*;
pub(super) fn combo<T: AsRef<str>>(
    ui: &mut Ui,
    rect: Rect,
    id: &str,
    text: &mut String,
    items: &[T],
) -> bool {
    let arrow = Rect::from_min_max(pos2(rect.right() - 23., rect.top()), rect.max);
    let response = ui.interact(arrow, ui.id().with(id), Sense::click());
    ui.painter().rect(
        rect,
        0,
        Color32::WHITE,
        Stroke::new(1., Color32::from_gray(200)),
        StrokeKind::Inside,
    );
    let p = arrow.center();
    ui.painter().add(egui::Shape::convex_polygon(
        vec![p + vec2(-4., -2.), p + vec2(4., -2.), p + vec2(0., 3.)],
        Color32::from_gray(150),
        Stroke::NONE,
    ));
    let mut changed = ui
        .put(
            Rect::from_min_max(rect.min + vec2(6., 2.), arrow.left_bottom() - vec2(2., 2.)),
            TextEdit::singleline(text)
                .id(ui.id().with((id, "text")))
                .frame(egui::Frame::NONE)
                .font(crate::native_dialog::font(ui.ctx(), 12.))
                .char_limit(8192),
        )
        .changed();
    egui::Popup::menu(&response)
        .anchor(rect)
        .align(egui::emath::RectAlign::BOTTOM_START)
        .align_alternatives(&[])
        .width(rect.width())
        .show(|ui| {
            egui::ScrollArea::vertical()
                .max_height(
                    (ui.ctx().content_rect().bottom() - rect.bottom() - 16.).clamp(0., 15. * 26.),
                )
                .min_scrolled_height(0.)
                .show(ui, |ui| {
                    for item in items {
                        if ui
                            .selectable_label(text == item.as_ref(), item.as_ref())
                            .clicked()
                        {
                            *text = item.as_ref().into();
                            changed = true;
                            ui.close();
                        }
                    }
                });
        });
    changed
}
fn checkbox(ui: &Ui, origin: egui::Pos2, label: &str, value: &mut bool) -> bool {
    let response = ui.interact(
        Rect::from_min_size(origin, vec2(110., 15.)),
        ui.id().with(label),
        Sense::click(),
    );
    let rect = Rect::from_min_size(origin, vec2(12., 12.));
    ui.painter().rect(
        rect,
        0,
        Color32::WHITE,
        Stroke::new(1., Color32::from_gray(85)),
        StrokeKind::Inside,
    );
    if *value {
        ui.painter().line_segment(
            [origin + vec2(2., 6.), origin + vec2(5., 9.)],
            Stroke::new(1., Color32::BLACK),
        );
        ui.painter().line_segment(
            [origin + vec2(5., 9.), origin + vec2(10., 2.)],
            Stroke::new(1., Color32::BLACK),
        );
    }
    ui.painter().text(
        origin + vec2(17., 6.),
        Align2::LEFT_CENTER,
        label,
        crate::native_dialog::font(ui.ctx(), 12.),
        Color32::BLACK,
    );
    if response.clicked() {
        *value = !*value;
        true
    } else {
        false
    }
}
pub(super) fn properties(ui: &mut Ui, draft: &mut Draft, writable: bool) {
    for name in ["field-default", "field-charset", "field-collation"] {
        draft.own_text(ui.id().with((name, "text")));
    }
    let key_length_id = ui.id().with("field-key-length");
    draft.own_text(key_length_id);
    let Some(row) = draft
        .focus
        .map(|cell| cell.row)
        .filter(|row| *row < draft.request.columns.len())
    else {
        ui.painter().text(
            ui.max_rect().left_center(),
            Align2::LEFT_CENTER,
            "请选择字段以编辑属性",
            crate::native_dialog::font(ui.ctx(), 12.),
            Color32::from_gray(153),
        );
        return;
    };
    if !writable {
        ui.disable();
    }
    let origin = ui.max_rect().min;
    let mut changed = false;
    let at = |y| Rect::from_min_size(origin + vec2(78., y), vec2(200., 28.));
    ui.painter().text(
        origin + vec2(0., 14.),
        Align2::LEFT_CENTER,
        "默认:",
        crate::native_dialog::font(ui.ctx(), 12.),
        Color32::BLACK,
    );
    let default_changed = combo(
        ui,
        at(0.),
        "field-default",
        &mut draft.request.columns[row].default_value,
        &["", "NULL", "CURRENT_TIMESTAMP"],
    );
    if default_changed {
        draft.normalize_default(row);
        changed = true;
    }
    let ty = draft.request.columns[row].data_type.to_ascii_lowercase();
    let string = [
        "char", "text", "enum", "set", "binary", "blob", "clob", "string",
    ]
    .iter()
    .any(|part| ty.contains(part));
    let numeric = [
        "int", "decimal", "numeric", "float", "double", "real", "number", "bit", "serial",
    ]
    .iter()
    .any(|part| ty.contains(part));
    let mut y = 36.;
    if string {
        for (which, label) in [(0, "字符集:"), (1, "排序规则:"), (2, "键长度:")] {
            ui.painter().text(
                origin + vec2(0., y + 14.),
                Align2::LEFT_CENTER,
                label,
                crate::native_dialog::font(ui.ctx(), 12.),
                Color32::BLACK,
            );
            let metadata = draft.metadata.as_deref();
            let c = &mut draft.request.columns[row];
            match which {
                0 => {
                    let items: Vec<_> = metadata
                        .into_iter()
                        .flat_map(|m| m.charsets.iter().map(|c| c.name.as_str()))
                        .collect();
                    let previous = c.charset.clone();
                    changed |= combo(ui, at(y), "field-charset", &mut c.charset, &items);
                    if previous != c.charset
                        && let Some(charset) = metadata
                            .and_then(|m| m.charsets.iter().find(|item| item.name == c.charset))
                        && !charset.collations.contains(&c.collation)
                    {
                        c.collation = charset.collations.first().cloned().unwrap_or_default();
                    }
                }
                1 => {
                    let items = metadata
                        .and_then(|m| m.charsets.iter().find(|cs| cs.name == c.charset))
                        .map_or(&[][..], |cs| cs.collations.as_slice());
                    changed |= combo(ui, at(y), "field-collation", &mut c.collation, items);
                }
                _ => {
                    changed |= ui
                        .put(
                            at(y),
                            TextEdit::singleline(&mut c.key_length)
                                .id(key_length_id)
                                .char_limit(20),
                        )
                        .changed()
                }
            }
            y += 36.;
        }
        let mut value = draft.request.columns[row].binary;
        if checkbox(ui, origin + vec2(0., y), "二进制", &mut value) {
            draft.set_flag(row, 5, value);
        }
        y += 19.;
    } else {
        y += 19.;
    }
    for (bit, label, visible) in [
        (2, "自动递增", draft.request.columns[row].primary_key),
        (3, "无符号", numeric),
        (4, "填充零", numeric),
    ] {
        if visible {
            let mut value = match bit {
                2 => draft.request.columns[row].auto_increment,
                3 => draft.request.columns[row].unsigned,
                _ => draft.request.columns[row].zerofill,
            };
            if checkbox(ui, origin + vec2(0., y), label, &mut value) {
                draft.set_flag(row, bit, value);
            }
            y += 21.;
        }
    }
    if changed {
        draft.changed();
    }
}
pub(super) fn render(ui: &mut Ui, draft: &mut Draft, writable: bool) -> Option<Intent> {
    match draft.page {
        Page::Options => {
            let origin = ui.max_rect().min + vec2(12., 12.);
            let mut changed = false;
            ui.add_enabled_ui(
                writable && matches!(draft.dialect, tomato_core::table_design::Dialect::Mysql),
                |ui| {
                    for name in ["engine", "charset", "collation", "row-format"] {
                        draft.own_text(ui.id().with((name, "text")));
                    }
                    for index in [3, 5] {
                        draft.own_text(ui.id().with(("option-number", index)));
                    }

                    for (index, label) in [
                        "引擎:",
                        "字符集:",
                        "排序规则:",
                        "自增值:",
                        "行格式:",
                        "平均行长:",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let y = index as f32 * 36.;
                        ui.painter().text(
                            origin + vec2(0., y + 14.),
                            Align2::LEFT_CENTER,
                            label,
                            crate::native_dialog::font(ui.ctx(), 12.),
                            Color32::BLACK,
                        );
                        let rect = Rect::from_min_size(origin + vec2(61., y), vec2(220., 28.));
                        let metadata = draft.metadata.as_deref();
                        match index {
                            0 => {
                                changed |= combo(
                                    ui,
                                    rect,
                                    "engine",
                                    &mut draft.request.options.engine,
                                    metadata.map_or(&[][..], |m| m.engines.as_slice()),
                                )
                            }
                            1 => {
                                let items: Vec<_> = metadata
                                    .into_iter()
                                    .flat_map(|m| m.charsets.iter().map(|c| c.name.as_str()))
                                    .collect();
                                let before = draft.request.options.charset.clone();
                                changed |= combo(
                                    ui,
                                    rect,
                                    "charset",
                                    &mut draft.request.options.charset,
                                    &items,
                                );
                                if before != draft.request.options.charset
                                    && let Some(cs) = metadata.and_then(|m| {
                                        m.charsets
                                            .iter()
                                            .find(|cs| cs.name == draft.request.options.charset)
                                    })
                                    && !cs.collations.contains(&draft.request.options.collation)
                                {
                                    draft.request.options.collation =
                                        cs.collations.first().cloned().unwrap_or_default();
                                }
                            }
                            2 => {
                                let items = metadata
                                    .and_then(|m| {
                                        m.charsets
                                            .iter()
                                            .find(|c| c.name == draft.request.options.charset)
                                    })
                                    .map_or(&[][..], |cs| cs.collations.as_slice());
                                changed |= combo(
                                    ui,
                                    rect,
                                    "collation",
                                    &mut draft.request.options.collation,
                                    items,
                                );
                            }
                            4 => {
                                changed |= combo(
                                    ui,
                                    rect,
                                    "row-format",
                                    &mut draft.request.options.row_format,
                                    &[
                                        "Compact",
                                        "Dynamic",
                                        "Fixed",
                                        "Compressed",
                                        "Redundant",
                                        "Default",
                                    ],
                                )
                            }
                            _ => {
                                changed |= ui
                                    .put(
                                        rect,
                                        TextEdit::singleline(if index == 3 {
                                            &mut draft.request.options.auto_increment
                                        } else {
                                            &mut draft.request.options.avg_row_length
                                        })
                                        .id(ui.id().with(("option-number", index)))
                                        .char_limit(100),
                                    )
                                    .changed()
                            }
                        }
                    }
                },
            );
            if changed {
                draft.changed();
            }
        }
        Page::Comment => {
            let id = ui.id().with("designer-comment");
            draft.own_text(id);
            let height = ui.available_height();
            let response = ui.add_sized(
                vec2(ui.available_width(), height.max(30.)),
                TextEdit::multiline(&mut draft.request.comment)
                    .id(id)
                    .interactive(writable)
                    .char_limit(512 * 1024),
            );
            if response.changed() {
                draft.changed();
            }
        }
        Page::Sql => {
            let bounds = ui.max_rect();
            let selector =
                Rect::from_min_size(pos2(bounds.left(), bounds.bottom() - 28.), vec2(81., 28.));
            let response = crate::native_dialog::button(
                ui,
                selector,
                if draft.preview_save_as {
                    "另存为"
                } else {
                    "保存"
                },
                false,
            );
            let arrow = pos2(selector.right() - 12., selector.center().y);
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    arrow + vec2(-4., -2.),
                    arrow + vec2(4., -2.),
                    arrow + vec2(0., 3.),
                ],
                Color32::from_gray(150),
                Stroke::NONE,
            ));
            egui::Popup::menu(&response).width(81.).show(|ui| {
                for (value, label) in [(false, "保存"), (true, "另存为")] {
                    if ui
                        .selectable_label(draft.preview_save_as == value, label)
                        .clicked()
                    {
                        draft.preview_save_as = value;
                        if draft
                            .saved
                            .as_ref()
                            .is_some_and(|saved| saved.view || value)
                        {
                            draft.invalidate_sql();
                        }
                        draft.preview_revision = None;
                        ui.close();
                    }
                }
            });
            draft.update_preview();
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(Rect::from_min_max(
                    bounds.min,
                    pos2(bounds.right(), selector.top() - 6.),
                )),
                |ui| super::preview::render(ui, draft),
            );
        }
        Page::Indexes | Page::ForeignKeys | Page::Triggers | Page::Fields => {}
    }
    None
}
