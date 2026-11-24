use super::*;
use crate::native_dialog::{
    self,
    message::{self, Button, Kind, Message, Role},
};
pub(super) fn render(
    ctx: &egui::Context,
    draft: &mut Draft,
    icons: &mut crate::icons::Icons,
) -> Option<String> {
    if let Some(notice) = &mut draft.notice {
        let answer = message::show(
            ctx,
            Message {
                id: egui::ViewportId::from_hash_of("table-design-notice"),
                owner: ctx.input(|i| i.viewport().inner_rect),
                title: &notice.title,
                header: notice.header.as_deref(),
                content: &notice.text,
                kind: if notice.error {
                    Kind::Error
                } else if notice.header.is_some() {
                    Kind::Warning
                } else {
                    Kind::Information
                },
                buttons: &[Button {
                    label: "确定",
                    role: Role::Ok,
                }],
                active: true,
            },
            &mut notice.focus,
            icons,
            |_, _| {},
        );
        if answer.is_some() {
            draft.notice = None;
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    }
    let name = draft.name.as_mut()?;
    let size = vec2(218., 154.);
    let mut builder = egui::ViewportBuilder::default()
        .with_title("新建表")
        .with_inner_size(size)
        .with_resizable(false);
    if name.focus
        && let Some(rect) = ctx.input(|i| i.viewport().inner_rect)
    {
        builder = builder.with_position(rect.center() - size / 2.);
    }
    let answer = ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("table-design-name"),
        builder,
        |ui, _| {
            let origin = ui.max_rect().min;
            let at = |x, y, w, h| Rect::from_min_size(origin + vec2(x, y), vec2(w, h));
            ui.painter()
                .rect_filled(ui.max_rect(), 0, Color32::from_gray(244));
            native_dialog::gradient(
                ui,
                at(0., 0., 218., 68.),
                Color32::from_gray(249),
                Color32::from_gray(242),
            );
            ui.painter().text(
                origin + vec2(10., 34.),
                Align2::LEFT_CENTER,
                "请输入表名",
                native_dialog::font(ui.ctx(), 14.),
                Color32::BLACK,
            );
            icons.paint(
                ui,
                "upstream/openjfx/dialog-confirm.png",
                at(160., 10., 48., 48.),
            );
            ui.painter().line_segment(
                [origin + vec2(0., 67.), origin + vec2(218., 67.)],
                Stroke::new(1., Color32::from_gray(200)),
            );
            ui.painter().text(
                origin + vec2(10., 89.5),
                Align2::LEFT_CENTER,
                "表名:",
                native_dialog::font(ui.ctx(), 12.),
                Color32::BLACK,
            );
            let id = name
                .editor
                .prepare_id(ui.ctx(), egui::Id::new("table-design-name-input"));
            if name.focus {
                ui.memory_mut(|memory| memory.request_focus(id));
                name.editor.select(ui.ctx(), 0..name.text.chars().count());
                name.focus = false;
            }
            ui.put(
                at(47., 78., 161., 23.),
                TextEdit::singleline(&mut name.text)
                    .id(id)
                    .font(native_dialog::font(ui.ctx(), 12.))
                    .desired_width(161.)
                    .min_size(vec2(161., 23.))
                    .margin(vec2(6., 3.))
                    .char_limit(1024),
            );
            let ok = native_dialog::button(ui, at(48., 121., 75., 23.), "确定", true);
            let cancel = native_dialog::button(ui, at(133., 121., 75., 23.), "取消", false);
            let canceled = cancel.clicked()
                || ui.input(|i| i.key_pressed(egui::Key::Escape) || i.viewport().close_requested());
            let accepted =
                !canceled && (ok.clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if accepted || canceled {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                Some(accepted)
            } else {
                None
            }
        },
    );
    if let Some(accepted) = answer {
        let name = draft.name.take().unwrap();
        name.editor.release(ctx);
        if accepted {
            return Some(name.text.trim_matches(|c: char| c <= ' ').into());
        }
    }
    None
}
