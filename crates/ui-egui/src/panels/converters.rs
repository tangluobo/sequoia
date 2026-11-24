use super::*;
use crate::icons::Icons;
pub(super) fn render(
    ui: &mut Ui,
    tool: &mut ToolState,
    busy: bool,
    icons: &mut Icons,
) -> Option<Intent> {
    let dataset = tool.kind == "dataset_converter";
    super::tools::header(
        ui,
        icons,
        &tool.kind,
        if dataset {
            "数据集格式转换"
        } else {
            "图片格式转换"
        },
        if dataset {
            "PASCAL VOC → COCO"
        } else {
            "SVG → PNG"
        },
    );
    let state = tool.converter.get_or_insert_with(Default::default);
    let mut intent = None;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 25,
            right: 25,
            top: 20,
            bottom: 25,
        })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 20.;
            section(ui, "转换类型", |ui| {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 41.),
                    egui::Sense::hover(),
                );
                ui.painter().rect(
                    rect,
                    4,
                    Color32::from_rgb(248, 249, 250),
                    egui::Stroke::new(1., Color32::from_gray(224)),
                    egui::StrokeKind::Inside,
                );
                super::tools::place_left(
                    ui,
                    rect.shrink2(egui::vec2(16., 11.)),
                    egui::Label::new(
                        RichText::new(if dataset {
                            "PASCAL VOC (XML) → COCO (JSON)"
                        } else {
                            "SVG → PNG"
                        })
                        .size(14.)
                        .color(Color32::from_gray(85)),
                    ),
                );
            });
            section(
                ui,
                if dataset {
                    "源数据集目录"
                } else {
                    "选择源文件/目录"
                },
                |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.;
                        let width =
                            (ui.available_width() - if dataset { 66. } else { 158. }).max(40.);
                        let response = ui.add_sized(
                            [width, 32.],
                            edit(
                                &mut tool.source,
                                if dataset {
                                    "选择包含 VOC XML 标注文件的目录"
                                } else {
                                    "选择 SVG 文件或目录"
                                },
                            ),
                        );
                        if state.focus_source && ui.is_enabled() {
                            response.request_focus();
                            let mut edit_state =
                                TextEdit::load_state(ui.ctx(), response.id).unwrap_or_default();
                            edit_state
                                .cursor
                                .set_char_range(Some(egui::text::CCursorRange::two(
                                    egui::text::CCursor::new(0),
                                    egui::text::CCursor::new(tool.source.chars().count()),
                                )));
                            TextEdit::store_state(ui.ctx(), response.id, edit_state);
                            state.focus_source = false;
                        }
                        if !dataset
                            && ui.add_sized([84., 31.], button("选择文件", 13.)).clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .set_title("选择SVG文件")
                                .add_filter("SVG文件", &["svg"])
                                .pick_file()
                        {
                            tool.source = path.display().to_string();
                        }
                        if ui.add_sized([58., 31.], button("浏览", 13.)).clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .set_title(if dataset {
                                    "选择源数据集目录"
                                } else {
                                    "选择SVG文件目录"
                                })
                                .pick_folder()
                        {
                            tool.source = path.display().to_string();
                        }
                    });
                },
            );
            section(
                ui,
                if dataset {
                    "目标数据集目录"
                } else {
                    "输出目录"
                },
                |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.;
                        ui.add_sized(
                            [(ui.available_width() - 66.).max(40.), 32.],
                            edit(
                                &mut tool.destination,
                                if dataset {
                                    "选择 COCO JSON 输出目录"
                                } else {
                                    "选择 PNG 输出目录"
                                },
                            ),
                        );
                        if ui.add_sized([58., 31.], button("浏览", 13.)).clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .set_title(if dataset {
                                    "选择目标数据集目录"
                                } else {
                                    "选择输出目录"
                                })
                                .pick_folder()
                        {
                            tool.destination = path.display().to_string();
                        }
                    });
                },
            );
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 15.;
                if ui
                    .add_enabled(
                        !busy && !state.running(),
                        button("开始转换", 14.).min_size(egui::vec2(116., 36.)),
                    )
                    .clicked()
                {
                    intent = state
                        .start(dataset, &tool.source, &tool.destination)
                        .map(Intent::command);
                }
                if !state.status.is_empty() {
                    ui.add(
                        egui::Label::new(RichText::new(&state.status).size(13.).color(
                            if state.error {
                                Color32::from_rgb(229, 57, 53)
                            } else if state.running() {
                                Color32::from_rgb(25, 118, 210)
                            } else {
                                Color32::from_rgb(56, 142, 60)
                            },
                        ))
                        .wrap(),
                    );
                }
            });
        });
    intent
}
fn edit<'a>(text: &'a mut String, hint: &'a str) -> TextEdit<'a> {
    TextEdit::singleline(text)
        .hint_text(hint)
        .font(egui::FontId::proportional(13.))
        .margin(egui::vec2(10., 6.))
}
fn button(label: &str, size: f32) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).size(size).color(Color32::WHITE))
        .fill(Color32::from_rgb(76, 175, 80))
        .stroke(egui::Stroke::NONE)
        .corner_radius(4)
}
fn section(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 8.;
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.), egui::Sense::hover());
        super::tools::place_left(
            ui,
            rect,
            egui::Label::new(
                RichText::new(title)
                    .size(15.)
                    .family(egui::FontFamily::Name("md-bold".into()))
                    .color(Color32::from_gray(51)),
            ),
        );
        body(ui);
    });
}
