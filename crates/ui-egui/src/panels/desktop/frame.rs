//! Upload changed pixels while retaining the existing desktop texture.
use crate::tabs::desktop::DesktopView;
use egui::{Color32, Context, TextureOptions};
use tomato_services::remote_desktop::Frame;
pub(super) fn timing_started() -> Option<std::time::Instant> {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    ENABLED
        .get_or_init(|| std::env::var("TOMATO_RDP_PIPELINE_TIMING").as_deref() == Ok("1"))
        .then(std::time::Instant::now)
}
#[derive(Clone)]
struct UploadTiming {
    since: std::time::Instant,
    polls: u64,
    pictures: u64,
    take_micros: u128,
    upload_micros: u128,
    worst_micros: u128,
    reports: std::sync::Arc<UploadReports>,
}
#[derive(Default)]
struct UploadReports(std::sync::Mutex<std::collections::VecDeque<String>>);
impl Drop for UploadReports {
    fn drop(&mut self) {
        use std::io::Write as _;
        let Ok(reports) = self.0.get_mut() else {
            return;
        };
        let stderr = std::io::stderr();
        let mut output = std::io::BufWriter::with_capacity(64 * 1024, stderr.lock());
        for record in reports {
            let _ = writeln!(output, "{record}");
        }
        let _ = output.flush();
    }
}
pub(super) fn record_timing(
    ctx: &Context,
    started: Option<std::time::Instant>,
    taken: Option<std::time::Instant>,
    changed: bool,
) {
    let (Some(started), Some(taken)) = (started, taken) else {
        return;
    };
    let ended = std::time::Instant::now();
    ctx.data_mut(|data| {
        let stats = data.get_temp_mut_or_insert_with(
            egui::Id::new("tomato-rdp-pipeline-upload-timing"),
            || UploadTiming {
                since: started,
                polls: 0,
                pictures: 0,
                take_micros: 0,
                upload_micros: 0,
                worst_micros: 0,
                reports: Default::default(),
            },
        );
        stats.polls += 1;
        stats.pictures += u64::from(changed);
        stats.take_micros += taken.duration_since(started).as_micros();
        stats.upload_micros += ended.duration_since(taken).as_micros();
        stats.worst_micros = stats
            .worst_micros
            .max(ended.duration_since(started).as_micros());
        if stats.since.elapsed().as_secs() >= 1 {
            let record = format!(
                "RDP_UPLOAD seconds={:.3} polls={} pictures={} take_ms={:.3} upload_ms={:.3} worst_ms={:.3}",
                stats.since.elapsed().as_secs_f64(),
                stats.polls,
                stats.pictures,
                stats.take_micros as f64 / 1000.,
                stats.upload_micros as f64 / 1000.,
                stats.worst_micros as f64 / 1000.,
            );
            if let Ok(mut reports) = stats.reports.0.lock() {
                if reports.len() == 128 { reports.pop_front(); }
                reports.push_back(record);
            }
            stats.since = ended;
            stats.polls = 0;
            stats.pictures = 0;
            stats.take_micros = 0;
            stats.upload_micros = 0;
            stats.worst_micros = 0;
        }
    });
}
pub(super) fn needs_full_snapshot(ctx: &Context, view: &DesktopView, size: [u16; 2]) -> bool {
    if crate::gpu_rdp::state(ctx).is_some() {
        view.rdp_texture.as_ref().is_none_or(|t| t.size() != size)
    } else {
        view.texture
            .as_ref()
            .is_none_or(|t| t.size() != [usize::from(size[0]), usize::from(size[1])])
    }
}
pub(super) fn upload_snapshot(
    ctx: &Context,
    view: &mut DesktopView,
    id: &str,
    snapshot: tomato_services::remote_desktop::RdpSnapshot,
) {
    if let Some(state) = crate::gpu_rdp::state(ctx) {
        let size = [snapshot.width, snapshot.height];
        if view
            .rdp_texture
            .as_ref()
            .is_none_or(|texture| texture.size() != size)
        {
            view.rdp_texture = Some(crate::gpu_rdp::Texture::new(state, size));
            view.needs_focus = true;
        }
        view.rdp_texture.as_ref().unwrap().upload(&snapshot);
        view.size = size;
        view.video = None;
        view.texture = None;
        mark_rdp_capture(ctx);
        crate::gpu_video::record_upload(ctx, "RGBA");
    } else {
        let pixels = std::sync::Arc::try_unwrap(snapshot.pixels)
            .unwrap_or_else(|pixels| pixels.as_ref().clone());
        upload_rgba_region(
            ctx,
            view,
            id,
            tomato_services::remote_desktop::RgbaFrame {
                width: snapshot.width,
                height: snapshot.height,
                pixels,
            },
            snapshot.region,
        );
    }
}
pub(super) fn upload_rgba(
    ctx: &Context,
    view: &mut DesktopView,
    id: &str,
    frame: tomato_services::remote_desktop::RgbaFrame,
) {
    upload_rgba_region(ctx, view, id, frame, None);
}
pub(super) fn upload_rgba_region(
    ctx: &Context,
    view: &mut DesktopView,
    id: &str,
    frame: tomato_services::remote_desktop::RgbaFrame,
    region: Option<tomato_services::remote_desktop::Region>,
) {
    view.rdp_texture = None;
    view.video = None;
    crate::gpu_video::record_upload(ctx, "RGBA");
    view.size = [frame.width, frame.height];
    let size = region.map_or([usize::from(frame.width), usize::from(frame.height)], |r| {
        [usize::from(r.width), usize::from(r.height)]
    });
    mark_rdp_capture(ctx);
    // Both types have size/alignment 4. Transfer the allocation, with no channel repacking.
    if region.is_some()
        && view.texture.as_ref().is_none_or(|texture| {
            texture.size() != [usize::from(frame.width), usize::from(frame.height)]
        })
    {
        // A patch is never a texture-sized image. Reserve the complete canvas so
        // later distant patches cannot address outside a tiny first allocation.
        let canvas = egui::ColorImage::filled(
            [usize::from(frame.width), usize::from(frame.height)],
            Color32::BLACK,
        );
        view.texture =
            Some(ctx.load_texture(format!("desktop-{id}"), canvas, TextureOptions::NEAREST));
        view.needs_focus = true;
    }
    let pixels = bytemuck::allocation::cast_vec::<u32, Color32>(frame.pixels);
    let image = egui::ColorImage::new(size, pixels);
    if let Some(texture) = &mut view.texture {
        if let Some(region) = region {
            texture.set_partial(
                [usize::from(region.x), usize::from(region.y)],
                image,
                TextureOptions::NEAREST,
            );
        } else if texture.size() == size {
            texture.set_partial([0, 0], image, TextureOptions::NEAREST);
        } else {
            texture.set(image, TextureOptions::NEAREST);
        }
    } else {
        view.texture =
            Some(ctx.load_texture(format!("desktop-{id}"), image, TextureOptions::NEAREST));
        view.needs_focus = true;
    }
}
pub(super) fn upload(ctx: &Context, view: &mut DesktopView, id: &str, frame: Frame) {
    view.rdp_texture = None;
    view.video = None;
    view.size = [frame.width, frame.height];
    mark_rdp_capture(ctx);
    let size = frame.region.map_or(
        [usize::from(frame.width), usize::from(frame.height)],
        |region| [usize::from(region.width), usize::from(region.height)],
    );
    let pixels = frame
        .pixels
        .into_iter()
        .map(|pixel| Color32::from_rgb((pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8))
        .collect();
    let image = egui::ColorImage::new(size, pixels);
    if let Some(texture) = &mut view.texture {
        if let Some(region) = frame.region {
            texture.set_partial(
                [usize::from(region.x), usize::from(region.y)],
                image,
                TextureOptions::NEAREST,
            );
        } else if texture.size() == size {
            texture.set_partial([0, 0], image, TextureOptions::NEAREST);
        } else {
            texture.set(image, TextureOptions::NEAREST);
        }
    } else {
        view.texture = Some(ctx.load_texture(format!("rdp-{id}"), image, TextureOptions::NEAREST));
        view.needs_focus = true;
    }
}
pub(super) fn record_rdp_upload(ctx: &Context, pixels: usize, patches: usize) {
    if std::env::var("TOMATO_UI_PREVIEW").as_deref() != Ok("rdp")
        || std::env::var_os("TOMATO_UI_CAPTURE").is_none()
    {
        return;
    }
    let now = ctx.input(|i| i.time);
    ctx.data_mut(|data| {
        let s =
            data.get_temp_mut_or_default::<[f64; 7]>(egui::Id::new("tomato-rdp-capture-metrics"));
        if s[0] == 0. {
            s[0] = now;
        }
        if now - s[0] < 1. {
            return;
        }
        if s[3] == 0. {
            s[1] = now;
        } else {
            s[4] = s[4].max(now - s[2]);
        }
        s[2] = now;
        s[3] += 1.;
        s[5] += pixels as f64;
        s[6] += patches as f64;
    });
}
fn mark_rdp_capture(ctx: &Context) {
    if std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("rdp")
        && std::env::var_os("TOMATO_UI_CAPTURE").is_some()
    {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-rdp-first-frame"), true));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use tomato_services::remote_desktop::Region;
    #[test]
    fn first_partial_snapshot_allocates_full_canvas_and_accepts_a_distant_patch() {
        let ctx = Context::default();
        let mut view = DesktopView::default();
        let mut output = ctx.run_ui(Default::default(), |_| {
            upload_rgba_region(
                &ctx,
                &mut view,
                "partial-first",
                tomato_services::remote_desktop::RgbaFrame {
                    width: 120,
                    height: 80,
                    pixels: vec![u32::from_ne_bytes([255, 0, 0, 255]); 6],
                },
                Some(Region {
                    x: 3,
                    y: 4,
                    width: 3,
                    height: 2,
                }),
            );
            assert_eq!(view.texture.as_ref().unwrap().size(), [120, 80]);
            upload_rgba_region(
                &ctx,
                &mut view,
                "partial-first",
                tomato_services::remote_desktop::RgbaFrame {
                    width: 120,
                    height: 80,
                    pixels: vec![u32::from_ne_bytes([0, 255, 0, 255]); 4],
                },
                Some(Region {
                    x: 118,
                    y: 78,
                    width: 2,
                    height: 2,
                }),
            );
        });
        let deltas = output
            .textures_delta
            .set
            .values()
            .flat_map(|d| d.iter())
            .collect::<Vec<_>>();
        assert!(
            deltas
                .iter()
                .any(|d| d.pos.is_none() && d.image.size() == [120, 80])
        );
        assert!(
            deltas
                .iter()
                .any(|d| d.pos == Some([3, 4]) && d.image.size() == [3, 2])
        );
        assert!(
            deltas
                .iter()
                .any(|d| d.pos == Some([118, 78]) && d.image.size() == [2, 2])
        );
        assert!(!needs_full_snapshot(&ctx, &view, [120, 80]));
        drop(deltas);
        output.textures_delta.clear();
    }
    #[test]
    fn shared_snapshot_uses_the_same_pixels_without_a_native_gpu() {
        let ctx = Context::default();
        let mut view = DesktopView::default();
        let pixels = std::sync::Arc::new(vec![u32::from_ne_bytes([12, 34, 56, 255]); 6]);
        let mut snapshot = Some(tomato_services::remote_desktop::RdpSnapshot {
            width: 3,
            height: 2,
            pixels: pixels.clone(),
            region: None,
        });
        let mut output = ctx.run_ui(Default::default(), |_| {
            if let Some(snapshot) = snapshot.take() {
                upload_snapshot(&ctx, &mut view, "snapshot-fallback", snapshot);
            }
        });
        assert!(view.rdp_texture.is_none());
        assert_eq!(view.size, [3, 2]);
        let delta = &output.textures_delta.set[&view.texture_id().unwrap()][0];
        let egui::ImageData::Color(image) = &delta.image;
        assert!(
            image
                .pixels
                .iter()
                .all(|pixel| pixel.to_array() == [12, 34, 56, 255])
        );
        assert_eq!(
            pixels.as_slice(),
            &[u32::from_ne_bytes([12, 34, 56, 255]); 6]
        );
        output.textures_delta.clear();
    }
    #[test]
    fn rgba_rdp_patches_share_the_canvas_without_conversion_or_full_upload() {
        use tomato_services::remote_desktop::RgbaFrame;
        let ctx = Context::default();
        let mut view = DesktopView::default();
        let mut initial = ctx.run_ui(Default::default(), |_| {
            upload_rgba_region(
                &ctx,
                &mut view,
                "rdp-native",
                RgbaFrame {
                    width: 1920,
                    height: 1080,
                    pixels: vec![u32::from_ne_bytes([0, 0, 0, 255]); 1920 * 1080],
                },
                None,
            )
        });
        initial.textures_delta.clear();
        let texture = view.texture.as_ref().unwrap().id();
        let color = u32::from_ne_bytes([17, 34, 51, 255]);
        let mut pointers = Vec::new();
        let mut output = ctx.run_ui(Default::default(), |_| {
            for (x, y) in [(0, 0), (1888, 1048)] {
                let pixels = vec![color; 1024];
                pointers.push(pixels.as_ptr() as usize);
                upload_rgba_region(
                    &ctx,
                    &mut view,
                    "rdp-native",
                    RgbaFrame {
                        width: 1920,
                        height: 1080,
                        pixels,
                    },
                    Some(Region {
                        x,
                        y,
                        width: 32,
                        height: 32,
                    }),
                );
            }
        });
        let deltas = output.textures_delta.set.get(&texture).unwrap();
        assert_eq!(deltas.len(), 2);
        for ((delta, pointer), position) in deltas.iter().zip(pointers).zip([[0, 0], [1888, 1048]])
        {
            assert_eq!(delta.pos, Some(position));
            assert_eq!(delta.image.size(), [32, 32]);
            let egui::ImageData::Color(image) = &delta.image;
            assert_eq!(image.pixels.as_ptr() as usize, pointer);
            assert!(
                image
                    .pixels
                    .iter()
                    .all(|p| p.to_array() == [17, 34, 51, 255])
            );
        }
        assert_eq!(view.texture.as_ref().unwrap().size(), [1920, 1080]);
        output.textures_delta.clear();
    }
    #[test]
    fn rgba_video_upload_transfers_the_allocation_and_preserves_channels() {
        let ctx = Context::default();
        let mut view = DesktopView::default();
        let mut pixels = vec![0u32; 2];
        bytemuck::cast_slice_mut::<u32, u8>(&mut pixels)
            .copy_from_slice(&[11, 22, 33, 255, 44, 55, 66, 255]);
        let pointer = pixels.as_ptr() as usize;
        let mut pixels = Some(pixels);
        let mut output = ctx.run_ui(Default::default(), |_| {
            if let Some(pixels) = pixels.take() {
                upload_rgba(
                    &ctx,
                    &mut view,
                    "owned-video",
                    tomato_services::remote_desktop::RgbaFrame {
                        width: 2,
                        height: 1,
                        pixels,
                    },
                );
            }
        });
        let delta = &output
            .textures_delta
            .set
            .get(&view.texture.as_ref().unwrap().id())
            .unwrap()[0];
        let egui::ImageData::Color(image) = &delta.image;
        assert_eq!(
            bytemuck::cast_slice::<Color32, u8>(&image.pixels),
            &[11, 22, 33, 255, 44, 55, 66, 255]
        );
        assert_eq!(image.pixels.as_ptr() as usize, pointer);
        assert_eq!(view.size, [2, 1]);
        output.textures_delta.clear();
        let texture_id = view.texture.as_ref().unwrap().id();
        for (width, position) in [(2u16, Some([0, 0])), (3, None)] {
            let mut next = ctx.run_ui(Default::default(), |_| {
                upload_rgba(
                    &ctx,
                    &mut view,
                    "owned-video",
                    tomato_services::remote_desktop::RgbaFrame {
                        width,
                        height: 1,
                        pixels: vec![u32::from_ne_bytes([11, 22, 33, 255]); usize::from(width)],
                    },
                );
            });
            assert_eq!(view.texture.as_ref().unwrap().id(), texture_id);
            let delta = &next.textures_delta.set.get(&texture_id).unwrap()[0];
            assert_eq!(delta.pos, position);
            assert_eq!(delta.image.size(), [usize::from(width), 1]);
            next.textures_delta.clear();
        }
    }
    #[test]
    fn desktop_texture_keeps_canvas_and_uploads_only_changed_rectangle() {
        let ctx = Context::default();
        let mut view = DesktopView::default();
        let mut first = ctx.run_ui(Default::default(), |_| {
            upload(
                &ctx,
                &mut view,
                "owned-frame",
                Frame {
                    width: 1920,
                    height: 1080,
                    pixels: vec![0x123456; 1920 * 1080],
                    region: None,
                },
            )
        });
        let texture = view.texture.as_ref().unwrap().id();
        let initial = &first.textures_delta.set.get(&texture).unwrap()[0];
        assert_eq!(initial.pos, None);
        assert_eq!(initial.image.size(), [1920, 1080]);
        first.textures_delta.clear();
        let patch = Region {
            x: 450,
            y: 300,
            width: 32,
            height: 2,
        };
        let mut next = ctx.run_ui(Default::default(), |_| {
            upload(
                &ctx,
                &mut view,
                "owned-frame",
                Frame {
                    width: 1920,
                    height: 1080,
                    pixels: vec![0xaabbcc; 64],
                    region: Some(patch),
                },
            )
        });
        let changed = &next.textures_delta.set.get(&texture).unwrap()[0];
        assert_eq!(changed.pos, Some([450, 300]));
        assert_eq!(changed.image.size(), [32, 2]);
        assert_eq!(view.texture.as_ref().unwrap().size(), [1920, 1080]);
        assert_eq!(view.size, [1920, 1080]);
        next.textures_delta.clear();
    }
}
