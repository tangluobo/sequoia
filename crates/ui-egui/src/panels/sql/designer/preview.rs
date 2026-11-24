//! Read-only selectable SQL, original palette, 60 px gutter and synchronized folds.
use super::*;
fn layout(ui: &Ui, buffer: &dyn egui::TextBuffer, _width: f32) -> std::sync::Arc<egui::Galley> {
    super::super::highlight::bold_font(ui.ctx());
    let bold = ui.ctx().fonts(|f| {
        f.definitions()
            .families
            .contains_key(&egui::FontFamily::Name("sql-bold".into()))
    });
    let text = buffer.as_str();
    let mut job = egui::text::LayoutJob::default();
    let plain = egui::TextFormat {
        font_id: egui::FontId::monospace(13.),
        line_height: Some(17.),
        color: Color32::from_gray(51),
        ..Default::default()
    };
    const WORDS: &str = "SELECT FROM WHERE INSERT INTO VALUES UPDATE SET DELETE CREATE DROP ALTER TABLE INDEX VIEW DATABASE AND OR NOT IN EXISTS BETWEEN LIKE IS NULL JOIN INNER LEFT RIGHT OUTER FULL CROSS ON GROUP BY ORDER HAVING LIMIT OFFSET UNION ALL AS DISTINCT CASE WHEN THEN ELSE END COUNT SUM AVG MIN MAX PRIMARY KEY FOREIGN REFERENCES CONSTRAINT DEFAULT CHECK UNIQUE AUTO_INCREMENT IF CASCADE RENAME TO BEGIN COMMIT ROLLBACK TRANSACTION GRANT REVOKE PRIVILEGES SHOW DESCRIBE EXPLAIN USE TRUNCATE CHARACTER COLLATE REPLACE COMMENT COLUMN MODIFY ADD";
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        let start = at;
        let mut format = plain.clone();
        if bytes[at] == b'\'' {
            at += 1;
            at += text[at..].find('\'').map_or(bytes.len() - at, |n| n + 1);
            format.color = Color32::from_rgb(163, 21, 21);
        } else if text[at..].starts_with("--") {
            at += text[at..].find('\n').unwrap_or(bytes.len() - at);
            format.color = Color32::from_rgb(106, 153, 85);
            format.italics = true;
        } else if text[at..].starts_with("/*")
            && let Some(end) = text[at + 2..].find("*/")
        {
            at += end + 4;
            format.color = Color32::from_rgb(106, 153, 85);
            format.italics = true;
        } else if bytes[at].is_ascii_digit()
            && (at == 0 || !bytes[at - 1].is_ascii_alphanumeric() && bytes[at - 1] != b'_')
        {
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                at += 1;
            }
            if bytes.get(at) == Some(&b'.') && bytes.get(at + 1).is_some_and(u8::is_ascii_digit) {
                at += 1;
                while at < bytes.len() && bytes[at].is_ascii_digit() {
                    at += 1;
                }
            }
            format.color = Color32::from_rgb(9, 134, 88);
        } else if bytes[at].is_ascii_alphabetic() || bytes[at] == b'_' {
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
            if WORDS
                .split_ascii_whitespace()
                .any(|word| word.eq_ignore_ascii_case(&text[start..at]))
            {
                format.color = Color32::BLUE;
                if bold {
                    format.font_id =
                        egui::FontId::new(13., egui::FontFamily::Name("sql-bold".into()));
                }
            }
        } else {
            at += text[at..].chars().next().unwrap().len_utf8();
        }
        job.append(&text[start..at], 0., format);
    }
    ui.painter().layout_job(job)
}
pub(super) fn render(ui: &mut Ui, draft: &mut Draft) {
    let key = (draft.revision, draft.preview_save_as);
    if draft.preview_view.key != Some(key) {
        draft.preview_view.reset(&draft.preview, key);
    }
    let bounds = ui.max_rect();
    let gutter = Rect::from_min_size(bounds.min, vec2(60., bounds.height()));
    ui.painter().rect_filled(gutter, 0, Color32::from_gray(248));
    let mut output = None;
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(Rect::from_min_max(
            pos2(gutter.right(), bounds.top() + 4.),
            bounds.max,
        )),
        |ui| {
            let id = draft.preview_editor.prepare(ui, "table-design-sql");
            let mut text = draft.preview_view.text.as_deref().unwrap_or(&draft.preview);
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    output = Some(
                        egui::TextEdit::multiline(&mut text)
                            .id(id)
                            .code_editor()
                            .frame(egui::Frame::NONE.inner_margin(egui::Margin::same(2)))
                            .margin(vec2(2., 2.))
                            .desired_rows(1)
                            .desired_width(f32::INFINITY)
                            .layouter(&mut layout)
                            .show(ui),
                    );
                });
        },
    );
    let Some(output) = output else { return };
    let painter = ui.painter_at(gutter);
    let top = output.galley_pos.y;
    let start = output
        .galley
        .rows
        .partition_point(|row| top + row.pos.y + row.size.y < gutter.top());
    let mut toggle = None;
    for (index, row) in output.galley.rows.iter().enumerate().skip(start) {
        let y = top + row.pos.y;
        if y > gutter.bottom() {
            break;
        }
        let Some(line) = draft.preview_view.visible.get(index).copied() else {
            break;
        };
        let fold = draft
            .preview_view
            .folds
            .iter()
            .any(|(start, _)| *start == line);
        if fold {
            let rect = Rect::from_min_size(pos2(gutter.left() + 2., y), vec2(16., 17.));
            let response = ui.interact(rect, ui.id().with(("preview-fold", line)), Sense::click());
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                if draft.preview_view.collapsed.contains(&line) {
                    "▶"
                } else {
                    "▼"
                },
                egui::FontId::proportional(10.),
                Color32::from_gray(85),
            );
            if response.clicked() {
                toggle = Some(line);
            }
        }
        painter.text(
            pos2(gutter.left() + if fold { 19. } else { 12. }, y),
            Align2::LEFT_TOP,
            (line + 1).to_string(),
            egui::FontId::monospace(13.),
            Color32::from_gray(136),
        );
    }
    if let Some(line) = toggle {
        draft.preview_view.toggle(&draft.preview, line);
        draft.preview_editor.release(ui.ctx());
        draft.preview_editor.reset();
    }
}
