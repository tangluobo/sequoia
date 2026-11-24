//! Opt-in loopback framebuffer fixture; never uses the system clipboard.
use super::*;
use std::sync::{Arc, Mutex};
use tomato_services::files::{
    clipboard::{Content, Repository},
    object_copy::{self, Selection},
    session::FileSession,
};
struct Clipboard(Mutex<Selection>);
impl Repository for Clipboard {
    fn read(&self) -> anyhow::Result<Content> {
        Ok(Content::Objects(
            self.0
                .lock()
                .map_err(|_| anyhow::anyhow!("fixture clipboard lock"))?
                .clone(),
        ))
    }
    fn write_objects(&self, selection: &Selection) -> anyhow::Result<()> {
        *self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("fixture clipboard lock"))? = selection.clone();
        Ok(())
    }
}
fn endpoint(connection: &Connection) -> bool {
    connection
        .text("endpoint")
        .strip_prefix("http://127.0.0.1:")
        .and_then(|port| port.parse::<u16>().ok())
        .is_some_and(|port| port != 0)
}
impl TomatoApp {
    pub(super) fn preview_copy(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("copy fixture requires isolated profile"))?;
        anyhow::ensure!(
            profile
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("tomato-copy-fixture-")),
            "copy fixture profile name mismatch"
        );
        let text = std::env::var("TOMATO_UI_COPY_SELECTION")?;
        let selection = Selection::parse(&text)?;
        let source = self
            .store
            .connections
            .iter()
            .find(|c| c.id == selection.connection_id && c.name == "对象复制验收-源")
            .ok_or_else(|| anyhow::anyhow!("copy fixture source missing"))?;
        let target = self
            .store
            .connections
            .iter()
            .find(|c| c.name == "对象复制验收-目标")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("copy fixture target missing"))?;
        anyhow::ensure!(
            source.kind() == "S3"
                && target.kind() == "ALIYUN_OSS"
                && endpoint(source)
                && endpoint(&target),
            "copy fixture must use native loopback S3/OSS"
        );
        anyhow::ensure!(
            selection.bucket == "tomato-fixture-source"
                && target.text("bucket") == "tomato-fixture-target"
                && selection.items.len() == 1
                && selection.items[0].key == "large.bin"
                && !selection.items[0].directory,
            "copy fixture object mismatch"
        );
        let context = object_copy::Context {
            clipboard: Arc::new(Clipboard(Mutex::new(selection))),
            connections: Arc::new(object_copy::SavedConnections),
        };
        let session = tomato_services::terminal::Terminal::from_file_session(
            FileSession::start_with_context(
                self.runtime
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("fixture runtime unavailable"))?
                    .handle(),
                target.clone(),
                context,
            ),
        );
        let mut workspace = crate::tabs::ssh::Workspace::standalone_files("LIST", "/");
        workspace.paste_objects(ctx, "/".into());
        let tab = Tab {
            id: Connection::new("TOOL").id,
            source_id: target.id.clone(),
            title: target.name.clone(),
            panel: Panel::Terminal {
                connection: target,
                session,
                parser: Box::new(vt100::Parser::new(1, 1, 0)),
                view: crate::tabs::terminal::TerminalViewState {
                    ssh: Some(Box::new(workspace)),
                    ..Default::default()
                },
            },
            busy: false,
            error: None,
        };
        self.active = Some(tab.id.clone());
        self.tabs.push(tab);
        self.section = 0;
        Ok(())
    }
}
