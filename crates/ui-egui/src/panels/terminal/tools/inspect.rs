//! The original scrollable ContainerInspectPane and its real SSH operations.
use super::*;
use crate::tabs::ssh::inspect::Inspector;
#[cfg(test)]
mod tests;
mod widgets;

pub(super) fn render(ui: &mut Ui, workspace: &mut Workspace, icons: &mut crate::icons::Icons) {
    notice(ui, workspace, icons);
    let bounds = ui.max_rect();
    let busy = workspace.busy(Section::Docker);
    let header = egui::Rect::from_min_size(bounds.min, egui::vec2(bounds.width(), 30.));
    let footer = egui::Rect::from_min_max(
        egui::pos2(bounds.left(), (bounds.bottom() - 18.).max(header.bottom())),
        bounds.max,
    );
    ui.painter().rect_filled(header, 0, Color32::from_gray(245));
    ui.painter().rect_filled(footer, 0, Color32::from_gray(245));
    let name = workspace
        .inspector
        .as_ref()
        .map(|i| i.name.as_str())
        .filter(|n| !n.is_empty())
        .or(workspace.inspector_id.as_deref())
        .unwrap_or_default();
    ui.painter().text(
        header.left_center() + egui::vec2(8., 0.),
        egui::Align2::LEFT_CENTER,
        format!("容器属性 - {name}"),
        crate::native_dialog::font(ui.ctx(), 12.),
        Color32::from_gray(51),
    );
    let refresh = egui::Rect::from_center_size(
        header.right_center() - egui::vec2(20., 0.),
        egui::vec2(24., 22.),
    );
    icons.paint(
        ui,
        "images/connect/refresh.png",
        egui::Rect::from_center_size(refresh.center(), egui::vec2(16., 16.)),
    );
    let refreshing = ui
        .add_enabled_ui(!busy, |ui| {
            ui.interact(
                refresh,
                ui.id().with("inspect-refresh"),
                egui::Sense::click(),
            )
            .on_hover_text("刷新")
        })
        .inner
        .clicked();
    let mut action = None;
    let mut message = None;
    let mut error = None;
    let content = egui::Rect::from_min_max(header.left_bottom(), footer.right_top());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content));
    child.set_clip_rect(ui.clip_rect().intersect(content));
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .id_salt("inspect-content")
        .show(&mut child, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8., 10.);
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_min_width((content.width() - 40.).max(620.));
                    if let Some(inspector) = &mut workspace.inspector {
                        body(ui, inspector, busy, &mut action, &mut message, &mut error);
                    } else if busy {
                        ui.label("正在获取容器属性…");
                    }
                });
        });
    if let Some(error) = error {
        workspace.inspector_notice = Some(("输入错误".into(), error));
        workspace.inspector_notice_focus = true;
    }
    if let Some(message) = message {
        workspace.panes.entry(Section::Docker).or_default().status = message;
    }
    if refreshing && let Some(id) = &workspace.inspector_id {
        action = Some(ManagementAction::DockerInspect(id.clone()));
    }
    if let Some(action) = action {
        workspace.enqueue(Section::Docker, Operation::Manage(action));
    }
    let status = workspace
        .panes
        .get(&Section::Docker)
        .map_or("就绪", |p| p.status.as_str());
    ui.painter().with_clip_rect(footer).text(
        footer.left_center() + egui::vec2(8., 0.),
        egui::Align2::LEFT_CENTER,
        status,
        crate::native_dialog::font(ui.ctx(), 10.),
        Color32::from_gray(136),
    );
}
fn body(
    ui: &mut Ui,
    inspector: &mut Inspector,
    busy: bool,
    action: &mut Option<ManagementAction>,
    message: &mut Option<String>,
    error: &mut Option<String>,
) {
    ui.spacing_mut().interact_size.y = 23.;
    widgets::section(ui, "基本信息");
    egui::Grid::new("inspect-basic")
        .num_columns(2)
        .min_row_height(14.)
        .spacing([12., 6.])
        .show(ui, |ui| {
            for (label, value) in ["名称", "容器ID", "镜像", "状态", "创建时间", "启动命令"]
                .into_iter()
                .zip(&inspector.info)
            {
                ui.allocate_ui_with_layout(
                    egui::vec2(80., 14.),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_width(80.);
                        ui.add(egui::Label::new(
                            egui::RichText::new(label)
                                .font(crate::native_dialog::font(ui.ctx(), 11.))
                                .color(Color32::from_gray(136)),
                        ))
                    },
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(if value.is_empty() { "-" } else { value })
                            .font(crate::native_dialog::font(ui.ctx(), 11.)),
                    )
                    .wrap()
                    .selectable(true),
                );
                ui.end_row();
            }
        });
    widgets::section(ui, "重启策略");
    ui.vertical(|ui|{ui.spacing_mut().item_spacing.y=6.;
    ui.push_id("inspect-restart", |ui| {
        ui.horizontal(|ui| {
            ui.label("策略:");
            egui::ComboBox::from_id_salt("policy")
                .width(140.)
                .selected_text(&inspector.policy)
                .show_ui(ui, |ui| {
                    for policy in ["no", "on-failure", "unless-stopped", "always"] {
                        ui.selectable_value(&mut inspector.policy, policy.into(), policy);
                    }
                });
            ui.label("重试次数:");
            ui.add_enabled_ui(inspector.policy == "on-failure", |ui| {
                widgets::retries(ui, &mut inspector.retry_input, &mut inspector.retries)
            });
            if ui
                .add_enabled_ui(!busy, |ui| widgets::button(ui, "应用"))
                .inner
                .clicked()
            {
                *action = Some(ManagementAction::DockerUpdate {
                    id: inspector.id.clone(),
                    restart: Some(
                        if inspector.policy == "on-failure" && inspector.retries > 0 {
                            format!("on-failure:{}", inspector.retries)
                        } else {
                            inspector.policy.clone()
                        },
                    ),
                    memory: None,
                    memory_swap: None,
                    cpus: None,
                    shares: None,
                });
            }
        });
    });
    widgets::hint(
        ui,
        &format!(
            "等效命令: docker update --restart=<策略>[:重试次数] {}   （如 --restart=unless-stopped）",
            inspector.id
        ),
    );
    });
    widgets::section(ui, "资源限制");
    ui.vertical(|ui|{ui.spacing_mut().item_spacing.y=6.;
    ui.push_id("inspect-resources", |ui| {
        ui.horizontal(|ui| {
            ui.label("内存:");
            widgets::field(ui, "memory", &mut inspector.memory, "如 512m", 90.);
            ui.label("CPU数:");
            widgets::field(ui, "cpus", &mut inspector.cpus, "如 1.5", 70.);
            ui.label("CPU份额:");
            widgets::field(ui, "shares", &mut inspector.shares, "如 1024", 80.);
            if ui
                .add_enabled_ui(!busy, |ui| widgets::button(ui, "应用"))
                .inner
                .clicked()
            {
                match management::docker::resource_update(
                    &inspector.id,
                    &inspector.memory,
                    &inspector.cpus,
                    &inspector.shares,
                    inspector.limits,
                ) {
                    Ok(Some(update)) => *action = Some(update),
                    Ok(None) => *message = Some("未输入任何修改项（留空表示不修改）".into()),
                    Err(e) => *error = Some(e.to_string()),
                }
            }
        });
    });
    widgets::hint(
        ui,
        &format!(
            "等效命令: docker update --memory=512m --cpus=1.5 --cpu-shares=1024 {}   （留空表示不修改该项；内存支持 b/k/m/g 后缀，输入 0 可取消限制）",
            inspector.id
        ),
    );
    });
    for (label, text) in ["端口映射", "环境变量", "挂载", "网络"]
        .into_iter()
        .zip(&inspector.details)
    {
        widgets::section(ui, label);
        widgets::detail(ui, label, text);
    }
}
fn notice(ui: &mut Ui, workspace: &mut Workspace, icons: &mut crate::icons::Icons) {
    use crate::native_dialog::message::{self, Button, Kind, Message, Role};
    let Some((title, content)) = &workspace.inspector_notice else {
        return;
    };
    let answer = message::show(
        ui.ctx(),
        Message {
            id: egui::ViewportId::from_hash_of(ui.id().with("inspect-notice")),
            owner: ui.ctx().viewport_rect().into(),
            title,
            header: None,
            content,
            kind: Kind::Error,
            buttons: &[Button {
                label: "确定",
                role: Role::Ok,
            }],
            active: true,
        },
        &mut workspace.inspector_notice_focus,
        icons,
        |_, _| {},
    );
    if answer.is_some() {
        workspace.inspector_notice = None;
    }
}
