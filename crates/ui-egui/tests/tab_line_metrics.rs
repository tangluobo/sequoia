//! Manual diagnostics for the owned JavaFX header/overflow scene.
#[test]
#[ignore = "requires the owned tomato-tabs-fixture profile and tab_strip preview environment"]
fn tab_line_metrics() -> anyhow::Result<()> {
    anyhow::ensure!(
        std::env::var_os("TOMATO_DATA_DIR").is_some_and(|path| std::path::Path::new(&path)
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("tomato-tabs-fixture-"))),
        "isolated profile required"
    );
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = tomato_ui_egui::TomatoApp::new(&ctx, runtime);
    for pass in 0..10 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800., 600.),
                )),
                time: Some(pass as f64 * 0.2),
                ..Default::default()
            },
            |ui| app.ui(ui),
        );
        if pass == 9 {
            for shape in &output.shapes {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.pos.x > 500.
                    && text.pos.y > 90.
                {
                    println!(
                        "{:?}: pos={:?}, size={:?}, ink={:?}, opacity={}",
                        text.galley.job.text,
                        text.pos,
                        text.galley.size(),
                        text.galley.mesh_bounds,
                        text.opacity_factor
                    );
                }
            }
        }
        output.textures_delta.clear();
    }
    Ok(())
}
