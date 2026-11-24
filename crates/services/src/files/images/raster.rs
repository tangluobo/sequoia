//! Source dimensions and display dimensions have separate budgets.
use super::*;
mod png;

const MAX_SOURCE_PIXELS: u64 = 256 * 1024 * 1024;
const MAX_TEXTURE_EDGE: u32 = 8192;

#[derive(Clone, Copy)]
pub(super) enum Target {
    Thumbnail,
    Preview,
    Document,
}
impl From<Mode> for Target {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Thumbnail => Self::Thumbnail,
            Mode::Full => Self::Preview,
            Mode::Document => Self::Document,
        }
    }
}
impl Target {
    pub(super) fn dimensions(self, width: u32, height: u32) -> Result<(u32, u32)> {
        ensure!(width > 0 && height > 0, "图片尺寸无效");
        let pixels = u64::from(width) * u64::from(height);
        ensure!(pixels <= MAX_SOURCE_PIXELS, "图片超过 256 Mi 像素预览预算");
        let edge = match self {
            Self::Thumbnail => 48,
            Self::Preview => MAX_TEXTURE_EDGE,
            Self::Document => 1600,
        };
        let mut scale = f64::from(edge) / f64::from(width.max(height));
        if !matches!(self, Self::Thumbnail) {
            scale = scale.min(1.);
        }
        scale = scale.min((MAX_RGBA as f64 / (pixels as f64 * 4.)).sqrt());
        Ok((
            (f64::from(width) * scale).floor().max(1.) as u32,
            (f64::from(height) * scale).floor().max(1.) as u32,
        ))
    }
}

pub(super) struct Raster {
    pub width: u32,
    pub height: u32,
    pub format: Option<image::ImageFormat>,
    pub picture: crate::markdown::Picture,
}

pub(super) fn decode(bytes: &[u8], target: Target, cancel: &CancellationToken) -> Result<Raster> {
    ensure!(!cancel.is_cancelled(), "图片加载已取消");
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let format = reader.format();
    if format == Some(image::ImageFormat::Png)
        && let Some(scaled) = png::scaled(bytes, target, cancel)?
    {
        return Ok(scaled);
    }
    // A long, narrow image need not allocate much memory. Do not reject it just
    // because an edge exceeds the texture limit; bound the decoded allocation.
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_RGBA);
    reader.limits(limits);
    let decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    let (out_width, out_height) = target.dimensions(width, height)?;
    ensure!(
        decoder.total_bytes() <= MAX_RGBA && u64::from(width) * u64::from(height) * 4 <= MAX_RGBA,
        "此格式需要完整解码，超过 64 MiB 内存预算"
    );
    let decoded = image::DynamicImage::from_decoder(decoder)?;
    ensure!(!cancel.is_cancelled(), "图片加载已取消");
    let image = if (width, height) == (out_width, out_height) {
        decoded
    } else {
        decoded.resize_exact(out_width, out_height, image::imageops::FilterType::Triangle)
    }
    .into_rgba8();
    Ok(Raster {
        width,
        height,
        format,
        picture: crate::markdown::Picture {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
            gif: None,
        },
    })
}
