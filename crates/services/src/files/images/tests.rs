use super::*;
use std::io::Cursor;

fn png(width: u32, height: u32) -> Result<Vec<u8>> {
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        width,
        height,
        image::Rgba([12, 34, 56, 128]),
    ))
    .write_to(&mut bytes, image::ImageFormat::Png)?;
    Ok(bytes.into_inner())
}

async fn read(path: &str, mode: Mode, bytes: Vec<u8>) -> Result<Asset> {
    let request = Request::new(path.into(), mode);
    let permit = request.permit().await?;
    decode(request, bytes, permit).await
}

#[tokio::test]
async fn thumbnails_upscale_preserve_aspect_alpha_and_full_size() -> Result<()> {
    let bytes = png(4, 2)?;
    let thumb = read("中文.PNG", Mode::Thumbnail, bytes.clone()).await?;
    assert_eq!((thumb.width, thumb.height), (4, 2));
    assert_eq!((thumb.picture.width, thumb.picture.height), (48, 24));
    assert_eq!(&thumb.picture.rgba[..4], &[12, 34, 56, 128]);
    let full = read("中文.PNG", Mode::Full, bytes).await?;
    assert_eq!((full.picture.width, full.picture.height), (4, 2));
    assert!(full.gif.is_none());
    Ok(())
}

#[tokio::test]
async fn svg_renders_alpha_and_rejects_excessive_canvas() -> Result<()> {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4"><rect width="8" height="4" fill="#ff0000" fill-opacity="0.5"/></svg>"##;
    let asset = read("sample.svg", Mode::Thumbnail, svg.to_vec()).await?;
    assert_eq!((asset.picture.width, asset.picture.height), (48, 24));
    assert_eq!(&asset.picture.rgba[..4], &[255, 0, 0, 128]);
    let huge = br#"<svg xmlns="http://www.w3.org/2000/svg" width="8192" height="8192"/>"#;
    assert!(
        read("huge.svg", Mode::Thumbnail, huge.to_vec())
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn cancellation_corruption_and_growing_reads_are_bounded() -> Result<()> {
    let request = Request::new("cancel.png".into(), Mode::Full);
    request.cancel.cancel();
    assert!(request.permit().await.is_err());
    assert!(
        read("broken.png", Mode::Full, b"broken".to_vec())
            .await
            .is_err()
    );
    let mut bytes = vec![0; MAX_BYTES - 1];
    append(&mut bytes, &[1])?;
    assert!(append(&mut bytes, &[2]).is_err());
    assert_eq!(bytes.len(), MAX_BYTES);
    assert!(supported("photo.TIFF"));
    assert!(!supported("photo.png.exe"));
    Ok(())
}

fn scanline_png(width: u32, height: u32) -> Result<Vec<u8>> {
    use std::io::Write;
    let mut bytes = Vec::new();
    let mut encoder = ::png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(::png::ColorType::Rgba);
    encoder.set_depth(::png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    {
        let mut stream = writer.stream_writer()?;
        let row = [12, 34, 56, 128].repeat(width as usize);
        for _ in 0..height {
            stream.write_all(&row)?;
        }
        stream.finish()?;
    }
    writer.finish()?;
    Ok(bytes)
}

#[tokio::test]
async fn large_pngs_stream_to_thumbnail_preview_and_markdown_with_bounded_output() -> Result<()> {
    // Both old guards fail: an edge > 8192 and an RGBA canvas > 64 MiB.
    // The fixture itself is also encoded scanline by scanline.
    let bytes = scanline_png(9000, 2000)?;
    assert!(bytes.len() < 1024 * 1024);
    for mode in [Mode::Thumbnail, Mode::Full] {
        let asset = read("大图.png", mode, bytes.clone()).await?;
        assert_eq!((asset.width, asset.height), (9000, 2000));
        let picture = asset.picture;
        assert!(u64::from(picture.width) * u64::from(picture.height) * 4 <= MAX_RGBA);
        assert!(picture.width <= 8192 && picture.height <= 8192);
        assert_eq!(
            picture.rgba.len(),
            picture.width as usize * picture.height as usize * 4
        );
        assert_eq!(&picture.rgba[..4], &[12, 34, 56, 128]);
        assert_eq!(&picture.rgba[picture.rgba.len() - 4..], &[12, 34, 56, 128]);
        if mode == Mode::Thumbnail {
            assert_eq!((picture.width, picture.height), (48, 10));
        }
    }
    let picture = document(std::future::ready(Ok(bytes))).await?;
    assert_eq!((picture.width, picture.height), (1600, 355));
    assert_eq!(&picture.rgba[..4], &[12, 34, 56, 128]);
    Ok(())
}

#[tokio::test]
async fn streaming_png_checks_crc_and_dimensions_before_returning_pixels() -> Result<()> {
    let mut bytes = scanline_png(9000, 2)?;
    bytes.truncate(bytes.len() - 8);
    assert!(read("truncated.png", Mode::Thumbnail, bytes).await.is_err());
    let mut bytes = Vec::new();
    // A valid IHDR advertising excessive decompression work must be rejected
    // without creating a source canvas or reading any pixel rows.
    let encoder = ::png::Encoder::new(&mut bytes, 300_000, 1000);
    drop(encoder.write_header()?);
    let error = read("bomb.png", Mode::Thumbnail, bytes).await.unwrap_err();
    assert!(error.to_string().contains("256 Mi"));
    Ok(())
}
