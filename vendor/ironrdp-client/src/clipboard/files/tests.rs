use super::*;
use ironrdp_cliprdr::backend::CliprdrBackend;
use ironrdp_cliprdr::pdu::{
    ClipboardGeneralCapabilityFlags, FormatDataRequest, FormatDataResponse, LockDataId,
};
use ironrdp_cliprdr::{Cliprdr, CliprdrClient, CliprdrServer, Role};
use ironrdp_core::impl_as_any;
use ironrdp_svc::{SvcMessage, SvcProcessor};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Instant;

#[derive(Debug)]
struct HostBackend(backend::Backend);
impl_as_any!(HostBackend);
impl CliprdrBackend for HostBackend {
    fn temporary_directory(&self) -> &str {
        self.0.temporary_directory()
    }
    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        self.0.client_capabilities()
    }
    fn on_ready(&mut self) {
        self.0.on_ready();
    }
    fn on_request_format_list(&mut self) {
        self.0.on_request_format_list();
    }
    fn on_remote_copy(&mut self, formats: &[ClipboardFormat]) {
        self.0.on_remote_copy(formats);
    }
    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        self.0.on_format_data_request(request);
    }
    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        self.0.on_format_data_response(response);
    }
    fn on_process_negotiated_capabilities(&mut self, caps: ClipboardGeneralCapabilityFlags) {
        self.0.on_process_negotiated_capabilities(caps);
    }
    fn on_remote_file_list(&mut self, files: &[FileDescriptor], lock: Option<u32>) {
        self.0.on_remote_file_list(files, lock);
    }
    fn on_file_contents_request(&mut self, request: FileContentsRequest) {
        // The owned remote host has its own file system. Serve it immediately
        // so the client's shared file worker is never also its simulated host.
        let files = match request.data_id {
            Some(id) => self.0.bridge.locked.lock().unwrap().get(&id).cloned(),
            None => self.0.bridge.local.lock().unwrap().clone(),
        };
        let response = files
            .and_then(|files| source::serve(&files, &request).ok())
            .unwrap_or_else(|| FileContentsResponse::new_error(request.stream_id));
        self.0
            .bridge
            .send(ClipboardMessage::SendFileContentsResponse(response))
            .unwrap();
    }
    fn on_file_contents_response(&mut self, response: FileContentsResponse<'_>) {
        self.0.on_file_contents_response(response);
    }
    fn on_lock(&mut self, id: LockDataId) {
        self.0.on_lock(id);
    }
    fn on_unlock(&mut self, id: LockDataId) {
        self.0.on_unlock(id);
    }
}

fn backend(bridge: Arc<Bridge>) -> backend::Backend {
    backend::Backend {
        inner: ironrdp_cliprdr_native::StubClipboard::new()
            .backend_factory()
            .build_cliprdr_backend(),
        bridge,
    }
}
fn command<R: Role>(channel: &mut Cliprdr<R>, event: ClipboardMessage) -> Vec<SvcMessage> {
    match event {
        ClipboardMessage::SendInitiateCopy(formats) => {
            channel.initiate_copy(&formats).unwrap().into()
        }
        ClipboardMessage::SendInitiateFileCopy(files) => {
            channel.initiate_file_copy(files).unwrap().into()
        }
        ClipboardMessage::SendInitiatePaste(format) => {
            channel.initiate_paste(format).unwrap().into()
        }
        ClipboardMessage::SendFileContentsRequest(request) => {
            channel.request_file_contents(request).unwrap().into()
        }
        ClipboardMessage::SendFileContentsResponse(response) => {
            channel.submit_file_contents(response).unwrap().into()
        }
        ClipboardMessage::SendFormatData(response) => {
            channel.submit_format_data(response).unwrap().into()
        }
        ClipboardMessage::Error(error) => panic!("Owned clipboard backend: {error}"),
    }
}

struct Pair {
    client: CliprdrClient,
    server: CliprdrServer,
    client_bridge: Arc<Bridge>,
    server_bridge: Arc<Bridge>,
    client_events: tokio::sync::mpsc::UnboundedReceiver<RdpInputEvent>,
    server_events: tokio::sync::mpsc::UnboundedReceiver<RdpInputEvent>,
    queue: VecDeque<(bool, SvcMessage)>,
    sockets: (TcpStream, TcpStream),
    max_wire_payload: usize,
}
impl Pair {
    fn new() -> Self {
        Self::start(None)
    }
    fn start(initial_files: Option<Files>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let first = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let second = listener.accept().unwrap().0;
        for socket in [&first, &second] {
            socket.set_nodelay(true).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
        }
        let (tx, client_events) = tokio::sync::mpsc::unbounded_channel::<RdpInputEvent>();
        let client_bridge = Bridge::new(tx.into());
        let (tx, server_events) = tokio::sync::mpsc::unbounded_channel::<RdpInputEvent>();
        let server_bridge = Bridge::new(tx.into());
        let client = CliprdrClient::new(Box::new(backend(client_bridge.clone())));
        let mut server = CliprdrServer::new(Box::new(HostBackend(backend(server_bridge.clone()))));
        let queue = server
            .start()
            .unwrap()
            .into_iter()
            .map(|m| (false, m))
            .collect();
        let mut pair = Self {
            client,
            server,
            client_bridge,
            server_bridge,
            client_events,
            server_events,
            queue,
            sockets: (first, second),
            max_wire_payload: 0,
        };
        pair.pump();
        if let Some(files) = initial_files {
            pair.client_bridge.offer_local(files).unwrap();
        } else {
            pair.queue.extend(
                Vec::<SvcMessage>::from(pair.client.initiate_copy(&[]).unwrap())
                    .into_iter()
                    .map(|m| (true, m)),
            );
        }
        pair.pump();
        assert!(pair.server_bridge.file_enabled.load(Ordering::Acquire));
        assert!(pair.client_bridge.file_enabled.load(Ordering::Acquire));
        pair
    }
    fn wire(&mut self, from_client: bool, message: SvcMessage) -> Vec<u8> {
        let bytes = message.encode_unframed_pdu().unwrap();
        self.max_wire_payload = self.max_wire_payload.max(bytes.len());
        let (writer, reader) = if from_client {
            (&self.sockets.0, &self.sockets.1)
        } else {
            (&self.sockets.1, &self.sockets.0)
        };
        let mut writer = writer.try_clone().unwrap();
        let mut reader = reader.try_clone().unwrap();
        std::thread::scope(|scope| {
            let send = scope.spawn(move || {
                writer
                    .write_all(&(bytes.len() as u32).to_le_bytes())
                    .unwrap();
                writer.write_all(&bytes).unwrap();
            });
            let mut length = [0; 4];
            reader.read_exact(&mut length).unwrap();
            let length = u32::from_le_bytes(length) as usize;
            assert!(length <= CHUNK + 100_000);
            let mut received = vec![0; length];
            reader.read_exact(&mut received).unwrap();
            send.join().unwrap();
            received
        })
    }
    fn pump(&mut self) {
        for _ in 0..256 {
            while let Ok(RdpInputEvent::Clipboard(event)) = self.client_events.try_recv() {
                self.queue.extend(
                    command(&mut self.client, event)
                        .into_iter()
                        .map(|m| (true, m)),
                );
            }
            while let Ok(RdpInputEvent::Clipboard(event)) = self.server_events.try_recv() {
                self.queue.extend(
                    command(&mut self.server, event)
                        .into_iter()
                        .map(|m| (false, m)),
                );
            }
            let Some((from_client, message)) = self.queue.pop_front() else {
                return;
            };
            let bytes = self.wire(from_client, message);
            let responses = if from_client {
                self.server.process(&bytes).unwrap()
            } else {
                self.client.process(&bytes).unwrap()
            };
            self.queue
                .extend(responses.into_iter().map(|m| (!from_client, m)));
        }
        panic!("Clipboard protocol did not quiesce");
    }
}
struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "tomato-rdp-files-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
use std::time::SystemTime;
impl Drop for Directory {
    fn drop(&mut self) {
        let base = std::env::temp_dir().canonicalize().unwrap();
        if self
            .0
            .canonicalize()
            .is_ok_and(|p| p.starts_with(&base) && p != base)
        {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
fn offer(bridge: &Bridge) -> Option<Arc<Offer>> {
    match bridge.publication.lock().unwrap().take() {
        Some(Publication::Files(offer)) => Some(offer),
        _ => None,
    }
}

pub(super) struct NativePeer {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for NativePeer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
pub(super) fn native_remote_offer(files: Files) -> (Arc<Offer>, NativePeer) {
    let mut pair = Pair::new();
    pair.server_bridge.offer_local(files).unwrap();
    pair.pump();
    let offered = offer(&pair.client_bridge).expect("Native receiver needs the actual wire offer");
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let thread = std::thread::spawn(move || {
        while !stopping.load(Ordering::Acquire) {
            pair.pump();
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    (
        offered,
        NativePeer {
            stop,
            thread: Some(thread),
        },
    )
}
#[test]
fn tomato_file_clipboard_can_initialize_with_files_already_copied() {
    let directory = Directory::new();
    let path = directory.0.join("连接前已复制-中文.bin");
    let expected: Vec<u8> = (0..CHUNK + 91).map(|i| (i % 251) as u8).collect();
    std::fs::write(&path, &expected).unwrap();
    let files = Arc::new(source::local_entries(&[path.to_string_lossy().into_owned()]).unwrap());
    let mut pair = Pair::start(Some(files));
    let received = offer(&pair.server_bridge)
        .expect("Initial file list must complete clipboard initialization");
    let source = received.files[0].source.clone();
    let reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        source.open().unwrap().read_to_end(&mut data).unwrap();
        data
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !reader.is_finished() {
        assert!(
            Instant::now() < deadline,
            "Initial native file copy stalled"
        );
        pair.pump();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(reader.join().unwrap(), expected);
}
#[test]
fn tomato_file_clipboard_streams_real_files_and_relays_between_remote_sessions() {
    let directory = Directory::new();
    std::fs::create_dir(directory.0.join("空目录")).unwrap();
    let data: Vec<u8> = (0..CHUNK * 2 + 73).map(|i| (i % 251) as u8).collect();
    std::fs::write(directory.0.join("中文.bin"), &data).unwrap();
    let files =
        Arc::new(source::local_entries(&[directory.0.to_string_lossy().into_owned()]).unwrap());
    assert_eq!(files.len(), 3);
    assert!(files.iter().any(|entry| {
        entry.descriptor.name == "空目录"
            && entry
                .descriptor
                .attributes
                .unwrap()
                .contains(ironrdp_cliprdr::pdu::ClipboardFileAttributes::DIRECTORY)
    }));
    let mut first = Pair::new();
    first.server_bridge.offer_local(files).unwrap();
    first.pump();
    let remote = offer(&first.client_bridge)
        .expect("Remote file metadata reached the actual client backend");
    assert!(
        first.client_bridge.pending.lock().unwrap().is_empty(),
        "Copy must not eagerly download file bytes"
    );
    let mut second = Pair::new();
    second
        .client_bridge
        .offer_local(remote.files.clone())
        .unwrap();
    second.pump();
    let target =
        offer(&second.server_bridge).expect("Second remote host received the relayed file offer");
    let file = target
        .files
        .iter()
        .find(|entry| entry.descriptor.name == "中文.bin")
        .unwrap()
        .source
        .clone();
    let reader = std::thread::spawn(move || {
        let mut reader = file.open().unwrap();
        let mut data = Vec::new();
        reader.read_to_end(&mut data).unwrap();
        data
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !reader.is_finished() {
        assert!(
            Instant::now() < deadline,
            "Remote-to-remote streaming stalled"
        );
        first.pump();
        second.pump();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(reader.join().unwrap(), data);
    assert!(first.max_wire_payload <= CHUNK + 100);
    assert!(second.max_wire_payload <= CHUNK + 100);
    assert!(first.client_bridge.pending.lock().unwrap().is_empty());
    assert!(second.server_bridge.pending.lock().unwrap().is_empty());
}

#[test]
fn tomato_file_clipboard_disconnect_wakes_waiting_readers() {
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel::<RdpInputEvent>();
    let bridge = Bridge::new(tx.into());
    bridge.remote_locks.lock().unwrap().insert(7, 0);
    let source = source::Source::Remote(source::Remote {
        bridge: Arc::downgrade(&bridge),
        index: 0,
        lock: Some(7),
        epoch: 0,
        size: Some(100),
    });
    let reader = std::thread::spawn(move || source.read(0, 100));
    let deadline = Instant::now() + Duration::from_secs(2);
    while events.try_recv().is_err() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let started = Instant::now();
    bridge.close();
    assert!(reader.join().unwrap().is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(bridge.pending.lock().unwrap().is_empty());
}

#[test]
fn tomato_file_clipboard_expired_snapshot_cannot_read_the_replacement_file_list() {
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel::<RdpInputEvent>();
    let bridge = Bridge::new(tx.into());
    bridge.remote_locks.lock().unwrap().insert(7, 0);
    bridge.retire_locks(&[LockDataId(7)]);
    assert!(bridge.request(0, Some(7), 0, 0, 100, false).is_err());
    assert!(
        events.try_recv().is_err(),
        "Expired streams must not emit requests for the current clipboard"
    );
    assert!(bridge.pending.lock().unwrap().is_empty());
}

#[test]
fn tomato_file_clipboard_changed_while_waiting_rejects_late_unlocked_bytes() {
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel::<RdpInputEvent>();
    let bridge = Bridge::new(tx.into());
    let requester = bridge.clone();
    let reader = std::thread::spawn(move || requester.request(0, None, 0, 0, 3, false));
    let deadline = Instant::now() + Duration::from_secs(2);
    let request = loop {
        if let Ok(RdpInputEvent::Clipboard(ClipboardMessage::SendFileContentsRequest(request))) =
            events.try_recv()
        {
            break request;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    };
    bridge.epoch.fetch_add(1, Ordering::AcqRel);
    bridge.response(FileContentsResponse::new_data_response(
        request.stream_id,
        vec![1, 2, 3],
    ));
    assert!(reader.join().unwrap().is_err());
    assert!(bridge.pending.lock().unwrap().is_empty());
}

#[test]
fn tomato_file_clipboard_reconnect_reuses_factory_without_reviving_old_streams() {
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel::<RdpInputEvent>();
    let bridge = Bridge::new(tx.into());
    bridge.remote_locks.lock().unwrap().insert(7, 0);
    bridge.file_enabled.store(true, Ordering::Release);
    bridge.last_local_sequence.store(99, Ordering::Release);
    let factory = bridge.wrap(ironrdp_cliprdr_native::StubClipboard::new().backend_factory());
    let backend = factory.build_cliprdr_backend();
    assert!(bridge.active.load(Ordering::Acquire));
    assert!(!bridge.file_enabled.load(Ordering::Acquire));
    assert_eq!(bridge.last_local_sequence.load(Ordering::Acquire), 0);
    let new_epoch = bridge.epoch.load(Ordering::Acquire);
    bridge.remote_locks.lock().unwrap().insert(7, new_epoch);
    assert!(bridge.request(0, Some(7), 0, 0, 3, false).is_err());
    assert!(events.try_recv().is_err());
    drop(backend);
    assert!(!bridge.active.load(Ordering::Acquire));
    let backend = factory.build_cliprdr_backend();
    assert!(bridge.active.load(Ordering::Acquire));
    assert!(bridge.remote_locks.lock().unwrap().is_empty());
    drop(backend);
}
