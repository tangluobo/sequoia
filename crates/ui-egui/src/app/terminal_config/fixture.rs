//! Synthetic input is enabled only for the owned loopback SSH framebuffer fixture.
use egui::{Context, Event, Id, PointerButton, RawInput};
mod docker;
mod storage;
mod tree;
pub(super) fn install(ctx: &Context) -> bool {
    let Ok(mode) = std::env::var("TOMATO_SSH_TAB_FIXTURE") else {
        return false;
    };
    if !matches!(
        mode.as_str(),
        "menu"
            | "session"
            | "global"
            | "copy"
            | "fonts"
            | "docker"
            | "docker_tree"
            | "storage"
            | "storage_error"
            | "storage_form"
            | "ports"
            | "monitor"
    ) || std::env::var_os("TOMATO_UI_CAPTURE").is_none()
    {
        return false;
    }
    ctx.data_mut(|data| data.insert_temp(Id::new("ssh-tab-fixture-active"), true));
    ctx.add_plugin(Input {
        mode,
        frame: 0,
        target: None,
        font_arrow: None,
        docker: Default::default(),
        tree: Default::default(),
        storage: Default::default(),
    });
    true
}
struct Input {
    mode: String,
    frame: usize,
    target: Option<egui::Pos2>,
    font_arrow: Option<egui::Pos2>,
    docker: docker::Input,
    tree: tree::Input,
    storage: storage::Input,
}
impl egui::plugin::Plugin for Input {
    fn debug_name(&self) -> &'static str {
        "owned SSH tab fixture"
    }
    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        input
            .events
            .retain(|event| matches!(event, Event::Screenshot { .. }));
        input.focused = true;
        if matches!(self.mode.as_str(), "ports" | "monitor") {
            return;
        }
        if self.mode.starts_with("storage") {
            self.storage.input(ctx, input);
            return;
        }
        if self.mode == "docker_tree" {
            self.tree.input(ctx, input);
            return;
        }
        if self.mode == "docker" {
            self.docker.input(ctx, input);
            return;
        }
        if input.viewport_id != egui::ViewportId::ROOT {
            if self.mode == "session" {
                ctx.data_mut(|data| {
                    data.insert_temp(Id::new("tomato-capture-viewport"), input.viewport_id)
                });
            }
            return;
        }
        self.frame += 1;
        let click = match self.frame {
            10 | 11 => Some((
                egui::pos2(350., 70.),
                PointerButton::Secondary,
                self.frame == 10,
            )),
            24 | 25 if self.mode != "menu" => self
                .target
                .map(|point| (point, PointerButton::Primary, self.frame == 24)),
            60 | 61 if self.mode == "fonts" => self
                .font_arrow
                .map(|point| (point, PointerButton::Primary, self.frame == 60)),
            _ => None,
        };
        if let Some((pos, button, pressed)) = click {
            input.events.push(Event::PointerMoved(pos));
            input.events.push(Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers: Default::default(),
            });
        }
        if self.frame < if self.mode == "fonts" { 90 } else { 32 } {
            ctx.request_repaint();
        }
    }
    fn output_hook(&mut self, ctx: &Context, output: &mut egui::FullOutput) {
        if self.mode.starts_with("storage") {
            self.storage.output(ctx, output, &self.mode);
            return;
        }
        if self.mode == "docker_tree" {
            self.tree.output(ctx, output);
            return;
        }
        if self.mode == "docker" {
            self.docker.output(ctx, output);
            return;
        }
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
        let target = if self.mode == "copy" {
            "复制会话"
        } else if self.mode == "session" {
            "会话配置"
        } else {
            "全局配置"
        };
        if let Some((_, pos, size)) = texts.iter().find(|(text, _, _)| *text == target) {
            self.target = Some(*pos + *size / 2.);
        }
        if self.mode == "fonts"
            && let Some((_, pos, size)) = texts.iter().find(|(text, _, _)| *text == "▾")
        {
            self.font_arrow = Some(*pos + *size / 2.);
        }
        let expected: &[&str] = match self.mode.as_str() {
            "monitor" => &["系统监控", "每1秒更新"],
            "ports" => &["端口监听", "协议:", "TCP", "UDP", "sshd"],
            "session" => &["回滚行数:", "默认文件视图:", "确定", "取消"],
            "global" => &["字体名称", "字体大小", "回滚行数", "应用并保存"],
            "fonts" => &["字体名称", "字体大小", "回滚行数", "monospace"],
            "copy" => &[],
            _ => &["复制会话", "会话配置", "全局配置"],
        };
        if expected
            .iter()
            .all(|label| texts.iter().any(|(text, _, _)| text == label))
            && let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from)
        {
            let ready = ctx
                .data(|data| data.get_temp::<bool>(Id::new("ssh-tab-fixture-ready")))
                .unwrap_or(false);
            let counts = ctx
                .data(|data| data.get_temp::<(usize, usize)>(Id::new("ssh-tab-fixture-counts")))
                .unwrap_or_default();
            let rows:Vec<_>=output.shapes.iter().filter_map(|shape| if let egui::Shape::Text(text)=&shape.shape{Some(serde_json::json!({"text":text.galley.job.text,"rows":text.galley.rows.len()}))}else{None}).collect();
            let names = (self.mode == "fonts").then(|| {
                crate::font_assets::terminal::names(ctx)
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
            });
            let monitor = ctx
                .data(|data| data.get_temp::<serde_json::Value>(Id::new("monitor-fixture-state")));
            let proof = serde_json::json!({"monitor":monitor,"mode":self.mode,"frame":self.frame,"texts":texts.iter().map(|(text,pos,size)|serde_json::json!({"text":text,"x":pos.x,"y":pos.y,"width":size.x,"height":size.y})).collect::<Vec<_>>(),"textRows":rows,"fontNames":names,"fontArrow":self.font_arrow.map(|point|[point.x,point.y]),"osInputUsed":false,"realLoopbackSsh":true,"sessionReady":ready,"sessions":counts.0,"readySessions":counts.1});
            let _ = std::fs::write(path.with_extension("ssh-tab.json"), proof.to_string());
        }
    }
}
