//! Image-space selection and all eight Java CropOverlay resize handles.
#[derive(Clone, Copy, Debug)]
pub struct Crop {
    pub image: [u32; 2],
    pub rect: [u32; 4],
    pub square: bool,
}
impl Default for Crop {
    fn default() -> Self {
        Self {
            image: [0; 2],
            rect: [0; 4],
            square: true,
        }
    }
}
pub fn round(value: f64) -> i32 {
    (value + 0.5).floor() as i32
}
impl Crop {
    pub fn reset(&mut self, image: [u32; 2]) {
        self.image = image;
        let [w, h] = image;
        self.rect = if self.square {
            let side = w.min(h);
            [(w - side) / 2, (h - side) / 2, side, side]
        } else {
            [0, 0, w, h]
        };
    }
    pub fn margins(&self) -> [u32; 4] {
        let [x, y, w, h] = self.rect;
        [
            x,
            y,
            self.image[0].saturating_sub(x + w),
            self.image[1].saturating_sub(y + h),
        ]
    }
    pub fn set_margins(&mut self, [left, top, right, bottom]: [u32; 4]) {
        let [w, h] = self.image;
        if w == 0 || h == 0 {
            return;
        }
        let left = left.min(w - 4.min(w));
        let top = top.min(h - 4.min(h));
        let right = right.min(w - 4.min(w) - left);
        let bottom = bottom.min(h - 4.min(h) - top);
        let (width, height) = (w - left - right, h - top - bottom);
        self.rect = if self.square {
            let side = width.min(height);
            [
                left + (width - side) / 2,
                top + (height - side) / 2,
                side,
                side,
            ]
        } else {
            [left, top, width, height]
        };
    }
    pub fn drag(&mut self, start: [u32; 4], handle: u8, dx: i32, dy: i32) {
        let [iw, ih] = self.image;
        if iw == 0 || ih == 0 {
            return;
        }
        let dx = dx.clamp(-(iw as i32) * 2, iw as i32 * 2);
        let dy = dy.clamp(-(ih as i32) * 2, ih as i32 * 2);
        let [x, y, w, h] = start.map(|v| v as i32);
        if handle == 8 {
            self.rect = [
                (x + dx).clamp(0, iw as i32 - w) as u32,
                (y + dy).clamp(0, ih as i32 - h) as u32,
                w as u32,
                h as u32,
            ];
            return;
        }
        if self.square {
            let (x, y, w, h) = (f64::from(x), f64::from(y), f64::from(w), f64::from(h));
            let points = [
                (x, y),
                (x + w / 2., y),
                (x + w, y),
                (x + w, y + h / 2.),
                (x + w, y + h),
                (x + w / 2., y + h),
                (x, y + h),
                (x, y + h / 2.),
            ];
            let index = usize::from(handle.min(7));
            let (hx, hy) = points[index];
            let (ax, ay) = points[(index + 4) % 8];
            let side = round(
                (hx + f64::from(dx) - ax)
                    .abs()
                    .max((hy + f64::from(dy) - ay).abs()),
            )
            .clamp(4.min(iw.min(ih)) as i32, iw.min(ih) as i32);
            let s = f64::from(side);
            let (xx, yy) = match index {
                0 => (ax - s, ay - s),
                1 => (ax - s / 2., ay - s),
                2 => (ax, ay - s),
                3 => (ax, ay - s / 2.),
                4 => (ax, ay),
                5 => (ax - s / 2., ay),
                6 => (ax - s, ay),
                _ => (ax - s, ay - s / 2.),
            };
            self.rect = [
                round(xx).clamp(0, iw as i32 - side) as u32,
                round(yy).clamp(0, ih as i32 - side) as u32,
                side as u32,
                side as u32,
            ];
        } else {
            let (mut left, mut top, mut right, mut bottom) = (x, y, x + w, y + h);
            let minw = 4.min(iw) as i32;
            let minh = 4.min(ih) as i32;
            if matches!(handle, 0 | 6 | 7) {
                left = (x + dx).clamp(0, right - minw);
            }
            if matches!(handle, 2..=4) {
                right = (x + w + dx).clamp(x + minw, iw as i32);
            }
            if matches!(handle, 0..=2) {
                top = (y + dy).clamp(0, bottom - minh);
            }
            if matches!(handle, 4..=6) {
                bottom = (y + h + dy).clamp(y + minh, ih as i32);
            }
            self.rect = [
                left as u32,
                top as u32,
                (right - left) as u32,
                (bottom - top) as u32,
            ];
        }
    }
}
