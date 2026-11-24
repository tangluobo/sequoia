//! Exact metrics and colors from tomato-view.fxml and connect-tree.css.
use egui::{Color32, FontId, Stroke, TextStyle};
pub const NAV_WIDTH: f32 = 62.0;
pub const SIDEBAR_WIDTH: f32 = 240.0;
pub const HEADER_HEIGHT: f32 = 52.0;
pub const TREE_ROW_HEIGHT: f32 = 35.0;
pub const TAB_HEIGHT: f32 = 37.0;
pub const NAV_BACKGROUND: Color32 = Color32::from_rgb(236, 236, 234);
pub const DIVIDER: Color32 = Color32::from_rgb(229, 229, 229);
pub const HEADER_DIVIDER: Color32 = Color32::from_rgb(217, 217, 215);
pub const GREEN: Color32 = Color32::from_rgb(7, 193, 96);
pub const TREE_SELECTED: Color32 = Color32::from_rgb(201, 235, 198);
pub const TREE_HOVER: Color32 = Color32::from_rgb(221, 242, 219);

/// Java's logical UI font keeps Latin and CJK labels on the same baseline.
/// This family reuses the already loaded font data; it adds no font allocation.
pub fn java_font(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("java-ui".into()))
}
pub(crate) fn java_bold_font(ctx: &egui::Context, size: f32) -> FontId {
    crate::font_assets::bold::ensure(ctx);
    let family = egui::FontFamily::Name("java-ui-bold".into());
    if ctx.fonts(|fonts| fonts.definitions().families.contains_key(&family)) {
        FontId::new(size, family)
    } else {
        FontId::proportional(size)
    }
}

pub fn install(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Light)).clone();
    let mut visuals = egui::Visuals::light();
    visuals.text_options.lcd_subpixel = crate::font_assets::lcd_enabled(ctx);
    visuals.text_options.font_hinting = true;
    if cfg!(windows) {
        visuals.text_options.subpixel_binning = false;
    }
    // The Java application does not globally zoom on terminal/editor shortcuts.
    // Native monitor DPI remains managed by winit for each viewport.
    ctx.options_mut(|options| options.zoom_with_keyboard = false);
    visuals.panel_fill = Color32::WHITE;
    visuals.window_fill = Color32::WHITE;
    visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    visuals.override_text_color = Some(Color32::from_rgb(51, 51, 51));
    visuals.selection.bg_fill = TREE_SELECTED;
    visuals.selection.stroke = Stroke::new(1.0, Color32::BLACK);
    visuals.extreme_bg_color = Color32::WHITE;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, DIVIDER);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(244, 244, 244);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(244, 244, 244);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_gray(208));
    visuals.widgets.hovered.bg_fill = TREE_HOVER;
    visuals.widgets.hovered.weak_bg_fill = TREE_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, GREEN);
    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.interact_size.y = 28.0;
    for (kind, size) in [
        (TextStyle::Body, 13.0),
        (TextStyle::Button, 13.0),
        (TextStyle::Small, 11.0),
        (TextStyle::Heading, 16.0),
    ] {
        style.text_styles.insert(kind, FontId::proportional(size));
    }
    ctx.set_visuals(style.visuals.clone());
    ctx.set_style_of(egui::Theme::Light, style);
}

pub fn green_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(label).color(Color32::WHITE))
        .fill(Color32::from_rgb(76, 175, 80))
        .stroke(Stroke::NONE)
        .corner_radius(4)
}
