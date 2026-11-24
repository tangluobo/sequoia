use super::*;

fn assert_baselines(galley: &egui::Galley, ppp: f32) {
    for row in &galley.rows {
        let Some(first) = row.glyphs.first() else {
            continue;
        };
        for glyph in &row.glyphs {
            assert_eq!(glyph.pos.y, first.pos.y, "misaligned {:?}", glyph.chr);
            assert!(glyph.pos.y + 0.5 / ppp >= glyph.font_face_ascent);
            assert!(
                glyph.pos.y + glyph.font_face_height - glyph.font_face_ascent
                    <= row.size.y + 1. / ppp,
                "descent clipped for {:?}",
                glyph.chr
            );
        }
    }
}

#[test]
fn document_baseline_mode_keeps_wrapping_carets_and_cached_paragraphs_consistent()
-> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let _app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(root.path().join("connections.json")),
    );
    for ppp in [1., 1.25, 1.5, 2.] {
        let mut input = egui::RawInput::default();
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .native_pixels_per_point = Some(ppp);
        let mut output = ctx.run_ui(input, |ui| {
            let mut job = egui::text::LayoutJob::default();
            for _ in 0..24 {
                for (text, font_id) in [
                    ("普通文字 Ag ", egui::FontId::proportional(13.)),
                    (
                        "Bold 加粗 ",
                        egui::FontId::new(20., egui::FontFamily::Name("md-bold".into())),
                    ),
                    ("code 中文 gj ", egui::FontId::monospace(12.)),
                    ("尾部\n\n", egui::FontId::proportional(13.)),
                ] {
                    job.append(
                        text,
                        0.,
                        egui::TextFormat {
                            font_id,
                            ..Default::default()
                        },
                    );
                }
            }
            job.wrap.max_width = 220.;
            ui.fonts_mut(|fonts| {
                let ordinary = fonts.layout_job(job.clone());
                assert!(ordinary.rows.iter().any(|row| {
                    row.glyphs
                        .windows(2)
                        .any(|pair| pair[0].pos.y != pair[1].pos.y)
                }));
                job.align_on_baseline = true;
                let aligned = fonts.layout_job(job.clone());
                assert_baselines(&aligned, ppp);
                assert_eq!(
                    ordinary.rows.iter().map(|r| r.text()).collect::<Vec<_>>(),
                    aligned.rows.iter().map(|r| r.text()).collect::<Vec<_>>()
                );
                assert!(std::sync::Arc::ptr_eq(
                    &aligned,
                    &fonts.layout_job(job.clone())
                ));
                job.align_on_baseline = false;
                assert!(std::sync::Arc::ptr_eq(&ordinary, &fonts.layout_job(job)));
                for index in 0..=aligned.end().index.0 {
                    let cursor = egui::text::CCursor::new(index);
                    let rect = aligned.pos_from_cursor(cursor);
                    let picked = aligned.cursor_from_pos(rect.center().to_vec2());
                    assert_eq!(aligned.pos_from_cursor(picked), rect, "caret {index}");
                }
            });
        });
        output.textures_delta.clear();
    }
    Ok(())
}

#[test]
fn actual_markdown_preview_and_source_align_mixed_faces_at_multiple_dpis() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(root.path().join("connections.json")),
    );
    let source = "# 标题 Heading `code中文`\n\n普通 Ag **粗体 Bold中文** *斜体 Italic* `code中文` [链接 link](https://example.invalid) 结束\n\n> 引用 **Bold粗体** `code中文` 文字\n\n| 表头中文 | 第二列 |\n| --- | --- |\n| 普通 **加粗** `code中文` | 另一列 |\n| 短 | 这是一段用于检验换行后单元格上边距的较长文本，另一列只有一个字，内容依然从同一条线开始。 |\n\n```rust\nlet 中文 = \"value\"; // 注释\n```\n";
    let mut state = crate::tabs::markdown::State {
        document: tomato_services::markdown::parse(source)?,
        parsed: Some(0),
        ..Default::default()
    };
    state.mode = crate::tabs::markdown::Mode::Split;
    app.tabs.push(Tab {
        id: "markdown-alignment".into(),
        source_id: "file:alignment.md".into(),
        title: "alignment.md".into(),
        busy: false,
        error: None,
        panel: Panel::Editor {
            path: root.path().join("alignment.md"),
            text: source.into(),
            saved: source.into(),
            markdown: Box::new(state),
        },
    });
    app.active = Some("markdown-alignment".into());
    for ppp in [1., 1.25, 1.5, 2.] {
        let mut last = None;
        for _ in 0..6 {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400., 1000.),
                )),
                time: Some(ctx.input(|input| input.time) + 0.1),
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .native_pixels_per_point = Some(ppp);
            let mut output = ctx.run_ui(input, |ui| app.ui(ui));
            output.textures_delta.clear();
            last = Some(output);
        }
        let output = last.unwrap();
        let texts = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>();
        for needle in ["标题 Heading", "普通 Ag", "引用", "表头中文", "let 中文"] {
            let text = texts
                .iter()
                .find(|text| text.galley.job.text.starts_with(needle))
                .unwrap_or_else(|| panic!("missing actual Markdown text {needle:?}"));
            assert!(text.galley.job.align_on_baseline);
            assert_baselines(&text.galley, ppp);
        }
        assert!(
            texts
                .iter()
                .any(|text| text.galley.job.text == source && text.galley.job.align_on_baseline),
            "actual source editor"
        );
        for first in ["表头中文", "普通 加粗 code中文", "短"] {
            let text = texts
                .iter()
                .find(|text| text.galley.job.text == first)
                .unwrap();
            let text_center = text.pos.x + text.galley.size().x / 2.;
            let row_top = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect)
                        if rect.fill == egui::Color32::from_rgb(223, 226, 229)
                            && rect.rect.height() <= 1.1
                            && rect.rect.x_range().contains(text_center)
                            && rect.rect.bottom() <= text.pos.y =>
                    {
                        Some(rect.rect.bottom())
                    }
                    _ => None,
                })
                .max_by(f32::total_cmp)
                .expect("horizontal edge above actual Markdown cell");
            assert!(
                (text.pos.y - row_top - 6.).abs() <= 1. / ppp,
                "cell text must keep Java's top padding at {ppp} for {first:?}: {row_top} {:?}",
                text.pos
            );
        }
        let source_text = texts
            .iter()
            .find(|text| text.galley.job.text == source)
            .unwrap();
        for (index, row) in source_text.galley.rows.iter().enumerate() {
            let Some(glyph) = row.glyphs.first() else {
                continue;
            };
            let expected = source_text.pos.y + row.pos.y + glyph.pos.y;
            let number = texts
                .iter()
                .find(|text| {
                    text.galley.job.text == (index + 1).to_string()
                        && text.pos.x < source_text.pos.x
                        && (text.pos.y - source_text.pos.y - row.pos.y).abs() < 8.
                })
                .expect("actual editor line number");
            let actual = number.pos.y + number.galley.rows[0].glyphs[0].pos.y;
            assert!(
                (actual - expected).abs() <= 1. / ppp,
                "source line {}",
                index + 1
            );
        }
        assert!(
            app.pending.is_empty(),
            "no background protocol or parse task needed"
        );
    }
    Ok(())
}
