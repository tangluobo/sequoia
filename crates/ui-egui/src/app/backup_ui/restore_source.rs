//! Pick an archive from the same scoped directory cache as the backup tree.
use super::{draft::Draft, view::button};
use crate::{app::backup_tree::Directory, compact_menu::Item};
use egui::{Popup, PopupCloseBehavior, Rect, RectAlign, ScrollArea, Ui};

pub(super) enum Choice {
    Archive(String),
    OpenDirectory(String),
    Parent,
    Reload,
}

pub(super) fn render(
    ui: &mut Ui,
    rect: Rect,
    draft: &Draft,
    directory: Option<&Directory>,
) -> Option<Choice> {
    let caption = draft.restore.as_deref().unwrap_or("选择备份文件");
    let response = ui
        .add_enabled_ui(!draft.running, |ui| button(ui, rect, caption, false))
        .inner;
    Popup::menu(&response)
        .align(RectAlign::BOTTOM_START)
        .width(rect.width())
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(rect.width());
            if !draft.scope.path.is_empty() && ui.add(Item::new("返回上一级")).clicked() {
                return Some(Choice::Parent);
            }
            let Some(directory) = directory else {
                return ui
                    .add(Item::new("刷新"))
                    .clicked()
                    .then_some(Choice::Reload);
            };
            if directory.loading {
                ui.add_enabled(false, Item::new("正在读取备份目录…"));
                return None;
            }
            if let Some(error) = &directory.error {
                ui.add_enabled(false, Item::new(error).max_width(rect.width()));
                return ui
                    .add(Item::new("重试"))
                    .clicked()
                    .then_some(Choice::Reload);
            }
            if directory.items.is_empty() {
                ui.add_enabled(false, Item::new("此目录没有备份文件"));
                return ui
                    .add(Item::new("刷新"))
                    .clicked()
                    .then_some(Choice::Reload);
            }
            ui.spacing_mut().item_spacing.y = 0.;
            ScrollArea::vertical()
                .id_salt(&draft.scope.path)
                .max_height(230.)
                .show_rows(
                    ui,
                    crate::compact_menu::ROW_HEIGHT,
                    directory.items.len(),
                    |ui, rows| {
                        for item in &directory.items[rows] {
                            let caption = if item.directory {
                                format!("{}/", item.name)
                            } else {
                                format!("{}.nb3", item.name)
                            };
                            if ui
                                .add(Item::new(&caption).max_width(rect.width()))
                                .clicked()
                            {
                                if item.directory {
                                    return Some(Choice::OpenDirectory(item.name.clone()));
                                }
                                ui.close();
                                return Some(Choice::Archive(item.name.clone()));
                            }
                        }
                        None
                    },
                )
                .inner
        })
        .and_then(|result| result.inner)
}
