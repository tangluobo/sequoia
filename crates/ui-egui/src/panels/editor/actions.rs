use egui::{
    Context, Id,
    text::{CCursor, CCursorRange},
    widgets::text_edit::TextEditState,
};
#[derive(Clone, Copy)]
pub(super) enum Action {
    Wrap(&'static str, &'static str),
    Prefix(&'static str),
    Link,
    Image,
    CodeBlock,
    Rule,
    Table,
    Undo,
    Redo,
}
pub(super) fn apply(ctx: &Context, id: Id, text: &mut String, action: Action) {
    let mut state = TextEditState::load(ctx, id).unwrap_or_default();
    let range = state
        .cursor
        .char_range()
        .unwrap_or_else(|| CCursorRange::one(CCursor::new(text.chars().count())));
    let [start, end] = range.sorted_cursors();
    let start: usize = start.index.into();
    let end: usize = end.index.into();
    let mut undo = state.take_undoer();
    let now = ctx.input(|i| i.time);
    if matches!(action, Action::Undo | Action::Redo) {
        let value = if matches!(action, Action::Undo) {
            undo.undo(&(range, text.clone()))
        } else {
            undo.redo(&(range, text.clone()))
        };
        if let Some((range, value)) = value.cloned() {
            *text = value;
            state.cursor.set_char_range(Some(range));
        }
    } else {
        undo.add_undo(&(range, text.clone()));
        let offset = |index| {
            text.char_indices()
                .nth(index)
                .map_or(text.len(), |(index, _)| index)
        };
        let (mut left, mut right) = (offset(start), offset(end));
        let (replacement, selection) = match action {
            Action::Wrap(before, after) => {
                let selected = &text[left..right];
                if selected.starts_with(before)
                    && selected.ends_with(after)
                    && selected.len() >= before.len() + after.len()
                {
                    let inner = selected[before.len()..selected.len() - after.len()].to_owned();
                    let len = inner.chars().count();
                    (inner, (start, start + len))
                } else {
                    let len = selected.chars().count();
                    (
                        format!("{before}{selected}{after}"),
                        (
                            start + before.chars().count(),
                            start + before.chars().count() + len,
                        ),
                    )
                }
            }
            Action::Prefix(prefix) => {
                left = text[..left].rfind('\n').map_or(0, |index| index + 1);
                right = text[right..]
                    .find('\n')
                    .map_or(text.len(), |index| right + index);
                let selected = &text[left..right];
                let remove = selected.split('\n').all(|line| line.starts_with(prefix));
                let mut replacement = String::new();
                for line in selected.split('\n') {
                    if !replacement.is_empty() {
                        replacement.push('\n');
                    }
                    if remove {
                        replacement.push_str(&line[prefix.len()..]);
                    } else {
                        replacement.push_str(prefix);
                        replacement.push_str(line);
                    }
                }
                let begin = text[..left].chars().count();
                let end = begin + replacement.chars().count();
                (replacement, (begin, end))
            }
            Action::Link | Action::Image => {
                let (opening, placeholder) = if matches!(action, Action::Image) {
                    ("![", "替代文本")
                } else {
                    ("[", "链接文本")
                };
                let label = if left == right {
                    placeholder
                } else {
                    &text[left..right]
                };
                let url = start + opening.len() + label.chars().count() + 2;
                (format!("{opening}{label}](url)"), (url, url + 3))
            }
            Action::CodeBlock | Action::Rule | Action::Table => {
                let caret = usize::from(range.primary.index);
                left = if caret == start { left } else { right };
                right = left;
                let (value, advance) = match action {
                    Action::CodeBlock => ("\n```\n\n```\n", 5),
                    Action::Rule if left == 0 || text[..left].ends_with('\n') => ("\n---\n", 5),
                    Action::Rule => ("\n\n---\n", 6),
                    Action::Table => {
                        let table =
                            "\n| 列1 | 列2 | 列3 |\n|---|---|---|\n|  |  |  |\n|  |  |  |\n";
                        (table, table.chars().count())
                    }
                    _ => unreachable!(),
                };
                (value.into(), (caret + advance, caret + advance))
            }
            _ => unreachable!(),
        };
        text.replace_range(left..right, &replacement);
        let range = CCursorRange::two(CCursor::new(selection.0), CCursor::new(selection.1));
        state.cursor.set_char_range(Some(range));
        undo.add_undo(&(range, text.clone()));
    }
    undo.feed_state(
        now,
        &(state.cursor.char_range().unwrap_or(range), text.clone()),
    );
    state.set_undoer(undo);
    state.store(ctx, id);
    ctx.memory_mut(|memory| memory.request_focus(id));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toolbar_wrap_prefix_undo_redo_preserve_unicode_and_selection() {
        let ctx = Context::default();
        let id = Id::new("editor-test");
        let mut text = "中文\n下一行".to_owned();
        let mut state = TextEditState::default();
        state
            .cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(2))));
        state.store(&ctx, id);
        apply(&ctx, id, &mut text, Action::Wrap("**", "**"));
        assert_eq!(text, "**中文**\n下一行");
        apply(&ctx, id, &mut text, Action::Undo);
        assert_eq!(text, "中文\n下一行");
        apply(&ctx, id, &mut text, Action::Redo);
        assert_eq!(text, "**中文**\n下一行");
        apply(&ctx, id, &mut text, Action::Prefix("# "));
        assert_eq!(text, "# **中文**\n下一行");
        apply(&ctx, id, &mut text, Action::Prefix("# "));
        assert_eq!(text, "**中文**\n下一行");
    }

    #[test]
    fn original_insertions_keep_selections_and_select_url_or_code_caret() {
        let ctx = Context::default();
        let id = Id::new("toolbar-original-insertions");
        let table = "\n| 列1 | 列2 | 列3 |\n|---|---|---|\n|  |  |  |\n|  |  |  |\n";
        let with_table = format!("甲{table}乙");
        for (action, source, anchor, caret, expected, expected_selection) in [
            (Action::Link, "甲😀乙", 1, 2, "甲[😀](url)乙", 5..8),
            (Action::Image, "甲乙", 1, 1, "甲![替代文本](url)乙", 9..12),
            (
                Action::CodeBlock,
                "甲乙丙",
                3,
                1,
                "甲\n```\n\n```\n乙丙",
                6..6,
            ),
            (Action::Rule, "甲乙", 1, 1, "甲\n\n---\n乙", 7..7),
            (Action::Rule, "甲\n乙", 2, 2, "甲\n\n---\n乙", 7..7),
            (
                Action::Table,
                "甲乙",
                0,
                1,
                with_table.as_str(),
                1 + table.chars().count()..1 + table.chars().count(),
            ),
            (Action::Prefix("# "), "", 0, 0, "# ", 0..2),
        ] {
            let mut text = source.to_owned();
            let mut state = TextEditState::default();
            state.cursor.set_char_range(Some(CCursorRange::two(
                CCursor::new(anchor),
                CCursor::new(caret),
            )));
            state.store(&ctx, id);
            apply(&ctx, id, &mut text, action);
            assert_eq!(text, expected);
            let selection = TextEditState::load(&ctx, id)
                .unwrap()
                .cursor
                .char_range()
                .unwrap()
                .as_sorted_char_range();
            let actual = usize::from(selection.start)..usize::from(selection.end);
            // The checked ranges are character offsets, independent of UTF-8 byte size.
            assert_eq!(actual, expected_selection);
            apply(&ctx, id, &mut text, Action::Undo);
            assert_eq!(text, source);
        }
    }
}
