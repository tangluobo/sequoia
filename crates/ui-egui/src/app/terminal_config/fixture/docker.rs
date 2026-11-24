//! Only the owned loopback framebuffer fixture injects these in-process egui events.
use super::*;
#[derive(Default)]
pub(super) struct Input {
    phase: u8,
    memory: Option<egui::Pos2>,
    resource: Option<egui::Pos2>,
    restart: Option<egui::Pos2>,
    combo: Option<egui::Pos2>,
    always: Option<egui::Pos2>,
}
impl Input {
    pub(super) fn input(&mut self, ctx: &Context, input: &mut RawInput) {
        if input.viewport_id != egui::ViewportId::ROOT {
            return;
        }
        input
            .events
            .push(Event::ModifiersChanged(egui::Modifiers::NONE));
        let target = match self.phase {
            1 | 2 => self.memory,
            4 | 5 => self.resource,
            8 | 9 => self.combo,
            10 | 11 => self.always,
            12 | 13 => self.restart,
            _ => None,
        };
        if let Some(pos) = target {
            input.events.extend([
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: matches!(self.phase, 1 | 4 | 8 | 10 | 12),
                    modifiers: Default::default(),
                },
            ]);
            self.phase += 1;
        } else if self.phase == 3 {
            let modifiers = egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            };
            input.events.push(Event::ModifiersChanged(modifiers));
            input.events.extend([
                Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                },
                Event::Text("2g".into()),
                Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers,
                },
            ]);
            self.phase = 4;
        }
        if self.phase < 15 {
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
        let find = |text: &str, index: usize| {
            labels
                .iter()
                .filter(|(label, _)| *label == text)
                .nth(index)
                .map(|(_, p)| *p)
        };
        self.memory = find("512m", 0).or(self.memory);
        self.resource = find("应用", 1).or(self.resource);
        self.restart = find("应用", 0).or(self.restart);
        self.combo = find("on-failure", 0).or(self.combo);
        self.always = find("always", 0).or(self.always);
        let state = ctx.data(|data| {
            data.get_temp::<(u64, String, bool)>(Id::new("docker-fixture-inspection"))
        });
        if self.phase == 0 && self.memory.is_some() && self.resource.is_some() {
            self.phase = 1;
        }
        if self.phase == 6
            && state
                .as_ref()
                .is_some_and(|(memory, _, busy)| *memory == 2_u64 << 30 && !busy)
        {
            self.phase = 8;
        }
        if self.phase == 14
            && state.as_ref().is_some_and(|(memory, policy, busy)| {
                *memory == 2_u64 << 30 && policy == "always" && !busy
            })
        {
            self.phase = 15;
        }
        if self.phase == 15
            && let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from)
        {
            let ready = ctx
                .data(|data| data.get_temp::<bool>(Id::new("ssh-tab-fixture-ready")))
                .unwrap_or(false);
            let proof = serde_json::json!({"mode":"docker","phase":self.phase,"sessionReady":ready,"osInputUsed":false,"realLoopbackSsh":true,"memoryBytes":state.as_ref().map(|s|s.0),"policy":state.as_ref().map(|s|s.1.as_str()),"busy":state.as_ref().map(|s|s.2),"visibleLabels":labels.iter().map(|s|s.0).collect::<Vec<_>>()});
            let _ = std::fs::write(path.with_extension("ssh-tab.json"), proof.to_string());
        }
    }
}
