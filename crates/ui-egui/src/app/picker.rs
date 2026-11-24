use super::*;
use crate::{icons, theme};

impl TomatoApp {
    pub(super) fn type_picker(&mut self, ctx: &egui::Context) {
        let Some(mut category) = self.picker else {
            return;
        };
        let mut open = true;
        let mut cancel = false;
        let mut picked = None;
        egui::Window::new("新建连接")
            .id(egui::Id::new("type-picker"))
            .open(&mut open)
            .default_size([780.0, 800.0])
            .min_size([560.0, 520.0])
            .show(ctx, |ui| {
                egui::Frame::new().inner_margin(20).show(ui, |ui| {
                    ui.label(RichText::new("选择连接类型").size(16.0).strong());
                    ui.add_space(15.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                        for (index, label) in
                            ["数据库", "消息队列", "客户端", "工具", "服务器", "轻松一下"]
                                .iter()
                                .enumerate()
                        {
                            let button = egui::Button::new(RichText::new(*label).size(12.0))
                                .fill(if index == category {
                                    Color32::from_rgb(252, 255, 255)
                                } else {
                                    Color32::from_gray(224)
                                })
                                .corner_radius(0);
                            if ui.add(button).clicked() {
                                category = index;
                            }
                        }
                    });
                    let columns = ((ui.available_width() + 10.0) / 155.0).floor().max(1.0) as usize;
                    ScrollArea::vertical().max_height(620.0).show(ui, |ui| {
                        let mut tiles: Vec<(String, String, String, u16, Option<String>)> =
                            Vec::new();
                        if category == 0 {
                            for product in DATABASE_PROFILES {
                                tiles.push((
                                    product.family.into(),
                                    product.label.into(),
                                    format!(
                                        "images/connect/database-logos/{}.svg.png",
                                        product.id.split('-').next().unwrap_or(product.id)
                                    ),
                                    product.port,
                                    Some(product.id.into()),
                                ));
                            }
                            for (id, label) in [("REDIS", "Redis"), ("MONGODB", "MongoDB")] {
                                tiles.push((
                                    id.into(),
                                    label.into(),
                                    icons::connection(id).into(),
                                    if id == "REDIS" { 6379 } else { 27017 },
                                    None,
                                ));
                            }
                        } else if category == 3 || category == 5 {
                            for &(id, label) in TOOLS {
                                if tomato_core::catalog::is_relaxation_tool(id) != (category == 5) {
                                    continue;
                                }
                                tiles.push((
                                    "TOOL".into(),
                                    label.into(),
                                    icons::tool(id),
                                    0,
                                    Some(id.into()),
                                ));
                            }
                        } else {
                            for kind in KINDS {
                                let show = match category {
                                    1 => kind.group == "消息队列",
                                    4 => kind.group == "服务器",
                                    2 => !["数据库", "消息队列", "服务器", "工具"]
                                        .contains(&kind.group),
                                    _ => false,
                                };
                                if show {
                                    tiles.push((
                                        kind.id.into(),
                                        kind.label.into(),
                                        icons::connection(kind.id).into(),
                                        kind.port,
                                        None,
                                    ));
                                }
                            }
                        }
                        egui::Grid::new("connection-tiles")
                            .spacing([10.0, 10.0])
                            .show(ui, |ui| {
                                for (index, (kind, label, icon, port, profile)) in
                                    tiles.iter().enumerate()
                                {
                                    let (rect, response) = ui.allocate_exact_size(
                                        egui::vec2(145.0, 90.0),
                                        egui::Sense::click(),
                                    );
                                    let active = response.hovered();
                                    ui.painter().rect(
                                        rect,
                                        8,
                                        if active {
                                            Color32::from_rgb(232, 245, 233)
                                        } else {
                                            Color32::from_gray(245)
                                        },
                                        egui::Stroke::new(
                                            if active { 2.0 } else { 1.0 },
                                            if active {
                                                theme::GREEN
                                            } else {
                                                Color32::from_gray(224)
                                            },
                                        ),
                                        egui::StrokeKind::Inside,
                                    );
                                    self.icons.paint(
                                        ui,
                                        icon,
                                        egui::Rect::from_center_size(
                                            rect.center() - egui::vec2(0.0, 16.0),
                                            egui::vec2(32.0, 32.0),
                                        ),
                                    );
                                    ui.painter().text(
                                        rect.center() + egui::vec2(0.0, 14.0),
                                        egui::Align2::CENTER_CENTER,
                                        label,
                                        theme::java_font(12.0),
                                        Color32::from_gray(51),
                                    );
                                    if *port > 0 {
                                        ui.painter().text(
                                            rect.center() + egui::vec2(0.0, 32.0),
                                            egui::Align2::CENTER_CENTER,
                                            format!("端口 {port}"),
                                            theme::java_font(10.0),
                                            Color32::from_gray(153),
                                        );
                                    }
                                    if response.clicked() {
                                        let mut config = Connection::new(kind);
                                        config.name.clear();
                                        if kind == "TOOL" {
                                            config.options.remove("host");
                                            config.options.remove("port");
                                            if profile.as_deref() == Some("music_downloader") {
                                                config.name = label.clone();
                                            }
                                        } else {
                                            config.set("port", *port);
                                        }
                                        if let Some(profile) = profile {
                                            config.set(
                                                if kind == "TOOL" {
                                                    "toolType"
                                                } else {
                                                    "databaseProfileId"
                                                },
                                                profile.clone(),
                                            );
                                        }
                                        picked = Some(config);
                                    }
                                    if (index + 1) % columns == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_sized([80.0, 28.0], egui::Button::new("取消"))
                            .clicked()
                        {
                            cancel = true;
                        }
                    });
                });
            });
        self.picker = if open && !cancel {
            Some(category)
        } else {
            None
        };
        if let Some(mut config) = picked {
            self.picker = None;
            if config.text("toolType") == "game_dungeons" {
                self.open_tool("game_dungeons", "游戏副本管理");
                return;
            }
            config.parent_id = self.picker_parent.clone();
            self.edit_connection(config);
        }
    }
}
