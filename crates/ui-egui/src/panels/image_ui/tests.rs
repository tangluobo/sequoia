use super::*;
use crate::tabs::ssh::images::Item;
use std::sync::Arc;
fn frame(ctx: &egui::Context, preview: &mut Preview, time: f64, events: Vec<egui::Event>) {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(784., 561.),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ui| ready(ui, preview),
    );
    output.textures_delta.clear();
}
fn pointer(point: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(point),
        egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
fn click(ctx: &egui::Context, preview: &mut Preview, time: f64, point: egui::Pos2) {
    frame(ctx, preview, time, pointer(point, true));
    frame(ctx, preview, time + 0.05, pointer(point, false));
}
#[test]
fn actual_preview_controls_fit_reset_pan_zoom_navigate_and_request_delete() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let items = Arc::new(
        (0..2)
            .map(|index| Item {
                name: format!("{index}.png"),
                path: format!("/{index}.png"),
                size: 100,
            })
            .collect(),
    );
    let mut preview = Preview::new(items, 0);
    preview.accept(
        tomato_services::files::images::Asset {
            width: 1200,
            height: 800,
            picture: tomato_services::markdown::Picture {
                gif: None,
                width: 1200,
                height: 800,
                rgba: vec![255; 1200 * 800 * 4],
            },
            gif: None,
        },
        &ctx,
    );
    frame(&ctx, &mut preview, 0., vec![]);
    assert_eq!(preview.scale, 0.625);
    click(&ctx, &mut preview, 0.1, egui::pos2(652., 16.));
    assert_eq!(preview.scale, 1.);
    let point = egui::pos2(400., 300.);
    frame(
        &ctx,
        &mut preview,
        0.3,
        vec![
            egui::Event::PointerMoved(point),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: egui::vec2(0., 1.),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!((preview.scale - 1.05).abs() < 0.0001);
    frame(&ctx, &mut preview, 0.4, pointer(point, true));
    frame(
        &ctx,
        &mut preview,
        0.45,
        vec![egui::Event::PointerMoved(point + egui::vec2(50., 20.))],
    );
    frame(
        &ctx,
        &mut preview,
        0.5,
        pointer(point + egui::vec2(50., 20.), false),
    );
    assert_eq!(preview.translation, egui::vec2(50., 20.));
    click(&ctx, &mut preview, 0.6, egui::pos2(598., 16.));
    assert_eq!(preview.translation, egui::Vec2::ZERO);
    assert!((preview.scale - 0.635).abs() < 0.0001);
    click(&ctx, &mut preview, 0.8, egui::pos2(755., 16.));
    assert!(preview.confirm_delete);
    preview.confirm_delete = false;
    let old = preview.request.clone();
    frame(
        &ctx,
        &mut preview,
        1.,
        vec![egui::Event::Key {
            key: egui::Key::ArrowRight,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(preview.index, 1);
    assert!(old.cancel.is_cancelled());
    assert!(preview.texture.is_none());
}
