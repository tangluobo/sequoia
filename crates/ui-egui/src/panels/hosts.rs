use super::*;
use tomato_services::hosts::{self, Action, Group};

pub(super) fn render(
    ui: &mut Ui,
    tool: &mut ToolState,
    busy: bool,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    let view = tool.hosts.get_or_insert_with(Default::default);
    let paths = view
        .paths
        .clone()
        .unwrap_or_else(|| (hosts::legacy_path(), hosts::system_path()));
    let command = |action| {
        Intent::command(Command::Hosts {
            config_path: paths.0.clone(),
            system_path: paths.1.clone(),
            action,
        })
    };
    let Some(snapshot) = view.snapshot.as_ref() else {
        ui.heading("Hosts 文件管理");
        if !busy && (view.status.is_empty() || ui.button("重新加载").clicked()) {
            view.status = "正在读取 Hosts 配置".into();
            return Some(command(Action::Load));
        }
        return None;
    };
    let snapshot = snapshot.clone();
    if let Some(until) = view.status_until {
        if let Some(remaining) = until.checked_duration_since(std::time::Instant::now()) {
            ui.ctx().request_repaint_after(remaining);
        } else {
            view.status.clear();
            view.status_until = None;
        }
    }
    ui.spacing_mut().item_spacing = egui::vec2(10., 0.);
    ui.spacing_mut().interact_size.y = 22.;
    let mut save = None;
    egui::Frame::new()
        .fill(Color32::from_rgb(247, 248, 250))
        .inner_margin(egui::Margin::symmetric(10, 14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(18., 18.), egui::Sense::hover());
                icons.paint(ui, "images/tools/hosts-header.svg", rect);
                ui.label(RichText::new("Hosts 文件管理").size(16.).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("管理和切换不同环境的 Hosts 配置")
                            .size(12.)
                            .color(Color32::from_gray(153)),
                    );
                });
            });
        });
    let modal = view.rename.is_some()
        || view.delete
        || view.failure.is_some()
        || view.authorization.is_some();
    ui.add_enabled_ui(!busy && !modal, |ui| {
        egui::Panel::left(ui.id().with("hosts-environments"))
            .default_size(240.0)
            .min_size(150.0)
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let (title, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 44.),
                    egui::Sense::hover(),
                );
                ui.painter().text(
                    title.min + egui::vec2(10., 14.),
                    egui::Align2::LEFT_TOP,
                    "环境列表",
                    egui::FontId::proportional(14.),
                    Color32::from_gray(51),
                );
                let (buttons, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 51.),
                    egui::Sense::hover(),
                );
                let add_rect = egui::Rect::from_min_size(
                    buttons.min + egui::vec2(10., 5.),
                    egui::vec2(85., 26.),
                );
                if ui
                    .place(
                        add_rect,
                        egui::Button::new(
                            RichText::new("+ 新建分组").size(12.).color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(33, 150, 243)),
                    )
                    .clicked()
                {
                    let mut config = view.candidate();
                    let index = config.groups.len();
                    config.groups.push(Group {
                        name: format!("新分组{}", index + 1),
                        ..Default::default()
                    });
                    view.pending_selection = Some(Some(index));
                    view.pending_rename = true;
                    save = Some((config, None, false));
                }
                let (system, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 43.),
                    egui::Sense::click(),
                );
                if response.hovered() {
                    ui.painter()
                        .rect_filled(system, 0., Color32::from_gray(245));
                }
                ui.painter().text(
                    system.min + egui::vec2(10., 12.),
                    egui::Align2::LEFT_TOP,
                    "📄",
                    egui::FontId::proportional(16.),
                    Color32::from_gray(51),
                );
                ui.painter().text(
                    system.min + egui::vec2(36., 13.),
                    egui::Align2::LEFT_TOP,
                    "系统Hosts",
                    egui::FontId::proportional(14.),
                    Color32::from_gray(51),
                );
                ui.painter().hline(
                    system.x_range(),
                    system.bottom(),
                    egui::Stroke::new(1., Color32::from_gray(224)),
                );
                if response.clicked() {
                    view.pending_selection = Some(None);
                    save = Some((view.candidate(), None, false));
                }
                ScrollArea::vertical().show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.;
                    for (index, group) in snapshot.config.groups.iter().enumerate() {
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 53.),
                            egui::Sense::click(),
                        );
                        if view.selected == Some(index) {
                            ui.painter()
                                .rect_filled(rect, 0., Color32::from_rgb(232, 244, 255));
                        } else if response.hovered() {
                            ui.painter().rect_filled(rect, 0., Color32::from_gray(245));
                        }
                        ui.painter().text(
                            rect.min + egui::vec2(10., 8.),
                            egui::Align2::LEFT_TOP,
                            &group.name,
                            egui::FontId::proportional(14.),
                            Color32::from_gray(51),
                        );
                        ui.painter().text(
                            rect.min + egui::vec2(10., 29.),
                            egui::Align2::LEFT_TOP,
                            format!("{} 个条目", group.entries.len()),
                            egui::FontId::proportional(11.),
                            Color32::from_gray(153),
                        );
                        ui.painter().hline(
                            rect.x_range(),
                            rect.bottom(),
                            egui::Stroke::new(1., Color32::from_gray(240)),
                        );
                        let switch_rect = egui::Rect::from_center_size(
                            egui::pos2(rect.right() - 29., rect.center().y),
                            egui::vec2(38., 20.),
                        );
                        let toggled = ui
                            .place(switch_rect, |ui: &mut Ui| switch(ui, group.enabled))
                            .clicked();
                        if toggled {
                            let mut config = view.candidate();
                            config.groups[index].enabled = !group.enabled;
                            save = Some((config, None, true));
                        }
                        if response.clicked()
                            && !response
                                .interact_pointer_pos()
                                .is_some_and(|p| switch_rect.contains(p))
                        {
                            view.pending_selection = Some(Some(index));
                            save = Some((view.candidate(), None, false));
                        }
                        response.context_menu(|ui| {
                            if ui.button("编辑分组名").clicked() {
                                view.pending_selection = Some(Some(index));
                                view.pending_rename = true;
                                save = Some((view.candidate(), None, false));
                                ui.close();
                            }
                            if ui.button("删除分组").clicked() {
                                view.pending_selection = Some(Some(index));
                                view.pending_delete = true;
                                save = Some((view.candidate(), None, false));
                                ui.close();
                            }
                        });
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                if ui.is_enabled()
                    && ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::S))
                {
                    let apply = view
                        .selected
                        .and_then(|i| snapshot.config.groups.get(i))
                        .is_some_and(|g| g.enabled);
                    save = Some((
                        view.candidate(),
                        view.selected.is_none().then(|| view.text.clone()),
                        apply,
                    ));
                }
                numbered_editor(ui, view);
            });
    });
    if let Some(name) = &mut view.rename {
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new("编辑分组名")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                if busy || view.failure.is_some() || view.authorization.is_some() {
                    ui.disable();
                }
                ui.label("修改分组名称");
                let id = view.rename_editor.prepare(ui, "hosts-rename-name");
                let response = ui.add(TextEdit::singleline(name).id(id));
                view.rename_editor.apply_focus(&response);
                ui.horizontal(|ui| {
                    confirm = ui.button("确定").clicked();
                    cancel = ui.button("取消").clicked();
                });
            });
        if confirm
            && !name.trim().is_empty()
            && let Some(index) = view.selected
        {
            let name = name.trim().to_owned();
            let mut config = view.candidate();
            config.groups[index].name = name;
            save = Some((config, None, snapshot.config.groups[index].enabled));
        }
        if cancel {
            view.rename = None;
        }
    }
    if view.rename.is_none() {
        view.rename_editor.release(ui.ctx());
    }
    if view.delete {
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new("删除分组")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                if busy || view.failure.is_some() || view.authorization.is_some() {
                    ui.disable();
                }
                ui.label("确定删除此分组？已启用的分组将从系统 Hosts 中移除。");
                ui.horizontal(|ui| {
                    confirm = ui.button("删除").clicked();
                    cancel = ui.button("取消").clicked();
                });
            });
        if confirm && let Some(index) = view.selected {
            let mut config = view.candidate();
            let removed = config.groups.remove(index);
            view.pending_selection = Some(None);
            save = Some((config, None, removed.enabled));
        }
        if cancel {
            view.delete = false;
        }
    }
    let mut reload = false;
    if let Some(message) = &view.failure {
        let mut open = true;
        let mut dismiss = false;
        egui::Window::new("操作失败")
            .id(ui.id().with("hosts-failure"))
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(420.)
            .show(ui.ctx(), |ui| {
                ui.set_max_width(440.);
                ui.add(egui::Label::new(message).wrap());
                ui.horizontal(|ui| {
                    dismiss = ui.button("确定").clicked();
                    reload = ui.button("重新加载（放弃草稿）").clicked();
                });
            });
        if !open || dismiss || reload {
            view.failure = None;
            if view.rename.is_some() {
                view.rename_editor.focus();
            } else {
                view.editor.focus();
            }
        }
    }
    if reload {
        return Some(command(Action::Load));
    }
    if let Some(action) = authorization_dialog(ui, view) {
        return Some(command(action));
    }
    if !view.status.is_empty() {
        ui.label(&view.status);
    }
    save.map(|(config, system, apply)| {
        command(Action::Save {
            previous: Box::new(snapshot.as_ref().clone()),
            config,
            system,
            apply,
        })
    })
}

fn authorization_dialog(ui: &mut Ui, view: &mut crate::tabs::hosts::State) -> Option<Action> {
    let prompt = view.authorization.as_mut()?;
    let mut accept = false;
    let mut cancel = false;
    let mut open = true;
    let mut reload = false;
    egui::Window::new("需要管理员权限")
        .id(ui.id().with("hosts-sudo"))
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .default_width(340.)
        .show(ui.ctx(), |ui| {
            ui.label("写入系统 hosts 文件需要管理员权限");
            ui.add_space(10.);
            ui.label("请输入 sudo 密码:");
            ui.add_enabled_ui(!prompt.running, |ui| {
                let id = prompt.editor.prepare(ui, "hosts-sudo-password");
                let response = ui.add_sized(
                    [320., 26.],
                    TextEdit::singleline(prompt.password.edit())
                        .id(id)
                        .password(true)
                        .hint_text("sudo 密码")
                        .char_limit(1024),
                );
                prompt.editor.apply_focus(&response);
                // Keep cursor/focus but no password-bearing undo history in egui memory.
                if let Some(mut state) = egui::widgets::text_edit::TextEditState::load(ui.ctx(), id)
                {
                    state.set_undoer(Default::default());
                    state.store(ui.ctx(), id);
                }
                accept = (response.has_focus() || response.lost_focus())
                    && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            });
            if let Some(error) = &prompt.error {
                ui.add(egui::Label::new(error).wrap());
                reload = ui
                    .add_enabled(!prompt.running, egui::Button::new("重新加载（放弃草稿）"))
                    .clicked();
            }
            if prompt.running {
                ui.label("正在写入 Hosts…");
            }
            ui.add_space(10.);
            ui.horizontal(|ui| {
                accept |= ui
                    .add_enabled(!prompt.running, egui::Button::new("确认"))
                    .clicked();
                cancel = ui.button("取消").clicked();
            });
            cancel |= ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        });
    if reload {
        view.cancel_authorization(ui.ctx());
        return Some(Action::Load);
    }
    if cancel || !open {
        if prompt.running {
            prompt.cancel_requested = true;
            prompt.control.cancel();
            prompt.password = Default::default();
            prompt.error = Some("正在取消…".into());
        } else {
            view.cancel_authorization(ui.ctx());
        }
        return None;
    }
    if accept && !prompt.running {
        prompt.control = tomato_services::backup::Control::default();
        match std::mem::take(&mut prompt.password).into_authorization(prompt.control.clone()) {
            Ok(authorization) => {
                prompt.editor.release(ui.ctx());
                prompt.editor.reset();
                prompt.running = true;
                prompt.error = None;
                return Some(Action::SaveElevated {
                    request: prompt.request.clone(),
                    authorization: Some(authorization),
                });
            }
            Err(error) => {
                prompt.error = Some(error.to_string());
                prompt.editor.focus();
            }
        }
    }
    None
}

pub(super) fn switch(ui: &mut Ui, enabled: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(38., 20.), egui::Sense::click());
    let track = if !ui.is_enabled() {
        Color32::from_gray(224)
    } else if enabled {
        Color32::from_rgb(76, 175, 80)
    } else {
        Color32::from_gray(189)
    };
    ui.painter().rect_filled(rect, 10., track);
    let x = rect.center().x + if enabled { 9. } else { -9. };
    ui.painter().circle_filled(
        egui::pos2(x, rect.center().y + 1.),
        9.,
        Color32::from_black_alpha(25),
    );
    ui.painter()
        .circle_filled(egui::pos2(x, rect.center().y), 8., Color32::WHITE);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
fn numbered_editor(ui: &mut Ui, view: &mut crate::tabs::hosts::State) {
    ScrollArea::both()
        .id_salt("hosts-editor")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let id = view.editor.prepare(ui, "hosts-text");
            if ui.memory(|m| m.has_focus(id))
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::D))
            {
                duplicate_line(ui.ctx(), id, &mut view.text);
            }
            let origin = ui.cursor().min;
            let gutter = 45.;
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(origin.x, ui.clip_rect().top()),
                    egui::pos2(origin.x + gutter, ui.clip_rect().bottom()),
                ),
                0.,
                Color32::from_gray(245),
            );
            let area = egui::Rect::from_min_max(origin + egui::vec2(gutter, 0.), ui.max_rect().max);
            let output = ui
                .scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
                    TextEdit::multiline(&mut view.text)
                        .id(id)
                        .font(egui::FontId::monospace(12.))
                        .frame(egui::Frame::NONE)
                        .margin(egui::vec2(8., 8.))
                        .desired_width(ui.available_width() - 16.)
                        .desired_rows(32)
                        .show(ui)
                })
                .inner;
            view.editor.apply_focus(&output.response);
            let mut line = 1;
            let mut new_line = true;
            for row in &output.galley.rows {
                let y = output.galley_pos.y + row.pos.y;
                if new_line
                    && y + row.size.y >= ui.clip_rect().top()
                    && y <= ui.clip_rect().bottom()
                {
                    ui.painter().text(
                        egui::pos2(origin.x + 5., y),
                        egui::Align2::LEFT_TOP,
                        line.to_string(),
                        egui::FontId::monospace(12.),
                        Color32::from_gray(153),
                    );
                }
                new_line = row.ends_with_newline;
                if new_line {
                    line += 1;
                }
            }
            view.editor.trim(ui.ctx());
        });
}
fn duplicate_line(ctx: &egui::Context, id: egui::Id, text: &mut String) {
    use egui::text::{CCursor, CCursorRange};
    let mut state = egui::widgets::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    let range = state
        .cursor
        .char_range()
        .unwrap_or_else(|| CCursorRange::one(CCursor::new(text.chars().count())));
    let caret = range.primary.index.into();
    let byte = text
        .char_indices()
        .nth(caret)
        .map_or(text.len(), |(i, _)| i);
    let start = text[..byte].rfind('\n').map_or(0, |i| i + 1);
    let end = text[byte..].find('\n').map_or(text.len(), |i| byte + i);
    let insert = format!("{}\n", &text[start..end]);
    if text.len().saturating_add(insert.len()) > 8 * 1024 * 1024 {
        return;
    }
    let mut undo = state.take_undoer();
    undo.add_undo(&(range, text.clone()));
    let next = caret + insert.chars().count();
    text.insert_str(start, &insert);
    state
        .cursor
        .set_char_range(Some(CCursorRange::one(CCursor::new(next))));
    state.set_undoer(undo);
    state.store(ctx, id);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sudo_dialog_types_retries_and_cancels_without_background_save_or_password_history() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tool = fixture();
        let view = tool.hosts.as_mut().unwrap();
        let request = hosts::SaveRequest {
            previous: Box::new(view.snapshot.as_ref().unwrap().as_ref().clone()),
            config: view.candidate(),
            system: Some("127.0.0.2 fixture.test".into()),
            apply: false,
        };
        view.fail(&ctx, &hosts::ElevationRequired(Box::new(request)).into());
        frame(&ctx, &mut tool, vec![]);
        frame(&ctx, &mut tool, vec![]);
        let key = |key, modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let text = tool.hosts.as_ref().unwrap().text.clone();
        let (_, intent) = frame(
            &ctx,
            &mut tool,
            vec![key(egui::Key::S, egui::Modifiers::CTRL)],
        );
        assert!(
            intent.is_none(),
            "Ctrl+S must not save through the password modal"
        );
        frame(
            &ctx,
            &mut tool,
            vec![egui::Event::Text("fixture-secret".into())],
        );
        let password_id = ctx.memory(|m| m.focused()).unwrap();
        let state = egui::widgets::text_edit::TextEditState::load(&ctx, password_id).unwrap();
        assert_eq!(
            state
                .take_undoer()
                .memory_usage(|(_, text)| text.capacity()),
            0
        );
        let (_, intent) = frame(
            &ctx,
            &mut tool,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        let Some(Intent::Command(command)) = intent else {
            panic!("Enter must submit transient authorization")
        };
        let Command::Hosts {
            action:
                Action::SaveElevated {
                    request,
                    authorization,
                },
            ..
        } = *command
        else {
            panic!()
        };
        assert!(authorization.is_some());
        assert_eq!(request.system.as_deref(), Some("127.0.0.2 fixture.test"));
        let view = tool.hosts.as_mut().unwrap();
        assert!(view.authorization.as_ref().unwrap().password.is_empty());
        assert_eq!(view.text, text);
        assert!(egui::widgets::text_edit::TextEditState::load(&ctx, password_id).is_none());
        view.fail(&ctx, &anyhow::anyhow!("密码错误或权限不足"));
        frame(&ctx, &mut tool, vec![]);
        frame(
            &ctx,
            &mut tool,
            vec![egui::Event::Text("second-fixture-secret".into())],
        );
        assert!(
            !tool
                .hosts
                .as_ref()
                .unwrap()
                .authorization
                .as_ref()
                .unwrap()
                .password
                .is_empty()
        );
        let (_, intent) = frame(
            &ctx,
            &mut tool,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert!(
            intent.is_some(),
            "a corrected password must retry the frozen save request"
        );
        let control = tool
            .hosts
            .as_ref()
            .unwrap()
            .authorization
            .as_ref()
            .unwrap()
            .control
            .clone();
        frame(
            &ctx,
            &mut tool,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(
            control.cancelled(),
            "Escape must reach the running process controller"
        );
        tool.hosts
            .as_mut()
            .unwrap()
            .fail(&ctx, &anyhow::anyhow!("操作已取消"));
        assert!(tool.hosts.as_ref().unwrap().authorization.is_none());
        assert_eq!(tool.hosts.as_ref().unwrap().text, text);
    }
    fn fixture() -> ToolState {
        let mut tool = ToolState::new("hosts_file");
        let mut state = crate::tabs::hosts::State {
            selected: Some(0),
            ..Default::default()
        };
        let config:hosts::Config=serde_json::from_str(r#"{"groups":[{"name":"开发环境","enabled":false,"entries":[{"ip":"127.0.0.1","domain":"dev.test"}]}]}"#).unwrap();
        state.accept(hosts::Snapshot {
            config,
            system: "127.0.0.1 localhost".into(),
            config_stamp: None,
            system_stamp: None,
        });
        state.status.clear();
        tool.hosts = Some(Box::new(state));
        tool
    }
    fn frame(
        ctx: &egui::Context,
        tool: &mut ToolState,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Option<Intent>) {
        let mut intent = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900., 600.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    intent = render(ui, tool, false, &mut crate::icons::Icons::default())
                });
            },
        );
        output.textures_delta.clear();
        (output, intent)
    }
    #[test]
    fn disabled_group_ctrl_s_saves_config_without_system_write_and_switch_has_own_hitbox() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tool = fixture();
        let (output, _) = frame(&ctx, &mut tool, vec![]);
        let switch = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Rect(r)
                    if (r.rect.width() - 38.).abs() < 0.1
                        && (r.rect.height() - 20.).abs() < 0.1 =>
                {
                    Some(r.rect.center())
                }
                _ => None,
            })
            .expect("original 38x20 switch must render");
        let (_, intent) = frame(
            &ctx,
            &mut tool,
            vec![egui::Event::Key {
                key: egui::Key::S,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            }],
        );
        let Some(Intent::Command(command)) = intent else {
            panic!("Ctrl+S must save");
        };
        let Command::Hosts {
            action: Action::Save { apply, system, .. },
            ..
        } = *command
        else {
            panic!()
        };
        assert!(!apply);
        assert!(system.is_none());
        frame(
            &ctx,
            &mut tool,
            vec![
                egui::Event::PointerMoved(switch),
                egui::Event::PointerButton {
                    pos: switch,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let (_, intent) = frame(
            &ctx,
            &mut tool,
            vec![egui::Event::PointerButton {
                pos: switch,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let Some(Intent::Command(command)) = intent else {
            panic!("switch must submit");
        };
        let Command::Hosts {
            action: Action::Save { apply, config, .. },
            ..
        } = *command
        else {
            panic!()
        };
        assert!(apply);
        assert!(config.groups[0].enabled);
    }
    #[test]
    fn duplicate_unicode_line_preserves_caret_and_can_be_undone() {
        use egui::text::{CCursor, CCursorRange};
        let ctx = egui::Context::default();
        let id = egui::Id::new("hosts-text");
        let mut text = "第一行\n第二\n".to_owned();
        let mut state = egui::widgets::text_edit::TextEditState::default();
        state
            .cursor
            .set_char_range(Some(CCursorRange::one(CCursor::new(1))));
        state.store(&ctx, id);
        duplicate_line(&ctx, id, &mut text);
        assert_eq!(text, "第一行\n第一行\n第二\n");
        let state = egui::widgets::text_edit::TextEditState::load(&ctx, id).unwrap();
        let range = state.cursor.char_range().unwrap();
        assert_eq!(usize::from(range.primary.index), 5);
        let mut undo = state.take_undoer();
        let (_, before) = undo.undo(&(range, text)).unwrap();
        assert_eq!(before, "第一行\n第二\n");
    }
}

#[cfg(test)]
mod layout_regressions {
    use super::*;
    #[test]
    fn environment_switch_does_not_compress_the_next_row() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tool = ToolState::new("hosts_file");
        let view = tool.hosts.get_or_insert_with(Default::default);
        view.selected = Some(0);
        let config = hosts::Config {
            groups: vec![
                Group {
                    name: "开发环境".into(),
                    ..Default::default()
                },
                Group {
                    name: "测试环境".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        view.accept(hosts::Snapshot {
            config,
            system: String::new(),
            system_stamp: None,
            config_stamp: None,
        });
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900., 600.),
                )),
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let _ = render(ui, &mut tool, false, &mut crate::icons::Icons::default());
                });
            },
        );
        output.textures_delta.clear();
        let mut ys = Vec::new();
        for shape in output.shapes {
            if let egui::Shape::Text(text) = shape.shape
                && ["开发环境", "测试环境"].contains(&text.galley.text())
            {
                ys.push(text.pos.y);
            }
        }
        assert_eq!(ys.len(), 2);
        assert!((ys[1] - ys[0] - 53.).abs() < 0.1, "row spacing {ys:?}");
    }
}
