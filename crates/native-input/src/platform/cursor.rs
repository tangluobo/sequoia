//! A receiving host without a mouse may never compose a system cursor. Draw a
//! small click-through window in that case; never change Windows cursor settings.
use anyhow::{Result, ensure};
use std::{ffi::c_void, mem::size_of};
use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, POINT, SIZE},
        Graphics::Gdi::*,
        UI::WindowsAndMessaging::*,
    },
    core::w,
};

fn needed(receiving: bool, showing: bool, suppressed: bool, mouse_present: bool) -> bool {
    receiving && (!showing || suppressed || !mouse_present)
}

#[derive(Default)]
pub(super) struct Feedback {
    overlay: Option<Overlay>,
}
impl Feedback {
    pub fn clear(&mut self) {
        self.overlay = None;
    }
    pub fn sync(&mut self, receiving: bool) -> Result<()> {
        if !receiving {
            self.clear();
            return Ok(());
        }
        let mut info = CURSORINFO {
            cbSize: size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        // Another input desktop may be active (for example a UAC prompt).
        if unsafe { GetCursorInfo(&mut info) }.is_err() {
            self.clear();
            return Ok(());
        }
        let showing = info.flags.0 & CURSOR_SHOWING.0 != 0;
        let suppressed = info.flags.0 & CURSOR_SUPPRESSED.0 != 0;
        if !needed(
            receiving,
            showing,
            suppressed,
            unsafe { GetSystemMetrics(SM_MOUSEPRESENT) } != 0,
        ) {
            self.clear();
            return Ok(());
        }
        if self.overlay.is_none() {
            self.overlay = Some(Overlay::new()?);
        }
        self.overlay.as_mut().unwrap().show_at(info.ptScreenPos)
    }
}

struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut c_void,
    width: i32,
    height: i32,
}
impl Surface {
    fn new(width: i32, height: i32) -> Result<Self> {
        ensure!(
            (1..=256).contains(&width) && (1..=256).contains(&height),
            "无效的共享指针尺寸"
        );
        let mut result = Self {
            dc: unsafe { CreateCompatibleDC(None) },
            bitmap: HBITMAP::default(),
            previous: HGDIOBJ::default(),
            bits: std::ptr::null_mut(),
            width,
            height,
        };
        ensure!(!result.dc.0.is_null(), "无法创建共享指针绘图上下文");
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
        result.bitmap = unsafe {
            CreateDIBSection(
                Some(result.dc),
                &info,
                DIB_RGB_COLORS,
                &mut result.bits,
                None,
                0,
            )?
        };
        ensure!(!result.bits.is_null(), "无法分配共享指针像素");
        result.previous = unsafe { SelectObject(result.dc, HGDIOBJ(result.bitmap.0)) };
        ensure!(
            !result.previous.0.is_null() && result.previous.0 as isize != -1,
            "无法选择共享指针位图"
        );
        Ok(result)
    }
    fn pixels(&mut self) -> &mut [u8] {
        // Owned top-down 32-bit DIB; its dimensions were bounded at allocation.
        unsafe {
            std::slice::from_raw_parts_mut(
                self.bits.cast(),
                (self.width * self.height * 4) as usize,
            )
        }
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            if !self.previous.0.is_null() && self.previous.0 as isize != -1 {
                let _ = SelectObject(self.dc, self.previous);
            }
            if !self.bitmap.0.is_null() {
                let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            }
            if !self.dc.0.is_null() {
                let _ = DeleteDC(self.dc);
            }
        }
    }
}
struct Icon(ICONINFO);
impl Drop for Icon {
    fn drop(&mut self) {
        unsafe {
            for bitmap in [self.0.hbmColor, self.0.hbmMask] {
                if !bitmap.0.is_null() {
                    let _ = DeleteObject(HGDIOBJ(bitmap.0));
                }
            }
        }
    }
}
/// Draw against two backgrounds to recover premultiplied alpha even for legacy
/// mask-based system arrows. A color key would lose black outlines or leave a box.
fn arrow() -> Result<(Surface, POINT)> {
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW)? };
    let mut icon = Icon(ICONINFO::default());
    unsafe {
        GetIconInfo(HICON(cursor.0), &mut icon.0)?;
    }
    let monochrome = icon.0.hbmColor.0.is_null();
    let bitmap = if monochrome {
        icon.0.hbmMask
    } else {
        icon.0.hbmColor
    };
    let mut dimensions = BITMAP::default();
    ensure!(
        unsafe {
            GetObjectW(
                HGDIOBJ(bitmap.0),
                size_of::<BITMAP>() as i32,
                Some((&mut dimensions as *mut BITMAP).cast()),
            )
        } != 0,
        "无法读取共享指针尺寸"
    );
    let original_width = dimensions.bmWidth.max(1);
    let original_height = (dimensions.bmHeight / if monochrome { 2 } else { 1 }).max(1);
    let width = original_width.clamp(1, 256);
    let height = original_height.clamp(1, 256);
    let mut black = Surface::new(width, height)?;
    let mut white = Surface::new(width, height)?;
    for pixel in black.pixels().as_chunks_mut::<4>().0.iter_mut() {
        pixel.copy_from_slice(&[0, 0, 0, 255]);
    }
    white.pixels().fill(255);
    unsafe {
        DrawIconEx(
            black.dc,
            0,
            0,
            HICON(cursor.0),
            width,
            height,
            0,
            None,
            DI_NORMAL,
        )?;
        DrawIconEx(
            white.dc,
            0,
            0,
            HICON(cursor.0),
            width,
            height,
            0,
            None,
            DI_NORMAL,
        )?;
        ensure!(GdiFlush().as_bool(), "无法完成共享指针绘制");
    }
    for (b, w) in black
        .pixels()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(white.pixels().as_chunks::<4>().0.iter())
    {
        let transparency = (0..3).map(|i| w[i].saturating_sub(b[i])).max().unwrap();
        let alpha = 255 - transparency;
        for channel in &mut b[..3] {
            *channel = (*channel).min(alpha);
        }
        b[3] = alpha;
    }
    let hotspot = POINT {
        x: (u64::from(icon.0.xHotspot) * width as u64 / original_width as u64)
            .min((width - 1) as u64) as i32,
        y: (u64::from(icon.0.yHotspot) * height as u64 / original_height as u64)
            .min((height - 1) as u64) as i32,
    };
    Ok((black, hotspot))
}
struct Overlay {
    hwnd: HWND,
    surface: Surface,
    hotspot: POINT,
    position: Option<(i32, i32)>,
}
impl Overlay {
    fn new() -> Result<Self> {
        let (surface, hotspot) = arrow()?;
        Self::with_image(surface, hotspot)
    }
    fn with_image(surface: Surface, hotspot: POINT) -> Result<Self> {
        // Layered WS_EX_TRANSPARENT passes mouse events to windows underneath,
        // including other processes. It also cannot activate or appear in Alt-Tab.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!("Tomato shared pointer"),
                WS_POPUP,
                0,
                0,
                surface.width,
                surface.height,
                None,
                None,
                None,
                None,
            )?
        };
        Ok(Self {
            hwnd,
            surface,
            hotspot,
            position: None,
        })
    }
    fn show_at(&mut self, point: POINT) -> Result<()> {
        let position = (point.x - self.hotspot.x, point.y - self.hotspot.y);
        // A menu or another topmost window may have appeared since the last
        // move. Restore our Z order even when the pointer itself is stationary.
        if self.position.is_none() {
            let dest = POINT {
                x: position.0,
                y: position.1,
            };
            let source = POINT::default();
            let size = SIZE {
                cx: self.surface.width,
                cy: self.surface.height,
            };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
                ..Default::default()
            };
            unsafe {
                UpdateLayeredWindow(
                    self.hwnd,
                    None,
                    Some(&dest),
                    Some(&size),
                    Some(self.surface.dc),
                    Some(&source),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                )?;
            }
        }
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                position.0,
                position.1,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )?;
        }
        self.position = Some(position);
        Ok(())
    }
}
impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_mouse_or_suppressed_system_cursor_needs_feedback_only_while_receiving() {
        assert!(needed(true, false, false, false));
        assert!(needed(true, true, false, false));
        assert!(needed(true, false, false, true));
        assert!(needed(true, true, true, true));
        assert!(!needed(true, true, false, true));
        assert!(!needed(false, false, true, false));
    }
    #[test]
    fn native_arrow_has_transparent_background_and_opaque_pixels_and_owned_window_is_released()
    -> Result<()> {
        let (mut surface, hotspot) = arrow()?;
        let pixels = surface.pixels();
        assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 0));
        assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 255));
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[..3].iter().all(|c| *c <= p[3]))
        );
        assert!(hotspot.x >= 0 && hotspot.y >= 0);
        let foreground = unsafe { GetForegroundWindow() };
        let overlay = Overlay::new()?;
        let hwnd = overlay.hwnd;
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
        assert_eq!(
            style & (WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW).0,
            (WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW).0
        );
        assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
        assert_eq!(unsafe { GetForegroundWindow() }, foreground);
        drop(overlay);
        assert!(!unsafe { IsWindow(Some(hwnd)) }.as_bool());
        Ok(())
    }
    #[test]
    #[ignore = "requires an interactive desktop and TOMATO_INPUT_CURSOR_VISUAL_OUTPUT; shows only owned, non-activating windows"]
    fn visible_software_cursor_does_not_move_or_activate_the_real_cursor() -> Result<()> {
        let output = std::env::var_os("TOMATO_INPUT_CURSOR_VISUAL_OUTPUT")
            .ok_or_else(|| anyhow::anyhow!("Set an owned BMP output path"))?;
        let mut background = Surface::new(200, 160)?;
        for pixel in background.pixels().as_chunks_mut::<4>().0.iter_mut() {
            pixel.copy_from_slice(&[96, 96, 96, 255]);
        }
        let position = unsafe {
            POINT {
                x: GetSystemMetrics(SM_CXSCREEN) / 2 - 100,
                y: GetSystemMetrics(SM_CYSCREEN) / 2 - 80,
            }
        };
        let foreground = unsafe { GetForegroundWindow() };
        let mut cursor_before = POINT::default();
        unsafe {
            GetCursorPos(&mut cursor_before)?;
        }
        let mut fixture = Overlay::with_image(background, POINT::default())?;
        fixture.show_at(position)?;
        settle();
        let crop = POINT {
            x: position.x + 40,
            y: position.y + 30,
        };
        let mut before = capture(crop)?;
        ensure!(
            before
                .pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[..3].iter().all(|c| c.abs_diff(96) <= 2)),
            "Owned background was obscured"
        );
        let mut pointer = Overlay::new()?;
        pointer.show_at(POINT {
            x: crop.x + 8,
            y: crop.y + 8,
        })?;
        let mut covering = Surface::new(200, 160)?;
        for pixel in covering.pixels().as_chunks_mut::<4>().0.iter_mut() {
            pixel.copy_from_slice(&[96, 96, 96, 255]);
        }
        let mut popup = Overlay::with_image(covering, POINT::default())?;
        popup.show_at(position)?;
        pointer.show_at(POINT {
            x: crop.x + 8,
            y: crop.y + 8,
        })?;
        settle();
        ensure!(
            unsafe { IsWindowVisible(pointer.hwnd) }.as_bool(),
            "Software cursor window did not become visible"
        );
        let mut after = capture(crop)?;
        let changed = before
            .pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .zip(after.pixels().as_chunks::<4>().0.iter())
            .filter(|(a, b)| a[..3] != b[..3])
            .count();
        ensure!(
            changed > 12,
            "No software cursor pixels in the owned window"
        );
        ensure!(
            after
                .pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .take(64)
                .all(|p| p[..3].iter().all(|c| c.abs_diff(96) <= 2)),
            "Owned capture was obscured"
        );
        let mut cursor_after = POINT::default();
        unsafe {
            GetCursorPos(&mut cursor_after)?;
        }
        ensure!(
            cursor_before.x == cursor_after.x && cursor_before.y == cursor_after.y,
            "Physical cursor changed during probe"
        );
        ensure!(
            unsafe { GetForegroundWindow() } == foreground,
            "Probe activated a window"
        );
        save_bmp(std::path::Path::new(&output), &mut after)?;
        drop(pointer);
        drop(popup);
        drop(fixture);
        println!(
            "Visible software cursor: {changed} pixels; no input injected; foreground and physical cursor unchanged"
        );
        Ok(())
    }
    fn settle() {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let mut msg = MSG::default();
        for _ in 0..64 {
            unsafe {
                if !PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    fn capture(point: POINT) -> Result<Surface> {
        let surface = Surface::new(64, 64)?;
        let dc = unsafe { GetDC(None) };
        ensure!(!dc.0.is_null(), "Screen capture DC unavailable");
        let result = unsafe {
            BitBlt(
                surface.dc,
                0,
                0,
                64,
                64,
                Some(dc),
                point.x,
                point.y,
                SRCCOPY | CAPTUREBLT,
            )
        };
        unsafe {
            let _ = ReleaseDC(None, dc);
        }
        result?;
        ensure!(unsafe { GdiFlush() }.as_bool(), "Capture flush failed");
        Ok(surface)
    }
    fn save_bmp(path: &std::path::Path, surface: &mut Surface) -> Result<()> {
        let bytes = (surface.width * surface.height * 4) as u32;
        let mut output = Vec::new();
        output.extend_from_slice(b"BM");
        output.extend_from_slice(&(54 + bytes).to_le_bytes());
        output.extend_from_slice(&[0; 4]);
        output.extend_from_slice(&54_u32.to_le_bytes());
        output.extend_from_slice(&40_u32.to_le_bytes());
        output.extend_from_slice(&surface.width.to_le_bytes());
        output.extend_from_slice(&(-surface.height).to_le_bytes());
        output.extend_from_slice(&1_u16.to_le_bytes());
        output.extend_from_slice(&32_u16.to_le_bytes());
        output.extend_from_slice(&[0; 4]);
        output.extend_from_slice(&bytes.to_le_bytes());
        output.extend_from_slice(&[0; 16]);
        output.extend_from_slice(surface.pixels());
        std::fs::write(path, output)?;
        Ok(())
    }
}
