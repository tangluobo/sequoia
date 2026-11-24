mod body;
mod config;
mod dialogs;
mod forms;
use super::*;
use crate::tabs::server::{Edit, ServerView};
use tomato_core::server::{ServerAccount, ServerType, SharedDirectory};
pub(super) fn render(
    ui: &mut Ui,
    tab: &mut Tab,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    let Panel::Server(view) = &mut tab.panel else {
        return None;
    };
    let running = view.running.as_ref().is_some_and(|s| !s.finished());
    let mut intent = None;
    let mut changed = false;
    ui.spacing_mut().item_spacing.y = 0.;
    egui::Frame::new()
        .fill(Color32::from_rgb(247, 248, 250))
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.spacing_mut().button_padding.y = 3.;
            ui.spacing_mut().interact_size.y = 25.;
            ui.horizontal(|ui| {
                let (icon, title, port) = match view.config.kind {
                    ServerType::Http => (
                        "images/connect/connection-icons/http-server.svg.png",
                        "HTTP文件服务器",
                        8080,
                    ),
                    ServerType::Ftp => (
                        "images/connect/connection-icons/ftp-server.svg.png",
                        "FTP文件服务器",
                        2121,
                    ),
                    ServerType::Smb => (
                        "images/connect/connection-icons/smb-server.svg.png",
                        "SMB文件共享服务器",
                        445,
                    ),
                };
                let (rect, _) = ui.allocate_exact_size(egui::vec2(24., 24.), egui::Sense::hover());
                icons.paint(ui, icon, rect);
                ui.label(RichText::new(format!("{title} 服务器")).size(16.).strong());
                egui::Frame::new()
                    .fill(Color32::from_gray(240))
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(8, 2))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("默认端口 {port}"))
                                .size(11.)
                                .color(Color32::GRAY),
                        );
                    });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(10.);
                    if let Some(server) = &view.running {
                        ui.label(
                            RichText::new(format!(
                                "{}://{}",
                                view.config.kind.scheme(),
                                server.address
                            ))
                            .size(11.)
                            .color(Color32::GRAY),
                        );
                    }
                    if ui
                        .add_enabled(
                            !tab.busy,
                            egui::Button::new(
                                RichText::new(if running {
                                    "停止服务"
                                } else {
                                    "启动服务"
                                })
                                .color(Color32::WHITE),
                            )
                            .min_size(egui::vec2(90., 25.))
                            .corner_radius(4)
                            .fill(if running {
                                Color32::from_rgb(244, 67, 54)
                            } else {
                                Color32::from_rgb(7, 193, 96)
                            }),
                        )
                        .clicked()
                    {
                        if running {
                            view.running = None;
                            view.log("==== 停止服务 ====");
                        } else if let Ok(port) = view.port.parse::<u16>() {
                            view.config.port = port;
                            if view.config.root_directory.is_empty()
                                && view.config.shared_directories.is_empty()
                                && !view
                                    .config
                                    .accounts
                                    .iter()
                                    .any(|a| !a.home_directory.is_empty())
                            {
                                view.dialog = Some(Edit::ConfirmStart);
                            } else {
                                intent = Some(Intent::StartConfigured(view.config.clone()));
                            }
                        } else {
                            tab.error = Some("端口必须为 0–65535 的整数".into());
                        }
                    }
                    ui.label(
                        RichText::new(if running { "运行中" } else { "已停止" }).color(
                            if running {
                                Color32::from_rgb(76, 175, 80)
                            } else {
                                Color32::from_gray(153)
                            },
                        ),
                    );
                    let (dot, _) =
                        ui.allocate_exact_size(egui::vec2(10., 10.), egui::Sense::hover());
                    ui.painter().circle_filled(
                        dot.center(),
                        5.,
                        if running {
                            Color32::from_rgb(7, 193, 96)
                        } else {
                            Color32::from_gray(189)
                        },
                    );
                });
            });
        });
    ScrollArea::vertical()
        .id_salt("file-server-body")
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 18,
                    right: 18,
                    top: 16,
                    bottom: 20,
                })
                .show(ui, |ui| {
                    changed |= ui.scope(|ui| config::render(ui, view)).inner;
                    ui.add_space(18.);
                    let (separator, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 1.),
                        egui::Sense::hover(),
                    );
                    ui.painter().hline(
                        separator.x_range(),
                        separator.top(),
                        egui::Stroke::new(1., Color32::from_gray(238)),
                    );
                    ui.add_space(18.);
                    config::tabs(ui, view);
                    egui::Frame::new()
                        .inner_margin(egui::Margin {
                            left: 0,
                            right: 0,
                            top: 0,
                            bottom: 4,
                        })
                        .show(ui, |ui| body::render(ui, view));
                });
        });
    if let Some(next) = dialogs::render(ui, view, &mut tab.error, &mut changed) {
        intent = Some(next);
    }
    if changed {
        view.changed = true;
        view.save_failed = false;
        view.last_edit = std::time::Instant::now();
    }
    if view.changed && !view.save_failed {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
        if view.last_edit.elapsed() >= std::time::Duration::from_millis(750)
            && intent.is_none()
            && let Ok(port) = view.port.parse::<u16>()
        {
            view.config.port = port;
            intent = Some(Intent::ConfigureServer(view.config.clone()));
        }
    }
    intent
}
