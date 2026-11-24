//! Render the real UI without a swapchain when the Windows desktop is disconnected.
use eframe::egui_wgpu::{self, wgpu};
use std::{error::Error, time::Duration};
pub fn capture() -> Result<(), Box<dyn Error>> {
    let path = std::path::PathBuf::from(
        std::env::var_os("TOMATO_UI_CAPTURE").ok_or("Missing capture path")?,
    );
    if path.exists() {
        return Err("Use a fresh capture path".into());
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()?;
    let config = egui_wgpu::WgpuConfiguration::default();
    let instance = runtime.block_on(config.wgpu_setup.new_instance());
    let state = runtime.block_on(egui_wgpu::RenderState::create(
        &config,
        &instance,
        None,
        egui_wgpu::RendererOptions::default(),
    ))?;
    let ctx = egui::Context::default();
    tomato_ui_egui::configure_text_renderer(&ctx, &state);
    let mut app = tomato_ui_egui::TomatoApp::new(&ctx, runtime);
    let ppp = std::env::var("TOMATO_UI_CAPTURE_PPP")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(1.0);
    if !ppp.is_finite() || !(0.75..=4.).contains(&ppp) {
        return Err("Invalid fixture DPI".into());
    }
    let baseline = std::env::var_os("TOMATO_UI_CAPTURE_BASELINE_FONTS").is_some();
    let mut renderer = state.renderer.write();
    let mut last = None;
    for frame in 0..80 {
        app.logic(&ctx);
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200., 900.),
            )),
            time: Some(frame as f64 * 0.05),
            ..Default::default()
        };
        raw.viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .native_pixels_per_point = Some(ppp);
        let mut output = ctx.run_ui(raw, |ui| app.ui(ui));
        if frame == 0 && baseline {
            let mut fonts = ctx.fonts(|f| f.definitions().clone());
            for (name, data) in &mut fonts.font_data {
                if name.contains("fontawesome") || !matches!(data.font, std::borrow::Cow::Owned(_))
                {
                    continue;
                }
                let data = std::sync::Arc::make_mut(data);
                data.tweak.hinting_target = Default::default();
                data.tweak.hinting = None;
                data.tweak.subpixel_binning = None;
            }
            ctx.set_fonts(fonts);
            ctx.style_mut_of(egui::Theme::Light, |style| {
                style.visuals.text_options.subpixel_binning = true
            });
        }
        for (id, delta) in &output.textures_delta.set {
            for image in delta {
                renderer.update_texture(&state.device, &state.queue, *id, image);
            }
        }
        for id in &output.textures_delta.free {
            renderer.free_texture(id);
        }
        output.textures_delta.clear();
        last = Some(output);
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = last.ok_or("No UI frame")?;
    if (output.pixels_per_point - ppp).abs() > 0.001 {
        return Err("Native DPI was not applied to the UI font atlas".into());
    }
    let text_geometry:Vec<_>=output.shapes.iter().filter_map(|shape|if let egui::Shape::Text(text)=&shape.shape {Some(serde_json::json!({"text":text.galley.job.text,"x":text.pos.x,"y":text.pos.y,"width":text.galley.size().x,"height":text.galley.size().y,"alignOnBaseline":text.galley.job.align_on_baseline,"rows":text.galley.rows.iter().map(|row|serde_json::json!({"y":row.pos.y,"height":row.size.y,"text":row.text(),"glyphBaselines":row.glyphs.iter().map(|glyph|glyph.pos.y).collect::<Vec<_>>()})).collect::<Vec<_>>()}))}else {None}).collect();
    let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
    let size = wgpu::Extent3d {
        width: (1200. * ppp).round() as u32,
        height: (900. * ppp).round() as u32,
        depth_or_array_layers: 1,
    };
    let texture = state.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Tomato UI verification"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: state.target_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let mut encoder = state.device.create_command_encoder(&Default::default());
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [size.width, size.height],
        pixels_per_point: output.pixels_per_point,
    };
    let commands =
        renderer.update_buffers(&state.device, &state.queue, &mut encoder, &jobs, &screen);
    {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("UI verification"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
    }
    let stride = (size.width * 4).div_ceil(256) * 256;
    let buffer = state.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("UI verification readback"),
        size: u64::from(stride) * u64::from(size.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(size.height),
            },
        },
        size,
    );
    let submission = state
        .queue
        .submit(commands.into_iter().chain([encoder.finish()]));
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
    state.device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: Some(Duration::from_secs(10)),
    })?;
    rx.recv_timeout(Duration::from_secs(10))??;
    let mapped = buffer.slice(..).get_mapped_range()?;
    let mut rgba = Vec::with_capacity((size.width * size.height * 4) as usize);
    for row in mapped.chunks_exact(stride as usize) {
        rgba.extend_from_slice(&row[..(size.width * 4) as usize]);
    }
    if state.target_format == wgpu::TextureFormat::Bgra8Unorm {
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
    }
    image::save_buffer_with_format(
        &path,
        &rgba,
        size.width,
        size.height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )?;
    std::fs::write(
        path.with_extension("render.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"pixelsPerPoint":ppp,"pixelWidth":size.width,"pixelHeight":size.height,"baselineFonts":baseline,"nativeWindowVerified":false,"lcdText":ctx.style_of(egui::Theme::Light).visuals.text_options.lcd_subpixel,"textGeometry":text_geometry}),
        )?,
    )?;
    Ok(())
}
