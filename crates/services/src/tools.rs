use anyhow::{Context, Result, bail};
use std::path::Path;
pub mod conversion;
pub mod icon;
pub mod music;
pub mod shortcut;

pub fn convert_image(source: &Path, destination: &Path, width: u32) -> Result<()> {
    let control = crate::backup::Control::default();
    if source.is_dir() {
        let counts = conversion::svg::batch(source, destination, width, &control)?;
        if counts.failed > 0 {
            bail!("{}", conversion::Report::Images(counts).message());
        }
        return Ok(());
    }
    let target = if destination.is_dir() {
        destination
            .join(source.file_name().context("图片文件名为空")?)
            .with_extension("png")
    } else {
        destination.to_path_buf()
    };
    conversion::svg::Renderer::new().convert(source, &target, width, &control)
}

pub fn make_icon(source: &Path, destination: &Path, tolerance: u8, size: u32) -> Result<()> {
    if !(16..=1024).contains(&size) {
        bail!("图标边长必须在 16–1024 之间");
    }
    let image = icon::process(
        icon::load(source)?,
        &icon::Options {
            tolerance: tolerance.min(160),
            ..Default::default()
        },
    )?;
    let image = image::DynamicImage::ImageRgba8(image)
        .resize(size, size, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let mut square = image::RgbaImage::new(size, size);
    image::imageops::overlay(
        &mut square,
        &image,
        ((size - image.width()) / 2).into(),
        ((size - image.height()) / 2).into(),
    );
    square.save(destination)?;
    Ok(())
}

pub fn voc_to_coco(source: &Path, destination: &Path) -> Result<usize> {
    Ok(usize::try_from(
        conversion::voc::convert(source, destination, &crate::backup::Control::default())?.success,
    )?)
}

pub fn desktop_shortcut(
    destination: &Path,
    name: &str,
    executable: &Path,
    icon: &Path,
) -> Result<()> {
    if [name, &executable.to_string_lossy(), &icon.to_string_lossy()]
        .iter()
        .any(|s| s.contains(['\n', '\r']))
    {
        bail!("快捷方式字段不能包含换行");
    }
    let escaped = executable
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$")
        .replace('%', "%%");
    let content = format!(
        "[Desktop Entry]\nType=Application\nName={name}\nExec=\"{escaped}\"\nIcon={}\nTerminal=false\nCategories=Utility;\n",
        icon.display()
    );
    tomato_core::config::atomic_write(destination, content.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn dataset_conversion_builds_boxes_and_categories() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let output = directory.path().join("coco.json");
        std::fs::write(
            directory.path().join("test.xml"),
            "<annotation><filename>test.jpg</filename><size><width>100</width><height>80</height></size><object><name>cat</name><bndbox><xmin>10</xmin><ymin>20</ymin><xmax>40</xmax><ymax>60</ymax></bndbox></object></annotation>",
        )?;
        assert_eq!(voc_to_coco(directory.path(), &output)?, 1);
        let value: Value = serde_json::from_slice(&std::fs::read(output)?)?;
        assert_eq!(value["annotations"][0]["bbox"], json!([10, 20, 30, 40]));
        assert_eq!(value["categories"][0]["name"], "cat");
        Ok(())
    }
    #[test]
    fn svg_to_png_and_transparent_icon() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let svg = directory.path().join("test.svg");
        let png = directory.path().join("test.png");
        let icon = directory.path().join("icon.png");
        std::fs::write(
            &svg,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#ffffff"/><circle cx="20" cy="10" r="8" fill="#ff0000"/></svg>"##,
        )?;
        convert_image(&svg, &png, 80)?;
        assert_eq!(image::open(&png)?.width(), 80);
        make_icon(&png, &icon, 10, 64)?;
        let image = image::open(icon)?.to_rgba8();
        assert_eq!(image.width(), 64);
        assert_eq!(image.get_pixel(0, 0)[3], 0);
        Ok(())
    }
}
