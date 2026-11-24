//! Port of Java IconImageProcessor. Byte mattes replace Java's int-per-pixel
//! buffers; batch callers process one source at a time.
use anyhow::{Result, ensure};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::path::Path;
pub mod job;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub tolerance: u8,
    pub auto_crop: bool,
    pub smooth: bool,
    pub feather: u8,
    /// Left, top, right and bottom margins in processed image pixels.
    pub margins: [u32; 4],
}
impl Default for Options {
    fn default() -> Self {
        Self {
            tolerance: 20,
            auto_crop: true,
            smooth: true,
            feather: 2,
            margins: [0; 4],
        }
    }
}
fn byte(v: f64) -> u8 {
    v.round().clamp(0., 255.) as u8
}
fn step(t: f64) -> f64 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn distance(a: [u8; 3], b: [u8; 3]) -> f64 {
    ((0..3)
        .map(|c| (f64::from(a[c]) - f64::from(b[c])).powi(2))
        .sum::<f64>()
        / 3.)
        .sqrt()
}
fn rgb(p: &image::Rgba<u8>) -> [u8; 3] {
    [p[0], p[1], p[2]]
}
fn background(image: &RgbaImage) -> [u8; 3] {
    let (w, h) = image.dimensions();
    let border = (w.min(h) / 100).clamp(1, 4);
    let is_border = |x: u32, y: u32| {
        x < border || y < border || x >= w.saturating_sub(border) || y >= h.saturating_sub(border)
    };
    let mut histogram = [0u32; 4096];
    for (x, y, p) in image.enumerate_pixels() {
        if is_border(x, y) {
            histogram[(usize::from(p[0] >> 4) << 8)
                | (usize::from(p[1] >> 4) << 4)
                | usize::from(p[2] >> 4)] += 1;
        }
    }
    let mut bin = 0;
    for i in 1..4096 {
        if histogram[i] > histogram[bin] {
            bin = i;
        }
    }
    let center = [
        (((bin >> 8) & 15) * 16 + 8) as u8,
        (((bin >> 4) & 15) * 16 + 8) as u8,
        ((bin & 15) * 16 + 8) as u8,
    ];
    let mut sum = [0u64; 4];
    for (x, y, p) in image.enumerate_pixels() {
        if is_border(x, y) && distance(rgb(p), center) <= 24. {
            for c in 0..3 {
                sum[c] += u64::from(p[c]);
            }
            sum[3] += 1;
        }
    }
    std::array::from_fn(|c| sum[c].checked_div(sum[3]).unwrap_or(u64::from(center[c])) as u8)
}
fn neighborhood(
    w: usize,
    h: usize,
    x: usize,
    y: usize,
    r: usize,
    mut visit: impl FnMut(usize, usize, usize),
) {
    for yy in y.saturating_sub(r)..=(y + r).min(h - 1) {
        for xx in x.saturating_sub(r)..=(x + r).min(w - 1) {
            visit(xx, yy, yy * w + xx);
        }
    }
}
fn refined(raw: &[u8], w: usize, h: usize, r: usize) -> Vec<u8> {
    let stabilized: Vec<u8> = raw
        .iter()
        .map(|&a| byte(step((f64::from(a) - 24.) / 208.) * 255.))
        .collect();
    let mut cleaned = stabilized.clone();
    for y in 0..h {
        for x in 0..w {
            let (mut visible, mut strong, mut sum, mut count) = (0, 0, 0, 0);
            neighborhood(w, h, x, y, 1, |_, _, i| {
                let a = stabilized[i];
                visible += u32::from(a > 24);
                strong += u32::from(a >= 192);
                sum += u32::from(a);
                count += 1;
            });
            let center = stabilized[y * w + x];
            if visible <= 1 || (center <= 96 && visible <= 2) {
                cleaned[y * w + x] = 0;
            } else if center < 192 && strong >= 5.max(count - 2) {
                cleaned[y * w + x] = center.max((sum / count) as u8);
            }
        }
    }
    let sigma = (r as f64 * 0.65).max(0.8);
    let mut kernel: Vec<f64> = (-(r as i32)..=r as i32)
        .map(|k| (-f64::from(k * k) / (2. * sigma * sigma)).exp())
        .collect();
    let total: f64 = kernel.iter().sum();
    for v in &mut kernel {
        *v /= total;
    }
    // Double precision matches Java's two-pass convolution; the other mattes are bytes.
    let mut horizontal = vec![0f64; raw.len()];
    for y in 0..h {
        for x in 0..w {
            horizontal[y * w + x] = kernel
                .iter()
                .enumerate()
                .map(|(k, v)| {
                    f64::from(
                        cleaned[y * w
                            + (x as isize + k as isize - r as isize).clamp(0, w as isize - 1)
                                as usize],
                    ) * v
                })
                .sum();
        }
    }
    let mut output = vec![0; raw.len()];
    for y in 0..h {
        for x in 0..w {
            let a = byte(
                kernel
                    .iter()
                    .enumerate()
                    .map(|(k, v)| {
                        horizontal[(y as isize + k as isize - r as isize).clamp(0, h as isize - 1)
                            as usize
                            * w
                            + x]
                            * v
                    })
                    .sum(),
            );
            let a = byte(step(f64::from(a) / 255.) * 255.);
            output[y * w + x] = if a <= 3 {
                0
            } else if a >= 252 {
                255
            } else {
                a
            };
        }
    }
    output
}
pub fn process(mut source: RgbaImage, options: &Options) -> Result<RgbaImage> {
    let (w, h) = source.dimensions();
    ensure!(
        w > 0 && h > 0 && u64::from(w) * u64::from(h) <= 16_777_216,
        "图片超过 1600 万像素限制"
    );
    // Match Java's white compositing before background detection.
    for p in source.pixels_mut() {
        let a = u32::from(p[3]);
        for c in 0..3 {
            p[c] = ((u32::from(p[c]) * a + 255 * (255 - a) + 127) / 255) as u8;
        }
        p[3] = 255;
    }
    let bg = background(&source);
    let tolerance = f64::from(options.tolerance.min(160));
    let raw: Vec<u8> = source
        .pixels()
        .map(|p| byte(((distance(rgb(p), bg) - tolerance) / 36.).clamp(0., 1.) * 255.))
        .collect();
    let (w, h) = (w as usize, h as usize);
    let radius = usize::from(options.feather.clamp(1, 5));
    let alpha = if options.smooth {
        refined(&raw, w, h, radius)
    } else {
        raw.clone()
    };
    let mut output = RgbaImage::new(w as u32, h as u32);
    let mut seeds = vec![false; raw.len()];
    if options.smooth {
        for y in 0..h {
            for x in 0..w {
                if raw[y * w + x] < 250 {
                    continue;
                }
                let mut reliable = true;
                neighborhood(w, h, x, y, radius.min(2), |_, _, i| {
                    reliable &= raw[i] >= 250;
                });
                seeds[y * w + x] = reliable;
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut a = alpha[i];
            let observed = source.get_pixel(x as u32, y as u32);
            let mut color = rgb(observed);
            if a == 0 {
                color = [255; 3];
            } else if options.smooth && !(a == 255 && seeds[i]) {
                let search = (radius * 2 + 3).clamp(3, 13);
                let mut sums = [0f64; 3];
                let mut weight = 0.;
                neighborhood(w, h, x, y, search, |xx, yy, j| {
                    let d = x.abs_diff(xx).pow(2) + y.abs_diff(yy).pow(2);
                    if seeds[j] && d <= search * search {
                        let v = (f64::from(raw[j]) / 255.) / (1. + d as f64);
                        let p = source.get_pixel(xx as u32, yy as u32);
                        for c in 0..3 {
                            sums[c] += f64::from(p[c]) * v;
                        }
                        weight += v;
                    }
                });
                if weight > 0. {
                    for c in 0..3 {
                        color[c] = byte(sums[c] / weight);
                    }
                    let mut denominator = 0.;
                    let mut projection = 0.;
                    for c in 0..3 {
                        let vector = f64::from(color[c]) - f64::from(bg[c]);
                        denominator += vector * vector;
                        projection += (f64::from(observed[c]) - f64::from(bg[c])) * vector;
                    }
                    let coverage = if denominator < 16. {
                        255
                    } else {
                        byte((projection / denominator).clamp(0., 1.) * 255.)
                    };
                    a = byte(f64::from(coverage) * 0.75 + f64::from(a) * 0.25);
                    a = if a <= 3 {
                        0
                    } else if a >= 252 {
                        255
                    } else {
                        a
                    };
                } else {
                    let matte = f64::from(raw[i].max(a).max(32)) / 255.;
                    let amount = step((250. - f64::from(raw[i])) / 128.);
                    for c in 0..3 {
                        let edge = byte(
                            (f64::from(observed[c]) - f64::from(bg[c]) * (1. - matte)) / matte,
                        );
                        color[c] = byte(
                            f64::from(observed[c])
                                + (f64::from(edge) - f64::from(observed[c])) * amount,
                        );
                    }
                }
            }
            output.put_pixel(
                x as u32,
                y as u32,
                image::Rgba([color[0], color[1], color[2], a]),
            );
        }
    }
    if options.auto_crop {
        let (mut left, mut top, mut right, mut bottom) = (w as u32, h as u32, 0, 0);
        let mut found = false;
        for (x, y, p) in output.enumerate_pixels() {
            if p[3] > 8 {
                found = true;
                left = left.min(x);
                top = top.min(y);
                right = right.max(x);
                bottom = bottom.max(y);
            }
        }
        if found {
            output =
                image::imageops::crop_imm(&output, left, top, right - left + 1, bottom - top + 1)
                    .to_image();
        }
    }
    let [left, top, right, bottom] = options.margins;
    ensure!(
        left.saturating_add(right) < output.width() && top.saturating_add(bottom) < output.height(),
        "裁切距离超过图片尺寸"
    );
    if options.margins != [0; 4] {
        output = image::imageops::crop_imm(
            &output,
            left,
            top,
            output.width() - left - right,
            output.height() - top - bottom,
        )
        .to_image();
    }
    Ok(output)
}
pub fn load(path: &Path) -> Result<RgbaImage> {
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(96 * 1024 * 1024);
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    reader.limits(limits);
    let source = reader.decode()?.to_rgba8();
    ensure!(
        u64::from(source.width()) * u64::from(source.height()) <= 16_777_216,
        "图片超过 1600 万像素限制"
    );
    Ok(source)
}
pub fn save(source: &Path, destination: &Path, options: &Options) -> Result<()> {
    save_checked(source, destination, options, None)
}
pub(super) fn file_hash(path: &Path) -> Result<[u8; 32]> {
    use std::io::Read;
    ensure!(
        std::fs::metadata(path)?.len() <= 96 * 1024 * 1024,
        "源图片文件超过 96 MiB"
    );
    let mut file = std::fs::File::open(path)?;
    let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = [0; 65536];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(hash.finish().as_ref().try_into().unwrap())
}
pub(super) fn save_checked(
    source: &Path,
    destination: &Path,
    options: &Options,
    expected: Option<[u8; 32]>,
) -> Result<()> {
    let stamp = std::fs::metadata(source)?;
    if let Some(expected) = expected {
        ensure!(
            file_hash(source)? == expected,
            "源图片已被修改，请重新选择文件后再保存"
        );
    }
    let result = process(load(source)?, options)?;
    let current = std::fs::metadata(source)?;
    ensure!(
        stamp.len() == current.len() && stamp.modified()? == current.modified()?,
        "源图片已被修改，请重新选择文件"
    );
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut writer = std::io::BufWriter::new(temporary.reopen()?);
    result.write_to(&mut writer, image::ImageFormat::Png)?;
    std::io::Write::flush(&mut writer)?;
    drop(writer);
    temporary.as_file().sync_all()?;
    temporary.persist(destination)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn java_processor_pixels_match_for_colored_borders_smoothing_and_crop() -> Result<()> {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/icons");
        for i in 0..4 {
            for smooth in [false, true] {
                for crop in [false, true] {
                    let source = load(&fixtures.join(format!("source-{i}.png")))?;
                    let actual = process(
                        source,
                        &Options {
                            smooth,
                            auto_crop: crop,
                            ..Default::default()
                        },
                    )?;
                    let expected = load(&fixtures.join(format!(
                        "expected-{i}-{}-{}.png",
                        u8::from(smooth),
                        u8::from(crop)
                    )))?;
                    assert_eq!(
                        actual.dimensions(),
                        expected.dimensions(),
                        "fixture {i} smooth {smooth} crop {crop}"
                    );
                    assert_eq!(
                        actual.as_raw(),
                        expected.as_raw(),
                        "fixture {i} smooth {smooth} crop {crop}"
                    );
                }
            }
        }
        Ok(())
    }
    #[test]
    fn manual_crop_saved_png_and_invalid_bounds_preserve_target() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let source = directory.path().join("source.png");
        let output = directory.path().join("icon.png");
        RgbaImage::from_pixel(20, 30, image::Rgba([0, 0, 0, 255])).save(&source)?;
        let options = Options {
            margins: [2, 3, 4, 5],
            ..Default::default()
        };
        save(&source, &output, &options)?;
        assert_eq!(load(&output)?.dimensions(), (14, 22));
        let before = std::fs::read(&output)?;
        assert!(
            save(
                &source,
                &output,
                &Options {
                    margins: [20, 0, 0, 0],
                    ..options
                }
            )
            .is_err()
        );
        assert_eq!(std::fs::read(output)?, before);
        Ok(())
    }
    #[test]
    fn changed_source_cannot_replace_the_result_bound_to_preview() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let source = directory.path().join("source.png");
        let destination = directory.path().join("result.png");
        RgbaImage::from_pixel(20, 30, image::Rgba([255, 255, 255, 255])).save(&source)?;
        let expected = file_hash(&source)?;
        std::fs::write(&destination, b"previous output")?;
        RgbaImage::from_pixel(20, 30, image::Rgba([30, 40, 50, 255])).save(&source)?;
        assert!(save_checked(&source, &destination, &Options::default(), Some(expected)).is_err());
        assert_eq!(std::fs::read(destination)?, b"previous output");
        Ok(())
    }
}
