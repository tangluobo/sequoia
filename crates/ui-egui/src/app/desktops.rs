use super::*;
pub(super) mod menu;
#[cfg(test)]
mod tests;

impl TomatoApp {
    pub(super) fn submit_vnc_password(&mut self, id: &str) {
        let Some((connection, password, remember)) = self
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .and_then(|tab| match &tab.panel {
                Panel::NativeDesktop {
                    connection,
                    session: Some(session),
                    view,
                } if connection.kind() == "VNC"
                    && session.status()
                        == tomato_services::remote_desktop::Status::PasswordRequired =>
                {
                    Some((connection.clone(), view.password.clone(), view.remember))
                }
                _ => None,
            })
        else {
            return;
        };
        let result = self.supplied_credentials(&connection, &password, remember);
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        if let Panel::NativeDesktop {
            connection,
            session: Some(session),
            view,
        } = &mut tab.panel
        {
            match result.and_then(|authenticated| {
                session.provide_password(password)?;
                Ok(authenticated)
            }) {
                Ok(authenticated) => {
                    if remember {
                        *connection = authenticated;
                    }
                    view.clear_vnc_password(&self.context);
                    tab.error = None;
                }
                Err(error) => tab.error = Some(format!("{error:#}")),
            }
        }
    }

    pub(super) fn connect_desktop(&mut self, id: &str) {
        let Some((connection, password, remember)) = self
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .and_then(|tab| match &tab.panel {
                Panel::NativeDesktop {
                    connection, view, ..
                } => Some((connection.clone(), view.password.clone(), view.remember)),
                _ => None,
            })
        else {
            return;
        };
        let authenticated = if password.is_empty() {
            Ok(connection)
        } else {
            self.supplied_credentials(&connection, &password, remember)
        };
        let result = authenticated.and_then(|connection| {
            let runtime = self
                .runtime
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("后台运行时不可用"))?;
            tomato_services::remote_desktop::DesktopSession::start(&connection, runtime.handle())
                .map(|session| (session, connection))
        });
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        match result {
            Ok((new_session, authenticated)) => {
                if let Panel::NativeDesktop {
                    connection,
                    session,
                    view,
                } = &mut tab.panel
                {
                    let repaint = self.context.clone();
                    let target = if view.window.is_some() {
                        rdp_windows::viewport(id)
                    } else {
                        egui::ViewportId::ROOT
                    };
                    new_session.on_output(move || repaint.request_repaint_of(target));
                    if session
                        .as_ref()
                        .is_some_and(|old| new_session.shares_adb_transport(old))
                    {
                        let mut current = connection.clone();
                        new_session.apply_adb_settings(&mut current);
                        if let Err(error) = new_session.reconfigure_adb(&current) {
                            tab.error = Some(format!("{error:#}"));
                            return;
                        }
                    }
                    // Remember controls persistence; an RDP retry must still retain
                    // the credentials supplied for this in-memory session.
                    if remember || connection.kind() == "RDP" {
                        *connection = authenticated;
                    }
                    *session = Some(new_session);
                    if connection.kind() == "VNC" {
                        view.clear_vnc_password(&self.context);
                        view.reset_vnc_input();
                    } else {
                        let window = view.window.take();
                        *view = Default::default();
                        view.window = window;
                    }
                    tab.error = None;
                }
            }
            Err(error) => tab.error = Some(format!("{error:#}")),
        }
    }
}
