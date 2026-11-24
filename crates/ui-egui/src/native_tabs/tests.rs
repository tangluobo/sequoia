use super::*;

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut fonts = egui::FontDefinitions::default();
    fonts.families.insert(
        egui::FontFamily::Name("java-ui".into()),
        fonts.families[&egui::FontFamily::Proportional].clone(),
    );
    ctx.set_fonts(fonts);
    ctx
}
fn frame(
    ctx: &egui::Context,
    selected: &mut usize,
    width: f32,
    enabled: bool,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Rect) {
    let mut strip = Rect::NOTHING;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 250.))),
            time: Some(ctx.input(|input| input.time) + 0.2),
            events,
            ..Default::default()
        },
        |ui| {
            strip = Rect::from_min_size(
                ui.next_widget_position(),
                vec2(ui.available_width(), Style::Compact.height()),
            );
            ui.add_enabled_ui(enabled, |ui| {
                show(
                    ui,
                    "tabs",
                    selected,
                    Style::Compact,
                    [(0, "info"), (1, "result"), (2, "statistics")],
                );
            });
        },
    );
    output.textures_delta.clear();
    (output, strip)
}
fn label(output: &egui::FullOutput, value: &str) -> Option<egui::Pos2> {
    output.shapes.iter().rev().find_map(|shape| {
        if let egui::Shape::Text(text) = &shape.shape {
            let center = text.pos + text.galley.size() * 0.5;
            if text.galley.job.text == value && shape.clip_rect.contains(center) {
                return Some(center);
            }
        }
        None
    })
}
fn click(ctx: &egui::Context, selected: &mut usize, width: f32, enabled: bool, point: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            ctx,
            selected,
            width,
            enabled,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}
fn key(ctx: &egui::Context, selected: &mut usize, enabled: bool, key: egui::Key) {
    frame(
        ctx,
        selected,
        896.,
        enabled,
        vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
}
#[test]
fn inner_tabs_route_pointer_keyboard_and_disabled_selection() {
    let ctx = context();
    let mut selected = 0;
    let (output, _) = frame(&ctx, &mut selected, 896., true, vec![]);
    let point = label(&output, "result").unwrap();
    click(&ctx, &mut selected, 896., true, point);
    assert_eq!(selected, 1);
    key(&ctx, &mut selected, true, egui::Key::ArrowRight);
    assert_eq!(selected, 2);
    key(&ctx, &mut selected, true, egui::Key::Home);
    assert_eq!(selected, 0);
    key(&ctx, &mut selected, true, egui::Key::End);
    assert_eq!(selected, 2);
    let (output, _) = frame(&ctx, &mut selected, 896., false, vec![]);
    let point = label(&output, "info").unwrap();
    click(&ctx, &mut selected, 896., false, point);
    assert_eq!(selected, 2);
    key(&ctx, &mut selected, false, egui::Key::Home);
    assert_eq!(selected, 2);
}
#[test]
fn inner_tab_overflow_selects_a_hidden_page_and_reveals_it() {
    let ctx = context();
    let mut selected = 0;
    let (_, strip) = frame(&ctx, &mut selected, 140., true, vec![]);
    assert!(
        label(
            &frame(&ctx, &mut selected, 140., true, vec![]).0,
            "statistics"
        )
        .is_none()
    );
    click(
        &ctx,
        &mut selected,
        140.,
        true,
        pos2(strip.right() - 13., strip.center().y),
    );
    frame(&ctx, &mut selected, 140., true, vec![]);
    let (output, _) = frame(&ctx, &mut selected, 140., true, vec![]);
    let point = label(&output, "statistics").expect("hidden tab must appear in overflow menu");
    click(&ctx, &mut selected, 140., true, point);
    assert_eq!(selected, 2);
    for _ in 0..8 {
        frame(&ctx, &mut selected, 140., true, vec![]);
    }
    let (output, _) = frame(&ctx, &mut selected, 140., true, vec![]);
    assert!(
        label(&output, "statistics").is_some(),
        "selected hidden page must scroll into view"
    );
}
