//! The original SQL editor menu. Clipboard access is requested only by user action.
use egui::{Color32, Event, Response, text::CCursorRange};

// JavaFX SqlEditorPane's Modena menu, excluding its shadow.
const CONTENT_WIDTH: f32 = 82.;
const ITEM_HEIGHT: f32 = 23.;
const SEPARATOR_HEIGHT: f32 = 3.;
const TEXT_INSET: f32 = 5.;
pub(super) enum Action {
    Event(Event),
    SelectAll,
    Run,
    Ai,
}
pub(super) fn show(
    response: &Response,
    text: &str,
    range: CCursorRange,
    enabled: bool,
) -> Option<Action> {
    let sorted = range.as_sorted_char_range();
    let byte = |index| {
        text.char_indices()
            .nth(index)
            .map_or(text.len(), |(index, _)| index)
    };
    let selected = &text[byte(sorted.start.into())..byte(sorted.end.into())];
    let no_selection = tomato_core::ai_sql::is_blank(selected);
    let mut action = None;
    egui::Popup::context_menu(response)
        .width(CONTENT_WIDTH)
        .frame(
            egui::Frame::new()
                .fill(Color32::WHITE)
                .stroke(egui::Stroke::new(1., Color32::from_gray(181)))
                .inner_margin(egui::Margin::symmetric(0, 3))
                .corner_radius(0)
                .shadow(egui::epaint::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: Color32::from_black_alpha(51),
                }),
        )
        .show(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            ui.set_width(CONTENT_WIDTH);
            ui.visuals_mut().disabled_alpha = 0.4;
            if item(ui, "运行已选择", !no_selection).clicked() {
                action = Some(Action::Run);
                ui.close();
            }
            if item(ui, "AI 生成 SQL", true).clicked() {
                action = Some(Action::Ai);
                ui.close();
            }
            separator(ui);
            for (name, allowed, event) in [
                ("剪切", !no_selection && enabled, Event::Cut),
                ("复制", !no_selection, Event::Copy),
            ] {
                if item(ui, name, allowed).clicked() {
                    action = Some(Action::Event(event));
                    ui.close();
                }
            }
            if item(ui, "粘贴", enabled).clicked() {
                response.request_focus();
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                ui.close();
            }
            separator(ui);
            if item(ui, "全选", true).clicked() {
                action = Some(Action::SelectAll);
                ui.close();
            }
        });
    action
}

fn item(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(CONTENT_WIDTH, ITEM_HEIGHT), egui::Sense::click());
        let highlighted = enabled && (response.hovered() || response.has_focus());
        if highlighted {
            ui.painter()
                .rect_filled(rect, 0., Color32::from_rgb(0, 150, 201));
        }
        let color = if highlighted {
            Color32::WHITE
        } else {
            Color32::from_gray(51)
        };
        ui.painter().text(
            rect.left_center() + egui::vec2(TEXT_INSET, 0.),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.),
            color,
        );
        response
    })
    .inner
}
fn separator(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(CONTENT_WIDTH, SEPARATOR_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.top() + 0.5,
        egui::Stroke::new(1., Color32::from_gray(200)),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.top() + 1.5,
        egui::Stroke::new(1., Color32::from_gray(230)),
    );
}
