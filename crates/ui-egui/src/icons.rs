//! Adapter between original JavaFX image resources and cached egui textures.
#[cfg(test)]
mod size_tests;
use egui::emath::GuiRounding as _;
use egui::{ColorImage, TextureHandle, TextureOptions};
use std::collections::HashMap;

#[derive(Default)]
pub struct Icons {
    textures: HashMap<String, Cached>,
    clock: u64,
}
struct Cached {
    texture: TextureHandle,
    used: u64,
}
fn gray(pixel: &mut [u8]) {
    let value = ((u32::from(pixel[0]) * 299
        + u32::from(pixel[1]) * 587
        + u32::from(pixel[2]) * 114)
        / 1000) as u8;
    pixel[..3].fill(value);
}

/// Match JavaFX's smooth minification for packaged icons. A box at each
/// source sample avoids the ringing that Lanczos introduces at hard edges.
fn minify_icon(source: &image::RgbaImage, edge: u32) -> image::RgbaImage {
    let (width, height) = source.dimensions();
    let box_width = width.div_ceil(edge);
    let box_height = height.div_ceil(edge);
    image::RgbaImage::from_fn(edge, edge, |x, y| {
        let left = i64::from(x * width / edge) - i64::from(box_width / 2);
        let top = i64::from(y * height / edge) - i64::from(box_height / 2);
        let mut sum = [0u32; 4];
        for row in 0..box_height {
            let sy = (top + i64::from(row)).clamp(0, i64::from(height - 1)) as u32;
            let mut line = [0u32; 4];
            for column in 0..box_width {
                let sx = (left + i64::from(column)).clamp(0, i64::from(width - 1)) as u32;
                for (total, value) in line.iter_mut().zip(source.get_pixel(sx, sy).0) {
                    *total += u32::from(value);
                }
            }
            for (total, value) in sum.iter_mut().zip(line) {
                *total += value / box_width;
            }
        }
        image::Rgba(sum.map(|total| (total / box_height) as u8))
    })
}

/// The title gear is drawn from a full-size JavaFX ImageView. Sample the
/// premultiplied source at destination pixel centers just as ImageView does.
fn sample_image_view(mut source: image::RgbaImage, edge: u32) -> ColorImage {
    for pixel in source.pixels_mut() {
        let [red, green, blue, alpha] = pixel.0;
        let premultiply = |channel| ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8;
        pixel.0 = [
            premultiply(red),
            premultiply(green),
            premultiply(blue),
            alpha,
        ];
    }
    let pixels = image::RgbaImage::from_fn(edge, edge, |x, y| {
        let u = (x as f32 + 0.5) / edge as f32;
        let v = (y as f32 + 0.5) / edge as f32;
        image::imageops::sample_bilinear(&source, u, v).expect("pixel center in nonempty icon")
    });
    ColorImage::from_rgba_premultiplied([edge as usize, edge as usize], pixels.as_raw())
}

impl Icons {
    pub fn get(&mut self, ctx: &egui::Context, name: &str) -> Option<egui::TextureId> {
        self.get_at_resolution(ctx, name, None)
    }
    fn get_at_resolution(
        &mut self,
        ctx: &egui::Context,
        name: &str,
        resolution: Option<u32>,
    ) -> Option<egui::TextureId> {
        let edge = resolution.unwrap_or(64);
        let key = if resolution.is_some() {
            format!("{name}@{edge}")
        } else {
            name.into()
        };
        self.clock = self.clock.wrapping_add(1);
        if let Some(cached) = self.textures.get_mut(&key) {
            cached.used = self.clock;
            return Some(cached.texture.id());
        }
        let source = name.strip_prefix("gray:").unwrap_or(name);
        // The Java connection and tool icon loader uses its bundled raster
        // variants when available. Use the same source pixels for those assets.
        let raster = (resolution.is_some()
            && source.starts_with("images/connect/")
            && source.ends_with(".svg"))
        .then(|| format!("{source}.png"))
        .filter(|path| crate::asset_bytes::bytes(path).is_some());
        let source = raster.as_deref().unwrap_or(source);
        let bytes = crate::asset_bytes::bytes(source)?;
        let mut image = if source.ends_with(".svg") {
            let options = resvg::usvg::Options::default();
            let tree = resvg::usvg::Tree::from_data(bytes, &options).ok()?;
            let mut pixmap = resvg::tiny_skia::Pixmap::new(edge, edge)?;
            let scale = edge as f32 / tree.size().width().max(tree.size().height());
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::from_scale(scale, scale),
                &mut pixmap.as_mut(),
            );
            ColorImage::from_rgba_premultiplied([edge as usize, edge as usize], pixmap.data())
        } else if name == "images/settings.png" && resolution.is_some() {
            sample_image_view(image::load_from_memory(bytes).ok()?.into_rgba8(), edge)
        } else {
            let mut image = image::load_from_memory(bytes).ok()?;
            if name.starts_with("gray:") {
                let mut pixels = image.into_rgba8();
                for pixel in pixels.pixels_mut() {
                    gray(&mut pixel.0);
                }
                image = image::DynamicImage::ImageRgba8(pixels);
            }
            let image = if resolution.is_some() {
                if image.width() == edge && image.height() == edge {
                    image.into_rgba8()
                } else if image.width() >= edge && image.height() >= edge {
                    minify_icon(&image.into_rgba8(), edge)
                } else {
                    image
                        .resize_exact(edge, edge, image::imageops::FilterType::Lanczos3)
                        .into_rgba8()
                }
            } else if image.width() > 64 || image.height() > 64 {
                image.thumbnail(64, 64).into_rgba8()
            } else {
                image.into_rgba8()
            };
            ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            )
        };
        if name.starts_with("gray:") && source.ends_with(".svg") {
            for pixel in &mut image.pixels {
                let mut rgba = pixel.to_srgba_unmultiplied();
                gray(&mut rgba);
                *pixel = egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
        }
        if self.textures.len() >= 64
            && let Some(oldest) = self
                .textures
                .iter()
                .max_by_key(|(_, cached)| self.clock.wrapping_sub(cached.used))
                .map(|(name, _)| name.clone())
        {
            self.textures.remove(&oldest);
        }
        let texture = ctx.load_texture(name, image, TextureOptions::LINEAR);
        let id = texture.id();
        self.textures.insert(
            key,
            Cached {
                texture,
                used: self.clock,
            },
        );
        Some(id)
    }
    pub fn paint(&mut self, ui: &egui::Ui, name: &str, rect: egui::Rect) {
        let scale = ui.ctx().pixels_per_point();
        let rect = egui::Rect::from_min_size(
            rect.min.round_to_pixels(scale),
            (rect.size() * scale).round() / scale,
        );
        if matches!(name, "ssh:root" | "ssh:parent") {
            let p = |x: f32, y: f32| {
                rect.min + egui::vec2(x * rect.width() / 18., y * rect.height() / 18.)
            };
            let bg = egui::Color32::from_rgb(120, 144, 156);
            let fg = egui::Color32::from_rgb(84, 110, 122);
            if name == "ssh:root" {
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![p(9., 2.), p(2., 9.), p(16., 9.)],
                    bg,
                    egui::Stroke::NONE,
                ));
                ui.painter()
                    .rect_filled(egui::Rect::from_min_max(p(4., 9.), p(14., 16.)), 0, bg);
                ui.painter()
                    .rect_filled(egui::Rect::from_min_max(p(7., 11.), p(11., 16.)), 0, fg);
            } else {
                ui.painter()
                    .rect_filled(egui::Rect::from_min_max(p(1., 5.), p(15., 14.)), 2, bg);
                ui.painter()
                    .rect_filled(egui::Rect::from_min_max(p(1., 2.), p(7., 6.)), 2, bg);
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![p(8., 3.), p(11.5, 0.), p(14., 3.)],
                    fg,
                    egui::Stroke::NONE,
                ));
                ui.painter()
                    .rect_filled(egui::Rect::from_min_max(p(10.5, 3.), p(12., 8.)), 0, fg);
            }
            return;
        }
        if matches!(name, "ssh:port" | "ssh:docker") {
            let p = |x: f32, y: f32| {
                rect.min + egui::vec2(x * rect.width() / 16., y * rect.height() / 16.)
            };
            if name == "ssh:port" {
                let blue = egui::Color32::from_rgb(74, 144, 217);
                ui.painter().rect_stroke(
                    egui::Rect::from_min_max(p(2., 3.), p(14., 13.)),
                    2,
                    egui::Stroke::new(1.2, blue),
                    egui::StrokeKind::Inside,
                );
                for x in [5., 8., 11.] {
                    ui.painter()
                        .line_segment([p(x, 3.), p(x, 1.)], egui::Stroke::new(1.2, blue));
                }
                ui.painter()
                    .rect_filled(egui::Rect::from_min_max(p(4., 6.), p(12., 10.)), 1, blue);
                ui.painter().circle_filled(
                    p(8., 12.),
                    rect.width() / 16.,
                    egui::Color32::from_rgb(45, 125, 70),
                );
            } else {
                let blue = egui::Color32::from_rgb(36, 150, 237);
                for y in [5., 9.] {
                    for x in [2., 6., 10.] {
                        ui.painter().rect_filled(
                            egui::Rect::from_min_max(p(x, y), p(x + 3., y + 3.)),
                            0,
                            blue,
                        );
                    }
                }
                ui.painter()
                    .line_segment([p(2., 3.), p(13., 3.)], egui::Stroke::new(1., blue));
            }
            return;
        }
        // Rasterize vectors at their destination's physical resolution. Rendering
        // every small SVG to 64px first introduces a second antialiasing filter.
        let edge = (rect.size().max_elem() * ui.ctx().pixels_per_point())
            .round()
            .clamp(1., 256.) as u32;
        if let Some(id) = self.get_at_resolution(ui.ctx(), name, Some(edge)) {
            ui.painter().image(
                id,
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }
}

pub fn connection(kind: &str) -> &'static str {
    match kind {
        "MYSQL" => "images/connect/mysql.png",
        "POSTGRESQL" => "images/connect/postgresql.png",
        "ORACLE" => "images/connect/oracle.png",
        "REDIS" => "images/connect/redis.png",
        "HIVE" => "images/connect/database-logos/hive.svg",
        "TOOL" => "images/connect/beautiful.png",
        "MONGODB" => "images/connect/database-logos/mongodb.svg",
        "SSH" => "images/connect/connection-icons/ssh.svg.png",
        "SFTP" => "images/connect/connection-icons/sftp.svg.png",
        "FTP" => "images/connect/connection-icons/ftp-client.svg.png",
        "RDP" => "images/connect/windows.png",
        "VNC" => "images/connect/vnc.svg.png",
        "SPICE" => "images/connect/spice.svg.png",
        "ADB" => "images/connect/adb.svg.png",
        "LOCAL_TERMINAL" => "images/connect/shell.png",
        "LOCAL_DIRECTORY" | "FOLDER" => "images/connect/folder.png",
        "S3" => "images/connect/s3.png",
        "ALIYUN_OSS" => "images/connect/connection-icons/aliyun-oss.svg.png",
        "ALIYUN" => "images/connect/connection-icons/aliyun.svg.png",
        "TENCENT_CLOUD" => "images/connect/connection-icons/tencent-cloud.svg.png",
        "KAFKA" => "images/connect/kafka.png",
        "ROCKETMQ" => "images/connect/rocketmq.png",
        "HTTP_SERVER" => "images/connect/connection-icons/http-server.svg.png",
        "FTP_SERVER" => "images/connect/connection-icons/ftp-server.svg.png",
        "SMB_SERVER" => "images/connect/connection-icons/smb-server.svg.png",
        _ => "images/connect/connect.png",
    }
}

/// Match Java ConnectionIconResolver, including product overrides and saved tools.
pub fn for_connection(connection: &tomato_core::Connection) -> String {
    if connection.kind() == "TOOL" {
        return tool(connection.text("toolType"));
    }
    let profile = connection
        .text("databaseProfileId")
        .trim()
        .to_ascii_lowercase();
    let profile = tomato_core::catalog::DATABASE_PROFILES
        .iter()
        .find(|p| p.id == profile)
        .or_else(|| {
            let default = match connection.kind() {
                "MYSQL" => "mysql",
                "POSTGRESQL" => "postgresql",
                "ORACLE" => "oracle",
                "HIVE" => "hive",
                _ => "",
            };
            tomato_core::catalog::DATABASE_PROFILES
                .iter()
                .find(|p| p.id == default)
        });
    if let Some(profile) = profile {
        let logo = if profile.id == "oceanbase-mysql" {
            "oceanbase"
        } else {
            profile.id
        };
        return format!("images/connect/database-logos/{logo}.svg");
    }
    self::connection(connection.kind()).replace(".svg.png", ".svg")
}
pub fn tool(kind: &str) -> String {
    let name = match kind {
        "image_format_converter" => "image-converter",
        "dataset_converter" => "dataset-converter",
        "json_tool" => "json-tool",
        "desktop_shortcut" => "desktop-shortcut",
        "hosts_file" => "hosts-file",
        "image_background_remover" => "icon-maker",
        "trae_session" => "session",
        "resource_extractor" => "resource-extractor",
        "port_mapping" => "port-mapping",
        "downloader" | "music_downloader" => "downloader",
        "color_transpose_game" => "color-transpose",
        "game_dungeons" => "game-dungeons",
        _ => return "images/connect/beautiful.png".into(),
    };
    format!("images/connect/tool-icons/{name}.svg")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_legacy_type_product_and_tool_resolves_an_original_asset() {
        for kind in tomato_core::catalog::KINDS {
            if kind.id == "SQLITE" {
                continue;
            }
            let connection = tomato_core::Connection::new(kind.id);
            assert!(
                crate::asset_bytes::bytes(&for_connection(&connection)).is_some(),
                "missing icon: {}",
                kind.id
            );
        }
        for profile in tomato_core::catalog::DATABASE_PROFILES {
            let mut connection = tomato_core::Connection::new(profile.family);
            connection.set("databaseProfileId", profile.id);
            let asset = for_connection(&connection);
            assert!(asset.ends_with(".svg"));
            assert!(
                crate::asset_bytes::bytes(&asset).is_some(),
                "missing product: {}",
                profile.id
            );
        }
        for &(tool, _) in tomato_core::catalog::TOOLS {
            let mut connection = tomato_core::Connection::new("TOOL");
            connection.set("toolType", tool);
            let asset = for_connection(&connection);
            assert_ne!(
                asset, "images/connect/beautiful.png",
                "specific tool icon: {tool}"
            );
            assert!(crate::asset_bytes::bytes(&asset).is_some());
        }
        let mut connection = tomato_core::Connection::new("MYSQL");
        connection.set("databaseProfileId", "oceanbase-mysql");
        assert_eq!(
            for_connection(&connection),
            "images/connect/database-logos/oceanbase.svg"
        );
        connection.set("databaseProfileId", "unknown-future-product");
        assert_eq!(
            for_connection(&connection),
            "images/connect/database-logos/mysql.svg"
        );
    }
}
