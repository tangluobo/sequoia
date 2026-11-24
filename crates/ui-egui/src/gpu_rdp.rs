//! Upload immutable RDP snapshots directly; native texture registration uses
//! egui's existing shader and sampler, without a second CPU canvas copy.
use egui_wgpu::{RenderState, wgpu};
use std::sync::Arc;
use tomato_services::remote_desktop::RdpSnapshot;

pub fn enable_gpu_rdp(ctx: &egui::Context, state: &RenderState) {
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("tomato-rdp-renderer"),
            Arc::new(state.clone()),
        )
    });
}
pub(crate) fn state(ctx: &egui::Context) -> Option<Arc<RenderState>> {
    ctx.data(|data| data.get_temp(egui::Id::new("tomato-rdp-renderer")))
}
pub(crate) struct Texture {
    state: Arc<RenderState>,
    texture: wgpu::Texture,
    id: egui::TextureId,
    size: [u16; 2],
}
impl Texture {
    pub(crate) fn new(state: Arc<RenderState>, size: [u16; 2]) -> Self {
        let texture = state.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("RDP retained RGBA canvas"),
            size: wgpu::Extent3d {
                width: u32::from(size[0]),
                height: u32::from(size[1]),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let id = state.renderer.write().register_native_texture(
            &state.device,
            &view,
            wgpu::FilterMode::Nearest,
        );
        Self {
            state,
            texture,
            id,
            size,
        }
    }
    pub(crate) fn id(&self) -> egui::TextureId {
        self.id
    }
    pub(crate) fn size(&self) -> [u16; 2] {
        self.size
    }
    pub(crate) fn upload(&self, snapshot: &RdpSnapshot) {
        let region = snapshot
            .region
            .unwrap_or(tomato_services::remote_desktop::Region {
                x: 0,
                y: 0,
                width: snapshot.width,
                height: snapshot.height,
            });
        self.state.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: u32::from(region.x),
                    y: u32::from(region.y),
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(snapshot.pixels.as_slice()),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(u32::from(region.width) * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: u32::from(region.width),
                height: u32::from(region.height),
                depth_or_array_layers: 1,
            },
        );
    }
}
impl Drop for Texture {
    fn drop(&mut self) {
        self.state.renderer.write().free_texture(&self.id);
    }
}
