use super::*;
use crate::tabs::redis::{Dialog, Node};
use tomato_services::redis_data::Action;

mod dialogs;
mod tree;
#[cfg(test)]
mod workflow_tests;
pub(super) fn render(
    ui: &mut Ui,
    tab: &mut Tab,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    let Panel::Redis {
        connection, view, ..
    } = &mut tab.panel
    else {
        return None;
    };
    if view.current.is_none() || matches!(view.kind.as_str(), "none" | "stream") {
        view.editor.release(ui.ctx());
    }
    let modal = view.dialog.is_some() || view.failure.is_some();
    let mut action = None;
    if !view.loaded && view.followup.is_none() && !tab.busy && !modal && tab.error.is_none() {
        action = Some(Action::Scan {
            pattern: view
                .scan_pattern
                .get_or_insert_with(|| view.pattern.clone())
                .clone(),
            cursor: 0,
        });
    }
    ui.spacing_mut().button_padding = egui::vec2(10., 3.);
    ui.spacing_mut().interact_size.y = 23.;
    ui.add_enabled_ui(!tab.busy && !modal, |ui| {
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(RichText::new("刷新").color(Color32::WHITE))
                        .fill(crate::theme::GREEN),
                )
                .clicked()
            {
                action = Some(view.begin_scan());
            }
            if ui.button("添加Key").clicked() {
                view.open_add();
            }
            if ui
                .add_enabled(!view.selected.is_empty(), egui::Button::new("删除Key"))
                .clicked()
            {
                view.open_delete();
            }
            if ui
                .add_enabled(view.single_selected().is_some(), egui::Button::new("TTL"))
                .clicked()
            {
                view.open_ttl();
            }
        });
        ui.separator();
        egui::Panel::left(ui.id().with("redis-key-tree"))
            .default_size(ui.available_width() * 0.3)
            .min_size(150.)
            .max_size((ui.available_width() * 0.7).max(150.))
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let width = ui.available_width();
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(width, 23.), egui::Sense::hover());
                let input_rect = egui::Rect::from_min_max(
                    rect.min,
                    egui::pos2(rect.right() - 45., rect.bottom()),
                );
                let button_rect =
                    egui::Rect::from_min_max(egui::pos2(rect.right() - 40., rect.top()), rect.max);
                let response = ui.place(
                    input_rect,
                    TextEdit::singleline(&mut view.pattern)
                        .hint_text("搜索Key...")
                        .desired_width(input_rect.width() - 8.),
                );
                if ui
                    .place(button_rect, |ui: &mut Ui| {
                        ui.spacing_mut().button_padding = egui::vec2(6., 3.);
                        ui.add(
                            egui::Button::new(
                                RichText::new("搜索").size(12.).color(Color32::WHITE),
                            )
                            .fill(crate::theme::GREEN)
                            .wrap_mode(egui::TextWrapMode::Extend)
                            .min_size(button_rect.size()),
                        )
                    })
                    .clicked()
                    || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    action = Some(view.begin_scan());
                }
                tree::render(ui, view, &mut action, icons);
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.inner_margin(20))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 15.;
                if let Some(key) = &view.current {
                    ui.strong(format!("Key: {key}"));
                    ui.label(format!(
                        "Type: {}",
                        if view.kind == "none" {
                            "目录".to_owned()
                        } else {
                            view.kind.to_uppercase()
                        }
                    ));
                    let elapsed = (ui.input(|i| i.time) - view.received).max(0.) as i64;
                    let ttl = if view.ttl >= 0 {
                        (view.ttl - elapsed).max(0)
                    } else {
                        view.ttl
                    };
                    let (label, color) = if view.kind == "none" {
                        ("TTL: -".into(), Color32::from_gray(102))
                    } else {
                        crate::tabs::redis::ttl_label(ttl)
                    };
                    ui.colored_label(color, label);
                    if ttl > 0 {
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_secs(1));
                    }
                    ui.separator();
                    if view.kind == "none" {
                        ui.weak("这是一个目录前缀，不是实际的Key");
                    } else if view.kind == "stream" {
                        ui.colored_label(Color32::from_rgb(204, 0, 0), "不支持的类型: stream");
                    } else {
                        ui.label(match view.kind.as_str() {
                            "list" | "set" => "值（每行一个元素）：",
                            "hash" => "值（格式: field value，每行一个，空格分隔）：",
                            "zset" => "值（格式: score member，每行一个）：",
                            _ => "值：",
                        });
                        ScrollArea::vertical()
                            .id_salt("redis-value")
                            .max_height((ui.available_height() - 95.).max(80.))
                            .show(ui, |ui| {
                                let id = view.editor.prepare(ui, "redis-value-text");
                                ui.add_enabled(
                                    view.kind != "stream" && view.kind != "none",
                                    TextEdit::multiline(&mut view.text)
                                        .id(id)
                                        .code_editor()
                                        .desired_width(f32::INFINITY)
                                        .desired_rows(10),
                                );
                                view.editor.trim(ui.ctx());
                            });
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    view.kind != "stream" && view.kind != "none",
                                    egui::Button::new("保存"),
                                )
                                .clicked()
                            {
                                action = Some(Action::Write {
                                    key: key.clone(),
                                    kind: view.kind.clone(),
                                    text: view.text.clone(),
                                    create: false,
                                });
                            }
                            if ui.button("重置").clicked() {
                                action = Some(Action::Read { key: key.clone() });
                            }
                            if ui.button("复制Key名").clicked() {
                                ui.ctx().copy_text(key.clone());
                            }
                        });
                    }
                } else {
                    ui.strong("Key:");
                    ui.label("Type:");
                    ui.label("TTL:");
                    ui.separator();
                    ui.weak("选择左侧Key查看值");
                }
                if !view.status.is_empty() {
                    ui.label(&view.status);
                }
            });
    });
    dialogs::render(ui, view, tab.busy, &mut action);
    if action.is_none() && !tab.busy && view.dialog.is_none() && view.failure.is_none() {
        action = view.followup.take();
    }
    action.map(|action| request(connection, view.database, action))
}
fn request(connection: &tomato_core::Connection, database: u32, action: Action) -> Intent {
    let mut connection = connection.clone();
    connection.set("redisDatabase", database);
    Intent::command(Command::RedisData { connection, action })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redis_tree_virtualizes_rows_and_shift_selects_visible_range() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut view = crate::tabs::redis::State::default();
        view.accept(
            tomato_services::redis_data::Output::Keys {
                keys: (0..10_000).map(|i| format!("namespace:{i:05}")).collect(),
                cursor: 0,
            },
            0.,
        );
        view.expanded.insert("namespace".into());
        let mut frame = |mut events: Vec<egui::Event>| {
            let modifiers = events
                .iter()
                .rev()
                .find_map(|event| match event {
                    egui::Event::PointerButton { modifiers, .. } => Some(*modifiers),
                    _ => None,
                })
                .unwrap_or_default();
            events.insert(0, egui::Event::ModifiersChanged(modifiers));
            let mut action = None;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600., 400.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        tree::render(
                            ui,
                            &mut view,
                            &mut action,
                            &mut crate::icons::Icons::default(),
                        )
                    });
                },
            );
            output.textures_delta.clear();
            assert!(
                output.shapes.len() < 400,
                "10,000 keys must paint only visible rows"
            );
        };
        frame(vec![]);
        let first = egui::pos2(105., 53.);
        let third = egui::pos2(105., 113.);
        for (pos, modifiers) in [
            (first, egui::Modifiers::NONE),
            (third, egui::Modifiers::SHIFT),
        ] {
            frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
                },
            ]);
            frame(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers,
            }]);
        }
        assert_eq!(view.selected.len(), 3);
        assert!(view.selected.contains("namespace:00000"));
        assert!(view.selected.contains("namespace:00002"));
    }
}

#[cfg(test)]
mod layout_regressions {
    use super::*;
    #[test]
    fn redis_search_does_not_expand_the_sidebar_on_each_frame() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tab = crate::tabs::create_tab(tomato_core::Connection::new("REDIS"), None).unwrap();
        if let Panel::Redis { view, .. } = &mut tab.panel {
            view.loaded = true;
        }
        let mut icons = crate::icons::Icons::default();
        let mut positions = Vec::new();
        for index in 0..30 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900., 600.),
                    )),
                    time: Some(index as f64 / 60.),
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let _ = render(ui, &mut tab, &mut icons);
                    });
                },
            );
            output.textures_delta.clear();
            let key_x = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == "Key:" => Some(text.pos.x),
                    _ => None,
                })
                .unwrap();
            for shape in &output.shapes {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.text() == "搜索"
                {
                    assert_eq!(text.galley.rows.len(), 1, "search caption must never wrap");
                }
            }
            positions.push(key_x);
        }
        assert!(
            (260. ..330.).contains(&positions[0]),
            "30% sidebar expected, got {}",
            positions[0]
        );
        assert!(
            (positions[29] - positions[0]).abs() < 1.,
            "sidebar width drifted: {positions:?}"
        );
    }
}
