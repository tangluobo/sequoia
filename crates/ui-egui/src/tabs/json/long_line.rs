//! Sparse glyph positions let the renderer shape only the horizontal viewport.
use std::collections::HashMap;
#[derive(Clone, Copy)]
pub struct Prefix {
    pub key: bool,
    pub escaped: bool,
}
struct Checkpoint {
    byte: u32,
    x: f64,
    prefix: Option<Prefix>,
}
pub struct LongLine {
    points: Vec<Checkpoint>,
    pub width: f64,
}
impl LongLine {
    pub fn new(text: &str, mut advance: impl FnMut(char) -> f32) -> Self {
        let mut points = vec![Checkpoint {
            byte: 0,
            x: 0.,
            prefix: None,
        }];
        let mut widths = HashMap::new();
        let mut x = 0.;
        let mut prefix: Option<Prefix> = None;
        for (byte, c) in text.char_indices() {
            if byte - points.last().map_or(0, |p| p.byte as usize) >= 256 {
                points.push(Checkpoint {
                    byte: byte as u32,
                    x,
                    prefix,
                });
            }
            if let Some(state) = &mut prefix {
                if c == '"' && !state.escaped {
                    prefix = None;
                } else {
                    state.escaped = c == '\\' && !state.escaped;
                }
            } else if c == '"' {
                let mut escaped = false;
                let mut end = text.len();
                for (offset, b) in text.as_bytes()[byte + 1..].iter().enumerate() {
                    if *b == b'"' && !escaped {
                        end = byte + offset + 2;
                        break;
                    }
                    escaped = *b == b'\\' && !escaped;
                }
                prefix = Some(Prefix {
                    key: text[end..].trim_start().starts_with(':'),
                    escaped: false,
                });
            }
            x += f64::from(*widths.entry(c).or_insert_with(|| advance(c)));
        }
        Self { points, width: x }
    }
    pub fn window<'a>(
        &self,
        text: &'a str,
        left: f64,
        width: f64,
    ) -> (&'a str, f64, Option<Prefix>) {
        let first = self
            .points
            .partition_point(|p| p.x < left - 32.)
            .saturating_sub(1);
        let end = self
            .points
            .partition_point(|p| p.x <= left + width.max(0.) + 32.);
        let point = &self.points[first];
        let mut last = self
            .points
            .get(end)
            .map_or(text.len(), |p| p.byte as usize)
            .max(point.byte as usize);
        last = last.min(point.byte as usize + 4096);
        while !text.is_char_boundary(last) {
            last -= 1;
        }
        (&text[point.byte as usize..last], point.x, point.prefix)
    }
    pub fn byte_at_x(&self, text: &str, x: f64, mut advance: impl FnMut(char) -> f32) -> usize {
        let point = &self.points[self.points.partition_point(|p| p.x <= x).saturating_sub(1)];
        let mut position = point.x;
        for (offset, ch) in text[point.byte as usize..].char_indices() {
            let width = f64::from(advance(ch));
            if position + width / 2. > x {
                return super::selection::floor(text, point.byte as usize + offset);
            }
            position += width;
        }
        text.len()
    }
    pub fn x_at_byte(&self, text: &str, byte: usize, mut advance: impl FnMut(char) -> f32) -> f64 {
        let byte = super::selection::floor(text, byte);
        let point = &self.points[self
            .points
            .partition_point(|p| p.byte as usize <= byte)
            .saturating_sub(1)];
        point.x
            + text[point.byte as usize..byte]
                .chars()
                .map(|ch| f64::from(advance(ch)))
                .sum::<f64>()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn megabyte_unicode_lines_keep_utf8_boundaries_and_small_render_windows() {
        let text = format!("{{\"key\":\"{}\"}}", "中文🍅\\\"".repeat(100_000));
        let line = LongLine::new(&text, |c| if c.is_ascii() { 7. } else { 12. });
        for x in [0., 1000., line.width / 2., line.width - 700.] {
            let (window, _, prefix) = line.window(&text, x, 700.);
            assert!(window.len() < 2048, "Only viewport glyphs should be shaped");
            assert!(text.contains(window));
            if x > 1000. {
                assert!(prefix.is_some_and(|p| !p.key));
            }
        }
        let zero = "\u{301}".repeat(100_000);
        let index = LongLine::new(&zero, |_| 0.);
        let (window, _, _) = index.window(&zero, 0., 700.);
        assert!(window.len() <= 4096 && zero.contains(window));
    }
}
