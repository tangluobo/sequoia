//! Streaming GIF playback: retain the decoder canvas and the latest frame only.
use super::*;
use image::{AnimationDecoder, codecs::gif::GifDecoder};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
struct Shared {
    cancel: AtomicBool,
    done: AtomicBool,
    frame: Mutex<Option<crate::markdown::Picture>>,
    error: Mutex<Option<String>>,
}
pub struct Animation {
    shared: Arc<Shared>,
    thread: thread::JoinHandle<()>,
}
static DOCUMENT_PLAYERS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(2)));

pub struct DocumentGif {
    bytes: Arc<[u8]>,
    info: Info,
}
impl std::fmt::Debug for DocumentGif {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentGif")
            .field("bytes", &self.bytes.len())
            .field("frames", &self.info.frames)
            .finish()
    }
}

impl DocumentGif {
    pub(super) fn new(bytes: Vec<u8>) -> Option<Self> {
        let info = info(&bytes).ok()?;
        (info.frames > 1).then(|| Self {
            bytes: Arc::from(bytes),
            info,
        })
    }

    pub fn retained_bytes(&self) -> usize {
        self.bytes.len()
    }

    pub fn try_start(&self, wake: impl Fn() + Send + Sync + 'static) -> Result<Option<Animation>> {
        let Ok(permit) = DOCUMENT_PLAYERS.clone().try_acquire_owned() else {
            return Ok(None);
        };
        Animation::spawn(self.bytes.clone(), self.info, Some(permit), wake).map(Some)
    }
}

impl Animation {
    pub fn start(bytes: Arc<[u8]>, wake: impl Fn() + Send + Sync + 'static) -> Result<Self> {
        let info = info(&bytes)?;
        Self::spawn(bytes, info, None, wake)
    }

    fn spawn(
        bytes: Arc<[u8]>,
        info: Info,
        document_player: Option<OwnedSemaphorePermit>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        let shared = Arc::new(Shared {
            cancel: AtomicBool::new(false),
            done: AtomicBool::new(false),
            frame: Mutex::new(None),
            error: Mutex::new(None),
        });
        let state = shared.clone();
        let thread = thread::Builder::new()
            .name("tomato-image-animation".into())
            .stack_size(512 * 1024)
            .spawn(move || {
                let document = document_player.is_some();
                let result = play(bytes, info, &state, &wake, document);
                drop(document_player);
                if let Err(error) = result
                    && let Ok(mut last) = state.error.lock()
                {
                    *last = Some(format!("图片加载失败: {error:#}"));
                }
                state.done.store(true, Ordering::Release);
                if document || !state.cancel.load(Ordering::Acquire) {
                    wake();
                }
            })?;
        Ok(Self { shared, thread })
    }
    pub fn frame(&self) -> Option<crate::markdown::Picture> {
        self.shared.frame.lock().ok()?.take()
    }
    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().ok()?.take()
    }
    pub fn is_finished(&self) -> bool {
        self.shared.done.load(Ordering::Acquire)
    }
}
impl Drop for Animation {
    fn drop(&mut self) {
        self.shared.cancel.store(true, Ordering::Release);
        self.thread.thread().unpark();
    }
}
fn wait(state: &Shared, delay: Duration) {
    let end = Instant::now() + delay;
    while !state.cancel.load(Ordering::Acquire) {
        let Some(left) = end.checked_duration_since(Instant::now()) else {
            break;
        };
        thread::park_timeout(left);
    }
}
fn play(
    bytes: Arc<[u8]>,
    info: Info,
    state: &Shared,
    wake: &impl Fn(),
    document: bool,
) -> Result<()> {
    if info.frames < 2 {
        return Ok(());
    }
    let mut cycle = 0;
    while info.cycles == 0 || cycle < info.cycles {
        let mut decoder = GifDecoder::new(std::io::Cursor::new(bytes.clone()))?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(MAX_RGBA);
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        decoder.set_limits(limits)?;
        let mut frames = decoder.into_frames();
        let mut index = 0;
        loop {
            if state.cancel.load(Ordering::Acquire) {
                return Ok(());
            }
            let permit = loop {
                if state.cancel.load(Ordering::Acquire) {
                    return Ok(());
                }
                if let Some(permit) = super::try_permit() {
                    break permit;
                }
                thread::park_timeout(Duration::from_millis(10));
            };
            let next = frames.next();
            let Some(frame) = next else { break };
            let frame = frame?;
            let (numer, denom) = frame.delay().numer_denom_ms();
            // JavaFX 27: zero-delay frames use 25 FPS; nonzero GIF delays are unchanged.
            let delay = if numer == 0 {
                Duration::from_millis(40)
            } else {
                Duration::from_secs_f64(f64::from(numer) / f64::from(denom) / 1000.)
            };
            if document || cycle != 0 || index != 0 {
                let mut image = frame.into_buffer();
                ensure!(
                    u64::from(image.width()) * u64::from(image.height()) * 4 <= MAX_RGBA,
                    "图片帧超过 64 MiB"
                );
                if document {
                    let (width, height) = super::raster::Target::Document
                        .dimensions(image.width(), image.height())?;
                    if (width, height) != image.dimensions() {
                        image = image::imageops::resize(
                            &image,
                            width,
                            height,
                            image::imageops::FilterType::Triangle,
                        );
                    }
                }
                if state.cancel.load(Ordering::Acquire) {
                    return Ok(());
                }
                let picture = crate::markdown::Picture {
                    width: image.width(),
                    height: image.height(),
                    rgba: image.into_raw(),
                    gif: None,
                };
                if let Ok(mut latest) = state.frame.lock() {
                    *latest = Some(picture);
                }
                wake();
            } else {
                drop(frame);
            }
            drop(permit);
            index += 1;
            wait(state, delay);
        }
        if index == 0 {
            break;
        }
        cycle += 1;
    }
    Ok(())
}
#[derive(Clone, Copy, Debug)]
struct Info {
    frames: usize,
    cycles: u32,
}
fn info(bytes: &[u8]) -> Result<Info> {
    ensure!(
        bytes.len() >= 13 && matches!(&bytes[..6], b"GIF87a" | b"GIF89a"),
        "GIF 文件头无效"
    );
    let mut at = 13
        + if bytes[10] & 128 != 0 {
            3 * (2usize << usize::from(bytes[10] & 7))
        } else {
            0
        };
    let mut frames = 0;
    let mut cycles = 1;
    let mut current_cycles = 1;
    while at < bytes.len() {
        let marker = bytes[at];
        at += 1;
        match marker {
            0x3b => break,
            0x2c => {
                ensure!(at + 9 <= bytes.len(), "GIF 图片描述无效");
                let flags = bytes[at + 8];
                at += 9;
                if flags & 128 != 0 {
                    at += 3 * (2usize << usize::from(flags & 7));
                }
                ensure!(at < bytes.len(), "GIF 数据缺失");
                at += 1;
                skip(bytes, &mut at)?;
                frames += 1;
                cycles = current_cycles;
            }
            0x21 => {
                let label = *bytes.get(at).context("GIF 扩展缺失")?;
                at += 1;
                if label == 0xff {
                    let len = usize::from(*bytes.get(at).context("GIF 扩展长度缺失")?);
                    at += 1;
                    let app = bytes.get(at..at + len).context("GIF 扩展越界")?;
                    at += len;
                    if app == b"NETSCAPE2.0" && bytes.get(at..at + 2) == Some(&[3, 1]) {
                        let count = bytes.get(at + 2..at + 4).context("GIF 循环次数缺失")?;
                        current_cycles = u32::from(u16::from_le_bytes([count[0], count[1]]));
                    }
                }
                skip(bytes, &mut at)?;
            }
            _ => anyhow::bail!("GIF 数据块无效"),
        }
    }
    ensure!(frames > 0, "GIF 没有图片帧");
    // JavaFX uses the metadata cycle count directly, rather than adding a cycle.
    Ok(Info { frames, cycles })
}
fn skip(bytes: &[u8], at: &mut usize) -> Result<()> {
    loop {
        let size = usize::from(*bytes.get(*at).context("GIF 数据块缺失")?);
        *at += 1;
        if size == 0 {
            break;
        }
        *at = at.checked_add(size).context("GIF 数据块越界")?;
        ensure!(*at <= bytes.len(), "GIF 数据块越界");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{
        Delay, Frame, Rgba, RgbaImage,
        codecs::gif::{GifEncoder, Repeat},
    };
    fn gif(repeat: Option<Repeat>) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut bytes);
            if let Some(repeat) = repeat {
                encoder.set_repeat(repeat)?;
            }
            for color in [[255, 0, 0, 255], [0, 255, 0, 255]] {
                encoder.encode_frame(Frame::from_parts(
                    RgbaImage::from_pixel(2, 2, Rgba(color)),
                    0,
                    0,
                    Delay::from_numer_denom_ms(0, 1),
                ))?;
            }
        }
        Ok(bytes)
    }
    #[test]
    fn java_cycles_and_container_validation() -> Result<()> {
        for (repeat, expected) in [
            (None, 1),
            (Some(Repeat::Finite(2)), 2),
            (Some(Repeat::Infinite), 0),
        ] {
            let bytes = gif(repeat)?;
            let parsed = info(&bytes)?;
            assert_eq!(parsed.frames, 2);
            assert_eq!(parsed.cycles, expected);
            assert!(info(&bytes[..bytes.len() / 2]).is_err());
        }
        Ok(())
    }
    #[test]
    fn streaming_playback_uses_default_delay_and_latest_frame_without_collecting() -> Result<()> {
        let bytes = gif(None)?;
        let start = Instant::now();
        let animation = Animation::start(Arc::from(bytes), || {})?;
        while !animation.is_finished() && start.elapsed() < Duration::from_secs(2) {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(animation.is_finished());
        assert!(animation.error().is_none());
        assert!(start.elapsed() >= Duration::from_millis(70));
        let frame = animation.frame().context("second frame missing")?;
        assert_eq!(&frame.rgba[..4], &[0, 255, 0, 255]);
        assert!(animation.frame().is_none());
        Ok(())
    }
    #[test]
    fn dropping_infinite_animation_stops_worker_and_releases_shared_frames() -> Result<()> {
        let animation = Animation::start(Arc::from(gif(Some(Repeat::Infinite))?), || {})?;
        let shared = Arc::downgrade(&animation.shared);
        drop(animation);
        let start = Instant::now();
        while shared.strong_count() != 0 && start.elapsed() < Duration::from_secs(2) {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(shared.strong_count(), 0);
        Ok(())
    }

    #[test]
    fn visible_document_animation_streams_frames_and_releases_its_player_slot() -> Result<()> {
        let gif = DocumentGif::new(gif(Some(Repeat::Infinite))?).context("animated fixture")?;
        let first = gif.try_start(|| {})?.context("first player")?;
        let second = gif.try_start(|| {})?.context("second player")?;
        assert!(gif.try_start(|| {})?.is_none());
        let mut colors = std::collections::BTreeSet::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while colors.len() < 2 && Instant::now() < deadline {
            if let Some(frame) = first.frame() {
                colors.insert(frame.rgba[0]);
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(colors, [0, 255].into_iter().collect());
        drop(first);
        let deadline = Instant::now() + Duration::from_secs(2);
        while DOCUMENT_PLAYERS.available_permits() == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(gif.try_start(|| {})?.is_some());
        drop(second);
        Ok(())
    }
}
