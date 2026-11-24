//! Area resampling of ordinary PNG scanlines without a full source canvas.
use super::*;
use ::png::{BitDepth, ColorType, Decoder, Limits, Transformations};

pub(super) fn scaled(
    bytes: &[u8],
    target: Target,
    cancel: &CancellationToken,
) -> Result<Option<Raster>> {
    let mut decoder = Decoder::new_with_limits(
        std::io::Cursor::new(bytes),
        Limits {
            bytes: MAX_RGBA as usize,
        },
    );
    decoder.set_ignore_text_chunk(true);
    decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
    let info = decoder.read_header_info()?;
    let (width, height) = (info.width, info.height);
    let (out_width, out_height) = target.dimensions(width, height)?;
    // Adam7 needs a canvas; the regular decoder retains its bounded fallback.
    // Small images retain the original interpolating resize, including upscaling.
    if info.interlaced || (out_width >= width && out_height >= height) {
        return Ok(None);
    }
    let mut reader = decoder.read_info()?;
    let (color, depth) = reader.output_color_type();
    ensure!(depth == BitDepth::Eight, "PNG 颜色深度无效");
    let channels = color.samples();
    ensure!(color != ColorType::Indexed, "PNG 调色板未展开");
    let output_bytes = u64::from(out_width) * u64::from(out_height) * 4;
    ensure!(output_bytes <= MAX_RGBA, "图片预览超过 64 MiB 内存预算");
    let stride = out_width as usize * 4;
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(output_bytes as usize)?;
    let mut horizontal = vec![[0f32; 4]; out_width as usize];
    let mut vertical = vec![[0f32; 4]; out_width as usize];
    let mut output_y = 0u32;
    for source_y in 0..height {
        ensure!(!cancel.is_cancelled(), "图片加载已取消");
        let row = reader.next_row()?.context("PNG 图片数据不完整")?;
        ensure!(
            row.data().len() == width as usize * channels,
            "PNG 行尺寸无效"
        );
        resample_row(row.data(), color, width, out_width, &mut horizontal);
        let mut top = u64::from(source_y) * u64::from(out_height);
        let bottom = u64::from(source_y + 1) * u64::from(out_height);
        while top < bottom {
            let output_bottom = u64::from(output_y + 1) * u64::from(height);
            let end = bottom.min(output_bottom);
            let weight = (end - top) as f32;
            for (sum, pixel) in vertical.iter_mut().zip(&horizontal) {
                for channel in 0..4 {
                    sum[channel] += pixel[channel] * weight;
                }
            }
            if end == output_bottom {
                for sum in &mut vertical {
                    let alpha = sum[3];
                    for value in &sum[..3] {
                        rgba.push(if alpha > 0. {
                            (value / alpha).round() as u8
                        } else {
                            0
                        });
                    }
                    rgba.push((alpha / height as f32).round() as u8);
                    *sum = [0.; 4];
                }
                output_y += 1;
            }
            top = end;
        }
    }
    ensure!(
        output_y == out_height && rgba.len() == stride * out_height as usize,
        "PNG 图片数据不完整"
    );
    reader.finish()?;
    Ok(Some(Raster {
        width,
        height,
        format: Some(image::ImageFormat::Png),
        picture: crate::markdown::Picture {
            gif: None,
            width: out_width,
            height: out_height,
            rgba,
        },
    }))
}

fn resample_row(row: &[u8], color: ColorType, width: u32, out_width: u32, output: &mut [[f32; 4]]) {
    let channels = color.samples();
    for (x, pixel) in output.iter_mut().enumerate() {
        let left = x as u64 * u64::from(width);
        let right = (x as u64 + 1) * u64::from(width);
        let start = left / u64::from(out_width);
        let end = right.div_ceil(u64::from(out_width));
        let mut sum = [0f32; 4];
        for source_x in start..end {
            let at = source_x as usize * channels;
            let rgba = match color {
                ColorType::Grayscale => [row[at], row[at], row[at], 255],
                ColorType::GrayscaleAlpha => [row[at], row[at], row[at], row[at + 1]],
                ColorType::Rgb => [row[at], row[at + 1], row[at + 2], 255],
                ColorType::Rgba => [row[at], row[at + 1], row[at + 2], row[at + 3]],
                ColorType::Indexed => unreachable!("palette was expanded"),
            };
            let weight = (((source_x + 1) * u64::from(out_width)).min(right)
                - (source_x * u64::from(out_width)).max(left)) as f32;
            let alpha = f32::from(rgba[3]);
            for channel in 0..3 {
                sum[channel] += f32::from(rgba[channel]) * alpha * weight;
            }
            sum[3] += alpha * weight;
        }
        for channel in 0..4 {
            pixel[channel] = sum[channel] / width as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_filter_weights_edges_and_premultiplies_alpha_without_color_bleed() {
        let mut output = [[0.; 4]; 1];
        resample_row(
            &[255, 0, 0, 0, 0, 0, 255, 255],
            ColorType::Rgba,
            2,
            1,
            &mut output,
        );
        assert_eq!(output[0], [0., 0., 32512.5, 127.5]);
        let mut output = [[0.; 4]; 2];
        resample_row(&[0, 90, 180], ColorType::Grayscale, 3, 2, &mut output);
        assert_eq!(output[0], [30. * 255., 30. * 255., 30. * 255., 255.]);
        assert_eq!(output[1], [150. * 255., 150. * 255., 150. * 255., 255.]);
    }

    #[test]
    fn scanline_transform_expands_palette_transparency_and_strips_sixteen_bit_samples() -> Result<()>
    {
        let mut palette = Vec::new();
        {
            let mut encoder = ::png::Encoder::new(&mut palette, 9000, 2);
            encoder.set_color(ColorType::Indexed);
            encoder.set_depth(BitDepth::One);
            encoder.set_palette(vec![255, 0, 0, 0, 0, 255]);
            encoder.set_trns(vec![0, 255]);
            encoder
                .write_header()?
                .write_image_data(&vec![0xaa; 1125 * 2])?;
        }
        let cancel = CancellationToken::new();
        let picture = scaled(&palette, Target::Document, &cancel)?
            .unwrap()
            .picture;
        for rgba in picture.rgba.as_chunks::<4>().0 {
            assert_eq!(&rgba[..3], &[0, 0, 255]);
            assert!((100..=150).contains(&rgba[3]));
        }
        let mut sixteen_bit = Vec::new();
        {
            let mut encoder = ::png::Encoder::new(&mut sixteen_bit, 9000, 2);
            encoder.set_color(ColorType::GrayscaleAlpha);
            encoder.set_depth(BitDepth::Sixteen);
            encoder
                .write_header()?
                .write_image_data(&[255, 255, 128, 128].repeat(18_000))?;
        }
        let picture = scaled(&sixteen_bit, Target::Document, &cancel)?
            .unwrap()
            .picture;
        assert_eq!(&picture.rgba[..4], &[255, 255, 255, 128]);
        Ok(())
    }

    #[test]
    fn vertical_area_filter_preserves_distinct_rows_and_transparency() -> Result<()> {
        let mut bytes = Vec::new();
        {
            let mut encoder = ::png::Encoder::new(&mut bytes, 3200, 4);
            encoder.set_color(ColorType::Rgba);
            encoder.set_depth(BitDepth::Eight);
            let pixels: Vec<_> = [
                [255, 0, 0, 255],
                [0, 0, 255, 255],
                [0, 255, 0, 128],
                [255, 0, 0, 0],
            ]
            .into_iter()
            .flat_map(|rgba| rgba.repeat(3200))
            .collect();
            encoder.write_header()?.write_image_data(&pixels)?;
        }
        let picture = scaled(&bytes, Target::Document, &CancellationToken::new())?
            .unwrap()
            .picture;
        assert_eq!((picture.width, picture.height), (1600, 2));
        for rgba in picture.rgba[..1600 * 4].as_chunks::<4>().0 {
            assert_eq!(rgba, &[128, 0, 128, 255]);
        }
        for rgba in picture.rgba[1600 * 4..].as_chunks::<4>().0 {
            assert_eq!(rgba, &[0, 255, 0, 64]);
        }
        Ok(())
    }
}
