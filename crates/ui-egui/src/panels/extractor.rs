use super::*;
use crate::tabs::extractor::{CATEGORIES, State};
use tomato_services::extractor::Mode;
mod results;
pub(super) fn render(
    ui: &mut Ui,
    tool: &mut ToolState,
    busy: bool,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    let view = tool
        .extractor
        .get_or_insert_with(|| Box::new(State::default()));
    let mut action = None;
    ui.spacing_mut().item_spacing = egui::vec2(10., 8.);
    ui.spacing_mut().interact_size.y = 22.;
    egui::Frame::new()
        .fill(Color32::from_rgb(247, 248, 250))
        .inner_margin(egui::Margin::symmetric(20, 14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(18., 18.), egui::Sense::hover());
                icons.paint(ui, "images/tools/extractor-header.svg", rect);
                ui.label(RichText::new("资源图标提取").size(16.).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("从二进制文件中提取嵌入资源")
                            .size(12.)
                            .color(Color32::from_gray(153)),
                    );
                });
            });
        });
    egui::Panel::bottom(ui.id().with("extractor-results"))
        .default_size(ui.available_height() * 0.3)
        .min_size(130.)
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            egui::Frame::new()
                .fill(Color32::from_gray(245))
                .inner_margin(egui::Margin::symmetric(15, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("扫描结果")
                                .size(13.)
                                .strong()
                                .color(Color32::from_gray(85)),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.checkbox(&mut view.show_log, "显示详细日志");
                        });
                    });
                });
            if view.show_log {
                egui::Panel::bottom(ui.id().with("extractor-log"))
                    .default_size(130.)
                    .min_size(70.)
                    .frame(
                        egui::Frame::new()
                            .fill(Color32::from_rgb(30, 30, 30))
                            .inner_margin(6),
                    )
                    .show(ui, |ui| {
                        ScrollArea::both()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if let Some(report) = &view.report {
                                    ui.label(
                                        RichText::new(&report.log)
                                            .monospace()
                                            .size(12.)
                                            .color(Color32::from_rgb(212, 212, 212)),
                                    );
                                }
                            });
                    });
            }
            results::render(ui, view);
        });
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 20,
                    right: 20,
                    top: 15,
                    bottom: 20,
                })
                .show(ui, |ui| {
                    ui.add_enabled_ui(!busy, |ui| {
                        heading(ui, "运行模式");
                        egui::ComboBox::from_id_salt("resource-mode")
                            .width(ui.available_width())
                            .selected_text(mode(&view.options.mode))
                            .show_ui(ui, |ui| {
                                for value in [Mode::Scan, Mode::Extract, Mode::PeOnly] {
                                    let label = mode(&value);
                                    ui.selectable_value(&mut view.options.mode, value, label);
                                }
                            });
                        ui.add_space(7.);
                        heading(ui, "源文件/目录");
                        ui.horizontal(|ui| {
                            ui.add(
                                TextEdit::singleline(&mut tool.source)
                                    .desired_width((ui.available_width() - 150.).max(80.))
                                    .hint_text("选择要扫描的文件或目录"),
                            );
                            if ui
                                .add(
                                    crate::theme::green_button("选择文件")
                                        .min_size(egui::vec2(80., 32.)),
                                )
                                .clicked()
                                && let Some(path) = rfd::FileDialog::new().pick_file()
                            {
                                tool.source = path.display().to_string();
                            }
                            if ui
                                .add(
                                    crate::theme::green_button("选择目录")
                                        .min_size(egui::vec2(80., 32.)),
                                )
                                .clicked()
                                && let Some(path) = rfd::FileDialog::new().pick_folder()
                            {
                                tool.source = path.display().to_string();
                            }
                        });
                        ui.add_space(7.);
                        heading(ui, "输出目录");
                        ui.horizontal(|ui| {
                            ui.add(
                                TextEdit::singleline(&mut tool.destination)
                                    .desired_width((ui.available_width() - 80.).max(80.))
                                    .hint_text("选择提取输出目录（可选）"),
                            );
                            if ui
                                .add(
                                    crate::theme::green_button("浏览")
                                        .min_size(egui::vec2(54., 32.)),
                                )
                                .clicked()
                                && let Some(path) = rfd::FileDialog::new().pick_folder()
                            {
                                tool.destination = path.display().to_string();
                            }
                        });
                        ui.add_space(7.);
                        heading(ui, "格式过滤");
                        ui.label(
                            RichText::new("勾选要包含的分类与具体格式（不勾选则扫描所有格式）")
                                .size(12.)
                                .color(Color32::from_gray(153)),
                        );
                        egui::Grid::new("resource-categories")
                            .num_columns(2)
                            .spacing([15., 5.])
                            .show(ui, |ui| {
                                ui.spacing_mut().interact_size.y = 14.;
                                ui.style_mut().override_font_id =
                                    Some(egui::FontId::proportional(12.));
                                for (index, (id, label, _)) in CATEGORIES.iter().enumerate() {
                                    let mut checked = view.categories.contains(*id);
                                    if ui.checkbox(&mut checked, *label).changed() {
                                        if checked {
                                            view.categories.insert((*id).into());
                                        } else {
                                            view.categories.remove(*id);
                                        }
                                    }
                                    if index % 2 == 1 {
                                        ui.end_row();
                                    }
                                }
                            });
                        egui::Frame::new()
                            .fill(Color32::from_rgb(248, 249, 250))
                            .inner_margin(5)
                            .corner_radius(4)
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                if view.categories.is_empty() {
                                    ui.label("（未选择任何分类，将扫描所有格式）");
                                }
                                for (id, label, formats) in CATEGORIES {
                                    if !view.categories.contains(*id) {
                                        continue;
                                    }
                                    ui.label(RichText::new(format!("{label}:")).size(11.).strong());
                                    ui.horizontal_wrapped(|ui| {
                                        for format in *formats {
                                            let mut selected = view.selected.contains(*format);
                                            if ui
                                                .checkbox(
                                                    &mut selected,
                                                    RichText::new(format.to_uppercase()).size(11.),
                                                )
                                                .changed()
                                            {
                                                if selected {
                                                    view.selected.insert((*format).into());
                                                } else {
                                                    view.selected.remove(*format);
                                                }
                                            }
                                        }
                                    });
                                }
                            });
                        ui.add_space(7.);
                        heading(ui, "高级选项");
                        egui::Grid::new("resource-options")
                            .num_columns(2)
                            .spacing([30., 8.])
                            .show(ui, |ui| {
                                ui.spacing_mut().interact_size.y = 14.;
                                ui.style_mut().override_font_id =
                                    Some(egui::FontId::proportional(12.));
                                ui.checkbox(&mut view.options.recursive, "递归扫描子目录");
                                ui.checkbox(&mut view.options.overwrite, "覆盖已存在的文件");
                                ui.end_row();
                                ui.checkbox(&mut view.options.unpack, "解包ZIP并扫描内容");
                                ui.checkbox(
                                    &mut view.options.pe_scan,
                                    "扫描PE资源段 (Qt/Electron必备)",
                                );
                                ui.end_row();
                                ui.checkbox(
                                    &mut view.options.pe_also,
                                    "同时提取PE资源 (图标/位图等)",
                                );
                                ui.checkbox(&mut view.options.dedup, "按源+偏移去重");
                                ui.end_row();
                                ui.checkbox(&mut view.options.verbose, "详细日志输出");
                                ui.end_row();
                            });
                        ui.horizontal_wrapped(|ui| {
                            ui.label("最小资源大小:");
                            ui.add(
                                egui::DragValue::new(&mut view.options.min_size)
                                    .range(16..=1024)
                                    .speed(16.),
                            );
                            ui.label("最大文件大小(MB):");
                            ui.add(
                                egui::DragValue::new(&mut view.options.max_file_mib)
                                    .range(64..=2048)
                                    .speed(64.),
                            );
                        });
                    });
                    ui.add_space(7.);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !busy,
                                egui::Button::new(
                                    RichText::new(if busy {
                                        "运行中..."
                                    } else {
                                        "开始扫描"
                                    })
                                    .size(14.)
                                    .color(Color32::WHITE),
                                )
                                .fill(Color32::from_rgb(25, 118, 210))
                                .min_size(egui::vec2(130., 36.)),
                            )
                            .clicked()
                        {
                            let options = view.request(&tool.source, &tool.destination);
                            if options.source.as_os_str().is_empty() {
                                view.status = "请选择源文件或目录".into();
                            } else if options.mode != Mode::Scan
                                && options.output.as_os_str().is_empty()
                            {
                                view.status = "请选择输出目录".into();
                            } else {
                                view.begin();
                                action =
                                    Some(Intent::command(Command::ExtractResources { options }));
                            }
                        }
                        ui.label(&view.status);
                    });
                });
        });
    action
}
fn heading(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).size(14.).strong());
}
fn mode(mode: &Mode) -> &'static str {
    match mode {
        Mode::Scan => "扫描 - 仅列出发现的资源",
        Mode::Extract => "提取 - 扫描并保存到输出目录",
        Mode::PeOnly => "PE提取 - 仅提取PE资源(EXE/DLL/SCR)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tomato_services::commands::{CommandHandler, Engine, Output};
    #[tokio::test]
    async fn actual_start_button_runs_scan_and_extract_and_publishes_the_real_report()
    -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("fixture.bin");
        let data = b"\x89PNG\r\n\x1a\nfixture payloadIEND\xae\x42\x60\x82";
        std::fs::write(&source, data)?;
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tool = ToolState::new("resource_extractor");
        tool.source = source.to_string_lossy().into_owned();
        tool.destination = root.path().join("out").to_string_lossy().into_owned();
        let mut icons = crate::icons::Icons::default();
        let mut engine = Engine::default();
        for mode in [Mode::Scan, Mode::Extract] {
            tool.extractor
                .get_or_insert_with(Default::default)
                .options
                .mode = mode.clone();
            let mut frame = |events| {
                let mut intent = None;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1200., 2000.),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        intent = render(ui, &mut tool, false, &mut icons);
                    },
                );
                let point = output.shapes.iter().find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == "开始扫描" => {
                        Some(text.pos + text.galley.size() / 2.)
                    }
                    _ => None,
                });
                output.textures_delta.clear();
                (intent, point)
            };
            frame(Vec::new());
            let (_, point) = frame(Vec::new());
            let point = point.expect("start button must be visible in tall test viewport");
            frame(vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            let (intent, _) = frame(vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            let Some(Intent::Command(command)) = intent else {
                panic!("actual start button did not produce a command");
            };
            assert!(
                matches!(&*command,Command::ExtractResources {options} if options.mode==mode && options.source==source)
            );
            let Output::Resources(report) = engine.execute(*command).await? else {
                panic!("wrong backend output");
            };
            assert_eq!(report.errors, 0);
            assert_eq!(report.total, 1);
            if mode == Mode::Extract {
                assert_eq!(report.written, 1);
                assert_eq!(
                    std::fs::read(report.hits[0].output.as_ref().unwrap())?,
                    data
                );
            }
            tool.extractor.as_mut().unwrap().accept(report);
            assert!(tool.extractor.as_ref().unwrap().report.is_some());
        }
        Ok(())
    }
}
