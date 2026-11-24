//! Positions reference visible UTF-8 text; scrolling never snapshots the document.
use std::ops::Range;
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation as _};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Point {
    pub row: usize,
    pub byte: usize,
}
#[derive(Clone, Copy, Default)]
enum Unit {
    #[default]
    Character,
    Word,
    Line,
}
#[derive(Default)]
pub struct Selection {
    pub anchor: Point,
    pub head: Point,
    pub dragging: bool,
    pub preferred_x: Option<f64>,
    pub blink_start: f64,
    unit: Unit,
    base: (Point, Point),
    click: Option<(f64, egui::Pos2, u8)>,
}
impl Selection {
    pub fn range(&self) -> Option<(Point, Point)> {
        (self.anchor != self.head).then(|| (self.anchor.min(self.head), self.anchor.max(self.head)))
    }
    pub fn move_to(&mut self, point: Point, extend: bool) {
        if !extend {
            self.anchor = point;
        }
        self.head = point;
        self.preferred_x = None;
    }
    pub fn all(&mut self, last: Point) {
        self.anchor = Point::default();
        self.head = last;
    }
    pub fn row_range(&self, row: usize, length: usize) -> Option<Range<usize>> {
        let (start, end) = self.range()?;
        if row < start.row || row > end.row {
            return None;
        }
        Some(
            (if row == start.row {
                start.byte.min(length)
            } else {
                0
            })..if row == end.row {
                end.byte.min(length)
            } else {
                length
            },
        )
    }
    pub fn press(
        &mut self,
        point: Point,
        line: &str,
        shift: bool,
        time: f64,
        position: egui::Pos2,
        next_line: bool,
    ) {
        let count = self
            .click
            .filter(|(last, pos, _)| time - *last < 0.4 && pos.distance(position) < 5.)
            .map_or(1, |(_, _, count)| count % 3 + 1);
        self.click = Some((time, position, count));
        self.unit = if shift {
            Unit::Character
        } else {
            match count {
                2 => Unit::Word,
                3 => Unit::Line,
                _ => Unit::Character,
            }
        };
        let range = self.unit_range(point, line, next_line);
        if shift {
            self.head = point;
        } else {
            self.anchor = range.0;
            self.head = range.1;
        }
        self.base = if shift {
            (self.anchor, self.anchor)
        } else {
            (self.anchor, self.head)
        };
        self.dragging = true;
        self.preferred_x = None;
        self.blink_start = time;
    }
    pub fn drag_to(&mut self, point: Point, line: &str, next_line: bool) {
        let range = self.unit_range(point, line, next_line);
        if point < self.base.0 {
            self.anchor = self.base.1;
            self.head = range.0;
        } else {
            self.anchor = self.base.0;
            self.head = range.1;
        }
    }
    fn unit_range(&self, point: Point, line: &str, next_line: bool) -> (Point, Point) {
        if matches!(self.unit, Unit::Line) && next_line {
            return (
                Point {
                    row: point.row,
                    byte: 0,
                },
                Point {
                    row: point.row + 1,
                    byte: 0,
                },
            );
        }
        let range = match self.unit {
            Unit::Character => point.byte..point.byte,
            Unit::Word => word(line, point.byte),
            Unit::Line => 0..line.len(),
        };
        (
            Point {
                row: point.row,
                byte: range.start,
            },
            Point {
                row: point.row,
                byte: range.end,
            },
        )
    }
}
pub fn floor(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    let mut cursor = GraphemeCursor::new(byte, text.len(), true);
    if cursor.is_boundary(text, 0) == Ok(true) {
        byte
    } else {
        cursor.prev_boundary(text, 0).ok().flatten().unwrap_or(0)
    }
}
pub fn previous(text: &str, byte: usize) -> usize {
    GraphemeCursor::new(byte.min(text.len()), text.len(), true)
        .prev_boundary(text, 0)
        .ok()
        .flatten()
        .unwrap_or(0)
}
pub fn next(text: &str, byte: usize) -> usize {
    GraphemeCursor::new(byte.min(text.len()), text.len(), true)
        .next_boundary(text, 0)
        .ok()
        .flatten()
        .unwrap_or(text.len())
}
pub fn word(text: &str, byte: usize) -> Range<usize> {
    let byte = byte.min(text.len().saturating_sub(1));
    text.split_word_bound_indices()
        .find(|(start, s)| *start + s.len() > byte)
        .map_or(0..0, |(start, s)| start..start + s.len())
}
pub fn previous_word(text: &str, byte: usize) -> usize {
    text.unicode_word_indices()
        .take_while(|(start, _)| *start < byte)
        .last()
        .map_or(0, |(start, _)| start)
}
pub fn next_word(text: &str, byte: usize) -> usize {
    text.unicode_word_indices()
        .map(|(start, s)| start + s.len())
        .find(|end| *end > byte)
        .unwrap_or(text.len())
}
pub fn next_word_start(text: &str, byte: usize) -> usize {
    text.unicode_word_indices()
        .map(|(start, _)| start)
        .find(|start| *start > byte)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn double_word_drag_triple_line_and_shift_drag_keep_the_original_anchor() {
        let mut selection = Selection::default();
        let p = Point { row: 1, byte: 2 };
        let pos = egui::pos2(30., 20.);
        selection.press(p, "hello world", false, 1., pos, true);
        selection.press(p, "hello world", false, 1.1, pos, true);
        assert_eq!(
            selection.range(),
            Some((Point { row: 1, byte: 0 }, Point { row: 1, byte: 5 }))
        );
        selection.drag_to(Point { row: 2, byte: 8 }, "next world", true);
        assert_eq!(selection.head, Point { row: 2, byte: 10 });
        selection.press(p, "hello world", false, 1.2, pos, true);
        assert_eq!(
            selection.range(),
            Some((Point { row: 1, byte: 0 }, Point { row: 2, byte: 0 }))
        );
        selection.move_to(Point { row: 1, byte: 3 }, false);
        selection.press(Point { row: 5, byte: 4 }, "long text", true, 2., pos, true);
        selection.drag_to(Point { row: 0, byte: 1 }, "first", true);
        assert_eq!(selection.anchor, Point { row: 1, byte: 3 });
        assert_eq!(selection.head, Point { row: 0, byte: 1 });
    }
    #[test]
    fn graphemes_words_and_reversed_cross_row_ranges_preserve_unicode() {
        let text = "A👩‍💻e\u{301}中文";
        let a = next(text, 1);
        assert_eq!(&text[1..a], "👩‍💻");
        let b = next(text, a);
        assert_eq!(&text[a..b], "e\u{301}");
        assert_eq!(previous(text, b), a);
        assert_eq!(floor(text, a + 1), a);
        assert_eq!(word("hello 世界", 2), 0..5);
        let s = Selection {
            anchor: Point { row: 8, byte: 3 },
            head: Point { row: 2, byte: 1 },
            ..Default::default()
        };
        assert_eq!(s.row_range(2, 9), Some(1..9));
        assert_eq!(s.row_range(5, 8), Some(0..8));
        assert_eq!(s.row_range(8, 10), Some(0..3));
        assert_eq!(s.row_range(9, 2), None);
    }
}
