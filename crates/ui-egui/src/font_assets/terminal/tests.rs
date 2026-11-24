use super::*;
fn frame(ctx: &Context) {
    let mut output = ctx.run_ui(Default::default(), |ui| {
        ui.label("font fixture");
    });
    output.textures_delta.clear();
}
#[test]
fn terminal_and_preview_defaults_share_loaded_fonts_without_changing_other_families() {
    let ctx = Context::default();
    let definitions = egui::FontDefinitions::default();
    let count = definitions.font_data.len();
    let original = definitions.font_data.values().next().unwrap().clone();
    crate::font_assets::install(&ctx, definitions);
    ensure(&ctx, "monospace");
    frame(&ctx);
    let _ = preview(&ctx, "monospace", 13.);
    frame(&ctx);
    ctx.fonts(|fonts| {
        let definitions = fonts.definitions();
        assert_eq!(definitions.font_data.len(), count);
        assert!(
            definitions
                .font_data
                .values()
                .any(|font| Arc::ptr_eq(font, &original))
        );
        assert_eq!(
            definitions.families[&FontFamily::Name("ssh-terminal".into())],
            definitions.families[&FontFamily::Monospace]
        );
        assert_eq!(
            definitions.families[&FontFamily::Name("ssh-preview".into())],
            definitions.families[&FontFamily::Monospace]
        );
    });
}
#[test]
#[cfg(windows)]
fn installed_font_preview_coalesces_requests_and_does_not_change_applied_terminal_face() {
    if !std::path::Path::new("C:/Windows/Fonts/cour.ttf").exists() {
        return;
    }
    let ctx = Context::default();
    crate::font_assets::install(&ctx, Default::default());
    ensure(&ctx, "monospace");
    frame(&ctx);
    for n in 0..32 {
        let _ = preview(&ctx, &format!("fixture-missing-{n}"), 13.);
    }
    let _ = preview(&ctx, "Courier New", 15.);
    let shared = state(&ctx, true);
    let start = std::time::Instant::now();
    while shared.lock().unwrap().working {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(15),
            "font worker did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        frame(&ctx);
    }
    frame(&ctx);
    ctx.fonts(|fonts| {
        let definitions = fonts.definitions();
        assert_eq!(
            definitions.families[&FontFamily::Name("ssh-terminal".into())],
            definitions.families[&FontFamily::Monospace]
        );
        assert_eq!(
            definitions.families[&FontFamily::Name("ssh-preview".into())][0],
            "terminal-preview-custom"
        );
        assert!(
            definitions
                .font_data
                .contains_key("terminal-preview-custom")
        );
        assert!(!definitions.font_data.contains_key("terminal-custom"));
    });
    let _ = preview(&ctx, "monospace", 13.);
    frame(&ctx);
    ctx.fonts(|fonts| {
        assert!(
            !fonts
                .definitions()
                .font_data
                .contains_key("terminal-preview-custom")
        )
    });
}
