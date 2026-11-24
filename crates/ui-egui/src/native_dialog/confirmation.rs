//! Original JavaFX warning alert. Draw from the root controller, outside native callbacks.
use super::message::{self, Button, Kind, Message, Role};
use egui::Rect;
pub(crate) enum Answer {
    Yes,
    No,
}
pub(crate) struct Question<'a> {
    pub id: egui::ViewportId,
    pub owner: Option<Rect>,
    pub title: &'a str,
    pub header: &'a str,
    pub content: &'a str,
    pub active: bool,
}
pub(crate) fn show(
    ctx: &egui::Context,
    question: Question<'_>,
    focus: &mut bool,
    icons: &mut crate::icons::Icons,
    mut observe: impl FnMut(&egui::Ui, egui::ViewportId, [Rect; 2]),
) -> Option<Answer> {
    let id = question.id;
    message::show(
        ctx,
        Message {
            id,
            owner: question.owner,
            title: question.title,
            header: Some(question.header),
            content: question.content,
            kind: Kind::Warning,
            buttons: &[
                Button {
                    label: "是",
                    role: Role::Yes,
                },
                Button {
                    label: "否",
                    role: Role::No,
                },
            ],
            active: question.active,
        },
        focus,
        icons,
        |ui, rects| observe(ui, id, [rects[0], rects[1]]),
    )
    .map(|answer| match answer {
        message::Answer::Button(0) => Answer::Yes,
        _ => Answer::No,
    })
}
