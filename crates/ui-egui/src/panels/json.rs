use super::*;
mod history;
mod output;
use crate::icons::Icons;
pub(super) use output::viewer;

pub(super) fn render(
    ui: &mut Ui,
    tool: &mut ToolState,
    busy: bool,
    icons: &mut Icons,
) -> Option<Intent> {
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    super::tools::header(
        ui,
        icons,
        &tool.kind,
        "JSON处理工具",
        "格式化、压缩、编码转换、转义处理",
    );
    let mut intent = None;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 8,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.spacing_mut().button_padding = egui::vec2(14., 6.);
            ui.spacing_mut().item_spacing.x = 10.;
            ui.horizontal_wrapped(|ui| {
                for (action, label) in [
                    ("format", "格式化"),
                    ("minify", "压缩"),
                    ("unicode", "中文转Unicode"),
                    ("decode_unicode", "Unicode转中文"),
                    ("escape", "添加转义"),
                    ("unescape", "去除转义"),
                ] {
                    if ui
                        .add_enabled(
                            !busy,
                            egui::Button::new(RichText::new(label).size(13.).color(Color32::WHITE))
                                .fill(Color32::from_rgb(76, 175, 80))
                                .stroke(egui::Stroke::NONE)
                                .corner_radius(4)
                                .min_size(egui::vec2(0., 31.)),
                        )
                        .clicked()
                    {
                        if tool.input.trim_matches(|c: char| c <= ' ').is_empty() {
                            tool.json_status.error(match action {
                                "format" | "minify" => "请输入JSON数据",
                                "unicode" => "请输入要转换的文本",
                                "decode_unicode" => "请输入要转换的Unicode文本",
                                "escape" => "请输入要添加转义的文本",
                                _ => "请输入要去除转义的文本",
                            });
                        } else if tool.input.len() > 8 * 1024 * 1024 {
                            tool.json_status
                                .error("输入超过 8 MiB，请使用文件或 CLI 分批处理");
                        } else {
                            intent = Some(Intent::command(if action == "format" {
                                Command::JsonHistory {
                                    path: tool.history_path.clone(),
                                    action: tomato_services::json_history::Action::Format {
                                        json: tool.input.clone(),
                                    },
                                }
                            } else {
                                Command::Json {
                                    text: tool.input.clone(),
                                    action: action.into(),
                                }
                            }));
                        }
                    }
                }
            });
        });
    let (separator, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.), egui::Sense::hover());
    ui.painter().hline(
        separator.x_range(),
        separator.top(),
        egui::Stroke::new(1., Color32::from_gray(221)),
    );
    let status_height = ui.fonts_mut(|fonts| fonts.row_height(&egui::FontId::proportional(13.)));
    let height = ui.available_height().max(100.0) - status_height - 15.;
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
            ui.cursor().min,
            egui::vec2(ui.available_width(), height),
        )),
        |ui| {
            egui::Panel::left("json-history")
                .default_size(180.0)
                .min_size(150.0)
                .max_size(400.0)
                .resizable(true)
                .frame(egui::Frame::NONE.fill(Color32::from_gray(250)))
                .show(ui, |ui| {
                    if let Some(command) = history::render(ui, tool, busy) {
                        intent = Some(Intent::command(command));
                    }
                });
            let width = ui.available_width();
            egui::Panel::left("json-input-panel")
                .default_size(width * 0.5)
                .min_size(150.0)
                .resizable(true)
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    ui.label(RichText::new("JSON输入").size(14.0).strong());
                    let reset = std::mem::take(&mut tool.json_reset_input_cursor);
                    let mut scroll = ScrollArea::both()
                        .id_salt("json-input-scroll")
                        .auto_shrink([false, false]);
                    if reset {
                        scroll = scroll.scroll_offset(egui::Vec2::ZERO);
                    }
                    scroll.show(ui, |ui| {
                        let id = ui.make_persistent_id("json-input");
                        if reset {
                            // TextArea.setText in the original also discards the previous undo buffer.
                            let mut state = egui::text_edit::TextEditState::default();
                            state
                                .cursor
                                .set_char_range(Some(egui::text::CCursorRange::one(
                                    egui::text::CCursor::new(0),
                                )));
                            TextEdit::store_state(ui.ctx(), id, state);
                        }
                        let mut galley = ui.fonts_mut(|fonts| {
                            fonts.layout_no_wrap(
                                tool.input.clone(),
                                egui::FontId::monospace(13.),
                                Color32::from_gray(51),
                            )
                        });
                        // Give the editor its document width. Constraining it to the
                        // viewport makes egui shrink/offset a no-wrap galley internally,
                        // so the outer horizontal scrollbar never sees its real extent.
                        let size = egui::vec2(
                            ui.available_width().max(galley.size().x + 16.),
                            ui.available_height().max(galley.size().y + 8.),
                        );
                        let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, _: f32| {
                            if text.as_str() != galley.job.text.as_str() {
                                galley = ui.fonts_mut(|fonts| {
                                    fonts.layout_no_wrap(
                                        text.as_str().to_owned(),
                                        egui::FontId::monospace(13.),
                                        Color32::from_gray(51),
                                    )
                                });
                            }
                            galley.clone()
                        };
                        ui.add_sized(
                            size,
                            TextEdit::multiline(&mut tool.input)
                                .id(id)
                                .cursor_at_end(false)
                                .code_editor()
                                .font(egui::FontId::monospace(13.))
                                .layouter(&mut layouter)
                                .hint_text("请输入JSON数据")
                                .margin(egui::vec2(8., 4.))
                                .desired_width(size.x),
                        );
                    });
                });
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("JSON输出").size(14.0).strong());
                        for (label, color, action) in [
                            ("展开全部", Color32::from_rgb(255, 152, 0), 0),
                            ("折叠全部", Color32::from_rgb(156, 39, 176), 1),
                            ("复制全部", Color32::from_rgb(33, 150, 243), 2),
                        ] {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(label).size(12.0).color(Color32::WHITE),
                                    )
                                    .fill(color),
                                )
                                .clicked()
                            {
                                match action {
                                    0 | 1 => {
                                        if !tool.json_tree
                                            && let Ok(formatted) =
                                                tomato_core::tools::json_transform(
                                                    &tool.input,
                                                    "format",
                                                )
                                        {
                                            tool.set_json_tree(formatted);
                                        }
                                        tool.json_view.all(action == 1);
                                        if tool.json_tree {
                                            tool.json_status.success(if action == 1 {
                                                "已折叠所有节点"
                                            } else {
                                                "已展开所有节点"
                                            });
                                        } else {
                                            tool.json_status.error(if action == 1 {
                                                "没有可折叠的内容"
                                            } else {
                                                "没有可展开的内容"
                                            });
                                        }
                                    }
                                    _ => {
                                        if tool.output.is_empty() {
                                            tool.json_status.error("没有可复制的内容");
                                        } else {
                                            ui.ctx().copy_text(if tool.json_tree {
                                                tool.json_view.visible_text(&tool.output)
                                            } else {
                                                tool.output.clone()
                                            });
                                            tool.json_status.success("已复制到剪贴板！");
                                        }
                                    }
                                }
                            }
                        }
                    });
                    output::render(ui, tool);
                });
        },
    );
    if let Some(remaining) = tool.json_status.expire() {
        ui.ctx().request_repaint_after(remaining);
    }
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 5,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.set_min_height(status_height);
            ui.add(
                egui::Label::new(RichText::new(&tool.json_status.text).size(13.).color(
                    if tool.json_status.error {
                        Color32::from_rgb(229, 57, 53)
                    } else {
                        Color32::from_rgb(56, 142, 60)
                    },
                ))
                .truncate(),
            );
        });
    intent
}

fn highlight(line: &str) -> egui::text::LayoutJob {
    highlight_from(line, None)
}
fn highlight_from(
    line: &str,
    prefix: Option<crate::tabs::json::long_line::Prefix>,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let mut start = 0;
    if let Some(prefix) = prefix {
        let mut escaped = prefix.escaped;
        let mut end = line.len();
        for (index, ch) in line.char_indices() {
            if ch == '"' && !escaped {
                end = index + 1;
                break;
            }
            escaped = ch == '\\' && !escaped;
        }
        job.append(
            &line[..end],
            0.,
            egui::TextFormat {
                font_id: egui::FontId::monospace(12.),
                color: if prefix.key {
                    Color32::from_rgb(4, 81, 165)
                } else {
                    Color32::from_rgb(6, 125, 23)
                },
                ..Default::default()
            },
        );
        start = end;
    }
    while start < line.len() {
        let rest = &line[start..];
        let first = rest.chars().next().unwrap_or_default();
        let (len, color) = if first == '"' {
            let mut escaped = false;
            let mut end = rest.len();
            for (index, ch) in rest.char_indices().skip(1) {
                if ch == '"' && !escaped {
                    end = index + 1;
                    break;
                }
                escaped = ch == '\\' && !escaped;
            }
            (
                end,
                if rest[end..].trim_start().starts_with(':') {
                    Color32::from_rgb(4, 81, 165)
                } else {
                    Color32::from_rgb(6, 125, 23)
                },
            )
        } else if rest.starts_with("/*") {
            (rest.find("*/").map_or(rest.len(), |n| n + 2), Color32::GRAY)
        } else if first.is_ascii_digit() || first == '-' {
            (
                rest.find(|c: char| !c.is_ascii_digit() && !".eE+-".contains(c))
                    .unwrap_or(rest.len()),
                Color32::from_rgb(9, 134, 88),
            )
        } else if rest.starts_with("true") || rest.starts_with("false") {
            (if rest.starts_with("true") { 4 } else { 5 }, Color32::BLUE)
        } else if rest.starts_with("null") {
            (4, Color32::GRAY)
        } else {
            (first.len_utf8(), Color32::BLACK)
        };
        job.append(
            &rest[..len],
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::monospace(12.0),
                color,
                italics: rest.starts_with("/*"),
                ..Default::default()
            },
        );
        start += len;
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_input_starts_at_the_first_character_and_uses_outer_horizontal_scrolling() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tool = ToolState::new("json_tool");
        tool.input = format!("{{\"prefix\":\"{}\",\"suffix\":1}}", "x".repeat(2048));
        let mut icons = Icons::default();
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(896., 815.),
                    )),
                    ..Default::default()
                },
                |ui| {
                    render(ui, &mut tool, false, &mut icons);
                },
            );
            output.textures_delta.clear();
            let input = output
                .shapes
                .iter()
                .find_map(|clipped| {
                    if let egui::Shape::Text(text) = &clipped.shape
                        && text.galley.job.text == tool.input
                    {
                        Some((clipped.clip_rect, text))
                    } else {
                        None
                    }
                })
                .expect("input text must be rendered");
            assert!(
                input.1.galley.size().x > input.0.width(),
                "fixture must overflow horizontally"
            );
            assert!(
                input.1.pos.x >= input.0.left(),
                "opening/restoring text must keep its prefix visible"
            );
        }
    }
}
