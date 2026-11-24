use super::*;
#[derive(Clone, Copy)]
enum Kind {
    Directory,
    Rename,
    Delete,
}
pub(crate) struct Dialog {
    target: Target,
    kind: Kind,
    text: String,
    focus: bool,
    editor: tabs::text_state::Editor,
    error: Option<String>,
}
impl Dialog {
    pub(super) fn directory(mut target: Target) -> anyhow::Result<Self> {
        target.path = target.directory()?.path;
        target.item = None;
        Ok(Self {
            target,
            kind: Kind::Directory,
            text: "新目录".into(),
            focus: true,
            editor: Default::default(),
            error: None,
        })
    }
    pub(super) fn rename(target: Target) -> anyhow::Result<Self> {
        let text = target
            .item
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("请选择需要重命名的备份项目"))?
            .name
            .clone();
        Ok(Self {
            target,
            kind: Kind::Rename,
            text,
            focus: true,
            editor: Default::default(),
            error: None,
        })
    }
    pub(super) fn delete(target: Target) -> anyhow::Result<Self> {
        anyhow::ensure!(target.item.is_some(), "请选择需要删除的备份项目");
        Ok(Self {
            target,
            kind: Kind::Delete,
            text: String::new(),
            focus: true,
            editor: Default::default(),
            error: None,
        })
    }
}
impl TomatoApp {
    pub(crate) fn backup_file_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.backup_edit.take() else {
            return;
        };
        let deleting = matches!(dialog.kind, Kind::Delete);
        let directory = dialog
            .target
            .item
            .as_ref()
            .is_some_and(|item| item.directory);
        let title = match dialog.kind {
            Kind::Directory => "新建目录",
            Kind::Rename => {
                if directory {
                    "重命名目录"
                } else {
                    "重命名备份"
                }
            }
            Kind::Delete => {
                if directory {
                    "删除目录"
                } else {
                    "删除备份"
                }
            }
        };
        let size = if deleting {
            egui::vec2(if directory { 361. } else { 360. }, 142.)
        } else if matches!(dialog.kind, Kind::Directory) {
            egui::vec2(309., 101.)
        } else {
            egui::vec2(297., 101.)
        };
        let (mut accept, mut cancel) = (false, false);
        let mut builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size(size)
            .with_resizable(false);
        if dialog.focus
            && let Some(rect) = ctx.input(|input| input.viewport().inner_rect)
        {
            builder = builder.with_position(rect.center() - size / 2.);
        }
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("backup-file-dialog"),
            builder,
            |ui, _| {
                let origin = ui.max_rect().min;
                let at = |x, y, w, h| {
                    egui::Rect::from_min_size(origin + egui::vec2(x, y), egui::vec2(w, h))
                };
                ui.painter()
                    .rect_filled(ui.max_rect(), 0., Color32::from_gray(244));
                ui.style_mut().override_font_id = Some(egui::FontId::proportional(12.));
                self.icons.paint(
                    ui,
                    "upstream/openjfx/dialog-confirm.png",
                    at(if deleting { size.x - 58. } else { 10. }, 10., 48., 48.),
                );
                if deleting {
                    let item = dialog.target.item.as_ref().expect("validated backup item");
                    let message = if directory {
                        format!("确定要删除目录 \"{}\" 及其所有内容吗？", item.name)
                    } else {
                        format!("确定要删除备份 \"{}\" 吗？", item.name)
                    };
                    ui.put(
                        at(10., 18., size.x - 78., 34.),
                        egui::Label::new(message).wrap(),
                    );
                    ui.painter().line_segment(
                        [
                            origin + egui::vec2(0., 67.),
                            origin + egui::vec2(size.x, 67.),
                        ],
                        egui::Stroke::new(1., Color32::from_gray(200)),
                    );
                } else {
                    ui.painter().text(
                        origin + egui::vec2(68., 29.5),
                        egui::Align2::LEFT_CENTER,
                        if matches!(dialog.kind, Kind::Directory) {
                            "目录名称："
                        } else {
                            "新名称："
                        },
                        egui::FontId::proportional(12.),
                        Color32::BLACK,
                    );
                    let id = dialog
                        .editor
                        .prepare_id(ctx, egui::Id::new("backup-file-name"));
                    let output = ui
                        .scope_builder(
                            egui::UiBuilder::new().max_rect(at(size.x - 171., 18., 161., 23.)),
                            |ui| {
                                egui::TextEdit::singleline(&mut dialog.text)
                                    .id(id)
                                    .char_limit(255)
                                    .desired_width(161.)
                                    .min_size(egui::vec2(161., 23.))
                                    .margin(egui::vec2(6., 3.))
                                    .show(ui)
                            },
                        )
                        .inner;
                    if dialog.focus {
                        output.response.request_focus();
                        dialog.editor.select(ctx, 0..dialog.text.chars().count());
                    }
                    dialog.editor.trim(ctx);
                }
                let valid = deleting || !dialog.text.trim().is_empty();
                let ok = ui
                    .add_enabled_ui(valid, |ui| {
                        crate::native_dialog::button(
                            ui,
                            at(size.x - 170., size.y - 33., 75., 23.),
                            "确定",
                            true,
                        )
                    })
                    .inner;
                let no = crate::native_dialog::button(
                    ui,
                    at(size.x - 85., size.y - 33., 75., 23.),
                    "取消",
                    false,
                );
                if deleting && dialog.focus {
                    no.request_focus();
                }
                dialog.focus = false;
                accept = ok.clicked()
                    || valid && !deleting && ui.input(|input| input.key_pressed(egui::Key::Enter));
                cancel = no.clicked()
                    || ui.input(|input| {
                        input.viewport().close_requested() || input.key_pressed(egui::Key::Escape)
                    });
                if let Some(error) = dialog.error.clone() {
                    accept = false;
                    cancel = ui.input(|input| input.viewport().close_requested());
                    egui::Modal::new(egui::Id::new("backup-file-admission-error")).show(
                        ui.ctx(),
                        |ui| {
                            ui.label("操作失败");
                            ui.label(error);
                            if ui.button("确定").clicked()
                                || ui.input(|input| input.key_pressed(egui::Key::Escape))
                            {
                                dialog.error = None;
                            }
                        },
                    );
                }
            },
        );
        if cancel {
            dialog.editor.release(ctx);
            return;
        }
        if !accept {
            self.backup_edit = Some(dialog);
            return;
        }
        let scope = dialog.target.parent();
        let action = match dialog.kind {
            Kind::Directory => FileAction::CreateDirectory {
                name: dialog.text.trim().into(),
            },
            Kind::Rename => FileAction::Rename {
                item: dialog.target.item.clone().expect("validated backup item"),
                name: dialog.text.trim().into(),
            },
            Kind::Delete => FileAction::Delete {
                items: vec![dialog.target.item.clone().expect("validated backup item")],
            },
        };
        match self.backup_file_command(&dialog.target.connection.id, scope, action, JobKind::Change)
        {
            Ok(()) => dialog.editor.release(ctx),
            Err(error) => {
                dialog.error = Some(error.to_string());
                self.backup_edit = Some(dialog);
            }
        }
    }
}
