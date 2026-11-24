//! Virtual result grid with a fixed row selector/header and borrowed cell payloads.
use crate::tabs::query_results::{Cell, State};
use egui::{Align2, Color32, FontId, Id, Key, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use tomato_services::QueryResult;
mod batch;
mod editing;
pub(super) mod filter_dialog;
mod scrollbar;
pub(in crate::panels) mod temporal;
mod transfer;
mod view;
const ROW: f32 = 28.;
const HEADER: f32 = 28.;
const SELECTOR: f32 = 15.;
const DATA: f32 = SELECTOR + 1.;

fn copy(ui: &Ui, result: &QueryResult, state: &mut State) {
    match state.copy(result) {
        Ok(Some(text)) => {
            state.error.clear();
            ui.ctx().copy_text(text);
        }
        Ok(None) => {
            state.error.clear();
        }
        Err(error) => state.error = error.to_string(),
    }
}
fn keyboard(
    ui: &mut Ui,
    result: &QueryResult,
    state: &mut State,
    id: Id,
    body_height: f32,
) -> bool {
    if !ui.memory(|memory| memory.has_focus(id)) {
        return false;
    }
    let mut reveal = false;
    let events = ui.input(|input| {
        input
            .events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    egui::Event::Copy | egui::Event::Paste(_) | egui::Event::Key { .. }
                )
            })
            .cloned()
            .collect::<Vec<_>>()
    });
    for event in events {
        match event {
            egui::Event::Copy => copy(ui, result, state),
            egui::Event::Paste(text) => {
                if let Err(error) = state.paste(result, &text) {
                    state.error = error.to_string();
                }
            }
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                if key == Key::A && modifiers.command {
                    state.select_all();
                    continue;
                }
                if state.visible_rows() == 0 || state.visible_columns().is_empty() {
                    continue;
                }
                let mut cell = state.focus.unwrap_or(Cell {
                    row: 0,
                    column: state.visible_columns()[0],
                });
                if matches!(key, Key::F2 | Key::Enter) {
                    if let Some(editing) = &state.editing {
                        editing.release(ui.ctx());
                    }
                    state.start_edit(result, cell);
                    continue;
                }
                match key {
                    Key::ArrowUp => cell.row = cell.row.saturating_sub(1),
                    Key::ArrowDown => cell.row = (cell.row + 1).min(state.visible_rows() - 1),
                    Key::ArrowLeft => {
                        cell.column = state
                            .column_neighbor(cell.column, -1)
                            .unwrap_or(cell.column)
                    }
                    Key::ArrowRight => {
                        cell.column = state.column_neighbor(cell.column, 1).unwrap_or(cell.column)
                    }
                    Key::Home => {
                        cell.column = state.visible_columns()[0];
                        if modifiers.command {
                            cell.row = 0;
                        }
                    }
                    Key::End => {
                        cell.column = *state.visible_columns().last().unwrap();
                        if modifiers.command {
                            cell.row = state.visible_rows() - 1;
                        }
                    }
                    Key::PageUp => {
                        cell.row = cell
                            .row
                            .saturating_sub((body_height / ROW).max(1.) as usize)
                    }
                    Key::Tab if !modifiers.command => {
                        if modifiers.shift {
                            if let Some(column) = state.column_neighbor(cell.column, -1) {
                                cell.column = column;
                            } else if cell.row > 0 {
                                cell.row -= 1;
                                cell.column = *state.visible_columns().last().unwrap();
                            }
                        } else if let Some(column) = state.column_neighbor(cell.column, 1) {
                            cell.column = column;
                        } else if cell.row + 1 < state.visible_rows() {
                            cell.row += 1;
                            cell.column = state.visible_columns()[0];
                        }
                    }
                    Key::PageDown => {
                        cell.row = (cell.row + (body_height / ROW).max(1.) as usize)
                            .min(state.visible_rows() - 1)
                    }
                    _ => continue,
                }
                state.select(cell, modifiers.shift, false, false);
                state.end_drag();
                reveal = true;
            }
            _ => {}
        }
    }
    reveal
}
pub(super) fn render(ui: &mut Ui, result: &QueryResult, state: &mut State, id: Id) {
    state.initialize(result);
    if !ui.input(|input| input.pointer.primary_down()) {
        state.end_drag();
    }
    let visible_columns = state.visible_columns();
    let total_width = DATA
        + visible_columns
            .iter()
            .map(|column| state.widths[*column])
            .sum::<f32>();
    let total_height = HEADER + state.visible_rows() as f32 * ROW;
    let table_fonts = ui.ctx().fonts(|fonts| {
        fonts
            .definitions()
            .families
            .contains_key(&egui::FontFamily::Name("table".into()))
    });
    if table_fonts {
        crate::panels::editor::ensure_bold_cjk(ui.ctx());
    }
    let font = if table_fonts {
        FontId::new(10., egui::FontFamily::Name("table".into()))
    } else {
        FontId::proportional(10.)
    };
    let header_font = if table_fonts {
        FontId::new(10., egui::FontFamily::Name("table-bold".into()))
    } else {
        font.clone()
    };
    ui.painter()
        .rect_filled(ui.available_rect_before_wrap(), 0, Color32::from_gray(244));
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    ui.spacing_mut().scroll.bar_width = 12.;
    let mut outer = ui.available_rect_before_wrap();
    if !state.error.is_empty() {
        outer.max.y = (outer.max.y - 30.).max(outer.min.y);
    }
    let mut vertical = total_height > outer.height() - 2.;
    let mut horizontal = total_width > outer.width() - if vertical { 14. } else { 1. };
    for _ in 0..2 {
        vertical = total_height > outer.height() - 2. - if horizontal { 13. } else { 0. };
        horizontal = total_width > outer.width() - if vertical { 14. } else { 1. };
    }
    let pane = Rect::from_min_max(
        outer.min + Vec2::new(0., 1.),
        outer.max
            - Vec2::new(
                if vertical { 14. } else { 1. },
                if horizontal { 14. } else { 1. },
            ),
    );
    ui.painter().rect_filled(pane, 0, Color32::WHITE);
    ui.painter().rect_filled(
        Rect::from_min_max(pane.min, Pos2::new(outer.right() - 1., pane.top() + HEADER)),
        0,
        Color32::WHITE,
    );
    let enabled = state
        .editing
        .as_ref()
        .is_some_and(|editing| editing.enabled && editing.editor.is_none());
    let batch_ready =
        super::batch_input::handle(ui, id, enabled, &mut batch::Input { state, result });
    let modal = state.view.dialog.is_some() || !batch_ready;
    let reveal = !modal && keyboard(ui, result, state, id, (pane.height() - HEADER).max(0.));
    let mut output = ui
        .scope_builder(egui::UiBuilder::new().max_rect(pane), |ui| {
            if modal {
                ui.disable();
            }
            egui::ScrollArea::both()
                .id_salt(id)
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .scroll_source(egui::scroll_area::ScrollSource {
                    drag: egui::scroll_area::DragScroll::Never,
                    ..Default::default()
                })
                .animated(false)
                .show_viewport(ui, |ui, viewport| {
                    let origin = ui.cursor().min;
                    let visible_left = origin.x + viewport.min.x;
                    let visible_top = origin.y + viewport.min.y;
                    let width = total_width.max(viewport.width());
                    ui.allocate_space(Vec2::new(width, total_height));
                    let body_clip = Rect::from_min_max(
                        Pos2::new(visible_left, visible_top + HEADER),
                        ui.clip_rect().max,
                    )
                    .intersect(ui.clip_rect());
                    let painter = ui.painter().with_clip_rect(body_clip);
                    let cells_clip = body_clip.intersect(Rect::from_min_max(
                        Pos2::new(visible_left + DATA, body_clip.min.y),
                        body_clip.max,
                    ));
                    let response = ui.interact(body_clip, id, Sense::click());
                    let first = (viewport.min.y / ROW).floor().max(0.) as usize;
                    let last =
                        ((viewport.max.y / ROW).ceil() as usize + 1).min(state.visible_rows());
                    let pointer = ui.input(|input| input.pointer.interact_pos());
                    let pressed = !modal && ui.input(|input| input.pointer.primary_pressed());
                    let secondary = !modal && ui.input(|input| input.pointer.secondary_pressed());
                    let dragging = !modal && ui.input(|input| input.pointer.primary_down());
                    let modifiers = ui.input(|input| {
                        input
                            .events
                            .iter()
                            .rev()
                            .find_map(|event| match event {
                                egui::Event::PointerButton {
                                    button: egui::PointerButton::Primary,
                                    pressed: true,
                                    modifiers,
                                    ..
                                } => Some(*modifiers),
                                _ => None,
                            })
                            .unwrap_or(input.modifiers)
                    });
                    for row in first..last {
                        let top = origin.y + HEADER + row as f32 * ROW;
                        let rect =
                            Rect::from_min_size(Pos2::new(origin.x, top), Vec2::new(width, ROW));
                        painter.rect_filled(rect, 0, Color32::WHITE);
                        let mut x = origin.x + DATA;
                        for column in visible_columns.iter().copied() {
                            let rect = Rect::from_min_size(
                                Pos2::new(x, top),
                                Vec2::new(state.widths[column], ROW),
                            );
                            x += state.widths[column];
                            if !rect.intersects(cells_clip) {
                                continue;
                            }
                            let cell = Cell { row, column };
                            let selected = state.contains(cell);
                            let cell_painter = painter.with_clip_rect(rect.intersect(cells_clip));
                            if selected {
                                cell_painter.rect_filled(rect, 0, Color32::from_rgb(53, 146, 203));
                            }
                            let data_row = state.row(row);
                            if !modal
                                && enabled
                                && ui.memory(|memory| memory.has_focus(id))
                                && state.focus == Some(cell)
                            {
                                super::batch_input::ime(ui, rect.intersect(cells_clip));
                            }
                            let null = state.display_value(result, data_row, column).is_none();
                            let text = if let Some(text) = state
                                .editing
                                .as_ref()
                                .and_then(|editing| editing.batch.as_ref())
                                .and_then(|batch| batch.preview(data_row, column))
                            {
                                text
                            } else if null {
                                "NULL"
                            } else {
                                state.display_value(result, data_row, column).unwrap_or("")
                            };
                            cell_painter.text(
                                rect.left_center() + Vec2::new(6., 0.),
                                Align2::LEFT_CENTER,
                                text,
                                font.clone(),
                                if selected {
                                    Color32::WHITE
                                } else if null {
                                    Color32::from_gray(153)
                                } else {
                                    Color32::from_gray(51)
                                },
                            );
                            cell_painter.line_segment(
                                [
                                    rect.right_top() - Vec2::new(0.5, 0.),
                                    rect.right_bottom() - Vec2::splat(0.5),
                                ],
                                Stroke::new(1., Color32::from_gray(224)),
                            );
                            cell_painter.line_segment(
                                [
                                    rect.left_bottom() - Vec2::new(0., 0.5),
                                    rect.right_bottom() - Vec2::splat(0.5),
                                ],
                                Stroke::new(1., Color32::from_gray(224)),
                            );
                            let editing_cell = state
                                .editing
                                .as_ref()
                                .and_then(|editing| editing.editor.as_ref())
                                .is_some_and(|editor| {
                                    editor.row == data_row && editor.column == column
                                });
                            if !modal
                                && !editing_cell
                                && pointer
                                    .is_some_and(|point| rect.intersect(cells_clip).contains(point))
                            {
                                if pressed {
                                    state.select(cell, modifiers.shift, modifiers.command, false);
                                    ui.memory_mut(|memory| memory.request_focus(id));
                                } else if secondary {
                                    if !selected {
                                        state.select(cell, false, false, false);
                                        state.end_drag();
                                    }
                                    ui.memory_mut(|memory| memory.request_focus(id));
                                } else if dragging {
                                    state.drag_to(cell);
                                }
                            }
                            if !modal
                                && !editing_cell
                                && pointer
                                    .is_some_and(|point| rect.intersect(cells_clip).contains(point))
                                && ui.input(|input| {
                                    input
                                        .pointer
                                        .button_double_clicked(egui::PointerButton::Primary)
                                })
                            {
                                if let Some(editing) = &state.editing {
                                    editing.release(ui.ctx());
                                }
                                state.start_edit(result, cell);
                            }
                            editing::editor(
                                ui,
                                result,
                                state,
                                cell,
                                rect.intersect(cells_clip),
                                id,
                            );
                        }
                        let scroll_fraction = if total_height > viewport.height() {
                            viewport.min.y / (total_height - viewport.height())
                        } else {
                            0.
                        };
                        let selector = Rect::from_min_size(
                            Pos2::new(visible_left, top - 1. + scroll_fraction),
                            Vec2::new(SELECTOR, ROW),
                        );
                        painter.rect_filled(selector, 0, Color32::WHITE);
                        painter.line_segment(
                            [
                                selector.left_bottom() - egui::vec2(0., 0.5),
                                selector.right_bottom() - egui::Vec2::splat(0.5),
                            ],
                            Stroke::new(1., Color32::from_rgb(190, 190, 188)),
                        );
                        if state.row_selected(row) {
                            let center = selector.center();
                            painter.add(egui::Shape::convex_polygon(
                                vec![
                                    center + Vec2::new(-2.5, -5.),
                                    center + Vec2::new(2.5, 0.),
                                    center + Vec2::new(-2.5, 5.),
                                ],
                                Color32::BLACK,
                                Stroke::NONE,
                            ));
                        }
                        if pointer
                            .is_some_and(|point| selector.intersect(body_clip).contains(point))
                            && !result.columns.is_empty()
                        {
                            let cell = Cell {
                                row,
                                column: visible_columns.first().copied().unwrap_or(0),
                            };
                            if pressed {
                                state.select(cell, modifiers.shift, modifiers.command, true);
                                ui.memory_mut(|memory| memory.request_focus(id));
                            } else if dragging {
                                state.drag_to(cell);
                            }
                        }
                    }
                    if reveal && let Some(cell) = state.focus {
                        let x = origin.x
                            + DATA
                            + visible_columns
                                .iter()
                                .take_while(|column| **column != cell.column)
                                .map(|column| state.widths[*column])
                                .sum::<f32>();
                        let rect = Rect::from_min_size(
                            Pos2::new(x, origin.y + HEADER + cell.row as f32 * ROW),
                            Vec2::new(state.widths[cell.column], ROW),
                        );
                        ui.scroll_to_rect(rect.expand2(Vec2::new(0., HEADER)), None);
                    }
                    if dragging
                        && state.dragging()
                        && let Some(point) = pointer
                    {
                        let step = if point.y < body_clip.min.y {
                            ROW
                        } else if point.y > body_clip.max.y {
                            -ROW
                        } else {
                            0.
                        };
                        let horizontal = if state.row_drag() {
                            0.
                        } else if point.x < body_clip.min.x {
                            24.
                        } else if point.x > body_clip.max.x {
                            -24.
                        } else {
                            0.
                        };
                        if step != 0. || horizontal != 0. {
                            if state.visible_rows() > 0 && !visible_columns.is_empty() {
                                let row =
                                    ((point.y - origin.y - HEADER) / ROW).floor().max(0.) as usize;
                                let mut right = origin.x + DATA;
                                let column = visible_columns
                                    .iter()
                                    .copied()
                                    .find(|column| {
                                        right += state.widths[*column];
                                        point.x < right
                                    })
                                    .unwrap_or(*visible_columns.last().unwrap());
                                state.drag_to(Cell {
                                    row: row.min(state.visible_rows() - 1),
                                    column,
                                });
                            }
                            ui.scroll_with_delta(Vec2::new(horizontal, step));
                            ui.ctx()
                                .request_repaint_after(std::time::Duration::from_millis(16));
                        }
                    }
                    // Header is painted after cells and stays at the top while rows scroll.
                    let header = Rect::from_min_size(
                        Pos2::new(visible_left, visible_top),
                        Vec2::new(viewport.width(), HEADER),
                    );
                    ui.painter().rect_filled(header, 0, Color32::WHITE);
                    // The original frozen TableView omits the data table's 1px inset.
                    let selector_header = Rect::from_min_size(
                        header.min - Vec2::new(0., 1.),
                        Vec2::new(SELECTOR, HEADER),
                    );
                    ui.painter()
                        .rect_filled(selector_header, 0, Color32::from_gray(245));
                    ui.painter().line_segment(
                        [
                            selector_header.left_bottom() - Vec2::new(0., 0.5),
                            selector_header.right_bottom() - Vec2::splat(0.5),
                        ],
                        Stroke::new(1., Color32::from_gray(204)),
                    );
                    let header_clip =
                        header
                            .intersect(ui.clip_rect())
                            .intersect(Rect::from_min_max(
                                header.min + Vec2::new(DATA, 0.),
                                header.max,
                            ));
                    let mut x = origin.x + DATA;
                    let mut sort = None;
                    if let Some(drag) = &mut state.view.dragging {
                        drag.target = None;
                    }
                    for column in visible_columns.iter().copied() {
                        let rect = Rect::from_min_size(
                            Pos2::new(x, visible_top),
                            Vec2::new(state.widths[column], HEADER),
                        );
                        x += state.widths[column];
                        if !rect.intersects(header_clip) {
                            continue;
                        }
                        let painter = ui.painter().with_clip_rect(rect.intersect(header_clip));
                        painter.rect_filled(rect, 0, Color32::from_gray(245));
                        painter.text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            &result.columns[column],
                            header_font.clone(),
                            Color32::from_gray(51),
                        );
                        painter.line_segment(
                            [
                                rect.right_top() - Vec2::new(0.5, 0.),
                                rect.right_bottom() - Vec2::splat(0.5),
                            ],
                            Stroke::new(1., Color32::from_rgb(218, 218, 216)),
                        );
                        painter.line_segment(
                            [
                                rect.left_bottom() - Vec2::new(0., 0.5),
                                rect.right_bottom() - Vec2::splat(0.5),
                            ],
                            Stroke::new(1., Color32::from_gray(204)),
                        );
                        if let Some((_, descending)) =
                            state.view.sorts.iter().find(|sort| sort.0 == column)
                        {
                            painter.text(
                                rect.right_center() - Vec2::new(5., 0.),
                                Align2::RIGHT_CENTER,
                                if *descending { "▼" } else { "▲" },
                                FontId::proportional(7.),
                                Color32::BLACK,
                            );
                        }
                        let resize = Rect::from_min_max(
                            rect.right_top() - Vec2::new(3., 0.),
                            rect.right_bottom() + Vec2::new(3., 0.),
                        )
                        .intersect(header_clip);
                        let handle = ui
                            .interact(resize, id.with(("resize", column)), Sense::drag())
                            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                        if handle.dragged() {
                            state.widths[column] =
                                (state.widths[column] + handle.drag_delta().x).max(10.);
                        }
                        let button = Rect::from_min_max(rect.min, rect.max - Vec2::new(3., 0.))
                            .intersect(header_clip);
                        let response = ui.interact(
                            button,
                            id.with(("header", column)),
                            Sense::click_and_drag(),
                        );
                        if response.clicked() {
                            sort = Some(column);
                            ui.memory_mut(|memory| memory.request_focus(id));
                        }
                        if response.drag_started() {
                            state.view.dragging = Some(crate::tabs::query_results::view::Drag {
                                column,
                                target: None,
                            });
                        }
                        if let Some(drag) = &mut state.view.dragging
                            && pointer.is_some_and(|point| rect.contains(point))
                        {
                            let after = pointer.unwrap().x >= rect.center().x;
                            drag.target = Some((column, after));
                            let x = if after { rect.right() } else { rect.left() };
                            painter.line_segment(
                                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                                Stroke::new(2., Color32::from_rgb(53, 146, 203)),
                            );
                        }
                    }
                    if let Some(column) = sort
                        && editing::finish(ui, result, state)
                    {
                        state.cycle_sort(result, column, ui.input(|input| input.modifiers.shift));
                    }
                    if ui.input(|input| input.pointer.primary_released())
                        && let Some(drag) = state.view.dragging.take()
                        && let Some((target, after)) = drag.target
                        && let Err(error) = state.move_column(drag.column, target, after)
                    {
                        state.error = error.to_string();
                    }
                    for x in [visible_left + 0.5, visible_left + SELECTOR - 0.5] {
                        ui.painter().line_segment(
                            [
                                Pos2::new(x, visible_top - 1.),
                                Pos2::new(x, visible_top + viewport.height()),
                            ],
                            Stroke::new(1., Color32::from_rgb(190, 190, 188)),
                        );
                    }
                    ui.painter().rect_filled(
                        Rect::from_min_size(
                            Pos2::new(visible_left + SELECTOR, visible_top),
                            Vec2::new(1., viewport.height()),
                        ),
                        0,
                        Color32::from_gray(244),
                    );
                    response.context_menu(|ui| {
                        if !editing::finish(ui, result, state) {
                            ui.disable();
                        }
                        let writable = state.editing.as_ref().is_some_and(|editing| {
                            editing.enabled && editing.target.plan.is_editable()
                        });
                        if ui
                            .add_enabled(writable, egui::Button::new("设置为空字符串"))
                            .clicked()
                        {
                            editing::set_selected(ui, result, state, Some(""));
                            ui.close();
                        }
                        if ui
                            .add_enabled(writable, egui::Button::new("设置为 NULL"))
                            .clicked()
                        {
                            editing::set_selected(ui, result, state, None);
                            ui.close();
                        }
                        ui.separator();
                        let delete = writable
                            && state
                                .editing
                                .as_ref()
                                .is_some_and(|editing| editing.target.plan.can_delete());
                        if ui
                            .add_enabled(delete, egui::Button::new("删除 记录"))
                            .clicked()
                            && let Some(editing) = &mut state.editing
                        {
                            editing.confirm_delete = true;
                            ui.close();
                        }
                        if ui.button("复制").clicked() {
                            copy(ui, result, state);
                            ui.close();
                        }
                        transfer::menu(ui, result, state, id, writable);
                        ui.separator();
                        view::menu(ui, result, state, state.focus.map(|cell| cell.column));
                    });
                    if state.visible_rows() == 0 {
                        ui.painter().text(
                            body_clip.center(),
                            Align2::CENTER_CENTER,
                            "无数据",
                            font.clone(),
                            Color32::from_gray(136),
                        );
                    }
                })
        })
        .inner;
    if !batch_ready {
        ui.memory_mut(|memory| memory.request_focus(id));
    }
    let mut offset = output.state.offset;
    if vertical {
        scrollbar::show(
            ui,
            Rect::from_min_max(
                Pos2::new(pane.right(), pane.top() + HEADER),
                Pos2::new(outer.right() - 1., pane.bottom()),
            ),
            true,
            &mut offset.y,
            scrollbar::Metrics {
                extent: (pane.height() - HEADER).max(0.),
                total: state.visible_rows() as f32 * ROW,
                unit: 10.,
            },
            id.with("vertical"),
        );
    }
    if horizontal {
        scrollbar::show(
            ui,
            Rect::from_min_max(
                Pos2::new(outer.left() + SELECTOR, pane.bottom()),
                Pos2::new(pane.right(), outer.bottom() - 1.),
            ),
            false,
            &mut offset.x,
            scrollbar::Metrics {
                extent: (pane.width() - SELECTOR).max(0.),
                total: total_width - SELECTOR,
                unit: 10.,
            },
            id.with("horizontal"),
        );
    }
    if offset != output.state.offset {
        output.state.offset = offset;
        output.state.store(ui.ctx(), output.id);
    }
    ui.advance_cursor_after_rect(outer);
    if !state.error.is_empty() {
        ui.colored_label(Color32::from_rgb(180, 70, 40), &state.error);
    }
}
pub(super) fn take_intent(state: &mut State, index: usize) -> Option<crate::panels::Intent> {
    if std::mem::take(&mut state.export_requested) {
        return Some(crate::panels::Intent::ExportData(Some(index)));
    }
    state.pending_write.take().map(|(path, text)| {
        crate::panels::Intent::command(tomato_services::commands::Command::WriteText { path, text })
    })
}

pub(super) fn dialog(ctx: &egui::Context, result: &QueryResult, state: &mut State, id: Id) {
    view::dialog(ctx, result, state, id);
    temporal::show(ctx, result, state, id);
}
