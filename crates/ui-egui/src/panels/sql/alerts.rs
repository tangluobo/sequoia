//! Keep final database failures out of the loaded table's layout.
use crate::native_dialog::message::{self, Answer, Button, Kind, Message, Role};
use crate::{icons::Icons, tabs::workbench::Workbench};
pub(super) fn show(ctx: &egui::Context, tab: &str, state: &mut Workbench, icons: &mut Icons) {
    let Some(notice) = &mut state.failure else {
        return;
    };
    let answer = message::show(
        ctx,
        Message {
            id: egui::ViewportId::from_hash_of(("database-error", tab)),
            owner: ctx.input(|input| input.viewport().inner_rect),
            title: "数据库操作失败",
            header: None,
            content: &notice.text,
            kind: Kind::Error,
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
    if matches!(answer, Some(Answer::Button(_)) | Some(Answer::Closed)) {
        state.failure = None;
    }
}
