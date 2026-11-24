use crate::tabs::terminal::Selection;
pub(super) fn copy(
    screen: &vt100::Screen,
    view: &crate::tabs::terminal::TerminalViewState,
) -> String {
    view.selection_snapshot
        .clone()
        .unwrap_or_else(|| text(screen, view.selection.as_ref()))
}
pub(super) fn snapshot(
    screen: &vt100::Screen,
    view: &mut crate::tabs::terminal::TerminalViewState,
) {
    let text = text(screen, view.selection.as_ref());
    view.selection_snapshot = (text.len() <= 16 * 1024 * 1024).then_some(text);
}
pub(super) fn character(screen: &vt100::Screen, (row, mut col): (u16, u16)) -> Selection {
    if col > 0
        && screen
            .cell(row, col)
            .is_some_and(vt100::Cell::is_wide_continuation)
    {
        col -= 1;
    }
    let end = col + u16::from(screen.cell(row, col).is_some_and(vt100::Cell::is_wide));
    let row = screen.visible_row_id() + u64::from(row);
    Selection {
        anchor: (row, col),
        end: (row, end),
    }
}
pub(super) fn word(screen: &vt100::Screen, (row, col): (u16, u16)) -> Selection {
    let (_, cols) = screen.size();
    let is_word = |c| {
        screen.cell(row, c).is_some_and(|cell| {
            cell.is_wide_continuation()
                || cell
                    .contents()
                    .chars()
                    .any(|c| !c.is_whitespace() && !c.is_control())
        })
    };
    let (mut left, mut right) = (col, col);
    if is_word(col) {
        while left > 0 && is_word(left - 1) {
            left -= 1;
        }
        while right + 1 < cols && is_word(right + 1) {
            right += 1;
        }
    }
    Selection {
        anchor: (screen.visible_row_id() + u64::from(row), left),
        end: (screen.visible_row_id() + u64::from(row), right),
    }
}
pub(super) fn text(screen: &vt100::Screen, selection: Option<&Selection>) -> String {
    let Some(selection) = selection else {
        return String::new();
    };
    let (mut start, mut end) = if selection.anchor <= selection.end {
        (selection.anchor, selection.end)
    } else {
        (selection.end, selection.anchor)
    };
    let first = screen.first_retained_row();
    let last = first + screen.history_len() as u64 + u64::from(screen.size().0) - 1;
    if end.0 < first || start.0 > last {
        return String::new();
    }
    if start.0 < first {
        start = (first, 0);
    }
    end.0 = end.0.min(last);
    let mut text = String::new();
    let (_, cols) = screen.size();
    for row in start.0..=end.0 {
        if row > start.0 && !screen.absolute_row_wrapped(row - 1) {
            text.push('\n');
        }
        let mut line = String::new();
        for col in
            if row == start.0 { start.1 } else { 0 }..=if row == end.0 { end.1 } else { cols - 1 }
        {
            if let Some(cell) = screen.absolute_cell(row, col)
                && !cell.is_wide_continuation()
            {
                if cell.has_contents() {
                    line.push_str(&cell.contents());
                } else {
                    line.push(' ');
                }
            }
        }
        text.push_str(line.trim_end_matches(' '));
    }
    text
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn words_include_paths_punctuation_and_wide_characters() {
        let mut p = vt100::Parser::new(3, 40, 0);
        p.process("abc /tmp/中文.txt next".as_bytes());
        let s = word(p.screen(), (0, 10));
        assert_eq!(text(p.screen(), Some(&s)), "/tmp/中文.txt");
    }
    #[test]
    fn backward_selection_preserves_wrapped_rows() {
        let mut p = vt100::Parser::new(3, 5, 0);
        p.process(b"abcdefghi");
        assert_eq!(
            text(
                p.screen(),
                Some(&Selection {
                    anchor: (1, 3),
                    end: (0, 0)
                })
            ),
            "abcdefghi"
        );
    }

    #[test]
    fn selecting_across_scrollback_keeps_the_same_text_after_scrolling() {
        let mut p = vt100::Parser::new(3, 20, 20);
        for index in 0..12 {
            p.process(format!("line{index:02}\r\n").as_bytes());
        }
        p.set_scrollback(8);
        let first = p.screen().visible_row_id();
        let selection = Selection {
            anchor: (first, 0),
            end: (first + 5, 5),
        };
        let before = text(p.screen(), Some(&selection));
        p.set_scrollback(0);
        assert_eq!(text(p.screen(), Some(&selection)), before);
        assert_eq!(before.lines().count(), 6);
        assert!(before.starts_with("line02\n"));
    }
    #[test]
    fn discarded_history_clamps_selection_without_selecting_newer_unrelated_rows() {
        let mut p = vt100::Parser::new(2, 20, 3);
        p.process(b"zero\r\none\r\ntwo\r\n");
        let selection = Selection {
            anchor: (0, 0),
            end: (1, 19),
        };
        p.process(b"three\r\nfour\r\nfive\r\nsix\r\n");
        assert!(p.screen().first_retained_row() > 1);
        assert!(text(p.screen(), Some(&selection)).is_empty());
        assert!(p.screen().history_len() <= 3);
    }
    #[test]
    fn dragging_from_wide_character_continuation_selects_the_whole_character() {
        let mut p = vt100::Parser::new(2, 20, 3);
        p.process("A中文".as_bytes());
        let point = character(p.screen(), (0, 2));
        assert_eq!(point.anchor, (0, 1));
        assert_eq!(point.end, (0, 2));
        assert_eq!(text(p.screen(), Some(&point)), "中");
    }
    #[test]
    fn released_selection_can_still_be_copied_after_tail_output_discards_history() {
        let mut parser = vt100::Parser::new(2, 20, 2);
        parser.process(b"selected\r\nnext");
        let mut view = crate::tabs::terminal::TerminalViewState {
            selection: Some(Selection {
                anchor: (0, 0),
                end: (0, 7),
            }),
            ..Default::default()
        };
        snapshot(parser.screen(), &mut view);
        parser.process(b"\r\n1\r\n2\r\n3\r\n4\r\n");
        assert_eq!(copy(parser.screen(), &view), "selected");
        assert!(view.has_selection());
        view.clear_selection();
        assert_eq!(copy(parser.screen(), &view), "");
    }
}
