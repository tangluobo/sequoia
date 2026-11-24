//! Use the Windows rasterizer used by JavaFX, retaining egui shaping and atlas ownership.
//! All native objects belong to one font face and are released with its font cache.
use super::fonts::Blob;
use crate::{Color32, FontColorTransferFunction};

/// Gamma exponent for compositing LCD glyphs using the Windows text contrast.
pub fn lcd_gamma() -> f32 {
    use ::windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETFONTSMOOTHINGCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut contrast = 0u32;
    // SAFETY: this query writes a single UINT through pvParam.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETFONTSMOOTHINGCONTRAST,
            0,
            Some((&mut contrast as *mut u32).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    let contrast = if ok.is_ok() && contrast > 0 {
        contrast
    } else {
        1300
    };
    1000. / contrast as f32
}
use std::mem::ManuallyDrop;
use windows::Win32::Graphics::DirectWrite::*;
use windows_core::{IUnknown, Interface as _, implement};

// DirectWrite retains this owner, so it can share the already loaded font bytes
// instead of making another complete copy of every CJK font.
#[implement(windows_core::imp::IAgileObject)]
struct FontBytes {
    _bytes: Blob,
}
impl windows_core::imp::IAgileObject_Impl for FontBytes_Impl {}

struct Registration {
    factory: IDWriteFactory,
    loader: IDWriteInMemoryFontFileLoader,
}
impl Drop for Registration {
    fn drop(&mut self) {
        // SAFETY: this loader was successfully registered with this factory.
        // Font::face is dropped first; no native font/analysis escapes the adapter.
        let _ = unsafe { self.factory.UnregisterFontFileLoader(&self.loader) };
    }
}

pub(super) struct Font {
    // Field order matters: destroy faces before unregistering their file loader.
    face: IDWriteFontFace,
    registration: Registration,
}
pub(super) struct Mask {
    pub x: i32,
    pub y: i32,
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Color32>,
}
impl Font {
    pub fn new(bytes: Blob, index: u32) -> Option<Self> {
        Self::create(bytes, index).ok()
    }
    fn create(bytes: Blob, index: u32) -> windows_core::Result<Self> {
        let data = AsRef::<[u8]>::as_ref(bytes.as_ref());
        let length = u32::try_from(data.len()).map_err(|_| {
            windows_core::Error::from_hresult(windows_core::HRESULT(0x80070057u32 as i32))
        })?;
        let pointer = data.as_ptr();
        let owner: IUnknown = FontBytes { _bytes: bytes }.into();
        // SAFETY: DirectWrite owns its COM objects; the in-memory file holds an
        // AddRef on FontBytes for the entire lifetime of its referenced byte slice.
        unsafe {
            let factory5: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_ISOLATED)?;
            let factory: IDWriteFactory = factory5.cast()?;
            let loader = factory5.CreateInMemoryFontFileLoader()?;
            factory.RegisterFontFileLoader(&loader)?;
            let registration = Registration { factory, loader };
            let file = registration.loader.CreateInMemoryFontFileReference(
                &registration.factory,
                pointer.cast(),
                length,
                &owner,
            )?;
            let mut supported = windows_core::BOOL::default();
            let mut file_type = DWRITE_FONT_FILE_TYPE::default();
            let mut face_type = DWRITE_FONT_FACE_TYPE::default();
            let mut faces = 0;
            file.Analyze(
                &mut supported,
                &mut file_type,
                Some(&mut face_type),
                &mut faces,
            )?;
            if !supported.as_bool() || index >= faces {
                return Err(windows_core::Error::from_hresult(windows_core::HRESULT(
                    0x80070057u32 as i32,
                )));
            }
            let face = registration.factory.CreateFontFace(
                face_type,
                &[Some(file)],
                index,
                DWRITE_FONT_SIMULATIONS_NONE,
            )?;
            Ok(Self { face, registration })
        }
    }
    pub fn rasterize(
        &self,
        glyph: u16,
        size: f32,
        stripe_offset: i32,
        transfer: FontColorTransferFunction,
        grayscale: bool,
    ) -> Option<Mask> {
        self.mask(glyph, size, stripe_offset, transfer, grayscale).ok()
    }
    fn mask(
        &self,
        glyph: u16,
        size: f32,
        stripe_offset: i32,
        transfer: FontColorTransferFunction,
        grayscale: bool,
    ) -> windows_core::Result<Mask> {
        // The Windows ABI uses ManuallyDrop for interface fields. Balance the
        // retained face explicitly after the synchronous analysis call.
        let run = DWRITE_GLYPH_RUN {
            fontFace: ManuallyDrop::new(Some(self.face.clone())),
            fontEmSize: size,
            glyphCount: 1,
            glyphIndices: &glyph,
            ..Default::default()
        };
        // SAFETY: glyphIndices references a live u16, the face is retained, and
        // all output slices are checked against the rectangle returned by DW.
        let analysis = unsafe {
            if grayscale {
                self.registration.factory.cast::<IDWriteFactory2>().and_then(|factory| {
                    factory.CreateGlyphRunAnalysis(
                        &run,
                        None,
                        DWRITE_RENDERING_MODE_NATURAL,
                        DWRITE_MEASURING_MODE_NATURAL,
                        DWRITE_GRID_FIT_MODE_ENABLED,
                        DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE,
                        stripe_offset as f32 / 3.,
                        0.,
                    )
                })
            } else {
                self.registration.factory.CreateGlyphRunAnalysis(
                    &run,
                    1.,
                    None,
                    DWRITE_RENDERING_MODE_NATURAL,
                    DWRITE_MEASURING_MODE_NATURAL,
                    0.,
                    0.,
                )
            }
        };
        // Balance the clone stored in the ABI's ManuallyDrop field, on success
        // and failure, before propagating the result.
        let mut run = run;
        unsafe { ManuallyDrop::drop(&mut run.fontFace) };
        let analysis = analysis?;
        // DirectWrite's grayscale mode writes one coverage byte per pixel.
        // Taking just green from a ClearType mask retains its RGB stripe filter
        // and does not produce the native grayscale outline.
        let texture = if grayscale {
            DWRITE_TEXTURE_ALIASED_1x1
        } else {
            DWRITE_TEXTURE_CLEARTYPE_3x1
        };
        let bounds = unsafe { analysis.GetAlphaTextureBounds(texture)? };
        let width = usize::try_from(bounds.right - bounds.left).unwrap_or(0);
        let height = usize::try_from(bounds.bottom - bounds.top).unwrap_or(0);
        if width == 0 || height == 0 || width > 512 || height > 512 {
            return Err(windows_core::Error::from_hresult(windows_core::HRESULT(
                0x80070057u32 as i32,
            )));
        }
        let channels = if grayscale { 1 } else { 3 };
        let mut coverage = vec![0; width * height * channels];
        unsafe { analysis.CreateAlphaTexture(texture, &bounds, &mut coverage)? };
        if grayscale {
            return Ok(Mask {
                x: bounds.left,
                y: bounds.top,
                width,
                height,
                pixels: coverage
                    .into_iter()
                    .map(|value| {
                        let alpha = (transfer.alpha_from_coverage(f32::from(value) / 255.) * 255.)
                            .round() as u8;
                        Color32::from_rgba_premultiplied(alpha, alpha, alpha, alpha)
                    })
                    .collect(),
            });
        }
        let mut pixels = Vec::with_capacity(width * height);
        for row in coverage.chunks_exact(width * 3) {
            for x in 0..width {
                // JavaFX uses one DirectWrite mask and samples its packed RGB
                // channels at the quantized position. Pad at the glyph edge,
                // never sample into the previous or next row of the mask.
                let start = (x * 3) as i32 - stripe_offset;
                let rgb = [0, 1, 2].map(|i| {
                    let value = row.get((start + i) as usize).copied().unwrap_or(0);
                    (transfer.alpha_from_coverage(f32::from(value) / 255.) * 255.).round() as u8
                });
                pixels.push(super::lcd::coverage_color(rgb, grayscale));
            }
        }
        Ok(Mask {
            x: bounds.left,
            y: bounds.top,
            width,
            height,
            pixels,
        })
    }
}
