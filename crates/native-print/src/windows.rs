use anyhow::{Result, bail, ensure};
use std::{ffi::OsStr, ops::Range, os::windows::ffi::OsStrExt, path::Path};
use windows::{
    Win32::{
        Foundation::{GlobalFree, HGLOBAL, HWND},
        Graphics::Gdi::*,
        Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage},
        System::Memory::{GlobalLock, GlobalSize, GlobalUnlock},
        UI::{Controls::Dialogs::*, Input::KeyboardAndMouse::GetActiveWindow},
    },
    core::{PCWSTR, w},
};

#[derive(Clone, Copy)]
pub struct Owner(usize);
impl Owner {
    pub fn active() -> Self {
        // SAFETY: Reads the active window of the calling GUI thread.
        Self(unsafe { GetActiveWindow() }.0 as usize)
    }
}

struct Global(HGLOBAL);
impl Drop for Global {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: PrintDlg hands its movable allocations to the caller.
            let _ = unsafe { GlobalFree(Some(self.0)) };
        }
    }
}

pub struct Printer {
    dc: HDC,
    started: bool,
    pages: Range<usize>,
}

impl Printer {
    pub fn select(owner: Owner, count: usize) -> Result<Option<Self>> {
        let last = u16::try_from(count)?;
        ensure!(last > 0, "没有可打印的页面");
        let mut dialog = PRINTDLGW {
            lStructSize: size_of::<PRINTDLGW>() as u32,
            hwndOwner: HWND(owner.0 as *mut _),
            Flags: PD_RETURNDC
                | PD_NOSELECTION
                | PD_DISABLEPRINTTOFILE
                | PD_USEDEVMODECOPIESANDCOLLATE,
            nFromPage: 1,
            nToPage: last,
            nMinPage: 1,
            nMaxPage: last,
            nCopies: 1,
            ..Default::default()
        };
        // SAFETY: The owner is an opaque window ID from the requesting UI thread;
        // the structure stays alive on this worker until the modal API returns.
        let selected = unsafe { PrintDlgW(&mut dialog) }.as_bool();
        let mut printer = Self {
            dc: dialog.hDC,
            started: false,
            pages: 0..count,
        };
        let mode = Global(dialog.hDevMode);
        let _names = Global(dialog.hDevNames);
        if !selected {
            // SAFETY: Read the common dialog result on its calling thread.
            let error = unsafe { CommDlgExtendedError() };
            if error.0 == 0 {
                return Ok(None);
            }
            bail!("无法打开系统打印对话框（{:#06x}）", error.0);
        }
        ensure!(!printer.dc.is_invalid(), "打印机没有返回设备上下文");
        if dialog.Flags.contains(PD_PAGENUMS) {
            let start = usize::from(dialog.nFromPage).clamp(1, count) - 1;
            let end = usize::from(dialog.nToPage).clamp(start + 1, count);
            printer.pages = start..end;
        }
        printer.configure_paper(&mode);
        Ok(Some(printer))
    }

    /// An explicit file destination for the Windows PDF printer. Also used by the
    /// isolated print acceptance fixture without selecting any physical printer.
    pub fn pdf(count: usize) -> Result<Self> {
        let mut mode = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        set_a4(&mut mode);
        // SAFETY: Static UTF-16 names and a fully initialized public DEVMODE.
        let dc = unsafe {
            CreateDCW(
                w!("WINSPOOL"),
                w!("Microsoft Print to PDF"),
                PCWSTR::null(),
                Some(&mode),
            )
        };
        ensure!(!dc.is_invalid(), "未找到 Microsoft Print to PDF");
        Ok(Self {
            dc,
            started: false,
            pages: 0..count,
        })
    }

    fn configure_paper(&mut self, mode: &Global) {
        // SAFETY: This worker exclusively owns both the DC and the dialog's
        // DEVMODE. The GlobalLock is balanced and kept during ResetDCW.
        unsafe {
            if mode.0.is_invalid() || GlobalSize(mode.0) < size_of::<DEVMODEW>() {
                return;
            }
            let raw = GlobalLock(mode.0).cast::<DEVMODEW>();
            if let Some(mode_value) = raw.as_mut() {
                set_a4(mode_value);
                let configured = ResetDCW(self.dc, raw);
                if !configured.is_invalid() {
                    self.dc = configured;
                }
                let _ = GlobalUnlock(mode.0);
            }
        }
    }

    pub fn pages(&self) -> Range<usize> {
        self.pages.clone()
    }

    pub fn begin(&mut self, name: &str, output: Option<&Path>) -> Result<()> {
        ensure!(!self.started, "打印任务已启动");
        let title = wide(OsStr::new(name))?;
        let path = output.map(|path| wide(path.as_os_str())).transpose()?;
        let info = DOCINFOW {
            cbSize: size_of::<DOCINFOW>() as i32,
            lpszDocName: PCWSTR(title.as_ptr()),
            lpszOutput: path
                .as_ref()
                .map_or(PCWSTR::null(), |value| PCWSTR(value.as_ptr())),
            ..Default::default()
        };
        // SAFETY: The owned DC and nul-terminated names live through StartDocW.
        self.started = unsafe { StartDocW(self.dc, &info) } > 0;
        ensure!(self.started, "打印任务未启动或已取消");
        Ok(())
    }

    pub fn page(&mut self, width: u32, height: u32, bgra: &[u8]) -> Result<()> {
        ensure!(self.started, "打印任务尚未启动");
        ensure!(
            width > 0
                && height > 0
                && width <= 8192
                && height <= 8192
                && bgra.len() == (width as usize * height as usize * 4),
            "打印页面尺寸无效"
        );
        let width = width as i32;
        let height = height as i32;
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: This worker exclusively owns the DC. The DIB header describes
        // the exact checked length and top-down BGRA layout of the live slice.
        unsafe {
            let caps = |index| GetDeviceCaps(Some(self.dc), index);
            let physical = [caps(PHYSICALWIDTH), caps(PHYSICALHEIGHT)];
            let offset = [caps(PHYSICALOFFSETX), caps(PHYSICALOFFSETY)];
            let printable = [caps(HORZRES), caps(VERTRES)];
            ensure!(
                physical.into_iter().all(|value| value > 0)
                    && printable.into_iter().all(|value| value > 0),
                "打印机纸张尺寸无效"
            );
            // Print the A4 layout centered on the selected paper. Device margins
            // clip only the whitespace whenever the driver supports the A4 form.
            let scale = (f64::from(physical[0]) / f64::from(width))
                .min(f64::from(physical[1]) / f64::from(height));
            let w = (f64::from(width) * scale).round() as i32;
            let h = (f64::from(height) * scale).round() as i32;
            ensure!(StartPage(self.dc) > 0, "无法开始打印页面");
            SetStretchBltMode(self.dc, HALFTONE);
            let lines = StretchDIBits(
                self.dc,
                (physical[0] - w) / 2 - offset[0],
                (physical[1] - h) / 2 - offset[1],
                w,
                h,
                0,
                0,
                width,
                height,
                Some(bgra.as_ptr().cast()),
                &info,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
            ensure!(lines > 0, "打印机拒绝页面图像");
            ensure!(EndPage(self.dc) > 0, "无法结束打印页面");
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        // SAFETY: Only this worker can finalize its live print job.
        ensure!(
            self.started && unsafe { EndDoc(self.dc) } > 0,
            "打印失败，请重试"
        );
        self.started = false;
        Ok(())
    }
}

impl Drop for Printer {
    fn drop(&mut self) {
        if !self.dc.is_invalid() {
            // SAFETY: The DC is exclusively owned and released once. A failure
            // after StartDoc aborts the incomplete spool job before deleting it.
            unsafe {
                if self.started {
                    AbortDoc(self.dc);
                }
                let _ = DeleteDC(self.dc);
            }
        }
    }
}

fn wide(value: &OsStr) -> Result<Vec<u16>> {
    let mut value: Vec<_> = value.encode_wide().collect();
    ensure!(!value.contains(&0), "打印名称或路径含无效字符");
    value.push(0);
    Ok(value)
}

fn set_a4(mode: &mut DEVMODEW) {
    mode.dmFields |= DM_ORIENTATION | DM_PAPERSIZE;
    mode.dmFields &= !(DM_PAPERWIDTH | DM_PAPERLENGTH | DM_FORMNAME);
    mode.Anonymous1.Anonymous1.dmOrientation = DMORIENT_PORTRAIT as i16;
    mode.Anonymous1.Anonymous1.dmPaperSize = DMPAPER_A4 as i16;
}
