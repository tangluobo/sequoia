//! PasswordPromptDialog for the current RFB handshake.
use egui::{Color32, Context, Id, Key, Modifiers, RichText, TextEdit};

pub(in crate::panels::desktop) enum Action {
    Submit,
    Cancel,
}

pub(in crate::panels::desktop) fn show(
    ctx: &Context,
    tab: &str,
    connection: &tomato_core::Connection,
    view: &mut crate::tabs::desktop::DesktopView,
    error: Option<&str>,
) -> Option<Action> {
    let id = Id::new(("vnc-password", tab));
    let first_frame = view.vnc_password_editor.is_none();
    let editor = view.vnc_password_editor.get_or_insert_default();
    let mut action = None;
    let dialog = egui::Modal::new(id)
        .frame(egui::Frame::NONE)
        .backdrop_color(Color32::TRANSPARENT)
        .show(ctx, |ui| {
            use egui::{Align2, FontId, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
            // The Java DialogPane measures 346 × 161 points without its title bar.
            const WIDTH: f32 = 346.;
            const HEIGHT: f32 = 161.;
            const TITLE: f32 = 26.;
            let error = error.map(|message| {
                ui.painter().layout(
                    message.into(),
                    FontId::proportional(12.),
                    Color32::from_rgb(180, 30, 30),
                    WIDTH - 20.,
                )
            });
            let extra = error.as_ref().map_or(0., |text| text.size().y + 12.);
            let (outer, _) = ui.allocate_exact_size(
                vec2(WIDTH + 2., TITLE + HEIGHT + extra + 2.),
                Sense::hover(),
            );
            let inside = outer.shrink(1.);
            let title = Rect::from_min_size(inside.min, vec2(WIDTH, TITLE));
            let body = Rect::from_min_max(title.left_bottom(), inside.max);
            let point = |x: f32, y: f32| body.min + vec2(x, y);
            let area = |x, y, width, height| Rect::from_min_size(point(x, y), vec2(width, height));
            ui.painter().rect_filled(outer, 0., Color32::from_gray(244));
            ui.painter().rect_filled(title, 0., Color32::WHITE);
            ui.painter().rect_stroke(
                outer,
                0.,
                Stroke::new(1., Color32::from_gray(166)),
                StrokeKind::Inside,
            );
            ui.painter().text(
                title.left_center() + vec2(10., 0.),
                Align2::LEFT_CENTER,
                "输入 VNC 密码",
                FontId::proportional(13.),
                Color32::BLACK,
            );
            let close =
                Rect::from_min_max(pos2(title.right() - 36., title.top()), title.right_bottom());
            let close_response = ui.interact(close, id.with("close"), Sense::click());
            if close_response.hovered() {
                ui.painter()
                    .rect_filled(close, 0., Color32::from_rgb(232, 17, 35));
            }
            let close_color = if close_response.hovered() {
                Color32::WHITE
            } else {
                Color32::BLACK
            };
            let center = close.center();
            for direction in [-1., 1.] {
                ui.painter().line_segment(
                    [
                        center - vec2(4., direction * 4.),
                        center + vec2(4., direction * 4.),
                    ],
                    Stroke::new(1., close_color),
                );
            }
            if close_response.clicked() {
                action = Some(Action::Cancel);
            }

            let host = format!(
                "{} ({}:{})",
                connection.name,
                connection.text("host"),
                tomato_services::remote_desktop::vnc_port(connection),
            );
            let host_rect = area(10., 0., WIDTH - 20., 38.);
            ui.painter().with_clip_rect(host_rect).text(
                host_rect.left_center(),
                Align2::LEFT_CENTER,
                &host,
                FontId::proportional(13.),
                Color32::BLACK,
            );
            ui.interact(host_rect, id.with("host"), Sense::hover())
                .on_hover_text(host);
            ui.painter().hline(
                body.x_range(),
                body.top() + 38.,
                Stroke::new(1., Color32::from_gray(200)),
            );
            if let Some(error) = error {
                ui.painter()
                    .galley(point(10., 47.), error, Color32::from_rgb(180, 30, 30));
            }
            ui.painter().text(
                point(10., 70.5 + extra),
                Align2::LEFT_CENTER,
                "VNC 密码：",
                FontId::proportional(12.),
                Color32::BLACK,
            );
            let field_id = editor.prepare_id(ctx, id.with("field"));
            let field = ui.place(
                area(86., 59. + extra, 250., 23.),
                TextEdit::singleline(&mut view.password)
                    .id(field_id)
                    .password(true)
                    .char_limit(1024),
            );
            if first_frame {
                field.request_focus();
            }
            ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(area(86., 90. + extra, 120., 20.))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            )
            .checkbox(
                &mut view.remember,
                RichText::new("保存密码")
                    .size(11.)
                    .color(Color32::from_gray(102)),
            );
            if ui
                .place(
                    area(175., 127. + extra, 76., 24.),
                    egui::Button::new(RichText::new("确定").size(12.))
                        .fill(Color32::from_rgb(170, 215, 234))
                        .stroke(Stroke::new(1., Color32::from_rgb(62, 175, 224))),
                )
                .clicked()
            {
                action = Some(Action::Submit);
            }
            if ui
                .place(
                    area(261., 127. + extra, 75., 24.),
                    egui::Button::new(RichText::new("取消").size(12.)),
                )
                .clicked()
            {
                action = Some(Action::Cancel);
            }
        });
    if dialog.is_top_modal && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
    {
        return Some(Action::Cancel);
    }
    if dialog.is_top_modal
        && !first_frame
        && action.is_none()
        && ctx.input(|input| {
            input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: Key::Enter,
                        pressed: true,
                        repeat: false,
                        ..
                    }
                )
            })
        })
    {
        action = Some(Action::Submit);
    }
    match action {
        Some(Action::Submit) if view.password.is_empty() => Some(Action::Cancel),
        action => action,
    }
}
