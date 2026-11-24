//! Java's global table font and its fixed row metrics; toolbar fonts stay independent.
use egui::{Context, FontFamily, FontId, Id};
use tomato_core::preferences::TableFont;
#[derive(Clone, Default)]
struct Style {
    font: TableFont,
    designer_applied: bool,
}
pub(crate) fn sync(ctx: &Context, font: TableFont, apply: bool) {
    ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<Style>(Id::new("table-style"));
        s.font = font;
        s.designer_applied |= apply;
    });
}
fn style(ctx: &Context) -> Style {
    ctx.data(|d| d.get_temp::<Style>(Id::new("table-style")))
        .unwrap_or_default()
}
pub(crate) fn font(ctx: &Context, bold: bool) -> FontId {
    let s = style(ctx);
    crate::font_assets::table::ensure(ctx, &s.font.name);
    let custom = !crate::font_assets::table::default_face(&s.font.name);
    let name = match (custom, bold) {
        (true, true) => "table-user-bold",
        (true, false) => "table-user",
        (false, true) => "table-bold",
        _ => "table",
    };
    let family = FontFamily::Name(name.into());
    FontId::new(
        f32::from(s.font.size),
        if ctx.fonts(|f| f.definitions().families.contains_key(&family)) {
            family
        } else {
            FontFamily::Proportional
        },
    )
}
pub(crate) fn row_height(ctx: &Context) -> f32 {
    f32::from(style(ctx).font.size) + 18.
}
fn customized(ctx: &Context) -> bool {
    let s = style(ctx);
    s.designer_applied || s.font != TableFont::default()
}
pub(crate) fn field_font(ctx: &Context) -> FontId {
    if customized(ctx) {
        font(ctx, false)
    } else {
        crate::native_dialog::font(ctx, 12.)
    }
}
pub(crate) fn field_height(ctx: &Context) -> f32 {
    if customized(ctx) {
        row_height(ctx)
    } else {
        30.
    }
}

pub(crate) fn field_bold_font(ctx: &Context) -> FontId {
    if customized(ctx) {
        return font(ctx, true);
    }
    let family = FontFamily::Name("table-bold".into());
    if ctx.fonts(|f| f.definitions().families.contains_key(&family)) {
        FontId::new(12., family)
    } else {
        field_font(ctx)
    }
}
