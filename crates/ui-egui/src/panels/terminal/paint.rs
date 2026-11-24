use super::*;
const BG: Color32 = Color32::from_rgb(30, 30, 30);
const FG: Color32 = Color32::from_rgb(229, 229, 229);
pub(super) fn screen(
    ui: &Ui,
    screen: &vt100::Screen,
    rect: egui::Rect,
    font: &egui::FontId,
    cell_size: egui::Vec2,
    selection: Option<&crate::tabs::terminal::Selection>,
    cursor_visible: bool,
) {
    let egui::Vec2 {
        x: width,
        y: height,
    } = cell_size;
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect_filled(rect, 0, BG);
    let baseline = centered_baseline(&painter, font, height);
    let (rows, cols) = screen.size();
    let range = selection.map(|s| {
        if s.anchor <= s.end {
            (s.anchor, s.end)
        } else {
            (s.end, s.anchor)
        }
    });
    for row in 0..rows {
        for col in 0..cols {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let pos =
                rect.min + egui::vec2(2. + f32::from(col) * width, 2. + f32::from(row) * height);
            let cell_rect = egui::Rect::from_min_size(
                pos,
                egui::vec2(width * if cell.is_wide() { 2.0 } else { 1.0 }, height),
            );
            if !cell_rect.intersects(rect) {
                continue;
            }
            let (mut fg, mut bg) = (color(cell.fgcolor(), FG), background(cell.bgcolor()));
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            let absolute = (screen.visible_row_id() + u64::from(row), col);
            if range.is_some_and(|(start, end)| start <= absolute && absolute <= end) {
                bg = Color32::from_rgb(65, 85, 115);
            }
            if bg != BG {
                painter.rect_filled(cell_rect, 0, bg);
            }
            if cell.has_contents() {
                text(&painter, pos, cell.contents(), font, fg, baseline);
            }
            if cell.underline() {
                painter.hline(
                    cell_rect.x_range(),
                    cell_rect.bottom() - 1.0,
                    egui::Stroke::new(1.0, fg),
                );
            }
        }
    }
    if cursor_visible && !screen.hide_cursor() && screen.scrollback() == 0 {
        let (row, mut col) = screen.cursor_position();
        if col > 0
            && screen
                .cell(row, col)
                .is_some_and(vt100::Cell::is_wide_continuation)
        {
            col -= 1;
        }
        let Some(cell) = screen.cell(row, col) else {
            return;
        };
        let cursor = egui::Rect::from_min_size(
            rect.min + egui::vec2(2. + f32::from(col) * width, 2. + f32::from(row) * height),
            egui::vec2(width * if cell.is_wide() { 2. } else { 1. }, height),
        );
        let (mut fill, mut foreground) = (color(cell.fgcolor(), FG), background(cell.bgcolor()));
        if fill == foreground {
            fill = Color32::WHITE;
            foreground = Color32::BLACK;
        }
        painter.rect_filled(cursor, 0, fill);
        if cell.has_contents() {
            text(
                &painter,
                cursor.min,
                cell.contents(),
                font,
                foreground,
                baseline,
            );
        }
    }
}
/// A separate galley per terminal cell must still use the same baseline.
/// Aligning every fallback face's top puts CJK and Latin at different heights.
fn text(
    painter: &egui::Painter,
    pos: egui::Pos2,
    contents: String,
    font: &egui::FontId,
    color: Color32,
    baseline: f32,
) {
    let galley = layout(painter, contents, font, color);
    painter.galley(text_origin(pos, &galley, baseline), galley, color);
}

fn layout(
    painter: &egui::Painter,
    contents: String,
    font: &egui::FontId,
    color: Color32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(contents, font.clone(), color, f32::INFINITY);
    job.align_on_baseline = true;
    painter.layout_job(job)
}

fn first_baseline(galley: &egui::Galley) -> Option<f32> {
    galley
        .rows
        .first()
        .and_then(|row| row.glyphs.first().map(|glyph| row.pos.y + glyph.pos.y))
}

fn centered_baseline(painter: &egui::Painter, font: &egui::FontId, height: f32) -> f32 {
    // Center capitals and digits using their ink, then share the baseline across the grid.
    let sample = layout(painter, "H0".into(), font, FG);
    let bounds = if sample.mesh_bounds.is_positive() {
        sample.mesh_bounds
    } else {
        sample.rect
    };
    height * 0.5 + first_baseline(&sample).unwrap_or(bounds.center().y) - bounds.center().y
}

fn text_origin(pos: egui::Pos2, galley: &egui::Galley, baseline: f32) -> egui::Pos2 {
    pos + egui::vec2(0., baseline - first_baseline(galley).unwrap_or(baseline))
}

pub(super) fn composition(
    painter: &egui::Painter,
    cursor: egui::Rect,
    contents: &str,
    font: &egui::FontId,
) {
    let galley = layout(painter, contents.to_owned(), font, FG);
    let bounds =
        egui::Rect::from_min_size(cursor.min, egui::vec2(galley.size().x, cursor.height()));
    painter.rect_filled(bounds, 0, BG);
    let baseline = centered_baseline(painter, font, cursor.height());
    painter.galley(text_origin(cursor.min, &galley, baseline), galley, FG);
    painter.hline(
        bounds.x_range(),
        bounds.bottom() - 1.,
        egui::Stroke::new(1., FG),
    );
}
fn background(value: vt100::Color) -> Color32 {
    if matches!(value, vt100::Color::Default | vt100::Color::Idx(0)) {
        BG
    } else {
        color(value, BG)
    }
}
fn color(color: vt100::Color, default: Color32) -> Color32 {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        vt100::Color::Idx(index) => {
            const BASE: [[u8; 3]; 16] = [
                [0, 0, 0],
                [205, 0, 0],
                [0, 205, 0],
                [205, 205, 0],
                [0, 0, 238],
                [205, 0, 205],
                [0, 205, 205],
                [229, 229, 229],
                [127, 127, 127],
                [255, 0, 0],
                [0, 255, 0],
                [255, 255, 0],
                [92, 92, 255],
                [255, 0, 255],
                [0, 255, 255],
                [255, 255, 255],
            ];
            let [r, g, b] = if index < 16 {
                BASE[usize::from(index)]
            } else if index >= 232 {
                let gray = 8 + (index - 232) * 10;
                [gray, gray, gray]
            } else {
                let value = index - 16;
                let level = |n: u8| if n == 0 { 0 } else { 55 + 40 * n };
                [level(value / 36), level((value / 6) % 6), level(value % 6)]
            };
            Color32::from_rgb(r, g, b)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn mixed_cjk_pid_combining_marks_and_cursor_share_a_baseline_at_each_dpi() -> anyhow::Result<()>
    {
        for dpi in [1., 1.25, 1.5, 2.] {
            for lcd in [false, true] {
                let context = egui::Context::default();
                let mut fonts = egui::FontDefinitions::default();
                let mono =
                    egui::FontData::from_owned(std::fs::read("C:/Windows/Fonts/consola.ttf")?);
                let mut cjk =
                    egui::FontData::from_owned(std::fs::read("C:/Windows/Fonts/msyh.ttc")?);
                cjk.index = 1;
                fonts.font_data.insert("terminal-mono".into(), mono.into());
                fonts.font_data.insert("terminal-cjk".into(), cjk.into());
                fonts.families.insert(
                    egui::FontFamily::Monospace,
                    vec!["terminal-mono".into(), "terminal-cjk".into()],
                );
                crate::font_assets::install(&context, fonts);
                context.set_theme(egui::Theme::Light);
                context.style_mut_of(egui::Theme::Light, |style| {
                    style.visuals.text_options.lcd_subpixel = lcd
                });
                let mut parser = vt100::Parser::new(2, 100, 0);
                parser.process("[INFO] 终止进程 PID: 2107207 e\u{301}\r".as_bytes());
                let mut input = egui::RawInput::default();
                input
                    .viewports
                    .get_mut(&egui::ViewportId::ROOT)
                    .unwrap()
                    .native_pixels_per_point = Some(dpi);
                let mut row_center = 0.;
                let mut output = context.run_ui(input, |ui| {
                    let font = egui::FontId::monospace(13.);
                    row_center = ui.max_rect().min.y + 2. + 9.;
                    let width = ui.fonts_mut(|fonts| fonts.glyph_width(&font, 'M'));
                    screen(
                        ui,
                        parser.screen(),
                        ui.max_rect(),
                        &font,
                        egui::vec2(width, 18.),
                        None,
                        true,
                    );
                });
                let mut baselines = Vec::new();
                let mut fallback = false;
                let mut digit_center = None;
                for shape in &output.shapes {
                    if let egui::Shape::Text(text) = &shape.shape {
                        fallback |= text.galley.job.text.contains('进');
                        if text.galley.job.text == "0" {
                            digit_center = Some(text.visual_bounding_rect().center().y);
                        }
                        for row in &text.galley.rows {
                            for glyph in &row.glyphs {
                                baselines.push(text.pos.y + row.pos.y + glyph.pos.y);
                            }
                        }
                    }
                }
                assert!(fallback && baselines.len() > 25);
                assert!(
                    baselines.iter().all(|y| (y - baselines[0]).abs() < 0.001),
                    "DPI {dpi}, LCD {lcd}: {baselines:?}"
                );
                assert!((digit_center.unwrap() - row_center).abs() <= 1. / dpi);
                output.textures_delta.clear();
            }
        }
        Ok(())
    }
    fn frame(screen: &vt100::Screen, visible: bool) -> egui::FullOutput {
        let context = egui::Context::default();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(200., 100.),
                )),
                ..Default::default()
            },
            |ui| {
                super::screen(
                    ui,
                    screen,
                    ui.max_rect(),
                    &egui::FontId::monospace(13.),
                    egui::vec2(8., 18.),
                    None,
                    visible,
                );
            },
        );
        output.textures_delta.clear();
        output
    }
    fn block(output: &egui::FullOutput, fill: Color32) -> Option<egui::Rect> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect) if rect.fill == fill => Some(rect.rect),
            _ => None,
        })
    }
    #[test]
    fn reverse_header_and_shell_prompt_use_the_terminal_grid() {
        let lines = [
            "USER PR NI  VIRT   RES",
            "root 20  0 25604 13128",
            "root@host:~# ",
        ];
        let mut parser = vt100::Parser::new(3, 24, 0);
        parser.process(
            format!("\x1b[7m{}\x1b[0m\r\n{}\r\n{}", lines[0], lines[1], lines[2]).as_bytes(),
        );
        let output = frame(parser.screen(), true);
        let mut text = output.shapes.iter().filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if !text.galley.job.text.trim().is_empty() => Some(text),
            _ => None,
        });
        let mut row_baseline = None;
        for (row, line) in lines.iter().enumerate() {
            for (col, character) in line.char_indices().filter(|(_, ch)| *ch != ' ') {
                let shape = text.next().expect("missing character");
                assert_eq!(shape.galley.job.text, character.to_string());
                assert_eq!(shape.pos.x, 2. + col as f32 * 8.);
                let placed = &shape.galley.rows[0];
                let top = 2. + row as f32 * 18.;
                let baseline = shape.pos.y + placed.pos.y + placed.glyphs[0].pos.y - top;
                assert!((baseline - *row_baseline.get_or_insert(baseline)).abs() < 0.001);
                if character == '0' {
                    assert!((shape.visual_bounding_rect().center().y - (top + 9.)).abs() <= 1.);
                }
            }
        }
        assert!(text.next().is_none());
        assert!(output.shapes.iter().any(|shape| matches!(
            &shape.shape,
            egui::Shape::Rect(r) if r.fill == FG && r.rect == egui::Rect::from_min_size(
                egui::pos2(2. + lines[2].len() as f32 * 8., 2. + 2. * 18.),
                egui::vec2(8., 18.),
            )
        )));
    }
    #[test]
    fn blinking_block_disappears_without_erasing_the_underlying_character() {
        let mut parser = vt100::Parser::new(2, 8, 20);
        parser.process(b"A\r");
        let on = frame(parser.screen(), true);
        assert_eq!(
            block(&on, FG),
            Some(egui::Rect::from_min_size(
                egui::pos2(2., 2.),
                egui::vec2(8., 18.)
            ))
        );
        assert!(on.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "A" && text.fallback_color == BG)));
        let off = frame(parser.screen(), false);
        assert!(block(&off, FG).is_none());
        assert!(off.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "A" && text.fallback_color == FG)));
    }
    #[test]
    fn remote_hidden_cursor_and_scrollback_do_not_paint_a_block() {
        let mut parser = vt100::Parser::new(2, 8, 20);
        parser.process(b"A\x1b[?25l");
        assert!(block(&frame(parser.screen(), true), FG).is_none());
        parser.process(b"\x1b[?25h\r\nb\r\nc");
        parser.set_scrollback(1);
        assert!(parser.screen().scrollback() > 0);
        assert!(block(&frame(parser.screen(), true), FG).is_none());
    }
    #[test]
    fn scrollback_beyond_one_screen_paints_only_the_visible_history() {
        let mut parser = vt100::Parser::new(3, 16, 40);
        let output = (1..=20)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\r\n");
        parser.process(output.as_bytes());
        for (offset, expected) in [
            (0, ["line 18", "line 19", "line 20"]),
            (2, ["line 16", "line 17", "line 18"]),
            (3, ["line 15", "line 16", "line 17"]),
            (10, ["line 8", "line 9", "line 10"]),
            (usize::MAX, ["line 1", "line 2", "line 3"]),
            (0, ["line 18", "line 19", "line 20"]),
        ] {
            parser.set_scrollback(offset);
            let screen = parser.screen();
            assert_eq!(screen.rows(0, 16).collect::<Vec<_>>(), expected);
            assert!(screen.cell(3, 0).is_none());
            assert_eq!(block(&frame(screen, true), FG).is_some(), offset == 0);
        }
    }
    #[test]
    fn cursor_uses_the_whole_wide_character_and_legacy_attribute_colors() {
        let mut parser = vt100::Parser::new(2, 8, 0);
        parser.process("\x1b[31;44m中\x1b[1;2H".as_bytes());
        assert_eq!(
            block(&frame(parser.screen(), true), Color32::from_rgb(205, 0, 0)),
            Some(egui::Rect::from_min_size(
                egui::pos2(2., 2.),
                egui::vec2(16., 18.)
            ))
        );
        let mut parser = vt100::Parser::new(2, 8, 0);
        parser.process(b"\x1b[31;41mA\r");
        assert!(block(&frame(parser.screen(), true), Color32::WHITE).is_some());
    }
}
