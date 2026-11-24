//! Source RdpPane's 420-pixel, five-second auto-hide control bar.
use super::*;
use crate::tabs::desktop::Window;
pub(super) fn show(
    ui: &mut Ui,
    title: &str,
    state: &mut Window,
    icons: &mut crate::icons::Icons,
) -> Option<Action> {
    let bounds = ui.max_rect();
    let now = ui.input(|input| input.time);
    let width = 420_f32.min(bounds.width());
    let left = (bounds.center().x - width / 2. + state.bar_x)
        .clamp(bounds.left(), (bounds.right() - width).max(bounds.left()));
    let rect = egui::Rect::from_min_size(egui::pos2(left, bounds.top()), egui::vec2(width, 32.));
    if ui.input(|input| {
        input
            .pointer
            .hover_pos()
            .is_some_and(|pos| pos.y <= bounds.top() + 2. || rect.contains(pos))
    }) {
        state.bar_until = now + 5.;
    }
    let visible = state.pinned || now < state.bar_until;
    if !visible {
        return None;
    }
    if !state.pinned {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(
                (state.bar_until - now).max(0.01),
            ));
    }
    let mut action = None;
    egui::Area::new(ui.id().with("rdp-control-bar"))
        .fixed_pos(rect.min)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            let (rect, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
            crate::native_dialog::gradient(
                ui,
                rect,
                Color32::from_rgb(61, 139, 210),
                Color32::from_rgb(7, 81, 154),
            );
            let at = |x: f32| {
                egui::Rect::from_min_size(rect.min + egui::vec2(x, 4.), egui::vec2(24., 24.))
            };
            for (x, label, tip, verb) in [
                (8., "pin", "固定控制栏", None),
                (38., "quality", "连接质量", None),
                (width - 92., "minimize", "最小化", Some(Action::Minimize)),
                (width - 62., "restore", "退出全屏", Some(Action::Restore)),
                (width - 32., "close", "关闭远程桌面", Some(Action::Close)),
            ] {
                let response = ui
                    .interact(at(x), ui.id().with(tip), egui::Sense::click())
                    .on_hover_text(tip);
                if response.hovered() || (x == 8. && state.pinned) {
                    ui.painter().rect_filled(
                        at(x),
                        3.,
                        if x == width - 32. {
                            Color32::from_rgb(196, 43, 28)
                        } else {
                            Color32::from_white_alpha(36)
                        },
                    );
                }
                if let Some(texture) =
                    icons.get(ui.ctx(), &format!("images/rdp/control-{label}.svg"))
                {
                    ui.painter().image(
                        texture,
                        at(x).shrink(2.),
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                        Color32::WHITE,
                    );
                }
                if response.clicked() {
                    if let Some(verb) = verb {
                        action = Some(verb);
                    } else if x == 8. {
                        state.pinned = !state.pinned;
                    } else {
                        state.quality = true;
                        state.alert_focus = true;
                    }
                }
            }
            let title_rect = egui::Rect::from_min_max(
                rect.min + egui::vec2(68., 0.),
                egui::pos2(rect.right() - 100., rect.bottom()),
            );
            let drag = ui.interact(
                title_rect,
                ui.id().with("rdp-title-drag"),
                egui::Sense::drag(),
            );
            if drag.dragged() {
                state.bar_x = (state.bar_x + ui.input(|input| input.pointer.delta().x)).clamp(
                    -(bounds.width() - width) / 2.,
                    (bounds.width() - width) / 2.,
                );
                state.bar_until = now + 5.;
            }
            ui.painter().with_clip_rect(title_rect).text(
                title_rect.center(),
                egui::Align2::CENTER_CENTER,
                title,
                egui::FontId::proportional(14.),
                Color32::WHITE,
            );
        });
    if state.quality {
        use crate::native_dialog::message::{self, Button, Kind, Message, Role};
        if message::show(
            ui.ctx(),
            Message {
                id: egui::ViewportId::from_hash_of((ui.id(), "rdp-quality")),
                owner: ui.input(|input| input.viewport().inner_rect),
                title: "远程桌面连接",
                header: None,
                content: "与远程计算机连接的质量非常好。",
                kind: Kind::Information,
                buttons: &[Button {
                    label: "确定",
                    role: Role::Ok,
                }],
                active: true,
            },
            &mut state.alert_focus,
            icons,
            |_, _| {},
        )
        .is_some()
        {
            state.quality = false;
        }
    }
    action
}
