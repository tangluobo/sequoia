//! RGB coverage packed into the existing atlas; no permanent 3x atlas allocation.
use crate::{Color32, FontColorTransferFunction};
use vello_cpu::{color, kurbo};

pub(super) fn rasterize(
    path: &kurbo::BezPath,
    bounds: kurbo::Rect,
    transfer: FontColorTransferFunction,
    grayscale: bool,
) -> Vec<Color32> {
    let width = bounds.width() as u16;
    let height = bounds.height() as u16;
    let samples = width * 3;
    let mut ctx = vello_cpu::RenderContext::new(samples, height);
    ctx.set_transform(kurbo::Affine::new([
        3.,
        0.,
        0.,
        1.,
        -bounds.x0 * 3.,
        -bounds.y0,
    ]));
    ctx.set_paint(color::OpaqueColor::<color::Srgb>::WHITE);
    ctx.fill_path(path);
    let mut pixmap = vello_cpu::Pixmap::new(samples, height);
    ctx.render(&mut pixmap, &mut vello_cpu::Resources::new());
    let bytes = pixmap.data_as_u8_slice();
    let mut pixels = Vec::with_capacity(usize::from(width) * usize::from(height));
    for row in bytes.chunks_exact(usize::from(samples) * 4) {
        for x in 0..usize::from(width) {
            let channels = [0, 1, 2].map(|channel| {
                let position = (3 * x + channel) as i32;
                // Symmetric five-tap LCD filter. Each tap is one subpixel;
                // padding in the glyph bounds prevents clipping the fringes.
                let coverage: f32 = [-2, -1, 0, 1, 2]
                    .into_iter()
                    .zip([1., 4., 6., 4., 1.])
                    .map(|(offset, weight)| {
                        let p = position + offset;
                        if (0..i32::from(samples)).contains(&p) {
                            f32::from(row[p as usize * 4 + 3]) * weight / (16. * 255.)
                        } else {
                            0.
                        }
                    })
                    .sum();
                (transfer.alpha_from_coverage(coverage) * 255.).round() as u8
            });
            pixels.push(coverage_color(channels, grayscale));
        }
    }
    pixels
}

pub(super) fn coverage_color([r, g, b]: [u8; 3], grayscale: bool) -> Color32 {
    if grayscale {
        // The center sample avoids extending an edge into the neighboring
        // pixel merely because the outer subpixel touched it.
        Color32::from_rgba_premultiplied(g, g, g, g)
    } else {
        Color32::from_rgba_premultiplied(r, g, b, r.max(g).max(b))
    }
}
