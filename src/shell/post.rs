//! The post-process chain: offscreen scene -> bloom -> swapchain.
//!
//! The console's glass, the same pass the cockpit runs (`~/Work/tachikoma`,
//! `post.rs`): a bright-pass and two separable blurs at quarter resolution,
//! then a composite that applies aberration, scanlines, vignette and grain.
//! Curvature is deliberately absent because it distorts the rectilinear
//! grid, precisely the content it looks worst on. Nothing here varies with
//! time: the treatment is static, so a stopped stage costs nothing.
//!
//! Deliberately knows nothing about egui or the stage. It takes a device
//! and a scene texture view and writes to a target view; the bloom
//! targets and bind groups follow the scene's generation, so a resize
//! rebuilds them once and never per frame.

const BLOOM_DIV: u32 = 4;

/// How much of the bright pass is added back.
/// @tune 0..1
const BLOOM: f32 = 0.30;
/// How much a scanline takes.
/// @tune 0..0.5
const SCANLINE: f32 = 0.10;
/// Chromatic aberration at the edge, in pixels.
/// @tune 0..2 px
const ABERRATION: f32 = 0.35;
/// How far the field edge falls toward the cool ground.
/// @tune 0..1
const VIGNETTE: f32 = 0.22;
/// Static grain, as a share of full scale.
/// @tune 0..0.1
const GRAIN: f32 = 0.015;

/// What the glass does, as the shader reads it.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Params {
    pub resolution: [f32; 2],
    pub bloom: f32,
    pub scanline: f32,
    pub aberration: f32,
    pub vignette: f32,
    pub grain: f32,
    pub _pad: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            resolution: [1.0, 1.0],
            bloom: BLOOM,
            scanline: SCANLINE,
            aberration: ABERRATION,
            vignette: VIGNETTE,
            grain: GRAIN,
            _pad: 0.0,
        }
    }
}

pub struct Post {
    params: Params,
    params_buf: wgpu::Buffer,
    sampler: wgpu::Sampler,
    bloom_bgl: wgpu::BindGroupLayout,
    comp_bgl: wgpu::BindGroupLayout,
    bright_pipeline: wgpu::RenderPipeline,
    blur_h_pipeline: wgpu::RenderPipeline,
    blur_v_pipeline: wgpu::RenderPipeline,
    comp_pipeline: wgpu::RenderPipeline,
    format: wgpu::TextureFormat,
    /// The bloom targets and binds for one scene texture, by generation.
    bound: Option<Bound>,
}

struct Bound {
    generation: u64,
    bloom_a: wgpu::TextureView,
    bloom_b: wgpu::TextureView,
    bright: wgpu::BindGroup,
    blur_h: wgpu::BindGroup,
    blur_v: wgpu::BindGroup,
    comp: wgpu::BindGroup,
}

impl Post {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let bloom_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bloom_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("bloom.wgsl").into()),
        });
        let post_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("post.wgsl").into()),
        });
        let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("post_params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let tex_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let smp_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let bloom_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom_bgl"),
            entries: &[uniform_entry, tex_entry(1), smp_entry(2)],
        });
        let comp_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post_comp_bgl"),
            entries: &[uniform_entry, tex_entry(1), tex_entry(2), smp_entry(3)],
        });
        let bloom_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bloom_layout"),
            bind_group_layouts: &[Some(&bloom_bgl)],
            immediate_size: 0,
        });
        let comp_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post_comp_layout"),
            bind_group_layouts: &[Some(&comp_bgl)],
            immediate_size: 0,
        });
        let make_pipeline = |label: &str,
                             layout: &wgpu::PipelineLayout,
                             module: &wgpu::ShaderModule,
                             entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(format.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            params: Params::default(),
            bright_pipeline: make_pipeline(
                "bloom_bright",
                &bloom_layout,
                &bloom_shader,
                "fs_bright",
            ),
            blur_h_pipeline: make_pipeline(
                "bloom_blur_h",
                &bloom_layout,
                &bloom_shader,
                "fs_blur_h",
            ),
            blur_v_pipeline: make_pipeline(
                "bloom_blur_v",
                &bloom_layout,
                &bloom_shader,
                "fs_blur_v",
            ),
            comp_pipeline: make_pipeline("post_comp", &comp_layout, &post_shader, "fs_main"),
            params_buf,
            sampler,
            bloom_bgl,
            comp_bgl,
            format,
            bound: None,
        }
    }

    /// Bloom targets and binds for this scene, rebuilt only when the
    /// scene's generation moves.
    fn ensure_bound(
        &mut self,
        device: &wgpu::Device,
        scene: &wgpu::TextureView,
        size: [u32; 2],
        generation: u64,
    ) {
        if self
            .bound
            .as_ref()
            .is_some_and(|b| b.generation == generation)
        {
            return;
        }
        let (bloom_a, bloom_b) = make_bloom_targets(device, self.format, size[0], size[1]);
        let simple = |label: &str, src: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.bloom_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.params_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(src),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            })
        };
        let comp = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post_comp_bind"),
            layout: &self.comp_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(scene),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&bloom_a),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.bound = Some(Bound {
            generation,
            bright: simple("bloom_bright_bind", scene),
            blur_h: simple("bloom_blur_h_bind", &bloom_a),
            blur_v: simple("bloom_blur_v_bind", &bloom_b),
            comp,
            bloom_a,
            bloom_b,
        });
    }

    /// Run the chain from `source` onto `target`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        size: [u32; 2],
        generation: u64,
        target: &wgpu::TextureView,
    ) {
        self.ensure_bound(device, source, size, generation);
        self.params.resolution = [size[0].max(1) as f32, size[1].max(1) as f32];
        self.params.bloom = crate::tune!(BLOOM);
        self.params.scanline = crate::tune!(SCANLINE);
        self.params.aberration = crate::tune!(ABERRATION);
        self.params.vignette = crate::tune!(VIGNETTE);
        self.params.grain = crate::tune!(GRAIN);
        queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&self.params));
        let Some(b) = &self.bound else {
            return;
        };
        // bright writes a; blur_h reads a, writes b; blur_v reads b, writes
        // a; the composite reads the scene and a.
        pass(
            encoder,
            "bloom_bright",
            &b.bloom_a,
            &self.bright_pipeline,
            &b.bright,
        );
        pass(
            encoder,
            "bloom_blur_h",
            &b.bloom_b,
            &self.blur_h_pipeline,
            &b.blur_h,
        );
        pass(
            encoder,
            "bloom_blur_v",
            &b.bloom_a,
            &self.blur_v_pipeline,
            &b.blur_v,
        );
        pass(encoder, "post_comp", target, &self.comp_pipeline, &b.comp);
    }
}

fn pass(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    target: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    bind: &wgpu::BindGroup,
) {
    let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
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
    rp.set_pipeline(pipeline);
    rp.set_bind_group(0, bind, &[]);
    rp.draw(0..3, 0..1);
}

fn make_bloom_targets(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
) -> (wgpu::TextureView, wgpu::TextureView) {
    let w = (width / BLOOM_DIV).max(1);
    let h = (height / BLOOM_DIV).max(1);
    let make = |label: &str| {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default())
    };
    (make("bloom_a"), make("bloom_b"))
}

#[cfg(test)]
mod tests {
    /// The glass is static. A treatment that varied with time would pin
    /// a stopped stage at sixty frames to decorate nothing.
    #[test]
    fn nothing_in_the_glass_varies_with_time() {
        for source in [include_str!("post.wgsl"), include_str!("bloom.wgsl")] {
            let code: String = source
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(!code.contains("time"), "the glass reads a clock");
        }
    }

    /// Every knob the shader reads is in the params block, in its order.
    #[test]
    fn the_params_match_the_shader() {
        let source = include_str!("post.wgsl");
        for field in [
            "resolution",
            "bloom",
            "scanline",
            "aberration",
            "vignette",
            "grain",
        ] {
            assert!(
                source.contains(&format!("{field}:")),
                "shader lacks {field}"
            );
        }
        assert_eq!(std::mem::size_of::<super::Params>(), 32);
    }
}
