use super::*;
mod actions;
mod find;
mod highlight;
mod markdown;
mod preview;
#[cfg(windows)]
pub(crate) use preview::print_document;
mod rectangle;
pub(super) use highlight::ensure_bold_cjk;

pub(super) fn render(ui: &mut Ui, tab: &mut Tab) -> Option<Intent> {
    let Panel::Editor {
        path,
        text,
        saved,
        markdown,
    } = &mut tab.panel
    else {
        return None;
    };
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| {
            ["md", "markdown", "mdown", "mkd"]
                .iter()
                .any(|v| ext.eq_ignore_ascii_case(v))
        })
    {
        return markdown::render(ui, tab);
    }
    let editor_id = ui.make_persistent_id(("document-editor", &tab.id));
    markdown.editor_id = Some(editor_id);
    let mut intent = None;
    ui.horizontal(|ui| {
        ui.label(path.display().to_string());
        if ui
            .add_enabled(!tab.busy, egui::Button::new("保存 Ctrl+S"))
            .clicked()
        {
            intent = Some(Intent::Save(path.clone(), text.clone()));
        }
        if text != saved {
            ui.label(RichText::new("未保存").color(Color32::from_rgb(210, 145, 40)));
        }
    });
    if !tab.busy && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
        intent = Some(Intent::Save(path.clone(), text.clone()));
    }
    ScrollArea::both().show(ui, |ui| {
        ui.add_enabled(
            !tab.busy,
            TextEdit::multiline(text)
                .id(editor_id)
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(40),
        );
    });

    crate::tabs::markdown::trim_undo(ui.ctx(), editor_id);
    intent
}
