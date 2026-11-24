use super::*;
fn fixture() -> anyhow::Result<(tempfile::TempDir, TomatoApp, egui::Context)> {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("global.json"),br#"{"sshTerminalFontName":"monospace","sshTerminalFontSize":13,"scrollbackLines":1000,"rdpOpenMode":"WINDOW","uiGrayscaleText":false,"adbExecutablePath":"C:/old/adb.exe","future":{"nested":42}}"#)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    let app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(dir.path().join("connections.json")),
    );
    Ok((dir, app, ctx))
}
#[test]
fn legacy_preferences_default_to_neutral_text_and_respect_an_explicit_choice() {
    let mut settings = SettingsState::default();
    assert!(grayscale_text(&settings));
    assert!(Form::from(&settings).grayscale_text);
    Form::from(&settings).apply(&mut settings, Page::System);
    assert!(!settings.extra.contains_key(GRAYSCALE_TEXT));
    settings.extra.insert(GRAYSCALE_TEXT.into(), false.into());
    assert!(!grayscale_text(&settings));
    assert!(!Form::from(&settings).grayscale_text);
    settings.extra.insert(GRAYSCALE_TEXT.into(), true.into());
    assert!(grayscale_text(&settings));
}
#[test]
fn settings_edits_remain_a_draft_and_each_page_applies_only_its_own_fields() -> anyhow::Result<()> {
    let (dir, mut app, ctx) = fixture()?;
    let before = serde_json::to_value(&app.settings)?;
    let mut form = Form::from(&app.settings);
    form.sidebar_visible = false;
    form.grayscale_text = true;
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.visuals.text_options.lcd_subpixel = true;
    });
    form.rdp_open_mode = "TAB".into();
    form.scrollback_lines = 4;
    form.ssh_terminal_font_size = 20.;
    form.ssh_service_management_enabled = false;
    form.ssh_default_file_view_mode = "COLUMN".into();
    app.settings_form = Some(Box::new(form.clone()));
    app.settings_page = Page::Ssh;
    let mut out = ctx.run_ui(Default::default(), |ui| app.settings_view(ui));
    out.textures_delta.clear();
    assert_eq!(serde_json::to_value(&app.settings)?, before);
    assert!(app.save_settings(&form, Page::System));
    assert!(!app.settings.sidebar_visible);
    assert!(grayscale_text(&app.settings));
    assert!(
        ctx.style_of(egui::Theme::Light)
            .visuals
            .text_options
            .lcd_grayscale
    );
    assert_eq!(app.settings.scrollback_lines, 1000);
    assert_eq!(app.settings.ssh_terminal_font_size, 13.);
    assert_eq!(
        super::super::rdp_windows::global_mode(&app.settings),
        "WINDOW"
    );
    assert!(app.save_settings(&form, Page::Rdp));
    assert_eq!(super::super::rdp_windows::global_mode(&app.settings), "TAB");
    assert_eq!(app.settings.scrollback_lines, 1000);
    assert!(app.save_settings(&form, Page::Ssh));
    assert_eq!(app.settings.scrollback_lines, 4);
    assert_eq!(app.settings.ssh_terminal_font_size, 20.);
    assert!(!app.settings.ssh_service_management_enabled);
    assert_eq!(app.settings.ssh_default_file_view_mode, "COLUMN");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("global.json"))?)?;
    assert_eq!(saved["future"]["nested"], 42);
    assert_eq!(saved["rdpOpenMode"], "TAB");
    assert_eq!(saved["uiGrayscaleText"], true);
    assert_eq!(saved["adbExecutablePath"], "C:/old/adb.exe");
    Ok(())
}
#[test]
fn failed_global_save_keeps_applied_settings_and_the_unsaved_form() -> anyhow::Result<()> {
    let (dir, mut app, ctx) = fixture()?;
    let before = serde_json::to_value(&app.settings)?;
    let mut form = Form::from(&app.settings);
    form.scrollback_lines = 0;
    form.ssh_terminal_font_size = 18.;
    app.settings_form = Some(Box::new(form.clone()));
    app.settings_page = Page::System;
    std::fs::write(dir.path().join("global.json"), br#"{"external":true}"#)?;
    assert!(!app.save_settings(&form, Page::System));
    assert_eq!(serde_json::to_value(&app.settings)?, before);
    let mut out = ctx.run_ui(Default::default(), |ui| app.settings_view(ui));
    out.textures_delta.clear();
    assert_eq!(
        app.settings_form.as_ref().unwrap().ssh_terminal_font_size,
        18.
    );
    assert_eq!(app.settings_form.as_ref().unwrap().scrollback_lines, 0);
    assert_eq!(
        std::fs::read(dir.path().join("global.json"))?,
        br#"{"external":true}"#
    );
    Ok(())
}

fn sidebar_point(app: &mut TomatoApp, ctx: &egui::Context) -> anyhow::Result<egui::Pos2> {
    let mut out = ctx.run_ui(Default::default(), |ui| app.settings_view(ui));
    let point = out
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                (text.galley.job.text == "开启侧边栏").then(|| text.pos + text.galley.size() / 2.)
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow::anyhow!("Missing actual sidebar checkbox"))?;
    out.textures_delta.clear();
    Ok(point)
}
fn sidebar_click(app: &mut TomatoApp, ctx: &egui::Context, point: egui::Pos2) {
    for pressed in [true, false] {
        let input = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| app.settings_view(ui));
        out.textures_delta.clear();
    }
}
#[test]
fn actual_sidebar_checkbox_applies_and_persists_without_saving_other_drafts() -> anyhow::Result<()>
{
    let (dir, mut app, ctx) = fixture()?;
    let mut form = Form::from(&app.settings);
    form.ssh_terminal_font_size = 19.;
    app.settings_form = Some(Box::new(form));
    let point = sidebar_point(&mut app, &ctx)?;
    sidebar_click(&mut app, &ctx, point);
    assert!(!app.settings.sidebar_visible);
    assert_eq!(app.settings.ssh_terminal_font_size, 13.);
    assert_eq!(
        app.settings_form.as_ref().unwrap().ssh_terminal_font_size,
        19.
    );
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("global.json"))?)?;
    assert_eq!(saved["sidebarVisible"], false);
    assert_eq!(saved["future"]["nested"], 42);
    let point = sidebar_point(&mut app, &ctx)?;
    sidebar_click(&mut app, &ctx, point);
    assert!(app.settings.sidebar_visible);
    Ok(())
}
#[test]
fn sidebar_checkbox_reverts_when_legacy_file_changes_externally() -> anyhow::Result<()> {
    let (dir, mut app, ctx) = fixture()?;
    let point = sidebar_point(&mut app, &ctx)?;
    let external = br#"{"sidebarVisible":true,"future":{"external":99}}"#;
    std::fs::write(dir.path().join("global.json"), external)?;
    sidebar_click(&mut app, &ctx, point);
    assert!(app.settings.sidebar_visible);
    assert!(app.settings_form.as_ref().unwrap().sidebar_visible);
    assert!(app.error.is_some());
    assert_eq!(std::fs::read(dir.path().join("global.json"))?, external);
    Ok(())
}
