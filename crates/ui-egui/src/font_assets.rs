//! Merge asynchronous font additions, including definitions awaiting the next frame.
use egui::{Context, FontDefinitions, Id};
use std::sync::{Arc, Mutex};
pub(crate) mod bold;
pub(crate) mod table;
pub(crate) mod terminal;
pub(crate) fn java_face(bytes: Vec<u8>) -> egui::FontData {
    let mut face = egui::FontData::from_owned(bytes);
    if face.font.get(0..4) == Some(b"ttcf") && face.font.get(8..12) == Some(&[0, 0, 0, 2]) {
        face.index = 1;
    }
    face
}
type Shared = Arc<Mutex<FontDefinitions>>;
type RendererRef = std::sync::Weak<egui::epaint::mutex::RwLock<egui_wgpu::Renderer>>;
pub(crate) fn lcd_enabled(ctx: &Context) -> bool {
    ctx.data(|data| data.get_temp::<bool>(Id::new("lcd-text-renderer")))
        .unwrap_or(false)
}
/// Negotiate the font atlas format with the renderer; never upload RGB coverage
/// to a backend that only understands scalar alpha.
pub fn configure_text_renderer(ctx: &Context, state: &egui_wgpu::RenderState) {
    #[cfg(windows)]
    crate::app::markdown_print::install(ctx, state);
    let enabled = state.renderer.read().supports_lcd_text();
    ctx.data_mut(|data| {
        data.insert_temp(Id::new("lcd-text-renderer"), enabled);
        data.insert_temp(
            Id::new("text-coverage-renderer"),
            Arc::downgrade(&state.renderer),
        );
    });
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        let options = &mut style.visuals.text_options;
        options.lcd_subpixel = enabled;
        if enabled {
            // The LCD compositor applies contrast to raw coverage.
            options.color_transfer_function = egui::epaint::FontColorTransferFunction::Off;
        }
    });
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.visuals.text_options.lcd_subpixel = enabled
    });
}
pub(crate) fn set_grayscale(ctx: &Context, enabled: bool) {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        ctx.style_mut_of(theme, |style| {
            let options = &mut style.visuals.text_options;
            options.lcd_grayscale = options.lcd_subpixel && enabled;
        });
    }
    let renderer = ctx
        .data(|data| data.get_temp::<RendererRef>(Id::new("text-coverage-renderer")))
        .and_then(|renderer| renderer.upgrade());
    if let Some(renderer) = renderer {
        #[cfg(windows)]
        let gamma = if enabled {
            1.0
        } else {
            egui::epaint::text::lcd_gamma()
        };
        #[cfg(not(windows))]
        let gamma = 1.0;
        renderer.write().set_font_gamma(gamma);
    }
    ctx.request_repaint();
}

fn key() -> Id {
    Id::new("tomato-font-definitions")
}
pub(crate) fn install(ctx: &Context, mut fonts: FontDefinitions) {
    configure_rasterization(&mut fonts);
    ctx.data_mut(|data| data.insert_temp(key(), Arc::new(Mutex::new(fonts.clone()))));
    ctx.set_fonts(fonts);
}
pub(crate) fn update(ctx: &Context, change: impl FnOnce(&mut FontDefinitions)) {
    let shared = ctx
        .data(|data| data.get_temp::<Shared>(key()))
        .unwrap_or_else(|| {
            let baseline = ctx.fonts(|fonts| fonts.definitions().clone());
            ctx.data_mut(|data| {
                data.get_temp_mut_or_insert_with(key(), || Arc::new(Mutex::new(baseline)))
                    .clone()
            })
        });
    if let Ok(mut fonts) = shared.lock() {
        change(&mut fonts);
        configure_rasterization(&mut fonts);
        ctx.set_fonts(fonts.clone());
    }
}
fn configure_rasterization(fonts: &mut FontDefinitions) {
    if !cfg!(windows) {
        return;
    }
    let smooth = egui::epaint::text::HintingTarget::Smooth(egui::epaint::text::SmoothHinting {
        light: false,
        symmetric_rendering: false,
        preserve_linear_metrics: false,
    });
    for (name, data) in &mut fonts.font_data {
        // CJK strokes need full pixel fitting at the original 12–13 pt sizes.
        // Mono selects the TrueType instruction target; the glyph atlas still
        // uses antialiasing, and shaping advances/layout remain unchanged.
        let target = if name == "cjk" || name.ends_with("-cjk") {
            egui::epaint::text::HintingTarget::Mono
        } else {
            smooth
        };
        // System and user text fonts are owned. Leave bundled symbol faces alone.
        if name.contains("fontawesome") || !matches!(data.font, std::borrow::Cow::Owned(_)) {
            continue;
        }
        // JavaFX lays out ordinary Latin/CJK labels with nominal advances.
        let simple_latin_spacing = matches!(name.as_str(), "cjk" | "md-bold-cjk");
        if data.tweak.hinting == Some(true)
            && data.tweak.subpixel_binning == Some(false)
            && data.tweak.hinting_target == target
            && data.tweak.simple_latin_spacing == simple_latin_spacing
        {
            continue;
        }
        // Already-configured shared fonts stay shared when an asynchronous font is
        // added; this must not copy the complete CJK font on every update.
        let data = Arc::make_mut(data);
        data.tweak.hinting = Some(true);
        data.tweak.subpixel_binning = Some(false);
        data.tweak.hinting_target = target;
        data.tweak.simple_latin_spacing = simple_latin_spacing;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn java_and_markdown_bold_share_one_lazy_cjk_face() -> anyhow::Result<()> {
        if !cfg!(windows) {
            return Ok(());
        }
        let ctx = Context::default();
        install(&ctx, FontDefinitions::default());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let mut output = ctx.run_ui(Default::default(), |ui| {
                let font = crate::theme::java_bold_font(ui.ctx(), 14.);
                ui.label(egui::RichText::new("选择要导入的项目").font(font));
            });
            output.textures_delta.clear();
            let ready =
                ctx.fonts(|fonts| fonts.definitions().font_data.contains_key("md-bold-cjk"));
            if ready {
                break;
            }
            anyhow::ensure!(
                std::time::Instant::now() < deadline,
                "bold font load timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let owner = ctx.fonts(|fonts| {
            let definitions = fonts.definitions();
            let data = definitions.font_data["md-bold-cjk"].clone();
            assert_eq!(
                data.index, 1,
                "use the same YaHei UI face as regular Java UI text"
            );
            assert_eq!(
                definitions.families[&egui::FontFamily::Name("java-ui-bold".into())][0],
                "md-bold-cjk"
            );
            data
        });
        bold::ensure(&ctx);
        let mut output = ctx.run_ui(Default::default(), |ui| {
            ui.label("next frame");
        });
        output.textures_delta.clear();
        ctx.fonts(|fonts| {
            assert!(Arc::ptr_eq(
                &fonts.definitions().font_data["md-bold-cjk"],
                &owner
            ))
        });
        Ok(())
    }
    #[cfg(windows)]
    #[test]
    fn native_lcd_font_cache_releases_its_shared_byte_owner() -> anyhow::Result<()> {
        let data = Arc::new(egui::FontData::from_owned(std::fs::read(
            "C:/Windows/Fonts/msyh.ttc",
        )?));
        let weak = Arc::downgrade(&data);
        let ctx = Context::default();
        ctx.set_theme(egui::Theme::Light);
        ctx.style_mut_of(egui::Theme::Light, |style| {
            style.visuals.text_options.lcd_subpixel = true
        });
        let mut definitions = FontDefinitions::empty();
        definitions.font_data.insert("native".into(), data.clone());
        definitions
            .families
            .insert(egui::FontFamily::Proportional, vec!["native".into()]);
        ctx.set_fonts(definitions);
        let mut output = ctx.run_ui(Default::default(), |ui| {
            ui.label("本地终端 native font cache");
        });
        output.textures_delta.clear();
        assert!(Arc::strong_count(&data) > 1);
        drop(output);
        drop(ctx);
        assert_eq!(
            Arc::strong_count(&data),
            1,
            "native file loader retained font bytes after cache release"
        );
        drop(data);
        assert!(weak.upgrade().is_none());
        Ok(())
    }
    #[test]
    fn lcd_masks_preserve_layout_and_atlas_budget_across_dpis_and_fallback() -> anyhow::Result<()> {
        let ctx = Context::default();
        ctx.set_theme(egui::Theme::Light);
        let mut definitions = FontDefinitions::default();
        if cfg!(windows) {
            let bytes = std::fs::read("C:/Windows/Fonts/msyh.ttc")?;
            definitions
                .font_data
                .insert("cjk".into(), egui::FontData::from_owned(bytes).into());
            definitions
                .families
                .insert(egui::FontFamily::Proportional, vec!["cjk".into()]);
        }
        install(&ctx, definitions);
        for dpi in [1., 1.25, 1.5, 2.] {
            let mut previous = None;
            for (lcd, grayscale) in [
                (false, false),
                (true, false),
                (true, true),
                (true, false),
                (false, false),
            ] {
                ctx.style_mut_of(egui::Theme::Light, |style| {
                    style.visuals.text_options.lcd_subpixel = lcd;
                });
                set_grayscale(&ctx, grayscale);
                let mut input = egui::RawInput::default();
                input
                    .viewports
                    .get_mut(&egui::ViewportId::ROOT)
                    .unwrap()
                    .native_pixels_per_point = Some(dpi);
                let mut geometry = None;
                let mut output = ctx.run_ui(input, |ui| {
                    let galley = ui.painter().layout_no_wrap(
                        "清晰度测试 mmm iii 192.168.0.11".into(),
                        egui::FontId::proportional(12.),
                        egui::Color32::BLACK,
                    );
                    geometry = Some((
                        galley.size(),
                        galley.rows[0]
                            .glyphs
                            .iter()
                            .map(|g| g.pos)
                            .collect::<Vec<_>>(),
                    ));
                });
                output.textures_delta.clear();
                let geometry = geometry.unwrap();
                if let Some(previous) = &previous {
                    assert_eq!(
                        &geometry, previous,
                        "LCD must not change wrapping, advances or caret positions"
                    );
                }
                previous = Some(geometry);
                let image = ctx.fonts(|fonts| fonts.image());
                let chromatic = image
                    .pixels
                    .iter()
                    .any(|pixel| pixel.r() != pixel.g() || pixel.g() != pixel.b());
                assert_eq!(
                    chromatic,
                    lcd && !grayscale,
                    "DPI {dpi}: actual cached glyph coverage"
                );
                assert!(image.size[0] <= 2048 && image.size[1] <= 2048);
                assert!(
                    image
                        .pixels
                        .iter()
                        .all(|p| p.r() <= p.a() && p.g() <= p.a() && p.b() <= p.a())
                );
            }
        }
        Ok(())
    }
    #[cfg(windows)]
    #[test]
    fn native_grayscale_uses_its_own_coverage_without_moving_glyphs() -> anyhow::Result<()> {
        type Coverage = std::collections::BTreeMap<(usize, i32, i32), u8>;
        let ctx = Context::default();
        ctx.set_theme(egui::Theme::Light);
        let mut definitions = FontDefinitions::empty();
        definitions.font_data.insert(
            "cjk".into(),
            java_face(std::fs::read("C:/Windows/Fonts/msyh.ttc")?).into(),
        );
        definitions
            .families
            .insert(egui::FontFamily::Proportional, vec!["cjk".into()]);
        install(&ctx, definitions);
        for dpi in [1., 1.25, 1.5, 2.] {
            let mut baseline: Option<(egui::Vec2, Vec<egui::Pos2>, Coverage)> = None;
            for grayscale in [false, true, false] {
                ctx.style_mut_of(egui::Theme::Light, |style| {
                    style.visuals.text_options.lcd_subpixel = true;
                });
                set_grayscale(&ctx, grayscale);
                let mut input = egui::RawInput::default();
                input
                    .viewports
                    .get_mut(&egui::ViewportId::ROOT)
                    .unwrap()
                    .native_pixels_per_point = Some(dpi);
                let mut galley = None;
                let mut output = ctx.run_ui(input, |ui| {
                    galley = Some(ui.painter().layout_no_wrap(
                        "久拾体科 小主机 PID 0123456".into(),
                        egui::FontId::proportional(12.),
                        egui::Color32::BLACK,
                    ));
                });
                output.textures_delta.clear();
                let galley = galley.unwrap();
                let positions: Vec<_> = galley.rows[0].glyphs.iter().map(|g| g.pos).collect();
                let image = ctx.fonts(|fonts| fonts.image());
                let mut coverage = std::collections::BTreeMap::new();
                for (index, glyph) in galley.rows[0].glyphs.iter().enumerate() {
                    let rect = glyph.uv_rect;
                    for y in rect.min[1]..rect.max[1] {
                        for x in rect.min[0]..rect.max[0] {
                            let value = image[(usize::from(x), usize::from(y))].g();
                            if value > 0 {
                                coverage.insert(
                                    (
                                        index,
                                        (rect.offset.x * dpi).round() as i32
                                            + i32::from(x - rect.min[0]),
                                        (rect.offset.y * dpi).round() as i32
                                            + i32::from(y - rect.min[1]),
                                    ),
                                    value,
                                );
                            }
                        }
                    }
                }
                assert!(
                    coverage.values().any(|&value| value == 255),
                    "DPI {dpi}: solid stroke"
                );
                assert!(
                    coverage.values().any(|&value| value > 0 && value < 255),
                    "DPI {dpi}: antialiasing"
                );
                if let Some((size, positions_before, rgb_green)) = &baseline {
                    assert_eq!(galley.size(), *size);
                    assert_eq!(&positions, positions_before);
                    if grayscale {
                        let differences = coverage
                            .iter()
                            .filter(|(point, value)| rgb_green.get(*point) != Some(*value))
                            .count();
                        assert!(
                            differences >= 10,
                            "DPI {dpi}: native grayscale must replace the RGB stripe filter"
                        );
                    } else {
                        assert_eq!(
                            &coverage, rgb_green,
                            "DPI {dpi}: switching back must restore RGB text exactly"
                        );
                    }
                } else {
                    baseline = Some((galley.size(), positions, coverage));
                }
            }
        }
        Ok(())
    }
    #[test]
    fn actual_font_renderers_share_owned_bytes_across_cache_lifetimes() {
        let defaults = FontDefinitions::default();
        let original = defaults.font_data.values().next().unwrap();
        let data = Arc::new(egui::FontData::from_owned(original.font.as_ref().to_vec()));
        let mut definitions = FontDefinitions::empty();
        definitions.font_data.insert("owned".into(), data.clone());
        definitions
            .families
            .insert(egui::FontFamily::Proportional, vec!["owned".into()]);
        assert_eq!(Arc::strong_count(&data), 2);
        let first = egui::epaint::text::Fonts::new(Default::default(), definitions);
        assert_eq!(
            Arc::strong_count(&data),
            3,
            "font face must share the original byte owner"
        );
        let second =
            egui::epaint::text::Fonts::new(Default::default(), first.definitions().clone());
        assert_eq!(
            Arc::strong_count(&data),
            5,
            "a second DPI cache must not duplicate font bytes"
        );
        drop(second);
        assert_eq!(Arc::strong_count(&data), 3);
        drop(first);
        assert_eq!(Arc::strong_count(&data), 1);
    }
    #[test]
    fn additions_before_the_next_frame_do_not_replace_each_other_or_duplicate_font_bytes() {
        let ctx = Context::default();
        let fonts = FontDefinitions::default();
        let original = fonts.font_data.values().next().unwrap().clone();
        install(&ctx, fonts);
        for name in ["first-worker", "second-worker"] {
            update(&ctx, |fonts| {
                fonts.families.insert(
                    egui::FontFamily::Name(name.into()),
                    fonts.families[&egui::FontFamily::Proportional].clone(),
                );
            });
        }
        let mut output = ctx.run_ui(Default::default(), |ui| {
            ui.label("merged fonts");
        });
        output.textures_delta.clear();
        ctx.fonts(|fonts| {
            assert!(
                fonts
                    .definitions()
                    .families
                    .contains_key(&egui::FontFamily::Name("first-worker".into()))
            );
            assert!(
                fonts
                    .definitions()
                    .families
                    .contains_key(&egui::FontFamily::Name("second-worker".into()))
            );
            assert!(
                fonts
                    .definitions()
                    .font_data
                    .values()
                    .any(|font| Arc::ptr_eq(font, &original))
            );
        });
    }
}
