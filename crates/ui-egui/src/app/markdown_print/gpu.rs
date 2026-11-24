use super::layout::{Layout, SCALE, SHEET};
use anyhow::{Context as _, Result, ensure};
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor, wgpu};
use std::time::Duration;

#[derive(Clone)]
pub(super) struct Graphics {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

pub(super) struct Pages {
    graphics: Graphics,
    renderer: Renderer,
    texture: wgpu::Texture,
    buffer: wgpu::Buffer,
    screen: ScreenDescriptor,
    stride: u32,
    pixels: Vec<u8>,
}

impl Pages {
    pub fn new(graphics: Graphics, layout: &mut Layout) -> Result<Self> {
        let device = &graphics.device;
        let [width, height] = SHEET.map(|value| (value * SCALE).ceil() as u32);
        ensure!(
            width.max(height) <= device.limits().max_texture_dimension_2d,
            "显卡不支持 A4 打印尺寸"
        );
        let format = wgpu::TextureFormat::Bgra8Unorm;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Markdown print page"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let stride = (width * 4).div_ceil(alignment) * alignment;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Markdown print readback"),
            size: u64::from(stride) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut renderer = Renderer::new(device, format, RendererOptions::PREDICTABLE);
        // Print layouts contain scalar coverage, never LCD stripe masks.
        renderer.set_font_gamma(1.0);
        for (id, delta) in std::mem::take(&mut layout.textures) {
            renderer.update_texture(device, &graphics.queue, id, &delta);
        }
        Ok(Self {
            graphics,
            renderer,
            texture,
            buffer,
            screen: ScreenDescriptor {
                size_in_pixels: [width, height],
                pixels_per_point: SCALE,
            },
            stride,
            pixels: vec![0; width as usize * height as usize * 4],
        })
    }

    pub fn size(&self) -> [u32; 2] {
        self.screen.size_in_pixels
    }

    pub fn render(&mut self, layout: &Layout, page: usize) -> Result<&[u8]> {
        let Graphics { device, queue } = &self.graphics;
        let primitives = layout.primitives(page);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let callbacks =
            self.renderer
                .update_buffers(device, queue, &mut encoder, &primitives, &self.screen);
        ensure!(callbacks.is_empty(), "打印页面含有不支持的外部绘图组件");
        {
            let view = self.texture.create_view(&Default::default());
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Markdown print raster"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            self.renderer
                .render(&mut pass.forget_lifetime(), &primitives, &self.screen);
        }
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.stride),
                    rows_per_image: None,
                },
            },
            self.texture.size(),
        );
        let submission = queue.submit([encoder.finish()]);
        let slice = self.buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(30)),
            })
            .context("打印页面渲染超时")?;
        rx.recv_timeout(Duration::from_secs(1))
            .context("打印像素回读未完成")??;
        {
            let mapped = slice.get_mapped_range()?;
            let width = self.screen.size_in_pixels[0] as usize * 4;
            for (output, input) in self
                .pixels
                .chunks_exact_mut(width)
                .zip(mapped.chunks_exact(self.stride as usize))
            {
                output.copy_from_slice(&input[..width]);
            }
        }
        self.buffer.unmap();
        Ok(&self.pixels)
    }
}
