use super::*;
use egui::{Color32, Rect, Sense, vec2};
pub(super) fn show(ui: &mut egui::Ui, state: &mut State, icons: &mut Icons) {
    crate::panels::ensure_bold_font(ui.ctx());
    let blank = ui.interact(
        ui.available_rect_before_wrap(),
        ui.id().with("dev-directories"),
        Sense::click(),
    );
    blank.context_menu(|ui| {
        view::menu_style(ui);
        if ui.button("添加目录").clicked() {
            state.add = Some(settings::DirectoryDraft::new());
            ui.close();
        }
    });
    if !state.loaded {
        ui.label("正在加载...");
        return;
    }
    if state.config.projects.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("暂无目录，右键点击添加")
                    .color(Color32::from_gray(153))
                    .size(12.),
            );
        });
        return;
    }
    let mut navigation = None;
    let mut settings = None;
    egui::ScrollArea::vertical()
        .id_salt("development-projects")
        .show_rows(ui, 61., state.config.projects.len(), |ui, range| {
            for index in range {
                let project = &state.config.projects[index];
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 61.), Sense::click());
                let selected = state.selected == Some(index);
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
                let icon = Rect::from_min_size(rect.min + vec2(12., 10.), vec2(40., 40.));
                ui.painter()
                    .rect_filled(icon, 8, Color32::from_rgb(232, 244, 255));
                icons.paint(
                    ui,
                    "images/development/folder.svg",
                    Rect::from_center_size(icon.center(), vec2(21.6, 21.6)),
                );
                let name = Rect::from_min_max(rect.min + vec2(62., 10.), rect.max - vec2(50., 30.));
                ui.painter().with_clip_rect(name).text(
                    name.left_center(),
                    egui::Align2::LEFT_CENTER,
                    &project.name,
                    egui::FontId::new(14., egui::FontFamily::Name("md-bold".into())),
                    Color32::from_gray(51),
                );
                let desc = Rect::from_min_max(rect.min + vec2(62., 32.), rect.max - vec2(50., 10.));
                let text = format!(
                    "{} · {}",
                    tomato_core::development::project_type_label(&project.project_type),
                    project.path
                );
                let text = middle_ellipsis(
                    ui,
                    &text,
                    crate::theme::java_font(11.),
                    desc.width().min(180.),
                );
                ui.painter().with_clip_rect(desc).text(
                    desc.left_center(),
                    egui::Align2::LEFT_CENTER,
                    text,
                    crate::theme::java_font(11.),
                    Color32::from_gray(136),
                );
                let running = state
                    .logs
                    .get(&PathBuf::from(&project.path))
                    .is_some_and(|l| l.running);
                let color = if running {
                    Color32::from_rgb(7, 193, 96)
                } else {
                    Color32::from_gray(170)
                };
                let width =
                    (rect.width() - 24. - 40. - 30. - desc.width().min(180.) - 13.).clamp(8., 30.);
                let text_rect = Rect::from_center_size(
                    rect.right_center() - vec2(12. + width / 2., 0.),
                    vec2(width, 14.),
                );
                ui.painter().text(
                    text_rect.left_center() - vec2(7.5, 0.),
                    egui::Align2::CENTER_CENTER,
                    "●",
                    crate::theme::java_font(10.),
                    color,
                );
                ui.painter().with_clip_rect(text_rect).text(
                    text_rect.left_center(),
                    egui::Align2::LEFT_CENTER,
                    if width < 24. {
                        "..."
                    } else if running {
                        "运行中"
                    } else {
                        "未运行"
                    },
                    crate::theme::java_font(10.),
                    color,
                );
                ui.painter().hline(
                    rect.x_range(),
                    rect.bottom() - 0.5,
                    egui::Stroke::new(1., Color32::from_gray(240)),
                );
                if response.clicked() {
                    navigation = Some(index)
                }
                response.on_hover_text(&project.path).context_menu(|ui| {
                    view::menu_style(ui);
                    if ui.button("配置").clicked() {
                        settings = Some(index);
                        ui.close();
                    }
                });
            }
        });
    if let Some(index) = navigation {
        state.navigate(Navigation::Project(index));
    }
    if let Some(index) = settings {
        state.settings = Some(settings::Draft::new(index, &state.config));
    }
}

fn middle_ellipsis(ui: &egui::Ui, text: &str, font: egui::FontId, width: f32) -> String {
    let fits = |value: &str| {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(value.into(), font.clone(), Color32::BLACK)
                .size()
                .x
        }) <= width
    };
    if fits(text) {
        return text.into();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut low, mut high) = (0, chars.len());
    let make = |count: usize| {
        let left = count.div_ceil(2);
        let right = count / 2;
        let mut value = chars[..left].iter().collect::<String>();
        value.push_str("...");
        value.extend(chars[chars.len() - right..].iter());
        value
    };
    while low < high {
        let mid = (low + high).div_ceil(2);
        if fits(&make(mid)) {
            low = mid
        } else {
            high = mid - 1
        }
    }
    make(low)
}
