use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::{Duration, Instant},
};
use tomato_services::terminal::{
    Status, Terminal,
    ssh::tools::{Entry, Operation, Output, Section, Table},
};
mod copy;
mod files;
mod monitor;
mod rename;
pub(crate) use files::{join, parent};
pub use monitor::{Counters, Sample};
pub use rename::RenameDraft;
pub mod images;
pub mod inspect;
pub mod storage;
pub mod upload;

pub(crate) struct FileDrag {
    pub start: egui::Vec2,
    pub context: (String, String, String),
}
pub struct Pane {
    pub(crate) export: Option<Box<FileExport>>,
    pub(crate) file_drag: Option<FileDrag>,
    pub thumbnails: images::Thumbnails,
    pub table: Option<Table>,
    pub entries: Vec<Entry>,
    pub columns: Vec<(String, Vec<Entry>)>,
    pub path: String,
    pub path_input: Option<String>,
    pub filter: String,
    pub(crate) file_indices: Option<files::FileIndices>,
    pub file_sort: Option<(usize, bool)>,
    pub(crate) file_widths: Option<Box<[f32]>>,
    pub(crate) manager_widths: Option<Box<[f32]>>,
    pub selected: BTreeSet<usize>,
    pub anchor: Option<usize>,
    pub(crate) file_cursor: Option<usize>,
    pub(crate) content_bounds: Option<egui::Rect>,
    pub focus: Option<egui::Id>,
    pub rename: Option<RenameDraft>,
    pub(crate) column_selected: Option<String>,
    pub refresh: bool,
    pub status: String,
    pub notice_after_refresh: Option<(String, String)>,
    pub paste_shortcut_active: bool,
    pub tcp: bool,
    pub udp: bool,
    pub updated: Option<Instant>,
}
impl Default for Pane {
    fn default() -> Self {
        Self {
            file_drag: None,
            export: None,
            thumbnails: Default::default(),
            table: None,
            entries: Vec::new(),
            columns: Vec::new(),
            path: ".".into(),
            path_input: None,
            filter: String::new(),
            file_indices: None,
            file_sort: None,
            file_widths: None,
            manager_widths: None,
            selected: BTreeSet::new(),
            anchor: None,
            file_cursor: None,
            content_bounds: None,
            focus: None,
            rename: None,
            column_selected: None,
            refresh: true,
            status: "就绪".into(),
            notice_after_refresh: None,
            paste_shortcut_active: false,
            tcp: true,
            udp: false,
            updated: None,
        }
    }
}
pub(crate) struct FileExport {
    pub path: String,
    pub cancel: tomato_services::files::images::CancellationToken,
    pub asset: Option<std::sync::Arc<tomato_services::files::export::Asset>>,
    pub gesture: bool,
    pub request: tomato_services::files::export::Request,
    pub native: Option<tomato_services::files::export::Drag>,
}
impl Drop for FileExport {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
pub struct Editor {
    pub path: String,
    pub text: String,
    pub saved: String,
    pub pending: bool,
}
struct Pending {
    section: Section,
    mutation: bool,
    save: Option<(String, String)>,
    copying: bool,
    uploading: bool,
    image: Option<tomato_services::files::images::Request>,
    export: Option<tomato_services::files::export::Request>,
    preview_action: Option<(String, images::Item, images::PreviewAction)>,
    renamed: Option<String>,
    change: Option<FileChange>,
}
pub enum FileChange {
    Created(String),
    Renamed { from: String, to: String },
}
pub struct Workspace {
    pub storage: storage::State,
    pub external_file_toolbar: bool,
    pub document_active: Option<String>,
    pub file_changes: VecDeque<FileChange>,
    latest_listing: Option<u64>,
    pub upload: Option<Box<upload::Upload>>,
    pub upload_report: Option<std::sync::Arc<tomato_services::files::object_copy::Snapshot>>,
    pub previews: Vec<images::Preview>,
    pub object_copy: Option<tomato_services::files::object_copy::Owner>,
    pub copy_window_visible: bool,
    pub copy_window_positioned: bool,
    pub copy_inner_size: Option<egui::Vec2>,
    pub copy_errors: Option<std::sync::Arc<tomato_services::files::object_copy::Snapshot>>,
    pub file_view: String,
    pub file_status: Status,
    pub reconnect_requested: bool,
    pub follow_terminal: bool,
    pub inspector_id: Option<String>,
    pub inspector: Option<inspect::Inspector>,
    pub inspector_notice: Option<(String, String)>,
    pub inspector_notice_focus: bool,
    pub standalone: Option<Section>,
    pub visible: BTreeSet<Section>,
    pub panes: BTreeMap<Section, Pane>,
    pub ratio: f32,
    pub monitor_height: f32,
    pub selected_tool: Option<Section>,
    pub editor: Option<Editor>,
    pub output: Option<String>,
    pub dialog: Option<Dialog>,
    pub choosing_transfer: bool,
    pub history: VecDeque<Sample>,
    pub counters: Option<Counters>,
    pub metrics: [f32; 6],
    last_monitor: Instant,
    pending: BTreeMap<u64, Pending>,
    queued: VecDeque<(Section, Operation)>,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            storage: Default::default(),
            file_changes: VecDeque::new(),
            external_file_toolbar: false,
            document_active: None,
            latest_listing: None,
            upload: None,
            upload_report: None,
            object_copy: None,
            previews: Vec::new(),
            copy_window_visible: false,
            copy_window_positioned: false,
            copy_inner_size: None,
            copy_errors: None,
            file_view: "LIST".into(),
            file_status: Status::Connecting,
            reconnect_requested: false,
            follow_terminal: true,
            inspector_id: None,
            inspector: None,
            inspector_notice: None,
            inspector_notice_focus: true,
            standalone: None,
            visible: BTreeSet::new(),
            panes: BTreeMap::new(),
            ratio: 0.7,
            monitor_height: 400.0,
            selected_tool: None,
            editor: None,
            output: None,
            dialog: None,
            choosing_transfer: false,
            history: VecDeque::new(),
            counters: None,
            metrics: [0.; 6],
            last_monitor: Instant::now() - Duration::from_secs(2),
            pending: BTreeMap::new(),
            queued: VecDeque::new(),
        }
    }
}
pub struct Dialog {
    pub focus: bool,
    pub title: String,
    pub labels: Vec<String>,
    pub fields: Vec<String>,
    pub kind: Form,
    pub confirm: Option<Operation>,
}
pub enum Form {
    Notice,
    Mkdir,
    Create,
    InitializePv,
    VolumeGroup,
    ExtendLv,
    Mount,
    Confirm,
}
impl Workspace {
    pub fn reconnected(&mut self) {
        self.reset_monitor();
        self.document_active = None;
        self.pending.clear();
        self.file_changes.clear();
        self.latest_listing = None;
        self.queued.clear();
        self.choosing_transfer = false;
        for pane in self.panes.values_mut() {
            pane.refresh = true;
            pane.status = "正在重连…".into();
        }
        if let Some(id) = self.inspector_id.clone() {
            self.panes.entry(Section::Docker).or_default().refresh = false;
            self.enqueue(
                Section::Docker,
                Operation::Manage(
                    tomato_services::terminal::ssh::tools::ManagementAction::DockerInspect(id),
                ),
            );
        }
        if let Some(e) = &mut self.editor {
            e.pending = false;
        }
    }
    pub fn open(&mut self, section: Section) {
        let was_empty = self.visible.is_empty();
        self.visible.insert(section);
        self.panes.entry(section).or_default();
        self.selected_tool = Some(section);
        if was_empty {
            self.ratio = 0.6;
        }
        if section == Section::Files {
            self.ratio = 0.7;
        }
    }
    pub fn toggle(&mut self, section: Section) {
        if !self.visible.remove(&section) {
            self.open(section);
        }
    }
    pub fn enqueue(&mut self, section: Section, operation: Operation) {
        let bytes = self
            .queued
            .iter()
            .fold(operation.queued_bytes(), |total, (_, op)| {
                total.saturating_add(op.queued_bytes())
            });
        if self.queued.len() >= 64 || bytes > 16 * 1024 * 1024 {
            if let Operation::WriteText { path, .. } = &operation
                && let Some(editor) = &mut self.editor
                && editor.path == *path
            {
                editor.pending = false;
            }
            self.panes.entry(section).or_default().status =
                "操作队列已满，请等待当前操作完成".into();
            return;
        }
        if section == Section::Files && matches!(&operation, Operation::List(_)) {
            self.panes.entry(section).or_default().thumbnails.suspend();
        }
        self.queued.push_back((section, operation));
    }
    pub fn poll(&mut self, session: &mut Terminal, ctx: &egui::Context, active: bool) {
        if (!active || !self.visible.contains(&Section::Files))
            && let Some(pane) = self.panes.get_mut(&Section::Files)
            && let Some(operation) = pane.finish_file_rename(ctx, true)
        {
            self.enqueue(Section::Files, operation);
        }
        self.queue_images();
        self.file_status = session.status();
        while let Some(reply) = session.try_tool_reply() {
            if reply.id == 0 {
                let pane = self.panes.entry(Section::Files).or_default();
                pane.status = match reply.result {
                    Ok(Output::Done(text)) => {
                        pane.refresh = true;
                        text
                    }
                    Err(error) => error,
                    _ => "文件编辑监控已更新".into(),
                };
                continue;
            }
            let Some(Pending {
                section,
                mutation,
                save,
                copying,
                uploading,
                image,
                export,
                preview_action,
                renamed,
                change,
            }) = self.pending.remove(&reply.id)
            else {
                continue;
            };
            if let Some(request) = image {
                self.image_reply(request, reply.result, ctx);
                continue;
            }
            if let Some(action) = preview_action {
                self.preview_action_reply(action, reply.result);
                continue;
            }
            if matches!(&reply.result, Ok(Output::Directory { .. }))
                && self.latest_listing != Some(reply.id)
            {
                continue;
            }
            if reply.result.is_ok()
                && let Some(change) = change
            {
                if self.file_changes.len() == 32 {
                    self.file_changes.pop_front();
                }
                self.file_changes.push_back(change);
            }
            let pane = self.panes.entry(section).or_default();
            // A late actor cancellation must not hide the native drag error or
            // change the status of a newer drag of the same remote path.
            if export.is_some_and(|request| {
                pane.export
                    .as_ref()
                    .is_none_or(|active| !active.request.same_job(&request))
            }) {
                continue;
            }
            if reply.result.is_ok()
                && let Some(from) = renamed
                && (pane.path.trim_end_matches('/') == from.trim_end_matches('/')
                    || pane
                        .path
                        .starts_with(&format!("{}/", from.trim_end_matches('/'))))
            {
                pane.path = parent(&from);
                if let Some(index) = pane.columns.iter().position(|(path, _)| path == &pane.path) {
                    pane.columns.truncate(index);
                } else {
                    pane.columns.clear();
                }
                pane.column_selected = None;
            }
            match reply.result {
                Ok(Output::ExportReady(path, asset)) => {
                    if let Some(export) = &mut pane.export
                        && export.path == path
                    {
                        export.asset = Some(asset);
                        pane.status = "已准备系统文件拖放".into();
                    }
                }
                Ok(Output::Uploaded {
                    directory: _,
                    report,
                }) => {
                    if ctx
                        .data(|data| {
                            data.get_temp::<bool>(egui::Id::new("file-drop-preview-installed"))
                        })
                        .unwrap_or(false)
                    {
                        ctx.data_mut(|data| {
                            data.insert_temp(
                                egui::Id::new("file-drop-final-report"),
                                report.clone(),
                            )
                        });
                    }
                    self.upload = None;
                    let notice = upload::notice(&report);
                    pane.status = notice.clone();
                    if report.succeeded + report.failed > 0 {
                        pane.notice_after_refresh = Some((pane.path.clone(), notice));
                        pane.refresh = true;
                    }
                    if report.failed > 0
                        || report.phase == tomato_services::files::object_copy::Phase::Failed
                    {
                        self.upload_report = Some(report);
                    }
                }
                Ok(Output::ObjectsStaged(notice)) => pane.status = notice,
                Ok(Output::Image { .. }) => {}
                Ok(Output::ObjectsCopied(report)) => {
                    use tomato_services::files::object_copy::Phase;
                    self.object_copy = None;
                    self.copy_window_visible = false;
                    let notice = if report.phase == Phase::Cancelled {
                        format!(
                            "复制已取消: 成功 {} 个, 失败 {} 个",
                            report.succeeded, report.failed
                        )
                    } else if report.total == 0 {
                        "没有文件可复制".into()
                    } else if report.failed == 0 {
                        format!("粘贴完成: 成功 {} 个", report.succeeded)
                    } else {
                        format!(
                            "粘贴结束: 成功 {} 个, 失败 {} 个",
                            report.succeeded, report.failed
                        )
                    };
                    pane.status = notice.clone();
                    if report.succeeded + report.failed > 0 {
                        pane.notice_after_refresh = Some((pane.path.clone(), notice));
                        pane.refresh = true;
                    }
                    if report.failed > 0 {
                        self.copy_errors = Some(report);
                    }
                }
                Ok(Output::ClipboardFiles(files)) => {
                    if copying {
                        self.object_copy = None;
                        self.copy_window_visible = false;
                    }
                    let mut operations = Vec::new();
                    for local in files {
                        if let Some(name) = local
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(str::to_string)
                        {
                            operations.push(Operation::Upload {
                                local,
                                remote: format!("{}/{name}", pane.path.trim_end_matches('/')),
                            });
                        }
                    }
                    if operations.is_empty() {
                        pane.status = "剪贴板中没有文件".into();
                    } else {
                        self.dialog = Some(Dialog {
                            focus: true,
                            title: "上传剪贴板文件（含同名覆盖）".into(),
                            labels: Vec::new(),
                            fields: Vec::new(),
                            kind: Form::Confirm,
                            confirm: Some(Operation::Batch(operations)),
                        });
                    }
                }
                Ok(Output::EditCached { remote, .. }) => {
                    pane.status = format!("已打开 {remote}，保存后自动上传");
                }
                Ok(Output::Inspection { id, value }) => {
                    self.inspector = Some(inspect::Inspector::new(id, value));
                    pane.status = "已刷新".into();
                }
                Err(error) => {
                    if section == Section::Storage {
                        self.storage.fail(error.clone());
                    }
                    if self.inspector_id.is_some() {
                        self.inspector_notice = Some(("操作失败".into(), error.clone()));
                        self.inspector_notice_focus = true;
                    }
                    if uploading && let Some(upload) = self.upload.take() {
                        upload.owner.0.fail(&error);
                        self.upload_report = Some(upload.owner.0.snapshot());
                    }
                    if copying {
                        self.copy_errors =
                            self.object_copy.as_ref().map(|owner| owner.0.snapshot());
                        self.object_copy = None;
                        self.copy_window_visible = false;
                    }
                    pane.notice_after_refresh = None;
                    pane.status = error;
                    if let Some((path, _)) = &save
                        && let Some(editor) = &mut self.editor
                        && editor.path == *path
                    {
                        editor.pending = false;
                    }
                }
                Ok(Output::Directory { path, entries }) => {
                    pane.finish_file_rename(ctx, false);
                    pane.thumbnails.clear();
                    if path != pane.path {
                        if let Some(index) = pane.columns.iter().position(|(p, _)| p == &path) {
                            pane.columns.truncate(index);
                        } else if let Some(index) =
                            pane.columns.iter().position(|(p, _)| p == &parent(&path))
                        {
                            pane.columns.truncate(index + 1);
                        } else if path.starts_with(&format!("{}/", pane.path.trim_end_matches('/')))
                            && pane.entries.len() <= 2000
                        {
                            pane.columns.push((pane.path.clone(), pane.entries.clone()));
                        } else {
                            pane.columns.clear();
                        }
                    }
                    while pane.columns.iter().map(|(_, e)| e.len()).sum::<usize>() > 5000
                        || pane.columns.len() > 8
                    {
                        pane.columns.remove(0);
                    }
                    pane.path = path;
                    pane.entries = entries;
                    pane.file_cursor = None;
                    if let Some(export) = pane.export.take() {
                        export.cancel.cancel();
                    }
                    if pane
                        .column_selected
                        .as_ref()
                        .is_some_and(|selected| pane.file_entry(selected).is_none())
                    {
                        pane.column_selected = None;
                    }
                    pane.selected.clear();
                    pane.anchor = None;
                    pane.apply_file_sort();
                    pane.status = pane
                        .notice_after_refresh
                        .take()
                        .filter(|(expected, _)| expected == &pane.path)
                        .map(|(_, notice)| notice)
                        .unwrap_or_else(|| format!("共 {} 项", pane.entries.len()));
                }
                Ok(Output::Table(mut table)) => {
                    pane.refresh = false;
                    pane.updated = Some(Instant::now());
                    pane.status = format!("共 {} 项", table.rows.len());
                    pane.selected.clear();
                    if section == Section::Storage {
                        self.storage.replace(&table.detail);
                        table.detail = String::new();
                        pane.status = format!(
                            "已读取 {} 个块设备；写操作要求远程 sudo -n 权限",
                            table.rows.len()
                        );
                        for row in &mut table.rows {
                            if let Some(value) = row.get_mut(2) {
                                *value = storage::size(value);
                            }
                        }
                        self.storage.sort_rows(&table.rows);
                    }
                    if section == Section::Monitor {
                        self.update_monitor(&table.detail);
                    }
                    self.panes.entry(section).or_default().table = Some(table);
                }
                Ok(Output::Text { path, text }) => {
                    self.editor = Some(Editor {
                        path,
                        text: text.clone(),
                        saved: text,
                        pending: false,
                    })
                }
                Ok(Output::Done(text)) => {
                    pane.status = if text.trim().is_empty() {
                        "操作完成".into()
                    } else {
                        text.clone()
                    };
                    if mutation {
                        if let Some(id) = &self.inspector_id {
                            pane.refresh = false;
                            self.queued.push_back((section,Operation::Manage(tomato_services::terminal::ssh::tools::ManagementAction::DockerInspect(id.clone()))));
                        } else {
                            pane.refresh = true;
                        }
                    } else {
                        self.output = Some(text);
                    }
                    if let Some((path, text)) = save
                        && let Some(editor) = &mut self.editor
                        && editor.path == path
                    {
                        editor.saved = text;
                        editor.pending = false;
                    }
                }
            }
        }
        if session.status() != Status::Ready {
            return;
        }
        if active && self.visible.contains(&Section::Monitor) {
            ctx.request_repaint_after(Duration::from_secs(1));
            if self.last_monitor.elapsed() >= Duration::from_secs(1) {
                self.panes.entry(Section::Monitor).or_default().refresh = true;
                self.last_monitor = Instant::now();
            }
        }
        let sections: Vec<_> = self
            .panes
            .keys()
            .copied()
            .filter(|s| *s != Section::Monitor || self.visible.contains(s))
            .collect();
        for section in &sections {
            let pane = self.panes.entry(*section).or_default();
            if pane.refresh
                && !self.pending.values().any(|p| p.section == *section)
                && !self.queued.iter().any(|(s, _)| s == section)
            {
                pane.refresh = false;
                if *section == Section::Files {
                    pane.thumbnails.suspend();
                }
                self.queued.push_back((
                    *section,
                    if *section == Section::Files {
                        Operation::List(pane.path.clone())
                    } else {
                        Operation::Query(*section)
                    },
                ));
            }
        }
        self.poll_image_actions(session);
        while self.pending.len() < 4 {
            let Some((section, operation)) = self.queued.pop_front() else {
                break;
            };
            let mutation=!matches!(operation,Operation::ExportFile(_)|Operation::Query(_)|Operation::List(_)|Operation::ReadText(_)|Operation::ReadImage(_)|Operation::Manage(tomato_services::terminal::ssh::tools::ManagementAction::DockerLogs(_)|tomato_services::terminal::ssh::tools::ManagementAction::DockerInspect(_)));
            let image = if let Operation::ReadImage(request) = &operation {
                Some(request.clone())
            } else {
                None
            };
            let save = if let Operation::WriteText { path, text } = &operation {
                Some((path.clone(), text.clone()))
            } else {
                None
            };
            let copying = matches!(operation, Operation::PasteObjects { .. });
            let uploading = matches!(operation, Operation::UploadFiles { .. });
            let renamed = if let Operation::Rename { from, .. } = &operation {
                Some(from.clone())
            } else {
                None
            };
            let listing = matches!(operation, Operation::List(_));
            let change = match &operation {
                Operation::Create(path) => Some(FileChange::Created(path.clone())),
                Operation::Rename { from, to } => Some(FileChange::Renamed {
                    from: from.clone(),
                    to: to.clone(),
                }),
                _ => None,
            };
            let background_image = image.is_some();
            let export = if let Operation::ExportFile(request) = &operation {
                Some(request.clone())
            } else {
                None
            };
            match session.tool_request(operation) {
                Ok(id) => {
                    if listing {
                        self.latest_listing = Some(id);
                    }
                    self.pending.insert(
                        id,
                        Pending {
                            section,
                            mutation,
                            save,
                            copying,
                            uploading,
                            image,
                            export,
                            preview_action: None,
                            renamed,
                            change,
                        },
                    );
                    if !background_image {
                        self.panes.entry(section).or_default().status = "正在执行…".into();
                    }
                }
                Err(error) => {
                    if section == Section::Storage {
                        self.storage.fail(error.to_string());
                    }
                    if let Some(request) = export {
                        request.cancel.cancel();
                        let pane = self.panes.entry(section).or_default();
                        if pane
                            .export
                            .as_ref()
                            .is_some_and(|active| active.request.same_job(&request))
                        {
                            pane.export = None;
                        }
                    }
                    if uploading && let Some(upload) = self.upload.take() {
                        upload.owner.0.fail(&error.to_string());
                        self.upload_report = Some(upload.owner.0.snapshot());
                    }
                    if let Some(request) = image {
                        self.image_reply(request, Err(error.to_string()), ctx);
                        continue;
                    }
                    if copying {
                        self.object_copy = None;
                        self.copy_window_visible = false;
                    }
                    if save.is_some()
                        && let Some(editor) = &mut self.editor
                    {
                        editor.pending = false;
                    }
                    self.panes.entry(section).or_default().status = error.to_string();
                }
            }
        }
    }
    pub fn busy(&self, section: Section) -> bool {
        self.pending.values().any(|p| p.section == section)
            || self.queued.iter().any(|(s, _)| *s == section)
    }
}

#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_toggles_preserve_divider_until_files_are_opened() {
        let mut workspace = Workspace::default();
        workspace.open(Section::Docker);
        assert_eq!(workspace.ratio, 0.6);
        workspace.ratio = 0.53;
        workspace.open(Section::Monitor);
        assert_eq!(workspace.ratio, 0.53);
        workspace.open(Section::Files);
        assert_eq!(workspace.ratio, 0.7);
        workspace.open(Section::Ports);
        assert_eq!(workspace.ratio, 0.7);
        for section in [
            Section::Docker,
            Section::Monitor,
            Section::Files,
            Section::Ports,
        ] {
            workspace.toggle(section);
        }
        workspace.open(Section::Monitor);
        assert_eq!(workspace.ratio, 0.6);
    }
}
