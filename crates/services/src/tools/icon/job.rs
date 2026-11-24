use super::*;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

pub struct Thumbnail {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}
pub struct Preview {
    pub source_hash: [u8; 32],
    pub original: Thumbnail,
    pub processed: Thumbnail,
    pub original_size: [u32; 2],
    pub processed_size: [u32; 2],
}
pub enum Action {
    Preview,
    Save(Option<[u8; 32]>),
}
pub enum Output {
    Directory(usize),
    Preview(Box<Preview>),
    Saved {
        total: usize,
        success: usize,
        failed: usize,
    },
}
pub struct Job {
    receiver: mpsc::Receiver<Result<Output>>,
    cancel: Arc<AtomicBool>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Job {
    pub fn start(
        source: PathBuf,
        options: Options,
        action: Action,
        wake: impl FnOnce() + Send + 'static,
    ) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        std::thread::spawn(move || {
            let result = (|| {
                if let Action::Save(expected) = action {
                    let directory = source.is_dir();
                    let paths = if directory {
                        sources(&source)?
                    } else {
                        vec![source.clone()]
                    };
                    ensure!(paths.len() <= 10000, "单次最多处理 10000 个文件");
                    ensure!(!paths.is_empty(), "未找到支持的图片文件");
                    let total = paths.len();
                    let (mut success, mut failed) = (0, 0);
                    let mut options = options;
                    if directory {
                        options.margins = [0; 4];
                    } else {
                        // Java saves the currently displayed single-file image;
                        // automatic border crop belongs to directory processing.
                        options.auto_crop = false;
                    }
                    for path in paths {
                        ensure!(!stop.load(Ordering::Relaxed), "已取消");
                        let mut name = path
                            .file_stem()
                            .ok_or_else(|| anyhow::anyhow!("无效文件名"))?
                            .to_os_string();
                        name.push("_transparent.png");
                        let target = path.with_file_name(name);
                        match save_checked(
                            &path,
                            &target,
                            &options,
                            if directory { None } else { expected },
                        ) {
                            Ok(()) => success += 1,
                            Err(error) => {
                                if !directory {
                                    return Err(error);
                                }
                                failed += 1;
                            }
                        }
                    }
                    Ok(Output::Saved {
                        total,
                        success,
                        failed,
                    })
                } else {
                    if source.is_dir() {
                        return Ok(Output::Directory(sources(&source)?.len()));
                    }
                    ensure!(supported(&source), "请选择 JPG/JPEG/PNG 图片文件");
                    let stamp = std::fs::metadata(&source)?;
                    let source_hash = file_hash(&source)?;
                    let original = load(&source)?;
                    let original_size = [original.width(), original.height()];
                    let before = thumbnail(&original, false);
                    ensure!(!stop.load(Ordering::Relaxed), "已取消");
                    let mut options = options;
                    options.auto_crop = false;
                    let processed = process(original, &options)?;
                    let current = std::fs::metadata(&source)?;
                    ensure!(
                        stamp.len() == current.len() && stamp.modified()? == current.modified()?,
                        "源图片已被修改，请重新选择文件"
                    );
                    let processed_size = [processed.width(), processed.height()];
                    Ok(Output::Preview(Box::new(Preview {
                        source_hash,
                        original: before,
                        processed: thumbnail(&processed, true),
                        original_size,
                        processed_size,
                    })))
                }
            })();
            if !stop.load(Ordering::Relaxed) {
                let _ = sender.send(result);
                wake();
            }
        });
        Self { receiver, cancel }
    }
    pub fn poll(&self) -> Option<Result<Output>> {
        self.receiver.try_recv().ok()
    }
}
fn thumbnail(image: &RgbaImage, checker: bool) -> Thumbnail {
    let view = PreviewImage { image, checker };
    let thumb = image::imageops::thumbnail(&view, image.width().min(640), image.height().min(640));
    Thumbnail {
        size: [thumb.width() as usize, thumb.height() as usize],
        rgba: thumb.into_raw(),
    }
}
struct PreviewImage<'a> {
    image: &'a RgbaImage,
    checker: bool,
}
impl image::GenericImageView for PreviewImage<'_> {
    type Pixel = image::Rgba<u8>;
    fn dimensions(&self) -> (u32, u32) {
        self.image.dimensions()
    }
    fn get_pixel(&self, x: u32, y: u32) -> Self::Pixel {
        let pixel = self.image.get_pixel(x, y);
        let alpha = u32::from(pixel[3]);
        let background = if self.checker {
            if (x / 10 + y / 10).is_multiple_of(2) {
                255
            } else {
                224
            }
        } else {
            0
        };
        image::Rgba(std::array::from_fn(|channel| {
            if channel == 3 {
                if self.checker { 255 } else { pixel[3] }
            } else {
                ((u32::from(pixel[channel]) * alpha + background * (255 - alpha) + 127) / 255) as u8
            }
        }))
    }
}
fn supported(path: &Path) -> bool {
    path.is_file()
        && path.extension().is_some_and(|ext| {
            ["png", "jpg", "jpeg"]
                .iter()
                .any(|s| ext.eq_ignore_ascii_case(s))
        })
}
fn sources(root: &Path) -> Result<Vec<PathBuf>> {
    let mut stack = vec![(root.to_owned(), 0)];
    let mut paths = Vec::new();
    let mut visited = 0;
    while let Some((directory, depth)) = stack.pop() {
        ensure!(depth <= 32, "图片目录超过 32 层");
        for entry in std::fs::read_dir(directory)? {
            visited += 1;
            ensure!(visited <= 50000, "单次最多扫描 50000 个目录项");
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                stack.push((path, depth + 1));
            } else if supported(&path) {
                paths.push(path);
                ensure!(paths.len() <= 10000, "单次最多处理 10000 个文件");
            }
        }
    }
    paths.sort();
    Ok(paths)
}
