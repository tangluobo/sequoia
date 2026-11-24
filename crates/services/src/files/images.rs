//! Bounded image reads and decode policies shared by SFTP and object storage.
use anyhow::{Context, Result, ensure};
use image::ImageDecoder;
use std::sync::{Arc, LazyLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
pub use tokio_util::sync::CancellationToken;
mod animation;
mod raster;
mod svg;
pub use animation::{Animation, DocumentGif};
static LIMIT: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(2)));
static DECODE_LIMIT: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(2)));
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_RGBA: u64 = 64 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Thumbnail,
    Full,
    Document,
}
#[derive(Clone, Debug)]
pub struct Request {
    pub id: uuid::Uuid,
    pub path: String,
    pub mode: Mode,
    pub cancel: CancellationToken,
}
impl Request {
    pub fn new(path: String, mode: Mode) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            path,
            mode,
            cancel: CancellationToken::new(),
        }
    }
    pub async fn permit(&self) -> Result<OwnedSemaphorePermit> {
        tokio::select! {biased;_ = self.cancel.cancelled()=>anyhow::bail!("图片加载已取消"),permit=LIMIT.clone().acquire_owned()=>Ok(permit?)}
    }
    pub fn check(&self) -> Result<()> {
        ensure!(!self.cancel.is_cancelled(), "图片加载已取消");
        Ok(())
    }
}
pub(crate) fn try_permit() -> Option<OwnedSemaphorePermit> {
    DECODE_LIMIT.clone().try_acquire_owned().ok()
}
pub struct Asset {
    pub width: u32,
    pub height: u32,
    pub picture: crate::markdown::Picture,
    pub gif: Option<Arc<[u8]>>,
}
impl std::fmt::Debug for Asset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageAsset")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("rgbaBytes", &self.picture.rgba.len())
            .field("animated", &self.gif.is_some())
            .finish()
    }
}
pub fn supported(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| {
        [
            "jpg", "jpeg", "png", "gif", "bmp", "webp", "svg", "ico", "tiff", "tif",
        ]
        .iter()
        .any(|candidate| ext.eq_ignore_ascii_case(candidate))
    })
}
pub fn append(bytes: &mut Vec<u8>, chunk: &[u8]) -> Result<()> {
    let size = bytes.len().saturating_add(chunk.len());
    ensure!(size <= MAX_BYTES, "读取时图片增长超过 8 MiB");
    if size > bytes.capacity() {
        bytes.reserve_exact(size - bytes.len());
    }
    bytes.extend_from_slice(chunk);
    Ok(())
}
pub async fn decode(
    request: Request,
    bytes: Vec<u8>,
    permit: OwnedSemaphorePermit,
) -> Result<Asset> {
    ensure!(bytes.len() <= MAX_BYTES, "图片超过 8 MiB");
    request.check()?;
    let decoder = tokio::select! {biased;_=request.cancel.cancelled()=>anyhow::bail!("图片加载已取消"),permit=DECODE_LIMIT.clone().acquire_owned()=>permit?};
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let _decoder = decoder;
        request.check()?;
        if request
            .path
            .rsplit_once('.')
            .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("svg"))
        {
            return svg(&bytes, request.mode);
        }
        let mut decoded = raster::decode(&bytes, request.mode.into(), &request.cancel)?;
        request.check()?;
        let mut gif = None;
        if decoded.format == Some(image::ImageFormat::Gif) {
            match request.mode {
                Mode::Document => decoded.picture.gif = DocumentGif::new(bytes),
                Mode::Full => gif = Some(Arc::from(bytes)),
                Mode::Thumbnail => {}
            }
        }
        Ok(Asset {
            width: decoded.width,
            height: decoded.height,
            picture: decoded.picture,
            gif,
        })
    })
    .await?
}
pub(crate) async fn document(
    read: impl Future<Output = Result<Vec<u8>>>,
) -> Result<crate::markdown::Picture> {
    let permit = LIMIT.clone().acquire_owned().await?;
    let bytes = read.await?;
    ensure!(bytes.len() <= MAX_BYTES, "图片超过 8 MiB");
    let decoder = DECODE_LIMIT.clone().acquire_owned().await?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let _decoder = decoder;
        let mut raster =
            raster::decode(&bytes, raster::Target::Document, &CancellationToken::new())?;
        if raster.format == Some(image::ImageFormat::Gif) {
            raster.picture.gif = DocumentGif::new(bytes);
        }
        Ok(raster.picture)
    })
    .await?
}
fn svg(bytes: &[u8], mode: Mode) -> Result<Asset> {
    let tree = svg::parse(bytes)?;
    let size = tree.size();
    let width = size.width().ceil().max(1.) as u32;
    let height = size.height().ceil().max(1.) as u32;
    ensure!(
        width <= 8192 && height <= 8192 && u64::from(width) * u64::from(height) * 4 <= MAX_RGBA,
        "图片解码超过 64 MiB"
    );
    let long_edge = size.width().max(size.height());
    let scale = match mode {
        Mode::Thumbnail => 48. / long_edge,
        Mode::Full => 1.,
        Mode::Document => (1600. / long_edge).min(1.),
    };
    let out_width = (size.width() * scale).ceil().max(1.) as u32;
    let out_height = (size.height() * scale).ceil().max(1.) as u32;
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(out_width, out_height).context("无法分配图片内存")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let pixel = pixel.demultiply();
            [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]
        })
        .collect();
    Ok(Asset {
        width,
        height,
        picture: crate::markdown::Picture {
            width: out_width,
            height: out_height,
            rgba,
            gif: None,
        },
        gif: None,
    })
}

#[cfg(test)]
mod tests;
