//! Metadata is independent of network staging; Explorer can accept a pending file.
use super::*;
use std::mem::ManuallyDrop;
use windows::Win32::{
    Foundation::{E_OUTOFMEMORY, GlobalFree},
    Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL},
    System::{DataExchange::RegisterClipboardFormatW, Memory::*},
};
pub(super) struct Descriptor {
    files: Vec<FILEDESCRIPTORW>,
    pub group_format: u16,
    pub contents_format: u16,
    pub drag_format: u16,
}
impl Descriptor {
    pub fn new(files: &[File]) -> Result<Self> {
        anyhow::ensure!(
            !files.is_empty() && files.len() <= 4096,
            "文件拖放列表过大或为空"
        );
        let mut descriptors = Vec::with_capacity(files.len());
        for file in files {
            anyhow::ensure!(
                file.name.split('\\').all(|part| !part.is_empty()
                    && part != "."
                    && part != ".."
                    && !part.contains(['/', ':', '\0'])
                    && !part.ends_with(['.', ' '])),
                "拖放文件路径不是安全的相对路径"
            );
            let name: Vec<u16> = file.name.encode_utf16().collect();
            anyhow::ensure!(
                !name.is_empty() && name.len() < 260 && !name.contains(&0),
                "文件名超出 Windows 拖放长度限制"
            );
            let mut filename = [0; 260];
            filename[..name.len()].copy_from_slice(&name);
            descriptors.push(FILEDESCRIPTORW {
                dwFlags: (FD_ATTRIBUTES.0
                    | FD_UNICODE.0
                    | FD_PROGRESSUI.0
                    | if !file.directory && file.size.is_some() {
                        FD_FILESIZE.0
                    } else {
                        0
                    }) as u32,
                dwFileAttributes: if file.directory {
                    FILE_ATTRIBUTE_DIRECTORY.0
                } else {
                    FILE_ATTRIBUTE_NORMAL.0
                },
                cFileName: filename,
                nFileSizeHigh: (file.size.unwrap_or(0) >> 32) as u32,
                nFileSizeLow: file.size.unwrap_or(0) as u32,
                ..Default::default()
            });
        }
        let group_format = unsafe { RegisterClipboardFormatW(CFSTR_FILEDESCRIPTORW) } as u16;
        let contents_format = unsafe { RegisterClipboardFormatW(CFSTR_FILECONTENTS) } as u16;
        let drag_format = unsafe { RegisterClipboardFormatW(CFSTR_INDRAGLOOP) } as u16;
        anyhow::ensure!(
            group_format != 0 && contents_format != 0 && drag_format != 0,
            "无法注册系统文件拖放格式"
        );
        Ok(Self {
            files: descriptors,
            group_format,
            contents_format,
            drag_format,
        })
    }
    pub fn drag_state(&self, in_loop: bool) -> windows::core::Result<STGMEDIUM> {
        unsafe {
            let memory = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, std::mem::size_of::<u32>())?;
            let address = GlobalLock(memory);
            if address.is_null() {
                let _ = GlobalFree(Some(memory));
                return Err(E_OUTOFMEMORY.into());
            }
            address.cast::<u32>().write(u32::from(in_loop));
            let _ = GlobalUnlock(memory);
            Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 { hGlobal: memory },
                pUnkForRelease: ManuallyDrop::new(None),
            })
        }
    }
    pub fn data(&self) -> windows::core::Result<STGMEDIUM> {
        unsafe {
            let memory = GlobalAlloc(
                GMEM_MOVEABLE | GMEM_ZEROINIT,
                std::mem::offset_of!(FILEGROUPDESCRIPTORW, fgd)
                    + self.files.len() * std::mem::size_of::<FILEDESCRIPTORW>(),
            )?;
            let address = GlobalLock(memory);
            if address.is_null() {
                let _ = GlobalFree(Some(memory));
                return Err(E_OUTOFMEMORY.into());
            }
            address.cast::<u32>().write(self.files.len() as u32);
            std::ptr::copy_nonoverlapping(
                self.files.as_ptr(),
                address
                    .cast::<u8>()
                    .add(std::mem::offset_of!(FILEGROUPDESCRIPTORW, fgd))
                    .cast(),
                self.files.len(),
            );
            let _ = GlobalUnlock(memory);
            Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 { hGlobal: memory },
                pUnkForRelease: ManuallyDrop::new(None),
            })
        }
    }
}
