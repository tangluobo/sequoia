mod window;
pub use window::Window;
#[derive(Default)]
pub struct DesktopView {
    pub window: Option<Box<Window>>,
    pub texture: Option<egui::TextureHandle>,
    pub(crate) rdp_texture: Option<crate::gpu_rdp::Texture>,
    pub(crate) video: Option<crate::gpu_video::Video>,
    pub native_pointer: Option<egui::CustomCursorImage>,
    pub pointer_revision: u64,
    pub size: [u16; 2],
    pub pressed: Vec<(u8, bool)>,
    pub printable_pressed: Vec<(egui::Key, u16)>,
    pub modifiers: egui::Modifiers,
    pub position: Option<(u16, u16)>,
    pub buttons: u8,
    pub focused: bool,
    pub pending: std::collections::VecDeque<tomato_services::remote_desktop::Input>,
    pub password: String,
    pub composition: String,
    pub needs_focus: bool,
    pub auth_focused: bool,
    pub vnc_password_editor: Option<Box<super::text_state::Editor>>,
    pub protocol_clipboard: bool,
    pub remember: bool,
    pub fullscreen_initialized: bool,
}

impl DesktopView {
    pub fn clear_vnc_password(&mut self, ctx: &egui::Context) {
        if let Some(editor) = self.vnc_password_editor.take() {
            editor.release(ctx);
        }
        self.password = String::new();
        self.remember = false;
        self.auth_focused = false;
    }
    pub(crate) fn texture_id(&self) -> Option<egui::TextureId> {
        self.rdp_texture
            .as_ref()
            .map(|texture| texture.id())
            .or_else(|| self.texture.as_ref().map(|texture| texture.id()))
    }
    pub fn queue_inputs(
        &mut self,
        inputs: impl IntoIterator<Item = tomato_services::remote_desktop::Input>,
    ) {
        use tomato_services::remote_desktop::Input;
        let motion = |input: &Input| {
            matches!(
                input,
                Input::Pointer {
                    button: None,
                    wheel: 0,
                    ..
                }
            )
        };
        for input in inputs {
            if motion(&input)
                && let Some(last) = self.pending.back_mut()
                && motion(last)
            {
                *last = input;
            } else {
                self.pending.push_back(input);
            }
        }
    }
    pub fn update_modifiers(
        &mut self,
        next: egui::Modifiers,
        send: &mut impl FnMut(tomato_services::remote_desktop::Input),
    ) {
        use tomato_services::remote_desktop::Input;
        for (before, after, code, extended) in [
            (self.modifiers.shift, next.shift, 0x2a, false),
            (self.modifiers.ctrl, next.ctrl, 0x1d, false),
            (self.modifiers.alt, next.alt, 0x38, false),
            (self.modifiers.mac_cmd, next.mac_cmd, 0x5b, true),
        ] {
            if before != after {
                send(Input::ScanCode {
                    code,
                    extended,
                    down: after,
                });
            }
        }
        self.modifiers = next;
    }
    pub fn release_inputs(
        &mut self,
        send: &mut impl FnMut(tomato_services::remote_desktop::Input),
    ) {
        use tomato_services::remote_desktop::Input;
        self.composition.clear();
        for (_, value) in self.printable_pressed.drain(..) {
            send(Input::Unicode { value, down: false });
        }
        for (code, extended) in self.pressed.drain(..) {
            send(Input::ScanCode {
                code,
                extended,
                down: false,
            });
        }
        self.update_modifiers(Default::default(), send);
        let (x, y) = self.position.unwrap_or_default();
        for button in 1..=3 {
            if self.buttons & (1 << button) != 0 {
                send(Input::Pointer {
                    x,
                    y,
                    button: Some(button),
                    down: false,
                    wheel: 0,
                });
            }
        }
        self.buttons = 0;
        self.focused = false;
    }
    /// A VNC disconnect releases input state but keeps the last desktop visible,
    /// just as the original ImageView does until the next successful update.
    pub fn reset_vnc_input(&mut self) {
        let texture = self.texture.take();
        let size = self.size;
        let fullscreen_initialized = self.fullscreen_initialized;
        *self = Self {
            texture,
            size,
            fullscreen_initialized,
            ..Default::default()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tomato_services::remote_desktop::Input;
    #[test]
    fn movement_backpressure_keeps_latest_position_without_crossing_input_barriers() {
        let mut view = DesktopView::default();
        let motion = |x| Input::Pointer {
            x,
            y: 5,
            button: None,
            down: false,
            wheel: 0,
        };
        view.queue_inputs((0..10000).map(motion));
        assert_eq!(view.pending.len(), 1);
        view.queue_inputs([
            Input::Pointer {
                x: 9999,
                y: 5,
                button: Some(1),
                down: true,
                wheel: 0,
            },
            motion(3),
            motion(4),
            Input::Pointer {
                x: 4,
                y: 5,
                button: None,
                down: false,
                wheel: 120,
            },
            motion(8),
        ]);
        assert_eq!(view.pending.len(), 5);
        assert!(matches!(
            view.pending[0],
            Input::Pointer {
                x: 9999,
                button: None,
                wheel: 0,
                ..
            }
        ));
        assert!(matches!(
            view.pending[1],
            Input::Pointer {
                button: Some(1),
                down: true,
                ..
            }
        ));
        assert!(matches!(
            view.pending[2],
            Input::Pointer { x: 4, wheel: 0, .. }
        ));
        assert!(matches!(view.pending[3], Input::Pointer { wheel: 120, .. }));
        assert!(matches!(
            view.pending[4],
            Input::Pointer { x: 8, wheel: 0, .. }
        ));
    }
    #[test]
    fn leaving_a_desktop_releases_printable_shortcut_modifier_and_pointer_state() {
        let mut view = DesktopView {
            printable_pressed: vec![(egui::Key::A, 65)],
            pressed: vec![(0x48, true)],
            modifiers: egui::Modifiers::CTRL,
            position: Some((25, 30)),
            buttons: (1 << 1) | (1 << 3),
            focused: true,
            composition: "draft".into(),
            ..Default::default()
        };
        let mut result = Vec::new();
        view.release_inputs(&mut |input| result.push(input));
        assert!(view.printable_pressed.is_empty() && view.pressed.is_empty());
        assert!(
            view.composition.is_empty()
                && !view.focused
                && view.buttons == 0
                && view.modifiers == Default::default()
        );
        assert_eq!(result.len(), 5);
        assert!(matches!(
            result[0],
            Input::Unicode {
                value: 65,
                down: false
            }
        ));
        assert!(matches!(
            result[1],
            Input::ScanCode {
                code: 0x48,
                extended: true,
                down: false
            }
        ));
        assert!(matches!(
            result[2],
            Input::ScanCode {
                code: 0x1d,
                down: false,
                ..
            }
        ));
        assert!(matches!(
            result[3],
            Input::Pointer {
                x: 25,
                y: 30,
                button: Some(1),
                down: false,
                ..
            }
        ));
        assert!(matches!(
            result[4],
            Input::Pointer {
                button: Some(3),
                down: false,
                ..
            }
        ));
    }
}
