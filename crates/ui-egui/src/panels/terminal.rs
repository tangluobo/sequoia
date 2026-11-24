mod input;
#[cfg(all(test, windows))]
mod interaction_tests;
mod paint;
mod selection;
pub(crate) mod tools;
mod viewport;
use super::*;
use input::terminal_key;
use tomato_services::terminal::Status;

pub(super) fn render(
    ui: &mut Ui,
    tab: &mut Tab,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    let Panel::Terminal {
        connection,
        session,
        parser,
        view,
    } = &mut tab.panel
    else {
        return None;
    };
    let mut intent = None;
    let status = session.status();
    let transfer = session.transfer_state();
    if let Status::Failed(error) = &status {
        ui.colored_label(Color32::RED, error);
    }
    let bounds = ui.available_rect_before_wrap();
    let mut terminal = egui::Rect::from_min_max(
        bounds.min,
        egui::pos2(bounds.right(), (bounds.bottom() - 23.0).max(bounds.top())),
    );
    let standalone = view.ssh.as_ref().is_some_and(|w| w.standalone.is_some());
    if let Some(workspace) = &mut view.ssh {
        tools::choose_transfer(session, workspace, &transfer);
        if !workspace.visible.is_empty() {
            let divider = if standalone {
                bounds.left()
            } else {
                bounds.left() + bounds.width() * workspace.ratio
            };
            let tool_rect = egui::Rect::from_min_max(
                egui::pos2(divider + if standalone { 0. } else { 3. }, bounds.top()),
                egui::pos2(
                    bounds.right(),
                    if standalone {
                        bounds.bottom()
                    } else {
                        terminal.bottom()
                    },
                ),
            );
            intent = tools::render(ui, tool_rect, workspace, connection, icons);
            terminal.max.x = divider;
            if !standalone {
                let separator = egui::Rect::from_min_max(
                    egui::pos2(divider, terminal.top()),
                    egui::pos2(divider + 3., terminal.bottom()),
                );
                let response = ui.interact(
                    separator,
                    ui.make_persistent_id("ssh-split"),
                    egui::Sense::drag(),
                );
                ui.painter()
                    .rect_filled(separator, 0, Color32::from_gray(229));
                if response.dragged() {
                    workspace.ratio = ((ui
                        .input(|i| i.pointer.interact_pos())
                        .map(|p| p.x)
                        .unwrap_or(divider)
                        - bounds.left())
                        / bounds.width())
                    .clamp(0.15, 0.85);
                }
            }
        }
    }
    if !standalone && !parser.screen().alternate_screen() && parser.screen().history_len() > 0 {
        let track = egui::Rect::from_min_max(
            egui::pos2(terminal.right() - 12., terminal.top()),
            terminal.max,
        );
        terminal.max.x -= 12.;
        let old = parser.screen().scrollback();
        viewport::scrollbar(ui, parser, track);
        if old != parser.screen().scrollback() {
            view.paused_output = parser.screen().scrollback() > 0;
        }
    }
    let response = ui.interact(
        terminal,
        ui.make_persistent_id(("ssh-terminal", &tab.id)),
        egui::Sense::click_and_drag(),
    );
    ui.advance_cursor_after_rect(bounds);
    input::focus_terminal(ui, &response, view, status == Status::Ready, standalone);
    if !standalone
        && !response.has_focus()
        && (response.clicked() || response.secondary_clicked() || response.drag_started())
    {
        response.request_focus();
    }
    if !response.has_focus() {
        view.composition.clear();
    }
    if response.clicked() {
        view.clear_selection();
    }
    if response.is_pointer_button_down_on() {
        view.paused_output = true;
    }
    let font = crate::font_assets::terminal::font(ui.ctx(), view.font_size.unwrap_or(13.0));
    let cell_width = ui.fonts_mut(|fonts| fonts.glyph_width(&font, 'M')).max(1.0);
    let height = (font.size + 5.).max(18.);
    let cols = ((terminal.width() / cell_width) as u16).clamp(20, 500);
    let rows = ((terminal.height() / height) as u16).clamp(5, 200);
    if parser.screen().size() != (rows, cols) {
        parser.set_size(rows, cols);
        if let Err(error) = session.resize(rows, cols)
            && status == Status::Ready
        {
            tab.error = Some(error.to_string());
        }
    }
    if response.hovered() && !parser.screen().alternate_screen() {
        let delta = ui.input(|i| i.smooth_scroll_delta.y);
        if delta.abs() > 0.1 {
            let offset = parser.screen().scrollback();
            let lines = (delta.abs() / height).ceil() as usize;
            parser.set_scrollback(if delta > 0.0 {
                offset.saturating_add(lines)
            } else {
                offset.saturating_sub(lines)
            });
            view.paused_output = parser.screen().scrollback() > 0;
        }
    }
    let coordinate = |position: egui::Pos2| {
        (
            ((position.y - terminal.top() - 2.) / height)
                .floor()
                .clamp(0.0, f32::from(rows - 1)) as u16,
            ((position.x - terminal.left() - 2.) / cell_width)
                .floor()
                .clamp(0.0, f32::from(cols - 1)) as u16,
        )
    };
    if (response.double_clicked() || response.triple_clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let point = coordinate(pos);
        view.selection = Some(if response.triple_clicked() {
            crate::tabs::terminal::Selection {
                anchor: (parser.screen().visible_row_id() + u64::from(point.0), 0),
                end: (
                    parser.screen().visible_row_id() + u64::from(point.0),
                    cols - 1,
                ),
            }
        } else {
            selection::word(parser.screen(), point)
        });
        selection::snapshot(parser.screen(), view);
    }
    if response.has_focus()
        && matches!(status, Status::Exited | Status::Failed(_))
        && ui.input(|i| i.key_pressed(egui::Key::Enter))
    {
        intent = Some(Intent::ReconnectTerminal);
    }
    if response.drag_started()
        && let Some(pos) = ui.input(|i| i.pointer.press_origin())
    {
        let anchor = selection::character(parser.screen(), coordinate(pos));
        view.selection_snapshot = None;
        view.selection_anchor = Some(anchor.clone());
        view.selection = Some(anchor);
        view.paused_output = true;
    }
    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let now = ui.input(|i| i.time);
        if (pos.y < terminal.top() || pos.y > terminal.bottom())
            && !parser.screen().alternate_screen()
        {
            if now >= view.next_drag_scroll {
                let offset = parser.screen().scrollback();
                parser.set_scrollback(if pos.y < terminal.top() {
                    offset.saturating_add(1)
                } else {
                    offset.saturating_sub(1)
                });
                view.next_drag_scroll = now + 0.016;
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }
        let target = selection::character(parser.screen(), coordinate(pos));
        if let Some(anchor) = &view.selection_anchor {
            view.selection = Some(if target.anchor >= anchor.anchor {
                crate::tabs::terminal::Selection {
                    anchor: anchor.anchor,
                    end: target.end,
                }
            } else {
                crate::tabs::terminal::Selection {
                    anchor: anchor.end,
                    end: target.anchor,
                }
            });
        }
    }
    if response.drag_stopped() {
        selection::snapshot(parser.screen(), view);
    }
    if !standalone {
        let now = ui.input(|input| input.time);
        if response.has_focus()
            && ui.input(|input| {
                input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key { pressed: true, .. }
                            | egui::Event::Text(_)
                            | egui::Event::Paste(_)
                            | egui::Event::Copy
                            | egui::Event::Cut
                            | egui::Event::Ime(_)
                    )
                })
            })
        {
            view.cursor.reset(now);
        }
        let (cursor_visible, next_edge) = view.cursor.sample(now, status == Status::Ready);
        if !parser.screen().hide_cursor()
            && parser.screen().scrollback() == 0
            && ui.is_rect_visible(terminal)
            && ui.input(|input| input.viewport().minimized != Some(true))
            && let Some(delay) = next_edge
        {
            ui.ctx().request_repaint_after(delay);
        }
        paint::screen(
            ui,
            parser.screen(),
            terminal,
            &font,
            egui::vec2(cell_width, height),
            view.selection.as_ref(),
            cursor_visible,
        );
    }
    let bar = egui::Rect::from_min_max(egui::pos2(bounds.left(), terminal.bottom()), bounds.max);
    if !standalone {
        ui.painter()
            .rect_filled(bar, 0, Color32::from_rgb(255, 255, 251));
        ui.painter().hline(
            bar.x_range(),
            bar.top(),
            egui::Stroke::new(1.0, Color32::from_gray(224)),
        );
        let label = match &status {
            Status::Connecting => "连接中…",
            Status::Ready => match transfer.upload() {
                Some(true) => "ZModem 上传中...",
                Some(false) => "ZModem 下载中...",
                None => "已连接",
            },
            Status::Authentication { .. } => "等待认证",
            Status::Exited | Status::Failed(_) => "未连接",
        };
        ui.painter().circle_filled(
            egui::pos2(bar.left() + 14.0, bar.center().y),
            4.0,
            if status == Status::Ready {
                Color32::from_rgb(76, 175, 80)
            } else {
                Color32::RED
            },
        );
        ui.painter().text(
            egui::pos2(bar.left() + 25.0, bar.center().y),
            egui::Align2::LEFT_CENTER,
            format!(
                "{label}    {}@{}:{}    UTF-8",
                connection.text("username"),
                connection.text("host"),
                connection.number("port")
            ),
            egui::FontId::proportional(11.0),
            Color32::from_gray(51),
        );
        if let Some(workspace) = &mut view.ssh {
            tools::toolbar(ui, bar, workspace, icons, status == Status::Ready);
        }
    }
    let menu = egui::Popup::context_menu(&response).show(|ui| {
        if ui
            .add_enabled(view.has_selection(), egui::Button::new("复制"))
            .clicked()
        {
            ui.ctx().copy_text(selection::copy(parser.screen(), view));
            ui.close();
        }
        if ui.button("粘贴").clicked() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            ui.close();
        }
        if ui
            .add_enabled(view.has_selection(), egui::Button::new("复制并粘贴"))
            .clicked()
        {
            let copied = selection::copy(parser.screen(), view);
            ui.ctx().copy_text(copied.clone());
            let pasted = input::paste(copied, parser.screen().bracketed_paste());
            if let Err(error) = session.write(&pasted) {
                tab.error = Some(error.to_string());
            } else {
                view.clear_selection();
                parser.set_scrollback(0);
                view.paused_output = false;
            }
            ui.close();
        }
        if connection.kind() == "LOCAL_TERMINAL" {
            if ui.add(crate::compact_menu::Item::new("清屏")).clicked() {
                // Match LocalTerminalPane: clear the emulated display, never run a shell command.
                parser.process(b"\x1b[2J\x1b[H");
                parser.set_scrollback(0);
                view.clear_selection();
                view.paused_output = false;
                ui.ctx().request_repaint();
                ui.close();
            }
            return ui.will_parent_close();
        }
        ui.separator();
        if ui.button("全选").clicked() {
            view.paused_output = true;
            view.selection = Some(crate::tabs::terminal::Selection {
                anchor: (parser.screen().first_retained_row(), 0),
                end: (
                    parser.screen().first_retained_row()
                        + parser.screen().history_len() as u64
                        + u64::from(rows)
                        - 1,
                    cols - 1,
                ),
            });
            selection::snapshot(parser.screen(), view);
            ui.close();
        }
        if ui
            .add_enabled(view.has_selection(), egui::Button::new("清除选择"))
            .clicked()
        {
            view.clear_selection();
            ui.close();
        }
        ui.will_parent_close()
    });
    if menu.is_some_and(|menu| menu.inner) && !standalone {
        // A click on a popup item can take focus away from the terminal.
        view.focused_once = false;
        input::focus_terminal(ui, &response, view, status == Status::Ready, standalone);
        view.cursor.reset(ui.input(|input| input.time));
        ui.ctx().request_repaint();
    }
    if !standalone && response.has_focus() && status == Status::Ready {
        let (row, col) = parser.screen().cursor_position();
        let cursor = egui::Rect::from_min_size(
            terminal.min
                + egui::vec2(
                    2. + f32::from(col) * cell_width,
                    2. + f32::from(row) * height,
                ),
            egui::vec2(cell_width, height),
        );
        ui.output_mut(|output| {
            output.ime = Some(egui::output::IMEOutput {
                purpose: egui::IMEPurpose::Terminal,
                rect: terminal,
                cursor_rect: cursor,
                should_interrupt_composition: false,
            })
        });
        if !view.composition.is_empty() {
            paint::composition(
                &ui.painter().with_clip_rect(terminal),
                cursor,
                &view.composition,
                &font,
            );
        }
        let events = ui.input(|i| i.events.clone());
        let control_copy = input::has_control_key(&events, egui::Key::C);
        let control_paste = input::has_control_key(&events, egui::Key::V);
        for event in events {
            let data = match event {
                egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                    view.composition = text.chars().take(512).collect();
                    None
                }
                egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                    view.composition.clear();
                    Some(text)
                }
                egui::Event::Ime(egui::ImeEvent::DeleteSurrounding {
                    before_chars,
                    after_chars,
                }) => Some(format!(
                    "{}{}",
                    "\u{8}".repeat(before_chars.min(1024)),
                    "\u{1b}[3~".repeat(after_chars.min(1024))
                )),
                egui::Event::Key { .. } if !view.composition.is_empty() => None,
                egui::Event::Copy if control_copy => None,
                egui::Event::Copy => {
                    ui.ctx().copy_text(selection::copy(parser.screen(), view));
                    None
                }
                egui::Event::Cut => Some("\u{18}".into()),
                egui::Event::Paste(_) if control_paste => None,
                egui::Event::Paste(text) => {
                    view.clear_selection();
                    // RequestPaste delivers text on a later frame. The held modifiers may differ.
                    Some(input::paste(text, parser.screen().bracketed_paste()))
                }
                egui::Event::Text(text) => Some(text),
                egui::Event::Key {
                    key: egui::Key::V,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.shift && modifiers.ctrl => {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                    None
                }
                egui::Event::Key {
                    key: egui::Key::C,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.shift && modifiers.ctrl => {
                    ui.ctx().copy_text(selection::copy(parser.screen(), view));
                    None
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => terminal_key(key, modifiers, parser.screen().application_cursor()),
                _ => None,
            };
            if let Some(data) = data {
                let mut rz = false;
                for c in data.chars() {
                    match c {
                        '\r' | '\n' => {
                            let line = view.input_line.trim();
                            rz |= line == "rz" || line.starts_with("rz ");
                            view.input_line.clear();
                        }
                        '\u{3}' | '\u{15}' => view.input_line.clear(),
                        '\u{7f}' | '\u{8}' => {
                            view.input_line.pop();
                        }
                        c if !c.is_control() && view.input_line.len() < 4096 => {
                            view.input_line.push(c);
                        }
                        _ => {}
                    }
                }
                if let Err(error) = session.write(&data) {
                    tab.error = Some(error.to_string());
                } else if rz
                    && view.ssh.is_some()
                    && let Err(error) = session.prepare_upload()
                {
                    tab.error = Some(error.to_string());
                }
                parser.set_scrollback(0);
                view.paused_output = false;
                ui.ctx().request_repaint();
            }
        }
    }
    if let Status::Authentication { title, prompts } = status {
        view.focused_once = false;
        let first_prompt = view.answers.is_empty();
        view.answers.resize(prompts.len(), String::new());
        egui::Window::new(title)
            .id(egui::Id::new(("ssh-auth", &tab.id)))
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                let mut enter = false;
                for (index, prompt) in prompts.iter().enumerate() {
                    ui.label(&prompt.label);
                    let response = ui
                        .add(TextEdit::singleline(&mut view.answers[index]).password(!prompt.echo));
                    if index == 0 && first_prompt {
                        response.request_focus();
                    }
                    enter |= (response.has_focus() || response.lost_focus())
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                }
                ui.horizontal(|ui| {
                    if (ui.button("确定").clicked() || enter)
                        && let Err(error) = session.answer(std::mem::take(&mut view.answers))
                    {
                        tab.error = Some(error.to_string());
                    }
                    if ui.button("取消").clicked() {
                        intent = Some(Intent::Close);
                    }
                });
            });
    }
    intent
}
