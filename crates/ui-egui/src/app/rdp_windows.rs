//! Legacy WINDOW/TAB policy and native-window lifecycle; every viewport owns one existing tab/session.
use super::*;
use crate::tabs::desktop::Window;
#[cfg(test)]
mod close_tests;
#[cfg(test)]
mod tests;
mod toolbar;
#[derive(Clone, Copy)]
pub(super) enum Action {
    Fullscreen,
    Restore,
    Session,
    Global,
    Close,
    Minimize,
}
pub(super) fn viewport(id: &str) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("rdp-window", id))
}
pub(super) fn global_mode(settings: &SettingsState) -> &str {
    settings
        .extra
        .get("rdpOpenMode")
        .and_then(serde_json::Value::as_str)
        .filter(|mode| !tomato_core::java_text::is_blank(mode))
        .unwrap_or("WINDOW")
}
pub(super) fn independent(connection: &Connection, settings: &SettingsState) -> bool {
    let mode = connection.text("rdpOpenMode");
    let mode = if tomato_core::java_text::is_blank(mode) {
        global_mode(settings)
    } else {
        mode
    };
    !mode.eq_ignore_ascii_case("TAB")
}
impl TomatoApp {
    pub(super) fn toggle_adb_window(&mut self, id: &str) {
        let Some(Tab {
            panel: Panel::NativeDesktop {
                connection, view, ..
            },
            ..
        }) = self.tabs.iter_mut().find(|tab| tab.id == id)
        else {
            return;
        };
        if connection.kind() != "ADB" {
            return;
        }
        if view.window.is_some() {
            self.rdp_action(id, Action::Restore);
            self.context.send_viewport_cmd_to(
                egui::ViewportId::ROOT,
                egui::ViewportCommand::Minimized(false),
            );
            self.context
                .send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
            return;
        }
        let mut releases = Vec::new();
        view.release_inputs(&mut |input| releases.push(input));
        view.queue_inputs(releases);
        view.needs_focus = true;
        view.window = Some(Box::new(Window::new(true, false)));
        if self.active.as_deref() == Some(id) {
            self.active = self
                .tabs
                .iter()
                .rev()
                .find(|tab| tab.workspace_visible())
                .map(|tab| tab.id.clone());
        }
        self.bind_rdp_repaint(id);
        self.context.request_repaint_of(egui::ViewportId::ROOT);
    }
    pub(super) fn close_requested_rdp_windows(&mut self, ctx: &egui::Context) {
        // Hidden application-owned viewports receive logic ticks without a UI
        // pass. A parent tick may also observe the child's close before it paints.
        let closing = ctx.input(|input| {
            if !input
                .raw
                .viewports
                .values()
                .any(egui::ViewportInfo::close_requested)
            {
                return Vec::new();
            }
            self.tabs
                .iter()
                .filter(|tab| {
                    matches!(&tab.panel, Panel::NativeDesktop { view, .. } if view.window.is_some())
                        && input
                            .raw
                            .viewports
                            .get(&viewport(&tab.id))
                            .is_some_and(egui::ViewportInfo::close_requested)
                })
                .map(|tab| tab.id.clone())
                .collect::<Vec<_>>()
        });
        for id in closing {
            if std::env::var_os("TOMATO_CAPTURE_TRACE").is_some() {
                eprintln!("RDP close accepted from {:?}", ctx.viewport_id());
            }
            self.rdp_action(&id, Action::Close);
        }
    }
    pub(super) fn open_rdp(&mut self, connection: Connection) {
        let window = independent(&connection, &self.settings);
        if let Some(tab)=self.tabs.iter().find(|tab|tab.source_id==connection.id&&matches!(&tab.panel,Panel::NativeDesktop{view,..}if view.window.as_ref().is_some_and(|w|!w.restore_tab)==window)) {
            if matches!(&tab.panel,Panel::NativeDesktop{view,..}if view.window.is_some()) {
                self.context.send_viewport_cmd_to(viewport(&tab.id),egui::ViewportCommand::Minimized(false));
                self.context.send_viewport_cmd_to(viewport(&tab.id),egui::ViewportCommand::Focus);
            } else {self.active=Some(tab.id.clone());}
            return;
        }
        match tabs::create_tab(
            connection.clone(),
            self.runtime.as_ref().map(|runtime| runtime.handle()),
        ) {
            Ok(mut tab) => {
                if let Panel::NativeDesktop { view, session, .. } = &mut tab.panel {
                    if window || connection.flag("fullscreen") {
                        view.window = Some(Box::new(Window::new(
                            !window,
                            connection.flag("fullscreen"),
                        )));
                    }
                    if let Some(session) = session {
                        let ctx = self.context.clone();
                        let target = if view.window.is_some() {
                            viewport(&tab.id)
                        } else {
                            egui::ViewportId::ROOT
                        };
                        session.on_output(move || ctx.request_repaint_of(target));
                    }
                }
                let detached =
                    matches!(&tab.panel,Panel::NativeDesktop{view,..}if view.window.is_some());
                if !detached {
                    self.active = Some(tab.id.clone());
                }
                if detached
                    && std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("rdp")
                    && std::env::var_os("TOMATO_UI_CAPTURE").is_some()
                {
                    self.context.data_mut(|data| {
                        data.insert_temp(
                            egui::Id::new("tomato-capture-viewport"),
                            viewport(&tab.id),
                        )
                    });
                }
                self.tabs.push(tab);
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }
    pub(super) fn rdp_action(&mut self, id: &str, action: Action) {
        if matches!(action, Action::Global | Action::Session) {
            self.context.send_viewport_cmd_to(
                egui::ViewportId::ROOT,
                egui::ViewportCommand::Minimized(false),
            );
            self.context
                .send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
        }
        if matches!(
            action,
            Action::Fullscreen | Action::Restore | Action::Minimize
        ) && let Some(Tab {
            panel: Panel::NativeDesktop { view, .. },
            ..
        }) = self.tabs.iter_mut().find(|tab| tab.id == id)
        {
            let mut releases = Vec::new();
            view.release_inputs(&mut |input| releases.push(input));
            view.queue_inputs(releases);
            view.needs_focus = true;
        }
        match action {
            Action::Global => {
                self.section = 4;
                self.settings_page = super::settings::Page::Rdp;
            }
            Action::Session => self.open_session_config(id),
            Action::Close => self.close_tab(id, true),
            Action::Minimize => self
                .context
                .send_viewport_cmd_to(viewport(id), egui::ViewportCommand::Minimized(true)),
            Action::Fullscreen => {
                if let Some(Tab {
                    panel: Panel::NativeDesktop { view, .. },
                    ..
                }) = self.tabs.iter_mut().find(|tab| tab.id == id)
                {
                    if let Some(window) = &mut view.window {
                        window.enter_fullscreen(self.context.input(|input| input.time));
                    } else {
                        view.window = Some(Box::new(Window::new(true, true)));
                    }
                    view.needs_focus = true;
                    self.context.send_viewport_cmd_to(
                        viewport(id),
                        egui::ViewportCommand::Fullscreen(true),
                    );
                }
            }
            Action::Restore => {
                if let Some(Tab {
                    panel: Panel::NativeDesktop { view, .. },
                    ..
                }) = self.tabs.iter_mut().find(|tab| tab.id == id)
                {
                    let restore_tab = view
                        .window
                        .as_ref()
                        .is_some_and(|window| window.restore_tab);
                    if restore_tab {
                        view.window = None;
                        self.active = Some(id.into());
                        self.context
                            .send_viewport_cmd_to(viewport(id), egui::ViewportCommand::Close);
                    } else if let Some(window) = &mut view.window {
                        window.restore();
                    }
                    view.needs_focus = true;
                    if !restore_tab {
                        self.context.send_viewport_cmd_to(
                            viewport(id),
                            egui::ViewportCommand::Fullscreen(false),
                        );
                        self.context.send_viewport_cmd_to(
                            viewport(id),
                            egui::ViewportCommand::Maximized(false),
                        );
                    }
                }
            }
        }
        self.bind_rdp_repaint(id);
        self.context.request_repaint_of(egui::ViewportId::ROOT);
    }
    pub(super) fn bind_rdp_repaint(&self, id: &str) {
        if let Some(Tab {
            panel:
                Panel::NativeDesktop {
                    view,
                    session: Some(session),
                    ..
                },
            ..
        }) = self.tabs.iter().find(|tab| tab.id == id)
        {
            let ctx = self.context.clone();
            let target = if view.window.is_some() {
                viewport(id)
            } else {
                egui::ViewportId::ROOT
            };
            session.on_output(move || ctx.request_repaint_of(target));
            self.context.request_repaint_of(target);
        }
    }
    pub(super) fn rdp_windows(&mut self, ctx: &egui::Context) {
        let ids = self
            .tabs
            .iter()
            .filter_map(|tab| {
                matches!(&tab.panel,Panel::NativeDesktop{view,..}if view.window.is_some())
                    .then_some(tab.id.clone())
            })
            .collect::<Vec<_>>();
        let mut actions = Vec::new();
        for id in ids {
            let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
                continue;
            };
            let tab = &mut self.tabs[index];
            let Panel::NativeDesktop {
                connection, view, ..
            } = &mut tab.panel
            else {
                continue;
            };
            let state = view.window.as_mut().unwrap();
            let blocked = self.session_config.is_some();
            if state.blocked != blocked {
                state.blocked = blocked;
                ctx.request_repaint_of(viewport(&id));
            }
            let size = if connection.kind() == "ADB" {
                [640., 800.]
            } else {
                [
                    connection.number("screenWidth").max(640) as f32,
                    connection.number("screenHeight").max(480) as f32,
                ]
            };
            let mut builder = egui::ViewportBuilder::default()
                .with_title(&tab.title)
                .with_inner_size(size)
                .with_min_inner_size([320., 240.])
                .with_decorations(true)
                .with_taskbar(true)
                .with_fullscreen(state.fullscreen);
            // Omitting this on later frames makes eframe inherit the root window's icon.
            if connection.kind() == "RDP"
                && let Some(icon) = icon()
            {
                builder = builder.with_icon(icon);
            }
            if !ctx.show_viewport_app(viewport(&id), builder.clone()) {
                ctx.show_viewport_immediate(viewport(&id), builder, |ui, _| {
                    self.render_rdp_window(ui, &id, &mut actions);
                });
            }
        }
        self.apply_rdp_actions(actions);
    }
    fn apply_rdp_actions(&mut self, actions: Vec<(String, Option<Action>, Option<Intent>)>) {
        for (id, action, intent) in actions {
            if let Some(action) = action {
                self.rdp_action(&id, action);
            } else if let Some(intent) = intent {
                self.handle_intent(&id, intent);
            }
        }
    }
    pub(super) fn rdp_viewport_ui(&mut self, ui: &mut Ui) {
        let current = ui.ctx().viewport_id();
        let id = self.tabs.iter().find(|tab| {
            viewport(&tab.id) == current && matches!(&tab.panel, Panel::NativeDesktop { view, .. } if view.window.is_some())
        }).map(|tab| tab.id.clone());
        let Some(id) = id else {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        };
        let mut actions = Vec::new();
        self.render_rdp_window(ui, &id, &mut actions);
        self.apply_rdp_actions(actions);
    }
    fn render_rdp_window(
        &mut self,
        ui: &mut Ui,
        id: &str,
        actions: &mut Vec<(String, Option<Action>, Option<Intent>)>,
    ) {
        // Also cover embedded/immediate viewports whose callback has no separate
        // app logic tick. Closing takes precedence over modal UI and frame uploads.
        if ui.input(|input| input.viewport().close_requested()) {
            actions.push((id.to_owned(), Some(Action::Close), None));
            return;
        }
        let blocked = self.session_config.is_some();
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        if ui.ctx().embed_viewports() {
            Self::poll_desktop(tab, ui.ctx(), false);
        }
        if blocked {
            ui.disable();
        }
        let mut fullscreen = false;
        if let Panel::NativeDesktop {
            connection, view, ..
        } = &mut tab.panel
        {
            let state = view.window.as_mut().unwrap();
            state.focused = ui.input(|input| input.focused);
            let size = ui.max_rect().size();
            let now = ui.input(|input| input.time);
            if now > state.resize_after
                && !state.fullscreen
                && state
                    .last_size
                    .is_some_and(|last| (last - size).length() > 0.5)
            {
                state.scrollbars = true;
            }
            state.last_size = Some(size);

            if connection.kind() == "RDP"
                && state.initialized
                && ui.input(|input| input.viewport().maximized.unwrap_or(false))
                && !state.fullscreen
            {
                state.enter_fullscreen(now);
                actions.push((id.to_owned(), Some(Action::Fullscreen), None));
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Maximized(false));
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            }
            if !state.initialized {
                state.bar_until = ui.input(|input| input.time) + 5.;
                state.resize_after = now + 0.3;
                state.initialized = true;
            }
            fullscreen = state.fullscreen;
        }
        if let Some(intent) =
            panels::render_with_connections(ui, tab, &mut self.icons, &self.store.connections)
        {
            actions.push((id.to_owned(), None, Some(intent)));
        }
        if fullscreen
            && let Panel::NativeDesktop { view, .. } = &mut tab.panel
            && let Some(state) = &mut view.window
            && let Some(action) = toolbar::show(ui, &tab.title, state, &mut self.icons)
        {
            actions.push((id.to_owned(), Some(action), None));
        }
        if matches!(
            std::env::var("TOMATO_UI_PREVIEW").as_deref(),
            Ok("rdp" | "adb_window")
        ) && std::env::var_os("TOMATO_UI_CAPTURE").is_some()
            && let Some(image) = ui.input(|input| {
                input.events.iter().find_map(|event| {
                    if let egui::Event::Screenshot {
                        viewport_id, image, ..
                    } = event
                        && *viewport_id == viewport(id)
                    {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            })
        {
            ui.ctx()
                .data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-image"), image));
        }
    }
}
fn icon() -> Option<std::sync::Arc<egui::IconData>> {
    static ICON: std::sync::OnceLock<Option<std::sync::Arc<egui::IconData>>> =
        std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        let image = image::load_from_memory(include_bytes!("../../assets/images/rdp/mstsc.png"))
            .ok()?
            .thumbnail(32, 32)
            .into_rgba8();
        Some(std::sync::Arc::new(egui::IconData {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
        }))
    })
    .clone()
}
