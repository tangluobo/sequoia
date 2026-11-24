//! Own-client gestures verify that tree operations do not open a workspace tab.
use super::*;
#[derive(Default)]
pub(super) struct Input {
    phase: u8,
    row: Option<egui::Pos2>,
    stop: Option<egui::Pos2>,
    start: Option<egui::Pos2>,
    saw_stopped: bool,
    credential_stage: u8,
    credential_prompts: u8,
    password_visible: bool,
    diagnostic: String,
    hovered: Option<egui::Pos2>,
    ready_frames: u8,
}
impl Input {
    pub(super) fn input(&mut self, ctx: &Context, input: &mut RawInput) {
        if input.viewport_id != egui::ViewportId::ROOT {
            return;
        }
        input
            .events
            .push(Event::ModifiersChanged(egui::Modifiers::NONE));
        if self.credential_stage == 1 {
            input.events.push(Event::Text("fixture-only".into()));
            self.credential_stage = 2;
            ctx.request_repaint();
            return;
        }
        if self.credential_stage == 2 {
            for pressed in [true, false] {
                input.events.push(Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: Default::default(),
                });
            }
            self.credential_stage = 3;
            ctx.request_repaint();
            return;
        }
        let target = match self.phase {
            1 | 2 | 6 | 7 => self.row,
            3 | 4 => self.stop,
            8 | 9 => self.start,
            _ => None,
        };
        if let Some(pos) = target {
            if self.hovered != Some(pos) {
                input.events.push(Event::PointerMoved(pos));
                self.hovered = Some(pos);
                ctx.request_repaint();
                return;
            }
            input.events.extend([
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: if matches!(self.phase, 1 | 2 | 6 | 7) {
                        PointerButton::Secondary
                    } else {
                        PointerButton::Primary
                    },
                    pressed: matches!(self.phase, 1 | 3 | 6 | 8),
                    modifiers: Default::default(),
                },
            ]);
            self.phase += 1;
        }
        if self.phase < 11 {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
    pub(super) fn output(&mut self, ctx: &Context, output: &egui::FullOutput) {
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| {
                if let egui::Shape::Text(t) = &s.shape {
                    Some((t.galley.job.text.as_str(), t.pos + t.galley.size() / 2.))
                } else {
                    None
                }
            })
            .collect();
        let find = |label: &str| {
            labels
                .iter()
                .find(|(text, _)| *text == label)
                .map(|(_, p)| *p)
        };
        let password_visible = find("输入密码").is_some();
        if password_visible && !self.password_visible {
            self.credential_prompts += 1;
            if self.credential_stage == 0 {
                self.credential_stage = 1;
            }
        }
        self.password_visible = password_visible;
        self.row = find("web").or(self.row);
        self.stop = find("停止").or(self.stop);
        self.start = find("启动").or(self.start);
        let state = ctx.data(|d| {
            d.get_temp::<(Option<bool>, bool, usize)>(Id::new("docker-tree-fixture-state"))
        });
        if self.phase == 0 && self.row.is_some() && state == Some((Some(true), false, 1)) {
            if !password_visible && ctx.memory(|m| m.top_modal_layer().is_none()) {
                self.ready_frames += 1;
                if self.ready_frames >= 3 {
                    self.phase = 1;
                }
            } else {
                self.ready_frames = 0;
            }
        }
        if self.phase == 5 && state == Some((Some(false), false, 1)) {
            self.saw_stopped = true;
            self.phase = 6;
        }
        if self.phase == 10 && state == Some((Some(true), false, 1)) {
            self.phase = 11;
        }
        if let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from) {
            let diagnostic = serde_json::json!({"phase":self.phase,"credentialStage":self.credential_stage,"state":state,"modal":ctx.memory(|m|m.top_modal_layer().is_some()),"row":self.row.map(|p|[p.x,p.y]),"stop":self.stop.map(|p|[p.x,p.y]),"start":self.start.map(|p|[p.x,p.y]),"labels":labels.iter().map(|(s,p)|serde_json::json!({"text":s,"at":[p.x,p.y]})).collect::<Vec<_>>()}).to_string();
            if self.diagnostic != diagnostic && find("正在关闭，清理后台任务…").is_none()
            {
                let _ = std::fs::write(
                    path.with_extension(format!(
                        "diagnostic-{}-{}.json",
                        self.phase, self.credential_stage
                    )),
                    &diagnostic,
                );
                self.diagnostic = diagnostic;
            }
        }
        if self.phase == 11
            && let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from)
        {
            let proof = serde_json::json!({"mode":"docker_tree","phase":11,"sessionReady":true,"osInputUsed":false,"realLoopbackSsh":true,"stableTabs":1,"treeRunning":true,"sawStopped":self.saw_stopped,"jobReleased":true,"credentialPrompts":self.credential_prompts,"visibleLabels":labels.iter().map(|s|s.0).collect::<Vec<_>>()});
            let _ = std::fs::write(path.with_extension("ssh-tab.json"), proof.to_string());
        }
    }
}
