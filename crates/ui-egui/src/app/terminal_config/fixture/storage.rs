//! Owned egui gestures exercise real forms and the encrypted SSH actor.
use super::*;
enum Gesture {
    Wait(u8),
    Click(&'static str),
    Edit(&'static str, &'static str),
}
#[derive(Default)]
pub(super) struct Input {
    actions: Vec<Gesture>,
    step: usize,
    phase: u8,
    target: Option<egui::Pos2>,
    stable: u8,
    written: bool,
}
impl Input {
    fn initialize(&mut self, mode: &str) {
        use Gesture::*;
        self.actions = if mode == "storage_form" {
            vec![
                Wait(1),
                Click("/dev/data/lv_data"),
                Click("格式化/挂载"),
                Click("不格式化"),
            ]
        } else if mode == "storage_error" {
            vec![
                Wait(1),
                Click("LV  lv_data"),
                Click("扩展 LV"),
                Edit("+10G", "+1G"),
                Click("确定"),
                Click("确定"),
                Wait(32),
            ]
        } else {
            vec![
                Wait(1),
                Click("/dev/sdb"),
                Click("新硬盘初始化为 PV"),
                Click("确定"),
                Click("确定"),
                Wait(2),
                Click("PV  /dev/sdb1"),
                Click("创建/扩展 VG"),
                Click("确定"),
                Click("确定"),
                Wait(4),
                Click("LV  lv_data"),
                Click("扩展 LV"),
                Edit("+10G", "+1G"),
                Click("确定"),
                Click("确定"),
                Wait(8),
                Click("/dev/data/lv_data"),
                Click("格式化/挂载"),
                Edit("/data", "/fixture-data"),
                Click("写入 /etc/fstab"),
                Click("确定"),
                Click("确定"),
                Wait(16),
            ]
        };
    }
    pub(super) fn input(&mut self, ctx: &Context, input: &mut RawInput) {
        if input.viewport_id != egui::ViewportId::ROOT {
            return;
        }
        input
            .events
            .push(Event::ModifiersChanged(egui::Modifiers::NONE));
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
        let Some(gesture) = self.actions.get(self.step) else {
            return;
        };
        let Some(pos) = self.target.filter(|_| self.stable >= 2) else {
            return;
        };
        if matches!(gesture, Gesture::Wait(_)) {
            return;
        }
        match self.phase {
            0 => input.events.push(Event::PointerMoved(pos)),
            1 | 2 => input.events.extend([
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: self.phase == 1,
                    modifiers: Default::default(),
                },
            ]),
            3 => {
                if let Gesture::Edit(_, text) = gesture {
                    let modifiers = egui::Modifiers {
                        ctrl: true,
                        command: true,
                        ..Default::default()
                    };
                    input.events.extend([
                        Event::ModifiersChanged(modifiers),
                        Event::Key {
                            key: egui::Key::A,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers,
                        },
                        Event::Text((*text).into()),
                        Event::Key {
                            key: egui::Key::A,
                            physical_key: None,
                            pressed: false,
                            repeat: false,
                            modifiers,
                        },
                    ]);
                }
            }
            _ => {}
        }
        self.phase += 1;
    }
    pub(super) fn output(&mut self, ctx: &Context, output: &egui::FullOutput, mode: &str) {
        if self.actions.is_empty() {
            self.initialize(mode);
        }
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
        let state = ctx
            .data(|d| d.get_temp::<(u8, bool)>(Id::new("storage-fixture-state")))
            .unwrap_or((0, true));
        let mut advance = false;
        if let Some(gesture) = self.actions.get(self.step) {
            match gesture {
                Gesture::Wait(mask) => advance = state.0 & mask == *mask && !state.1,
                Gesture::Click(label) | Gesture::Edit(label, _) => {
                    let target = labels
                        .iter()
                        .find(|(text, _)| {
                            if label.starts_with("LV  ") || label.starts_with("PV  ") {
                                text.starts_with(label)
                            } else {
                                text == label
                            }
                        })
                        .map(|(_, p)| *p);
                    if self.phase == 0 {
                        if self.target == target {
                            self.stable = self.stable.saturating_add(1);
                        } else {
                            self.target = target;
                            self.stable = 0;
                        }
                    }
                    advance = self.phase
                        >= if matches!(gesture, Gesture::Edit(..)) {
                            4
                        } else {
                            3
                        };
                }
            }
        }
        if advance {
            self.step += 1;
            self.phase = 0;
            self.target = None;
            self.stable = 0;
        }
        let error_visible = labels
            .iter()
            .any(|(s, _)| s.contains("storage fixture rejected mutation"));
        let popup =
            labels.iter().any(|(s, _)| *s == "xfs") && labels.iter().any(|(s, _)| *s == "ext4");
        if !self.written
            && self.step == self.actions.len()
            && (!matches!(mode, "storage_error") || error_visible)
            && (!matches!(mode, "storage_form") || popup)
            && let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from)
        {
            let proof = serde_json::json!({"mode":mode,"workflowComplete":true,"sessionReady":ctx.data(|d|d.get_temp::<bool>(Id::new("ssh-tab-fixture-ready"))).unwrap_or(false),"realLoopbackSsh":true,"osInputUsed":false,"actualBlockDevicesTouched":false,"unexpectedFilesPane":state.0&64!=0,"errorVisible":error_visible,"filesystemPopupVisible":popup,"flags":state.0,"gestures":self.actions.len(),"visibleLabels":labels.iter().map(|(s,_)|*s).collect::<Vec<_>>()});
            if std::fs::write(path.with_extension("ssh-tab.json"), proof.to_string()).is_ok() {
                self.written = true;
            }
        }
        if let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from)
            && !self.written
        {
            let _=std::fs::write(path.with_extension("storage-diagnostic.json"),serde_json::json!({"step":self.step,"phase":self.phase,"state":state,"target":self.target.map(|p|[p.x,p.y]),"labels":labels.iter().map(|(s,_)|*s).collect::<Vec<_>>().into_iter().take(80).collect::<Vec<_>>()} ).to_string());
        }
    }
}
