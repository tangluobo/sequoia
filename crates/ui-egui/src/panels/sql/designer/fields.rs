use super::*;
pub(super) fn render(
    ui: &mut Ui,
    draft: &mut Draft,
    icons: &mut crate::icons::Icons,
    writable: bool,
) {
    let row_height = crate::table_style::field_height(ui.ctx());
    let was_editing = draft.edit.is_some();
    let type_popup = draft
        .edit
        .as_ref()
        .is_some_and(|edit| edit.type_picker.as_ref().is_some_and(|picker| picker.open));
    let navigation =
        if was_editing && writable && !type_popup && !egui::Popup::is_any_open(ui.ctx()) {
            [
                egui::Key::ArrowUp,
                egui::Key::ArrowDown,
                egui::Key::ArrowLeft,
                egui::Key::ArrowRight,
            ]
            .into_iter()
            .find(|key| ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, *key)))
        } else {
            None
        };
    let bounds = ui.max_rect();
    let maximum = (bounds.height() - 222.).max(60.);
    let upper = draft
        .split
        .unwrap_or((bounds.height() * 0.72).min(maximum))
        .clamp(60., maximum);
    let divider = Rect::from_min_size(
        pos2(bounds.left(), bounds.top() + upper),
        vec2(bounds.width(), 2.),
    );
    let drag = ui.interact(
        divider.expand2(vec2(0., 2.)),
        ui.id().with("field-divider"),
        Sense::drag(),
    );
    ui.painter()
        .rect_filled(divider, 0, Color32::from_gray(220));
    if drag.dragged()
        && let Some(pointer) = drag.interact_pointer_pos()
    {
        draft.split = Some((pointer.y - bounds.top()).clamp(60., maximum));
    }
    let grid = Rect::from_min_max(bounds.min, pos2(bounds.right(), divider.top())).shrink(1.);
    let mut edit_cell = None;
    let mut select = None;
    let mut flag = None;
    let mut finish = None;
    let mut column_selection = None;
    let mut menu_action = None;
    let mut resized = None;
    let mut type_popup_rect = None;
    let mut pointer_grid = super::selection::Grid {
        row_height,
        body: Rect::NOTHING,
        origin: grid.min,
        popup: None,
    };
    let mut vertical_output = None;
    let mut horizontal_output = None;
    ui.interact(grid, ui.id().with("field-grid-focus"), Sense::click());
    let scroll_to = draft.scroll_to.take();
    ui.scope_builder(egui::UiBuilder::new().max_rect(grid), |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(grid));
        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        ui.spacing_mut().scroll.bar_width = 12.;
        let output = egui::ScrollArea::horizontal()
            .id_salt("designer-horizontal")
            .auto_shrink([false, false])
            .animated(false)
            .scroll_source(egui::scroll_area::ScrollSource {
                drag: egui::scroll_area::DragScroll::Never,
                ..Default::default()
            })
            .show(ui, |ui| {
                let width = draft.widths.iter().sum::<f32>();
                ui.set_min_width(width);
                let (header, _) = ui.allocate_exact_size(vec2(width, row_height), Sense::hover());
                let mut x = header.left();
                for (index, label) in [
                    "",
                    "字段名",
                    "类型",
                    "长度",
                    "小数点",
                    "非空",
                    "主键",
                    "默认值",
                    "注释",
                ]
                .into_iter()
                .enumerate()
                {
                    let rect = Rect::from_min_size(
                        pos2(x, header.top()),
                        vec2(draft.widths[index], row_height),
                    );
                    super::grid::cell(ui, rect, Color32::from_gray(245), true, index == 0);
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        label,
                        crate::table_style::field_font(ui.ctx()),
                        Color32::BLACK,
                    );
                    if index > 0
                        && ui
                            .interact(
                                rect.shrink2(vec2(3., 0.)),
                                ui.id().with(("select-column", index)),
                                Sense::click(),
                            )
                            .clicked()
                    {
                        column_selection = Some(index - 1);
                    }
                    let resize = ui.interact(
                        Rect::from_center_size(rect.right_center(), vec2(5., row_height)),
                        ui.id().with(("column-resize", index)),
                        Sense::drag(),
                    );
                    if resize.dragged() {
                        resized = Some((
                            index,
                            (draft.widths[index] + ui.input(|i| i.pointer.delta().x))
                                .max(if index == 0 { 15. } else { 40. }),
                        ));
                    }
                    x += draft.widths[index];
                }
                let mut vertical = egui::ScrollArea::vertical();
                if let Some(row) = scroll_to {
                    vertical = vertical.vertical_scroll_offset(row as f32 * row_height);
                }
                let output = vertical
                    .id_salt("designer-vertical")
                    .auto_shrink([false, false])
                    .animated(false)
                    .scroll_source(egui::scroll_area::ScrollSource {
                        drag: egui::scroll_area::DragScroll::Never,
                        ..Default::default()
                    })
                    .max_height((grid.height() - row_height).max(1.))
                    .show_rows(ui, row_height, draft.request.columns.len(), |ui, rows| {
                        pointer_grid.body = ui.clip_rect().intersect(grid);
                        pointer_grid.origin = pos2(
                            header.left(),
                            ui.cursor().min.y - rows.start as f32 * row_height,
                        );
                        for row in rows {
                            let (line, _) =
                                ui.allocate_exact_size(vec2(width, row_height), Sense::hover());
                            let mut x = line.left();
                            for column in 0..9 {
                                let rect = Rect::from_min_size(
                                    pos2(x, line.top()),
                                    vec2(draft.widths[column], row_height),
                                );
                                x += draft.widths[column];
                                let selected = column > 0 && draft.selected(row, column - 1);
                                super::grid::cell(
                                    ui,
                                    rect,
                                    if selected {
                                        Color32::from_rgb(53, 146, 203)
                                    } else {
                                        Color32::WHITE
                                    },
                                    false,
                                    column == 0,
                                );
                                let response = ui.interact(
                                    rect,
                                    ui.id().with(("field", row, column)),
                                    Sense::click_and_drag(),
                                );
                                if column == 0 {
                                    if draft.row_selected(row) {
                                        let p = rect.center();
                                        ui.painter().add(egui::Shape::convex_polygon(
                                            vec![
                                                p + vec2(-2., -4.),
                                                p + vec2(3., 0.),
                                                p + vec2(-2., 4.),
                                            ],
                                            Color32::BLACK,
                                            Stroke::NONE,
                                        ));
                                    }
                                } else {
                                    let cell = Cell {
                                        row,
                                        column: column - 1,
                                    };
                                    if draft.edit.as_ref().is_some_and(|edit| edit.cell == cell) {
                                        let types = draft.types();
                                        let edit = draft.edit.as_mut().unwrap();
                                        if column == 2 {
                                            let output = super::type_picker::render(
                                                ui,
                                                rect.shrink(1.),
                                                edit,
                                                types,
                                            );
                                            type_popup_rect = output.popup;
                                            finish = output.finish;
                                            continue;
                                        }
                                        let id = edit.editor.prepare_id(
                                            ui.ctx(),
                                            ui.id().with("active-field-editor"),
                                        );
                                        let initial_focus = edit.focus;
                                        if initial_focus {
                                            ui.memory_mut(|m| m.request_focus(id));
                                            edit.editor
                                                .select(ui.ctx(), 0..edit.text.chars().count());
                                            edit.focus = false;
                                        }
                                        let lost = {
                                            // The row already owns its space. An editor must not
                                            // advance the grid cursor to its inset bottom edge.
                                            ui.place(
                                                rect.shrink(1.),
                                                TextEdit::singleline(&mut edit.text)
                                                    .id(id)
                                                    .font(crate::table_style::field_font(ui.ctx()))
                                                    .vertical_align(egui::Align::Center)
                                                    .char_limit(8192),
                                            )
                                            .lost_focus()
                                        };
                                        if initial_focus {
                                            ui.memory_mut(|m| m.request_focus(id));
                                        }
                                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                            finish = Some(false);
                                        } else if lost && !initial_focus
                                            || ui.input(|i| i.key_pressed(egui::Key::Enter))
                                        {
                                            finish = Some(true);
                                        }
                                    } else {
                                        let value = &draft.request.columns[row];
                                        if column == 5 {
                                            let box_rect = Rect::from_center_size(
                                                rect.center(),
                                                vec2(11., 11.),
                                            );
                                            ui.painter().rect(
                                                box_rect,
                                                0,
                                                Color32::WHITE,
                                                Stroke::new(1., Color32::from_gray(90)),
                                                StrokeKind::Inside,
                                            );
                                            if value.not_null {
                                                ui.painter().line_segment(
                                                    [
                                                        box_rect.min + vec2(2., 6.),
                                                        box_rect.min + vec2(4., 8.),
                                                    ],
                                                    Stroke::new(1., Color32::BLACK),
                                                );
                                                ui.painter().line_segment(
                                                    [
                                                        box_rect.min + vec2(4., 8.),
                                                        box_rect.min + vec2(9., 2.),
                                                    ],
                                                    Stroke::new(1., Color32::BLACK),
                                                );
                                            }
                                            if response.clicked()
                                                && writable
                                                && response
                                                    .interact_pointer_pos()
                                                    .is_some_and(|point| box_rect.contains(point))
                                            {
                                                flag = Some((row, 0, !value.not_null));
                                            }
                                        } else if column == 6 && value.primary_key {
                                            icons.paint(
                                                ui,
                                                "images/connect/primary_key.png",
                                                Rect::from_min_size(
                                                    rect.min + vec2(23., 7.),
                                                    vec2(16., 16.),
                                                ),
                                            );
                                            ui.painter().text(
                                                rect.min + vec2(41., 15.),
                                                Align2::LEFT_CENTER,
                                                (draft.request.columns[..=row]
                                                    .iter()
                                                    .filter(|c| c.primary_key)
                                                    .count())
                                                .to_string(),
                                                crate::table_style::field_font(ui.ctx()),
                                                Color32::from_gray(51),
                                            );
                                            if response.clicked() && writable {
                                                flag = Some((row, 1, false));
                                            }
                                        } else {
                                            let text =
                                                crate::tabs::table_design::field(value, column - 1);
                                            ui.painter()
                                                .with_clip_rect(rect.shrink2(vec2(6., 0.)))
                                                .text(
                                                    rect.left_center()
                                                        + vec2(
                                                            if column == 2 { 12. } else { 6. },
                                                            0.,
                                                        ),
                                                    Align2::LEFT_CENTER,
                                                    text,
                                                    if value.primary_key && column <= 2 {
                                                        crate::table_style::field_bold_font(
                                                            ui.ctx(),
                                                        )
                                                    } else {
                                                        crate::table_style::field_font(ui.ctx())
                                                    },
                                                    if selected {
                                                        Color32::WHITE
                                                    } else {
                                                        Color32::BLACK
                                                    },
                                                );
                                            if column == 2 {
                                                super::type_picker::arrow(
                                                    ui,
                                                    rect,
                                                    response.hovered(),
                                                );
                                            }
                                            if writable
                                                && (response.double_clicked()
                                                    && !ui.input(|input| {
                                                        input.modifiers.ctrl
                                                            || input.modifiers.shift
                                                    })
                                                    || column == 6 && response.clicked())
                                            {
                                                if column == 6 {
                                                    flag = Some((row, 1, true));
                                                } else {
                                                    edit_cell = Some(cell);
                                                }
                                            }
                                        }
                                    }
                                }
                                if response.secondary_clicked()
                                    && if column == 0 {
                                        !draft.row_selected(row)
                                    } else {
                                        !draft.selected(row, column - 1)
                                    }
                                {
                                    select = Some((row, column, false, false));
                                }
                                response.context_menu(|ui| {
                                    if ui.add(crate::compact_menu::Item::new("复制字段")).clicked()
                                    {
                                        draft.commit_edit(ui.ctx());
                                        match draft.copy_fields() {
                                            Ok(Some(text)) => ui.ctx().copy_text(text.to_owned()),
                                            Err(e) => draft.status = e.to_string(),
                                            _ => {}
                                        }
                                        ui.close();
                                    }
                                    if ui
                                        .add_enabled(
                                            writable,
                                            crate::compact_menu::Item::new("粘贴字段"),
                                        )
                                        .clicked()
                                    {
                                        ui.ctx()
                                            .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                                        ui.close();
                                    }
                                    crate::compact_menu::separator(ui);
                                    for (label, action) in [
                                        ("添加字段", Action::Add),
                                        ("插入字段", Action::Insert),
                                        ("删除字段", Action::Delete),
                                        ("切换主键", Action::PrimaryKey),
                                    ] {
                                        if ui
                                            .add_enabled(
                                                writable,
                                                crate::compact_menu::Item::new(label),
                                            )
                                            .clicked()
                                        {
                                            menu_action = Some(action);
                                            ui.close();
                                        }
                                    }
                                });
                            }
                        }
                    });
                vertical_output = Some(output);
            });
        horizontal_output = Some(output);
    });
    if let Some((column, width)) = resized {
        draft.widths[column] = width;
        ui.ctx().request_repaint();
    }
    pointer_grid.popup = type_popup_rect;
    let pointer = super::selection::pointer(ui, draft, &pointer_grid, writable);
    if let Some(cell) = pointer.edit {
        select = None;
        edit_cell = Some(cell);
    }
    if pointer.dragged {
        select = None;
        edit_cell = None;
        flag = None;
    }
    for (axis, output) in [
        (0, horizontal_output.as_ref()),
        (1, vertical_output.as_ref()),
    ] {
        if let Some(output) = output {
            super::selection::scroll(ui, draft, &pointer_grid, output, axis);
        }
    }
    if let Some(column) = column_selection {
        ui.memory_mut(|memory| memory.request_focus(ui.id().with("field-grid-focus")));
        draft.commit_edit(ui.ctx());
        draft.select_column(column);
    }
    if let Some(commit) = finish {
        if commit {
            draft.commit_edit(ui.ctx());
        } else {
            draft.cancel_edit(ui.ctx());
        }
    }
    if let Some((row, column, control, shift)) = select {
        draft.commit_edit(ui.ctx());
        if column == 0 {
            draft.select_row(row);
        } else {
            draft.select(
                Cell {
                    row,
                    column: column - 1,
                },
                control,
                shift,
            );
        }
    }
    if let Some(cell) = edit_cell {
        draft.begin_edit(ui.ctx(), cell);
    }
    if let Some(action) = menu_action {
        draft.perform(ui.ctx(), action);
    }
    if let Some((row, bit, value)) = flag {
        draft.set_flag(row, bit, value);
    }
    if let Some(key) = navigation
        && let Some(edit) = &draft.edit
    {
        let mut cell = edit.cell;
        match key {
            egui::Key::ArrowDown => {
                if cell.row + 1 >= draft.request.columns.len() {
                    draft.append();
                }
                cell.row = (cell.row + 1).min(draft.request.columns.len().saturating_sub(1));
            }
            egui::Key::ArrowUp => cell.row = cell.row.saturating_sub(1),
            _ => {
                let sequence = [0, 1, 2, 3, 7];
                let at = sequence
                    .iter()
                    .position(|value| *value == cell.column)
                    .unwrap_or(0);
                cell.column = sequence[if key == egui::Key::ArrowRight {
                    (at + 1).min(4)
                } else {
                    at.saturating_sub(1)
                }];
            }
        }
        draft.scroll_to = Some(cell.row);
        draft.begin_edit(ui.ctx(), cell);
    }
    if draft.edit.is_none() && !was_editing {
        let text_focus = ui
            .memory(|m| m.focused())
            .and_then(|id| egui::widgets::text_edit::TextEditState::load(ui.ctx(), id));
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::Key {
                    key: egui::Key::A,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.ctrl && text_focus.is_none() => draft.select_all(),
                egui::Event::Paste(text) if writable && text_focus.is_none() => {
                    if let Err(e) = draft.paste_fields(ui.ctx(), &text) {
                        draft.status = format!("粘贴失败: {e}");
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if !modifiers.ctrl && !modifiers.alt && writable && text_focus.is_none() => {
                    if let Some(mut cell) = draft.focus {
                        match key {
                            egui::Key::F2 | egui::Key::Enter => draft.begin_edit(ui.ctx(), cell),
                            egui::Key::ArrowDown => {
                                if cell.row + 1 < draft.request.columns.len() {
                                    cell.row += 1;
                                    draft.scroll_to = Some(cell.row);
                                    draft.select(cell, false, modifiers.shift);
                                } else {
                                    draft.perform(ui.ctx(), Action::Add);
                                }
                            }
                            egui::Key::ArrowUp => {
                                cell.row = cell.row.saturating_sub(1);
                                draft.select(cell, false, modifiers.shift);
                            }
                            egui::Key::ArrowLeft | egui::Key::ArrowRight => {
                                cell.column = if key == egui::Key::ArrowRight {
                                    (cell.column + 1).min(7)
                                } else {
                                    cell.column.saturating_sub(1)
                                };
                                draft.select(cell, false, modifiers.shift);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let properties = Rect::from_min_max(pos2(bounds.left(), divider.bottom()), bounds.max);
    ui.painter()
        .rect_filled(properties, 0, Color32::from_gray(248));
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(properties.shrink2(vec2(12., 8.))),
        |ui| super::pages::properties(ui, draft, writable),
    );
}

#[cfg(test)]
mod menu_metrics_tests {
    #[test]
    fn actual_designer_menu_has_compact_equal_rows() {
        let mut draft = crate::tabs::table_design::Draft::new("MYSQL", "fixture".into()).unwrap();
        let mut icons = crate::icons::Icons::default();
        crate::compact_menu::assert_actual_rows(
            |ui| super::render(ui, &mut draft, &mut icons, true),
            egui::pos2(90., 45.),
            &["复制字段", "粘贴字段"],
        );
    }
}
