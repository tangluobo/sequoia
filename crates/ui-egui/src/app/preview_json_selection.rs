//! Application-owned input only; clipboard commands are intercepted before the OS backend.
use super::*;
use egui::{Event, Id, Key, Modifiers, RawInput};
const INSTALLED: &str = "owned-json-selection-installed";
const GEOMETRY: &str = "owned-json-selection-geometry";
const FRAME: &str = "owned-json-selection-frame";
const DETAILS: &str = "owned-json-selection-details";
impl TomatoApp {
    pub(super) fn preview_json_selection(&mut self) -> anyhow::Result<()> {
        let profile = PathBuf::from(
            std::env::var_os("TOMATO_DATA_DIR")
                .ok_or_else(|| anyhow::anyhow!("JSON 验收需要隔离配置"))?,
        );
        anyhow::ensure!(
            profile.is_absolute()
                && profile.file_name().is_some_and(|n| n
                    .to_string_lossy()
                    .starts_with("tomato-json-selection-fixture-"))
                && self.store.connections.is_empty(),
            "JSON 验收需要专有空配置"
        );
        let profile = profile.canonicalize()?;
        anyhow::ensure!(
            self.repository
                .location()
                .and_then(|p| p.parent())
                .and_then(|p| p.canonicalize().ok())
                .as_ref()
                == Some(&profile),
            "JSON 验收配置位置不匹配"
        );
        let source = profile.join("input.txt");
        anyhow::ensure!(
            source.canonicalize()?.starts_with(&profile)
                && std::fs::metadata(&source)?.len() <= 128 * 1024,
            "JSON 验收仅允许专有小夹具"
        );
        let source = std::fs::read_to_string(source)?;
        let case = std::env::var("TOMATO_JSON_SELECTION_CASE").unwrap_or_default();
        anyhow::ensure!(
            matches!(
                case.as_str(),
                "structured"
                    | "folded"
                    | "raw"
                    | "mq"
                    | "kafka"
                    | "mq-folded"
                    | "mq-raw"
                    | "mq-scalar"
            ),
            "JSON 验收类型无效"
        );
        if matches!(
            case.as_str(),
            "mq" | "kafka" | "mq-folded" | "mq-raw" | "mq-scalar"
        ) {
            let mut connection =
                Connection::new(if case == "kafka" { "KAFKA" } else { "ROCKETMQ" });
            connection.name = "消息正文选择验收".into();
            connection.set("host", "127.0.0.1");
            connection.set("port", 0);
            let mut view = tabs::mq::State::default();
            view.page = tabs::mq::Page::Messages;
            view.loaded = true;
            view.topic = "owned-json-selection".into();
            view.selected = Some(0);
            view.rows =
                vec![serde_json::json!({"id":"owned-message","topic":"owned-json-selection"})];
            view.set_body(source);
            if case == "mq-folded" {
                view.body_view.all(true);
            }
            let id = connection.id.clone();
            self.tabs.push(Tab {
                id: id.clone(),
                source_id: id.clone(),
                title: "消息正文选择验收".into(),
                panel: Panel::MessageQueue {
                    connection,
                    view: Box::new(view),
                },
                busy: false,
                error: None,
            });
            self.active = Some(id);
            self.section = 0;
        } else {
            self.section = 1;
            self.open_tool("json_tool", "JSON处理工具");
            let Some(tab) = self.tabs.last_mut() else {
                anyhow::bail!("缺少 JSON 标签")
            };
            let Panel::Tool(tool) = &mut tab.panel else {
                anyhow::bail!("JSON 标签类型无效")
            };
            tool.input = source.clone();
            if case == "raw" {
                tool.set_output(tomato_core::tools::json_transform(&source, "unescape")?);
            } else {
                tool.set_json_tree(tomato_core::tools::json_transform(&source, "format")?);
                tool.json_status.success("格式化成功！");
                if case == "folded" {
                    tool.json_view.all(true);
                }
            }
        }
        let capture = PathBuf::from(
            std::env::var_os("TOMATO_UI_CAPTURE")
                .ok_or_else(|| anyhow::anyhow!("缺少 JSON 验收输出"))?,
        );
        let parent = capture
            .parent()
            .ok_or_else(|| anyhow::anyhow!("JSON 验收输出目录无效"))?
            .canonicalize()?;
        anyhow::ensure!(
            profile.parent() == Some(parent.as_path()),
            "JSON 验收输出须在专有夹具父目录"
        );
        self.context
            .data_mut(|d| d.insert_temp(Id::new(INSTALLED), true));
        self.context.add_plugin(Input {
            frame: 0,
            copies: 0,
            button: None,
            case,
            capture,
        });
        self.quit_approved = true;
        Ok(())
    }
    pub(super) fn json_selection_preview_ready(&self, ctx: &egui::Context) {
        if !ctx
            .data(|d| d.get_temp::<bool>(Id::new(INSTALLED)))
            .unwrap_or(false)
        {
            return;
        }
        let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| Some(&tab.id) == self.active.as_ref())
        else {
            return;
        };
        let view = match &tab.panel {
            Panel::Tool(tool) => &tool.json_view,
            Panel::MessageQueue { view, .. } => &view.body_view,
            _ => return,
        };
        if let Some(geometry) = view.viewport {
            ctx.data_mut(|d|{
                d.insert_temp(Id::new(GEOMETRY),geometry);
                d.insert_temp(Id::new(DETAILS),serde_json::json!({"rows":view.visible.len(),"longLineCacheEntries":view.long_lines.len(),"anchor":[view.selection.anchor.row,view.selection.anchor.byte],"head":[view.selection.head.row,view.selection.head.byte],"lineHeight":geometry.line_height,"gutter":geometry.gutter,"origin":[geometry.origin.x,geometry.origin.y],"viewport":[geometry.rect.left(),geometry.rect.top(),geometry.rect.width(),geometry.rect.height()],"busy":tab.busy,"error":tab.error.is_some()}));
            });
        }
    }
}
struct Input {
    frame: usize,
    copies: usize,
    case: String,
    capture: PathBuf,
    button: Option<egui::Pos2>,
}
fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}
impl egui::plugin::Plugin for Input {
    fn debug_name(&self) -> &'static str {
        "owned JSON selection input"
    }
    fn input_hook(&mut self, ctx: &egui::Context, input: &mut RawInput) {
        if input.viewport_id != egui::ViewportId::ROOT {
            return;
        }
        input
            .events
            .retain(|e| matches!(e, Event::Screenshot { .. }));
        input.focused = true;
        let Some(geometry) = ctx.data(|d| d.get_temp::<tabs::json::Viewport>(Id::new(GEOMETRY)))
        else {
            ctx.request_repaint();
            return;
        };
        self.frame += 1;
        ctx.data_mut(|d| d.insert_temp(Id::new(FRAME), self.frame));
        let control = Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        };
        let mut modifiers = Modifiers::NONE;
        match self.frame {
            1 | 2 => {
                let pos = geometry.rect.left_top() + egui::vec2(6., geometry.line_height / 2.);
                input.events.push(Event::PointerMoved(pos));
                input.events.push(Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: self.frame == 1,
                    modifiers,
                });
            }
            4 => {
                modifiers = control;
                input.events.push(key(Key::End, modifiers));
            }
            6 => {
                modifiers = Modifiers {
                    shift: true,
                    ..control
                };
                input.events.push(key(Key::Home, modifiers));
            }
            8 => input.events.push(Event::Copy),
            10 if self.case == "raw" => input.events.push(key(Key::ArrowRight, modifiers)),
            12 if self.case == "raw" => {
                modifiers.shift = true;
                input.events.push(key(Key::ArrowLeft, modifiers));
            }
            14 if self.case == "raw" => input.events.push(Event::Copy),
            16 | 17 => {
                if let Some(pos) = self.button {
                    input.events.push(Event::PointerMoved(pos));
                    input.events.push(Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: self.frame == 16,
                        modifiers,
                    });
                }
            }
            _ => {}
        }
        input.events.push(Event::ModifiersChanged(modifiers));
        if self.frame < 24 {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
    fn output_hook(&mut self, ctx: &egui::Context, output: &mut egui::FullOutput) {
        self.button = output.shapes.iter().find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape
                && t.galley.job.text == "复制全部"
            {
                Some(t.pos + t.galley.size() / 2.)
            } else {
                None
            }
        });
        output.platform_output.commands.retain(|command| {
            if let egui::OutputCommand::CopyText(text) = command {
                let path = self
                    .capture
                    .with_extension(format!("copy-{}.txt", self.copies));
                if std::fs::write(path, text).is_ok() {
                    self.copies += 1;
                }
                return false;
            }
            true
        });
        if ctx
            .data(|d| d.get_temp::<bool>(Id::new("owned-json-selection-ready")))
            .unwrap_or(false)
        {
            return;
        }
        if self.frame >= 22 && self.copies >= if self.case == "raw" { 3 } else { 2 } {
            let details = ctx
                .data(|d| d.get_temp::<serde_json::Value>(Id::new(DETAILS)))
                .unwrap_or_default();
            let geometry = ctx.data(|d| d.get_temp::<tabs::json::Viewport>(Id::new(GEOMETRY)));
            let max_bytes = output
                .shapes
                .iter()
                .filter(|s| {
                    geometry.is_some_and(|g| {
                        s.clip_rect.left() >= g.rect.left() - 1.
                            && s.clip_rect.right() <= g.rect.right() + 1.
                    })
                })
                .filter_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape {
                        Some(t.galley.job.text.len())
                    } else {
                        None
                    }
                })
                .max()
                .unwrap_or(0);
            let proof = serde_json::json!({"case":self.case,"frame":self.frame,"copies":self.copies,"details":details,"maxReadonlyShapedBytes":max_bytes,"physicalOsInputUsed":false,"systemClipboardWritten":false,"savedHostsContacted":false});
            if std::fs::write(
                self.capture.with_extension("selection.json"),
                proof.to_string(),
            )
            .is_ok()
            {
                ctx.data_mut(|d| d.insert_temp(Id::new("owned-json-selection-ready"), true));
                ctx.request_repaint();
            }
        }
    }
}
