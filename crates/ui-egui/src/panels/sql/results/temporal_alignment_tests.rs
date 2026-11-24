use super::*;

#[test]
fn actual_time_fields_center_the_galley_at_each_dpi_and_keep_the_full_click_area() {
    for ppp in [1., 1.25, 1.5, 2.] {
        let ctx = egui::Context::default();
        let mut picker = Picker::new(Kind::Time, "07:34:56", Pos2::ZERO);
        let id = Id::new("centered-time-fields");
        let mut fields = Vec::new();
        let mut last = None;
        for frame in 0..3 {
            let mut input = egui::RawInput {
                time: Some(f64::from(frame) * 0.1),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(300., 180.))),
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .native_pixels_per_point = Some(ppp);
            if frame > 0 {
                // Click near the bottom of the first input, away from the text.
                let point = egui::pos2(25., 37.);
                input.events = vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed: frame == 1,
                        modifiers: Default::default(),
                    },
                ];
            }
            fields.clear();
            let mut output = ctx.run_ui(input, |ui| {
                for index in 0..3 {
                    let rect = Rect::from_min_size(
                        egui::pos2(11., 10. + index as f32 * 40.),
                        egui::vec2(56., 30.),
                    );
                    fields.push((picker.fields[index].clone(), rect.center().y));
                    spinner(ui, &mut picker, index, rect, id, FontId::proportional(13.));
                }
            });
            output.textures_delta.clear();
            last = Some(output);
        }
        let output = last.unwrap();
        for (value, center) in &fields {
            let text = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == *value => Some(text),
                    _ => None,
                })
                .expect("actual spinner TextEdit galley");
            assert!(
                (text.pos.y + text.galley.size().y / 2. - center).abs() <= 1. / ppp,
                "{value}: text {:?}, field center {center}, DPI {ppp}",
                text.pos
            );
        }
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(id.with(("time-field", 0usize)))
        );
        assert_eq!(picker.value.time, [7, 34, 56]);
        picker.release(&ctx);
    }
}
