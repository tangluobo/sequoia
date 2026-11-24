//! Lazy CLIPRDR file streams. File bytes never pass through a GUI frame.
mod backend;
mod source;
mod windows;

use crate::rdp::{InputSender, RdpInputEvent};
use ironrdp_cliprdr::{
    backend::{ClipboardMessage, CliprdrBackendFactory},
    pdu::{
        ClipboardFormat, FileContentsFlags, FileContentsRequest, FileContentsResponse,
        FileDescriptor,
    },
};
use std::collections::BTreeMap;
use std::io;
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    mpsc,
};
use std::time::Duration;

pub(crate) use windows::FileOwner;
pub(super) const MAX_FILES: usize = 10_000;
const MAX_REQUESTS: usize = 4;
const CHUNK: usize = 1024 * 1024;
type Files = Arc<Vec<source::Entry>>;
type Reply = mpsc::SyncSender<io::Result<Vec<u8>>>;
struct Pending {
    reply: Reply,
    limit: usize,
    lock: Option<u32>,
    epoch: u64,
}
pub(super) enum Publication {
    Files(Arc<Offer>),
    Clear,
}
pub(super) struct Offer {
    origin: u64,
    files: Files,
    sequence: AtomicU32,
}

pub(crate) struct Bridge {
    id: u64,
    tx: InputSender,
    active: AtomicBool,
    file_enabled: AtomicBool,
    epoch: AtomicU64,
    next_stream: AtomicU32,
    last_local_sequence: AtomicU32,
    pending: Mutex<BTreeMap<u32, Pending>>,
    local: Mutex<Option<Files>>,
    locked: Mutex<BTreeMap<u32, Files>>,
    remote_locks: Mutex<BTreeMap<u32, u64>>,
    publication: Mutex<Option<Publication>>,
}
impl std::fmt::Debug for Bridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileClipboard")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}
impl Bridge {
    pub(crate) fn new(tx: InputSender) -> Arc<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Arc::new(Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            tx,
            active: AtomicBool::new(true),
            file_enabled: AtomicBool::new(false),
            epoch: AtomicU64::new(0),
            next_stream: AtomicU32::new(1),
            last_local_sequence: AtomicU32::new(0),
            pending: Mutex::new(BTreeMap::new()),
            local: Mutex::new(None),
            locked: Mutex::new(BTreeMap::new()),
            remote_locks: Mutex::new(BTreeMap::new()),
            publication: Mutex::new(None),
        })
    }
    fn send(&self, message: ClipboardMessage) -> io::Result<()> {
        if !self.active.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "RDP file clipboard closed",
            ));
        }
        self.tx
            .send(RdpInputEvent::Clipboard(message))
            .map_err(|_| {
                io::Error::new(io::ErrorKind::BrokenPipe, "RDP file clipboard disconnected")
            })
    }
    pub(crate) fn wrap(
        self: &Arc<Self>,
        inner: Box<dyn CliprdrBackendFactory + Send>,
    ) -> Box<dyn CliprdrBackendFactory + Send> {
        Box::new(backend::Factory {
            inner,
            bridge: self.clone(),
        })
    }
    pub(super) fn local_changed(
        self: &Arc<Self>,
        formats: Vec<ClipboardFormat>,
    ) -> Option<ClipboardMessage> {
        windows::local_changed(self, formats)
    }
    fn publish(&self, event: Publication) {
        *self.publication.lock().unwrap() = Some(event);
    }
    fn offer_local(&self, files: Files) -> io::Result<()> {
        if !self.file_enabled.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Remote clipboard file streaming is disabled",
            ));
        }
        let descriptors = files.iter().map(|f| f.descriptor.clone()).collect();
        *self.local.lock().unwrap() = Some(files);
        self.send(ClipboardMessage::SendInitiateFileCopy(descriptors))
    }
    fn remote_files(self: &Arc<Self>, descriptors: &[FileDescriptor], lock: Option<u32>) {
        if descriptors.is_empty() || descriptors.len() > MAX_FILES {
            return;
        }
        let epoch = self.epoch.load(Ordering::Acquire);
        if let Some(id) = lock {
            let mut locks = self.remote_locks.lock().unwrap();
            if locks.len() >= 64 && !locks.contains_key(&id) {
                self.publish(Publication::Clear);
                tracing::warn!("Remote clipboard lock limit reached");
                return;
            }
            locks.insert(id, epoch);
        }
        let files = descriptors
            .iter()
            .enumerate()
            .map(|(index, descriptor)| source::Entry {
                descriptor: descriptor.clone(),
                source: source::Source::Remote(source::Remote {
                    bridge: Arc::downgrade(self),
                    index: index as i32,
                    lock,
                    epoch,
                    size: descriptor.file_size,
                }),
            })
            .collect();
        self.publish(Publication::Files(Arc::new(Offer {
            origin: self.id,
            files: Arc::new(files),
            sequence: AtomicU32::new(0),
        })));
    }
    fn request(
        &self,
        index: i32,
        lock: Option<u32>,
        epoch: u64,
        position: u64,
        length: usize,
        size: bool,
    ) -> io::Result<Vec<u8>> {
        if lock.is_some_and(|id| self.remote_locks.lock().unwrap().get(&id) != Some(&epoch)) {
            return Err(io::Error::other("Remote clipboard file snapshot expired"));
        }
        if lock.is_none() && epoch != self.epoch.load(Ordering::Acquire) {
            return Err(io::Error::other("Remote clipboard changed"));
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        let length = length.min(CHUNK);
        let id = {
            let mut pending = self.pending.lock().unwrap();
            if pending.len() >= MAX_REQUESTS {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "RDP file request limit reached",
                ));
            }
            let mut id = self.next_stream.fetch_add(1, Ordering::Relaxed);
            while pending.contains_key(&id) {
                id = self.next_stream.fetch_add(1, Ordering::Relaxed);
            }
            pending.insert(
                id,
                Pending {
                    reply,
                    limit: length,
                    lock,
                    epoch,
                },
            );
            id
        };
        let request = FileContentsRequest {
            stream_id: id,
            index,
            flags: if size {
                FileContentsFlags::SIZE
            } else {
                FileContentsFlags::RANGE
            },
            position,
            requested_size: length as u32,
            data_id: lock,
        };
        if let Err(error) = self.send(ClipboardMessage::SendFileContentsRequest(request)) {
            self.pending.lock().unwrap().remove(&id);
            return Err(error);
        }
        let result = receiver.recv_timeout(Duration::from_secs(30));
        self.pending.lock().unwrap().remove(&id);
        result.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "RDP file read timed out"))?
    }
    fn response(&self, response: FileContentsResponse<'_>) {
        if let Some(pending) = self.pending.lock().unwrap().remove(&response.stream_id()) {
            let snapshot_valid = pending.lock.map_or_else(
                || pending.epoch == self.epoch.load(Ordering::Acquire),
                |id| self.remote_locks.lock().unwrap().get(&id) == Some(&pending.epoch),
            );
            let result = if !snapshot_valid {
                Err(io::Error::other("Remote clipboard file snapshot changed"))
            } else if response.is_error() {
                Err(io::Error::other("Remote file read failed"))
            } else if response.data().len() > pending.limit {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Remote file chunk exceeds request",
                ))
            } else {
                Ok(response.data().to_vec())
            };
            let _ = pending.reply.try_send(result);
        }
    }
    fn retire_locks(&self, locks: &[ironrdp_cliprdr::pdu::LockDataId]) {
        {
            let mut active = self.remote_locks.lock().unwrap();
            for id in locks {
                active.remove(&id.0);
            }
        }
        let mut pending = self.pending.lock().unwrap();
        let ids: Vec<_> = pending
            .iter()
            .filter(|(_, p)| locks.iter().any(|id| Some(id.0) == p.lock))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(p) = pending.remove(&id) {
                let _ = p
                    .reply
                    .try_send(Err(io::Error::other("Remote clipboard lock expired")));
            }
        }
    }
    fn begin_session(&self) {
        self.close();
        self.file_enabled.store(false, Ordering::Release);
        self.last_local_sequence.store(0, Ordering::Release);
        self.active.store(true, Ordering::Release);
    }
    pub(crate) fn close(&self) {
        self.active.store(false, Ordering::Release);
        self.epoch.fetch_add(1, Ordering::AcqRel);
        for (_, pending) in std::mem::take(&mut *self.pending.lock().unwrap()) {
            let _ = pending.reply.try_send(Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "RDP session closed",
            )));
        }
        self.local.lock().unwrap().take();
        self.locked.lock().unwrap().clear();
        self.remote_locks.lock().unwrap().clear();
        self.publish(Publication::Clear);
    }
}

fn file_format(format: &ClipboardFormat) -> bool {
    format
        .name()
        .is_some_and(|name| matches!(name.value(), "FileGroupDescriptorW" | "FileContents"))
}

#[cfg(test)]
mod tests;
