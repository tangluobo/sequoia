//! Shared, on-demand Windows bold face; one job and one byte owner per context.
pub(crate) fn ensure(ctx: &egui::Context) {
    let start = ctx.data_mut(|data| {
        let loaded =
            data.get_temp_mut_or_insert_with(egui::Id::new("md-bold-cjk-requested"), || false);
        if *loaded {
            false
        } else {
            *loaded = true;
            true
        }
    });
    if !start {
        return;
    }
    let ctx = ctx.clone();
    // One short-lived asset job, only when bold CJK is displayed. Idle/SSH-only
    // sessions do not retain another complete CJK font, and file I/O stays off-frame.
    let _ = std::thread::Builder::new()
        .name("tomato-font".into())
        .spawn(move || {
            let Ok(bytes) = std::fs::read("C:/Windows/Fonts/msyhbd.ttc") else {
                return;
            };
            crate::font_assets::update(&ctx, |fonts| {
                fonts
                    .font_data
                    .insert("md-bold-cjk".into(), super::java_face(bytes).into());
                if let Some(family) = fonts
                    .families
                    .get_mut(&egui::FontFamily::Name("md-bold".into()))
                {
                    family.insert(1.min(family.len()), "md-bold-cjk".into());
                }
                let mut table = fonts
                    .families
                    .get(&egui::FontFamily::Name("table".into()))
                    .cloned()
                    .unwrap_or_else(|| fonts.families[&egui::FontFamily::Proportional].clone());
                table.insert(0, "md-bold-cjk".into());
                fonts
                    .families
                    .insert(egui::FontFamily::Name("table-bold".into()), table);
                let mut ui_bold = fonts
                    .families
                    .get(&egui::FontFamily::Proportional)
                    .cloned()
                    .unwrap_or_default();
                ui_bold.insert(0, "md-bold-cjk".into());
                fonts
                    .families
                    .insert(egui::FontFamily::Name("java-ui-bold".into()), ui_bold);
                if let Some(family) = fonts
                    .families
                    .get_mut(&egui::FontFamily::Name("table-user-bold".into()))
                    && !family.iter().any(|name| name == "md-bold-cjk")
                {
                    let first =
                        usize::from(family.first().is_some_and(|name| name == "table-user-bold"));
                    family.insert(first, "md-bold-cjk".into());
                }
            });
            ctx.request_repaint();
        });
}
