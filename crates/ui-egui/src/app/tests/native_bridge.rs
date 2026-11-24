//! Own egui viewports only. No operating-system input, windows or clipboard writes.
use std::{cell::RefCell, rc::Rc};
#[derive(Default)]
struct State {
    time: f64,
    events: Vec<egui::Event>,
    close: bool,
    text: Vec<(String, egui::Pos2)>,
    target: Option<egui::ViewportId>,
    last: Option<egui::ViewportId>,
    copied_text: Option<String>,
}
pub(crate) struct Bridge(Rc<RefCell<State>>);
impl Bridge {
    pub fn root_frame(
        &self,
        ctx: &egui::Context,
        root: Vec<egui::Event>,
        child: Vec<egui::Event>,
        draw: impl FnMut(&mut egui::Ui),
    ) -> egui::FullOutput {
        let time = {
            let mut state = self.0.borrow_mut();
            state.time = state.time.max(ctx.input(|input| input.time)) + 0.1;
            state.events = child;
            state.time
        };
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200., 900.),
                )),
                time: Some(time),
                events: root,
                ..Default::default()
            },
            draw,
        );
        output.textures_delta.clear();
        let mut state = self.0.borrow_mut();
        state.target = state.last.take();
        output
    }
    pub fn new(ctx: &egui::Context) -> Self {
        let state = Rc::new(RefCell::new(State::default()));
        let native = state.clone();
        ctx.set_embed_viewports(false);
        egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
            let (time, events, close) = {
                let mut state = native.borrow_mut();
                let targeted = state
                    .target
                    .is_none_or(|target| target == viewport.ids.this);
                let events = if targeted {
                    std::mem::take(&mut state.events)
                } else {
                    Vec::new()
                };
                state.last = Some(viewport.ids.this);
                let close = targeted && std::mem::take(&mut state.close);
                (state.time, events, close)
            };
            let rect =
                egui::Rect::from_min_size(egui::Pos2::ZERO, viewport.builder.inner_size.unwrap());
            let mut input = egui::RawInput {
                viewport_id: viewport.ids.this,
                screen_rect: Some(rect),
                time: Some(time),
                events,
                ..Default::default()
            };
            input.viewports.insert(
                viewport.ids.this,
                egui::ViewportInfo {
                    events: if close {
                        vec![egui::ViewportEvent::Close]
                    } else {
                        Vec::new()
                    },
                    parent: Some(viewport.ids.parent),
                    inner_rect: Some(rect),
                    focused: Some(true),
                    ..Default::default()
                },
            );
            let mut output = ctx.run_ui(input, |ui| (viewport.viewport_ui_cb)(ui));
            // Nested native popups own the final input target and its labels.
            if native.borrow().last == Some(viewport.ids.this) {
                native.borrow_mut().text = output
                    .shapes
                    .iter()
                    .filter_map(|shape| {
                        if let egui::Shape::Text(text) = &shape.shape {
                            Some((
                                text.galley.job.text.clone(),
                                text.pos + text.galley.size() / 2.,
                            ))
                        } else {
                            None
                        }
                    })
                    .collect();
            }
            for command in std::mem::take(&mut output.platform_output.commands) {
                if let egui::OutputCommand::CopyText(text) = command {
                    native.borrow_mut().copied_text = Some(text);
                }
            }
            output.textures_delta.clear();
        });
        Self(state)
    }
    pub fn frame(
        &self,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        mut draw: impl FnMut(&egui::Context),
    ) {
        let time = {
            let mut state = self.0.borrow_mut();
            state.time = state.time.max(ctx.input(|input| input.time)) + 0.1;
            state.events = events;
            state.time
        };
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 800.),
                )),
                time: Some(time),
                ..Default::default()
            },
            |_| draw(ctx),
        );
        output.textures_delta.clear();
        let mut state = self.0.borrow_mut();
        state.target = state.last.take();
    }
    pub fn click(
        &self,
        ctx: &egui::Context,
        point: egui::Pos2,
        mut draw: impl FnMut(&egui::Context),
    ) {
        for pressed in [true, false] {
            self.frame(
                ctx,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
                &mut draw,
            );
        }
        self.frame(ctx, Vec::new(), draw);
    }
    pub fn close_viewport(
        &self,
        ctx: &egui::Context,
        viewport: egui::ViewportId,
        draw: impl FnMut(&egui::Context),
    ) {
        {
            let mut state = self.0.borrow_mut();
            state.target = Some(viewport);
            state.close = true;
        }
        self.frame(ctx, Vec::new(), draw);
    }
    /// The test renderer records output instead of writing the operating-system clipboard.
    pub fn take_copied_text(&self) -> Option<String> {
        self.0.borrow_mut().copied_text.take()
    }
    pub fn text(&self, text: &str) -> anyhow::Result<egui::Pos2> {
        self.0
            .borrow()
            .text
            .iter()
            .find(|(value, _)| value == text)
            .map(|(_, point)| *point)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "native control text missing: {text}; captured {:?}",
                    self.0
                        .borrow()
                        .text
                        .iter()
                        .map(|(text, _)| text)
                        .collect::<Vec<_>>()
                )
            })
    }
}
