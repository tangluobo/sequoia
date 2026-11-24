//! Java FlowPane geometry shared by painting, hit testing and keyboard navigation.
use egui::{FontId, Rect, Ui, Vec2, pos2, vec2};
pub(super) struct Grid {
    pub columns: usize,
    pub step: Vec2,
    height: f32,
}
impl Grid {
    pub fn new(ui: &Ui, width: f32) -> Self {
        let height = 64. + ui.fonts_mut(|fonts| fonts.row_height(&FontId::proportional(11.)));
        Self {
            columns: ((width - 24. + 8.) / 98.).floor().max(1.) as usize,
            step: vec2(98., height + 8.),
            height,
        }
    }
    pub fn content_height(&self, count: usize) -> f32 {
        let rows = count.div_ceil(self.columns);
        24. + (rows as f32 * self.step.y - 8.).max(0.)
    }
    pub fn cell(&self, bounds: Rect, index: usize) -> Rect {
        Rect::from_min_size(
            pos2(
                bounds.left() + 12. + (index % self.columns) as f32 * self.step.x,
                bounds.top() + 12. + (index / self.columns) as f32 * self.step.y,
            ),
            vec2(90., self.height),
        )
    }
}
