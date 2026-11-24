use super::*;
mod dialog;
mod widgets;
use crate::tabs::port_mapping::{Editor, State};
use tomato_services::terminal::ssh::forwarding::Mapping;

const ROW_HEIGHT: f32 = 30.;
const HEADER_HEIGHT: f32 = 25.;
const FOOTER_HEIGHT: f32 = 22.;
const ACTION_WIDTH: f32 = 42.;

fn column_widths(width: f32) -> [f32; 7] {
    let scale = width / 890.;
    let action = (80. * scale).max(ACTION_WIDTH + 12.).min(width);
    let status = (90. * scale).max(62.).min(width - action);
    let text_scale = (width - action - status) / 720.;
    [
        150. * text_scale,
        180. * text_scale,
        110. * text_scale,
        140. * text_scale,
        140. * text_scale,
        status,
        action,
    ]
}

pub(super) fn render(
    ui: &mut Ui,
    state: &mut State,
    hosts: &[tomato_core::Connection],
) -> Option<Intent> {
    state.poll();
    if state.editor.is_some() || state.delete.is_some() || !state.reverse.is_empty() {
        ui.disable();
    }
    ui.spacing_mut().item_spacing = egui::vec2(8., 0.);
    ui.spacing_mut().button_padding = egui::vec2(10., 4.);
    ui.spacing_mut().interact_size.y = 25.;
    let selected: Vec<_> = state.selected.iter().copied().collect();
    let mut start = None;
    let mut stop = None;
    let (toolbar, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 42.), egui::Sense::hover());
    ui.painter()
        .rect_filled(toolbar, 0, Color32::from_gray(250));
    ui.painter().hline(
        toolbar.x_range(),
        toolbar.bottom(),
        egui::Stroke::new(1., Color32::from_gray(224)),
    );
    let stopped = !selected.is_empty()
        && selected.iter().all(|i| {
            !state.active.contains_key(&state.mappings[*i].id)
                && !state.busy(&state.mappings[*i].id)
        });
    let running = !selected.is_empty()
        && selected.iter().all(|i| {
            state.active.contains_key(&state.mappings[*i].id) || state.busy(&state.mappings[*i].id)
        });
    use widgets::Button;
    for (label, x, width, kind, enabled) in [
        ("添加", 10., 40., Button::Add, true),
        (
            "修改",
            58.,
            42.,
            Button::Neutral,
            stopped && selected.len() == 1,
        ),
        ("删除", 108., 42., Button::Neutral, stopped),
        ("启动", 158., 40., Button::Start, stopped),
        ("停止", 206., 42., Button::Neutral, running),
    ] {
        let filled = matches!(kind, Button::Add | Button::Start);
        let rect = egui::Rect::from_min_size(
            toolbar.min + egui::vec2(x, if filled { 9. } else { 8. }),
            egui::vec2(width, if filled { 24. } else { 26. }),
        );
        let response = widgets::button(
            ui,
            rect,
            ui.id().with(("mapping-toolbar", label)),
            label,
            kind,
            enabled,
        );
        if response.clicked() {
            match label {
                "添加" => {
                    state.editor = Some(Editor {
                        mapping: Mapping {
                            ssh_host_id: hosts
                                .iter()
                                .find(|h| matches!(h.kind(), "SSH" | "SFTP"))
                                .map(|h| h.id.clone())
                                .unwrap_or_default(),
                            ..Default::default()
                        },
                        local: String::new(),
                        remote: String::new(),
                        editing: None,
                        focus: true,
                    })
                }
                "修改" => {
                    let index = selected[0];
                    let mapping = state.mappings[index].clone();
                    state.editor = Some(Editor {
                        local: mapping.local_port.to_string(),
                        remote: mapping.remote_port.to_string(),
                        mapping,
                        editing: Some(index),
                        focus: true,
                    });
                }
                "删除" => {
                    state.delete = Some(
                        selected
                            .iter()
                            .map(|i| state.mappings[*i].id.clone())
                            .collect(),
                    )
                }
                "启动" => {
                    for index in &selected {
                        state.request_start(*index, hosts, ui.ctx());
                    }
                }
                "停止" => {
                    let ids: Vec<_> = selected
                        .iter()
                        .map(|i| state.mappings[*i].id.clone())
                        .collect();
                    for id in ids {
                        state.stop(&id);
                    }
                }
                _ => {}
            }
        }
    }
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    ui.spacing_mut().scroll.bar_width = 13.;
    let remaining = ui.available_rect_before_wrap();
    let footer = egui::Rect::from_min_max(
        egui::pos2(
            remaining.left(),
            (remaining.bottom() - FOOTER_HEIGHT).max(remaining.top()),
        ),
        remaining.max,
    );
    let (header, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), HEADER_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(header, 0, Color32::from_gray(245));
    let body = ScrollArea::vertical()
        .max_height((footer.top() - header.bottom()).max(0.))
        .min_scrolled_height(0.)
        .auto_shrink([false, false])
        .show_rows(ui, ROW_HEIGHT, state.mappings.len(), |ui, range| {
            // Measure after ScrollArea has reserved the scrollbar for this frame.
            let width = ui.available_width();
            let widths = column_widths(width);
            for i in range {
                let mapping = &state.mappings[i];
                let running = state.active.contains_key(&mapping.id) || state.busy(&mapping.id);
                let forward_state = state.active.get(&mapping.id).map(|forward| forward.state());
                let connected = matches!(forward_state, Some(tomato_services::terminal::ssh::forwarding::ForwardState::Running));
                let pending = state.busy(&mapping.id);
                let host = hosts
                    .iter()
                    .find(|h| h.id == mapping.ssh_host_id)
                    .map(|h| format!("{} ({}:{})", h.name, h.text("host"), h.number("port")))
                    .unwrap_or_else(|| "(未知主机)".into());
                let name = if mapping.name.is_empty() {
                    format!(
                        "{} {}→{}",
                        mapping.direction, mapping.local_port, mapping.remote_port
                    )
                } else {
                    mapping.name.clone()
                };
                let (row, response) = ui.allocate_exact_size(
                    egui::vec2(width, ROW_HEIGHT),
                    egui::Sense::click(),
                );
                if state.selected.contains(&i) {
                    ui.painter()
                        .rect_filled(row, 0, Color32::from_rgb(53, 146, 203));
                }
                if response.clicked() {
                    let mods = ui.input(|x| x.modifiers);
                    if mods.shift {
                        let anchor = state.anchor.unwrap_or(i);
                        if !mods.ctrl {
                            state.selected.clear();
                        }
                        state.selected.extend(anchor.min(i)..=anchor.max(i));
                    } else {
                        if !mods.ctrl {
                            state.selected.clear();
                        }
                        if !state.selected.insert(i) {
                            state.selected.remove(&i);
                        }
                        state.anchor = Some(i);
                    }
                }
                if response.double_clicked() && !running {
                    start = Some(i);
                }
                let texts = [
                    name,
                    host,
                    if mapping.direction == "L" {
                        "本地→远程"
                    } else {
                        "远程→本地"
                    }
                    .into(),
                    format!("{}:{}", mapping.local_host, mapping.local_port),
                    format!("{}:{}", mapping.remote_host, mapping.remote_port),
                    if pending || matches!(forward_state, Some(tomato_services::terminal::ssh::forwarding::ForwardState::Connecting)) {
                        "正在连接…"
                    } else if connected {
                        "● 运行中"
                    } else if running {
                        "正在重连…"
                    } else {
                        "● 已停止"
                    }
                    .into(),
                ];
                let mut x = row.left();
                for (column, (text, width)) in texts.into_iter().zip(widths).enumerate() {
                    let cell = egui::Rect::from_min_max(
                        egui::pos2(x + 6., row.top()),
                        egui::pos2(x + width - 4., row.bottom()),
                    );
                    let painter = ui.painter().with_clip_rect(cell.intersect(ui.clip_rect()));
                    if column == 5 {
                        painter.circle_filled(
                            cell.left_center() + egui::vec2(5., 0.),
                            5.,
                            if connected {
                                Color32::from_rgb(76, 175, 80)
                            } else if running || pending {
                                Color32::from_rgb(230, 160, 50)
                            } else {
                                Color32::from_gray(189)
                            },
                        );
                    }
                    let text_rect = egui::Rect::from_min_max(
                        cell.min + egui::vec2(if column == 5 { 16. } else { 0. }, 0.),
                        cell.max,
                    );
                    widgets::cell_text(
                        ui,
                        text_rect,
                        egui::Align2::LEFT_CENTER,
                        text.trim_start_matches("● "),
                        crate::native_dialog::font(ui.ctx(), 12.),
                        if column == 5 {
                            if connected { Color32::from_rgb(46, 125, 50) } else if running { Color32::from_rgb(175, 112, 20) } else { Color32::from_gray(153) }
                        } else if state.selected.contains(&i) { Color32::WHITE } else { Color32::BLACK },
                    );
                    x += width;
                    ui.painter().vline(x, row.y_range(), egui::Stroke::new(1., Color32::from_gray(224)));
                }
                let rect = egui::Rect::from_center_size(
                    egui::pos2(x + 6. + ACTION_WIDTH * 0.5, row.center().y),
                    egui::vec2(ACTION_WIDTH, 22.),
                );
                let response = widgets::button(ui, rect, ui.id().with(("mapping-toggle", &mapping.id)), if running { "停止" } else { "启动" }, if running { Button::Stop } else { Button::RowStart }, true);
                ui.painter().vline(row.right(), row.y_range(), egui::Stroke::new(1., Color32::from_gray(224)));
                ui.painter().hline(row.x_range(), row.bottom(), egui::Stroke::new(1., Color32::from_gray(224)));
                if let Some(tomato_services::terminal::ssh::forwarding::ForwardState::Retrying { attempt, delay, error }) = &forward_state {
                    response.clone().on_hover_text(format!("第 {attempt} 次重试，{} 秒后重新连接\n{error}", delay.as_secs()));
                }
                if response.clicked() {
                    if running {
                        stop = Some(mapping.id.clone());
                    } else {
                        start = Some(i);
                    }
                }
            }
            widths
        });
    let mut x = header.left();
    for (label, width) in ["名称", "SSH主机", "方向", "本地", "远程", "状态", "操作"]
        .into_iter()
        .zip(body.inner)
    {
        let cell = egui::Rect::from_min_size(
            egui::pos2(x, header.top()),
            egui::vec2(width, HEADER_HEIGHT),
        );
        widgets::cell_text(
            ui,
            cell,
            egui::Align2::CENTER_CENTER,
            label,
            widgets::font(ui.ctx(), 12., true),
            Color32::from_gray(51),
        );
        x += width;
        ui.painter().vline(
            x,
            header.y_range(),
            egui::Stroke::new(1., Color32::from_rgb(218, 218, 216)),
        );
    }
    ui.painter().hline(
        header.x_range(),
        header.bottom(),
        egui::Stroke::new(1., Color32::from_gray(204)),
    );
    if let Some(i) = start {
        state.request_start(i, hosts, ui.ctx());
    }
    if let Some(id) = stop {
        state.stop(&id);
    }
    ui.painter().rect_filled(footer, 0, Color32::from_gray(245));
    ui.painter()
        .with_clip_rect(footer.intersect(ui.clip_rect()))
        .text(
            footer.left_center() + egui::vec2(10., 0.),
            egui::Align2::LEFT_CENTER,
            &state.status,
            crate::native_dialog::font(ui.ctx(), 11.),
            Color32::from_gray(136),
        );
    if !state.active.is_empty() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(1));
    }
    dialog::show(ui.ctx(), state, hosts);
    None
}

#[cfg(test)]
mod tests;
