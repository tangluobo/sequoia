use super::*;
#[test]
fn stock_button_layers_match_resolved_original_modena_css_in_all_pointer_and_focus_states() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/dialogs/original-button-palette.json"
    ))
    .unwrap();
    for (index, case) in cases.as_array().unwrap()[..16].iter().enumerate() {
        let fills = case["fills"].as_array().unwrap();
        let native = button_palette::PALETTES[index];
        assert_eq!(native.len(), fills.len(), "{}", case["state"]);
        for (native, source) in native.iter().zip(fills) {
            for (actual, expected) in native
                .insets
                .iter()
                .zip(source["insets"].as_array().unwrap())
            {
                assert!((f64::from(*actual) - expected.as_f64().unwrap()).abs() < 1e-6);
            }
            assert_eq!(
                f64::from(native.radius),
                source["radii"][0].as_f64().unwrap()
            );
            let rgba = |value: &serde_json::Value| {
                let colors = value.as_array().unwrap();
                std::array::from_fn::<u8, 4, _>(|index| {
                    (colors[index].as_f64().unwrap() * 255.).round() as u8
                })
            };
            if !source["solid"].is_null() {
                assert_eq!(native.top, rgba(&source["solid"]));
                assert_eq!(native.bottom, native.top);
            } else {
                assert_eq!(native.top, rgba(&source["stops"][0]["color"]));
                assert_eq!(native.bottom, rgba(&source["stops"][1]["color"]));
            }
        }
    }
}
#[test]
fn stock_button_backgrounds_do_not_allocate_gradient_textures() {
    let ctx = egui::Context::default();
    let mut warm = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.label("Warm");
    });
    warm.textures_delta.clear();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        for index in 0..16 {
            button_paint::paint(
                ui,
                egui::Rect::from_min_size(
                    egui::pos2(
                        10. + (index % 4) as f32 * 85.,
                        10. + (index / 4) as f32 * 33.,
                    ),
                    egui::vec2(75., 23.),
                ),
                button_paint::State {
                    default: index >= 8,
                    hover: index % 2 == 1,
                    armed: index % 4 >= 2,
                    focused: index % 8 >= 4,
                    enabled: true,
                },
            );
        }
    });
    assert!(
        output.textures_delta.set.is_empty(),
        "button gradients must not leave additional texture allocations"
    );
    assert_eq!(
        output
            .shapes
            .iter()
            .filter(|shape| matches!(shape.shape, egui::Shape::Mesh(_)))
            .count(),
        16
    );
    output.textures_delta.clear();
}
