//! Isolated native-window upload fixture. Synthetic input stays in this egui context.
use super::*;
use egui::{Context, Event, Id, RawInput};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
const INSTALLED: &str = "file-drop-preview-installed";
const READY: &str = "file-drop-preview-ready";

impl TomatoApp {
    pub(super) fn preview_file_drop(&mut self, ctx: &Context) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("上传验收必须使用独立配置"))?;
        anyhow::ensure!(
            profile.file_name().is_some_and(|name| name
                .to_string_lossy()
                .starts_with("tomato-file-drop-fixture-"))
                && self.store.connections.len() == 1,
            "上传验收需要独立单连接配置"
        );
        let connection = self.store.connections[0].clone();
        anyhow::ensure!(connection.name == "文件拖放验收", "上传验收连接名称不符");
        let network = tomato_services::files::object_copy::is_object(&connection)
            && connection
                .text("endpoint")
                .strip_prefix("http://127.0.0.1:")
                .and_then(|port| port.parse::<u16>().ok())
                .is_some_and(|port| port > 0);
        let local = connection.kind() == "LOCAL_DIRECTORY"
            && connection.text("directoryType") != "S3"
            && PathBuf::from(connection.text("localDirectoryPath"))
                .canonicalize()?
                .starts_with(profile.canonicalize()?);
        anyhow::ensure!(
            (network || local) && !connection.flag("useSshTunnel"),
            "上传验收只能访问自己的本地目录或回环服务"
        );
        let case = std::env::var("TOMATO_FILE_DROP_CASE").unwrap_or_else(|_| "complete".into());
        if case == "manual" {
            let label = profile
                .parent()
                .and_then(|p| p.file_name())
                .unwrap_or_default()
                .to_string_lossy();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                "Tomato 文件拖出验收 · {label}"
            )));
        }
        anyhow::ensure!(
            matches!(case.as_str(), "complete" | "progress" | "cancel" | "manual"),
            "上传验收模式不符"
        );
        let source = profile.join("source");
        anyhow::ensure!(source.is_dir(), "缺少隔离上传源目录");
        let paths = if matches!(case.as_str(), "progress" | "cancel") {
            vec![source.join("large.bin")]
        } else {
            vec![
                source.join("投递目录"),
                source.join("中文.txt"),
                source.join("zero.txt"),
            ]
        };
        let reads = Arc::new(AtomicUsize::new(0));
        ctx.data_mut(|data| {
            data.insert_temp(Id::new(INSTALLED), true);
            data.insert_temp(Id::new("file-drop-ui-reads"), reads.clone());
        });
        if case != "manual" {
            ctx.add_plugin(Input {
                paths,
                reads,
                frame: 0,
                cancel_step: 0,
                case: case.clone(),
            });
        }
        if local {
            // A local connection opens its tree; exercise the existing file-tab factory.
            let tab = tabs::create_tab(
                connection,
                self.runtime.as_ref().map(|runtime| runtime.handle()),
            )?;
            if let Panel::Terminal { session, .. } = &tab.panel {
                let repaint = ctx.clone();
                session.on_output(move || repaint.request_repaint());
            }
            self.active = Some(tab.id.clone());
            self.tabs.push(tab);
        } else {
            self.open_connection(connection);
        }
        self.section = 0;
        self.quit_approved = true;
        let tab = self
            .tabs
            .iter_mut()
            .find(|tab| Some(&tab.id) == self.active.as_ref())
            .ok_or_else(|| anyhow::anyhow!("上传验收标签不存在"))?;
        let Panel::Terminal { view, .. } = &mut tab.panel else {
            anyhow::bail!("上传验收不是文件标签")
        };
        let workspace = view
            .ssh
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("上传验收文件视图不存在"))?;
        let mode = std::env::var("TOMATO_FILE_DROP_VIEW").unwrap_or_else(|_| "LIST".into());
        anyhow::ensure!(
            matches!(mode.as_str(), "LIST" | "ICON" | "COLUMN"),
            "上传验收视图不符"
        );
        workspace.file_view = mode;
        workspace
            .panes
            .entry(tomato_services::terminal::ssh::tools::Section::Files)
            .or_default()
            .path = std::env::var("TOMATO_FILE_DROP_TARGET")?;
        Ok(())
    }
    pub(super) fn file_drop_preview(&mut self, ctx: &Context) {
        if !ctx
            .data(|data| data.get_temp::<bool>(Id::new(INSTALLED)))
            .unwrap_or(false)
        {
            return;
        }
        let profile = PathBuf::from(std::env::var_os("TOMATO_DATA_DIR").unwrap());
        if std::env::var("TOMATO_FILE_DROP_CASE").as_deref() == Ok("manual") {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if profile.join("stop").is_file() {
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        }
        let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| Some(&tab.id) == self.active.as_ref())
        else {
            return;
        };
        let Panel::Terminal { view, .. } = &tab.panel else {
            return;
        };
        let Some(workspace) = view.ssh.as_ref() else {
            return;
        };
        let section = tomato_services::terminal::ssh::tools::Section::Files;
        let Some(pane) = workspace.panes.get(&section) else {
            return;
        };
        let available = workspace.file_status == tomato_services::terminal::Status::Ready
            && !workspace.busy(section);
        if available {
            ctx.data_mut(|data| data.insert_temp(Id::new(READY), true));
        }
        let report = ctx.data(|data| {
            data.get_temp::<Arc<tomato_services::files::object_copy::Snapshot>>(Id::new(
                "file-drop-final-report",
            ))
        });
        let active = workspace
            .upload
            .as_ref()
            .map(|upload| upload.owner.0.snapshot());
        let started = active.is_some()
            || report.is_some()
            || ctx
                .data(|data| data.get_temp::<bool>(Id::new("file-drop-started")))
                .unwrap_or(false);
        if started {
            ctx.data_mut(|data| data.insert_temp(Id::new("file-drop-started"), true));
        }
        let state = report.as_ref().or(active.as_ref());
        let phase = state.map(|report| format!("{:?}", report.phase));
        let case = std::env::var("TOMATO_FILE_DROP_CASE").unwrap_or_else(|_| "complete".into());
        let progress = case == "progress"
            && active.as_ref().is_some_and(|report| {
                report.phase == tomato_services::files::object_copy::Phase::Copying
                    && report.transferred > 0
            });
        let (events, pointer) = ctx.input(|i| {
            let events:Vec<_> = i.events.iter().filter_map(|event| match event {
                Event::PointerButton{pos,button,pressed,..} => Some(serde_json::json!({"kind":"button","x":pos.x,"y":pos.y,"button":format!("{button:?}"),"pressed":pressed,"time":i.time})),
                Event::PointerGone => Some(serde_json::json!({"kind":"gone","time":i.time})),
                Event::WindowFocused(focused) => Some(serde_json::json!({"kind":"focus","focused":focused,"time":i.time})),
                _ => None,
            }).collect();
            (events, serde_json::json!({"focused":i.focused,"primaryDown":i.pointer.primary_down(),"position":i.pointer.interact_pos().map(|p|[p.x,p.y]),"time":i.time}))
        });
        if !events.is_empty() {
            ctx.data_mut(|data| {
                let recorded = data.get_temp_mut_or_default::<Vec<serde_json::Value>>(Id::new(
                    "file-drop-pointer-events",
                ));
                recorded.extend(events);
                if recorded.len() > 128 {
                    recorded.drain(..recorded.len() - 128);
                }
            });
        }
        let proof = serde_json::json!({
            "ready":progress||(started&&report.is_some()&&available),"started":started,"case":case,"view":workspace.file_view,"path":pane.path,
            "listingReady":available&&!pane.refresh,"pointer":pointer,"pointerEvents":ctx.data(|data|data.get_temp::<Vec<serde_json::Value>>(Id::new("file-drop-pointer-events"))),
            "exportPrepared":pane.export.is_some(),"exportDispatched":pane.export.as_ref().is_some_and(|e|e.request.started()),
            "nativeState":ctx.data(|data|data.get_temp::<String>(Id::new("file-export-native-state"))),
            "phase":phase,"total":state.map(|state|state.total),"succeeded":state.map(|state|state.succeeded),"failed":state.map(|state|state.failed),"transferred":state.map(|state|state.transferred),"totalBytes":state.and_then(|state|state.total_bytes),"completedBytes":state.map(|state|state.completed_bytes),
            "status":pane.status,"entries":pane.entries.iter().map(|entry|&entry.name).collect::<Vec<_>>(),
            "exportEvents":ctx.data(|data|data.get_temp::<Vec<serde_json::Value>>(Id::new("file-export-events"))),
            "uploading":active.is_some(),"pending":workspace.busy(section),"error":tab.error,
            "syntheticInputUsed":case!="manual","savedHostsContacted":false,"systemClipboardWritten":false,
            "uiFileReads":ctx.data(|data|data.get_temp::<Arc<AtomicUsize>>(Id::new("file-drop-ui-reads"))).map(|reads|reads.load(Ordering::Relaxed)),
            "cancelButtonUsed":ctx.data(|data|data.get_temp::<bool>(Id::new("file-drop-cancel-clicked"))).unwrap_or(false)
        });
        if let Some(path) = std::env::var_os("TOMATO_FILE_DROP_PROOF").map(PathBuf::from) {
            let pending = path.with_extension("pending");
            if std::fs::write(&pending, proof.to_string()).is_ok() {
                let _ = std::fs::rename(pending, path);
            }
        }
    }
}
#[derive(Debug)]
struct File {
    path: PathBuf,
    reads: Arc<AtomicUsize>,
}
impl egui::DroppedFile for File {
    fn path(&self) -> &std::path::Path {
        &self.path
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        Err("验收禁止 UI 线程读取上传内容".into())
    }
}
struct Input {
    paths: Vec<PathBuf>,
    reads: Arc<AtomicUsize>,
    frame: usize,
    cancel_step: usize,
    case: String,
}
impl egui::plugin::Plugin for Input {
    fn debug_name(&self) -> &'static str {
        "isolated file drop"
    }
    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        input
            .events
            .retain(|event| matches!(event, Event::Screenshot { .. }));
        input.focused = true;
        if input.viewport_id == egui::ViewportId::ROOT {
            if !ctx
                .data(|data| data.get_temp::<bool>(Id::new(READY)))
                .unwrap_or(false)
            {
                return;
            }
            self.frame += 1;
            if self.frame == 4
                && let Some(bounds) =
                    ctx.data(|data| data.get_temp::<egui::Rect>(Id::new("file-drop-view-bounds")))
            {
                let point = bounds.right_bottom() - egui::vec2(35., 35.);
                input
                    .events
                    .extend([Event::PointerMoved(point), Event::PointerGone]);
                input.dropped_files.extend(self.paths.iter().map(|path| {
                    Arc::new(File {
                        path: path.clone(),
                        reads: self.reads.clone(),
                    }) as egui::DroppedFileHandle
                }));
            }
        } else if self.case == "cancel"
            && self.cancel_step < 2
            && std::env::var_os("TOMATO_DATA_DIR")
                .is_some_and(|path| PathBuf::from(path).join("cancel-ready").is_file())
            && ctx
                .data(|data| data.get_temp::<bool>(Id::new("file-drop-transfer-started")))
                .unwrap_or(false)
            && ctx.data(|data| {
                data.get_temp::<egui::ViewportId>(Id::new("file-drop-progress-viewport"))
            }) == Some(input.viewport_id)
            && let Some(point) =
                ctx.data(|data| data.get_temp::<egui::Pos2>(Id::new("file-drop-cancel-point")))
        {
            input.events.extend([
                Event::PointerMoved(point),
                Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: self.cancel_step == 0,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            self.cancel_step += 1;
            if self.cancel_step == 2 {
                ctx.data_mut(|data| data.insert_temp(Id::new("file-drop-cancel-clicked"), true));
            }
        }
        ctx.request_repaint_after_for(std::time::Duration::from_millis(16), input.viewport_id);
    }
}
