//! Validate embedded resources before resvg can allocate their decoded canvases.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
pub(super) fn parse(bytes: &[u8]) -> Result<resvg::usvg::Tree> {
    validate_xml(bytes)?;
    let used = Arc::new(AtomicU64::new(0));
    let failed = Arc::new(AtomicBool::new(false));
    let rejected = failed.clone();
    let resolver = resvg::usvg::ImageHrefResolver::default_data_resolver();
    let mut options = resvg::usvg::Options::default();
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    options.image_href_resolver.resolve_data = Box::new(move |mime, data, options| {
        let checked = (|| -> Result<()> {
            ensure!(data.len() <= MAX_BYTES, "SVG 内嵌资源超过 8 MiB");
            ensure!(
                !data.starts_with(&[0x1f, 0x8b]),
                "不支持压缩的 SVG 内嵌资源"
            );
            if mime == "image/svg+xml"
                || data
                    .iter()
                    .copied()
                    .find(|byte| !byte.is_ascii_whitespace())
                    == Some(b'<')
            {
                validate_xml(&data)?;
            } else {
                let decoder = image::ImageReader::new(std::io::Cursor::new(data.as_slice()))
                    .with_guessed_format()?
                    .into_decoder()?;
                let (width, height) = decoder.dimensions();
                ensure!(width <= 8192 && height <= 8192, "SVG 内嵌图片尺寸过大");
                let bytes = u64::from(width) * u64::from(height) * 4;
                let previous = used.fetch_add(bytes, Ordering::Relaxed);
                ensure!(
                    previous.saturating_add(bytes) <= MAX_RGBA,
                    "SVG 内嵌图片总量超过 64 MiB"
                );
            }
            Ok(())
        })();
        if checked.is_err() {
            rejected.store(true, Ordering::Relaxed);
            return None;
        }
        resolver(mime, data, options)
    });
    let tree = resvg::usvg::Tree::from_data(bytes, &options)?;
    ensure!(
        !failed.load(Ordering::Relaxed),
        "SVG 内嵌资源超出限制或格式无效"
    );
    Ok(tree)
}
fn validate_xml(bytes: &[u8]) -> Result<()> {
    ensure!(bytes.len() <= MAX_BYTES, "SVG 超过 8 MiB");
    std::str::from_utf8(bytes).context("SVG 不是 UTF-8 文本")?;
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let (mut depth, mut nodes) = (0usize, 0usize);
    loop {
        use quick_xml::events::Event;
        match reader.read_event()? {
            Event::Start(_) => {
                depth += 1;
                nodes += 1;
                ensure!(depth <= 128, "SVG 嵌套超过 128 层");
            }
            Event::Empty(_) => nodes += 1,
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::DocType(declaration) => ensure!(
                !declaration.windows(8).any(|chunk| chunk == b"<!ENTITY"),
                "SVG 不支持自定义实体"
            ),
            Event::Eof => break,
            _ => {}
        }
        ensure!(nodes <= 25_000, "SVG 节点超过 25000 个");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn svg_rejects_compressed_entities_and_excessive_structure() {
        assert!(validate_xml(&[0x1f, 0x8b]).is_err());
        assert!(validate_xml(br#"<!DOCTYPE svg [<!ENTITY x "hello">]><svg/>"#).is_err());
        let deep = format!("{}{}", "<svg>".repeat(129), "</svg>".repeat(129));
        assert!(validate_xml(deep.as_bytes()).is_err());
        let nodes = format!("<svg>{}</svg>", "<g/>".repeat(25_000));
        assert!(validate_xml(nodes.as_bytes()).is_err());
    }
    #[test]
    fn svg_resource_header_budget_blocks_expansion_and_local_files() -> Result<()> {
        use base64::Engine as _;
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([255, 0, 0, 255]),
        ))
        .write_to(&mut bytes, image::ImageFormat::Png)?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes.get_ref());
        let xml = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="2" height="2"><image width="2" height="2" xlink:href="data:image/png;base64,{encoded}"/></svg>"#
        );
        let rendered = super::super::svg(xml.as_bytes(), Mode::Full)?;
        assert_eq!(&rendered.picture.rgba[..4], &[255, 0, 0, 255]);
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("private.png");
        std::fs::write(&path, bytes.get_ref())?;
        let xml = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="2" height="2"><image width="2" height="2" xlink:href="{}"/></svg>"#,
            path.display()
        );
        let rendered = super::super::svg(xml.as_bytes(), Mode::Full)?;
        assert!(rendered.picture.rgba.iter().all(|byte| *byte == 0));
        Ok(())
    }
}
