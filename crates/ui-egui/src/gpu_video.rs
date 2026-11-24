//! I420 video stays planar until the fragment shader. Resources die with their desktop view.
use egui_wgpu::{CallbackResources, CallbackTrait, ScreenDescriptor, wgpu};
use std::sync::{Arc, Mutex, Weak};
use tomato_services::remote_desktop::YuvFrame;

pub fn enable_gpu_video(ctx: &egui::Context, format: wgpu::TextureFormat) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-video-format"), format));
}
pub(crate) fn format(ctx: &egui::Context) -> Option<wgpu::TextureFormat> {
    ctx.data(|data| data.get_temp(egui::Id::new("tomato-video-format")))
}
pub(crate) struct Video {
    frame: Arc<YuvFrame>,
    format: wgpu::TextureFormat,
    canvas: Arc<Mutex<Option<Canvas>>>,
}
impl Video {
    pub(crate) fn new(frame: Arc<YuvFrame>, format: wgpu::TextureFormat) -> Self {
        Self {
            frame,
            format,
            canvas: Default::default(),
        }
    }
    pub(crate) fn update(&mut self, frame: Arc<YuvFrame>) {
        self.frame = frame;
    }
    pub(crate) fn paint(&self, ui: &egui::Ui, clip: egui::Rect, rect: egui::Rect) {
        ui.painter()
            .with_clip_rect(clip)
            .add(egui_wgpu::Callback::new_paint_callback(
                rect,
                Draw {
                    frame: self.frame.clone(),
                    canvas: self.canvas.clone(),
                    format: self.format,
                    context: ui.ctx().clone(),
                },
            ));
    }
}
struct Program {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}
impl Program {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let mut entries = [0, 1, 2]
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            })
            .to_vec();
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ADB I420 planes"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ADB I420"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ADB BT.601"),
            source: wgpu::ShaderSource::Wgsl(include_str!("gpu_video.wgsl").into()),
        });
        let constants = [("SRGB_FRAMEBUFFER", if format.is_srgb() { 1. } else { 0. })];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ADB I420"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &constants,
                    ..Default::default()
                },
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ADB smooth scaling"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        Self {
            layout,
            pipeline,
            sampler,
        }
    }
}
struct CachedProgram(Arc<Program>);
struct Canvas {
    size: [u16; 2],
    textures: [wgpu::Texture; 3],
    binding: wgpu::BindGroup,
    program: Arc<Program>,
    uploaded: Weak<YuvFrame>,
}
impl Canvas {
    fn new(device: &wgpu::Device, program: Arc<Program>, size: [u16; 2]) -> Self {
        let dimensions = plane_dimensions(size);
        let textures = dimensions.map(|[width, height]| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("ADB I420 plane"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        });
        let views = textures
            .each_ref()
            .map(|texture| texture.create_view(&Default::default()));
        let mut entries = std::array::from_fn::<_, 3, _>(|index| wgpu::BindGroupEntry {
            binding: index as u32,
            resource: wgpu::BindingResource::TextureView(&views[index]),
        })
        .to_vec();
        entries.push(wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::Sampler(&program.sampler),
        });
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ADB I420"),
            layout: &program.layout,
            entries: &entries,
        });
        Self {
            size,
            textures,
            binding,
            program,
            uploaded: Weak::new(),
        }
    }
}
fn plane_dimensions([w, h]: [u16; 2]) -> [[u32; 2]; 3] {
    let (w, h) = (u32::from(w), u32::from(h));
    [
        [w, h],
        [w.div_ceil(2), h.div_ceil(2)],
        [w.div_ceil(2), h.div_ceil(2)],
    ]
}
struct Draw {
    frame: Arc<YuvFrame>,
    canvas: Arc<Mutex<Option<Canvas>>>,
    format: wgpu::TextureFormat,
    context: egui::Context,
}
impl CallbackTrait for Draw {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _: &ScreenDescriptor,
        _: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let size = [self.frame.width, self.frame.height];
        let dimensions = plane_dimensions(size);
        if size.contains(&0)
            || !self
                .frame
                .planes
                .iter()
                .zip(dimensions)
                .all(|(bytes, [w, h])| bytes.len() == (w * h) as usize)
        {
            return Vec::new();
        }
        let Ok(mut canvas) = self.canvas.lock() else {
            return Vec::new();
        };
        if canvas.as_ref().is_none_or(|canvas| canvas.size != size) {
            if resources.get::<CachedProgram>().is_none() {
                resources.insert(CachedProgram(Arc::new(Program::new(device, self.format))));
            }
            let program = resources.get::<CachedProgram>().unwrap().0.clone();
            *canvas = Some(Canvas::new(device, program, size));
        }
        let canvas = canvas.as_mut().unwrap();
        if !canvas.uploaded.ptr_eq(&Arc::downgrade(&self.frame)) {
            for (index, [width, height]) in dimensions.into_iter().enumerate() {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &canvas.textures[index],
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &self.frame.planes[index],
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(width),
                        rows_per_image: Some(height),
                    },
                    wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                );
            }
            canvas.uploaded = Arc::downgrade(&self.frame);
            record_upload(&self.context, "GPU-I420");
        }
        Vec::new()
    }
    fn paint(
        &self,
        _: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        _: &CallbackResources,
    ) {
        if let Ok(canvas) = self.canvas.lock()
            && let Some(canvas) = canvas.as_ref()
        {
            pass.set_pipeline(&canvas.program.pipeline);
            pass.set_bind_group(0, &canvas.binding, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

pub(crate) fn record_upload(ctx: &egui::Context, format: &str) {
    if std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("adb")
        && std::env::var_os("TOMATO_UI_CAPTURE").is_some()
    {
        let now = ctx.input(|i| i.time);
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("tomato-adb-video-format"), format.to_owned());
            let sample = data.get_temp_mut_or_default::<[f64; 5]>(egui::Id::new(
                "tomato-adb-video-capture-metrics",
            ));
            if sample[0] == 0. {
                sample[0] = now;
            }
            if now - sample[0] < 1. {
                return;
            }
            if sample[3] == 0. {
                sample[1] = now;
            } else {
                sample[4] = sample[4].max(now - sample[2]);
            }
            sample[2] = now;
            sample[3] += 1.;
        });
    }
}
