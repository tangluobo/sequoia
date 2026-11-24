//! Own GPU targets only: verifies per-channel blending on actual backgrounds.
use eframe::egui_wgpu::{self, wgpu};

#[test]
#[ignore = "requires TOMATO_LCD_PROOF and an owned GPU target"]
fn lcd_gpu_blends_rgb_coverage_on_white_selection_and_translucent_text()
-> Result<(), Box<dyn std::error::Error>> {
    let proof =
        std::path::PathBuf::from(std::env::var_os("TOMATO_LCD_PROOF").ok_or("Missing proof path")?);
    assert!(!proof.exists());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let config = egui_wgpu::WgpuConfiguration::default();
    let instance = runtime.block_on(config.wgpu_setup.new_instance());
    let state = runtime.block_on(egui_wgpu::RenderState::create(
        &config,
        &instance,
        None,
        Default::default(),
    ))?;
    #[cfg(windows)]
    let platform_gamma = egui::epaint::text::lcd_gamma();
    #[cfg(not(windows))]
    let platform_gamma = 1.0_f32;
    assert!(
        state.renderer.read().supports_lcd_text(),
        "This fixture requires a dual-source gamma framebuffer; unsupported devices must use grayscale"
    );
    let size = wgpu::Extent3d {
        width: 8,
        height: 8,
        depth_or_array_layers: 1,
    };
    let texture = state.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("LCD blend verification"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: state.target_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let ctx = egui::Context::default();
    let mut renderer = state.renderer.write();
    let mut cases = vec![];
    for (lcd, grayscale, gamma, coverage) in [
        (true, false, platform_gamma, None),
        // Exercise contrast independently of this machine's current ClearType setting.
        (true, false, 0.75, None),
        (true, true, 1.0, Some(0)),
        (true, true, 1.0, Some(32)),
        (true, true, 1.0, Some(64)),
        (true, true, 1.0, Some(128)),
        (true, true, 1.0, Some(192)),
        (true, true, 1.0, Some(254)),
        (true, true, 1.0, Some(255)),
        (false, false, 0.75, None),
        // Switching back must restore LCD contrast without rebuilding the renderer.
        (true, false, platform_gamma, None),
    ] {
        let mask = coverage.map_or_else(
            || egui::Color32::from_rgba_premultiplied(64, 128, 192, 192),
            egui::Color32::from_white_alpha,
        );
        renderer.set_font_gamma(gamma);
        let texture_id = egui::TextureId::Managed(if lcd { 0 } else { 1 });
        renderer.update_texture(
            &state.device,
            &state.queue,
            texture_id,
            &egui::epaint::ImageDelta::full(
                egui::ColorImage::filled([1, 1], mask),
                egui::TextureOptions::NEAREST,
            ),
        );
        for background in [[255u8; 3], [201, 235, 198], [32, 32, 32]] {
            for foreground in [
                egui::Color32::BLACK,
                egui::Color32::WHITE,
                egui::Color32::from_rgba_unmultiplied(26, 118, 210, 128),
            ] {
                let mut mesh = egui::Mesh::with_texture(texture_id);
                mesh.add_rect_with_uv(
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(8., 8.)),
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                    foreground,
                );
                let jobs = [egui::ClippedPrimitive {
                    clip_rect: egui::Rect::EVERYTHING,
                    primitive: egui::epaint::Primitive::Mesh(mesh),
                }];
                let mut encoder = state.device.create_command_encoder(&Default::default());
                let screen = egui_wgpu::ScreenDescriptor {
                    size_in_pixels: [8, 8],
                    pixels_per_point: 1.,
                };
                let commands = renderer.update_buffers(
                    &state.device,
                    &state.queue,
                    &mut encoder,
                    &jobs,
                    &screen,
                );
                {
                    let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("LCD background"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: f64::from(background[0]) / 255.,
                                    g: f64::from(background[1]) / 255.,
                                    b: f64::from(background[2]) / 255.,
                                    a: 1.,
                                }),
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
                let buffer = state.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("LCD readback"),
                    size: 256 * 8,
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
                            bytes_per_row: Some(256),
                            rows_per_image: Some(8),
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
                    timeout: Some(std::time::Duration::from_secs(15)),
                })?;
                rx.recv_timeout(std::time::Duration::from_secs(15))??;
                let data = buffer.slice(..).get_mapped_range()?;
                let mut actual: [u8; 4] = data[3 * 256 + 3 * 4..3 * 256 + 3 * 4 + 4].try_into()?;
                if state.target_format == wgpu::TextureFormat::Bgra8Unorm {
                    actual.swap(0, 2);
                }
                let expected: [u8; 3] = std::array::from_fn(|i| {
                    let mask_value = f32::from(mask[i]) / 255.;
                    let opacity = f32::from(foreground.a()) / 255.;
                    let backdrop = f32::from(background[i]) / 255.;
                    let result = if lcd {
                        let text = f32::from(foreground[i]) / f32::from(foreground.a());
                        let corrected = |bg: f32| {
                            (text.powf(1. / gamma) * mask_value
                                + bg.powf(1. / gamma) * (1. - mask_value))
                                .powf(gamma)
                        };
                        let painted = if text == 0. {
                            corrected(backdrop)
                        } else {
                            corrected(0.) * (1. - backdrop) + corrected(1.) * backdrop
                        };
                        painted * opacity + backdrop * (1. - opacity)
                    } else {
                        f32::from(foreground[i]) / 255. * mask_value
                            + backdrop * (1. - f32::from(mask.a()) / 255. * opacity)
                    };
                    (result * 255.).round() as u8
                });
                for i in 0..3 {
                    assert!(
                        actual[i].abs_diff(expected[i]) <= 2,
                        "background {background:?}, foreground {foreground:?}: {actual:?} vs {expected:?}"
                    );
                }
                assert_eq!(actual[3], 255);
                cases.push(serde_json::json!({"fontAtlas":lcd,"nativeGrayscale":grayscale,"fontGamma":gamma,"coverage":coverage,"background":background,"foreground":foreground.to_array(),"actual":actual,"expected":expected}));
                drop(data);
                buffer.unmap();
            }
        }
    }
    std::fs::write(
        proof,
        serde_json::to_vec_pretty(
            &serde_json::json!({"status":"passed","dualSource":true,"platformGamma":platform_gamma,"format":format!("{:?}",state.target_format),"cases":cases,"ownGpuTargetsOnly":true,"nativeWindowVerified":false}),
        )?,
    )?;
    drop(ctx);
    Ok(())
}
