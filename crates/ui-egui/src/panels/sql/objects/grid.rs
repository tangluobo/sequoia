use super::*;
use egui::{ScrollArea, TextEdit};
use window::DragSelection;
const GREEN: Color32 = Color32::from_rgb(7, 193, 96);

fn entry(
    ui: &mut Ui,
    state: &mut State,
    index: usize,
    rect: Rect,
    icon: bool,
    icons: &mut Icons,
    id: &str,
) -> Option<Action> {
    let object = &state.catalog.objects[index];
    let selected = state.selected.contains(&index);
    let editing = state
        .rename
        .as_ref()
        .is_some_and(|edit| edit.index == index);
    let response = ui
        .interact(
            rect,
            Id::new(("object-row", id, index)),
            if editing {
                Sense::hover()
            } else if icon {
                Sense::click()
            } else {
                Sense::click_and_drag()
            },
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(&object.table.name);
    if selected && (!editing || !icon) {
        if icon {
            ui.painter().rect(
                rect,
                4.,
                Color32::from_rgb(212, 237, 218),
                Stroke::new(1., GREEN),
                StrokeKind::Inside,
            );
        } else {
            ui.painter()
                .rect_filled(rect, 0, Color32::from_rgb(53, 146, 203));
        }
    } else if response.hovered() && !editing {
        ui.painter().rect_filled(
            rect,
            if icon { 4. } else { 0. },
            if icon {
                Color32::from_rgb(240, 249, 242)
            } else {
                Color32::from_rgb(216, 230, 242)
            },
        );
    }
    let asset = if object.view {
        "images/connect/view.png"
    } else {
        "images/connect/table.png"
    };
    if let Some(texture) = icons.get(ui.ctx(), asset) {
        let at = if icon {
            Rect::from_center_size(pos2(rect.center().x, rect.top() + 17.), vec2(32., 32.))
        } else {
            Rect::from_min_size(rect.min + vec2(8., 4.), vec2(16., 16.))
        };
        ui.painter().image(
            texture,
            at,
            Rect::from_min_max(Pos2::ZERO, pos2(1., 1.)),
            Color32::WHITE,
        );
    }
    if !editing && icon {
        let font = crate::native_dialog::font(ui.ctx(), 12.);
        let galley = ui.painter().layout_job(egui::text::LayoutJob {
            text: object.table.name.clone(),
            sections: vec![egui::text::LayoutSection {
                leading_space: 0.,
                byte_range: 0.into()..object.table.name.len().into(),
                format: egui::TextFormat {
                    font_id: font,
                    color: Color32::from_gray(51),
                    ..Default::default()
                },
            }],
            wrap: egui::text::TextWrapping {
                max_width: 76.,
                max_rows: 1,
                break_anywhere: true,
                ..Default::default()
            },
            ..Default::default()
        });
        ui.painter().galley(
            pos2(rect.center().x - galley.size().x / 2., rect.top() + 37.),
            galley,
            Color32::from_gray(51),
        );
    }
    let mut action = None;
    if response.clicked() {
        ui.memory_mut(|memory| memory.request_focus(Id::new(("objects-focus", id))));
        let modifiers = ui.input(|input| input.modifiers);
        state.rename_due = None;
        if state.rename.is_none()
            && state.selected.len() == 1
            && state.selected.contains(&index)
            && modifiers.is_none()
            && !response.double_clicked()
            && !response.triple_clicked()
        {
            state.rename_due = Some((index, ui.input(|i| i.time) + 0.3));
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(300));
        }
        if !icon {
            state.click_detail(index, modifiers.ctrl, modifiers.shift);
        } else if !response.double_clicked() {
            state.click(index, modifiers.ctrl, modifiers.shift);
        }
    }
    if response.double_clicked() {
        state.rename_due = None;
        state.rename = None;
        state.rename_editor.release(ui.ctx());
        state.rename_editor.reset();
        action = Some(if icon {
            Action::OpenSelected { structure: false }
        } else {
            Action::Open(index)
        });
    }
    response.context_menu(|ui| {
        if !state.selected.contains(&index) {
            state.click(index, false, false);
        }
        if let Some(next) = menu(ui, state, icons) {
            action = Some(next);
        }
    });
    action
}

pub(super) fn menu(ui: &mut Ui, state: &mut State, icons: &mut Icons) -> Option<Action> {
    let mut action = None;
    state.rename_due = None;
    let index = state.primary;

    for (label, operation) in [
        ("打开表", 0),
        ("设计表", 1),
        ("新建表", 2),
        ("新建查询", 3),
        ("复制表", 8),
        ("重命名", 11),
        ("清空表", 5),
        ("截断表", 6),
        ("删除表", 4),
        ("刷新", 10),
    ] {
        if matches!(operation, 8 | 5 | 4 | 10) {
            crate::compact_menu::separator(ui);
        }
        let enabled = (!matches!(operation, 0 | 1 | 4 | 8) || !state.selected.is_empty())
            && (operation != 11 || state.selected.len() == 1)
            && (!matches!(operation, 5 | 6)
                || state
                    .selected
                    .iter()
                    .any(|index| !state.catalog.objects[*index].view));
        let asset = match operation {
            0 => "table_open.png",
            1 => "table_edit.png",
            2 => "table_add.png",
            3 => "query.png",
            8 => "copy_tables.png",
            10 => "refresh.png",
            11 => "rename.png",
            _ => "table_drop.png",
        };
        let texture = icons.get(ui.ctx(), &format!("images/connect/{asset}"));
        let clicked = ui
            .add_enabled(enabled, crate::compact_menu::Item::new(label).icon(texture))
            .clicked();
        if clicked {
            match operation {
                0 => action = Some(Action::OpenSelected { structure: false }),
                1 => action = Some(Action::OpenSelected { structure: true }),
                2 => action = Some(Action::NewTable),
                3 => action = Some(Action::NewQuery),
                4 => state.confirm(Mutation::Drop),
                5 => state.confirm(Mutation::Clear),
                6 => state.confirm(Mutation::Truncate),
                8 => action = Some(Action::Copy),
                11 => {
                    if let Some(index) = index {
                        // The closing popup owns focus until the end of this frame.
                        state.rename_due = Some((index, ui.input(|i| i.time) + 0.001));
                        ui.ctx().request_repaint();
                    }
                }
                _ => action = Some(Action::Refresh),
            }
            ui.close();
        }
    }
    action
}

pub(super) fn icons(ui: &mut Ui, state: &mut State, icons: &mut Icons, id: &str) -> Option<Action> {
    let bounds = ui.available_rect_before_wrap();
    let columns = (((ui.available_width() - 24. + 16.) / 96.).floor() as usize).max(1);
    let count = state.filtered.len().div_ceil(columns);
    let mut action = None;
    let mut origin = ui.cursor().min;
    let output = ScrollArea::vertical()
        .id_salt(("object-icons", id))
        .auto_shrink([false, false])
        .show_rows(ui, 70., count, |ui, rows| {
            let mut editor = None;
            for row in rows {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 70.), Sense::hover());
                origin = rect.min - vec2(0., row as f32 * 70.);
                for column in 0..columns {
                    let Some(index) = state.filtered.get(row * columns + column).copied() else {
                        break;
                    };
                    let tile = Rect::from_min_size(
                        rect.min + vec2(12. + column as f32 * 96., 12.),
                        vec2(80., 54.),
                    );
                    if let Some(next) = entry(ui, state, index, tile, true, icons, id) {
                        action = Some(next);
                    }
                    if state
                        .rename
                        .as_ref()
                        .is_some_and(|edit| edit.index == index)
                    {
                        editor = Some((index, tile));
                    }
                }
            }
            if let Some((index, tile)) = editor
                && let Some(next) = rename(ui, state, index, tile, true, id)
            {
                action = Some(next);
            }
        });
    marquee(ui, state, output.inner_rect, origin, columns, true, id);
    scrollbars(ui, state, output, bounds);
    if state.filtered.is_empty() {
        ui.painter().text(
            ui.clip_rect().center(),
            Align2::CENTER_CENTER,
            "暂无对象",
            crate::native_dialog::font(ui.ctx(), 12.),
            Color32::from_gray(51),
        );
    }
    action
}

pub(super) fn details(
    ui: &mut Ui,
    state: &mut State,
    icons: &mut Icons,
    id: &str,
) -> Option<Action> {
    let mut action = None;
    if state.widths_pending {
        let font = crate::native_dialog::font(ui.ctx(), 12.);
        for (column, title) in [
            "名",
            "自动递增值",
            "修改日期",
            "数据长度",
            "引擎",
            "行",
            "注释",
        ]
        .into_iter()
        .enumerate()
        {
            let measure = |text: &str| {
                ui.painter()
                    .layout_no_wrap(text.to_owned(), font.clone(), Color32::BLACK)
                    .size()
                    .x
            };
            let mut width = measure(title) + 36.;
            for index in &state.detail_rows {
                width = width.max(
                    measure(window::field(&state.catalog.objects[*index], column))
                        + 16.
                        + if column == 0 { 22. } else { 0. },
                );
            }
            state.widths[column] = width.clamp(60., 400.);
        }
        state.widths_pending = false;
    }
    let width: f32 = state.widths.iter().sum();
    let mut canvas = None;
    let bounds = ui.available_rect_before_wrap();
    let output = ScrollArea::horizontal()
        .id_salt(("object-detail-x", id))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(width);
            let (header, _) =
                ui.allocate_exact_size(vec2(width.max(ui.available_width()), 25.), Sense::hover());
            ui.painter().rect_filled(header, 0, Color32::WHITE);
            let mut x = header.left();
            for (column, label) in [
                "名",
                "自动递增值",
                "修改日期",
                "数据长度",
                "引擎",
                "行",
                "注释",
            ]
            .into_iter()
            .enumerate()
            {
                let rect =
                    Rect::from_min_size(pos2(x, header.top()), vec2(state.widths[column], 25.));
                ui.painter().line_segment(
                    [rect.right_top(), rect.right_bottom()],
                    Stroke::new(1., Color32::from_gray(229)),
                );
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    label,
                    bold(ui),
                    Color32::from_gray(51),
                );
                if ui
                    .interact(rect, Id::new(("object-sort", id, column)), Sense::click())
                    .clicked()
                {
                    state.cycle_sort(column);
                }
                let resize = Rect::from_center_size(rect.right_center(), vec2(6., 25.));
                let response = ui
                    .interact(
                        resize,
                        Id::new(("object-resize", id, column)),
                        Sense::drag(),
                    )
                    .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                if response.dragged() {
                    state.widths[column] =
                        (state.widths[column] + response.drag_delta().x).clamp(60., 400.);
                }
                x += state.widths[column];
            }
            let mut origin = ui.cursor().min;
            let bounds = ui.available_rect_before_wrap();
            let output = ScrollArea::vertical()
                .id_salt(("object-detail-y", id))
                .auto_shrink([false, false])
                .show_rows(ui, 24., state.detail_rows.len(), |ui, rows| {
                    let mut editor = None;
                    for row in rows {
                        let index = state.detail_rows[row];
                        let (rect, _) = ui.allocate_exact_size(
                            vec2(width.max(ui.available_width()), 24.),
                            Sense::hover(),
                        );
                        origin = rect.min - vec2(0., row as f32 * 24.);
                        let data = Rect::from_min_size(rect.min, vec2(width, 24.));
                        if let Some(next) = entry(ui, state, index, data, false, icons, id) {
                            action = Some(next);
                        }
                        let mut x = rect.left();
                        for column in 0..7 {
                            let cell = Rect::from_min_size(
                                pos2(x, rect.top()),
                                vec2(state.widths[column], 24.),
                            );
                            let painter =
                                ui.painter().with_clip_rect(cell.intersect(ui.clip_rect()));
                            if !(column == 0
                                && state
                                    .rename
                                    .as_ref()
                                    .is_some_and(|rename| rename.index == index))
                            {
                                painter.text(
                                    cell.left_center()
                                        + vec2(if column == 0 { 30. } else { 8. }, 0.),
                                    Align2::LEFT_CENTER,
                                    window::field(&state.catalog.objects[index], column),
                                    crate::native_dialog::font(ui.ctx(), 12.),
                                    if state.selected.contains(&index) {
                                        Color32::WHITE
                                    } else {
                                        Color32::BLACK
                                    },
                                );
                            }
                            if column == 0
                                && state
                                    .rename
                                    .as_ref()
                                    .is_some_and(|edit| edit.index == index)
                            {
                                editor = Some((index, cell));
                            }
                            x += state.widths[column];
                        }
                    }
                    if let Some((index, cell)) = editor
                        && let Some(next) = rename(ui, state, index, cell, false, id)
                    {
                        action = Some(next);
                    }
                });
            canvas = Some((output.inner_rect, origin));
            scrollbars(ui, state, output, bounds);
        });
    if let Some((bounds, origin)) = canvas {
        marquee(ui, state, bounds, origin, 1, false, id);
    }
    scrollbars(ui, state, output, bounds);
    action
}
fn marquee(
    ui: &Ui,
    state: &mut State,
    bounds: Rect,
    origin: Pos2,
    columns: usize,
    icon: bool,
    id: &str,
) {
    if !ui.is_enabled() || egui::Popup::is_any_open(ui.ctx()) {
        state.drag_selection = None;
        return;
    }
    let rows = if icon {
        &state.state.filtered
    } else {
        &state.detail_rows
    };
    let width = state.widths.iter().sum();
    let rect_for = |ordinal: usize| {
        if icon {
            Rect::from_min_size(
                origin
                    + vec2(
                        12. + (ordinal % columns) as f32 * 96.,
                        12. + (ordinal / columns) as f32 * 70.,
                    ),
                vec2(80., 54.),
            )
        } else {
            Rect::from_min_size(origin + vec2(0., ordinal as f32 * 24.), vec2(width, 24.))
        }
    };
    let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) else {
        return;
    };
    let (pressed, down, dragged, modifiers) = ui.input(|input| {
        (
            input.pointer.primary_pressed(),
            input.pointer.primary_down(),
            input.pointer.is_decidedly_dragging(),
            input.modifiers,
        )
    });
    let row_at = |y: f32| ((y - origin.y) / 24.).floor().max(0.) as usize;
    if state.rename.is_none() && pressed && ui.rect_contains_pointer(bounds) {
        let hit = if icon {
            rows.iter()
                .enumerate()
                .find(|(ordinal, _)| rect_for(*ordinal).contains(pointer))
                .map(|(ordinal, _)| ordinal)
        } else {
            let ordinal = row_at(pointer.y);
            (ordinal < rows.len() && rect_for(ordinal).contains(pointer)).then_some(ordinal)
        };
        state.drag_selection = match hit {
            Some(row) if !icon && !modifiers.ctrl && !modifiers.shift => {
                Some(DragSelection::Rows(row))
            }
            None if icon || (!modifiers.ctrl && !modifiers.shift) => {
                if !modifiers.ctrl {
                    state.state.selected.clear();
                    state.state.selection_order.clear();
                    state.state.primary = None;
                    state.state.anchor = None;
                }
                Some(DragSelection::Marquee(pointer - origin))
            }
            _ => None,
        };
        if state.drag_selection.is_some() {
            state.rename_due = None;
            ui.memory_mut(|m| m.request_focus(Id::new(("objects-focus", id))));
        }
    }
    if let Some(DragSelection::Rows(first)) = state.drag_selection
        && dragged
        && first < rows.len()
    {
        let last = row_at(pointer.y).min(rows.len() - 1);
        let range = &rows[first.min(last)..=first.max(last)];
        state.state.selected.clear();
        state.state.selected.extend(range.iter().copied());
        state.state.selection_order.clear();
        state.state.selection_order.extend_from_slice(range);
        state.state.anchor = Some(rows[first]);
        state.state.primary = Some(rows[last]);
        state.rename_due = None;
        ui.ctx().request_repaint();
    } else if let Some(DragSelection::Marquee(start)) = state.drag_selection {
        let rect = Rect::from_two_pos(origin + start, pointer);
        if !modifiers.ctrl {
            state.state.selected.clear();
            state.state.selection_order.clear();
            state.state.primary = None;
        }
        for (ordinal, index) in rows.iter().enumerate() {
            let item = rect_for(ordinal);
            if item.left() < rect.right()
                && item.right() > rect.left()
                && item.top() < rect.bottom()
                && item.bottom() > rect.top()
                && state.state.selected.insert(*index)
            {
                state.state.selection_order.push(*index);
            }
        }
        if state.state.primary.is_none() {
            state.state.primary = state.state.selection_order.first().copied();
        }
        if down {
            ui.painter().with_clip_rect(bounds).rect(
                rect,
                0,
                Color32::from_rgba_unmultiplied(53, 146, 203, 38),
                Stroke::new(1., Color32::from_rgb(53, 146, 203)),
                StrokeKind::Inside,
            );
        }
    }
    if !down {
        state.drag_selection = None;
    }
}
fn rename(
    ui: &mut Ui,
    state: &mut State,
    index: usize,
    rect: Rect,
    icon: bool,
    id: &str,
) -> Option<Action> {
    if !state
        .rename
        .as_ref()
        .is_some_and(|edit| edit.index == index)
    {
        return None;
    }
    let editor_id = state
        .rename_editor
        .prepare_id(ui.ctx(), egui::Id::new(("object-rename", id, index)));
    let edit = state.rename.as_mut().filter(|edit| edit.index == index)?;
    let fresh = edit.focus;
    if fresh {
        ui.memory_mut(|m| m.request_focus(editor_id));
        state
            .rename_editor
            .select(ui.ctx(), 0..edit.text.chars().count());
        edit.focus = false;
    }
    let rect = if icon {
        Rect::from_min_size(pos2(rect.left(), rect.top() + 33.), vec2(rect.width(), 24.))
    } else {
        Rect::from_min_max(rect.min + vec2(28., 1.), rect.max - vec2(1., 1.))
    };
    let font = crate::native_dialog::font(ui.ctx(), 12.);
    let width = (ui
        .painter()
        .layout_no_wrap(edit.text.clone(), font.clone(), Color32::BLACK)
        .size()
        .x
        + 20.)
        .max(rect.width())
        .min(ui.clip_rect().width().max(rect.width()));
    let rect = if icon {
        Rect::from_center_size(rect.center(), vec2(width, rect.height()))
    } else {
        Rect::from_min_size(rect.min, vec2(width, rect.height()))
    };
    let rect = rect.translate(vec2(
        (ui.clip_rect().left() - rect.left()).max(0.)
            - (rect.right() - ui.clip_rect().right()).max(0.),
        0.,
    ));
    let response = ui.put(
        rect,
        TextEdit::singleline(&mut edit.text)
            .id(editor_id)
            .font(font)
            .margin(vec2(6., 0.))
            .vertical_align(egui::Align::Center)
            .horizontal_align(if icon {
                egui::Align::Center
            } else {
                egui::Align::LEFT
            })
            .frame(egui::Frame::new().fill(Color32::WHITE).stroke(Stroke::new(
                if icon { 1.5 } else { 1. },
                Color32::from_rgb(53, 146, 203),
            )))
            .char_limit(1024),
    );
    if let Some(text) = egui::widgets::text_edit::TextEditState::load(ui.ctx(), response.id) {
        text.trim_undo_memory(128 * 1024);
        text.store(ui.ctx(), response.id);
    }
    if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        response.surrender_focus();
        state.rename = None;
        state.state.selected.clear();
        state.state.selection_order.clear();
        state.state.primary = None;
        state.state.anchor = None;
        state.rename_editor.release(ui.ctx());
        state.rename_editor.reset();
        return None;
    }
    if (response.has_focus()
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)))
        || response.lost_focus()
        || !ui.input(|input| input.focused)
        || (!fresh
            && ui.input(|input| {
                input.pointer.any_pressed()
                    && input
                        .pointer
                        .interact_pos()
                        .is_some_and(|point| !response.rect.contains(point))
            }))
    {
        response.surrender_focus();
        let edit = state.rename.take()?;
        state.state.selected.clear();
        state.state.selection_order.clear();
        state.state.primary = None;
        state.state.anchor = None;
        state.rename_editor.release(ui.ctx());
        state.rename_editor.reset();
        return Some(Action::Rename(index, edit.text));
    }
    None
}

#[cfg(test)]
mod menu_metrics_tests {
    use egui::{Event, PointerButton, Pos2, Rect, Vec2, pos2, vec2};

    #[test]
    fn actual_objects_popup_shrinks_and_keeps_full_width_click_targets() {
        for scale in [1., 1.25, 1.5, 2.] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            ctx.set_pixels_per_point(scale);
            let mut draft = crate::tabs::database_objects::window::Draft::default();
            let mut icons = crate::icons::Icons::default();
            let mut frame = 0;
            let mut run = |events| {
                frame += 1;
                let mut popup = None;
                let mut action = None;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 900.))),
                        time: Some(frame as f64 * 0.05),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        ui.spacing_mut().interact_size.y = 100.;
                        let (_, target) =
                            ui.allocate_exact_size(vec2(900., 600.), egui::Sense::click());
                        popup = target
                            .context_menu(|ui| action = super::menu(ui, &mut draft, &mut icons))
                            .map(|menu| menu.response.rect);
                    },
                );
                output.textures_delta.clear();
                (popup, action)
            };
            let pointer = |pos, button, pressed| {
                vec![
                    Event::PointerMoved(pos),
                    Event::PointerButton {
                        pos,
                        button,
                        pressed,
                        modifiers: Default::default(),
                    },
                ]
            };
            run(vec![]);
            for pressed in [true, false] {
                run(pointer(pos2(300., 200.), PointerButton::Secondary, pressed));
            }
            let mut rect = Rect::NOTHING;
            for _ in 0..4 {
                rect = run(vec![]).0.expect("actual objects popup must stay open");
                assert!(
                    (150.0..180.0).contains(&rect.width()),
                    "popup inherited suggested area width at {scale}: {rect:?}"
                );
            }
            let point = rect.right_bottom() - Vec2::new(8., 16.);
            run(pointer(point, PointerButton::Primary, true));
            let (_, action) = run(pointer(point, PointerButton::Primary, false));
            assert!(matches!(action, Some(super::Action::Refresh)));
            assert!(run(vec![]).0.is_none(), "click must close the popup");
        }
    }

    #[test]
    fn actual_objects_menu_has_compact_equal_rows() {
        let mut draft = crate::tabs::database_objects::window::Draft::default();
        let mut icons = crate::icons::Icons::default();
        crate::compact_menu::assert_actual_rows(
            |ui| {
                let (_, response) =
                    ui.allocate_exact_size(egui::vec2(200., 200.), egui::Sense::click());
                response.context_menu(|ui| {
                    super::menu(ui, &mut draft, &mut icons);
                });
            },
            egui::pos2(90., 45.),
            &["打开表", "设计表", "新建表", "新建查询"],
        );
    }
}
