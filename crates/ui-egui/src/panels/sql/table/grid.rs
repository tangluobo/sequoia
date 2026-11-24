//! The open-table viewport shares the JavaFX fixed header/selector geometry.
use super::*;
// JavaFX adds the header bottom border to its 28px CSS size.
const SELECTOR: f32 = 15.;
const DATA: f32 = SELECTOR + 1.;
const LINE: Color32 = Color32::from_rgb(190, 190, 188);
const BLUE: Color32 = Color32::from_rgb(53, 146, 203);

fn selector_border(painter: &egui::Painter, rect: egui::Rect) {
    // Java's selector owns its left, right and bottom edges. Drawing a full
    // rectangle adds a second line above each row and above the header.
    let stroke = egui::Stroke::new(1., LINE);
    painter.vline(rect.left() + 0.5, rect.y_range(), stroke);
    painter.vline(rect.right() - 0.5, rect.y_range(), stroke);
    painter.hline(rect.x_range(), rect.bottom() - 0.5, stroke);
}

fn select(
    state: &mut Workbench,
    cell: (usize, usize),
    shift: bool,
    command: bool,
    whole_row: bool,
) {
    let whole_row = whole_row || (shift && state.table_view.row_anchor.is_some());
    if (!shift && !command) || shift {
        state.table_view.cells.clear();
        state.selected.clear();
        state.table_view.selector_rows.clear();
    }
    let anchor = if shift {
        if whole_row {
            (
                state
                    .table_view
                    .row_anchor
                    .or_else(|| state.table_view.focus.map(|cell| cell.0))
                    .unwrap_or(cell.0),
                0,
            )
        } else {
            state.table_view.anchor.unwrap_or(cell)
        }
    } else {
        cell
    };
    if !shift {
        state.table_view.anchor = Some(cell);
        state.table_view.row_anchor = whole_row.then_some(cell.0);
    }
    let columns = state
        .page
        .as_ref()
        .map_or(0, |page| page.definition.columns.len());
    let cols = if whole_row {
        0..columns
    } else {
        anchor.1.min(cell.1)..anchor.1.max(cell.1) + 1
    };
    let ordered = projection::rows(state);
    let selected_rows = ordered
        .iter()
        .position(|row| *row == anchor.0)
        .zip(ordered.iter().position(|row| *row == cell.0))
        .map_or_else(
            || vec![cell.0],
            |(a, b)| ordered[a.min(b)..=a.max(b)].to_vec(),
        );
    for row in selected_rows {
        let clear_row = whole_row && command && state.selected.contains(&row);
        if whole_row {
            if clear_row {
                state.table_view.selector_rows.remove(&row);
            } else {
                state.table_view.selector_rows.insert(row);
            }
        }
        for col in cols.clone() {
            if clear_row || (!whole_row && command && state.table_view.cells.contains(&(row, col)))
            {
                state.table_view.cells.remove(&(row, col));
            } else {
                state.table_view.cells.insert((row, col));
            }
        }
        if state.table_view.cells.row_selected(row) || state.table_view.selector_rows.contains(&row)
        {
            state.selected.insert(row);
        } else {
            state.selected.remove(&row);
        }
    }
    state.table_view.row_mode = !state.table_view.selector_rows.is_empty();
    state.table_view.selector_focus = whole_row;
    state.table_view.focus = Some(cell);
}

pub(super) fn grid(ui: &mut Ui, state: &mut Workbench, writable: bool, database: &str) {
    let can_interact = super::editor::before_grid(ui, state, database);
    if ui.input(|input| input.pointer.primary_pressed()) {
        state.table_view.press = None;
    }
    let id = ui.id().with("table-data-grid");
    let can_interact = can_interact
        && super::super::batch_input::handle(
            ui,
            id,
            writable && state.editor.is_none(),
            &mut super::batch::Input { state, database },
        );
    let reset_scroll = state.page.is_some() && std::mem::take(&mut state.table_view.reset_scroll);
    if reset_scroll {
        ui.memory_mut(|memory| memory.request_focus(id));
    }
    if ui.input(|input| input.pointer.secondary_pressed()) {
        state.table_view.paste_available = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(egui::Id::new("table-clipboard-available")))
            .unwrap_or_else(tomato_services::database::transfer::clipboard::text_available);
    }
    if can_interact
        && writable
        && state.editor.is_none()
        && (state.table_view.pending_paste || ui.memory(|memory| memory.has_focus(id)))
    {
        let paste = ui.input(|input| {
            input.events.iter().find_map(|event| {
                if let egui::Event::Paste(text) = event {
                    Some(text.clone())
                } else {
                    None
                }
            })
        });
        if let Some(text) = paste {
            state.table_view.pending_paste = false;
            if let Err(error) = context_actions::paste(state, &text) {
                state.status = error.to_string();
            }
        }
    }
    let mut context_response: Option<egui::Response> = None;
    let mut context_selection = None;
    let mut editor_end = None;
    let mut editor_visible = false;
    let Some(page) = &state.page else {
        ui.allocate_space(egui::vec2(
            ui.available_width(),
            (ui.available_height() - 48.).max(0.),
        ));
        return;
    };
    if state.table_view.widths.len() != page.definition.columns.len() {
        state.table_view.widths = page
            .definition
            .columns
            .iter()
            .map(|column| (column.name.encode_utf16().count() as f32 * 8. + 16.).max(60.))
            .collect();
    }
    let rows = projection::rows(state);
    if !state.table_view.projection.filters.is_empty() || !state.search.is_empty() {
        let visible: std::collections::BTreeSet<_> = rows.iter().copied().collect();
        state.table_view.cells.retain_rows(&visible);
        state.selected.retain(|row| visible.contains(row));
        state
            .table_view
            .selector_rows
            .retain(|row| visible.contains(row));
        state.table_view.row_mode = !state.table_view.selector_rows.is_empty();
        if state
            .table_view
            .focus
            .is_some_and(|cell| !visible.contains(&cell.0))
        {
            state.table_view.focus = None;
            state.table_view.selector_focus = false;
            state.table_view.row_drag = None;
            state.table_view.press = None;
        }
        if state
            .table_view
            .anchor
            .is_some_and(|cell| !visible.contains(&cell.0))
        {
            state.table_view.anchor = state.table_view.focus;
        }
        if state
            .table_view
            .row_anchor
            .is_some_and(|row| !visible.contains(&row))
        {
            state.table_view.row_anchor = None;
        }
    }

    let row_height = crate::table_style::row_height(ui.ctx());
    let header_height = row_height + 1.;
    crate::panels::editor::ensure_bold_cjk(ui.ctx());
    let font = crate::table_style::font(ui.ctx(), false);
    let header_font = crate::table_style::font(ui.ctx(), true);
    let mut selection = None;
    let mut sorting = None;
    let selection_input = ui.input(|input| !input.events.is_empty());
    let mut column_selection = None;
    let mut clear_selection = false;
    let reveal = ui.input(|i| {
        i.events
            .iter()
            .any(|event| matches!(event, egui::Event::Key { pressed: true, .. }))
    });
    let focused = ui.memory(|m| m.has_focus(id));
    if focused
        && state.editor.is_none()
        && let Some((row, col)) = state.table_view.focus
    {
        let visible_columns: Vec<_> = (0..page.definition.columns.len())
            .filter(|column| !state.table_view.projection.hidden.contains(column))
            .collect();
        let col = if state.table_view.selector_focus
            || state.table_view.projection.hidden.contains(&col)
        {
            visible_columns.first().copied().unwrap_or(col)
        } else {
            col
        };
        let previous = visible_columns
            .iter()
            .copied()
            .rev()
            .find(|column| *column < col)
            .unwrap_or(col);
        let next = visible_columns
            .iter()
            .copied()
            .find(|column| *column > col)
            .unwrap_or(col);
        let current = rows.iter().position(|&r| r == row).unwrap_or(0);
        for (key, destination) in [
            (
                egui::Key::ArrowDown,
                (
                    rows.get((current + 1).min(rows.len().saturating_sub(1)))
                        .copied()
                        .unwrap_or(row),
                    col,
                ),
            ),
            (
                egui::Key::ArrowUp,
                (
                    rows.get(current.saturating_sub(1)).copied().unwrap_or(row),
                    col,
                ),
            ),
            (egui::Key::ArrowLeft, (row, previous)),
            (egui::Key::ArrowRight, (row, next)),
            (
                egui::Key::Tab,
                (
                    row,
                    if ui.input(|i| i.modifiers.shift) {
                        previous
                    } else {
                        next
                    },
                ),
            ),
        ] {
            let modifiers = ui.input(|i| i.modifiers);
            if ui.input_mut(|i| i.consume_key(modifiers, key)) {
                selection = Some((
                    destination,
                    modifiers.shift && key != egui::Key::Tab,
                    modifiers.command,
                    false,
                ));
            }
        }
        if ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy))) {
            match transfer::formatted(state, context_menu::Copy::Plain, false) {
                Ok(text) => ui.ctx().copy_text(text),
                Err(error) => state.status = error.to_string(),
            }
        }
    }
    let total_width = DATA
        + state
            .table_view
            .widths
            .iter()
            .enumerate()
            .filter(|(column, _)| !state.table_view.projection.hidden.contains(column))
            .map(|(_, width)| width)
            .sum::<f32>();
    let height = (ui.available_height() - 48.).max(0.);
    let outer =
        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), height));
    // Register the same focus ID that cell/selector clicks request. Otherwise
    // egui removes that unregistered ID at frame end and arrow navigation dies.
    let response = ui.interact(outer, id, egui::Sense::click());
    if response.clicked() && state.editor.is_none() {
        ui.memory_mut(|memory| memory.request_focus(id));
    }
    let total_height = header_height + row_height * rows.len() as f32;
    let mut vertical = total_height > outer.height() - 1.;
    let mut horizontal = false;
    for _ in 0..2 {
        horizontal = total_width > outer.width() - if vertical { 14. } else { 1. };
        vertical = total_height > outer.height() - 1. - if horizontal { 14. } else { 0. };
    }
    let pane = egui::Rect::from_min_max(
        outer.min + egui::vec2(0., 1.),
        outer.max
            - egui::vec2(
                if vertical { 14. } else { 1. },
                if horizontal { 14. } else { 0. },
            ),
    );
    let mut output = ui
        .scope_builder(egui::UiBuilder::new().max_rect(pane), |ui| {
            let mut scroll = ScrollArea::both()
                .id_salt(id)
                .auto_shrink([false, false])
                .max_height(pane.height())
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .scroll_source(egui::scroll_area::ScrollSource {
                    drag: egui::scroll_area::DragScroll::Never,
                    ..Default::default()
                });
            if reset_scroll {
                scroll = scroll.vertical_scroll_offset(0.);
            }
            scroll.show_viewport(ui, |ui, viewport| {
                let origin = ui.cursor().min;
                ui.allocate_space(egui::vec2(
                    total_width.max(viewport.width()),
                    (header_height + row_height * rows.len() as f32).max(viewport.height()),
                ));
                let pane =
                    egui::Rect::from_min_size(origin + viewport.min.to_vec2(), viewport.size())
                        .intersect(ui.clip_rect());
                ui.painter().rect_filled(pane, 0, Color32::WHITE);
                let cells_clip =
                    egui::Rect::from_min_max(pane.min + egui::vec2(DATA, header_height), pane.max);
                let first = (viewport.min.y / row_height).floor() as usize;
                let last = ((viewport.max.y / row_height).ceil() as usize).min(rows.len());
                for (visible, &row_index) in rows
                    .iter()
                    .enumerate()
                    .take(last)
                    .skip(first.min(rows.len()))
                {
                    let inserted = row_index >= page.rows.len();
                    let top = origin.y + header_height + row_height * visible as f32;
                    let mut x = origin.x + DATA;
                    for (column_index, column) in page.definition.columns.iter().enumerate() {
                        if state.table_view.projection.hidden.contains(&column_index) {
                            continue;
                        }
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(x, top),
                            egui::vec2(state.table_view.widths[column_index], row_height),
                        );
                        x += rect.width();
                        if !rect.intersects(cells_clip) {
                            continue;
                        }
                        let selected = state.table_view.cells.contains(&(row_index, column_index));
                        if can_interact
                            && focused
                            && writable
                            && state.editor.is_none()
                            && state.table_view.focus == Some((row_index, column_index))
                        {
                            super::super::batch_input::ime(ui, rect.intersect(cells_clip));
                        }
                        let value = if inserted {
                            state.inserted[row_index - page.rows.len()][column_index].as_ref()
                        } else {
                            Some(
                                &state.edits.get(&row_index).unwrap_or(&page.rows[row_index])
                                    [column_index],
                            )
                        };
                        if let Some(editor) = &mut state.editor
                            && editor.row == row_index
                            && editor.column == column_index
                        {
                            editor_visible = true;
                            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                ui.set_clip_rect(ui.clip_rect().intersect(cells_clip));
                                editor_end = super::editor::cell(
                                    ui,
                                    editor,
                                    value,
                                    super::editor::kind(column, database),
                                    rect,
                                    font.clone(),
                                    id,
                                );
                            });
                            continue;
                        }
                        let dirty = !inserted
                            && state.edits.get(&row_index).is_some_and(|values| {
                                values[column_index] != page.rows[row_index][column_index]
                            });
                        let painter = ui.painter().with_clip_rect(rect.intersect(cells_clip));
                        painter.rect_filled(
                            rect,
                            0,
                            if selected {
                                BLUE
                            } else if state.deleted.contains(&row_index) {
                                Color32::from_rgb(255, 226, 226)
                            } else if inserted {
                                Color32::from_rgb(255, 255, 240)
                            } else if dirty {
                                Color32::from_rgb(232, 244, 253)
                            } else {
                                Color32::WHITE
                            },
                        );
                        painter.text(
                            rect.left_center() + egui::vec2(6., 0.),
                            egui::Align2::LEFT_CENTER,
                            state
                                .batch
                                .as_ref()
                                .and_then(|batch| batch.preview(row_index, column_index))
                                .map_or_else(
                                    || std::borrow::Cow::Owned(preview(value)),
                                    std::borrow::Cow::Borrowed,
                                ),
                            font.clone(),
                            if selected {
                                Color32::WHITE
                            } else if value.is_none() || value == Some(&Cell::Null) {
                                Color32::from_gray(153)
                            } else {
                                Color32::from_gray(51)
                            },
                        );
                        painter.vline(
                            rect.right() - 0.5,
                            rect.y_range(),
                            egui::Stroke::new(1., Color32::from_gray(224)),
                        );
                        painter.hline(
                            rect.x_range(),
                            rect.bottom() - 0.5,
                            egui::Stroke::new(1., Color32::from_gray(224)),
                        );
                        let response = ui.interact(
                            rect.intersect(cells_clip),
                            id.with(("cell", row_index, column_index)),
                            egui::Sense::click_and_drag(),
                        );
                        if response.clicked() && can_interact && state.editor.is_none() {
                            // Release restores keyboard focus without applying selection
                            // twice (in particular for Ctrl-click toggles).
                            ui.memory_mut(|memory| memory.request_focus(id));
                        }
                        let pressed = response.contains_pointer()
                            && ui.input(|input| input.pointer.primary_pressed());
                        if pressed && can_interact {
                            state.table_view.press = Some(crate::tabs::workbench::CellPress {
                                cell: (row_index, column_index),
                                selected: state.table_view.focus == Some((row_index, column_index))
                                    && state.table_view.cells.contains(&(row_index, column_index))
                                    && state.table_view.cells.iter().take(2).count() == 1,
                            });
                        }
                        let dragged = state.table_view.press.is_some()
                            && response.contains_pointer()
                            && state.table_view.focus != Some((row_index, column_index))
                            && ui.input(|input| {
                                input.pointer.primary_down()
                                    && input.pointer.delta() != egui::Vec2::ZERO
                                    && !input.pointer.primary_pressed()
                            });
                        let synthetic = response.clicked()
                            && !ui.input(|input| input.pointer.primary_released());
                        if can_interact
                            && state.editor.is_none()
                            && (pressed || dragged || synthetic)
                        {
                            let (shift, command) =
                                ui.input(|i| (i.modifiers.shift || dragged, i.modifiers.command));
                            selection = Some(((row_index, column_index), shift, command, false));
                            ui.memory_mut(|m| m.request_focus(id));
                        }
                        if (((response.double_clicked()
                            || (response.clicked()
                                && state.table_view.press.is_some_and(|press| {
                                    press.selected && press.cell == (row_index, column_index)
                                })))
                            && !ui.input(|input| input.modifiers.shift || input.modifiers.command))
                            || (focused
                                && state.table_view.focus == Some((row_index, column_index))
                                && ui.input(|i| {
                                    i.key_pressed(egui::Key::F2) || i.key_pressed(egui::Key::Enter)
                                })))
                            && can_interact
                            && state.editor.is_none()
                            && writable
                            && !column.generated
                            && !state.deleted.contains(&row_index)
                        {
                            state.editor = Some(CellEditor::new(
                                row_index,
                                column_index,
                                value,
                                column.binary,
                            ));
                            for key in [egui::Key::F2, egui::Key::Enter] {
                                ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key));
                            }
                            editor_visible = true;
                            if let Some(editor) = &mut state.editor {
                                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                    ui.set_clip_rect(ui.clip_rect().intersect(cells_clip));
                                    editor_end = super::editor::cell(
                                        ui,
                                        editor,
                                        value,
                                        super::editor::kind(column, database),
                                        rect,
                                        font.clone(),
                                        id,
                                    );
                                });
                            }
                        }
                        if can_interact
                            && response.contains_pointer()
                            && ui.input(|input| input.pointer.secondary_pressed())
                        {
                            if !state.table_view.cells.contains(&(row_index, column_index)) {
                                context_selection = Some(((row_index, column_index), false));
                            }
                            ui.memory_mut(|memory| memory.request_focus(id));
                        }
                        if response.secondary_clicked() || response.context_menu_opened() {
                            context_response = Some(response);
                        }
                    }
                    let selector = egui::Rect::from_min_size(
                        egui::pos2(pane.left(), top),
                        egui::vec2(SELECTOR, row_height),
                    );
                    let painter = ui.painter().with_clip_rect(egui::Rect::from_min_max(
                        pane.min + egui::vec2(0., header_height),
                        pane.max,
                    ));
                    painter.rect_filled(selector, 0, Color32::WHITE);
                    selector_border(&painter, selector);
                    if state.selected.contains(&row_index) {
                        let center = selector.center();
                        painter.add(egui::Shape::convex_polygon(
                            vec![
                                center + egui::vec2(-2.5, -5.),
                                center + egui::vec2(2.5, 0.),
                                center + egui::vec2(-2.5, 5.),
                            ],
                            Color32::BLACK,
                            egui::Stroke::NONE,
                        ));
                    }
                    let response = ui.interact(
                        selector.intersect(egui::Rect::from_min_max(
                            pane.min + egui::vec2(0., header_height),
                            pane.max,
                        )),
                        id.with(("row-selector", row_index)),
                        egui::Sense::click_and_drag(),
                    );
                    if can_interact && (response.clicked() || response.drag_stopped()) {
                        // The selector's release can take focus from the grid. Keep
                        // keyboard commands on the grid without selecting twice.
                        ui.memory_mut(|memory| memory.request_focus(id));
                    }
                    if can_interact
                        && response.contains_pointer()
                        && ui.input(|input| input.pointer.primary_pressed())
                    {
                        state.table_view.row_drag = Some(row_index);
                        selection = Some((
                            (row_index, 0),
                            ui.input(|input| input.modifiers.shift),
                            ui.input(|input| input.modifiers.command),
                            true,
                        ));
                        ui.memory_mut(|memory| memory.request_focus(id));
                    } else if can_interact
                        && response.contains_pointer()
                        && state.table_view.row_drag.is_some()
                        && ui.input(|input| {
                            input.pointer.primary_down()
                                && input.pointer.delta() != egui::Vec2::ZERO
                        })
                    {
                        selection = Some(((row_index, 0), true, false, true));
                    }
                    if can_interact
                        && response.contains_pointer()
                        && ui.input(|input| input.pointer.secondary_pressed())
                    {
                        if !state.selected.contains(&row_index) {
                            context_selection = Some(((row_index, 0), true));
                        }
                        ui.memory_mut(|memory| memory.request_focus(id));
                    }
                    if response.secondary_clicked() || response.context_menu_opened() {
                        context_response = Some(response);
                    }
                }
                let header =
                    egui::Rect::from_min_size(pane.min, egui::vec2(pane.width(), header_height));
                ui.painter().rect_filled(header, 0, Color32::WHITE);
                let mut x = origin.x + DATA;
                for (index, column) in page.definition.columns.iter().enumerate() {
                    if state.table_view.projection.hidden.contains(&index) {
                        continue;
                    }
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(x, pane.top()),
                        egui::vec2(state.table_view.widths[index], header_height),
                    );
                    x += rect.width();
                    let clip = rect.intersect(egui::Rect::from_min_max(
                        pane.min + egui::vec2(DATA, 0.),
                        header.max,
                    ));
                    if !clip.is_positive() {
                        continue;
                    }
                    let painter = ui.painter().with_clip_rect(clip);
                    painter.rect_filled(rect, 0, Color32::from_gray(245));
                    painter.vline(
                        rect.right() - 0.5,
                        rect.y_range(),
                        egui::Stroke::new(1., Color32::from_rgb(218, 218, 216)),
                    );
                    painter.hline(
                        rect.x_range(),
                        rect.bottom() - 0.5,
                        egui::Stroke::new(1., Color32::from_gray(204)),
                    );
                    let mark = state
                        .query
                        .sorts
                        .iter()
                        .find(|sort| sort.column == column.name)
                        .filter(|sort| sort.column == column.name)
                        .map_or("", |sort| if sort.descending { " ▼" } else { " ▲" });
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("{}{mark}", column.name),
                        header_font.clone(),
                        Color32::BLACK,
                    );
                    let edge = egui::Rect::from_center_size(
                        rect.right_center(),
                        egui::vec2(6., header_height),
                    )
                    .intersect(header);
                    let resize = ui
                        .interact(edge, id.with(("resize", index)), egui::Sense::drag())
                        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                    if resize.dragged() {
                        state.table_view.widths[index] = (state.table_view.widths[index]
                            + ui.input(|i| i.pointer.delta().x))
                        .clamp(24., 2000.);
                    }
                    let response = ui.interact(
                        clip.shrink2(egui::vec2(4., 0.)),
                        id.with(("column", index)),
                        egui::Sense::click(),
                    );
                    if response.clicked() {
                        column_selection = Some(index);
                        ui.memory_mut(|m| m.request_focus(id));
                    }
                    response.context_menu(|ui| {
                        for (label, descending) in [("正序排列", false), ("倒序排列", true)]
                        {
                            if ui.add(crate::compact_menu::Item::new(label)).clicked() {
                                sorting = Some(Some(Sort {
                                    column: column.name.clone(),
                                    descending,
                                }));
                                ui.close();
                            }
                        }
                        crate::compact_menu::separator(ui);
                        if ui.add(crate::compact_menu::Item::new("取消排序")).clicked() {
                            sorting = Some(None);
                            ui.close();
                        }
                    });
                }
                let selector =
                    egui::Rect::from_min_size(pane.min, egui::vec2(SELECTOR, header_height));
                ui.painter()
                    .rect_filled(selector, 0, Color32::from_gray(245));
                selector_border(ui.painter(), selector);
                if rows.is_empty() {
                    ui.painter().text(
                        pane.center(),
                        egui::Align2::CENTER_CENTER,
                        "当前表没有数据",
                        egui::FontId::proportional(13.),
                        Color32::from_gray(119),
                    );
                }
                if reveal
                    && focused
                    && let Some((cell, _, _, _)) = selection
                    && let Some(row) = rows.iter().position(|index| *index == cell.0)
                {
                    let x = origin.x
                        + SELECTOR
                        + state.table_view.widths[..cell.1]
                            .iter()
                            .enumerate()
                            .filter(|(column, _)| {
                                !state.table_view.projection.hidden.contains(column)
                            })
                            .map(|(_, width)| width)
                            .sum::<f32>();
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(x, origin.y + header_height + row_height * row as f32),
                        egui::vec2(state.table_view.widths[cell.1], row_height),
                    );
                    ui.scroll_to_rect(rect.expand2(egui::vec2(SELECTOR, header_height)), None);
                }
                let filler = egui::Rect::from_min_max(
                    egui::pos2(
                        (origin.x + total_width).max(pane.left() + SELECTOR),
                        pane.top(),
                    ),
                    pane.max,
                );
                for (name, rect) in [
                    (
                        "filler",
                        filler.intersect(egui::Rect::from_min_max(
                            pane.min + egui::vec2(0., header_height),
                            pane.max,
                        )),
                    ),
                    (
                        "empty-rows",
                        egui::Rect::from_min_max(
                            egui::pos2(
                                pane.left() + SELECTOR,
                                (origin.y + header_height + row_height * rows.len() as f32)
                                    .max(pane.top() + header_height),
                            ),
                            pane.max,
                        ),
                    ),
                ] {
                    if !rect.is_positive() {
                        continue;
                    }
                    let response = ui.interact(rect, id.with(name), egui::Sense::click());
                    if response.clicked() {
                        clear_selection = true;
                    }
                    if response.secondary_clicked() || response.context_menu_opened() {
                        context_response = Some(response);
                    }
                }
            })
        })
        .inner;
    let mut offset = output.state.offset;
    for (visible, vertical, rect, extent, total) in [
        (
            vertical,
            true,
            egui::Rect::from_min_max(
                egui::pos2(pane.right(), pane.top() + header_height),
                egui::pos2(outer.right() - 1., pane.bottom()),
            ),
            (pane.height() - header_height).max(0.),
            rows.len() as f32 * row_height,
        ),
        (
            horizontal,
            false,
            egui::Rect::from_min_max(
                egui::pos2(outer.left() + SELECTOR, pane.bottom()),
                egui::pos2(pane.right(), outer.bottom() - 1.),
            ),
            (pane.width() - SELECTOR).max(0.),
            total_width - SELECTOR,
        ),
    ] {
        if !visible {
            continue;
        }
        let scroll_id = id.with(if vertical { "vertical" } else { "horizontal" });
        let mut interaction = ui
            .ctx()
            .data(|data| data.get_temp::<crate::native_scrollbar::State>(scroll_id))
            .unwrap_or_default();
        crate::native_scrollbar::show(
            ui,
            rect,
            vertical,
            if vertical {
                &mut offset.y
            } else {
                &mut offset.x
            },
            crate::native_scrollbar::Metrics {
                extent,
                total,
                unit: 10.,
            },
            scroll_id,
            &mut interaction,
        );
        ui.ctx()
            .data_mut(|data| data.insert_temp(scroll_id, interaction));
    }
    if offset != output.state.offset {
        output.state.offset = offset;
        output.state.store(ui.ctx(), output.id);
    }
    ui.advance_cursor_after_rect(outer);
    if clear_selection {
        state.table_view.cells.clear();
        state.selected.clear();
        state.table_view.focus = None;
        state.table_view.anchor = None;
        state.table_view.row_mode = false;
        state.table_view.selector_focus = false;
        state.table_view.selector_rows.clear();
        state.table_view.row_anchor = None;
    }
    if let Some(column) = column_selection {
        state.table_view.cells.clear();
        state.selected.clear();
        for &row in &rows {
            state.table_view.cells.insert((row, column));
            state.selected.insert(row);
        }
        state.table_view.focus = rows.first().map(|&row| (row, column));
        state.table_view.anchor = state.table_view.focus;
        state.table_view.row_mode = false;
        state.table_view.selector_focus = false;
        state.table_view.selector_rows.clear();
        state.table_view.row_anchor = None;
    }
    if let Some((cell, shift, command, whole_row)) = selection {
        select(state, cell, shift, command, whole_row);
        if selection_input {
            ui.ctx().request_repaint();
        }
    }
    if let Some((cell, whole_row)) = context_selection {
        select(state, cell, false, false, whole_row);
    }
    if ui.input(|input| input.pointer.primary_released()) {
        state.table_view.row_drag = None;
        state.table_view.press = None;
    }
    if let Some(end) = editor_end {
        if super::editor::finish(ui.ctx(), state, database, end) {
            ui.memory_mut(|memory| memory.request_focus(id));
        }
    } else if !editor_visible && state.editor.is_some() {
        super::editor::finish(ui.ctx(), state, database, super::editor::End::Commit);
    }
    let mut menu_action = None;
    if context_response.is_none()
        && (response.context_menu_opened()
            || (response.secondary_clicked()
                && ui.input(|input| {
                    input
                        .pointer
                        .interact_pos()
                        .is_some_and(|point| point.y >= pane.top() + header_height)
                })))
    {
        context_response = Some(response);
    }
    if let Some(response) = context_response {
        let menu_font = crate::native_dialog::font(ui.ctx(), 10.);
        let shown = egui::Popup::context_menu(&response)
            .width(114.)
            .frame(context_menu::frame())
            .style(move |style: &mut egui::Style| {
                context_menu::style(style);
                style.override_font_id = Some(menu_font.clone());
            })
            .close_behavior(if response.secondary_clicked() {
                egui::PopupCloseBehavior::IgnoreClicks
            } else {
                egui::PopupCloseBehavior::CloseOnClickOutside
            })
            .show(|ui| {
                if let Some(action) = context_menu::show(ui, state, writable && can_interact) {
                    menu_action = Some(action);
                }
            });
        if let Some(shown) = shown
            && ui
                .ctx()
                .data(|data| data.get_temp::<bool>(egui::Id::new("table-menu-native-input")))
                .unwrap_or(false)
        {
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("table-context-menu-geometry"),
                    shown.response.rect,
                )
            });
        }
    }
    if let Some(action) = menu_action {
        context_actions::apply(ui.ctx(), state, action, id, database, writable);
    }
    if let Some(sort) = sorting
        && let Some(mut request) = state.request()
    {
        request.sort = sort;
        request.query.sorts = request.sort.take().into_iter().collect();
        request.offset = 0;
        state.pending_page = Some(request);
    }
}
fn preview(value: Option<&Cell>) -> String {
    match value {
        Some(Cell::Text(text)) => text.chars().take(160).collect(),
        Some(Cell::Binary(bytes)) => bytes.iter().take(64).map(|b| format!("{b:02x}")).collect(),
        Some(value) => value.display(),
        None => "DEFAULT".into(),
    }
}

#[cfg(test)]
#[path = "grid_tests.rs"]
mod tests;
