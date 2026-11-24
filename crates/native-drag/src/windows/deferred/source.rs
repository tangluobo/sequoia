//! Explicit OLE source: release drops, Escape/session cancellation cancels.
use super::*;
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};

#[implement(IDropSource, Agile = false)]
pub(super) struct Source {
    cancel: Cancellation,
    progress: Arc<Progress>,
}
impl Source {
    pub(super) fn new(cancel: Cancellation, progress: Arc<Progress>) -> Self {
        Self { cancel, progress }
    }
}
impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        self.progress.queries.fetch_add(1, Ordering::Relaxed);
        self.progress.keys.store(keys.0, Ordering::Relaxed);
        if escape.as_bool() || self.cancel.0.load(Ordering::Acquire) {
            DRAGDROP_S_CANCEL
        } else if keys.0 & MK_LBUTTON.0 == 0 {
            self.cancel.1.store(true, Ordering::Release);
            DRAGDROP_S_DROP
        } else {
            HRESULT(0)
        }
    }
    fn GiveFeedback(&self, effect: DROPEFFECT) -> HRESULT {
        self.progress.feedback.fetch_add(1, Ordering::Relaxed);
        self.progress.effect.store(effect.0, Ordering::Relaxed);
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ole_source_distinguishes_release_escape_and_session_cancellation() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Progress::default());
        let source: IDropSource = Source::new(
            Cancellation(cancelled.clone(), Arc::new(AtomicBool::new(false))),
            progress.clone(),
        )
        .into();
        unsafe {
            assert_eq!(source.QueryContinueDrag(false, MK_LBUTTON), HRESULT(0));
            assert_eq!(
                source.QueryContinueDrag(false, MODIFIERKEYS_FLAGS(0)),
                DRAGDROP_S_DROP
            );
            assert_eq!(
                source.QueryContinueDrag(true, MK_LBUTTON),
                DRAGDROP_S_CANCEL
            );
            cancelled.store(true, Ordering::Release);
            assert_eq!(
                source.QueryContinueDrag(false, MK_LBUTTON),
                DRAGDROP_S_CANCEL
            );
            assert_eq!(
                source.GiveFeedback(DROPEFFECT_COPY),
                DRAGDROP_S_USEDEFAULTCURSORS
            );
        }
        assert_eq!(progress.queries.load(Ordering::Relaxed), 4);
        assert_eq!(progress.feedback.load(Ordering::Relaxed), 1);
    }
}
