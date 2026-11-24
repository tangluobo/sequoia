use super::*;
use egui::{Color32, Rect, RichText, Sense, Ui, vec2};
pub(super) fn show(ui: &mut Ui, state: &mut State, icons: &mut Icons) {
    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
    toolbar(ui, state, icons);
    let previous_tab = state.tab;
    crate::native_tabs::show(
        ui,
        "development-tabs",
        &mut state.tab,
        crate::native_tabs::Style::Compact,
        [(0, "目录"), (1, "版本"), (2, "依赖"), (3, "日志")],
    );
    if state.tab == 1 && previous_tab != 1 {
        state.refresh_versions(false);
    }
    match state.tab {
        0 | 1 => {
            let index = state.tab;
            let rect = ui.available_rect_before_wrap();
            let width =
                (rect.width() * state.divider[index]).clamp(140., (rect.width() - 160.).max(140.));
            let (left, _) = ui.allocate_exact_size(vec2(width, rect.height()), Sense::hover());
            let split = Rect::from_min_size(left.right_top(), vec2(1., left.height()));
            let response = ui.interact(
                split.expand2(vec2(2., 0.)),
                ui.id().with(("dev-divider", index)),
                Sense::drag(),
            );
            if response.dragged()
                && let Some(pos) = response.interact_pointer_pos()
            {
                state.divider[index] = ((pos.x - rect.left()) / rect.width()).clamp(0.15, 0.85);
            }
            ui.painter().vline(
                split.left(),
                split.y_range(),
                egui::Stroke::new(1., Color32::from_gray(229)),
            );
            response.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(left));
                if index == 0 {
                    files(ui, state, icons)
                } else {
                    versions(ui, state, icons)
                }
            });
            let right = Rect::from_min_max(split.right_top(), rect.max);
            ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(right));
                editor(ui, state);
            });
        }
        2 => {
            dependencies(ui, state);
        }
        _ => {
            egui::Frame::new().inner_margin(10).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.;
                ui.label(egui::RichText::new("项目日志").size(13.).strong());
                log(ui, state, false);
            });
        }
    }
}
fn dependencies(ui: &mut Ui, state: &mut State) {
    egui::Frame::new().inner_margin(10).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(8., 8.);
        let root = state.root();
        let project = state.project().cloned();
        let log = root.as_ref().and_then(|r| state.dependency_logs.get(r));
        let status = if project.is_none() {
            "请选择 Maven 项目"
        } else if !project.as_ref().is_some_and(|p| {
            p.project_type.starts_with("MAVEN") || p.project_type.starts_with("GRADLE")
        }) {
            "当前项目不是 Maven/Gradle 项目"
        } else if project
            .as_ref()
            .is_some_and(|p| p.jdk_id.is_empty() || p.maven_id.is_empty())
        {
            "请先在项目配置中选择 JDK 和构建工具"
        } else if log.is_some_and(|l| l.running) {
            "正在加载构建依赖..."
        } else if log.is_some_and(|l| l.exit_code.is_some_and(|code| code != 0)) {
            "依赖加载失败"
        } else if log.is_some_and(|l| !l.dependency_rows.is_empty()) {
            "依赖加载完成"
        } else {
            "依赖加载完成，当前项目没有外部依赖"
        };
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(status)
                    .size(11.)
                    .color(Color32::from_gray(136)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("重新加载").clicked()
                    && let Some(project) = project.clone()
                {
                    state.ordinary(Action::Dependencies {
                        config: Box::new(state.config.clone()),
                        project,
                    });
                }
            });
        });
        let Some(root) = root else {
            ui.label("暂无依赖");
            return;
        };
        let Some(log) = state.dependency_logs.get_mut(&root) else {
            ui.label(
                if project
                    .as_ref()
                    .is_some_and(|p| p.jdk_id.is_empty() || p.maven_id.is_empty())
                {
                    "尚未配置构建环境"
                } else {
                    "暂无依赖"
                },
            );
            return;
        };
        let project_name = project.as_ref().map_or("暂无依赖", |p| p.name.as_str());
        let mut visible = vec![None];
        let mut hidden_below = None;
        if !log.dependency_open.contains(&usize::MAX) {
            for (i, (depth, _)) in log.dependency_rows.iter().enumerate() {
                if hidden_below.is_some_and(|level| *depth > level) {
                    continue;
                }
                hidden_below = None;
                visible.push(Some(i));
                let children = log
                    .dependency_rows
                    .get(i + 1)
                    .is_some_and(|(next, _)| next > depth);
                if children && !log.dependency_open.contains(&i) {
                    hidden_below = Some(*depth);
                }
            }
        }
        let mut toggle = None;
        egui::ScrollArea::vertical()
            .id_salt("development-dependencies-tree")
            .auto_shrink([false, false])
            .show_rows(ui, 35., visible.len(), |ui, range| {
                for row in range {
                    let index = visible[row];
                    let (depth, label) = index.map_or((0, project_name), |i| {
                        (log.dependency_rows[i].0, log.dependency_rows[i].1.as_str())
                    });
                    let key = index.unwrap_or(usize::MAX);
                    let children = index.is_none()
                        || index.is_some_and(|i| {
                            log.dependency_rows
                                .get(i + 1)
                                .is_some_and(|(next, _)| *next > depth)
                        });
                    let open = if index.is_none() {
                        !log.dependency_open.contains(&key)
                    } else {
                        log.dependency_open.contains(&key)
                    };
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 35.), Sense::click());
                    let x = rect.left() + depth as f32 * 18.;
                    if children {
                        ui.painter().text(
                            egui::pos2(x + 8., rect.center().y),
                            egui::Align2::CENTER_CENTER,
                            if open { "▾" } else { "▸" },
                            crate::theme::java_font(12.),
                            Color32::from_gray(110),
                        );
                    }
                    ui.painter().text(
                        egui::pos2(x + 20., rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        label,
                        crate::theme::java_font(12.),
                        Color32::from_gray(51),
                    );
                    if response.clicked() && children {
                        toggle = Some(key)
                    }
                }
            });
        if let Some(index) = toggle
            && !log.dependency_open.remove(&index)
        {
            log.dependency_open.insert(index);
        }
    });
}
fn toolbar(ui: &mut Ui, state: &mut State, icons: &mut Icons) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 41.), Sense::hover());
    ui.painter()
        .rect_filled(rect, 0, Color32::from_rgb(247, 248, 250));
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1., Color32::from_gray(226)),
    );
    let profile_rect = Rect::from_min_size(rect.min + vec2(9., 7.), vec2(140., 27.));
    let stop_rect = Rect::from_min_size(rect.right_top() + vec2(-37., 7.), vec2(28., 27.));
    let start_rect = stop_rect.translate(vec2(-36., 0.));
    let run_rect = Rect::from_min_max(
        profile_rect.right_top() + vec2(8., 0.),
        start_rect.left_bottom() - vec2(8., 0.),
    );
    let profiles = state
        .overview
        .as_ref()
        .map(|o| o.profiles.clone())
        .unwrap_or_default();
    let count = state.project().map_or(0, |p| p.active_profiles.len());
    let label = if count == 0 {
        "Spring Profiles".into()
    } else {
        format!("Spring Profiles ({count})")
    };
    let enabled = state.project().is_some() && !state.config_busy() && !profiles.is_empty();
    let mut response = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(profile_rect), |ui| {
        if !enabled {
            ui.disable();
        }
        let control = ui.put(profile_rect, egui::Button::new("").corner_radius(0));
        ui.painter().text(
            profile_rect.left_center() + vec2(7., 0.),
            egui::Align2::LEFT_CENTER,
            label,
            crate::theme::java_font(12.),
            Color32::from_gray(51),
        );
        let p = profile_rect.right_center() - vec2(10., 0.);
        ui.painter().add(egui::Shape::convex_polygon(
            vec![p + vec2(-3., -1.5), p + vec2(3., -1.5), p + vec2(0., 2.)],
            Color32::from_gray(136),
            egui::Stroke::NONE,
        ));
        response = Some(control);
    });
    if let Some(response) = response {
        egui::Popup::menu(&response).show(|ui| {
            if let Some(index) = state.selected {
                for profile in profiles {
                    let mut checked = state.config.projects[index]
                        .active_profiles
                        .contains(&profile);
                    if ui.checkbox(&mut checked, &profile).changed() {
                        let mut config = state.config.clone();
                        if checked {
                            config.projects[index].active_profiles.push(profile)
                        } else {
                            config.projects[index]
                                .active_profiles
                                .retain(|p| p != &profile)
                        }
                        state.save_config(config, None, None);
                        ui.close();
                    }
                }
            }
        });
    }
    let mut chosen = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(run_rect), |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(run_rect));
        ui.spacing_mut().interact_size.y = 27.;
        ui.spacing_mut().button_padding = vec2(7., 4.);
        if state.config_busy() {
            ui.disable();
        }
        let current = state
            .run()
            .map(RunItem::label)
            .unwrap_or_else(|| "选择 Java main 或 npm script".into());
        egui::ComboBox::from_id_salt("development-run-item")
            .width(run_rect.width())
            .selected_text(current)
            .show_ui(ui, |ui| {
                if let Some(overview) = &state.overview {
                    for (run_index, run) in overview.runs.iter().enumerate() {
                        if ui
                            .selectable_label(state.selected_run == Some(run_index), run.label())
                            .clicked()
                        {
                            chosen = Some((run_index, run.clone()));
                        }
                    }
                }
            });
    });
    if let (Some(index), Some((run_index, run))) = (state.selected, chosen) {
        state.selected_run = Some(run_index);
        let mut config = state.config.clone();
        if run.kind == "JAVA" {
            config.projects[index].main_class = run.target.clone();
            config.projects[index].main_source_path = run.source.to_string_lossy().into_owned();
        }
        config.projects[index].run_type = run.kind;
        config.projects[index].run_target = run.target;
        state.save_config(config, None, None);
    }
    let root = state.root();
    let running = root
        .as_ref()
        .and_then(|r| state.logs.get(r))
        .is_some_and(|l| l.running);
    let enabled = state.run().is_some() && !state.config_busy();
    let hint = if running { "重启" } else { "启动" };
    let icon = if running {
        "images/connect/refresh.png"
    } else {
        "images/connect/execute.png"
    };
    let response = ui.interact(
        start_rect,
        ui.id().with("development-start"),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    ui.ctx().data_mut(|data| {
        data.insert_temp(egui::Id::new(("development-control", hint)), start_rect);
        data.insert_temp(egui::Id::new("development-start-clip"), ui.clip_rect());
    });
    icons.paint(
        ui,
        icon,
        Rect::from_center_size(start_rect.center(), vec2(16., 16.)),
    );
    if response.on_hover_text(hint).clicked() {
        #[cfg(test)]
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("development-start-clicked"), true));
        state.launch();
    }
    let stop = ui.interact(
        stop_rect,
        ui.id().with("development-stop"),
        if running {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new(("development-control", "停止")), stop_rect));
    let opacity = if running && ui.is_enabled() { 1. } else { 0.4 };
    ui.painter().rect_filled(
        Rect::from_center_size(stop_rect.center(), vec2(10., 10.)),
        0,
        Color32::from_rgb(211, 47, 47).gamma_multiply(opacity),
    );
    if stop.on_hover_text("停止").clicked()
        && let Some(root) = root
    {
        state.ordinary(Action::Stop { root });
    }
    ui.advance_cursor_after_rect(rect);
}
pub(super) fn icon(name: &str, directory: bool) -> String {
    if directory {
        return "images/connect/folder.png".into();
    }
    let lower = name.to_lowercase();
    let ext = if lower.starts_with("dockerfile") {
        "DOCKERFILE".into()
    } else if lower.starts_with("makefile") {
        "MAKEFILE".into()
    } else if [".gitignore", ".gitattributes", ".editorconfig"].contains(&lower.as_str()) {
        "CONF".into()
    } else {
        match lower.rsplit('.').next().unwrap_or("") {
            "yml" => "YAML".into(),
            "jpeg" | "jfif" => "JPG".into(),
            "htm" => "HTML".into(),
            "kts" => "KT".into(),
            "cc" | "cxx" => "CPP".into(),
            "hpp" | "hxx" => "H".into(),
            "bash" | "zsh" => "SH".into(),
            "properties" | "config" => "CONF".into(),
            "ai" => "Ai".into(),
            other => other.to_uppercase(),
        }
    };
    let path = format!("images/connect/fileTypes/{ext}.png");
    if crate::asset_bytes::bytes(&path).is_some() {
        path
    } else {
        "images/connect/fileTypes/TXT.png".into()
    }
}
fn flatten<'a>(
    path: &PathBuf,
    depth: usize,
    state: &'a State,
    result: &mut Vec<(&'a PathBuf, &'a str, bool, usize)>,
) {
    if let Some(entries) = state.entries.get(path) {
        for entry in entries {
            result.push((&entry.path, entry.name.as_str(), entry.directory, depth));
            if entry.directory && state.expanded.contains(&entry.path) {
                flatten(&entry.path, depth + 1, state, result);
            }
        }
    }
}
fn files(ui: &mut Ui, state: &mut State, icons: &mut Icons) {
    let Some(root) = state.root() else {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("请先从中间栏选择目录")
                    .size(12.)
                    .color(Color32::from_gray(153)),
            );
        });
        return;
    };
    let root_name = root.file_name().unwrap_or_default().to_string_lossy();
    let mut rows = vec![(&root, root_name.as_ref(), true, 0)];
    if state.expanded.contains(&root) {
        flatten(&root, 1, state, &mut rows)
    }
    let mut toggle = None;
    let mut selection = None;
    let mut open = None;
    let mut run = None;
    ui.spacing_mut().item_spacing.y = 0.;
    egui::ScrollArea::both()
        .id_salt("development-files")
        .auto_shrink([false, false])
        .show_rows(ui, 35., rows.len(), |ui, range| {
            for index in range {
                let (path, name, directory, depth) = &rows[index];
                let (rect, response) = ui
                    .allocate_exact_size(vec2(ui.available_width().max(100.), 35.), Sense::click());
                let selected = state
                    .file_selection
                    .as_ref()
                    .is_some_and(|selected| selected == *path);
                if selected || response.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        0,
                        if selected {
                            Color32::from_rgb(232, 244, 255)
                        } else {
                            Color32::from_gray(245)
                        },
                    );
                }
                let x = rect.left() + *depth as f32 * 12.;
                if *directory {
                    let center = egui::pos2(x + 9., rect.center().y);
                    disclosure(ui, center, state.expanded.contains(*path));
                    if ui
                        .interact(
                            Rect::from_center_size(center, vec2(18., 30.)),
                            ui.id().with(("file-arrow", *path)),
                            Sense::click(),
                        )
                        .clicked()
                    {
                        toggle = Some((*path).clone());
                    }
                }
                icons.paint(
                    ui,
                    &icon(name, *directory),
                    Rect::from_min_size(egui::pos2(x + 18., rect.center().y - 8.), vec2(16., 16.)),
                );
                let at = egui::pos2(x + 39., rect.center().y);
                if index == 0 {
                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        name,
                        0.,
                        egui::TextFormat {
                            font_id: crate::theme::java_font(12.),
                            color: Color32::from_rgb(0, 0, 160),
                            ..Default::default()
                        },
                    );
                    job.append(
                        &format!("   {}", root.display()),
                        0.,
                        egui::TextFormat {
                            font_id: crate::theme::java_font(12.),
                            color: Color32::BLACK,
                            ..Default::default()
                        },
                    );
                    let galley = ui.fonts_mut(|f| f.layout_job(job));
                    let height = galley.size().y;
                    ui.painter()
                        .galley(at - vec2(0., height / 2.), galley, Color32::BLACK);
                } else {
                    ui.painter().text(
                        at,
                        egui::Align2::LEFT_CENTER,
                        *name,
                        crate::theme::java_font(12.),
                        Color32::from_gray(51),
                    );
                }
                if response.clicked() {
                    selection = Some((*path).clone());
                    if !*directory {
                        open = Some((*path).clone());
                    }
                }
                if response.double_clicked() && *directory {
                    toggle = Some((*path).clone());
                }
                response
                    .on_hover_text(path.display().to_string())
                    .context_menu(|ui| {
                        menu_style(ui);
                        if let Some(item) = state.overview.as_ref().and_then(|o| {
                            o.runs
                                .iter()
                                .find(|r| r.kind == "JAVA" && root.join(&r.source) == **path)
                        }) && ui.button(format!("运行 {}.main()", item.label())).clicked()
                        {
                            run = Some(item.clone());
                            ui.close();
                        }
                    });
            }
        });
    if let Some(path) = selection {
        state.file_selection = Some(path);
    }
    if let Some(path) = toggle
        && !state.expanded.remove(&path)
    {
        state.expanded.insert(path.clone());
        if !state.entries.contains_key(&path) {
            state.submit(
                Action::List {
                    root: root.clone(),
                    path,
                },
                Request::List(state.generation),
            );
        }
    }
    if let Some(path) = open
        && state.document.as_ref().is_none_or(|d| d.path != path)
    {
        state.navigate(Navigation::File(path));
    }
    if let Some(item) = run
        && let Some(index) = state.selected
    {
        state.selected_run = state.overview.as_ref().and_then(|o| {
            o.runs.iter().position(|r| {
                r.kind == item.kind && r.target == item.target && r.source == item.source
            })
        });
        state.config.projects[index].run_type = item.kind;
        state.config.projects[index].run_target = item.target;
        state.launch();
    }
}
fn editor(ui: &mut Ui, state: &mut State) {
    let reading = state
        .pending
        .values()
        .any(|p| matches!(p, Request::Read { .. }));
    let mut save = false;
    crate::panels::ensure_bold_font(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.), Sense::hover());
    ui.painter()
        .rect_filled(rect, 0, Color32::from_rgb(247, 248, 250));
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1., Color32::from_gray(232)),
    );
    ui.painter().text(
        rect.left_center() + vec2(12., 0.),
        egui::Align2::LEFT_CENTER,
        "代码编辑器",
        egui::FontId::new(14., egui::FontFamily::Name("md-bold".into())),
        Color32::from_gray(51),
    );
    let label = state
        .document
        .as_ref()
        .map(|d| {
            format!(
                "{}{}{}",
                d.path.file_name().unwrap_or_default().to_string_lossy(),
                if d.read_only {
                    "  [已删除，只读]"
                } else {
                    ""
                },
                if d.text != d.saved { " *" } else { "" }
            )
        })
        .unwrap_or_else(|| "请选择目录中的文件".into());
    let label_rect = Rect::from_min_max(rect.min + vec2(92., 0.), rect.max - vec2(78., 0.));
    ui.painter().with_clip_rect(label_rect).text(
        label_rect.left_center(),
        egui::Align2::LEFT_CENTER,
        label,
        crate::theme::java_font(11.),
        Color32::from_gray(136),
    );
    let button = Rect::from_min_size(rect.right_center() - vec2(64., 13.), vec2(52., 26.));
    ui.scope_builder(egui::UiBuilder::new().max_rect(button), |ui| {
        ui.spacing_mut().interact_size.y = 26.;
        ui.spacing_mut().button_padding = vec2(14., 4.);
        save = ui
            .add_enabled(
                state.dirty() && !reading && state.document.as_ref().is_some_and(|d| !d.read_only),
                egui::Button::new(egui::RichText::new("保存").size(12.).color(Color32::WHITE))
                    .fill(Color32::from_rgb(25, 118, 210))
                    .corner_radius(4)
                    .min_size(button.size()),
            )
            .clicked();
    });
    ui.advance_cursor_after_rect(rect);
    if let Some(document) = &mut state.document {
        super::code::show(ui, document, reading);
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::S)) {
            save = true;
        }
    } else {
        super::code::empty(ui);
    }
    if save {
        state.save_file(None);
    }
}
fn log(ui: &mut Ui, state: &mut State, dependencies: bool) {
    ui.painter().rect_filled(
        ui.available_rect_before_wrap(),
        0,
        Color32::from_rgb(251, 251, 252),
    );
    let Some(root) = state.root() else { return };
    let map = if dependencies {
        &mut state.dependency_logs
    } else {
        &mut state.logs
    };
    let Some(log) = map.get_mut(&root) else {
        return;
    };
    super::log::show(ui, log, dependencies);
}
fn versions(ui: &mut Ui, state: &mut State, icons: &mut Icons) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 8,
            right: 12,
            top: 12,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(8., 10.);
            ui.label(
                RichText::new(
                    state
                        .versions
                        .as_ref()
                        .map_or("请先在左侧选择目录", |v| v.message.as_str()),
                )
                .size(12.)
                .line_height(Some(14.))
                .color(Color32::from_gray(136)),
            );
            let height = (ui.available_height() - 181.).max(35.);
            super::tree::show(ui, state, icons, height);
            let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.), Sense::hover());
            ui.painter().hline(
                line.x_range(),
                line.center().y,
                egui::Stroke::new(1., Color32::from_gray(207)),
            );
            ui.label(
                RichText::new("提交说明")
                    .size(12.)
                    .line_height(Some(17.))
                    .strong()
                    .color(Color32::from_rgb(71, 85, 105)),
            );
            ui.add_sized(
                vec2(ui.available_width(), 100.),
                egui::TextEdit::multiline(&mut state.commit_message)
                    .font(crate::theme::java_font(12.))
                    .hint_text("填写本次提交说明...")
                    .desired_rows(5),
            );
            let git = state.versions.as_ref().is_some_and(|v| v.git) && !state.realtime;
            let enabled = git
                && !state.selected_changes.is_empty()
                && !state.commit_message.trim().is_empty()
                && !state.pending.values().any(|r| matches!(r, Request::Git(_)));
            let (buttons, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 24.), Sense::hover());
            let commit = Rect::from_min_size(buttons.right_top() - vec2(124., 0.), vec2(40., 24.));
            let push = Rect::from_min_size(buttons.right_top() - vec2(76., 0.), vec2(76., 24.));
            let mut accepted = false;
            let mut publish = false;
            ui.scope_builder(egui::UiBuilder::new().max_rect(buttons), |ui| {
                if !enabled {
                    ui.disable();
                    ui.set_opacity(0.4);
                }
                ui.spacing_mut().interact_size.y = 24.;
                ui.spacing_mut().button_padding = vec2(8., 4.);
                accepted = ui
                    .put(
                        commit,
                        egui::Button::new(RichText::new("提交").size(12.).color(Color32::WHITE))
                            .fill(Color32::from_rgb(71, 85, 105))
                            .corner_radius(4)
                            .sense(if enabled {
                                Sense::click()
                            } else {
                                Sense::hover()
                            }),
                    )
                    .clicked()
                    && enabled;
                publish = ui
                    .put(
                        push,
                        egui::Button::new(
                            RichText::new("提交并推送").size(12.).color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(37, 99, 235))
                        .corner_radius(4)
                        .sense(if enabled {
                            Sense::click()
                        } else {
                            Sense::hover()
                        }),
                    )
                    .clicked()
                    && enabled;
            });
            ui.advance_cursor_after_rect(buttons);
            if accepted {
                commit_changes(state, None)
            }
            if publish {
                state.confirm = Some(Confirm::Push)
            }
        });
}
pub(super) fn commit_changes(state: &mut State, push: Option<service::Push>) {
    if let Some(root) = state.root() {
        state.ordinary(Action::Git {
            root,
            action: service::GitAction::Commit {
                files: state.selected_changes.iter().cloned().collect(),
                message: state.commit_message.clone(),
                push,
            },
        });
    }
}

pub(super) fn disclosure(ui: &Ui, center: egui::Pos2, open: bool) {
    let points = if open {
        vec![
            center + vec2(-5., -2.5),
            center + vec2(0., 2.5),
            center + vec2(5., -2.5),
        ]
    } else {
        vec![
            center + vec2(-2.5, -5.),
            center + vec2(2.5, 0.),
            center + vec2(-2.5, 5.),
        ]
    };
    ui.painter().add(egui::Shape::line(
        points,
        egui::Stroke::new(2., Color32::from_gray(136)),
    ));
}

pub(super) fn menu_style(ui: &mut Ui) {
    ui.spacing_mut().item_spacing.y = 0.;
    ui.spacing_mut().interact_size.y = 23.;
    ui.spacing_mut().button_padding = vec2(8., 3.);
    let font = crate::theme::java_font(12.);
    ui.style_mut()
        .text_styles
        .insert(egui::TextStyle::Body, font.clone());
    ui.style_mut()
        .text_styles
        .insert(egui::TextStyle::Button, font);
}
