//! Synthetic input is restricted to the owned, isolated downloader capture profile.
use super::*;
use egui::{Context, Event, PointerButton, RawInput};
impl TomatoApp {
    pub(in crate::app) fn preview_saved_downloader(&mut self, ctx: &Context) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("Missing owned profile"))?;
        let mode = std::env::var("TOMATO_DOWNLOADER_FIXTURE")?;
        anyhow::ensure!(
            profile.is_absolute()
                && profile.file_name().is_some_and(|n| n
                    .to_string_lossy()
                    .starts_with("tomato-downloader-fixture-"))
                && matches!(
                    mode.as_str(),
                    "tree" | "add" | "settings" | "catalog" | "download" | "ed2k"
                )
                && self.store.connections.len() == 1
                && self.store.connections[0].kind() == "TOOL"
                && self.store.connections[0].text("toolType") == "downloader",
            "Requires owned downloader profile"
        );
        self.quit_approved = true;
        self.section = if mode == "catalog" { 1 } else { 0 };
        if mode == "catalog" {
            self.open_tool("downloader", "下载器");
        }
        let url = if matches!(mode.as_str(), "download" | "ed2k") {
            let url = std::fs::read_to_string(profile.join("fixture-url.txt"))?;
            if mode == "ed2k" {
                anyhow::ensure!(
                    url.trim() == "ed2k://|file|fixture.bin|3|A448017AAF21D8525FC10AE87AA6729D|/",
                    "Requires owned abc eD2k fixture"
                );
                let network: serde_json::Value = serde_json::from_slice(&std::fs::read(
                    profile.join("rust-ed2k/network.json"),
                )?)?;
                anyhow::ensure!(
                    network["loopbackOnly"] == true && network["kad"] == false,
                    "Requires isolated eD2k networking"
                );
                let servers = network["servers"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("Missing loopback server"))?;
                anyhow::ensure!(
                    !servers.is_empty()
                        && servers.iter().all(|v| v
                            .as_str()
                            .and_then(|s| s.parse::<std::net::SocketAddrV4>().ok())
                            .is_some_and(|a| a.ip().is_loopback() && a.port() > 0)),
                    "Requires owned loopback eD2k servers"
                );
            } else {
                let endpoint = url
                    .strip_prefix("http://127.0.0.1:")
                    .and_then(|value| value.strip_suffix("/fixture.bin"));
                anyhow::ensure!(
                    endpoint
                        .and_then(|port| port.parse::<u16>().ok())
                        .is_some_and(|port| port > 0),
                    "Requires owned loopback download URL"
                );
            }
            Some(url)
        } else {
            None
        };
        ctx.add_plugin(Input {
            mode,
            tree_name: self.store.connections[0].name.clone(),
            frame: 0,
            tree: None,
            button: None,
            opened: false,
            history: false,
            action: 0,
            written: false,
            url,
            child_frame: 0,
            submit: None,
        });
        Ok(())
    }
}
struct Input {
    mode: String,
    tree_name: String,
    frame: usize,
    tree: Option<egui::Pos2>,
    button: Option<egui::Pos2>,
    opened: bool,
    history: bool,
    action: u8,
    written: bool,
    url: Option<String>,
    child_frame: usize,
    submit: Option<egui::Pos2>,
}
impl egui::plugin::Plugin for Input {
    fn debug_name(&self) -> &'static str {
        "owned saved downloader fixture"
    }
    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        input
            .events
            .retain(|event| matches!(event, Event::Screenshot { .. }));
        input.focused = true;
        if input.viewport_id != egui::ViewportId::ROOT {
            if matches!(self.mode.as_str(), "download" | "ed2k") {
                self.child_frame += 1;
                if self.child_frame == 3 {
                    input.events.push(Event::Text(self.url.clone().unwrap()));
                }
                if matches!(self.child_frame, 5 | 6)
                    && let Some(pos) = self.submit
                {
                    input.events.push(Event::PointerMoved(pos));
                    input.events.push(Event::PointerButton {
                        pos,
                        pressed: self.child_frame == 5,
                        button: PointerButton::Primary,
                        modifiers: Default::default(),
                    });
                }
                if self.child_frame < 8 {
                    ctx.request_repaint();
                }
            }
            return;
        }
        self.frame += 1;
        let event = if self.mode != "catalog" && matches!(self.frame, 6 | 7 | 10 | 11) {
            self.tree.map(|point| (point, matches!(self.frame, 6 | 10)))
        } else if matches!(self.mode.as_str(), "add" | "settings" | "download" | "ed2k")
            && self.history
            && self.opened
            && self.action < 2
        {
            self.button.map(|point| {
                let pressed = self.action == 0;
                self.action += 1;
                (point, pressed)
            })
        } else {
            None
        };
        if let Some((pos, pressed)) = event {
            input.events.push(Event::PointerMoved(pos));
            input.events.push(Event::PointerButton {
                pos,
                pressed,
                button: PointerButton::Primary,
                modifiers: Default::default(),
            });
        }
        if self.frame < 48
            || !self.history
            || (matches!(self.mode.as_str(), "add" | "settings" | "download" | "ed2k")
                && self.action < 2)
        {
            ctx.request_repaint();
        }
    }
    fn output_hook(&mut self, ctx: &Context, output: &mut egui::FullOutput) {
        let texts: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    Some((text.galley.job.text.as_str(), text.pos, text.galley.size()))
                } else {
                    None
                }
            })
            .collect();
        let labels: Vec<_> = texts.iter().map(|(text, _, _)| *text).collect();
        // end_pass has already restored the parent context when this hook runs.
        // Identify this output by its painted controls, not ctx.viewport_id().
        if labels.contains(&self.tree_name.as_str()) || labels.contains(&"移除记录") {
            if let Some((_, pos, size)) = texts.iter().find(|(text, _, _)| *text == self.tree_name)
            {
                self.tree = Some(*pos + *size / 2.);
            }
            self.opened |= labels.contains(&"移除记录");
            self.history |= labels.contains(&"旧下载记录.bin");
            let action = if self.mode == "settings" {
                "设置"
            } else {
                "添加下载"
            };
            if let Some((_, pos, size)) = texts.iter().find(|(text, _, _)| *text == action) {
                self.button = Some(*pos + *size / 2.);
            }
        } else if matches!(self.mode.as_str(), "download" | "ed2k")
            && let Some((_, pos, size)) = texts.iter().find(|(text, _, _)| *text == "添加下载")
        {
            self.submit = Some(*pos + *size / 2.);
        }
        let expected: &[&str] = match self.mode.as_str() {
            "add" => &["添加新的下载任务", "下载地址", "取消"],
            "settings" => &["设置下载器默认值", "默认下载目录", "取消"],
            "download" => &["fixture.bin", "已完成"],
            "ed2k" => &["fixture.bin", "已完成", "ED2K · 0"],
            _ => &["移除记录", "旧下载记录.bin"],
        };
        if !self.written
            && self.opened
            && self.history
            && expected.iter().all(|text| labels.contains(text))
            && let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from)
        {
            if matches!(self.mode.as_str(), "download" | "ed2k") {
                ctx.data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new("tomato-capture-viewport"),
                        egui::ViewportId::ROOT,
                    )
                });
            }
            let child_title = if self.mode == "settings" {
                "下载器设置"
            } else {
                "添加下载"
            };
            let native_child = output.viewport_output.iter().any(|(id, viewport)| {
                *id != egui::ViewportId::ROOT
                    && viewport.parent == egui::ViewportId::ROOT
                    && viewport.class == egui::ViewportClass::Immediate
                    && viewport.builder.title.as_deref() == Some(child_title)
            });
            let proof = serde_json::json!({"mode":self.mode,"treeDoubleClicked":self.mode!="catalog","historyLoaded":true,"toolOpened":true,"osInputUsed":false,"nativeChild":native_child,"initializationFailureShown":labels.contains(&"下载器初始化失败"),"downloadCompleted":matches!(self.mode.as_str(), "download" | "ed2k"),"texts":labels});
            let _ = std::fs::write(path.with_extension("downloader.json"), proof.to_string());
            self.written = true;
        }
    }
}
