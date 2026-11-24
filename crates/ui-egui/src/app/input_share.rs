//! Feature controller. All OS and network work is dispatched through CommandHandler.
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tomato_services::{
    commands::{Command, CommandHandler, Engine, Output},
    input_share::{Action, PORT, Snapshot},
};
pub(super) struct State {
    pub open: bool,
    snapshot: Snapshot,
    address: String,
    code: String,
    port: u16,
    error: String,
    busy: bool,
    sender: mpsc::Sender<Action>,
    results: mpsc::Receiver<anyhow::Result<Snapshot>>,
    task: tokio::task::JoinHandle<()>,
    refreshed: Instant,
    dragging: Option<String>,
    found: Vec<String>,
    preview: bool,
    #[cfg(test)]
    cards: Vec<egui::Rect>,
}
impl Drop for State {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl State {
    pub fn new(runtime: &tokio::runtime::Runtime, ctx: &egui::Context) -> Self {
        let (sender, mut commands) = mpsc::channel(8);
        let (completed, results) = mpsc::channel(8);
        let repaint = ctx.clone();
        let task = runtime.spawn(async move {
            let mut engine = Engine::default();
            while let Some(action) = commands.recv().await {
                let result = engine
                    .execute(Command::InputShare { action })
                    .await
                    .and_then(|output| {
                        if let Output::InputShare(snapshot) = output {
                            Ok(snapshot)
                        } else {
                            anyhow::bail!("意外的共享服务结果")
                        }
                    });
                if completed.send(result).await.is_err() {
                    break;
                }
                repaint.request_repaint();
            }
        });
        Self {
            open: true,
            snapshot: Snapshot::default(),
            address: format!("192.168.1.2:{PORT}"),
            code: String::new(),
            port: PORT,
            error: String::new(),
            busy: false,
            sender,
            results,
            task,
            refreshed: Instant::now(),
            dragging: None,
            found: Vec::new(),
            preview: false,
            #[cfg(test)]
            cards: Vec::new(),
        }
    }
    fn send(&mut self, action: Action) {
        if self.preview {
            return;
        }
        if self.busy {
            return;
        }
        let status = matches!(action, Action::Status);
        match self.sender.try_send(action) {
            Ok(()) => {
                self.busy = true;
                if !status {
                    self.error.clear();
                }
            }
            Err(_) => self.error = "后台服务繁忙或已停止".into(),
        }
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        #[cfg(test)]
        self.cards.clear();
        while let Ok(result) = self.results.try_recv() {
            self.busy = false;
            match result {
                Ok(snapshot) => {
                    if !snapshot.discovered.is_empty() {
                        self.found = snapshot.discovered.clone();
                    }
                    self.snapshot = snapshot;
                }
                Err(error) => self.error = error.to_string(),
            }
        }
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("鼠标和键盘共享").open(&mut open).default_size(egui::vec2(780.0,440.0)).show(ctx,|ui| {
            ui.label("将各主机按实际摆放顺序横向排列，光标移出左右边缘后，键盘随光标切换。");
            ui.label("Windows · 按各主机虚拟桌面尺寸排列 · Ctrl + Alt + F12 紧急返回本机并暂停切换");
            ui.separator();
            ui.add_enabled_ui(!self.busy,|ui| {
                if !self.snapshot.running || (self.snapshot.hosting && self.snapshot.nodes.len()==1) {
                    ui.horizontal(|ui| {
                        ui.label("服务端口"); ui.add(egui::DragValue::new(&mut self.port).range(1..=65535));
                        if ui.add_enabled(!self.snapshot.running,egui::Button::new("启动服务")).clicked() { self.send(Action::Start { port:self.port }); }
                        if ui.button("扫描主机").clicked() { self.found.clear(); self.send(Action::Scan); }
                    });
                    for address in self.found.clone() {
                        if ui.selectable_label(self.address==address,&address).clicked() { self.address=address; }
                    }
                    ui.horizontal(|ui| { ui.label("主机地址"); ui.text_edit_singleline(&mut self.address); });
                    ui.horizontal(|ui| { ui.label("配对码"); ui.add(egui::TextEdit::singleline(&mut self.code).password(true));
                        if ui.button("主动加入").clicked() { self.send(Action::Join { address:self.address.trim().into(), code:self.code.trim().into() }); }
                    });
                    ui.weak("在一台主机启动服务；其他主机扫描后选择地址，输入该服务的配对码加入。");
                    ui.weak("局域网发现使用 UDP 24819；输入通信使用所选 TCP 端口。需允许防火墙访问专用网络。");
                }
                if self.snapshot.running {
                    ui.horizontal(|ui| {
                        ui.label(if self.snapshot.hosting { "服务已启动" } else { "已加入共享" });
                        if ui.button("停止并退出共享").clicked() { self.send(Action::Stop); }
                        if ui.button("返回本机 / 暂停").clicked() { self.send(Action::Release); }
                        if ui.button("恢复边缘切换").clicked() { self.send(Action::Resume); }
                    });
                    if self.snapshot.hosting {
                        ui.horizontal(|ui| { ui.label(format!("TCP {} · 配对码",self.snapshot.port));
                            if ui.button("复制配对码").clicked() { ctx.copy_text(self.snapshot.code.clone()); }
                            ui.label(egui::RichText::new(&self.snapshot.code).monospace());
                        });
                    }
                    ui.add_space(16.0);
                    ui.label("拖动主机调整左右顺序（所有节点同步）");
                    let nodes=self.snapshot.nodes.clone();
                    let mut move_to=None;
                    egui::ScrollArea::horizontal().show(ui,|ui| {
                        ui.horizontal(|ui| {
                            for (index,node) in nodes.iter().enumerate() {
                                let (rect,response)=ui.allocate_exact_size(egui::vec2(160.0,100.0),egui::Sense::drag());
                                #[cfg(test)]
                                self.cards.push(rect);
                                ui.painter().rect_filled(rect.shrink(3.0),6.0,if self.snapshot.active==node.id { egui::Color32::from_rgb(200,230,210) } else { egui::Color32::from_rgb(235,240,244) });
                                let suffix=if node.id==self.snapshot.local_id { "（本机）" } else { "" };
                                ui.painter().with_clip_rect(rect.shrink(5.0)).text(rect.center(),egui::Align2::CENTER_CENTER,format!("{}\n{} × {}{suffix}",node.name,node.width,node.height),egui::FontId::proportional(15.0),egui::Color32::from_gray(40));
                                if response.drag_started() { self.dragging=Some(node.id.clone()); }
                                if ui.input(|i| i.pointer.any_released()) && ui.input(|i|i.pointer.interact_pos()).is_some_and(|p|rect.contains(p)) {
                                    move_to=self.dragging.take().map(|id|(id,index));
                                }
                            }
                        });
                    });
                    if let Some((id,index))=move_to && let Some(old)=nodes.iter().position(|n|n.id==id) && old!=index {
                        let mut ids:Vec<_>=nodes.into_iter().map(|n|n.id).collect(); let id=ids.remove(old); ids.insert(index,id);
                        self.send(Action::Order { ids });
                    }
                    if ui.input(|i|i.pointer.any_released()) { self.dragging=None; }
                }
            });
            if self.busy { ui.spinner(); }
            if !self.error.is_empty() { ui.colored_label(egui::Color32::RED,&self.error); }
            if !self.snapshot.error.is_empty() { ui.colored_label(egui::Color32::RED,&self.snapshot.error); }
        });
        self.open = open;
        if !self.preview
            && self.snapshot.running
            && self.refreshed.elapsed() >= Duration::from_millis(500)
            && !self.busy
            && self.dragging.is_none()
            && !ctx.input(|i| i.pointer.any_down())
        {
            self.refreshed = Instant::now();
            self.send(Action::Status);
        }
        if self.snapshot.running || self.busy {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }
    pub fn preview(&mut self, connected: bool) {
        self.preview = true;
        if connected {
            use tomato_core::input_share::Node;
            self.snapshot = Snapshot {
                running: true,
                hosting: true,
                port: PORT,
                local_id: "work".into(),
                active: "laptop".into(),
                code: "00000000-0000-4000-8000-000000000000".into(),
                nodes: vec![
                    Node {
                        id: "work".into(),
                        name: "工作电脑".into(),
                        width: 3840,
                        height: 2160,
                    },
                    Node {
                        id: "laptop".into(),
                        name: "笔记本".into(),
                        width: 2560,
                        height: 1600,
                    },
                    Node {
                        id: "server".into(),
                        name: "测试主机".into(),
                        width: 1920,
                        height: 1080,
                    },
                ],
                ..Default::default()
            };
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drag_reorders_the_horizontal_nodes() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let ctx = egui::Context::default();
        let mut state = State::new(&runtime, &ctx);
        state.preview(true);
        state.preview = false;
        state.task.abort();
        let (sender, mut receiver) = mpsc::channel(8);
        state.sender = sender;
        let frame = |state: &mut State, events| {
            state.refreshed = Instant::now() + Duration::from_secs(3600);
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| state.show(ui.ctx()),
            );
            output.textures_delta.clear();
        };
        for _ in 0..3 {
            frame(&mut state, vec![]);
        }
        let source = state.cards[0].center();
        let target = state.cards[2].center();
        frame(
            &mut state,
            vec![
                egui::Event::PointerMoved(source),
                egui::Event::PointerButton {
                    pos: source,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(&mut state, vec![egui::Event::PointerMoved(target)]);
        frame(
            &mut state,
            vec![egui::Event::PointerButton {
                pos: target,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(
            matches!(receiver.try_recv(),Ok(Action::Order { ids }) if ids==["laptop","server","work"])
        );
    }
}
