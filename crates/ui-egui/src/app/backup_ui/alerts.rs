//! Completion/error alerts use a separate native window, like the Java wizard.
use super::{Draft, preview};
use crate::{
    icons::Icons,
    native_dialog::message::{self, Answer, Button, Kind, Message, Role},
};
#[cfg(test)]
mod tests;

/// Returns true only after acknowledging a successful operation. Errors keep
/// the wizard and its options/log intact so the operation can be retried.
pub(super) fn show(ctx: &egui::Context, draft: &mut Draft, icons: &mut Icons) -> bool {
    let object_count = draft.total();
    let success = draft.complete.is_some();
    let Some(content) = draft.complete.as_deref().or(draft.notice.as_deref()) else {
        draft.alert_focus = true;
        return false;
    };
    let failed = !success && draft.error.as_deref() == Some(content);
    let restoring = draft.restore.is_some();
    let title = match (success, failed, restoring) {
        (true, _, true) => "还原完成",
        (true, _, false) => "备份完成",
        (_, true, true) => "还原失败",
        (_, true, false) => "备份失败",
        _ => "提示",
    };
    let id = egui::ViewportId::from_hash_of(("backup-alert", &draft.id));
    let answered = message::show(
        ctx,
        Message {
            id,
            owner: draft.owner_rect,
            title,
            header: None,
            content,
            kind: if failed {
                Kind::Error
            } else {
                Kind::Information
            },
            buttons: &[Button {
                label: "确定",
                role: Role::Ok,
            }],
            active: !preview::active(),
        },
        &mut draft.alert_focus,
        icons,
        |ui, bounds| preview::capture_alert(ui, id, bounds, object_count),
    );
    if matches!(answered, Some(Answer::Button(_)) | Some(Answer::Closed)) {
        draft.alert_focus = true;
        if success {
            return true;
        }
        draft.notice = None;
        ctx.send_viewport_cmd_to(
            egui::ViewportId::from_hash_of(("backup-dialog", &draft.id)),
            egui::ViewportCommand::Focus,
        );
    }
    false
}
