//! Java TerminalView uses a 500 ms on/off block, reset by keyboard/IME input.
//! The view requests only the next edge; hidden tabs never own a ticking task.
#[derive(Default)]
pub struct CursorBlink {
    epoch: Option<f64>,
    stopped: Option<bool>,
}
impl CursorBlink {
    pub fn reset(&mut self, now: f64) {
        self.epoch = Some(now);
        self.stopped = None;
    }
    pub fn sample(&mut self, now: f64, running: bool) -> (bool, Option<std::time::Duration>) {
        if running && self.stopped.take().is_some() {
            self.epoch = Some(now);
        }
        let epoch = *self.epoch.get_or_insert(now);
        let elapsed = (now - epoch).max(0.);
        let visible = ((elapsed / 0.5).floor() as u64).is_multiple_of(2);
        if !running {
            return (*self.stopped.get_or_insert(visible), None);
        }
        (
            visible,
            Some(std::time::Duration::from_secs_f64(
                (0.5 - elapsed.rem_euclid(0.5)).max(0.001),
            )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idle_cursor_changes_at_half_seconds_and_input_restarts_the_visible_phase() {
        let mut blink = CursorBlink::default();
        assert_eq!(
            blink.sample(10., true),
            (true, Some(std::time::Duration::from_millis(500)))
        );
        assert!(blink.sample(10.499, true).0);
        assert!(!blink.sample(10.5, true).0);
        assert!(blink.sample(11., true).0);
        blink.reset(11.75);
        assert!(blink.sample(11.75, true).0);
        assert!(!blink.sample(12.25, true).0);
        // No samples or background timer are needed while a tab is hidden.
        assert!(blink.sample(14.8, true).0);
        assert!(!blink.sample(15.3, true).0);
    }
    #[test]
    fn disconnected_cursor_freezes_and_reconnect_starts_visible() {
        let mut blink = CursorBlink::default();
        blink.sample(0., true);
        assert_eq!(blink.sample(0.6, false), (false, None));
        assert_eq!(blink.sample(1.1, false), (false, None));
        assert!(blink.sample(2., true).0);
        assert!(!blink.sample(2.5, true).0);
    }
}
