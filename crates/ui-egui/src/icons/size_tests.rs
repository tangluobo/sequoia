use super::*;
#[test]
fn image_view_samples_pixel_centers_in_premultiplied_color() {
    let source = image::RgbaImage::from_fn(2, 2, |x, y| {
        if x == 0 && y == 0 {
            image::Rgba([255, 0, 0, 255])
        } else {
            image::Rgba([0, 0, 255, 0])
        }
    });
    let reduced = sample_image_view(source, 1);
    assert_eq!(reduced.pixels[0].to_array(), [64, 0, 0, 64]);
}

#[test]
fn java_minification_uses_the_original_samples_without_lanczos_ringing() {
    let source = image::RgbaImage::from_fn(5, 5, |x, y| {
        let value = (x * 36 + y * 9) as u8;
        image::Rgba([value, 0, 0, 255])
    });
    let reduced = minify_icon(&source, 2);
    // Destination centers map to source pixels 0 and 2. Each average spans
    // three samples on both axes with replicated samples at the boundary.
    assert_eq!(reduced.get_pixel(0, 0).0, [15, 0, 0, 255]);
    assert_eq!(reduced.get_pixel(1, 0).0, [75, 0, 0, 255]);
    assert_eq!(reduced.get_pixel(0, 1).0, [30, 0, 0, 255]);
    assert_eq!(reduced.get_pixel(1, 1).0, [90, 0, 0, 255]);
}

#[test]
fn fractional_tree_rows_paint_original_rasters_once_at_integer_physical_pixels() {
    let ctx = egui::Context::default();
    let mut icons = Icons::default();
    let name = "images/connect/windows.png";
    for ppp in [1., 1.25, 1.5, 2.] {
        let mut input = egui::RawInput::default();
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .native_pixels_per_point = Some(ppp);
        let mut output = ctx.run_ui(input, |ui| {
            icons.paint(
                ui,
                name,
                egui::Rect::from_min_size(egui::pos2(98., 102.5), egui::vec2(20., 20.)),
            );
        });
        let edge = (20f32 * ppp).round() as usize;
        let cached = &icons.textures[&format!("{name}@{edge}")];
        assert_eq!(cached.texture.size(), [edge, edge]);
        let mesh = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Mesh(mesh) = &shape.shape {
                    (mesh.texture_id == cached.texture.id()).then_some(mesh)
                } else {
                    None
                }
            })
            .expect("actual raster icon mesh");
        for vertex in &mesh.vertices {
            let pixel = vertex.pos * ppp;
            assert!(
                (pixel.x - pixel.x.round()).abs() < 0.001
                    && (pixel.y - pixel.y.round()).abs() < 0.001
            );
        }
        output.textures_delta.clear();
    }
}
#[test]
fn svg_paint_uses_destination_pixels_and_keeps_dpi_caches_bounded() {
    let ctx = egui::Context::default();
    let mut icons = Icons::default();
    for ppp in [1., 1.25, 1.5, 2.] {
        let mut input = egui::RawInput::default();
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .native_pixels_per_point = Some(ppp);
        let mut output = ctx.run_ui(input, |ui| {
            icons.paint(
                ui,
                "images/tools/icon-header.svg",
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(18., 18.)),
            );
        });
        output.textures_delta.clear();
        let edge = (18f32 * ppp).round() as usize;
        assert_eq!(
            icons.textures[&format!("images/tools/icon-header.svg@{edge}")]
                .texture
                .size(),
            [edge, edge]
        );
    }
    assert_eq!(icons.textures.len(), 4);
}
#[test]
fn small_original_pngs_keep_native_pixels_instead_of_expanding_the_texture_cache() {
    let ctx = egui::Context::default();
    let mut icons = Icons::default();
    for name in [
        "upstream/openjfx/dialog-confirm.png",
        "images/connect/save.png",
        "images/connect/query.png",
    ] {
        let original = image::load_from_memory(crate::asset_bytes::bytes(name).unwrap()).unwrap();
        assert!(original.width() <= 64 && original.height() <= 64);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert!(icons.get(ui.ctx(), name).is_some());
        });
        output.textures_delta.clear();
        assert_eq!(
            icons.textures[name].texture.size(),
            [original.width() as usize, original.height() as usize]
        );
    }
}
#[test]
fn stopped_container_grayscale_matches_the_actual_java_pixel_conversion() -> anyhow::Result<()> {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/icons/original-gray.json"
    ))?;
    for case in cases.as_array().unwrap() {
        let input = case["input"].as_u64().unwrap() as u32;
        let expected = case["output"].as_u64().unwrap() as u32;
        let mut pixel = [
            (input >> 16) as u8,
            (input >> 8) as u8,
            input as u8,
            (input >> 24) as u8,
        ];
        gray(&mut pixel);
        let output = (u32::from(pixel[3]) << 24)
            | (u32::from(pixel[0]) << 16)
            | (u32::from(pixel[1]) << 8)
            | u32::from(pixel[2]);
        assert_eq!(output, expected);
    }
    Ok(())
}
