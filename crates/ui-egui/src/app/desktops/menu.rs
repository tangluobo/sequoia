//! The actual VNC/ADB Java tab menus; dispatch outside the header paint closure.
use super::*;
#[derive(Clone, Copy)]
pub(in crate::app) enum Action {
    Reconnect,
    Disconnect,
    AndroidKey(u16),
    Fullscreen,
    IndependentWindow,
}
pub(in crate::app) fn show(ui: &mut Ui, tab: &Tab) -> Option<Action> {
    let Panel::NativeDesktop {
        connection,
        session,
        ..
    } = &tab.panel
    else {
        return None;
    };
    if !matches!(connection.kind(), "VNC" | "ADB") {
        return None;
    }
    let ready = session
        .as_ref()
        .is_some_and(|s| s.status() == tomato_services::remote_desktop::Status::Ready);
    let mut chosen = None;
    for (label, action) in [
        ("重新连接", Action::Reconnect),
        ("断开", Action::Disconnect),
    ] {
        if ui
            .add_enabled(
                !matches!(action, Action::Disconnect) || session.is_some(),
                crate::compact_menu::Item::new(label),
            )
            .clicked()
        {
            chosen = Some(action);
        }
    }
    if connection.kind() == "ADB" {
        crate::compact_menu::separator(ui);
        for (label, code) in [("返回", 4), ("Home", 3), ("最近任务", 187)] {
            if ui
                .add_enabled(ready, crate::compact_menu::Item::new(label))
                .clicked()
            {
                chosen = Some(Action::AndroidKey(code));
            }
        }
        crate::compact_menu::separator(ui);
    }
    let (label, window_action) = if connection.kind() == "ADB" {
        ("独立窗口", Action::IndependentWindow)
    } else {
        ("全屏", Action::Fullscreen)
    };
    if ui.add(crate::compact_menu::Item::new(label)).clicked() {
        chosen = Some(window_action);
    }
    if chosen.is_some() {
        ui.close();
    }
    chosen
}
impl TomatoApp {
    pub(in crate::app) fn desktop_menu_action(&mut self, id: &str, action: Action) {
        if matches!(action, Action::Reconnect) {
            if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id)
                && let Panel::NativeDesktop {
                    connection,
                    session,
                    view,
                } = &mut tab.panel
                && connection.kind() == "VNC"
            {
                *session = None;
                view.clear_vnc_password(&self.context);
                view.reset_vnc_input();
            }
            self.connect_desktop(id);
            return;
        }
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else {
            return;
        };
        let Panel::NativeDesktop {
            connection,
            session,
            view,
        } = &mut tab.panel
        else {
            return;
        };
        if !matches!(connection.kind(), "VNC" | "ADB") {
            return;
        }
        match action {
            Action::Disconnect => {
                *session = None;
                view.clear_vnc_password(&self.context);
                view.reset_vnc_input();
                tab.error = None;
            }
            Action::AndroidKey(code) if connection.kind() == "ADB" => {
                if let Some(active) = session.as_ref()
                    && let Err(error) =
                        active.send(tomato_services::remote_desktop::Input::AndroidKey(code))
                {
                    tab.error = Some(error.to_string());
                }
            }
            Action::IndependentWindow => self.toggle_adb_window(id),
            Action::Fullscreen if connection.kind() == "VNC" => {
                let fullscreen = self
                    .context
                    .input(|i| i.viewport().fullscreen.unwrap_or(false));
                self.context
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
            }
            _ => {}
        }
    }
}
