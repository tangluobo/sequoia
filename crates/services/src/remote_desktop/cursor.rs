//! Java FxRdpDisplay's bounded movement correlation for legacy VM software cursors.
use super::{Pointer, Region, surface::Surface};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
#[derive(Default)]
pub(super) struct CursorMode {
    eligible: bool,
    hidden_seen: bool,
    position_suppressed: bool,
    suppressed: bool,
    evidence: u8,
    position: Option<(u16, u16)>,
    last_probe: Option<Instant>,
    last_probe_position: Option<(u16, u16)>,
    probes: VecDeque<Probe>,
}
struct Probe {
    at: Instant,
    old: (Region, Vec<u32>),
    new: (Region, Vec<u32>),
}
impl CursorMode {
    pub fn shape(&mut self, pointer: Option<&Pointer>, hidden: bool) {
        self.hidden_seen |= hidden;
        self.eligible =
            !hidden && pointer.is_some_and(|p| p.size.into_iter().all(|v| v > 0 && v <= 24));
        self.position_suppressed = false;
        if hidden || !self.eligible {
            self.reset();
        }
    }
    fn reset(&mut self) {
        self.probes.clear();
        self.evidence = 0;
        self.suppressed = false;
        self.last_probe = None;
        self.last_probe_position = None;
    }
    pub fn button(&mut self, x: u16, y: u16) {
        self.position = Some((x, y));
        self.reset();
    }
    pub fn server_position(&mut self) {
        if self.hidden_seen && self.eligible {
            self.position_suppressed = true;
            self.reset();
        }
    }
    pub fn hidden(&mut self, now: Instant) -> bool {
        self.expire(now);
        self.position_suppressed || self.suppressed
    }
    pub fn motion(&mut self, surface: &Surface, x: u16, y: u16, now: Instant) {
        self.expire(now);
        let previous = self.position.replace((x, y));
        if !self.eligible
            || self.position_suppressed
            || self
                .last_probe
                .is_some_and(|last| now.duration_since(last) < Duration::from_millis(40))
        {
            return;
        }
        let Some((old_x, old_y)) = previous else {
            return;
        };
        if self.last_probe_position.is_some_and(|(px, py)| {
            i64::from(px).abs_diff(i64::from(x)).pow(2)
                + i64::from(py).abs_diff(i64::from(y)).pow(2)
                < 9
        }) {
            return;
        }
        let (Some(old), Some(new)) = (
            surface.cursor_patch(old_x, old_y),
            surface.cursor_patch(x, y),
        ) else {
            return;
        };
        if self.probes.len() >= 8 {
            self.probes.pop_front();
        }
        self.probes.push_back(Probe { at: now, old, new });
        self.last_probe = Some(now);
        self.last_probe_position = Some((x, y));
    }
    pub fn frame(&mut self, surface: &Surface, now: Instant) {
        self.expire(now);
        let matches = |patch: &(Region, Vec<u32>)| {
            surface
                .cursor_patch_changes(patch.0, &patch.1)
                .is_some_and(|n| n >= 6 && n <= 24.max(patch.1.len() * 45 / 100))
        };
        if let Some(index) = self
            .probes
            .iter()
            .rposition(|p| matches(&p.old) && matches(&p.new))
        {
            self.probes.drain(..=index);
            self.evidence = (self.evidence + 1).min(5);
            if self.evidence >= 3 {
                self.suppressed = true;
            }
        }
    }
    fn expire(&mut self, now: Instant) {
        while self
            .probes
            .front()
            .is_some_and(|p| now.duration_since(p.at) >= Duration::from_millis(400))
        {
            self.probes.pop_front();
            self.evidence = self.evidence.saturating_sub(1);
        }
        if self.evidence == 0 {
            self.suppressed = false;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn surface(x: u16) -> Surface {
        let mut s = Surface::default();
        let mut pixels = vec![u32::from_ne_bytes([30, 40, 50, 255]); 512 * 96];
        for y in 40..43 {
            for col in usize::from(x)..usize::from(x) + 3 {
                pixels[y * 512 + col] = u32::from_ne_bytes([255, 255, 255, 255]);
            }
        }
        s.apply_rgba(
            512,
            96,
            Region {
                x: 0,
                y: 0,
                width: 512,
                height: 96,
            },
            pixels,
        )
        .unwrap();
        s
    }
    fn pointer(size: u16) -> Pointer {
        Pointer {
            size: [size, size],
            hotspot: [0, 0],
            rgba: vec![255; usize::from(size) * usize::from(size) * 4],
        }
    }
    #[test]
    fn vm_frame_cursor_requires_three_correlated_moves_and_recovers_after_misses() {
        let now = Instant::now();
        let mut mode = CursorMode::default();
        mode.shape(Some(&pointer(24)), false);
        let mut canvas = surface(80);
        mode.motion(&canvas, 80, 40, now);
        for (index, x) in [160, 240, 320].into_iter().enumerate() {
            let at = now + Duration::from_millis(50 * (index as u64 + 1));
            mode.motion(&canvas, x, 40, at);
            canvas = surface(x);
            mode.frame(&canvas, at + Duration::from_millis(15));
            assert_eq!(mode.hidden(at + Duration::from_millis(16)), index == 2);
        }
        for (index, x) in [370, 420, 470].into_iter().enumerate() {
            mode.motion(
                &canvas,
                x,
                40,
                now + Duration::from_millis(200 + index as u64 * 50),
            );
        }
        assert!(!mode.hidden(now + Duration::from_millis(800)));
    }
    #[test]
    fn normal_cursor_scene_changes_and_default_position_echo_do_not_hide_the_pointer() {
        let now = Instant::now();
        let mut mode = CursorMode::default();
        mode.shape(Some(&pointer(32)), false);
        mode.server_position();
        for x in [80, 160, 240, 320] {
            mode.motion(&surface(x), x, 40, now);
            mode.frame(&surface(x + 5), now);
        }
        assert!(!mode.hidden(now));
        assert!(mode.probes.is_empty());
        mode.shape(Some(&pointer(24)), false);
        mode.motion(&surface(80), 80, 40, now);
        let mut scene = Surface::default();
        scene
            .apply_rgba(
                512,
                96,
                Region {
                    x: 0,
                    y: 0,
                    width: 512,
                    height: 96,
                },
                vec![0; 512 * 96],
            )
            .unwrap();
        for (index, x) in [160, 240, 320].into_iter().enumerate() {
            let at = now + Duration::from_millis(50 * (index as u64 + 1));
            mode.motion(&surface(80), x, 40, at);
            mode.frame(&scene, at);
        }
        assert!(!mode.hidden(now + Duration::from_millis(160)));
    }
    #[test]
    fn only_legacy_cursor_after_hidden_transition_uses_position_suppression() {
        let mut mode = CursorMode::default();
        let now = Instant::now();
        mode.shape(Some(&pointer(24)), false);
        mode.server_position();
        assert!(!mode.hidden(now));
        mode.shape(Some(&pointer(24)), true);
        assert!(!mode.eligible);
        mode.motion(&surface(80), 80, 40, now);
        mode.motion(&surface(160), 160, 40, now + Duration::from_millis(50));
        assert!(mode.probes.is_empty());
        mode.shape(Some(&pointer(24)), false);
        mode.server_position();
        assert!(mode.hidden(now));
        mode.shape(Some(&pointer(32)), false);
        mode.server_position();
        assert!(!mode.hidden(now));
        mode.shape(None, false);
        assert!(!mode.hidden(now));
    }
}
