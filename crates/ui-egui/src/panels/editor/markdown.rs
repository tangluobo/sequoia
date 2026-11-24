use super::*;
use crate::tabs::markdown::Mode;
use actions::Action;
fn icon(ui: &mut Ui, glyph: char, label: &str, selected: bool) -> bool {
    ui.add(
        egui::Button::new(
            RichText::new(glyph.to_string())
                .font(egui::FontId::new(
                    14.,
                    egui::FontFamily::Name("fontawesome".into()),
                ))
                .color(if label.starts_with("保存") {
                    crate::theme::GREEN
                } else if selected {
                    Color32::from_rgb(26, 115, 232)
                } else {
                    Color32::from_gray(68)
                }),
        )
        .frame(selected)
        .fill(if selected {
            Color32::from_gray(222)
        } else {
            Color32::TRANSPARENT
        })
        .min_size(egui::vec2(24., 24.)),
    )
    .on_hover_text(label)
    .clicked()
}
pub(super) fn render(ui: &mut Ui, tab: &mut Tab) -> Option<Intent> {
    let Panel::Editor {
        path,
        text,
        markdown: state,
        ..
    } = &mut tab.panel
    else {
        return None;
    };
    let mut intent = None;
    let mut action = None;
    let editor_id = ui.make_persistent_id(("document-editor", &tab.id));
    state.editor_id = Some(editor_id);
    let now = ui.input(|i| i.time);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 0.;
        ui.spacing_mut().interact_size.y = 24.;
        ui.spacing_mut().button_padding = egui::vec2(4., 4.);
        egui::Frame::new()
            .fill(Color32::from_rgb(249, 249, 249))
            .inner_margin(egui::Margin::symmetric(4, 2))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (glyph, label, command) in [
                        ('\u{f0e2}', "撤销 (Ctrl+Z)", Action::Undo),
                        ('\u{f01e}', "重做 (Ctrl+Y)", Action::Redo),
                        ('\u{f032}', "加粗 (Ctrl+B)", Action::Wrap("**", "**")),
                        ('\u{f033}', "斜体 (Ctrl+I)", Action::Wrap("*", "*")),
                        ('\u{f0cc}', "删除线 (Ctrl+T)", Action::Wrap("~~", "~~")),
                        ('\u{f121}', "行内代码 (Ctrl+`)", Action::Wrap("`", "`")),
                        ('\u{f0c1}', "链接 (Ctrl+L)", Action::Link),
                        ('\u{f03e}', "图片 (Ctrl+G)", Action::Image),
                        ('\u{f1dc}', "标题1 (Ctrl+1)", Action::Prefix("# ")),
                        ('\u{f1dc}', "标题2 (Ctrl+2)", Action::Prefix("## ")),
                        ('\u{f1dc}', "标题3 (Ctrl+3)", Action::Prefix("### ")),
                        ('\u{f0ca}', "无序列表 (Ctrl+U)", Action::Prefix("- ")),
                        ('\u{f0cb}', "有序列表", Action::Prefix("1. ")),
                        ('\u{f14a}', "任务列表", Action::Prefix("- [ ] ")),
                        ('\u{f10d}', "引用 (Ctrl+Q)", Action::Prefix("> ")),
                        ('\u{f1c9}', "代码块 (Ctrl+Shift+`)", Action::CodeBlock),
                        ('\u{f068}', "分隔线", Action::Rule),
                        ('\u{f0ce}', "表格", Action::Table),
                    ] {
                        if icon(ui, glyph, label, false) {
                            action = Some(command);
                        }
                        if matches!(
                            label,
                            "重做 (Ctrl+Y)"
                                | "行内代码 (Ctrl+`)"
                                | "图片 (Ctrl+G)"
                                | "标题3 (Ctrl+3)"
                                | "表格"
                        ) {
                            ui.separator();
                        }
                    }
                    ui.separator();
                    if !tab.busy && icon(ui, '\u{f1c9}', "导出HTML", false) {
                        intent = Some(Intent::ExportMarkdownHtml);
                    }
                    if !tab.busy && icon(ui, '\u{f1c1}', "导出PDF", false) {
                        intent = Some(Intent::PrintMarkdown);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon(ui, '\u{f0c7}', "保存 (Ctrl+S)", false) && !tab.busy {
                            intent = Some(Intent::Save(path.clone(), text.clone()));
                        }
                        ui.separator();
                        for (mode, glyph, label) in [
                            (Mode::Preview, '\u{f06e}', "预览"),
                            (Mode::Split, '\u{f0db}', "编辑+预览"),
                            (Mode::Edit, '\u{f040}', "编辑"),
                        ] {
                            if icon(ui, glyph, label, state.mode == mode) {
                                state.mode = mode;
                            }
                        }
                    });
                });
            });
    });
    let focused = ui.memory(|m| m.has_focus(editor_id));
    if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) && !tab.busy {
        intent = Some(Intent::Save(path.clone(), text.clone()));
    }
    if focused {
        if ui.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Backtick,
            )
        }) {
            action = Some(Action::CodeBlock);
        } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backtick)) {
            action = Some(Action::Wrap("`", "`"));
        }
        for (key, command) in [
            (egui::Key::B, Action::Wrap("**", "**")),
            (egui::Key::I, Action::Wrap("*", "*")),
            (egui::Key::T, Action::Wrap("~~", "~~")),
            (egui::Key::L, Action::Link),
            (egui::Key::G, Action::Image),
            (egui::Key::U, Action::Prefix("- ")),
            (egui::Key::Q, Action::Prefix("> ")),
            (egui::Key::Num1, Action::Prefix("# ")),
            (egui::Key::Num2, Action::Prefix("## ")),
            (egui::Key::Num3, Action::Prefix("### ")),
        ] {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, key)) {
                action = Some(command);
            }
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
            state.find = true;
            state.find_focus = true;
            state.replace_visible = false;
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::R)) {
            state.find = true;
            state.find_focus = true;
            state.replace_visible = true;
        }
    }
    if let Some(command) = action {
        actions::apply(ui.ctx(), editor_id, text, command);
        state.changed(now);
    }
    if state.find {
        super::find::render(ui, state, text, editor_id, now);
    }
    let bounds = ui.available_rect_before_wrap();
    let mut image = None;
    if state.mode != Mode::Preview {
        let edit = if state.mode == Mode::Split {
            egui::Rect::from_min_max(
                bounds.min,
                egui::pos2(
                    bounds.left() + bounds.width() * state.ratio,
                    bounds.bottom(),
                ),
            )
        } else {
            bounds
        };
        ui.scope_builder(egui::UiBuilder::new().max_rect(edit), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(edit));
            ui.spacing_mut().scroll.floating = false;
            ui.spacing_mut().scroll.bar_width = 12.;
            ui.spacing_mut().scroll.foreground_color = false;
            ui.visuals_mut().extreme_bg_color = Color32::from_gray(240);
            ui.visuals_mut().widgets.inactive.bg_fill = Color32::from_gray(180);
            ui.visuals_mut().widgets.hovered.bg_fill = Color32::from_gray(160);
            ui.visuals_mut().widgets.active.bg_fill = Color32::from_gray(140);
            ScrollArea::both()
                .id_salt("md-editor-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let digits = (text.lines().count() + 1).to_string().len();
                    let gutter = ui
                        .fonts_mut(|fonts| fonts.glyph_width(&egui::FontId::monospace(13.), '9'))
                        * digits as f32
                        + 12.;
                    let origin = ui.cursor().min;
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(origin.x, ui.clip_rect().top()),
                            egui::pos2(origin.x + gutter, ui.clip_rect().bottom()),
                        ),
                        0,
                        Color32::from_gray(221),
                    );
                    ui.add_space(0.);
                    let area = egui::Rect::from_min_max(
                        origin + egui::vec2(gutter, 0.),
                        ui.max_rect().max,
                    );
                    super::rectangle::before(ui.ctx(), editor_id, &mut state.rectangle, !tab.busy);
                    let selection_background = ui.painter().add(egui::Shape::Noop);
                    let output = ui
                        .scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
                            let mut layouter = super::highlight::layout;
                            let modifiers = ui.input(|input| input.modifiers);
                            let pointer_selection = state.rectangle.selection.is_none();
                            let mut edit_event =
                                |event: &egui::Event,
                                 text: &str,
                                 range: egui::text::CCursorRange| {
                                    super::rectangle::event_edit(
                                        event,
                                        text,
                                        range,
                                        &mut state.rectangle,
                                        &mut state.status,
                                        modifiers,
                                    )
                                };
                            TextEdit::multiline(text)
                                .id(editor_id)
                                .font(egui::FontId::monospace(13.))
                                .code_editor()
                                .frame(egui::Frame::NONE)
                                .margin(egui::Margin::ZERO)
                                .desired_width(edit.width() - gutter - 12.)
                                .desired_rows(40)
                                .layouter(&mut layouter)
                                .event_editor(&mut edit_event, 32 * 1024 * 1024)
                                .pointer_selection(pointer_selection)
                                .interactive(!tab.busy)
                                .show(ui)
                        })
                        .inner;
                    if output.response.changed() {
                        state.rectangle.selection = None;
                        state.changed(now);
                    }
                    super::rectangle::after(
                        ui,
                        &output,
                        &mut state.rectangle,
                        selection_background,
                    );
                    ui.set_min_width(gutter + output.galley.size().x + 12.);
                    if state.reveal_match
                        && let Some(range) = output.state.cursor.char_range()
                    {
                        let rect = output
                            .galley
                            .pos_from_cursor(range.primary)
                            .translate(output.galley_pos.to_vec2());
                        ui.scroll_to_rect(rect, Some(egui::Align::Center));
                        state.reveal_match = false;
                    }
                    let mut line = 1;
                    let mut new_line = true;
                    for row in &output.galley.rows {
                        let y = output.galley_pos.y + row.pos.y;
                        if new_line
                            && y + row.size.y >= ui.clip_rect().top()
                            && y <= ui.clip_rect().bottom()
                        {
                            super::highlight::line_number(
                                ui,
                                line,
                                egui::pos2(origin.x + gutter - 8., y),
                                row.glyphs.first().map(|glyph| glyph.pos.y),
                                13.,
                                Color32::from_gray(153),
                            );
                        }
                        new_line = row.ends_with_newline;
                        if new_line {
                            line += 1;
                        }
                    }
                });
        });
        if state.mode == Mode::Split {
            let divider = egui::Rect::from_min_size(
                egui::pos2(edit.right(), bounds.top()),
                egui::vec2(3., bounds.height()),
            );
            ui.painter()
                .rect_filled(divider, 0, Color32::from_gray(229));
            let response = ui
                .interact(
                    divider,
                    ui.make_persistent_id("md-split"),
                    egui::Sense::drag(),
                )
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            if response.dragged() {
                state.ratio =
                    (state.ratio + response.drag_delta().x / bounds.width()).clamp(0.1, 0.9);
            }
        }
    }
    if state.mode != Mode::Edit {
        let preview_rect = if state.mode == Mode::Split {
            egui::Rect::from_min_max(
                egui::pos2(
                    bounds.left() + bounds.width() * state.ratio + 3.,
                    bounds.top(),
                ),
                bounds.max,
            )
        } else {
            bounds
        };
        ui.scope_builder(egui::UiBuilder::new().max_rect(preview_rect), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(preview_rect));
            image = preview::render(ui, state);
        });
    }
    ui.advance_cursor_after_rect(bounds);
    if intent.is_none()
        && !tab.busy
        && !state.pending
        && state.parsed != Some(state.revision)
        && state.mode != Mode::Edit
    {
        let delay = (state.changed + 0.25 - now).max(0.);
        if delay == 0. || state.parsed.is_none() {
            state.pending = true;
            intent = Some(Intent::Markdown {
                source: text.clone(),
                revision: state.revision,
            });
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(delay));
        }
    }
    if let Some(source) = image {
        if intent.is_none() {
            intent = Some(Intent::MarkdownImage(source));
        } else {
            state.images.remove(&source);
        }
    }
    crate::tabs::markdown::trim_undo(ui.ctx(), editor_id);
    intent
}
