//! Drive the normal toolbar only inside an owned, empty capture profile.
use super::*;

fn key() -> egui::Id {
    egui::Id::new("owned-markdown-print-destination")
}
#[derive(Clone)]
struct Fixture {
    output: Option<PathBuf>,
    browser: Option<(String, &'static str)>,
}
const READY: &str = "owned-markdown-print-ready";

impl TomatoApp {
    pub(in crate::app) fn preview_markdown_print(&mut self) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("打印验收需要隔离配置目录"))?;
        anyhow::ensure!(
            profile.file_name().is_some_and(|name| name
                .to_string_lossy()
                .starts_with("tomato-markdown-print-fixture-")),
            "打印验收只能使用专有配置"
        );
        let profile = profile.canonicalize()?;
        anyhow::ensure!(
            self.repository
                .location()
                .and_then(|path| path.parent())
                .and_then(|path| path.canonicalize().ok())
                .as_ref()
                == Some(&profile),
            "打印验收配置必须在隔离目录内"
        );
        let mode = std::env::var("TOMATO_MARKDOWN_PRINT_TEST").unwrap_or_default();
        let output = (mode != "cancel-dialog").then(|| profile.join("printed.pdf"));
        let browser = if matches!(mode.as_str(), "sftp-fixture" | "s3-fixture") {
            let (connection, document) = remote(&self.store.connections, &mode)?;
            self.open_connection(connection.clone());
            let owner = self
                .active
                .clone()
                .ok_or_else(|| anyhow::anyhow!("未打开验收连接"))?;
            Some((owner, document))
        } else {
            anyhow::ensure!(self.store.connections.is_empty(), "本地打印验收需要空配置");
            let input = profile.join("input.md").canonicalize()?;
            anyhow::ensure!(input.starts_with(&profile), "打印验收文件必须在隔离目录内");
            self.open_editor(input);
            None
        };
        self.context
            .data_mut(|data| data.insert_temp(key(), Fixture { output, browser }));
        self.context.add_plugin(ClickPdf::default());
        Ok(())
    }
}

fn remote<'a>(
    connections: &'a [Connection],
    mode: &str,
) -> anyhow::Result<(&'a Connection, &'static str)> {
    let [connection] = connections else {
        anyhow::bail!("远端打印验收需要单个隔离连接");
    };
    anyhow::ensure!(
        !connection.flag("useSshTunnel") && connection.text("password") == "fixture-only",
        "远端打印验收需要专用连接"
    );
    if mode == "sftp-fixture" {
        anyhow::ensure!(
            connection.kind() == "SFTP"
                && connection.text("host") == "127.0.0.1"
                && connection.text("username") == "fixture"
                && u16::try_from(connection.number("port")).is_ok_and(|port| port != 0),
            "SFTP 打印验收只允许专用回环服务"
        );
        return Ok((connection, "/notes/README.md"));
    }
    anyhow::ensure!(
        connection.kind() == "S3"
            && connection.text("username") == "fixture-key"
            && connection.text("bucket").is_empty()
            && connection.flag("pathStyleAccess")
            && connection
                .text("endpoint")
                .strip_prefix("http://127.0.0.1:")
                .and_then(|port| port.parse::<u16>().ok())
                .is_some_and(|port| port != 0),
        "S3 打印验收只允许专用回环服务"
    );
    Ok((connection, "/fixture/notes/README.md"))
}

pub(super) fn poll(app: &mut TomatoApp, ctx: &egui::Context) {
    use tomato_services::terminal::Status;
    let Some(mut fixture) = ctx.data(|data| data.get_temp::<Fixture>(key())) else {
        return;
    };
    let Some((owner, path)) = fixture.browser.as_ref() else {
        return;
    };
    let status = app
        .tabs
        .iter_mut()
        .find(|tab| &tab.id == owner)
        .and_then(|tab| match &mut tab.panel {
            Panel::Terminal { session, .. } => Some(session.status()),
            _ => None,
        });
    match status {
        Some(Status::Ready) => app.open_file_document(owner, (*path).into()),
        Some(Status::Connecting) => {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            return;
        }
        Some(Status::Failed(error)) => complete(ctx, false, false, &error),
        _ => complete(ctx, false, false, "验收连接未就绪"),
    }
    fixture.browser = None;
    ctx.data_mut(|data| data.insert_temp(key(), fixture));
    ctx.request_repaint();
}

pub(super) fn destination(ctx: &egui::Context) -> Option<PathBuf> {
    ctx.data(|data| data.get_temp::<Fixture>(key()))
        .and_then(|fixture| fixture.output)
}

pub(super) fn complete(ctx: &egui::Context, success: bool, cancelled: bool, message: &str) {
    if ctx.data(|data| data.get_temp::<Fixture>(key())).is_none()
        || ctx
            .data(|data| data.get_temp::<bool>(egui::Id::new(READY)))
            .unwrap_or(false)
    {
        return;
    }
    if let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from) {
        let result = serde_json::json!({"passed":success,"cancelled":cancelled,"message":message});
        if std::fs::write(path.with_extension("print.json"), result.to_string()).is_ok() {
            ctx.data_mut(|data| data.insert_temp(egui::Id::new(READY), true));
            ctx.request_repaint();
        }
    }
}

#[derive(Default)]
struct ClickPdf {
    point: Option<egui::Pos2>,
    phase: u8,
}
impl egui::plugin::Plugin for ClickPdf {
    fn debug_name(&self) -> &'static str {
        "owned Markdown print toolbar"
    }
    fn output_hook(&mut self, _ctx: &egui::Context, output: &mut egui::FullOutput) {
        if self.phase != 0 {
            return;
        }
        self.point = output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "\u{f1c1}" => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            _ => None,
        });
    }
    fn input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        if input.viewport_id != egui::ViewportId::ROOT || self.phase >= 2 {
            return;
        }
        if let Some(pos) = self.point {
            input.events.push(egui::Event::PointerMoved(pos));
            input.events.push(egui::Event::PointerButton {
                pos,
                pressed: self.phase == 0,
                button: egui::PointerButton::Primary,
                modifiers: egui::Modifiers::NONE,
            });
            self.phase += 1;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(20));
    }
}
