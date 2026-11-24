use super::*;
use ironrdp_cliprdr::backend::CliprdrBackend;
use ironrdp_cliprdr::pdu::{
    ClipboardGeneralCapabilityFlags as Caps, FormatDataRequest, FormatDataResponse, LockDataId,
};
use ironrdp_core::impl_as_any;

pub(super) struct Factory {
    pub inner: Box<dyn CliprdrBackendFactory + Send>,
    pub bridge: Arc<Bridge>,
}
impl CliprdrBackendFactory for Factory {
    fn build_cliprdr_backend(&self) -> Box<dyn CliprdrBackend> {
        self.bridge.begin_session();
        Box::new(Backend {
            inner: self.inner.build_cliprdr_backend(),
            bridge: self.bridge.clone(),
        })
    }
}
#[derive(Debug)]
pub(super) struct Backend {
    pub inner: Box<dyn CliprdrBackend>,
    pub bridge: Arc<Bridge>,
}
impl_as_any!(Backend);
impl Drop for Backend {
    fn drop(&mut self) {
        self.bridge.close();
    }
}
impl CliprdrBackend for Backend {
    fn temporary_directory(&self) -> &str {
        self.inner.temporary_directory()
    }
    fn client_capabilities(&self) -> Caps {
        self.inner.client_capabilities()
            | Caps::STREAM_FILECLIP_ENABLED
            | Caps::FILECLIP_NO_FILE_PATHS
            | Caps::CAN_LOCK_CLIPDATA
            | Caps::HUGE_FILE_SUPPORT_ENABLED
    }
    fn on_ready(&mut self) {
        self.inner.on_ready();
    }
    fn on_request_format_list(&mut self) {
        self.inner.on_request_format_list();
    }
    fn on_process_negotiated_capabilities(&mut self, capabilities: Caps) {
        self.bridge.file_enabled.store(
            capabilities.contains(Caps::STREAM_FILECLIP_ENABLED),
            Ordering::Release,
        );
        self.inner.on_process_negotiated_capabilities(capabilities);
    }
    fn on_remote_copy(&mut self, formats: &[ClipboardFormat]) {
        self.bridge.epoch.fetch_add(1, Ordering::AcqRel);
        if self.bridge.file_enabled.load(Ordering::Acquire) {
            if let Some(format) = formats.iter().find(|f| {
                f.name()
                    .is_some_and(|n| n.value() == "FileGroupDescriptorW")
            }) {
                self.bridge.publish(Publication::Clear);
                self.inner.on_remote_copy(&[]);
                let _ = self
                    .bridge
                    .send(ClipboardMessage::SendInitiatePaste(format.id()));
                return;
            }
        }
        self.bridge.publish(Publication::Clear);
        self.inner.on_remote_copy(formats);
    }
    fn on_remote_file_list(&mut self, files: &[FileDescriptor], lock: Option<u32>) {
        self.bridge.remote_files(files, lock);
        self.inner.on_remote_file_list(files, lock);
    }
    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        self.inner.on_format_data_request(request);
    }
    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        self.inner.on_format_data_response(response);
    }
    fn on_format_list_response(&mut self, ok: bool) {
        if !ok {
            self.bridge.last_local_sequence.store(0, Ordering::Release);
            self.bridge.local.lock().unwrap().take();
        }
        self.inner.on_format_list_response(ok);
    }
    fn on_file_contents_request(&mut self, request: FileContentsRequest) {
        let files = match request.data_id {
            Some(id) => self.bridge.locked.lock().unwrap().get(&id).cloned(),
            None => self.bridge.local.lock().unwrap().clone(),
        };
        if let Some(files) = files {
            source::queue_read(self.bridge.clone(), files, request);
        } else {
            let _ = self.bridge.send(ClipboardMessage::SendFileContentsResponse(
                FileContentsResponse::new_error(request.stream_id),
            ));
        }
    }
    fn on_file_contents_response(&mut self, response: FileContentsResponse<'_>) {
        self.bridge.response(response);
    }
    fn on_lock(&mut self, id: LockDataId) {
        if let Some(files) = self.bridge.local.lock().unwrap().clone() {
            let mut locks = self.bridge.locked.lock().unwrap();
            if locks.len() < 16 {
                locks.insert(id.0, files);
            }
        }
        self.inner.on_lock(id);
    }
    fn on_unlock(&mut self, id: LockDataId) {
        self.bridge.locked.lock().unwrap().remove(&id.0);
        self.inner.on_unlock(id);
    }
    fn on_outgoing_locks_cleared(&mut self, locks: &[LockDataId]) {
        self.bridge.retire_locks(locks);
        self.inner.on_outgoing_locks_cleared(locks);
    }
    fn on_outgoing_locks_expired(&mut self, locks: &[LockDataId]) {
        self.inner.on_outgoing_locks_expired(locks);
    }
    fn now_ms(&self) -> u64 {
        self.inner.now_ms()
    }
    fn elapsed_ms(&self, since: u64) -> u64 {
        self.inner.elapsed_ms(since)
    }
}
