//! Test-only filesystem rooted in a disposable directory.
use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version,
};
use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, PathBuf},
};
enum Open {
    File(std::fs::File),
    Directory(Option<Vec<File>>),
    GeneratedDirectory(usize),
}
pub struct Fs {
    root: PathBuf,
    handles: HashMap<String, Open>,
    next: u32,
}
impl Fs {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            handles: HashMap::new(),
            next: 0,
        }
    }
    fn local(&self, path: &str) -> Result<PathBuf, StatusCode> {
        let mut result = self.root.clone();
        for part in std::path::Path::new(path.trim_start_matches('/')).components() {
            match part {
                Component::Normal(p) => result.push(p),
                Component::CurDir => {}
                _ => return Err(StatusCode::PermissionDenied),
            }
        }
        if result.starts_with(&self.root) {
            Ok(result)
        } else {
            Err(StatusCode::PermissionDenied)
        }
    }
    fn handle(&mut self, id: u32, value: Open) -> Handle {
        self.next += 1;
        let handle = self.next.to_string();
        self.handles.insert(handle.clone(), value);
        Handle { id, handle }
    }
}
fn status(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: String::new(),
        language_tag: "en".into(),
    }
}
fn error(e: std::io::Error) -> StatusCode {
    match e.kind() {
        std::io::ErrorKind::NotFound => StatusCode::NoSuchFile,
        std::io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        _ => StatusCode::Failure,
    }
}
fn metadata(meta: std::fs::Metadata) -> FileAttributes {
    FileAttributes {
        size: Some(meta.len()),
        permissions: Some(if meta.is_dir() { 0o40755 } else { 0o100644 }),
        mtime: Some(1_700_000_000),
        atime: Some(1_700_000_000),
        ..Default::default()
    }
}
impl russh_sftp::server::Handler for Fs {
    type Error = StatusCode;
    fn unimplemented(&self) -> StatusCode {
        StatusCode::OpUnsupported
    }
    async fn init(&mut self, _: u32, _: HashMap<String, String>) -> Result<Version, StatusCode> {
        Ok(Version::new())
    }
    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, StatusCode> {
        self.local(&path)?;
        Ok(Name {
            id,
            files: vec![File::dummy(if path == "." {
                "/".into()
            } else {
                format!("/{}", path.trim_start_matches('/'))
            })],
        })
    }
    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, StatusCode> {
        if path == "/too-many" {
            return Ok(self.handle(id, Open::GeneratedDirectory(0)));
        }
        let rows = std::fs::read_dir(self.local(&path)?)
            .map_err(error)?
            .map(|e| {
                let e = e.map_err(error)?;
                Ok(File::new(
                    e.file_name().to_string_lossy().into_owned(),
                    metadata(e.metadata().map_err(error)?),
                ))
            })
            .collect::<Result<Vec<_>, StatusCode>>()?;
        Ok(self.handle(id, Open::Directory(Some(rows))))
    }
    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, StatusCode> {
        match self.handles.get_mut(&handle) {
            Some(Open::GeneratedDirectory(emitted)) => {
                if *emitted >= 20_001 {
                    return Err(StatusCode::Eof);
                }
                let end = (*emitted + 64).min(20_001);
                let files = (*emitted..end)
                    .map(|i| {
                        File::new(
                            format!("entry-{i:05}"),
                            FileAttributes {
                                permissions: Some(0o100644),
                                ..Default::default()
                            },
                        )
                    })
                    .collect();
                *emitted = end;
                Ok(Name { id, files })
            }
            Some(Open::Directory(rows)) => rows
                .take()
                .map(|files| Name { id, files })
                .ok_or(StatusCode::Eof),
            _ => Err(StatusCode::Failure),
        }
    }
    async fn open(
        &mut self,
        id: u32,
        path: String,
        flags: OpenFlags,
        _: FileAttributes,
    ) -> Result<Handle, StatusCode> {
        let file = std::fs::OpenOptions::new()
            .read(flags.contains(OpenFlags::READ))
            .write(flags.contains(OpenFlags::WRITE))
            .create(flags.contains(OpenFlags::CREATE))
            .create_new(flags.contains(OpenFlags::EXCLUDE))
            .truncate(flags.contains(OpenFlags::TRUNCATE))
            .open(self.local(&path)?)
            .map_err(error)?;
        Ok(self.handle(id, Open::File(file)))
    }
    async fn close(&mut self, id: u32, handle: String) -> Result<Status, StatusCode> {
        self.handles.remove(&handle);
        Ok(status(id))
    }
    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, StatusCode> {
        let Some(Open::File(file)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };
        file.seek(SeekFrom::Start(offset)).map_err(error)?;
        let mut data = vec![0; len.min(64 * 1024) as usize];
        let n = file.read(&mut data).map_err(error)?;
        if n == 0 {
            return Err(StatusCode::Eof);
        }
        data.truncate(n);
        Ok(Data { id, data })
    }
    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, StatusCode> {
        let Some(Open::File(file)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };
        file.seek(SeekFrom::Start(offset)).map_err(error)?;
        file.write_all(&data).map_err(error)?;
        Ok(status(id))
    }
    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, StatusCode> {
        Ok(Attrs {
            id,
            attrs: metadata(std::fs::metadata(self.local(&path)?).map_err(error)?),
        })
    }
    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, StatusCode> {
        Ok(Attrs {
            id,
            attrs: metadata(std::fs::symlink_metadata(self.local(&path)?).map_err(error)?),
        })
    }
    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, StatusCode> {
        match self.handles.get(&handle) {
            Some(Open::File(file)) => Ok(Attrs {
                id,
                attrs: metadata(file.metadata().map_err(error)?),
            }),
            _ => Err(StatusCode::Failure),
        }
    }
    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        _: FileAttributes,
    ) -> Result<Status, StatusCode> {
        std::fs::create_dir(self.local(&path)?).map_err(error)?;
        Ok(status(id))
    }
    async fn remove(&mut self, id: u32, path: String) -> Result<Status, StatusCode> {
        std::fs::remove_file(self.local(&path)?).map_err(error)?;
        Ok(status(id))
    }
    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, StatusCode> {
        std::fs::remove_dir(self.local(&path)?).map_err(error)?;
        Ok(status(id))
    }
    async fn rename(&mut self, id: u32, from: String, to: String) -> Result<Status, StatusCode> {
        std::fs::rename(self.local(&from)?, self.local(&to)?).map_err(error)?;
        Ok(status(id))
    }
}
