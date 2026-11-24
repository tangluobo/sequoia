//! One Java file-browser menu for list, icon, column and empty-area surfaces.
use super::*;
use crate::tabs::ssh::Pane;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Open,
    Preview,
    Markdown,
    Download,
    AccessUrl,
    Copy,
    Paste,
    Mkdir,
    Upload,
    Create,
    Delete,
    Rename,
    View(&'static str),
    Refresh,
}
impl Action {
    fn key(self) -> &'static str {
        match self {
            Self::Open => "打开",
            Self::Preview => "预览图片",
            Self::Markdown => "编辑 Markdown",
            Self::Download => "下载...",
            Self::AccessUrl => "复制访问地址",
            Self::Copy => "复制",
            Self::Paste => "粘贴上传",
            Self::Mkdir => "新建目录",
            Self::Upload => "上传文件...",
            Self::Create => "创建文件",
            Self::Delete => "删除",
            Self::Rename => "重命名",
            Self::View(mode) => mode,
            Self::Refresh => "刷新",
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct Capabilities {
    pub object: bool,
    pub rich: bool,
    pub create: bool,
    pub columns: bool,
    pub buckets: bool,
}
impl Capabilities {
    fn of(connection: &tomato_core::Connection) -> Self {
        let object = tomato_services::files::object_copy::is_object(connection);
        Self {
            object,
            rich: connection.kind() != "FTP",
            create: connection.kind() != "FTP",
            columns: connection.kind() != "FTP",
            buckets: object,
        }
    }
}
pub(super) fn markdown(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| {
        ["md", "markdown", "mdown", "mkd"]
            .iter()
            .any(|v| ext.eq_ignore_ascii_case(v))
    })
}
fn actions(
    cap: Capabilities,
    first: Option<&tomato_services::files::operations::Entry>,
    count: usize,
    bucket: bool,
    files: bool,
    objects: bool,
) -> Vec<Option<Action>> {
    let single = count == 1;
    let file = single && first.is_some_and(|e| !e.directory);
    let mut items = Vec::with_capacity(20);
    if single {
        items.push(Some(Action::Open));
    }
    if file && cap.rich && first.is_some_and(|e| tomato_services::files::images::supported(&e.name))
    {
        items.push(Some(Action::Preview));
    }
    if file && cap.rich && first.is_some_and(|e| markdown(&e.name)) {
        items.push(Some(Action::Markdown));
    }
    if file {
        items.push(Some(Action::Download));
    }
    if file && cap.buckets {
        items.push(Some(Action::AccessUrl));
    }
    if cap.object && count > 0 {
        items.push(Some(Action::Copy));
    }
    if files || cap.object && objects {
        items.push(Some(Action::Paste));
    }
    if !items.is_empty() {
        items.push(None);
    }
    items.extend([Some(Action::Mkdir), Some(Action::Upload)]);
    if cap.create {
        items.push(Some(Action::Create));
    }
    if count > 0 {
        items.push(Some(Action::Delete));
    }
    if single && !bucket {
        items.push(Some(Action::Rename));
    }
    items.push(None);
    items
}
pub(super) fn show(
    ui: &mut Ui,
    pane: &Pane,
    connection: &tomato_core::Connection,
) -> Option<String> {
    ui.spacing_mut().interact_size.y = crate::compact_menu::ROW_HEIGHT;
    let cap = Capabilities::of(connection);
    let (first, count) = if pane.selected.is_empty() {
        let entry = pane
            .column_selected
            .as_deref()
            .and_then(|path| pane.file_entry(path));
        (entry, usize::from(entry.is_some()))
    } else {
        (
            pane.selected.first().and_then(|i| pane.entries.get(*i)),
            pane.selected.len(),
        )
    };
    let bucket = first.is_some_and(|entry| bucket_icon(connection, &parent(&entry.path)).is_some());
    let files = tomato_services::files::clipboard::files_available();
    let objects = cap.object && tomato_services::files::clipboard::objects_available();
    let mut chosen = None;
    for item in actions(cap, first, count, bucket, files, objects) {
        let Some(item) = item else {
            crate::compact_menu::separator(ui);
            continue;
        };
        let label = match item {
            Action::Copy | Action::Delete if count > 1 => format!("{}({count}项)", item.key()),
            Action::Paste => if files {
                "粘贴上传文件"
            } else {
                "粘贴"
            }
            .into(),
            _ => item.key().into(),
        };
        if ui.add(crate::compact_menu::Item::new(&label)).clicked() {
            chosen = Some(item.key().into());
            ui.close();
        }
    }
    let view = ui.add(crate::compact_menu::Item::new("视图"));
    let center = view.rect.right_center() - egui::vec2(8., 0.);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            center + egui::vec2(-2., -4.),
            center + egui::vec2(2., 0.),
            center + egui::vec2(-2., 4.),
        ],
        if view.hovered() {
            Color32::WHITE
        } else {
            Color32::BLACK
        },
        egui::Stroke::NONE,
    ));
    egui::containers::menu::SubMenu::new().show(ui, &view, |ui| {
        for (mode, label) in [
            ("ICON", "图标视图"),
            ("LIST", "列表视图"),
            ("COLUMN", "列视图"),
        ] {
            if mode == "COLUMN" && !cap.columns {
                continue;
            }
            if ui.add(crate::compact_menu::Item::new(label)).clicked() {
                chosen = Some(Action::View(mode).key().into());
                ui.close();
            }
        }
    });
    crate::compact_menu::separator(ui);
    if ui.add(crate::compact_menu::Item::new("刷新")).clicked() {
        chosen = Some(Action::Refresh.key().into());
        ui.close();
    }
    chosen
}
pub(super) fn background(
    response: egui::Response,
    pane: &mut Pane,
    connection: &tomato_core::Connection,
    action: &mut Option<String>,
) {
    if response.secondary_clicked() {
        pane.selected.clear();
        pane.anchor = None;
        pane.column_selected = None;
        response.request_focus();
        pane.focus = Some(response.id);
    }
    response.context_menu(|ui| {
        if let Some(next) = show(ui, pane, connection) {
            *action = Some(next);
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn java_capabilities_and_multi_selection_menu_order() {
        let entry = tomato_services::files::operations::Entry {
            name: "readme.md".into(),
            path: "/bucket/readme.md".into(),
            directory: false,
            symlink: false,
            size: 0,
            modified: 0,
            permissions: 0,
        };
        let s3 = Capabilities::of(&tomato_core::Connection::new("S3"));
        assert_eq!(
            actions(s3, Some(&entry), 1, false, false, true)
                .into_iter()
                .flatten()
                .collect::<Vec<_>>(),
            [
                Action::Open,
                Action::Markdown,
                Action::Download,
                Action::AccessUrl,
                Action::Copy,
                Action::Paste,
                Action::Mkdir,
                Action::Upload,
                Action::Create,
                Action::Delete,
                Action::Rename
            ]
        );
        let ftp = Capabilities::of(&tomato_core::Connection::new("FTP"));
        assert_eq!(
            actions(ftp, Some(&entry), 2, false, true, false)
                .into_iter()
                .flatten()
                .collect::<Vec<_>>(),
            [Action::Paste, Action::Mkdir, Action::Upload, Action::Delete]
        );
        assert!(!ftp.columns);
        assert!(!actions(s3, Some(&entry), 1, true, false, false).contains(&Some(Action::Rename)));
    }
}
