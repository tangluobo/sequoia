//! Keep the data object's STA alive until Explorer ends its background copy.
use super::*;
use std::cell::Cell;
use windows::Win32::Foundation::E_UNEXPECTED;

#[derive(Default)]
pub(super) struct Operation {
    enabled: Cell<bool>,
    active: Cell<bool>,
    outcome: Cell<Option<(HRESULT, u32)>>,
}
impl Operation {
    pub(super) fn negotiated(&self) -> bool {
        self.running() || self.outcome.get().is_some()
    }
    pub(super) fn running(&self) -> bool {
        self.active.get()
    }
    pub(super) fn wait(&self, control: &Cancellation) -> Result<bool> {
        while self.running() {
            anyhow::ensure!(!control.0.load(Ordering::Acquire), "文件拖放已取消");
            unsafe {
                let mut message = MSG::default();
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    anyhow::ensure!(message.message != WM_QUIT, "文件拖放已取消");
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                if self.running() {
                    MsgWaitForMultipleObjectsEx(None, 50, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
                }
            }
        }
        let (result, effect) = self
            .outcome
            .get()
            .ok_or_else(|| windows::core::Error::from(E_UNEXPECTED))?;
        result.ok()?;
        Ok(effect & DROPEFFECT_COPY.0 != 0)
    }
}
impl IDataObjectAsyncCapability_Impl for data::Deferred_Impl {
    fn SetAsyncMode(&self, enabled: BOOL) -> windows::core::Result<()> {
        if self.operation.running() {
            return Err(E_UNEXPECTED.into());
        }
        self.operation.enabled.set(enabled.as_bool());
        Ok(())
    }
    fn GetAsyncMode(&self) -> windows::core::Result<BOOL> {
        Ok(self.operation.enabled.get().into())
    }
    fn StartOperation(&self, _: Ref<'_, IBindCtx>) -> windows::core::Result<()> {
        if !self.operation.enabled.get() || self.operation.running() {
            return Err(E_UNEXPECTED.into());
        }
        self.operation.outcome.set(None);
        self.operation.active.set(true);
        Ok(())
    }
    fn InOperation(&self) -> windows::core::Result<BOOL> {
        Ok(self.operation.running().into())
    }
    fn EndOperation(
        &self,
        result: HRESULT,
        _: Ref<'_, IBindCtx>,
        effects: u32,
    ) -> windows::core::Result<()> {
        if !self.operation.running() {
            return Err(E_UNEXPECTED.into());
        }
        self.operation.outcome.set(Some((result, effects)));
        self.operation.active.set(false);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::E_ABORT;
    use windows::core::Interface as _;

    #[test]
    fn asynchronous_completion_reports_copy_failure_and_cancellation() -> Result<()> {
        let _apartment = Apartment::new()?;
        for result in [HRESULT(0), E_ABORT, E_UNEXPECTED] {
            let files = vec![File::new(
                "empty.txt".into(),
                Some(0),
                || None,
                |_| unreachable!(),
            )];
            let descriptor = descriptor::Descriptor::new(&files)?;
            let deferred = data::Deferred::new(descriptor, files, Cancellation::new());
            let state = deferred.operation.clone();
            let object: IDataObject = deferred.into();
            let operation: IDataObjectAsyncCapability = object.cast()?;
            unsafe {
                assert!(!operation.GetAsyncMode()?.as_bool());
                operation.SetAsyncMode(true)?;
                operation.StartOperation(None)?;
                assert!(operation.InOperation()?.as_bool());
                assert!(operation.SetAsyncMode(false).is_err());
                operation.EndOperation(result, None, DROPEFFECT_COPY.0)?;
                assert!(!operation.InOperation()?.as_bool());
                assert!(
                    state.negotiated(),
                    "completion before DoDragDrop returns is still asynchronous"
                );
            }
            assert_eq!(state.wait(&Cancellation::new()).is_ok(), result.is_ok());
        }
        Ok(())
    }
}
