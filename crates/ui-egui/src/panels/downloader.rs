use super::*;
mod dialog;
use crate::tabs::downloader::Kind;
use tomato_services::downloader::{Action, Manager};
pub(super) fn render(
    ui: &mut Ui,
    tool: &mut ToolState,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    let state = tool.downloader.get_or_insert_with(|| {
        Box::new(crate::tabs::downloader::State::new(
            tomato_core::Store::default_path().with_file_name("downloads.json"),
        ))
    });
    if state.show_header {
        super::tools::header(
            ui,
            icons,
            "downloader",
            "下载器",
            "HTTP 多线程 · BT 磁力/种子 · ED2K，全部由内置引擎下载",
        );
    }
    if state.manager.is_none() {
        let ctx = ui.ctx().clone();
        state.manager = Some(std::sync::Arc::new(Manager::open(
            state.path.clone(),
            move || ctx.request_repaint(),
        )));
    }
    let manager = state.manager.as_ref().unwrap();
    let revision = manager.revision();
    if state.revision != revision {
        let snapshot = manager.snapshot();
        if revision > 0 {
            state.defaults = snapshot.settings;
            state.loaded_settings = true;
        }
        state.rows = snapshot.rows;
        if snapshot.error != state.last_error {
            state.last_error = snapshot.error.clone();
            if snapshot.error.is_some() {
                state.error = snapshot.error;
                state.error_focus = true;
            }
        }
        state.revision = revision;
        state
            .rates
            .retain(|id, _| state.rows.iter().any(|r| &r.id == id));
        if state
            .selected
            .as_ref()
            .is_some_and(|id| !state.rows.iter().any(|r| &r.id == id))
        {
            state.selected = None;
        }
        for row in &state.rows {
            let now = std::time::Instant::now();
            let sample =
                state
                    .rates
                    .entry(row.id.clone())
                    .or_insert((row.downloaded_bytes, now, 0.));
            if now.duration_since(sample.1).as_secs_f64() >= 0.2 {
                *sample = (
                    row.downloaded_bytes,
                    now,
                    row.downloaded_bytes.saturating_sub(sample.0) as f64
                        / now.duration_since(sample.1).as_secs_f64(),
                );
            }
        }
    }
    ui.add_enabled_ui(
        state.loaded_settings && state.dialog.is_none() && state.error.is_none(),
        |ui| {
            egui::Frame::new().inner_margin(14).show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0., 0.);
                let (toolbar, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 28.),
                    egui::Sense::hover(),
                );
                let mut x = toolbar.left();
                for (i, (label, width, enabled)) in [
                    ("添加下载", 84., true),
                    ("设置", 41., true),
                    ("移除记录", 65., state.selected.is_some()),
                    (
                        "清空已结束",
                        77.,
                        state.rows.iter().any(|r| {
                            matches!(r.state.as_str(), "COMPLETED" | "FAILED" | "CANCELLED")
                        }),
                    ),
                ]
                .into_iter()
                .enumerate()
                {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(x, toolbar.top()),
                        egui::vec2(width, 28.),
                    );
                    if ui
                        .add_enabled_ui(enabled, |ui| button(ui, rect, label, i == 0))
                        .inner
                        .clicked()
                    {
                        match i {
                            0 => state.open(Kind::Add),
                            1 => state.open(Kind::Settings),
                            2 => {
                                if let Some(id) = state.selected.take() {
                                    state.send(Action::Remove(id));
                                }
                            }
                            _ => state.send(Action::ClearCompleted),
                        }
                    }
                    x += width + 8.;
                }
                let active = state
                    .rows
                    .iter()
                    .filter(|r| r.state == "DOWNLOADING")
                    .count();
                let done = state.rows.iter().filter(|r| r.state == "COMPLETED").count();
                ui.painter().text(
                    toolbar.right_center(),
                    egui::Align2::RIGHT_CENTER,
                    if !state.loaded_settings {
                        "正在加载下载记录…".into()
                    } else if state.rows.is_empty() {
                        "暂无下载任务".into()
                    } else {
                        format!(
                            "共 {} 项 · 下载中 {active} · 已完成 {done}",
                            state.rows.len()
                        )
                    },
                    egui::FontId::proportional(11.),
                    Color32::from_gray(102),
                );
                ui.add_space(12.);
                let table = ui.available_rect_before_wrap();
                let total: f32 = state.columns.iter().sum();
                let available = table.width().max(400.);
                let widths = state.columns.map(|w| w * available / total);
                let (header, _) =
                    ui.allocate_exact_size(egui::vec2(available, 26.), egui::Sense::hover());
                ui.painter().rect_filled(header, 0, Color32::from_gray(244));
                let mut x = header.left();
                for (i, width) in widths.into_iter().enumerate() {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(x, header.top()),
                        egui::vec2(width, 26.),
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        [
                            "文件名",
                            "大小",
                            "进度",
                            "已下载",
                            "速度",
                            "线程",
                            "状态",
                            "操作",
                        ][i],
                        egui::FontId::new(12., egui::FontFamily::Name("md-bold".into())),
                        Color32::BLACK,
                    );
                    ui.painter().vline(
                        rect.right(),
                        rect.y_range(),
                        egui::Stroke::new(1., Color32::from_gray(218)),
                    );
                    if i < 7 {
                        let edge =
                            egui::Rect::from_center_size(rect.right_center(), egui::vec2(5., 26.));
                        let response = ui
                            .interact(
                                edge,
                                ui.id().with(("download-column", i)),
                                egui::Sense::drag(),
                            )
                            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                        if response.dragged() {
                            let delta =
                                ui.input(|input| input.pointer.delta().x) * total / available;
                            let delta =
                                delta.clamp(35. - state.columns[i], state.columns[i + 1] - 35.);
                            state.columns[i] += delta;
                            state.columns[i + 1] -= delta;
                        }
                    }
                    x += width;
                }
                ui.painter().hline(
                    header.x_range(),
                    header.bottom(),
                    egui::Stroke::new(1., Color32::from_gray(204)),
                );
                if state.rows.is_empty() {
                    let empty = ui.available_rect_before_wrap();
                    ui.painter().text(
                        empty.center(),
                        egui::Align2::CENTER_CENTER,
                        "暂无下载任务，请点击“添加下载”创建任务",
                        egui::FontId::proportional(12.),
                        Color32::BLACK,
                    );
                } else {
                    let mut action = None;
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show_rows(ui, 42., state.rows.len(), |ui, visible| {
                            let row_width = ui.available_width();
                            let widths = state.columns.map(|w| w * row_width / total);
                            for row in &state.rows[visible] {
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(row_width, 42.),
                                    egui::Sense::click(),
                                );
                                let selected = state.selected.as_deref() == Some(&row.id);
                                if response.clicked() {
                                    state.selected = Some(row.id.clone());
                                }
                                if response.double_clicked()
                                    && row.state == "COMPLETED"
                                    && let Err(error) = open::that(&row.directory)
                                {
                                    state.error = Some(error.to_string());
                                    state.error_focus = true;
                                }
                                let color = if selected {
                                    Color32::WHITE
                                } else {
                                    Color32::BLACK
                                };
                                ui.painter().rect_filled(
                                    rect,
                                    0,
                                    if selected {
                                        Color32::from_rgb(53, 146, 203)
                                    } else {
                                        Color32::WHITE
                                    },
                                );
                                let mut x = rect.left();
                                for (i, width) in widths.into_iter().enumerate() {
                                    let cell = egui::Rect::from_min_size(
                                        egui::pos2(x, rect.top()),
                                        egui::vec2(width, 42.),
                                    );
                                    let painter = ui
                                        .painter()
                                        .with_clip_rect(cell.shrink2(egui::vec2(6., 0.)));
                                    match i {
                                        2 => {
                                            let fraction = if row.state == "COMPLETED" {
                                                1.
                                            } else if row.total_bytes > 0 {
                                                (row.downloaded_bytes as f32
                                                    / row.total_bytes as f32)
                                                    .clamp(0., 1.)
                                            } else {
                                                0.
                                            };
                                            let bar = egui::Rect::from_min_size(
                                                cell.min + egui::vec2(6., 7.),
                                                egui::vec2((width - 12.).max(0.), 10.),
                                            );
                                            painter.rect_filled(bar, 3, Color32::from_gray(225));
                                            painter.rect_filled(
                                                egui::Rect::from_min_size(
                                                    bar.min,
                                                    egui::vec2(
                                                        bar.width() * fraction,
                                                        bar.height(),
                                                    ),
                                                ),
                                                3,
                                                Color32::from_rgb(25, 118, 210),
                                            );
                                            painter.text(
                                                egui::pos2(cell.left() + 6., cell.top() + 23.),
                                                egui::Align2::LEFT_TOP,
                                                format!("{:.1}%", fraction * 100.),
                                                egui::FontId::proportional(11.),
                                                color,
                                            );
                                        }
                                        7 => {
                                            let caption = match row.state.as_str() {
                                                "DOWNLOADING" | "WAITING" => "取消",
                                                "CANCELLING" => "取消中",
                                                "COMPLETED" => "打开",
                                                _ => "重试",
                                            };
                                            let bounds = egui::Rect::from_center_size(
                                                cell.center(),
                                                egui::vec2(46., 23.),
                                            );
                                            if ui
                                                .add_enabled_ui(row.state != "CANCELLING", |ui| {
                                                    button(ui, bounds, caption, false)
                                                })
                                                .inner
                                                .clicked()
                                            {
                                                match row.state.as_str() {
                                                    "DOWNLOADING" | "WAITING" => {
                                                        action =
                                                            Some(Action::Cancel(row.id.clone()))
                                                    }
                                                    "COMPLETED" => {
                                                        if let Err(error) =
                                                            open::that(&row.directory)
                                                        {
                                                            state.error = Some(error.to_string());
                                                            state.error_focus = true;
                                                        }
                                                    }
                                                    _ => {
                                                        action = Some(Action::Start(row.id.clone()))
                                                    }
                                                }
                                            }
                                        }
                                        _ => {
                                            let text = match i {
                                                0 => row.file_name.clone(),
                                                1 => {
                                                    if row.total_bytes < 0 {
                                                        "未知".into()
                                                    } else {
                                                        bytes(row.total_bytes as u64)
                                                    }
                                                }
                                                3 => bytes(row.downloaded_bytes),
                                                4 => {
                                                    if row.state == "DOWNLOADING" {
                                                        format!(
                                                            "{}/s",
                                                            bytes(
                                                                state
                                                                    .rates
                                                                    .get(&row.id)
                                                                    .map_or(0, |r| r.2 as u64)
                                                            )
                                                        )
                                                    } else {
                                                        "—".into()
                                                    }
                                                }
                                                5 => format!(
                                                    "{} · {}",
                                                    row.protocol, row.actual_threads
                                                ),
                                                _ => row.status.clone(),
                                            };
                                            painter.text(
                                                cell.left_center() + egui::vec2(6., 0.),
                                                egui::Align2::LEFT_CENTER,
                                                &text,
                                                egui::FontId::proportional(12.),
                                                color,
                                            );
                                            if response.hovered()
                                                && ui.input(|input| {
                                                    input
                                                        .pointer
                                                        .hover_pos()
                                                        .is_some_and(|p| cell.contains(p))
                                                })
                                            {
                                                response.clone().on_hover_text(text);
                                            }
                                        }
                                    }
                                    ui.painter().vline(
                                        cell.right(),
                                        cell.y_range(),
                                        egui::Stroke::new(1., Color32::from_gray(224)),
                                    );
                                    x += width;
                                }
                                ui.painter().hline(
                                    rect.x_range(),
                                    rect.bottom(),
                                    egui::Stroke::new(1., Color32::from_gray(224)),
                                );
                            }
                        });
                    if let Some(action) = action {
                        state.send(action);
                    }
                }
            });
        },
    );
    dialog::show(ui.ctx(), state);
    if let Some(error) = state.error.clone() {
        use crate::native_dialog::message::{self, Button, Kind as AlertKind, Message, Role};
        let message = Message {
            id: egui::ViewportId::from_hash_of("download-error"),
            owner: ui.input(|input| input.viewport().inner_rect),
            title: "下载器",
            header: None,
            content: &error,
            kind: AlertKind::Error,
            buttons: &[Button {
                label: "确定",
                role: Role::Ok,
            }],
            active: true,
        };
        if message::show(ui.ctx(), message, &mut state.error_focus, icons, |_, _| {}).is_some() {
            state.error = None;
        }
    }
    None
}
fn button(ui: &mut Ui, rect: egui::Rect, label: &str, primary: bool) -> egui::Response {
    if primary {
        ui.place(
            rect,
            egui::Button::new(RichText::new(label).size(12.).color(Color32::WHITE))
                .fill(Color32::from_rgb(25, 118, 210))
                .stroke(egui::Stroke::NONE)
                .corner_radius(4),
        )
    } else {
        crate::native_dialog::button(ui, rect, label, false)
    }
}
fn bytes(value: u64) -> String {
    if value >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", value as f64 / (1024. * 1024. * 1024.))
    } else if value >= 1024 * 1024 {
        format!("{:.1} MB", value as f64 / (1024. * 1024.))
    } else if value >= 1024 {
        format!("{:.1} KB", value as f64 / 1024.)
    } else {
        format!("{value} B")
    }
}
