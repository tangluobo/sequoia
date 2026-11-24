//! Original TableObjectsView geometry and actions. Database I/O stays in the actor.
use crate::{
    icons::Icons,
    tabs::database_objects::window::{self, Action, Draft as State, Mode, Mutation},
};
use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
mod diagram;
mod dialogs;
mod grid;
#[cfg(test)]
mod rename_tests;

pub(super) struct Context<'a> {
    pub id: &'a str,
    pub busy: bool,
    pub error: Option<&'a str>,
}
pub(super) fn render(
    ui: &mut Ui,
    state: &mut State,
    icons: &mut Icons,
    context: Context<'_>,
) -> Option<Action> {
    let modal = state.confirmation.is_some() || state.notice.is_some();
    ui.spacing_mut().item_spacing = vec2(0., 0.);
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    ui.spacing_mut().scroll.bar_width = 13.;
    ui.spacing_mut().scroll.bar_inner_margin = 0.;
    ui.spacing_mut().scroll.bar_outer_margin = 0.;
    let mut action = None;
    let bounds = ui.available_rect_before_wrap();
    let toolbar = Rect::from_min_size(bounds.min, vec2(bounds.width(), 35.));
    let footer = Rect::from_min_max(pos2(bounds.left(), bounds.bottom() - 22.), bounds.max);
    let content = Rect::from_min_max(
        pos2(bounds.left(), toolbar.bottom()),
        pos2(bounds.right(), footer.top()),
    );
    ui.interact(
        content,
        Id::new(("objects-focus", context.id)),
        Sense::focusable_noninteractive(),
    );
    ui.painter().rect_filled(bounds, 0, Color32::WHITE);
    ui.painter()
        .rect_filled(toolbar, 0, Color32::from_gray(248));
    ui.painter().line_segment(
        [toolbar.left_bottom(), toolbar.right_bottom()],
        Stroke::new(1., Color32::from_gray(221)),
    );
    let enabled = !context.busy && !modal;
    if enabled && state.mode != Mode::Er {
        let background = ui.interact(
            content,
            Id::new(("objects-background", context.id)),
            Sense::click(),
        );
        background.context_menu(|ui| {
            if let Some(next) = grid::menu(ui, state, icons) {
                action = Some(next);
            }
        });
    }
    let selected = enabled && !state.selected.is_empty();
    if enabled && let Some((index, due)) = state.rename_due {
        let now = ui.input(|i| i.time);
        if now >= due {
            state.rename_due = None;
            if state.rename.is_none()
                && state.selected.len() == 1
                && state.selected.contains(&index)
            {
                state.start_rename(ui.ctx(), index);
            }
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(due - now));
        }
    }
    let mut x = toolbar.left() + 8.;
    for (label, asset, width, active, command) in [
        ("打开表", "table_open.png", 73., selected, 0),
        ("设计表", "table_edit.png", 73., selected, 1),
        ("新建表", "table_add.png", 73., enabled, 2),
        ("新建查询", "query.png", 85., enabled, 3),
        ("删除表", "table_drop.png", 73., selected, 4),
        ("导入向导", "table_import.png", 85., enabled, 5),
        ("导出向导", "table_export.png", 86., enabled, 6),
        ("刷新", "refresh.png", 62., enabled, 7),
    ] {
        if command == 5 || command == 7 {
            ui.painter().line_segment(
                [
                    pos2(x + 7., toolbar.top() + 6.),
                    pos2(x + 7., toolbar.bottom() - 7.),
                ],
                Stroke::new(1., Color32::from_gray(221)),
            );
            x += 16.;
        }
        let rect = Rect::from_min_size(pos2(x, toolbar.top() + 4.), vec2(width, 26.));
        if button(ui, icons, rect, label, asset, active).clicked() {
            action = match command {
                0 => Some(Action::OpenSelected { structure: false }),
                1 => Some(Action::OpenSelected { structure: true }),
                2 => Some(Action::NewTable),
                3 => Some(Action::NewQuery),
                4 => {
                    state.confirm(Mutation::Drop);
                    None
                }
                5 => Some(Action::Import),
                6 => Some(Action::Export),
                _ => Some(Action::Refresh),
            };
        }
        x += width + 2.;
    }
    let search = Rect::from_min_size(
        pos2((toolbar.right() - 228.).max(x + 2.), toolbar.top() + 4.),
        vec2(220., 26.),
    );
    ui.painter().rect_filled(search, 4, Color32::from_gray(240));
    let glass = search.left_center() + vec2(14., -1.);
    ui.painter()
        .circle_stroke(glass, 3.5, Stroke::new(1.2, Color32::from_gray(150)));
    ui.painter().line_segment(
        [glass + vec2(2.5, 2.5), glass + vec2(6., 6.)],
        Stroke::new(1.2, Color32::from_gray(150)),
    );
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(Rect::from_min_max(
            search.min + vec2(32., 4.),
            search.max - vec2(8., 4.),
        )),
        |ui| {
            let id = state
                .search_editor
                .prepare_id(ui.ctx(), egui::Id::new(("objects-search", context.id)));
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.search)
                    .id(id)
                    .font(crate::native_dialog::font(ui.ctx(), 12.))
                    .hint_text("搜索表名/注释/字段...")
                    .frame(egui::Frame::NONE)
                    .desired_width(180.)
                    .char_limit(4096),
            );
            if response.changed() {
                state.search_due = Some(ui.input(|input| input.time) + 0.3);
            }
            if let Some(text) = egui::widgets::text_edit::TextEditState::load(ui.ctx(), response.id)
            {
                text.trim_undo_memory(2 * 1024 * 1024);
                text.store(ui.ctx(), response.id);
            }
        },
    );
    if let Some(due) = state.search_due {
        let now = ui.input(|input| input.time);
        if now >= due {
            state.filter();
            state.search_due = None;
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(due - now));
        }
    }
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(content)
            .id_salt(("objects-body", context.id)),
        |ui| {
            if !enabled {
                ui.disable();
            }
            ui.set_clip_rect(ui.clip_rect().intersect(content));
            let next = match state.mode {
                Mode::Icon => grid::icons(ui, state, icons, context.id),
                Mode::Detail => grid::details(ui, state, icons, context.id),
                Mode::Er => diagram::render(ui, state, context.id),
            };
            if next.is_some() {
                action = next;
            }
        },
    );
    if context.busy {
        ui.put(
            Rect::from_center_size(content.center(), vec2(40., 40.)),
            egui::Spinner::new().size(40.),
        );
    }
    if let Some(error) = context.error {
        ui.painter().text(
            content.center(),
            Align2::CENTER_CENTER,
            format!("加载失败: {error}"),
            crate::native_dialog::font(ui.ctx(), 12.),
            Color32::from_rgb(180, 30, 30),
        );
    }
    ui.painter().rect_filled(footer, 0, Color32::from_gray(245));
    ui.painter().line_segment(
        [footer.left_top(), footer.right_top()],
        Stroke::new(1., Color32::from_gray(221)),
    );
    ui.painter().text(
        footer.left_center() + vec2(12., 0.),
        Align2::LEFT_CENTER,
        if context.busy {
            "加载中...".into()
        } else {
            state.status()
        },
        crate::native_dialog::font(ui.ctx(), 12.),
        Color32::from_gray(51),
    );
    for (index, (mode, label)) in [
        (Mode::Icon, "图标视图"),
        (Mode::Detail, "详细列表"),
        (Mode::Er, "ER视图"),
    ]
    .into_iter()
    .enumerate()
    {
        let rect = Rect::from_center_size(
            pos2(footer.right() - 50. + index as f32 * 16., footer.center().y),
            vec2(14., 14.),
        );
        let response = ui
            .interact(
                rect,
                Id::new(("objects-mode", context.id, index)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(label);
        mode_icon(
            ui,
            rect.center(),
            mode,
            if state.mode == mode {
                Color32::from_rgb(7, 193, 96)
            } else {
                Color32::from_gray(85)
            },
        );
        if response.clicked() {
            state.rename_due = None;
            state.drag_selection = None;
            state.rename = None;
            state.rename_editor.release(ui.ctx());
            state.mode = mode;
        }
    }
    ui.allocate_rect(bounds, Sense::hover());
    if enabled
        && state.rename.is_none()
        && ui.memory(|m| m.has_focus(Id::new(("objects-focus", context.id))))
    {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::A)) {
            state.select_all();
        }
        if ui.input(|i| {
            i.events
                .iter()
                .any(|event| matches!(event, egui::Event::Copy))
        }) {
            action = Some(Action::Copy);
        }
        if let Some(text) = ui.input(|i| {
            i.events.iter().find_map(|e| {
                if let egui::Event::Paste(text) = e {
                    Some(text.clone())
                } else {
                    None
                }
            })
        }) {
            action = Some(Action::Paste(Some(text)));
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F2))
            && let Some(index) = state.primary
        {
            state.start_rename(ui.ctx(), index);
        }
    }
    dialogs::show(ui.ctx(), state, icons, context.id).or(action)
}

fn button(
    ui: &Ui,
    icons: &mut Icons,
    rect: Rect,
    label: &str,
    asset: &str,
    enabled: bool,
) -> egui::Response {
    let response = ui.interact(
        rect,
        ui.id().with((label, "objects-button")),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, 2, Color32::from_rgb(216, 230, 242));
    }
    let tint = if enabled {
        Color32::WHITE
    } else {
        Color32::from_white_alpha(102)
    };
    if let Some(texture) = icons.get(ui.ctx(), &format!("images/connect/{asset}")) {
        ui.painter().image(
            texture,
            Rect::from_min_size(rect.min + vec2(8., 5.), vec2(16., 16.)),
            Rect::from_min_max(Pos2::ZERO, pos2(1., 1.)),
            tint,
        );
    }
    let color = if enabled {
        Color32::from_gray(51)
    } else {
        Color32::from_gray(170)
    };
    let galley = ui.painter().layout_job(egui::text::LayoutJob {
        text: label.into(),
        sections: vec![egui::text::LayoutSection {
            leading_space: 0.,
            byte_range: 0.into()..label.len().into(),
            format: egui::TextFormat {
                font_id: crate::native_dialog::font(ui.ctx(), 12.),
                color,
                ..Default::default()
            },
        }],
        wrap: egui::text::TextWrapping {
            max_width: rect.width() - 36.,
            max_rows: 1,
            break_anywhere: true,
            ..Default::default()
        },
        ..Default::default()
    });
    ui.painter().galley(
        rect.min + vec2(28., 13. - galley.size().y / 2.),
        galley,
        color,
    );
    response
}
fn bold(ui: &Ui) -> egui::FontId {
    if ui.ctx().fonts(|f| {
        f.definitions()
            .families
            .contains_key(&egui::FontFamily::Name("table-bold".into()))
    }) {
        egui::FontId::new(12., egui::FontFamily::Name("table-bold".into()))
    } else {
        crate::native_dialog::font(ui.ctx(), 12.)
    }
}
fn scrollbars<R>(
    ui: &mut Ui,
    state: &mut State,
    mut output: egui::scroll_area::ScrollAreaOutput<R>,
    bounds: Rect,
) {
    let mut offset = output.state.offset;
    for vertical in [false, true] {
        let axis = usize::from(vertical);
        let extent = output.inner_rect.size()[axis];
        let total = output.content_size[axis];
        if total <= extent + 0.5 {
            continue;
        }
        let rect = if vertical {
            Rect::from_min_max(
                pos2(output.inner_rect.right(), bounds.top()),
                pos2(bounds.right(), output.inner_rect.bottom()),
            )
        } else {
            Rect::from_min_max(
                pos2(bounds.left(), output.inner_rect.bottom()),
                pos2(output.inner_rect.right(), bounds.bottom()),
            )
        };
        crate::native_scrollbar::show(
            ui,
            rect,
            vertical,
            &mut offset[axis],
            crate::native_scrollbar::Metrics {
                extent,
                total,
                unit: if vertical { 24. } else { 32. },
            },
            output.id.with(("objects-native-bar", vertical)),
            &mut state.bars[axis],
        );
    }
    if offset != output.state.offset {
        output.state.offset = offset;
        output.state.store(ui.ctx(), output.id);
        ui.ctx().request_repaint();
    }
}
fn mode_icon(ui: &Ui, center: Pos2, mode: Mode, color: Color32) {
    let origin = center - vec2(5.5, 5.5);
    let painter = ui.painter();
    let stroke = Stroke::new(if mode == Mode::Icon { 2. } else { 1.5 }, color);
    let line = |a: egui::Vec2, b: egui::Vec2| {
        painter.line_segment([origin + a, origin + b], stroke);
    };
    match mode {
        Mode::Icon => {
            for y in [1.2, 5.5, 9.8] {
                line(vec2(0., y), vec2(11., y));
            }
        }
        Mode::Detail => {
            painter.rect_stroke(
                Rect::from_min_size(origin, vec2(11., 11.)),
                0,
                stroke,
                StrokeKind::Middle,
            );
            for value in [3.7, 7.3] {
                line(vec2(0., value), vec2(11., value));
                line(vec2(value, 0.), vec2(value, 11.));
            }
        }
        Mode::Er => {
            for offset in [vec2(0.6, 0.9), vec2(7.3, 7.)] {
                painter.rect_stroke(
                    Rect::from_min_size(origin + offset, vec2(3.1, 3.1)),
                    0,
                    stroke,
                    StrokeKind::Middle,
                );
            }
            line(vec2(3.7, 2.4), vec2(7.4, 2.4));
            line(vec2(2.1, 3.7), vec2(2.1, 6.8));
        }
    }
}
