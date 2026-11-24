//! Mouse-transparent progress near the original caret/selection anchor.
use egui::*;
pub(super) fn show(ui: &mut Ui, bounds: Rect, state: &crate::tabs::completion::State) {
    let inset = 6.;
    let galley = ui.fonts_mut(|fonts| {
        fonts.layout_no_wrap(
            "AI 正在生成 SQL...".into(),
            FontId::proportional(12.),
            Color32::from_gray(51),
        )
    });
    let size = vec2(8. + 18. + 7. + galley.size().x + 10., 30.);
    let mut anchor = state
        .galley
        .as_ref()
        .map(|galley| {
            galley
                .pos_from_cursor(state.selection.primary)
                .translate(state.galley_pos.to_vec2())
        })
        .unwrap_or(Rect::from_min_size(bounds.min, Vec2::ZERO));
    anchor = anchor.intersect(bounds);
    let mut x = anchor.right() + 8.;
    let mut y = anchor.top() + ((anchor.height() - size.y) / 2.).max(0.);
    if x + size.x > bounds.right() - inset {
        x = anchor.left();
        y = anchor.bottom() + 6.;
    }
    if y + size.y > bounds.bottom() - inset {
        y = anchor.top() - size.y - 6.;
    }
    let position = pos2(
        x.clamp(
            bounds.left() + inset,
            (bounds.right() - size.x - inset).max(bounds.left() + inset),
        ),
        y.clamp(
            bounds.top() + inset,
            (bounds.bottom() - size.y - inset).max(bounds.top() + inset),
        ),
    );
    let rect = Rect::from_min_size(position, size);
    let shadow = egui::epaint::Shadow {
        offset: [0, 2],
        blur: 8,
        spread: 0,
        color: Color32::from_black_alpha(41),
    };
    ui.painter().add(shadow.as_shape(rect, 5));
    ui.painter().rect(
        rect,
        5.,
        Color32::from_white_alpha(245),
        Stroke::new(1., Color32::from_rgb(190, 220, 250)),
        StrokeKind::Inside,
    );
    let spinner = Rect::from_min_size(position + vec2(8., 6.), vec2(18., 18.));
    ui.scope_builder(UiBuilder::new().max_rect(spinner), |ui| {
        ui.add(Spinner::new().size(18.));
    });
    ui.painter().galley(
        position + vec2(33., (size.y - galley.size().y) / 2.),
        galley,
        Color32::from_gray(51),
    );
}
