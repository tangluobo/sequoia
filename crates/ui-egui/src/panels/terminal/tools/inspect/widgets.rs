use super::*;
pub(super) fn section(ui: &mut Ui, label: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.;
        ui.label(
            egui::RichText::new(label)
                .font(crate::native_dialog::font(ui.ctx(), 12.))
                .strong(),
        );
        ui.separator();
    });
}
pub(super) fn hint(ui: &mut Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .font(crate::native_dialog::font(ui.ctx(), 10.))
                .color(Color32::from_gray(153)),
        )
        .wrap(),
    );
}
pub(super) fn button(ui: &mut Ui, label: &str) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(40., 23.), egui::Sense::hover());
    crate::native_dialog::button(ui, rect, label, false)
}
pub(super) fn field(
    ui: &mut Ui,
    id: &str,
    value: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    ui.add(
        egui::TextEdit::singleline(value)
            .id(ui.id().with(id))
            .desired_width(width)
            .min_size(egui::vec2(width, 23.))
            .font(crate::native_dialog::font(ui.ctx(), 12.))
            .hint_text(hint)
            .char_limit(40),
    )
}
pub(super) fn detail(ui: &mut Ui, label: &str, text: &str) {
    egui::ScrollArea::vertical()
        .id_salt(("inspect-detail-scroll", label))
        .auto_shrink([false, false])
        .min_scrolled_height(46.)
        .max_height(46.)
        .show(ui, |ui| {
            let mut text = text;
            let response = ui.add(
                egui::TextEdit::multiline(&mut text)
                    .id(ui.id().with(("inspect-detail", label)))
                    .font(egui::FontId::monospace(11.))
                    .desired_width(f32::INFINITY)
                    .desired_rows(2)
                    .min_size(egui::vec2(0., 46.))
                    .background_color(Color32::from_gray(250)),
            );
            if let Some(state) =
                egui::widgets::text_edit::TextEditState::load(ui.ctx(), response.id)
            {
                state.trim_undo_memory(0);
                state.store(ui.ctx(), response.id);
            }
        });
}
pub(super) fn retries(ui: &mut Ui, text: &mut String, value: &mut u32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(70., 23.), egui::Sense::hover());
    let field = egui::Rect::from_min_max(rect.min, rect.right_bottom() - egui::vec2(18., 0.));
    let response = ui.place(
        field,
        egui::TextEdit::singleline(text)
            .char_limit(3)
            .font(crate::native_dialog::font(ui.ctx(), 12.)),
    );
    if response.changed()
        && let Ok(number) = text.parse::<u32>()
    {
        *value = number.min(100);
    }
    if response.lost_focus() {
        *text = value.to_string();
    }
    for (up, key) in [(true, egui::Key::ArrowUp), (false, egui::Key::ArrowDown)] {
        let button = egui::Rect::from_min_size(
            egui::pos2(
                rect.right() - 18.,
                if up { rect.top() } else { rect.center().y },
            ),
            egui::vec2(18., 11.5),
        );
        let id = ui.id().with(("retry", up));
        let r = ui.interact(button, id, egui::Sense::click());
        crate::native_dialog::paint_button(
            ui,
            button,
            crate::native_dialog::ButtonState {
                default: false,
                hover: r.hovered(),
                armed: r.is_pointer_button_down_on(),
                focused: false,
                enabled: ui.is_enabled(),
            },
        );
        let p = button.center();
        let d = if up { -1. } else { 1. };
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                p + egui::vec2(-3., -d),
                p + egui::vec2(3., -d),
                p + egui::vec2(0., 2. * d),
            ],
            Color32::from_gray(70),
            egui::Stroke::NONE,
        ));
        if crate::native_dialog::spinner::repeat(ui, &r, id)
            || (response.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)))
        {
            *value = if up {
                value.saturating_add(1).min(100)
            } else {
                value.saturating_sub(1)
            };
            *text = value.to_string();
            response.request_focus();
        }
    }
}
