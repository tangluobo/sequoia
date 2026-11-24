use super::*;
use ironrdp_cliprdr::pdu::ClipboardFileAttributes;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::SystemTime;

#[derive(Clone)]
pub(super) struct Entry {
    pub descriptor: FileDescriptor,
    pub source: Source,
}
#[derive(Clone)]
pub(super) enum Source {
    Local {
        path: PathBuf,
        length: u64,
        modified: Option<SystemTime>,
        directory: bool,
    },
    Remote(Remote),
}
#[derive(Clone)]
pub(super) struct Remote {
    pub bridge: Weak<Bridge>,
    pub index: i32,
    pub lock: Option<u32>,
    pub epoch: u64,
    pub size: Option<u64>,
}
impl Remote {
    fn bridge(&self) -> io::Result<Arc<Bridge>> {
        self.bridge
            .upgrade()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "Source RDP session closed"))
    }
}
impl Source {
    pub(super) fn length(&self) -> io::Result<u64> {
        match self {
            Self::Local { length, .. } => Ok(*length),
            Self::Remote(remote) => match remote.size {
                Some(size) => Ok(size),
                None => {
                    let bytes = remote.bridge()?.request(
                        remote.index,
                        remote.lock,
                        remote.epoch,
                        0,
                        8,
                        true,
                    )?;
                    Ok(u64::from_le_bytes(bytes.try_into().map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "Invalid remote file size")
                    })?))
                }
            },
        }
    }
    pub(super) fn read(&self, position: u64, length: usize) -> io::Result<Vec<u8>> {
        let length = length.min(CHUNK);
        match self {
            Self::Local {
                path,
                length: total,
                modified,
                directory,
            } => {
                if *directory {
                    return Err(io::Error::new(
                        io::ErrorKind::IsADirectory,
                        "Cannot read a directory as a file",
                    ));
                }
                let mut file = File::open(path)?;
                let metadata = file.metadata()?;
                if metadata.len() != *total || metadata.modified().ok() != *modified {
                    return Err(io::Error::other(
                        "Local file changed during clipboard transfer",
                    ));
                }
                let count = length.min(
                    total
                        .saturating_sub(position)
                        .try_into()
                        .unwrap_or(usize::MAX),
                );
                let mut bytes = vec![0; count];
                file.seek(SeekFrom::Start(position))?;
                file.read_exact(&mut bytes)?;
                Ok(bytes)
            }
            Self::Remote(remote) => remote.bridge()?.request(
                remote.index,
                remote.lock,
                remote.epoch,
                position,
                length,
                false,
            ),
        }
    }
    pub(super) fn open(self) -> io::Result<Reader> {
        let size = self.length()?;
        Ok(Reader {
            source: self,
            size,
            position: 0,
        })
    }
}
pub(super) struct Reader {
    source: Source,
    size: u64,
    position: u64,
}
impl Read for Reader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let length = buffer.len().min(CHUNK).min(
            self.size
                .saturating_sub(self.position)
                .try_into()
                .unwrap_or(usize::MAX),
        );
        if length == 0 {
            return Ok(0);
        }
        let bytes = self.source.read(self.position, length)?;
        if bytes.is_empty() || bytes.len() > length {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Incomplete remote file stream",
            ));
        }
        buffer[..bytes.len()].copy_from_slice(&bytes);
        self.position += bytes.len() as u64;
        Ok(bytes.len())
    }
}
impl Seek for Reader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let next = match from {
            SeekFrom::Start(p) => i128::from(p),
            SeekFrom::Current(p) => i128::from(self.position) + i128::from(p),
            SeekFrom::End(p) => i128::from(self.size) + i128::from(p),
        };
        self.position = u64::try_from(next)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid file seek"))?;
        Ok(self.position)
    }
}

enum Work {
    Offer(Arc<Bridge>, Vec<String>, u32),
    Read(Arc<Bridge>, Files, FileContentsRequest),
}
fn worker() -> &'static mpsc::SyncSender<Work> {
    static WORKER: OnceLock<mpsc::SyncSender<Work>> = OnceLock::new();
    WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel(8);
        std::thread::Builder::new()
            .name("tomato-rdp-files".into())
            .stack_size(512 * 1024)
            .spawn(move || {
                while let Ok(work) = rx.recv() {
                    match work {
                        Work::Offer(bridge, paths, sequence) => {
                            if windows::sequence() != sequence {
                                continue;
                            }
                            match local_entries(&paths) {
                                Ok(files) if windows::sequence() == sequence => {
                                    let _ = bridge.offer_local(Arc::new(files));
                                }
                                Ok(_) => {}
                                Err(error) if windows::sequence() == sequence => {
                                    // The initial native clipboard can contain a stale
                                    // Explorer file list. Still complete CLIPRDR setup
                                    // and retire any previous offer before reporting it.
                                    bridge.last_local_sequence.store(0, Ordering::Release);
                                    bridge.local.lock().unwrap().take();
                                    let _ =
                                        bridge.send(ClipboardMessage::SendInitiateCopy(Vec::new()));
                                    let _ = bridge.send(ClipboardMessage::Error(Box::new(error)));
                                }
                                Err(_) => {}
                            }
                        }
                        Work::Read(bridge, files, request) => {
                            let response = serve(&files, &request).unwrap_or_else(|_| {
                                FileContentsResponse::new_error(request.stream_id)
                            });
                            let _ =
                                bridge.send(ClipboardMessage::SendFileContentsResponse(response));
                        }
                    }
                }
            })
            .expect("RDP file worker thread");
        tx
    })
}
pub(super) fn queue_offer(bridge: Arc<Bridge>, paths: Vec<String>, sequence: u32) {
    if worker()
        .try_send(Work::Offer(bridge.clone(), paths, sequence))
        .is_err()
    {
        bridge.last_local_sequence.store(0, Ordering::Release);
        bridge.local.lock().unwrap().take();
        let _ = bridge.send(ClipboardMessage::SendInitiateCopy(Vec::new()));
        let _ = bridge.send(ClipboardMessage::Error(Box::new(io::Error::new(
            io::ErrorKind::WouldBlock,
            "File clipboard worker is busy",
        ))));
    }
}
pub(super) fn queue_read(bridge: Arc<Bridge>, files: Files, request: FileContentsRequest) {
    let id = request.stream_id;
    if worker()
        .try_send(Work::Read(bridge.clone(), files, request))
        .is_err()
    {
        let _ = bridge.send(ClipboardMessage::SendFileContentsResponse(
            FileContentsResponse::new_error(id),
        ));
    }
}
pub(super) fn serve(
    files: &[Entry],
    request: &FileContentsRequest,
) -> io::Result<FileContentsResponse<'static>> {
    let entry = usize::try_from(request.index)
        .ok()
        .and_then(|i| files.get(i))
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Unknown clipboard file index")
        })?;
    request.flags.validate().map_err(io::Error::other)?;
    if request.flags.contains(FileContentsFlags::SIZE) {
        return Ok(FileContentsResponse::new_size_response(
            request.stream_id,
            entry.source.length()?,
        ));
    }
    if request.requested_size as usize > CHUNK {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "File chunk exceeds one MiB",
        ));
    }
    Ok(FileContentsResponse::new_data_response(
        request.stream_id,
        entry
            .source
            .read(request.position, request.requested_size as usize)?,
    ))
}

pub(super) fn local_entries(paths: &[String]) -> io::Result<Vec<Entry>> {
    let mut pending = Vec::new();
    for path in paths.iter().rev() {
        let path = PathBuf::from(path);
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| io::Error::other("Clipboard file has no valid name"))?
            .to_owned();
        pending.push((path, name, 0));
    }
    let mut entries = Vec::new();
    while let Some((path, relative, depth)) = pending.pop() {
        if entries.len() >= MAX_FILES || depth > 64 || relative.encode_utf16().count() >= 260 {
            return Err(io::Error::other(
                "Clipboard file list exceeds supported bounds",
            ));
        }
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Clipboard symbolic links require an explicit file copy",
            ));
        }
        let directory = metadata.is_dir();
        let length = if directory { 0 } else { metadata.len() };
        let modified = metadata.modified().ok();
        let wire = relative.replace('/', "\\");
        let (parent, name) = wire
            .rsplit_once('\\')
            .map_or((None, wire.as_str()), |(p, n)| (Some(p), n));
        let mut descriptor = FileDescriptor::new(name)
            .with_file_size(length)
            .with_attributes(if directory {
                ClipboardFileAttributes::DIRECTORY
            } else {
                ClipboardFileAttributes::NORMAL
            });
        if let Some(parent) = parent {
            descriptor = descriptor.with_relative_path(parent);
        }
        if let Some(time) = modified
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .and_then(|t| t.as_nanos().checked_div(100))
            .and_then(|t| t.checked_add(116_444_736_000_000_000))
            .and_then(|t| u64::try_from(t).ok())
        {
            descriptor = descriptor.with_last_write_time(time);
        }
        entries.push(Entry {
            descriptor,
            source: Source::Local {
                path: path.clone(),
                length,
                modified,
                directory,
            },
        });
        if directory {
            let mut children = std::fs::read_dir(&path)?
                .take(MAX_FILES + 1)
                .collect::<Result<Vec<_>, _>>()?;
            if children
                .len()
                .saturating_add(entries.len())
                .saturating_add(pending.len())
                > MAX_FILES
            {
                return Err(io::Error::other("Clipboard file count exceeds limit"));
            }
            children.sort_by_key(|e| e.file_name());
            for child in children.into_iter().rev() {
                let name = child
                    .file_name()
                    .into_string()
                    .map_err(|_| io::Error::other("Invalid clipboard file name"))?;
                pending.push((child.path(), format!("{relative}\\{name}"), depth + 1));
            }
        }
    }
    Ok(entries)
}
