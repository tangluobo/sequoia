use super::*;

// Sample the final tessellated draw order, including clip rectangles and alpha.
// Checking only the existence of a line misses a subsequent tab background.
pub(crate) fn solid_pixel(primitives: &[egui::ClippedPrimitive], point: egui::Pos2) -> [f32; 3] {
    let mut pixel = [255.0; 3];
    for primitive in primitives {
        if !primitive.clip_rect.contains(point) {
            continue;
        }
        let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive else {
            continue;
        };
        for indices in mesh.indices.as_chunks::<3>().0 {
            let vertices = indices.map(|index| mesh.vertices[index as usize]);
            let [a, b, c] = vertices.map(|vertex| vertex.pos);
            let cross = |a: egui::Vec2, b: egui::Vec2| a.x * b.y - a.y * b.x;
            let area = cross(b - a, c - a);
            if area.abs() < 0.00001 {
                continue;
            }
            let weights = [
                cross(b - point, c - point) / area,
                cross(c - point, a - point) / area,
                cross(a - point, b - point) / area,
            ];
            if weights.iter().any(|weight| *weight < -0.00001) {
                continue;
            }
            assert!(
                vertices
                    .iter()
                    .all(|vertex| vertex.uv == egui::epaint::WHITE_UV)
            );
            let color = std::array::from_fn::<_, 4, _>(|channel| {
                vertices
                    .iter()
                    .zip(weights)
                    .map(|(vertex, weight)| f32::from(vertex.color[channel]) * weight)
                    .sum::<f32>()
            });
            for channel in 0..3 {
                pixel[channel] = color[channel] + pixel[channel] * (1.0 - color[3] / 255.0);
            }
        }
    }
    pixel
}

#[test]
fn title_and_sidebar_bottom_borders_survive_tabs_switches_and_dpi() -> anyhow::Result<()> {
    for dpi in [1.0, 1.25, 1.5, 2.0] {
        let directory = tempfile::tempdir()?;
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        ctx.set_pixels_per_point(dpi);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let mut app = TomatoApp::with_repository(
            &ctx,
            runtime,
            ConnectionFileRepository::new(directory.path().join("connections.json")),
        );
        for state in 0..6 {
            if state == 1 {
                app.open_tool("desktop_shortcut", "桌面快捷方式");
            }
            if state == 2 {
                app.open_tool("image_format_converter", "图片格式转换");
            }
            if state == 3 {
                app.active = app.tabs.first().map(|tab| tab.id.clone());
            }
            if state == 4 {
                app.tabs.clear();
                app.active = None;
            }
            if state == 5 {
                app.settings.sidebar_visible = false;
            }
            for _ in 0..3 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1200., 900.),
                        )),
                        ..Default::default()
                    },
                    |ui| app.ui(ui),
                );
                output.textures_delta.clear();
                let primitives = ctx.tessellate(output.shapes, ctx.pixels_per_point());
                let header = app.layout.header;
                assert_eq!(header.height(), crate::theme::HEADER_HEIGHT);
                for x in [
                    header.left() + 10.25,
                    header.center().x + 0.25,
                    header.right() - 10.25,
                ] {
                    let pixel = solid_pixel(&primitives, egui::pos2(x, header.bottom() - 0.5));
                    for (actual, expected) in pixel.into_iter().zip([217., 217., 215.]) {
                        assert!(
                            (actual - expected).abs() < 1.1,
                            "dpi={dpi}, state={state}, title pixel={pixel:?}"
                        );
                    }
                }
                if app.settings.sidebar_visible {
                    let point =
                        egui::pos2(app.layout.sidebar.left() + 10.25, header.bottom() - 0.5);
                    let pixel = solid_pixel(&primitives, point);
                    assert!(
                        (pixel[0] - 217.).abs() < 1.1,
                        "dpi={dpi}, state={state}, sidebar pixel={pixel:?}"
                    );
                }
            }
        }
        assert!(
            app.pending.is_empty(),
            "drawing must not start protocol work"
        );
    }
    Ok(())
}

#[test]
fn navigation_and_connection_dividers_have_one_owner_at_each_dpi() -> anyhow::Result<()> {
    for dpi in [1., 1.25, 1.5, 2.] {
        let directory = tempfile::tempdir()?;
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        ctx.set_pixels_per_point(dpi);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let mut app = TomatoApp::with_repository(
            &ctx,
            runtime,
            ConnectionFileRepository::new(directory.path().join("connections.json")),
        );
        for width in [1200., 900.] {
            let mut pointer = egui::Pos2::ZERO;
            for phase in 0..8 {
                let events = match phase {
                    0 | 7 => vec![egui::Event::PointerGone],
                    3 => {
                        pointer = egui::pos2(
                            app.layout.sidebar.right() + 0.5,
                            app.layout.sidebar.top() + 140.37,
                        );
                        vec![egui::Event::PointerMoved(pointer)]
                    }
                    4 | 6 => vec![egui::Event::PointerButton {
                        pos: pointer,
                        button: egui::PointerButton::Primary,
                        pressed: phase == 4,
                        modifiers: Default::default(),
                    }],
                    5 => {
                        pointer.x += 12.;
                        vec![egui::Event::PointerMoved(pointer)]
                    }
                    _ => vec![],
                };
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 900.),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| app.ui(ui),
                );
                output.textures_delta.clear();
                let edge = app.layout.sidebar.right();
                let y = app.layout.sidebar.top() + 140.37;
                let lines = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::LineSegment { points, stroke }
                            if points[0].x == points[1].x
                                && (points[0].x - edge).abs() < 8.
                                && (points[1].y - points[0].y).abs()
                                    > app.layout.sidebar.height() * 0.9 =>
                        {
                            Some((points[0].x, stroke.width))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    lines.len(),
                    1,
                    "DPI {dpi}, phase {phase}: the panel must own the sole separator/highlight"
                );
                assert!(
                    !output.shapes.iter().any(
                        |shape| matches!(&shape.shape,egui::Shape::Rect(rect)
                    if rect.fill==crate::theme::DIVIDER && (rect.rect.center().x-edge).abs()<8.
                    && rect.rect.height()>app.layout.sidebar.height()*0.9)
                    ),
                    "A second divider panel was painted"
                );
                let primitives = ctx.tessellate(output.shapes, ctx.pixels_per_point());
                let (center, stroke_width) = lines[0];
                let clearance = stroke_width * 0.5 + 2. / ctx.pixels_per_point();
                for x in [
                    center - clearance,
                    center + clearance,
                    app.layout.nav.right() + 1.25,
                ] {
                    let pixel = solid_pixel(&primitives, egui::pos2(x, y));
                    assert!(
                        pixel.iter().all(|v| (v - 255.).abs() < 1.1),
                        "DPI {dpi}, phase {phase}, x {x}: border bleeds into neighbours {pixel:?}"
                    );
                }
                // The following panel clips the outer part of a resize
                // highlight, so inspect the visible inner part of the line.
                let border = solid_pixel(&primitives, egui::pos2(center.min(edge + 0.5), y));
                assert!(border.iter().all(|v| *v < 254.), "Separator is absent");
                let nav = solid_pixel(&primitives, egui::pos2(app.layout.nav.right() - 0.5, y));
                assert!(
                    (228.0..236.0).contains(&nav[0]) && nav[0] < 235.99,
                    "Navigation border is absent or duplicated: {nav:?}"
                );
            }
        }
        assert!(app.pending.is_empty());
    }
    Ok(())
}
