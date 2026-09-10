//! GPU resources belong to the active preview/export, not global callback maps.
//! Dropping the last Lease on Off releases all visual pipelines/buffers.
use super::Frame;
use std::sync::{Arc, Mutex};
use wgpu::util::DeviceExt;

pub struct Gpu {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    encode_srgb: bool,
}
impl Gpu {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("visual primitives"),
            source: wgpu::ShaderSource::Wgsl(include_str!("primitives.wgsl").into()),
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("visual frame"),
            contents: bytemuck::bytes_of(&<Frame as bytemuck::Zeroable>::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("visual primitives"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        Self {
            pipeline,
            uniform,
            bind,
            encode_srgb: !format.is_srgb(),
        }
    }
    pub fn update(&self, queue: &wgpu::Queue, frame: &Frame) {
        // Stage's egui target stores gamma-space bytes; offline targets are
        // sRGB attachments. Encode exactly once in either pipeline.
        let mut frame = *frame;
        frame.info[2] = f32::from(self.encode_srgb);
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&frame));
    }
    pub fn paint(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.draw(0..3, 0..1);
    }
}
/// Only a format tag is installed at shell creation: no shader/texture/worker.
struct Format(wgpu::TextureFormat);
pub fn install(renderer: &mut egui_wgpu::Renderer, format: wgpu::TextureFormat) {
    renderer.callback_resources.insert(Format(format));
}
pub type Lease = Arc<Mutex<Option<Gpu>>>;
pub fn lease() -> Lease {
    Arc::new(Mutex::new(None))
}
pub struct Callback {
    pub frame: Frame,
    pub gpu: Lease,
}
impl egui_wgpu::CallbackTrait for Callback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _: &egui_wgpu::ScreenDescriptor,
        _: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(format) = resources.get::<Format>()
            && let Ok(mut gpu) = self.gpu.lock()
        {
            let gpu = gpu.get_or_insert_with(|| Gpu::new(device, format.0));
            gpu.update(queue, &self.frame);
        }
        vec![]
    }
    fn paint(
        &self,
        _: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        _: &egui_wgpu::CallbackResources,
    ) {
        if let Ok(gpu) = self.gpu.lock()
            && let Some(gpu) = gpu.as_ref()
        {
            gpu.paint(pass);
        }
    }
}

/// Offline GPU target. Blocking readback belongs ONLY to the export worker.
pub struct Offline {
    device: wgpu::Device,
    queue: wgpu::Queue,
    gpu: Gpu,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    readback: wgpu::Buffer,
    size: [u32; 2],
    stride: u32,
    pub adapter: String,
}
impl Offline {
    pub fn new(size: [u32; 2]) -> Result<Self, String> {
        Self::with_format(size, wgpu::TextureFormat::Rgba8UnormSrgb)
    }
    fn with_format(size: [u32; 2], format: wgpu::TextureFormat) -> Result<Self, String> {
        if size.iter().any(|v| !(16..=3840).contains(v)) {
            return Err("video dimensions: 16..3840".into());
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .map_err(|e| e.to_string())?;
        let name = format!(
            "{} ({:?})",
            adapter.get_info().name,
            adapter.get_info().device_type
        );
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("visual export"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let gpu = Gpu::new(&device, format);
        if let Some(err) = pollster::block_on(scope.pop()) {
            return Err(err.to_string());
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("video frame"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let stride = (size[0] * 4).div_ceil(256) * 256;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video readback"),
            size: stride as u64 * size[1] as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            device,
            queue,
            gpu,
            texture,
            view,
            readback,
            size,
            stride,
            adapter: name,
        })
    }
    pub fn frame(&mut self, frame: &Frame) -> Result<Vec<u8>, String> {
        self.gpu.update(&self.queue, frame);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("video render"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.gpu.paint(&mut pass);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.stride),
                    rows_per_image: Some(self.size[1]),
                },
            },
            wgpu::Extent3d {
                width: self.size[0],
                height: self.size[1],
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        let slice = self.readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(5)),
            })
            .map_err(|e| e.to_string())?;
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let mapped = slice.get_mapped_range().map_err(|e| e.to_string())?;
        let mut rgba = Vec::with_capacity((self.size[0] * self.size[1] * 4) as usize);
        for row in mapped
            .chunks_exact(self.stride as usize)
            .take(self.size[1] as usize)
        {
            rgba.extend_from_slice(&row[..(self.size[0] * 4) as usize]);
        }
        drop(mapped);
        self.readback.unmap();
        Ok(rgba)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a real GPU; run explicitly with --include-ignored"]
    fn visual_gpu_gamma_preview_matches_srgb_export_and_seek() {
        let mut a = Offline::with_format([64, 64], wgpu::TextureFormat::Rgba8Unorm).unwrap();
        let mut b = Offline::new([64, 64]).unwrap();
        let mut score = crate::visuals::Score::default();
        for cmd in [
            "clip a 192",
            "layer a b field",
            "layer a c rings",
            "blend a c screen",
            "set a c brightness=4; hue=0.7",
            "place a 0 192 once",
            "lfo a b hue 0.25 0.3 0",
        ] {
            score = crate::visuals::command::edit(&score, cmd).unwrap();
        }
        let compiled = crate::visuals::Compiled::new(&score).unwrap();
        let f = compiled.frame(77.5, 1.0);
        let rgba = a.frame(&f).unwrap();
        let srgb = b.frame(&f).unwrap();
        let max = rgba
            .iter()
            .zip(&srgb)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(max <= 1, "gamma targets disagree by {max} / 255");
        a.frame(&compiled.frame(155.0, 1.0)).unwrap();
        assert_eq!(a.frame(&f).unwrap(), rgba, "seek must be pixel-repeatable");
        assert!(rgba.chunks_exact(4).all(|p| p[3] == 255));
    }
}
