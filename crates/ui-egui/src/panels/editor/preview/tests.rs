use super::*;

#[test]
fn inline_and_table_images_share_the_text_baseline_wrap_and_clipboard_text() -> anyhow::Result<()> {
    use crate::tabs::markdown::ReadyPicture;
    let ctx = egui::Context::default();
    let document = tomato_services::markdown::parse(
        "before ![图](inline.png) after\n\n| Title | Value |\n| --- | --- |\n| cell ![图](inline.png) end | plain |",
    )?;
    let mut fonts = egui::FontDefinitions::default();
    fonts.families.insert(
        egui::FontFamily::Name("md-bold".into()),
        fonts.families[&egui::FontFamily::Proportional].clone(),
    );
    ctx.set_fonts(fonts);
    let picture = ReadyPicture::new(
        &ctx,
        tomato_services::markdown::Picture {
            width: 64,
            height: 20,
            rgba: [12, 100, 220, 255].repeat(64 * 20),
            gif: None,
        },
    );
    let texture = picture.texture.id();
    let cache = HashMap::from([("inline.png".into(), Picture::Ready(picture))]);
    let images = Images {
        cache: &cache,
        interactive: true,
    };
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let wide = text_layout(ui, &document.blocks[0].spans, 13., false, 300., images);
        assert_eq!(wide.galley.rows.len(), 1);
        let glyphs = &wide.galley.rows[0].glyphs;
        let embedded = glyphs
            .iter()
            .position(|glyph| glyph.chr == '\u{fffc}')
            .unwrap();
        assert_eq!(glyphs[embedded].advance_width, 64.);
        assert_eq!(glyphs[embedded].font_face_ascent, 24.);
        assert_eq!(glyphs[embedded].pos.y, glyphs[0].pos.y);
        assert_eq!(glyphs[embedded].pos.y, glyphs.last().unwrap().pos.y);
        let text = &wide.galley.job.text;
        assert_eq!(
            wide.galley.job.text_without_objects(0..text.len()),
            "before  after"
        );
        let narrow = text_layout(ui, &document.blocks[0].spans, 13., false, 90., images);
        assert!(narrow.galley.rows.len() > 1);
        assert!(
            !narrow.galley.rows[0]
                .glyphs
                .iter()
                .any(|glyph| glyph.chr == '\u{fffc}')
        );
        let mut visits = Visits {
            interactive: true,
            ..Default::default()
        };
        paint_inline(ui, wide, &mut visits);
        block(ui, &document.blocks[1], images, &mut visits, None);
        assert!(visits.request.is_none());
    });
    output.textures_delta.clear();
    let pictures = output
        .shapes
        .iter()
        .filter(
            |shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == texture),
        )
        .count();
    assert_eq!(
        pictures, 2,
        "paragraph and table cell must both draw a picture"
    );
    Ok(())
}

#[test]
fn actual_preview_drag_copies_highlighted_unicode_and_context_copies_the_whole_paragraph()
-> anyhow::Result<()> {
    let ctx = egui::Context::default();
    let mut state = State {
        document: tomato_services::markdown::parse("start **中文 Bold** ending")?,
        ..Default::default()
    };
    // The normal application installs the bold family before opening a Markdown tab.
    let mut fonts = egui::FontDefinitions::default();
    fonts.families.insert(
        egui::FontFamily::Name("md-bold".into()),
        fonts.families[&egui::FontFamily::Proportional].clone(),
    );
    ctx.set_fonts(fonts);
    let mut frame = |events| {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600., 400.),
                )),
                events,
                time: Some(ctx.input(|i| i.time) + 0.15),
                ..Default::default()
            },
            |ui| {
                render(ui, &mut state);
            },
        );
        out.textures_delta.clear();
        out
    };
    frame(vec![]);
    let out = frame(vec![]);
    let text = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text.starts_with("start ") => Some(t),
            _ => None,
        })
        .unwrap();
    let cursor = |index| {
        text.pos
            + text
                .galley
                .pos_from_cursor(egui::text::CCursor::new(index))
                .center()
                .to_vec2()
    };
    let start = cursor(6);
    let end = cursor(13);
    frame(vec![egui::Event::PointerMoved(start)]);
    frame(vec![egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    }]);
    frame(vec![egui::Event::PointerMoved(end)]);
    let copied = frame(vec![egui::Event::PointerButton {
        pos: end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    }]);
    assert!(
        copied
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::CopyText(s) if s=="中文 Bold")),
        "{:?}",
        copied.platform_output.commands
    );
    for pressed in [true, false] {
        frame(vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Default::default(),
            },
        ]);
    }
    let menu = frame(vec![]);
    let pos = menu
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == "复制整段" => {
                Some(t.pos + t.galley.size() / 2.)
            }
            _ => None,
        })
        .unwrap();
    frame(vec![egui::Event::PointerMoved(pos)]);
    frame(vec![egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    }]);
    let copied = frame(vec![egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    }]);
    let copies: Vec<_> = copied
        .platform_output
        .commands
        .iter()
        .filter_map(|c| {
            if let egui::OutputCommand::CopyText(s) = c {
                Some(s.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(copies, ["start 中文 Bold ending"]);
    Ok(())
}
