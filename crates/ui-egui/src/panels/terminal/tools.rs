use super::*;
use crate::tabs::ssh::{Dialog, Form, Workspace};
use tomato_services::terminal::ssh::{
    tools::{ManagementAction, Operation, Section, management},
    zmodem::{Choice, TransferState},
};

mod columns;
mod dialogs;
mod files;
mod inspect;
mod monitor;
mod ports;
mod storage;
mod table;
mod transfer;
pub(super) use transfer::choose as choose_transfer;
pub(super) fn toolbar(
    ui: &mut Ui,
    bar: egui::Rect,
    workspace: &mut Workspace,
    icons: &mut crate::icons::Icons,
    enabled: bool,
) {
    for (i, section) in [
        Section::Monitor,
        Section::Files,
        Section::Ports,
        Section::Docker,
    ]
    .into_iter()
    .enumerate()
    {
        let rect = egui::Rect::from_center_size(
            egui::pos2(bar.right() - 14. - i as f32 * 28., bar.center().y),
            egui::vec2(24., 20.),
        );
        let response = ui
            .interact(
                rect,
                ui.make_persistent_id(("ssh-tool", section)),
                egui::Sense::click(),
            )
            .on_hover_text(if section == Section::Docker {
                "Docker"
            } else {
                section.label()
            });
        let active = workspace.visible.contains(&section);
        if active || response.hovered() {
            ui.painter().rect_filled(rect, 3, Color32::from_gray(224));
        }
        let icon = if section == Section::Docker {
            "ssh:docker"
        } else {
            section.icon()
        };
        icons.paint(
            ui,
            icon,
            egui::Rect::from_center_size(rect.center(), egui::vec2(16., 16.)),
        );
        if !active {
            ui.painter().rect_filled(
                rect,
                0,
                Color32::from_white_alpha(if enabled { 90 } else { 160 }),
            );
        }
        if enabled && response.clicked() {
            workspace.toggle(section);
        }
    }
}
pub(super) fn render(
    ui: &mut Ui,
    rect: egui::Rect,
    workspace: &mut Workspace,
    connection: &tomato_core::Connection,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    let mut intent = None;
    let file_page = workspace.standalone == Some(Section::Files);
    let content = if file_page {
        egui::Rect::from_min_max(
            rect.min,
            egui::pos2(rect.right(), (rect.bottom() - 27.).max(rect.top())),
        )
    } else {
        rect
    };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content));
    child.set_clip_rect(content.intersect(ui.clip_rect()));
    child.painter().rect_filled(rect, 0, Color32::WHITE);
    let external_toolbar = workspace.external_file_toolbar;
    if workspace.inspector_id.is_some() {
        inspect::render(&mut child, workspace, icons);
        return intent;
    }
    if workspace.standalone == Some(Section::Storage) {
        storage::render(&mut child, workspace);
        dialogs::render(&mut child, workspace);
        return intent;
    }
    if workspace.standalone == Some(Section::Ports) {
        intent = ports::render(&mut child, workspace, connection, icons);
        dialogs::render(&mut child, workspace);
        return intent;
    }
    if file_page && !external_toolbar && workspace.inspector_id.is_none() {
        intent = files::header(&mut child, workspace, connection, icons);
        workspace.external_file_toolbar = true;
    }
    egui::ScrollArea::vertical()
        .id_salt(("ssh-tools", &connection.id, workspace.standalone))
        .auto_shrink([false, false])
        .max_height(content.height())
        .show(&mut child, |ui| {
            let sections: Vec<_> = [
                Section::Files,
                Section::Monitor,
                Section::Ports,
                Section::Docker,
                Section::Storage,
                Section::Services,
            ]
            .into_iter()
            .filter(|s| workspace.visible.contains(s))
            .collect();
            for section in sections {
                ui.push_id(section, |ui| {
                    let panel_top = ui.cursor().min.y;
                    if section != Section::Files {
                        egui::Frame::new()
                            .fill(Color32::from_gray(245))
                            .inner_margin(egui::Margin::symmetric(8, 4))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(match section {
                                            Section::Files => "文件",
                                            Section::Monitor => "系统监控",
                                            Section::Ports => "端口监听",
                                            Section::Docker => "Docker 容器",
                                            Section::Storage => "存储",
                                            Section::Services => "服务",
                                            _ => "终端",
                                        })
                                        .strong()
                                        .size(12.)
                                        .color(Color32::from_gray(51)),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if section == Section::Monitor {
                                                ui.label(
                                                    RichText::new("每1秒更新")
                                                        .size(10.)
                                                        .color(Color32::from_gray(136)),
                                                );
                                            } else {
                                                let (rect, response) = ui.allocate_exact_size(
                                                    egui::vec2(24., 20.),
                                                    egui::Sense::click(),
                                                );
                                                icons.paint(
                                                    ui,
                                                    "images/connect/refresh.png",
                                                    egui::Rect::from_center_size(
                                                        rect.center(),
                                                        egui::vec2(16., 16.),
                                                    ),
                                                );
                                                if response.on_hover_text("刷新").clicked() {
                                                    workspace
                                                        .panes
                                                        .entry(section)
                                                        .or_default()
                                                        .refresh = true;
                                                }
                                            }
                                            if workspace.busy(section) {
                                                ui.spinner();
                                            }
                                        },
                                    );
                                });
                            });
                    }
                    match section {
                        Section::Files => {
                            if let Some(next) = files::render(ui, workspace, connection, icons) {
                                intent = Some(next);
                            }
                        }
                        Section::Monitor => monitor::render(ui, workspace),
                        _ => {
                            if let Some(next) = table::render(ui, workspace, section, connection) {
                                intent = Some(next);
                            }
                        }
                    }
                    if workspace.standalone.is_none()
                        && matches!(section, Section::Files | Section::Ports | Section::Docker)
                    {
                        ui.add_space((182. - (ui.cursor().min.y - panel_top)).max(0.));
                    }
                    if let Some(pane) = workspace.panes.get(&section)
                        && !file_page
                        && (section != Section::Monitor || !pane.status.starts_with("共 "))
                    {
                        ui.label(
                            RichText::new(&pane.status)
                                .size(11.)
                                .color(Color32::from_gray(90)),
                        );
                    }
                    if !file_page {
                        ui.separator();
                    }
                });
            }
        });
    workspace.external_file_toolbar = external_toolbar;
    if file_page {
        let footer = egui::Rect::from_min_max(egui::pos2(rect.left(), content.bottom()), rect.max);
        file_footer(ui, footer, workspace, connection);
    }
    dialogs::render(ui, workspace);
    if std::mem::take(&mut workspace.reconnect_requested) {
        intent = Some(Intent::ReconnectTerminal);
    }
    intent
}
pub(crate) fn file_header(
    ui: &mut Ui,
    workspace: &mut Workspace,
    connection: &tomato_core::Connection,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    files::header(ui, workspace, connection, icons)
}
pub(crate) fn file_footer(
    ui: &mut Ui,
    footer: egui::Rect,
    workspace: &mut Workspace,
    connection: &tomato_core::Connection,
) {
    ui.painter().rect_filled(footer, 0, Color32::from_gray(248));
    ui.painter().hline(
        footer.x_range(),
        footer.top(),
        egui::Stroke::new(1., Color32::from_gray(221)),
    );
    let label = workspace
        .panes
        .get(&Section::Files)
        .map(|p| p.status.as_str())
        .unwrap_or("就绪");
    let mut x = footer.left() + 10.;
    if matches!(connection.kind(), "FTP" | "SFTP") {
        let ready = workspace.file_status == tomato_services::terminal::Status::Ready;
        ui.painter().circle_filled(
            egui::pos2(x + 5., footer.center().y),
            5.,
            if ready {
                Color32::from_rgb(76, 175, 80)
            } else {
                Color32::GRAY
            },
        );
        x += 18.;
    }
    let text = match &workspace.file_status {
        tomato_services::terminal::Status::Connecting => "连接中...",
        tomato_services::terminal::Status::Failed(e) => e,
        _ => label,
    };
    let text = if matches!(connection.kind(), "FTP" | "SFTP") {
        format!(
            "{text}   |   {} ({}@{}:{})",
            connection.name,
            connection.text("username"),
            connection.text("host"),
            connection.number("port")
        )
    } else {
        text.to_owned()
    };
    ui.painter().with_clip_rect(footer).text(
        egui::pos2(x, footer.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(11.),
        Color32::from_gray(51),
    );
}
pub(crate) fn file_dialogs(ui: &mut Ui, workspace: &mut Workspace) {
    dialogs::render(ui, workspace);
}
