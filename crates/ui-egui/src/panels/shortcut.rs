use super::*;
use tomato_services::tools::shortcut::CATEGORIES;

pub(super) fn render(
    ui: &mut Ui,
    tool: &mut ToolState,
    busy: bool,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    ui.spacing_mut().button_padding = egui::vec2(6., 3.);
    super::tools::header(
        ui,
        icons,
        "desktop_shortcut",
        "桌面快捷方式",
        "创建 Linux .desktop 快捷方式",
    );
    let state = tool.shortcut.get_or_insert_with(Default::default);
    let (strip, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 37.), egui::Sense::hover());
    ui.painter()
        .rect_filled(strip, 0, Color32::from_rgb(236, 236, 234));
    let mut x = strip.left();
    for (i, (label, width)) in [("基础设置", 73.), ("高级设置", 73.), ("预览", 49.)]
        .into_iter()
        .enumerate()
    {
        let rect = egui::Rect::from_min_size(egui::pos2(x, strip.top()), egui::vec2(width, 37.));
        let response = ui.place(
            rect,
            egui::Button::new(RichText::new(label).size(12.))
                .corner_radius(0)
                .fill(if state.tab == i {
                    Color32::WHITE
                } else {
                    Color32::from_gray(222)
                }),
        );
        if response.clicked() {
            state.tab = i;
        }
        x += width;
    }
    let mut intent = None;
    egui::Panel::bottom(ui.id().with("shortcut-actions"))
        .exact_size(94.)
        .resizable(false)
        .frame(egui::Frame::NONE.fill(Color32::from_rgb(247, 248, 250)))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            ui.painter().hline(
                rect.x_range(),
                rect.top(),
                egui::Stroke::new(1., Color32::from_gray(232)),
            );
            let mut x = rect.center().x - 217.;
            for (i, (label, width, color)) in [
                ("刷新预览", 116., Color32::from_rgb(33, 150, 243)),
                ("复制到剪贴板", 144., Color32::from_rgb(156, 39, 176)),
                ("创建快捷方式", 144., Color32::from_rgb(76, 175, 80)),
            ]
            .into_iter()
            .enumerate()
            {
                let button = egui::Rect::from_min_size(
                    egui::pos2(x, rect.top() + 16.),
                    egui::vec2(width, 36.),
                );
                let response = ui
                    .add_enabled_ui(!busy, |ui| {
                        ui.place(button, colored_button(label, color, 14.))
                    })
                    .inner;
                if response.clicked() {
                    match i {
                        0 => tool.output = "预览已更新".into(),
                        1 => match state.fields.preview() {
                            Ok(text) => {
                                ui.ctx().copy_text(text);
                                tool.output = "已复制到剪贴板".into();
                            }
                            Err(e) => tool.output = e.to_string(),
                        },
                        _ => {
                            let name = if state.file_name.trim().is_empty() {
                                state.fields.file_name()
                            } else if state.file_name.ends_with(".desktop") {
                                state.file_name.clone()
                            } else {
                                format!("{}.desktop", state.file_name)
                            };
                            if [
                                &state.fields.name,
                                &state.fields.executable,
                                &state.fields.icon,
                                &state.fields.wm_class,
                            ]
                            .iter()
                            .any(|v| v.trim().is_empty())
                            {
                                tool.output = "请填写应用名称、执行路径、图标文件和窗口类名".into();
                            } else if std::path::Path::new(&name)
                                .file_name()
                                .is_some_and(|n| n == name.as_str())
                            {
                                intent = Some(Intent::command(Command::SaveDesktopShortcut {
                                    destination: PathBuf::from(&state.directory).join(name),
                                    shortcut: Box::new(state.fields.clone()),
                                }));
                            } else {
                                tool.output = "文件名不能包含目录".into();
                            }
                        }
                    }
                }
                x += width + 15.;
            }
            if !tool.output.is_empty() {
                ui.place(
                    egui::Rect::from_min_max(
                        rect.min + egui::vec2(25., 62.),
                        rect.max - egui::vec2(25., 10.),
                    ),
                    egui::Label::new(RichText::new(&tool.output).size(13.)).wrap(),
                );
            }
        });
    let body = ui.available_rect_before_wrap();
    ui.painter().rect_filled(body, 0, Color32::from_gray(244));
    if state.tab == 2 {
        let (title, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.), egui::Sense::hover());
        ui.painter().rect_filled(title, 0, Color32::WHITE);
        ui.painter().text(
            title.min,
            egui::Align2::LEFT_TOP,
            ".desktop 文件预览",
            egui::FontId::new(14., egui::FontFamily::Name("md-bold".into())),
            Color32::BLACK,
        );
        let text = state.fields.preview().unwrap_or_else(|e| e.to_string());
        let mut view = text.as_str();
        let edit = ui.available_rect_before_wrap().shrink(10.);
        ui.place(
            edit,
            TextEdit::multiline(&mut view)
                .font(egui::FontId::monospace(12.))
                .desired_width(f32::INFINITY)
                .margin(8),
        );
        return intent;
    }
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(25, 15))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 10.;
                    if state.tab == 0 {
                        text_field(
                            ui,
                            "应用名称 *",
                            &mut state.fields.name,
                            "请输入应用名称",
                            None,
                        );
                        text_field(
                            ui,
                            "执行路径 *",
                            &mut state.fields.executable,
                            "/usr/bin/app 或 /home/user/app",
                            Some(false),
                        );
                        text_field(
                            ui,
                            "图标文件 *",
                            &mut state.fields.icon,
                            "/usr/share/icons/hicolor/.../app.png",
                            Some(false),
                        );
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 25.),
                            egui::Sense::hover(),
                        );
                        label(ui, rect, "保存目录 *");
                        let mut input = field_rect(rect);
                        input.max.x -= 169.;
                        edit(ui, input, &mut state.directory, "保存位置");
                        let mut x = input.right() + 6.;
                        for (i, (caption, width, color)) in [
                            ("浏览...", 53., Color32::from_rgb(33, 150, 243)),
                            ("用户目录", 60., Color32::from_rgb(76, 175, 80)),
                            ("桌面", 38., Color32::from_rgb(255, 152, 0)),
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            if ui
                                .place(
                                    egui::Rect::from_min_size(
                                        egui::pos2(x, rect.top()),
                                        egui::vec2(width, 24.),
                                    ),
                                    colored_button(caption, color, 12.),
                                )
                                .clicked()
                            {
                                match i {
                                    0 => {
                                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                            state.directory = path.display().to_string();
                                        }
                                    }
                                    1 => {
                                        state.directory = home()
                                            .join(".local/share/applications")
                                            .display()
                                            .to_string()
                                    }
                                    _ => {
                                        state.directory =
                                            home().join("Desktop").display().to_string()
                                    }
                                }
                            }
                            x += width + 6.;
                        }
                        text_field(
                            ui,
                            "窗口类名 *",
                            &mut state.fields.wm_class,
                            "用于任务栏图标匹配，如：jetbrains-idea",
                            None,
                        );
                    } else {
                        text_field(
                            ui,
                            "通用名称",
                            &mut state.fields.generic_name,
                            "可选，如：文本编辑器",
                            None,
                        );
                        text_field(
                            ui,
                            "应用描述",
                            &mut state.fields.comment,
                            "简短描述应用功能",
                            None,
                        );
                        text_field(
                            ui,
                            "执行参数",
                            &mut state.fields.arguments,
                            "可选，如：--config /path/to/config",
                            None,
                        );
                        text_field(
                            ui,
                            "工作目录",
                            &mut state.fields.working_directory,
                            "可选，应用启动时的工作目录",
                            Some(true),
                        );
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 50.),
                            egui::Sense::hover(),
                        );
                        label(ui, rect, "文件名");
                        let mut field = field_rect(rect);
                        field.max.y = field.top() + 25.;
                        edit(ui, field, &mut state.file_name, "自动根据应用名称生成");
                        ui.painter().text(
                            field.min + egui::vec2(0., 28.),
                            egui::Align2::LEFT_TOP,
                            format!(
                                "将生成: {}",
                                if state.file_name.is_empty() {
                                    state.fields.file_name()
                                } else if state.file_name.ends_with(".desktop") {
                                    state.file_name.clone()
                                } else {
                                    format!("{}.desktop", state.file_name)
                                }
                            ),
                            egui::FontId::proportional(12.),
                            Color32::from_gray(102),
                        );
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 55.),
                            egui::Sense::hover(),
                        );
                        label(ui, rect, "应用分类");
                        let mut field = field_rect(rect);
                        field.max.y = field.top() + 25.;
                        ui.scope_builder(egui::UiBuilder::new().max_rect(field), |ui| {
                            ui.spacing_mut().interact_size.y = 25.;
                            egui::ComboBox::from_id_salt("shortcut-category")
                                .width(field.width())
                                .selected_text(
                                    CATEGORIES
                                        .iter()
                                        .find(|(_, id)| *id == state.fields.category)
                                        .map_or("", |(label, _)| *label),
                                )
                                .show_ui(ui, |ui| {
                                    for &(label, id) in CATEGORIES {
                                        ui.selectable_value(
                                            &mut state.fields.category,
                                            id.to_owned(),
                                            label,
                                        );
                                    }
                                });
                        });
                        field = field.translate(egui::vec2(0., 30.));
                        edit(
                            ui,
                            field,
                            &mut state.fields.custom_categories,
                            "如：Utility;Development",
                        );
                        ui.advance_cursor_after_rect(rect);
                        for (caption, text, value) in [
                            (
                                "终端模式",
                                "在终端中运行（适用于命令行工具）",
                                &mut state.fields.terminal,
                            ),
                            ("通知支持", "支持应用通知", &mut state.fields.notify),
                        ] {
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 15.),
                                egui::Sense::hover(),
                            );
                            label(ui, rect, caption);
                            super::tools::place_left(
                                ui,
                                field_rect(rect),
                                egui::Checkbox::new(value, RichText::new(text).size(12.)),
                            );
                        }
                    }
                });
        });
    intent
}
fn colored_button(label: &str, color: Color32, size: f32) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).size(size).color(Color32::WHITE))
        .fill(color)
        .stroke(egui::Stroke::NONE)
        .corner_radius(4)
        .wrap_mode(egui::TextWrapMode::Extend)
}
fn label(ui: &Ui, rect: egui::Rect, text: &str) {
    ui.painter().text(
        egui::pos2(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(12.),
        Color32::from_gray(85),
    );
}
fn field_rect(rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(rect.min + egui::vec2(102., 0.), rect.max)
}
fn edit(ui: &mut Ui, rect: egui::Rect, value: &mut String, hint: &str) {
    ui.place(
        rect,
        TextEdit::singleline(value)
            .hint_text(hint)
            .font(egui::FontId::proportional(12.))
            .margin(egui::Margin::symmetric(8, 4)),
    );
}
fn text_field(ui: &mut Ui, caption: &str, value: &mut String, hint: &str, browse: Option<bool>) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 25.), egui::Sense::hover());
    label(ui, rect, caption);
    let mut field = field_rect(rect);
    if let Some(directory) = browse {
        field.max.x -= 61.;
        if ui
            .place(
                egui::Rect::from_min_size(
                    egui::pos2(field.right() + 8., rect.top()),
                    egui::vec2(53., 24.),
                ),
                colored_button("浏览...", Color32::from_rgb(33, 150, 243), 12.),
            )
            .clicked()
        {
            let dialog = rfd::FileDialog::new();
            let selected = if directory {
                dialog.pick_folder()
            } else {
                dialog.pick_file()
            };
            if let Some(path) = selected {
                *value = path.display().to_string();
            }
        }
    }
    edit(ui, field, value, hint);
}
fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}
