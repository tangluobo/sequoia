//! Manual measurements of the owned inert layout scene, no OS input or clipboard.
#[test]
#[ignore = "requires owned tomato-layout-fixture profile and layout_alignment preview"]
fn layout_line_metrics() -> anyhow::Result<()> {
    anyhow::ensure!(
        std::env::var_os("TOMATO_DATA_DIR").is_some_and(|path| std::path::Path::new(&path)
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("tomato-layout-fixture-"))),
        "owned profile required"
    );
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = tomato_ui_egui::TomatoApp::new(&ctx, runtime);
    for (pass, width) in [
        1200., 1200., 1200., 1200., 900., 900., 900., 900., 1600., 1600., 1600., 1600.,
    ]
    .into_iter()
    .enumerate()
    {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 900.),
                )),
                time: Some(pass as f64 * 0.2),
                ..Default::default()
            },
            |ui| app.ui(ui),
        );
        if pass % 4 == 3 {
            for shape in &output.shapes {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.pos.x > 500.
                    && text.pos.y > 120.
                    && text.pos.y < 550.
                {
                    println!(
                        "width={width}: {:?}: pos={:?}, size={:?}, rows={}, wrap={}",
                        text.galley.job.text,
                        text.pos,
                        text.galley.size(),
                        text.galley.rows.len(),
                        text.galley.job.wrap.max_width
                    );
                }
            }
        }
        output.textures_delta.clear();
    }
    Ok(())
}
