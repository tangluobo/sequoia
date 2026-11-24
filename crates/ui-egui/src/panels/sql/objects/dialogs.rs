use super::*;
use crate::native_dialog::message::{self, Answer, Button, Kind, Message, Role};
pub(super) fn show(
    ctx: &egui::Context,
    state: &mut State,
    icons: &mut Icons,
    id: &str,
) -> Option<Action> {
    let mut action = None;
    if let Some(confirmation) = &state.confirmation {
        let mutation = confirmation.mutation;
        let tables = confirmation
            .targets
            .iter()
            .filter(|target| !target.view)
            .count();
        let views = confirmation.targets.len() - tables;
        let (title, content, label, kind) = match mutation {
            Mutation::Drop => {
                let counts = [(tables, "张表"), (views, "个视图")]
                    .into_iter()
                    .filter(|(n, _)| *n > 0)
                    .map(|(n, label)| format!("{n} {label}"))
                    .collect::<Vec<_>>()
                    .join("、");
                (
                    "确认删除",
                    format!("确定要删除选中的 {counts}吗？此操作不可恢复！"),
                    "确认删除",
                    Kind::Confirmation,
                )
            }
            Mutation::Clear => (
                "清空确认",
                format!(
                    "确定要清空选中的 {tables} 张表的数据吗？\n此操作将删除所有数据（DELETE FROM），不可恢复！"
                ),
                "确认清空",
                Kind::Warning,
            ),
            Mutation::Truncate => (
                "截断确认",
                format!(
                    "确定要截断选中的 {tables} 张表吗？\n此操作将删除所有数据（TRUNCATE TABLE），不可恢复，且会重置自增列！"
                ),
                "确认截断",
                Kind::Warning,
            ),
        };
        let answer = message::show(
            ctx,
            Message {
                id: egui::ViewportId::from_hash_of(("objects-confirm", id)),
                owner: ctx.input(|i| i.viewport().inner_rect),
                title,
                header: None,
                content: &content,
                kind,
                buttons: &[
                    Button {
                        label,
                        role: Role::Other,
                    },
                    Button {
                        label: "取消",
                        role: Role::Cancel,
                    },
                ],
                active: true,
            },
            &mut state.alert_focus,
            icons,
            |_, _| {},
        );
        if let Some(answer) = answer {
            let confirmation = state.confirmation.take().unwrap();
            state.alert_focus = true;
            ctx.request_repaint_of(egui::ViewportId::ROOT);
            if matches!(answer, Answer::Button(0)) {
                action = Some(Action::Mutate(tomato_core::database_objects::Request {
                    targets: confirmation.targets,
                    mutation: match mutation {
                        Mutation::Drop => tomato_core::database_objects::Mutation::Drop,
                        Mutation::Clear => tomato_core::database_objects::Mutation::Clear,
                        Mutation::Truncate => tomato_core::database_objects::Mutation::Truncate,
                    },
                }));
            }
        }
    } else if let Some(notice) = &state.notice {
        let answer = message::show(
            ctx,
            Message {
                id: egui::ViewportId::from_hash_of(("objects-error", id)),
                owner: ctx.input(|i| i.viewport().inner_rect),
                title: "操作失败",
                header: None,
                content: notice,
                kind: Kind::Error,
                buttons: &[Button {
                    label: "确定",
                    role: Role::Ok,
                }],
                active: true,
            },
            &mut state.alert_focus,
            icons,
            |_, _| {},
        );
        if answer.is_some() {
            state.notice = None;
            state.alert_focus = true;
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    }
    action
}
