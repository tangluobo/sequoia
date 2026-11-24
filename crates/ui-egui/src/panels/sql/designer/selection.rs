//! One press selects once; a captured gesture then extends rows or cells.
use super::*;

pub(super) struct Grid {
    pub row_height: f32,
    pub body: Rect,
    pub origin: egui::Pos2,
    pub popup: Option<Rect>,
}
#[derive(Default)]
pub(super) struct Outcome {
    pub edit: Option<Cell>,
    pub dragged: bool,
}
impl Grid {
    fn column(&self, point: egui::Pos2, draft: &Draft) -> Option<usize> {
        let mut right = self.origin.x;
        if point.x < right {
            return None;
        }
        draft.widths.iter().position(|width| {
            right += width;
            point.x < right
        })
    }
    fn hit(&self, point: egui::Pos2, draft: &Draft) -> Option<(usize, usize)> {
        if !self.body.contains(point) || point.y < self.origin.y {
            return None;
        }
        let row = ((point.y - self.origin.y) / self.row_height).floor() as usize;
        (row < draft.request.columns.len())
            .then(|| self.column(point, draft).map(|column| (row, column)))
            .flatten()
    }
    fn target(&self, point: egui::Pos2, draft: &Draft) -> Option<Cell> {
        let count = draft.request.columns.len();
        if count == 0 {
            return None;
        }
        let rows = draft.drag.is_some_and(|drag| drag.rows());
        let row = if rows && point.y < self.body.top() {
            0
        } else if rows && point.y >= self.body.bottom() {
            count - 1
        } else {
            ((point.y.clamp(self.body.top(), self.body.bottom() - 0.1) - self.origin.y)
                / self.row_height)
                .floor()
                .max(0.) as usize
        };
        if rows {
            Some(Cell {
                row: row.min(count - 1),
                column: 0,
            })
        } else {
            self.column(point, draft)
                .filter(|column| *column > 0)
                .map(|column| Cell {
                    row: row.min(count - 1),
                    column: column - 1,
                })
        }
    }
}
pub(super) fn pointer(ui: &Ui, draft: &mut Draft, grid: &Grid, writable: bool) -> Outcome {
    let mut result = Outcome::default();
    let (events, down, point, focused) = ui.input(|input| {
        (
            input
                .events
                .iter()
                .filter_map(|event| match *event {
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers,
                    } => Some(Pointer::Button {
                        point: pos,
                        pressed,
                        modifiers,
                    }),
                    egui::Event::PointerMoved(point) => Some(Pointer::Moved(point)),
                    egui::Event::PointerGone | egui::Event::WindowFocused(false) => {
                        Some(Pointer::Lost)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>(),
            input.pointer.primary_down(),
            input.pointer.interact_pos(),
            input.focused,
        )
    });
    if !ui.is_enabled() || !focused {
        draft.drag = None;
        return result;
    }
    // Winit may batch a press, moves and release together. Their individual
    // positions and order matter; interact_pos() is only the final position.
    for event in events {
        match event {
            Pointer::Button {
                point,
                pressed: true,
                modifiers,
            } => {
                draft.drag = None;
                if grid.popup.is_some_and(|rect| rect.contains(point))
                    || !grid.body.contains(point)
                    || ui.ctx().layer_id_at(point) != Some(ui.layer_id())
                {
                    continue;
                }
                if let Some((row, column)) = grid.hit(point, draft) {
                    let cell = Cell {
                        row,
                        column: column.saturating_sub(1),
                    };
                    if column == 0 || draft.edit.as_ref().is_none_or(|edit| edit.cell != cell) {
                        draft.commit_edit(ui.ctx());
                        ui.memory_mut(|memory| {
                            memory.request_focus(ui.id().with("field-grid-focus"))
                        });
                        if column == 2 && writable {
                            draft.select(cell, false, false);
                            result.edit = Some(cell);
                        } else {
                            draft.begin_selection(
                                cell,
                                column == 0,
                                modifiers.ctrl,
                                modifiers.shift,
                            );
                        }
                        ui.ctx().request_repaint();
                    }
                } else {
                    draft.commit_edit(ui.ctx());
                    draft.clear_selection();
                    ui.ctx().request_repaint();
                }
            }
            Pointer::Moved(point) => extend(ui, draft, grid, point, &mut result),
            Pointer::Button {
                point,
                pressed: false,
                ..
            } => {
                extend(ui, draft, grid, point, &mut result);
                draft.drag = None;
            }
            Pointer::Lost => draft.drag = None,
        }
    }
    // Edge scrolling can change the hit row without a new pointer move.
    if down && let Some(point) = point {
        extend(ui, draft, grid, point, &mut result);
    }
    if !down || point.is_none() {
        draft.drag = None;
    }
    result
}
enum Pointer {
    Button {
        point: egui::Pos2,
        pressed: bool,
        modifiers: egui::Modifiers,
    },
    Moved(egui::Pos2),
    Lost,
}
fn extend(ui: &Ui, draft: &mut Draft, grid: &Grid, point: egui::Pos2, result: &mut Outcome) {
    if draft.drag.is_some()
        && !grid.popup.is_some_and(|rect| rect.contains(point))
        && let Some(cell) = grid.target(point, draft)
    {
        let whole_row = draft.drag.is_some_and(|drag| drag.rows());
        if draft
            .focus
            .is_none_or(|focus| focus.row != cell.row || !whole_row && focus.column != cell.column)
        {
            draft.drag_to(cell);
            ui.ctx().request_repaint();
        }
    }
    result.dragged |= draft.drag.is_some_and(|drag| drag.moved());
}
pub(super) fn scroll(
    ui: &Ui,
    draft: &Draft,
    grid: &Grid,
    output: &egui::scroll_area::ScrollAreaOutput<()>,
    axis: usize,
) {
    let Some(drag) = draft.drag else { return };
    if axis == 0 && drag.rows() {
        return;
    }
    let Some(point) = ui.input(|input| input.pointer.interact_pos()) else {
        return;
    };
    if !grid.body.expand(grid.row_height).contains(point) {
        return;
    }
    let delta = if point[axis] < grid.body.min[axis] + 8. {
        -1.
    } else if point[axis] > grid.body.max[axis] - 8. {
        1.
    } else {
        0.
    };
    let mut state = output.state;
    let max = (output.content_size[axis] - output.inner_rect.size()[axis]).max(0.);
    let offset = (state.offset[axis]
        + delta * 360. * ui.input(|input| input.stable_dt.clamp(1. / 120., 0.05)))
    .clamp(0., max);
    if offset != state.offset[axis] {
        state.offset[axis] = offset;
        state.store(ui.ctx(), output.id);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }
}
