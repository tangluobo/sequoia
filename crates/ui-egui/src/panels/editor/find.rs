//! The active Java MarkdownEditorPane uses an owned, non-modal 408×177 window.
use super::*;
use crate::tabs::markdown::{
    State,
    search_job::{Job, Notice, Output, Work},
};
use egui::widgets::text_edit::TextEditState;
use std::{ops::Range, sync::Arc};
use tomato_core::search::{MAX_REPLACEMENT, MAX_TEXT, Search};

pub(super) fn render(ui: &mut Ui, state: &mut State, text: &mut String, id: egui::Id, now: f64) {
    let ctx = ui.ctx().clone();
    let viewport = egui::ViewportId::from_hash_of(("markdown-find", id));
    let capture = std::env::var_os("TOMATO_UI_CAPTURE").is_some()
        && std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("markdown_find");
    if capture {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-viewport"), viewport));
    }
    let mut builder = egui::ViewportBuilder::default()
        .with_title("查找与替换")
        .with_inner_size([408., 177.])
        .with_resizable(false)
        .with_active(!capture);
    if !state.find_positioned {
        if let Some(rect) = ctx.input(|input| input.viewport().inner_rect) {
            builder = builder.with_position(rect.center() - egui::vec2(204., 88.5));
        }
        state.find_positioned = true;
    }
    ctx.show_viewport_immediate(viewport, builder, |ui, _class| {
        if capture
            && let Some(image) = ui.input(|input| {
                input.events.iter().find_map(|event| {
                    if let egui::Event::Screenshot {
                        viewport_id, image, ..
                    } = event
                        && *viewport_id == viewport
                    {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            })
        {
            ui.ctx()
                .data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-image"), image));
            ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
        }
        if ui.input(|input| {
            input.viewport().close_requested() || input.key_pressed(egui::Key::Escape)
        }) {
            state.find = false;
            return;
        }
        content(ui, state, text, id, now);
    });
    if !state.find {
        state.search = None;
        state.search_key = None;
        state.search_job = None;
        state.find_notice = None;
        state.find_advance = false;
        ctx.memory_mut(|memory| memory.request_focus(id));
    }
}
fn content(ui: &mut Ui, state: &mut State, text: &mut String, id: egui::Id, now: f64) {
    ui.style_mut().override_font_id = Some(egui::FontId::proportional(12.));
    ui.spacing_mut().button_padding = egui::vec2(6., 2.);
    ui.spacing_mut().interact_size.y = 23.;
    let base = ui.max_rect().min;
    let rect = |x, y, width, height| {
        egui::Rect::from_min_size(base + egui::vec2(x, y), egui::vec2(width, height))
    };
    ui.painter().rect_filled(ui.max_rect(), 0, Color32::WHITE);
    ui.painter().rect_filled(
        rect(0., 0., ui.available_width(), 37.),
        0,
        Color32::from_gray(236),
    );
    for (replace, x, label) in [(false, 0., "查找"), (true, 49., "替换")] {
        let area = rect(x, 0., 49., 37.);
        if state.replace_visible == replace {
            ui.painter().rect_filled(area, 0, Color32::WHITE);
        }
        if ui
            .put(area, egui::Button::new(label).frame(false))
            .clicked()
        {
            state.replace_visible = replace;
            state.find_focus = true;
        }
    }
    let field_frame = egui::Frame::new()
        // The Java tab pane has a one-pixel frame around the selected tab.
        .fill(Color32::WHITE)
        .stroke(egui::Stroke::new(1., Color32::from_rgb(26, 115, 232)))
        .inner_margin(egui::Margin::symmetric(8, 3));
    let stroke = egui::Stroke::new(1., Color32::from_gray(196));
    let selected_x = if state.replace_visible { 49. } else { 0. };
    for x in [selected_x - 0.5, selected_x + 48.5] {
        if x >= 0. {
            ui.painter().line_segment(
                [base + egui::vec2(x, 0.), base + egui::vec2(x, 37.)],
                stroke,
            );
        }
    }
    if state.replace_visible {
        ui.painter().line_segment(
            [base + egui::vec2(0., 36.5), base + egui::vec2(49., 36.5)],
            stroke,
        );
    }
    ui.painter().line_segment(
        [
            base + egui::vec2(selected_x + 49., 36.5),
            base + egui::vec2(ui.available_width(), 36.5),
        ],
        stroke,
    );
    let field = usize::from(state.replace_visible);
    let query_id = ui.make_persistent_id(("markdown-find-query", field));
    let query_focused = ui.memory(|memory| memory.has_focus(query_id));
    let mut previous =
        ui.input_mut(|input| input.consume_key(egui::Modifiers::SHIFT, egui::Key::F3));
    let mut next = ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F3));
    if query_focused {
        previous |=
            ui.input_mut(|input| input.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter));
        next |= ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    }
    aligned_label(ui, rect(0., 37., 80., 25.), "查找:", true);
    let input = ui.put(
        rect(88., 37., 320., 25.),
        egui::TextEdit::singleline(&mut state.needle)
            .id(query_id)
            .font(egui::FontId::proportional(12.))
            .vertical_align(egui::Align::Center)
            .frame(field_frame)
            .hint_text("输入查找内容（回车=下一个，Shift+回车=上一个）"),
    );
    if state.find_focus {
        input.request_focus();
        state.find_focus = false;
    }
    state.find_field_ids[field] = Some(input.id);
    let options_y = if state.replace_visible {
        aligned_label(ui, rect(0., 70., 80., 25.), "替换为:", true);
        let input = ui.put(
            rect(88., 70., 320., 25.),
            egui::TextEdit::singleline(&mut state.replacement)
                .font(egui::FontId::proportional(12.))
                .vertical_align(egui::Align::Center)
                .frame(field_frame)
                .hint_text("替换为（正则模式下支持 $1 等回引用）"),
        );
        state.find_field_ids[2] = Some(input.id);
        if input.changed() {
            state.find_notice = None;
        }
        103.
    } else {
        70.
    };
    aligned_label(ui, rect(0., options_y, 80., 20.), "正则表达式", true);
    let switch = rect(88., options_y, 38., 20.);
    if ui
        .interact(
            switch,
            ui.make_persistent_id("regex-switch"),
            egui::Sense::click(),
        )
        .clicked()
    {
        state.regex = !state.regex;
    }
    ui.painter().rect_filled(
        switch,
        10.,
        if state.regex {
            Color32::from_rgb(76, 175, 80)
        } else {
            Color32::from_gray(189)
        },
    );
    ui.painter().circle_filled(
        egui::pos2(
            if state.regex {
                switch.right() - 10.
            } else {
                switch.left() + 10.
            },
            switch.center().y,
        ),
        8.,
        Color32::WHITE,
    );
    let y = options_y + 28.;
    previous |= ui
        .put(rect(0., y, 52., 23.), egui::Button::new("上一个"))
        .clicked();
    next |= ui
        .put(rect(60., y, 52., 23.), egui::Button::new("下一个"))
        .clicked();
    let replace = if state.replace_visible {
        let one = ui
            .put(rect(120., y, 40., 23.), egui::Button::new("替换"))
            .clicked();
        let all = ui
            .put(rect(168., y, 64., 23.), egui::Button::new("全部替换"))
            .clicked();
        one.then_some(Replace::One).or(all.then_some(Replace::All))
    } else {
        None
    };
    let status = update(
        ui.ctx(),
        state,
        text,
        id,
        now,
        Actions {
            previous,
            next,
            replace,
        },
    );
    aligned_label(
        ui,
        rect(0., y + 31., 408., 15.),
        RichText::new(status)
            .size(12.)
            .color(Color32::from_gray(102)),
        false,
    );
}

enum Replace {
    One,
    All,
}

struct Actions {
    previous: bool,
    next: bool,
    replace: Option<Replace>,
}

fn update(
    ctx: &egui::Context,
    state: &mut State,
    text: &mut String,
    id: egui::Id,
    now: f64,
    actions: Actions,
) -> String {
    let changed = state.search_key.as_ref().is_none_or(|key| {
        key.0 != state.revision
            || key.1 != state.needle
            || key.2 != state.match_case
            || key.3 != state.regex
    });
    if changed {
        if !matches!(state.find_notice, Some(Notice::Replaced(revision)) if revision == state.revision)
            || state
                .search_key
                .as_ref()
                .is_some_and(|key| key.1 != state.needle || key.3 != state.regex)
        {
            state.find_notice = None;
        }
        state.search_key = Some((
            state.revision,
            state.needle.clone(),
            state.match_case,
            state.regex,
        ));
        state.search = None;
        state.match_index = 0;
    }
    let key = state.search_key.as_ref().expect("search key");
    let mut new_matches = false;
    if let Some(job) = &mut state.search_job {
        let applies = job.applies(key, &state.replacement);
        if !applies {
            job.cancel();
        }
        let Some(result) = job.poll() else {
            return if job.is_replacement() {
                "正在替换…".into()
            } else {
                notice(state).unwrap_or_else(|| "正在查找…".into())
            };
        };
        let replacing = job.is_replacement();
        state.search_job = None;
        if applies {
            match result {
                Ok(Output::Matches(search)) => {
                    state.search = Some(Ok(search));
                    new_matches = true;
                }
                Ok(Output::Replaced(value, hit)) => {
                    apply_replacement(ctx, state, text, id, now, value, hit);
                    return notice(state).unwrap_or_default();
                }
                Err(error) if replacing => state.find_notice = Some(Notice::Error(error)),
                Err(error) => state.search = Some(Err(error)),
            }
        }
    }
    if state.needle.is_empty() {
        return String::new();
    }
    if state.search.is_none() {
        if state.regex {
            match Job::start(ctx, key.clone(), text, Work::Find) {
                Ok(Some(job)) => {
                    state.search_job = Some(job);
                    return notice(state).unwrap_or_else(|| "正在查找…".into());
                }
                Ok(None) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(40));
                    return "等待查找…".into();
                }
                Err(error) => state.search = Some(Err(error)),
            }
        } else {
            state.search = Some(
                Search::new(text, &state.needle, state.match_case, false)
                    .map(Arc::new)
                    .map_err(|error| error.to_string()),
            );
            new_matches = true;
        }
    }
    let Some(result) = &state.search else {
        return String::new();
    };
    let search = match result {
        Ok(search) => search,
        Err(error) => return format!("{}错误: {error}", if state.regex { "正则" } else { "查找" }),
    };
    let count = search.hits.len();
    let truncated = search.truncated;
    let advance = new_matches && std::mem::take(&mut state.find_advance);
    if count > 0 {
        if new_matches {
            let edit = TextEditState::load(ctx, id).unwrap_or_default();
            let caret = edit
                .cursor
                .char_range()
                .map_or(0, |range| usize::from(range.primary.index));
            let byte = text
                .char_indices()
                .nth(caret)
                .map_or(text.len(), |(byte, _)| byte);
            state.match_index = search
                .hits
                .partition_point(|hit| hit.start < byte)
                .min(count - 1);
        }
        state.match_index = state.match_index.min(count - 1);
        if actions.previous {
            state.match_index = (state.match_index + count - 1) % count;
        }
        if actions.next || advance {
            state.match_index = (state.match_index + 1) % count;
        }
        if new_matches || actions.previous || actions.next {
            select(ctx, id, text, search.hits[state.match_index].clone());
            state.reveal_match = true;
        }
    }
    if actions.previous || actions.next {
        state.find_notice = None;
    }
    if let Some(replace) = actions.replace.filter(|_| count > 0) {
        let index = matches!(replace, Replace::One).then_some(state.match_index);
        if state.regex {
            if state.replacement.len() > MAX_REPLACEMENT {
                return "替换错误: 正则替换模板超过 256 KiB".into();
            }
            let work = Work::Replace {
                search: search.clone(),
                index,
                value: Arc::new(state.replacement.clone()),
            };
            match Job::start(ctx, key.clone(), text, work) {
                Ok(Some(job)) => {
                    state.search_job = Some(job);
                    return "正在替换…".into();
                }
                Ok(None) => return "查找后台正忙，请稍后重试。".into(),
                Err(error) => state.find_notice = Some(Notice::Error(error)),
            }
        } else {
            match search.replace(text, index, &state.replacement, MAX_TEXT) {
                Ok(value) => {
                    let hit = index.map(|index| search.hits[index].clone());
                    apply_replacement(ctx, state, text, id, now, value, hit);
                }
                Err(error) => state.find_notice = Some(Notice::Error(error.to_string())),
            }
        }
    }
    notice(state).unwrap_or_else(|| {
        if count == 0 {
            "无匹配".into()
        } else {
            let suffix = if truncated { "+" } else { "" };
            format!("{} / {count}{suffix}", state.match_index + 1)
        }
    })
}

fn notice(state: &State) -> Option<String> {
    match &state.find_notice {
        Some(Notice::Replaced(_)) => Some("已替换".into()),
        Some(Notice::Error(error)) => Some(format!("替换错误: {error}")),
        None => None,
    }
}

fn apply_replacement(
    ctx: &egui::Context,
    state: &mut State,
    text: &mut String,
    id: egui::Id,
    now: f64,
    value: String,
    hit: Option<Range<usize>>,
) {
    let mut edit = TextEditState::load(ctx, id).unwrap_or_default();
    let range = edit.cursor.char_range().unwrap_or_default();
    let mut undo = edit.take_undoer();
    undo.add_undo(&(range, text.clone()));
    let old_length = text.len();
    *text = value;
    state.find_notice = None;
    let range = if let Some(hit) = hit {
        let end = hit.start
            + text
                .len()
                .saturating_add(hit.len())
                .saturating_sub(old_length);
        state.find_advance = true;
        egui::text::CCursorRange::two(
            egui::text::CCursor::new(text[..hit.start].chars().count()),
            egui::text::CCursor::new(text[..end].chars().count()),
        )
    } else {
        state.find_notice = Some(Notice::Replaced(state.revision.wrapping_add(1)));
        range
    };
    edit.cursor.set_char_range(Some(range));
    let snapshot = (range, text.clone());
    undo.add_undo(&snapshot);
    undo.feed_state(now, &snapshot);
    edit.set_undoer(undo);
    edit.store(ctx, id);
    state.changed(now);
    state.reveal_match = true;
    ctx.request_repaint();
    ctx.request_repaint_of(egui::ViewportId::ROOT);
}
fn aligned_label(ui: &mut Ui, rect: egui::Rect, text: impl Into<egui::WidgetText>, right: bool) {
    let layout = if right {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(layout), |ui| {
        ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend));
    });
}
fn select(ctx: &egui::Context, id: egui::Id, text: &str, range: std::ops::Range<usize>) {
    let mut state = TextEditState::load(ctx, id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(text[..range.start].chars().count()),
            egui::text::CCursor::new(text[..range.end].chars().count()),
        )));
    state.store(ctx, id);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        ctx: &egui::Context,
        state: &mut State,
        text: &mut String,
        id: egui::Id,
        events: Vec<egui::Event>,
        time: f64,
    ) {
        let input = egui::RawInput {
            events,
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(408., 177.),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| content(ui, state, text, id, time));
        output.textures_delta.clear();
    }
    #[test]
    fn f3_selects_unicode_matches_and_default_search_is_case_sensitive() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("parent-editor");
        let mut state = State {
            needle: "中文".into(),
            ..Default::default()
        };
        let mut text = "中文 甲 中文".to_owned();
        frame(&ctx, &mut state, &mut text, id, vec![], 0.);
        frame(
            &ctx,
            &mut state,
            &mut text,
            id,
            vec![egui::Event::Key {
                key: egui::Key::F3,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
            0.1,
        );
        assert_eq!(state.match_index, 1);
        let range = TextEditState::load(&ctx, id)
            .unwrap()
            .cursor
            .char_range()
            .unwrap();
        assert_eq!(usize::from(range.sorted_cursors()[0].index), 5);
        state.needle = "ABC".into();
        text = "abc ABC".into();
        state.revision += 1;
        frame(&ctx, &mut state, &mut text, id, vec![], 0.2);
        let hits = &state.search.as_ref().unwrap().as_ref().unwrap().hits;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0], 4..7);
    }
    #[test]
    fn clicking_replace_updates_parent_editor_and_preserves_undo_redo() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("parent-editor");
        let mut state = State {
            needle: "中文".into(),
            replacement: "替".into(),
            replace_visible: true,
            ..Default::default()
        };
        let mut text = "中文 甲 中文".to_owned();
        frame(&ctx, &mut state, &mut text, id, vec![], 0.);
        let point = egui::pos2(140., 142.);
        frame(
            &ctx,
            &mut state,
            &mut text,
            id,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            0.1,
        );
        frame(
            &ctx,
            &mut state,
            &mut text,
            id,
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            0.2,
        );
        assert_eq!(text, "替 甲 中文");
        super::super::actions::apply(&ctx, id, &mut text, super::super::actions::Action::Undo);
        assert_eq!(text, "中文 甲 中文");
        super::super::actions::apply(&ctx, id, &mut text, super::super::actions::Action::Redo);
        assert_eq!(text, "替 甲 中文");
    }

    fn click(
        ctx: &egui::Context,
        state: &mut State,
        text: &mut String,
        id: egui::Id,
        point: egui::Pos2,
        time: f64,
    ) {
        for pressed in [true, false] {
            frame(
                ctx,
                state,
                text,
                id,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                time + if pressed { 0. } else { 0.01 },
            );
        }
    }

    #[test]
    fn query_tabs_share_input_but_focus_their_own_fields_and_accept_enter() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("parent-editor");
        let mut state = State {
            needle: "苹果".into(),
            find_focus: true,
            ..Default::default()
        };
        let mut text = "苹果 梨 苹果".into();
        frame(&ctx, &mut state, &mut text, id, vec![], 0.);
        let find = state.find_field_ids[0].unwrap();
        assert!(ctx.memory(|memory| memory.has_focus(find)));
        click(&ctx, &mut state, &mut text, id, egui::pos2(74., 16.), 0.1);
        assert!(state.replace_visible);
        let replace = state.find_field_ids[1].unwrap();
        assert_ne!(find, replace);
        assert!(ctx.memory(|memory| memory.has_focus(replace)));
        assert_eq!(state.needle, "苹果");
        for (step, (index, modifiers)) in [(1, egui::Modifiers::NONE), (0, egui::Modifiers::SHIFT)]
            .into_iter()
            .enumerate()
        {
            frame(
                &ctx,
                &mut state,
                &mut text,
                id,
                vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                0.2 + step as f64 * 0.01,
            );
            assert_eq!(state.match_index, index);
            assert!(ctx.memory(|memory| memory.has_focus(replace)));
        }
    }

    #[test]
    fn regex_widget_replaces_and_discards_results_for_newer_editor_revisions() -> anyhow::Result<()>
    {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .build()?;
        let ctx = egui::Context::default();
        crate::tabs::markdown::search_job::install(&ctx, runtime.handle());
        let id = egui::Id::new("parent-editor");
        let mut state = State {
            needle: r"(?<=:)\b([A-Z])\1\b".into(),
            replacement: "$1".into(),
            replace_visible: true,
            regex: true,
            ..Default::default()
        };
        let mut text = ":AA :BB".to_owned();
        let mut time = 0.;
        let tick = |state: &mut State, text: &mut String, time: &mut f64| -> anyhow::Result<()> {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(4);
            loop {
                *time += 0.04;
                frame(&ctx, state, text, id, vec![], *time);
                if state.search_job.is_none() && state.search.is_some() {
                    break Ok(());
                }
                anyhow::ensure!(
                    std::time::Instant::now() < until,
                    "test search did not finish"
                );
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        };
        tick(&mut state, &mut text, &mut time)?;
        assert_eq!(
            state.search.as_ref().unwrap().as_ref().unwrap().hits.len(),
            2
        );
        let replace_all = egui::pos2(200., 142.);
        click(&ctx, &mut state, &mut text, id, replace_all, time + 0.02);
        assert!(state.search_job.as_ref().is_some_and(Job::is_replacement));
        text.push_str(" :CC");
        state.changed(time + 0.05);
        time += 0.05;
        tick(&mut state, &mut text, &mut time)?;
        assert_eq!(text, ":AA :BB :CC");
        assert_eq!(
            state.search.as_ref().unwrap().as_ref().unwrap().hits.len(),
            3
        );
        click(&ctx, &mut state, &mut text, id, replace_all, time + 0.02);
        tick(&mut state, &mut text, &mut time)?;
        assert_eq!(text, ":A :B :C");
        assert!(matches!(state.find_notice, Some(Notice::Replaced(_))));
        super::super::actions::apply(&ctx, id, &mut text, super::super::actions::Action::Undo);
        assert_eq!(text, ":AA :BB :CC");
        Ok(())
    }
}
