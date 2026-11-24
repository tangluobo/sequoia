use super::{Counts, directory::Files};
use crate::backup::Control;
use anyhow::{Context as _, Result, ensure};
use std::{io::Read as _, path::Path, sync::Arc};

pub(in crate::tools) struct Renderer {
    fonts: Option<Arc<resvg::usvg::fontdb::Database>>,
}
impl Renderer {
    pub(in crate::tools) fn new() -> Self {
        Self { fonts: None }
    }
    pub(in crate::tools) fn convert(
        &mut self,
        source: &Path,
        destination: &Path,
        width: u32,
        control: &Control,
    ) -> Result<()> {
        control.check()?;
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        let temporary = tempfile::Builder::new()
            .prefix(".tomato-convert-")
            .suffix(".png")
            .tempfile_in(parent)?;
        if source
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
        {
            let mut bytes = Vec::new();
            std::fs::File::open(source)?
                .take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 8 * 1024 * 1024, "SVG 文件超过8 MiB");
            let fonts = self.fonts.get_or_insert_with(|| {
                let mut db = resvg::usvg::fontdb::Database::new();
                db.load_system_fonts();
                Arc::new(db)
            });
            let options = resvg::usvg::Options {
                resources_dir: source.parent().map(Path::to_path_buf),
                fontdb: fonts.clone(),
                ..Default::default()
            };
            let tree = resvg::usvg::Tree::from_data(&bytes, &options)?;
            let size = tree.size();
            let scale = if width == 0 {
                1.0
            } else {
                width as f32 / size.width()
            };
            let w = (size.width() * scale).round().max(1.0) as u32;
            let h = (size.height() * scale).round().max(1.0) as u32;
            ensure!(
                u64::from(w) * u64::from(h) <= 64_000_000,
                "图片超过6400万像素限制"
            );
            control.check()?;
            let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).context("无法分配图片内存")?;
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::from_scale(scale, scale),
                &mut pixmap.as_mut(),
            );
            control.check()?;
            if destination
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
            {
                pixmap.save_png(temporary.path())?;
            } else {
                let image = image::load_from_memory(&pixmap.encode_png()?)?;
                image.save_with_format(
                    temporary.path(),
                    image::ImageFormat::from_path(destination)?,
                )?;
            }
        } else {
            let mut reader = image::ImageReader::open(source)?.with_guessed_format()?;
            let mut limits = image::Limits::default();
            limits.max_alloc = Some(256 * 1024 * 1024);
            reader.limits(limits);
            let image = reader.decode()?;
            ensure!(
                u64::from(image.width()) * u64::from(image.height()) <= 64_000_000,
                "图片超过6400万像素限制"
            );
            let image = if width > 0 && image.width() != width {
                image.resize(width, u32::MAX, image::imageops::FilterType::Lanczos3)
            } else {
                image
            };
            control.check()?;
            image.save_with_format(
                temporary.path(),
                image::ImageFormat::from_path(destination)?,
            )?;
        }
        control.check()?;
        temporary.as_file().sync_all()?;
        temporary.persist(destination).map_err(|e| e.error)?;
        Ok(())
    }
}
pub(in crate::tools) fn batch(
    source: &Path,
    directory: &Path,
    width: u32,
    control: &Control,
) -> Result<Counts> {
    control.check()?;
    ensure!(
        source.is_dir() || source.is_file(),
        "请选择有效的源文件或目录"
    );
    std::fs::create_dir_all(directory)?;
    let mut renderer = Renderer::new();
    let mut counts = Counts::default();
    let mut convert = |path: &Path| -> Result<()> {
        control.check()?;
        let relative = if source.is_dir() {
            path.strip_prefix(source)?
        } else {
            path.file_name().context("图片文件名为空")?.as_ref()
        };
        let target = directory.join(relative).with_extension("png");
        counts.total += 1;
        match renderer.convert(path, &target, width, control) {
            Ok(()) => counts.success += 1,
            Err(_) if !control.cancelled() => counts.failed += 1,
            Err(error) => return Err(error),
        }
        Ok(())
    };
    if source.is_dir() {
        let mut files = Files::new(source, "svg")?;
        while let Some(path) = files.next(control)? {
            convert(&path)?;
        }
    } else {
        convert(source)?;
    }
    if counts.total == 0 {
        return Err(super::NoImages.into());
    }
    Ok(counts)
}
