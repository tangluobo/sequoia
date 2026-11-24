//! CopyTableDialog geometry measured from its unchanged Java scene at 800×600.
use super::*;
use egui::{Align2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
pub(super) enum Action {
    Close,
    Target(String),
    Next,
    Preview,
    Execute,
}
pub(super) struct Layout {
    pub(super) left: f32,
    pub(super) preview: f32,
    columns_drag: Option<(f32, f32)>,
    logs_drag: Option<(f32, f32)>,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            left: 280.,
            preview: 0.5,
            columns_drag: None,
            logs_drag: None,
        }
    }
}
fn drag_divider(
    ui: &Ui,
    response: &egui::Response,
    value: &mut f32,
    anchor: &mut Option<(f32, f32)>,
    axis: usize,
    unit: f32,
    limits: [f32; 2],
) {
    if response.drag_started() {
        *anchor = ui
            .input(|i| i.pointer.press_origin())
            .or_else(|| response.interact_pointer_pos())
            .map(|point| (*value, point[axis]));
    }
    if response.dragged()
        && let Some((initial, origin)) = *anchor
        && let Some(point) = response.interact_pointer_pos()
    {
        *value = (initial + (point[axis] - origin) / unit).clamp(limits[0], limits[1]);
    }
    if response.drag_stopped() {
        *anchor = None;
    }
}
const BLUE: Color32 = Color32::from_rgb(24, 144, 255);
fn connection_label(c: &Connection) -> String {
    format!("{}  ({}:{})", c.name, c.text("host"), c.number("port"))
}
fn text(ui: &Ui, rect: Rect, value: &str, size: f32, color: Color32) {
    let clip = ui.painter().with_clip_rect(rect);
    clip.text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        value,
        crate::native_dialog::font(ui.ctx(), size),
        color,
    );
}
fn button(ui: &mut Ui, rect: Rect, label: &str, enabled: bool, green: bool) -> bool {
    ui.add_enabled_ui(enabled, |ui| {
        if !green {
            return crate::native_dialog::button(ui, rect, label, false).clicked();
        }
        let response = ui.interact(rect, ui.id().with(label), Sense::click());
        ui.painter().rect_filled(
            rect,
            4.,
            Color32::from_rgb(7, 193, 96).gamma_multiply(if enabled { 1. } else { 0.4 }),
        );
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            crate::native_dialog::font(ui.ctx(), 13.),
            Color32::WHITE,
        );
        response.clicked()
    })
    .inner
}
fn combo<'a>(
    ui: &mut Ui,
    rect: Rect,
    id: &str,
    value: &mut String,
    values: impl IntoIterator<Item = &'a str>,
    enabled: bool,
) -> bool {
    let mut changed = false;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect).id_salt(id), |ui| {
        ui.style_mut().override_font_id = Some(crate::native_dialog::font(ui.ctx(), 12.));
        ui.spacing_mut().interact_size.y = 28.;
        {
            let visuals = &mut ui.visuals_mut().widgets.inactive;
            visuals.bg_fill = Color32::WHITE;
            visuals.weak_bg_fill = Color32::WHITE;
            visuals.bg_stroke = Stroke::new(1., Color32::from_gray(204));
            visuals.fg_stroke.color = Color32::BLACK;
            visuals.corner_radius = egui::CornerRadius::ZERO;
        }
        ui.visuals_mut().widgets.hovered.bg_fill = Color32::WHITE;
        ui.visuals_mut().widgets.hovered.weak_bg_fill = Color32::WHITE;
        ui.visuals_mut().widgets.hovered.fg_stroke.color = Color32::BLACK;
        ui.visuals_mut().widgets.active.fg_stroke.color = Color32::BLACK;
        ui.visuals_mut().widgets.open.fg_stroke.color = Color32::BLACK;
        ui.add_enabled_ui(enabled, |ui| {
            egui::ComboBox::from_id_salt(id)
                .width(rect.width())
                .selected_text(value.as_str())
                .wrap_mode(egui::TextWrapMode::Truncate)
                .icon(move |ui, _, _, _| {
                    let center = egui::pos2(rect.right() - 12., rect.center().y);
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![
                            center + vec2(-4., -2.),
                            center + vec2(4., -2.),
                            center + vec2(0., 3.),
                        ],
                        Color32::from_gray(153),
                        Stroke::NONE,
                    ));
                })
                .show_ui(ui, |ui| {
                    for name in values {
                        if ui.selectable_label(value.as_str() == name, name).clicked() {
                            if value != name {
                                *value = name.to_owned();
                                changed = true;
                            }
                            ui.close();
                        }
                    }
                });
        });
    });
    changed
}
fn info(ui: &Ui, rect: Rect, c: &Connection) {
    ui.painter().rect_filled(rect, 4., Color32::from_gray(250));
    ui.painter().rect_stroke(
        rect,
        4.,
        Stroke::new(1., Color32::from_gray(229)),
        StrokeKind::Inside,
    );
    text(
        ui,
        Rect::from_min_size(rect.min + vec2(16., 16.), vec2(200., 22.)),
        "信息",
        16.,
        BLUE,
    );
    for (i, (label, value)) in [
        ("项目名:", "我的连接".into()),
        (
            "连接类型:",
            match c.kind() {
                "MYSQL" => "MySQL",
                "POSTGRESQL" => "PostgreSQL",
                _ => c.kind(),
            }
            .into(),
        ),
        ("连接名:", c.name.clone()),
        ("主机:", c.text("host").into()),
        ("端口:", c.number("port").to_string()),
        ("服务器版本:", String::new()),
    ]
    .into_iter()
    .enumerate()
    {
        let y = rect.top() + 46. + i as f32 * 23.;
        text(
            ui,
            Rect::from_min_size(pos2(rect.left() + 16., y), vec2(80., 15.)),
            label,
            12.,
            Color32::from_gray(102),
        );
        text(
            ui,
            Rect::from_min_size(pos2(rect.left() + 106., y), vec2(rect.width() - 121., 15.)),
            &value,
            12.,
            Color32::from_gray(51),
        );
    }
}
fn checkbox(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    value: &mut bool,
    enabled: bool,
    icons: &mut crate::icons::Icons,
) -> bool {
    ui.add_enabled_ui(enabled, |ui| {
        let next =
            crate::native_checkbox::checkbox(ui, rect, ui.id().with(label), *value, false, icons);
        let mut clipped = label.to_owned();
        let font = crate::native_dialog::font(ui.ctx(), 12.);
        let width = (rect.width() - 17.).max(0.);
        if ui
            .painter()
            .layout_no_wrap(clipped.clone(), font.clone(), Color32::BLACK)
            .size()
            .x
            > width
        {
            while !clipped.is_empty()
                && ui
                    .painter()
                    .layout_no_wrap(format!("{clipped}..."), font.clone(), Color32::BLACK)
                    .size()
                    .x
                    > width
            {
                clipped.pop();
            }
            clipped.push_str("...");
        }
        text(
            ui,
            Rect::from_min_max(rect.min + vec2(17., 0.), rect.max),
            &clipped,
            12.,
            Color32::from_gray(51),
        );
        if let Some(next) = next {
            *value = next;
            return true;
        }
        false
    })
    .inner
}
fn readonly(ui: &mut Ui, draft: &mut Draft, index: usize, rect: Rect) {
    let id = draft.editors[index].prepare_id(ui.ctx(), ui.id().with(("transfer-text", index)));
    let source = if index == 0 {
        draft.preview.as_str()
    } else {
        draft.log.as_str()
    };
    // SqlEditorPane also reserves its change-marker strip when no markers are present.
    let strip = Rect::from_min_max(pos2(rect.right() - 12., rect.top()), rect.max);
    let body = Rect::from_min_max(rect.min + vec2(40., 0.), pos2(strip.left(), rect.bottom()));
    let gutter = Rect::from_min_max(rect.min, pos2(body.left(), rect.bottom()));
    ui.painter().rect_filled(rect, 0., Color32::WHITE);
    ui.painter().rect_filled(strip, 0., Color32::from_gray(244));
    ui.painter().rect_stroke(
        rect,
        0.,
        Stroke::new(1., Color32::from_gray(230)),
        StrokeKind::Inside,
    );
    ui.painter()
        .rect_filled(gutter, 0., Color32::from_gray(248));
    let gutter_painter = ui.painter().with_clip_rect(gutter.intersect(rect));
    ui.scope_builder(egui::UiBuilder::new().max_rect(body).id_salt(index), |ui| {
        ui.set_clip_rect(body);
        let mut value = source;
        let output = ScrollArea::both()
            .id_salt("transfer-scroll")
            .max_width(body.width())
            .max_height(body.height())
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_sized(
                    body.size(),
                    TextEdit::multiline(&mut value)
                        .id(id)
                        .frame(egui::Frame::NONE)
                        .margin(vec2(0., 0.))
                        .font(egui::FontId::monospace(13.))
                        .desired_width(f32::INFINITY)
                        .layouter(&mut crate::panels::sql_preview_layout),
                );
            });
        draft.scroll_areas[index] = Some(output.id);
        let painter = &gutter_painter;
        let first = (output.state.offset.y / 17.).floor() as usize;
        let count = source.bytes().filter(|b| *b == b'\n').count() + 1;
        let end = ((output.state.offset.y + rect.height()) / 17.).ceil() as usize;
        for line in first..=end.min(count.saturating_sub(1)) {
            painter.text(
                pos2(
                    gutter.left() + 12.,
                    rect.top() + line as f32 * 17. - output.state.offset.y + 8.5,
                ),
                Align2::LEFT_CENTER,
                (line + 1).to_string(),
                egui::FontId::monospace(12.),
                Color32::from_gray(128),
            );
        }
    });
    draft.editors[index].trim(ui.ctx());
}
pub(super) fn render(
    ui: &mut Ui,
    draft: &mut Draft,
    connections: &[Connection],
    icons: &mut crate::icons::Icons,
) -> Option<Action> {
    if draft.options {
        ui.disable();
    }
    ui.style_mut().override_font_id = Some(crate::native_dialog::font(ui.ctx(), 13.));
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    ui.spacing_mut().scroll.bar_width = 13.;
    let bounds = ui.max_rect();
    let w = bounds.width();
    let h = bounds.height();
    let origin = bounds.min;
    let at = |x, y, width, height| Rect::from_min_size(origin + vec2(x, y), vec2(width, height));
    let mut action = None;
    ui.painter().rect_filled(bounds, 0., Color32::WHITE);
    ui.painter().line_segment(
        [origin + vec2(0., 63.), origin + vec2(w, 63.)],
        Stroke::new(1., Color32::from_gray(229)),
    );
    // The arrow and both database logos remain centered as the native window resizes.
    let source_label = connection_label(&draft.request.source);
    let target_label = connection_label(&draft.request.target);
    let widths = [source_label.as_str(), target_label.as_str()].map(|s| {
        ui.painter()
            .layout_no_wrap(
                s.into(),
                crate::native_dialog::font(ui.ctx(), 14.),
                Color32::BLACK,
            )
            .size()
            .x
    });
    let x = (w - widths[0] - widths[1] - 70.) * 0.5;
    for (index, c) in [&draft.request.source, &draft.request.target]
        .into_iter()
        .enumerate()
    {
        let left = if index == 0 { x } else { x + widths[0] + 70. };
        text(
            ui,
            at(left, 15., widths[index], 20.),
            if index == 0 {
                &source_label
            } else {
                &target_label
            },
            14.,
            Color32::BLACK,
        );
        if let Some(texture) = icons.get(ui.ctx(), &crate::icons::for_connection(c)) {
            ui.painter().image(
                texture,
                at(left, 38., 16., 16.),
                Rect::from_min_max(pos2(0., 0.), pos2(1., 1.)),
                Color32::WHITE,
            );
        }
        text(
            ui,
            at(left + 20., 36., widths[index] - 20., 18.),
            c.text("database"),
            12.,
            Color32::from_gray(136),
        );
    }
    text(
        ui,
        at(x + widths[0] + 24., 18., 35., 26.),
        "→",
        20.,
        Color32::from_gray(153),
    );
    let ready = !draft.loading && !draft.running;
    if draft.step == 0 {
        let width = (w - 114.) * 0.5;
        let right = w * 0.5 + 31.;
        text(ui, at(25., 84., width, 24.), "源", 16., BLUE);
        text(ui, at(right, 84., width, 24.), "目标", 16., BLUE);
        text(ui, at(25., 119., width, 20.), "连接:", 13., Color32::BLACK);
        let mut source = source_label.clone();
        combo(
            ui,
            at(25., 148., width, 28.),
            "source-connection",
            &mut source,
            [source_label.as_str()],
            false,
        );
        text(
            ui,
            at(25., 189., width, 20.),
            "数据库:",
            13.,
            Color32::BLACK,
        );
        let mut source_db = draft.request.source.text("database").to_owned();
        combo(
            ui,
            at(25., 218., width, 28.),
            "source-database",
            &mut source_db,
            [draft.request.source.text("database")],
            false,
        );
        button(ui, at(w * 0.5 - 18., 177., 36., 36.), "⇄", false, false);
        for (offset, label, enabled) in [(0., "连接", true), (65., "文件", false)] {
            let radio = at(right + offset, 118., 45., 16.);
            let response = ui
                .add_enabled_ui(enabled, |ui| {
                    ui.interact(
                        radio,
                        ui.id().with(("transfer-destination-type", label)),
                        Sense::click(),
                    )
                })
                .inner;
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::RadioButton, enabled, enabled, label)
            });
            let center = radio.min + vec2(8., 8.);
            ui.painter().circle_filled(
                center,
                7.5,
                Color32::from_gray(if enabled { 244 } else { 248 }),
            );
            ui.painter().circle_stroke(
                center,
                7.5,
                Stroke::new(
                    if enabled { 1.5 } else { 1. },
                    if enabled {
                        Color32::from_rgb(0, 164, 207)
                    } else {
                        Color32::from_gray(226)
                    },
                ),
            );
            if enabled {
                ui.painter()
                    .circle_filled(center, 3.5, Color32::from_gray(65));
            }
            text(
                ui,
                Rect::from_min_max(radio.min + vec2(20., 0.), radio.max),
                label,
                12.,
                Color32::from_gray(if enabled { 51 } else { 176 }),
            );
        }
        let mut target = target_label;
        let items = connections
            .iter()
            .filter(|c| matches!(c.kind(), "MYSQL" | "POSTGRESQL" | "ORACLE"))
            .map(connection_label)
            .collect::<Vec<_>>();
        if combo(
            ui,
            at(right, 146., width, 28.),
            "target-connection",
            &mut target,
            items.iter().map(String::as_str),
            ready,
        ) && let Some(c) = connections.iter().find(|c| connection_label(c) == target)
        {
            action = Some(Action::Target(c.id.clone()));
        }
        text(
            ui,
            at(right, 187., width, 20.),
            "数据库:",
            13.,
            Color32::BLACK,
        );
        let mut database = draft.target_database().to_owned();
        if combo(
            ui,
            at(right, 216., width, 28.),
            "target-database",
            &mut database,
            draft.databases.iter().map(String::as_str),
            ready,
        ) {
            draft.invalidate();
            draft.set_target_database(database);
        }
        info(
            ui,
            at(25., 271., (w - 88.) * 0.5, 192.),
            &draft.request.source,
        );
        info(
            ui,
            at(w * 0.5 + 19., 271., (w - 88.) * 0.5, 192.),
            &draft.request.target,
        );
        ui.painter().rect_filled(
            at(w * 0.5 - 4., 271., 8., 192.),
            0.,
            Color32::from_gray(242),
        );
        ui.painter().line_segment(
            [origin + vec2(w * 0.5, 271.), origin + vec2(w * 0.5, 463.)],
            Stroke::new(1., Color32::from_gray(215)),
        );
    } else {
        let vertical = at(draft.layout.left - 3., 64., 6., h - 121.);
        let divider = ui
            .interact(
                vertical,
                ui.id().with("transfer-columns-divider"),
                Sense::drag(),
            )
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        drag_divider(
            ui,
            &divider,
            &mut draft.layout.left,
            &mut draft.layout.columns_drag,
            0,
            1.,
            [220., w - 480.],
        );
        let split = draft.layout.left;
        ui.painter().line_segment(
            [origin + vec2(split, 63.), origin + vec2(split, h - 57.)],
            Stroke::new(1., Color32::from_gray(229)),
        );
        text(
            ui,
            at(15., 80., split - 155., 22.),
            "选择要同步的表",
            14.,
            BLUE,
        );
        if button(ui, at(split - 143., 79., 60., 26.), "全选", ready, false) {
            for (_, selected) in &mut draft.tables {
                *selected = true;
            }
            action = Some(Action::Preview);
        }
        if button(ui, at(split - 75., 79., 60., 26.), "全不选", ready, false) {
            for (_, selected) in &mut draft.tables {
                *selected = false;
            }
            action = Some(Action::Preview);
        }
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(at(15., 112., split - 30., h - 184.)),
            |ui| {
                ui.spacing_mut().item_spacing.y = 0.;
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show_rows(ui, 19., draft.tables.len(), |ui, range| {
                        for i in range {
                            let (name, selected) = &mut draft.tables[i];
                            let (rect, _) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 19.),
                                Sense::hover(),
                            );
                            if checkbox(ui, rect, name, selected, ready, icons) {
                                action = Some(Action::Preview);
                            }
                        }
                    });
            },
        );
        let right = split + 16.;
        let rw = w - right - 16.;
        text(ui, at(right, 79., rw, 22.), "结构同步 SQL 预览", 14., BLUE);
        let copy = at(right, 106., 80., 24.);
        let copied = button(ui, copy, "复制 SQL", true, true);
        ui.painter().rect_filled(copy, 3., BLUE);
        ui.painter().text(
            copy.center(),
            Align2::CENTER_CENTER,
            "复制 SQL",
            crate::native_dialog::font(ui.ctx(), 13.),
            Color32::WHITE,
        );
        if copied && !draft.preview.is_empty() {
            ui.ctx().copy_text(draft.preview.clone());
        }
        ui.painter().line_segment(
            [
                origin + vec2(right + 98., 107.),
                origin + vec2(right + 98., 130.),
            ],
            Stroke::new(1., Color32::from_gray(210)),
        );
        text(
            ui,
            at(right + 117., 108., 26., 20.),
            "过滤:",
            12.,
            Color32::from_gray(51),
        );
        let mut fx = right + 150.;
        for (flag, label, width) in [
            (&mut draft.request.drop, "删除字段", 56.),
            (&mut draft.request.add, "增加字段", 56.),
            (&mut draft.request.modify, "修改字段类型", 80.),
        ] {
            if checkbox(ui, at(fx, 108., width, 20.), label, flag, ready, icons) {
                action = Some(Action::Preview);
            }
            fx += width + 15.;
        }
        if let Some(plan) = &draft.plan {
            let count: usize = plan
                .tables
                .iter()
                .map(|table| table.sql.len() + usize::from(table.data_sql.is_some()))
                .sum();
            text(
                ui,
                at(w - 102., 108., 94., 20.),
                &format!("{count} 条 SQL，{} 张表", plan.tables.len()),
                12.,
                Color32::from_gray(102),
            );
        }
        let available = h - 210.;
        let preview_h = available * draft.layout.preview;
        readonly(ui, draft, 0, at(right, 137., rw, preview_h));
        let ly = 137. + preview_h + 1.;
        text(ui, at(right, ly, rw, 18.), "执行进度与日志", 13., BLUE);
        draft.control.read(|progress| {
            let fraction = if progress.total == 0 {
                0.
            } else {
                progress.objects as f32 / progress.total as f32
            };
            let bar = at(right, ly + 25., 220., 14.);
            ui.painter().rect_filled(bar, 3., Color32::from_gray(240));
            ui.painter().rect_stroke(
                bar,
                3.,
                Stroke::new(1., Color32::from_gray(190)),
                StrokeKind::Inside,
            );
            ui.painter().line_segment(
                [
                    bar.left_top() + vec2(2., 1.),
                    bar.right_top() - vec2(2., -1.),
                ],
                Stroke::new(1., Color32::WHITE),
            );
            if fraction > 0. {
                ui.painter().rect_filled(
                    Rect::from_min_size(
                        bar.min + vec2(1., 1.),
                        vec2(
                            (bar.width() - 2.) * fraction.clamp(0., 1.),
                            bar.height() - 2.,
                        ),
                    ),
                    2.,
                    BLUE,
                );
            }
            text(
                ui,
                at(right + 230., ly + 20., rw - 230., 22.),
                &if draft.running {
                    format!(
                        "{}/{}，{} 行",
                        progress.objects, progress.total, progress.records
                    )
                } else if draft.completed {
                    "执行完成".into()
                } else {
                    "尚未执行".into()
                },
                12.,
                Color32::from_gray(102),
            );
            draft.log.clear();
            for line in &progress.logs {
                draft.log.push_str(line);
                draft.log.push('\n');
            }
        });
        readonly(
            ui,
            draft,
            1,
            at(right, ly + 45., rw, (h - 72. - ly - 45.).max(0.)),
        );
        let horizontal = at(right, 137. + available * draft.layout.preview - 3., rw, 6.);
        let divider = ui
            .interact(
                horizontal,
                ui.id().with("transfer-logs-divider"),
                Sense::drag(),
            )
            .on_hover_cursor(egui::CursorIcon::ResizeVertical);
        drag_divider(
            ui,
            &divider,
            &mut draft.layout.preview,
            &mut draft.layout.logs_drag,
            1,
            available,
            [0.2, 0.8],
        );
    }
    ui.painter()
        .rect_filled(at(0., h - 57., w, 57.), 0., Color32::from_gray(245));
    ui.painter().line_segment(
        [origin + vec2(0., h - 57.), origin + vec2(w, h - 57.)],
        Stroke::new(1., Color32::from_gray(229)),
    );
    if draft.step == 0 {
        button(ui, at(20., h - 44., 88., 32.), "保存配置文件", false, false);
        button(
            ui,
            at(118., h - 44., 104., 32.),
            "加载配置文件",
            false,
            false,
        );
        if button(ui, at(232., h - 44., 40., 32.), "选项", ready, false) {
            draft.options = true;
            draft.option_values = [
                draft.request.structure,
                draft.request.data,
                draft.request.drop_existing,
            ];
        }
        if button(ui, at(w - 230., h - 44., 100., 32.), "取消", true, false) {
            action = Some(Action::Close);
        }
        if button(ui, at(w - 120., h - 44., 100., 32.), "下一步", ready, true) {
            action = Some(Action::Next);
        }
    } else {
        if button(ui, at(20., h - 44., 100., 32.), "取消", true, false) {
            action = Some(Action::Close);
        }
        if button(ui, at(130., h - 44., 100., 32.), "上一步", ready, false) {
            draft.step = 0;
            draft.invalidate();
        }
        if button(
            ui,
            at(w - 120., h - 44., 100., 32.),
            "执行",
            ready && !draft.completed && draft.plan.is_some(),
            true,
        ) {
            action = Some(Action::Execute);
        }
    }
    if let Some(error) = &draft.error {
        text(
            ui,
            at(25., h - 79., w - 50., 21.),
            error,
            12.,
            Color32::from_rgb(180, 30, 30),
        );
    }
    if ui.input(|i| i.viewport().close_requested()) && !draft.options {
        action = Some(Action::Close);
    }
    ui.allocate_rect(bounds, Sense::hover());
    action
}
pub(super) fn options(ctx: &egui::Context, draft: &mut Draft, active: bool) {
    if !draft.options {
        return;
    }
    let id = egui::ViewportId::from_hash_of(("table-transfer-options", &draft.id));
    ctx.show_viewport_immediate(
        id,
        egui::ViewportBuilder::default()
            .with_title("选项")
            .with_inner_size([398., 242.])
            .with_resizable(false)
            .with_active(active),
        |ui, _| {
            ui.style_mut().override_font_id = Some(crate::native_dialog::font(ui.ctx(), 13.));
            egui::Frame::new()
                .fill(Color32::WHITE)
                .inner_margin(20.)
                .show(ui, |ui| {
                    ui.label(RichText::new("复制选项").size(14.).color(BLUE));
                    ui.add_space(14.);
                    for (flag, label) in draft.option_values.iter_mut().zip([
                        "复制表结构 (CREATE TABLE)",
                        "复制表数据 (INSERT)",
                        "目标已存在时删除 (DROP TABLE IF EXISTS)",
                    ]) {
                        ui.checkbox(flag, label);
                        ui.add_space(10.);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_sized(
                                [80., 30.],
                                egui::Button::new(RichText::new("确定").color(Color32::WHITE))
                                    .fill(Color32::from_rgb(7, 193, 96)),
                            )
                            .clicked()
                        {
                            draft.invalidate();
                            [
                                draft.request.structure,
                                draft.request.data,
                                draft.request.drop_existing,
                            ] = draft.option_values;
                            draft.options = false;
                        }
                        if ui
                            .add_sized([80., 30.], egui::Button::new("取消"))
                            .clicked()
                        {
                            draft.options = false;
                        }
                    });
                });
            if ui.input(|i| i.viewport().close_requested()) {
                draft.options = false;
            }
        },
    );
}
