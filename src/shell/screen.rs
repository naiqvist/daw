//! A black-material phosphor pass.
//!
//! After egui has finished, this pass reads the whole offscreen frame. The
//! shader decides what is a screen from the authored pixels themselves: any
//! near-black field, plus the marks sitting immediately in that field, gets
//! the phosphor treatment. That makes the sequencer, session, mixer and
//! console glass one material without maintaining a second geometry map.

use bytemuck::{Pod, Zeroable};
use eframe::egui;
use wgpu::util::DeviceExt as _;

const MAX_SCREENS: usize = 64;

#[derive(Clone, Copy, Debug)]
pub struct State {
    /// Transport phase, 0..1 of a beat. It stays zero while parked.
    pub phase: f32,
    /// Signal, engine or addressed-control energy, 0..1.
    pub activity: f32,
}

impl State {
    pub fn new(phase: f32, activity: f32) -> Self {
        let finite = |value: f32| if value.is_finite() { value } else { 0.0 };
        Self {
            phase: finite(phase).rem_euclid(1.0),
            activity: finite(activity).clamp(0.0, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Region {
    rect: egui::Rect,
    state: State,
}

#[derive(Clone, Debug, Default)]
struct Registry(Vec<Region>);

fn registry_id() -> egui::Id {
    egui::Id::new("stage-screen-phosphor-registry")
}

/// Empty the per-frame registry before the application paints.
pub fn begin_frame(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(registry_id(), Registry::default()));
}

/// Report activity from one display aperture.
///
/// Rectangles no longer decide where the shader runs; blackness does. The
/// registrations remain only as activity probes for bloom and interference.
pub fn register(painter: &egui::Painter, rect: egui::Rect, state: State) {
    let rect = rect.intersect(painter.clip_rect());
    if rect.width() < 2.0 || rect.height() < 2.0 {
        return;
    }
    painter.ctx().data_mut(|data| {
        let registry = data.get_temp_mut_or_default::<Registry>(registry_id());
        if registry.0.len() < MAX_SCREENS {
            registry.0.push(Region { rect, state });
        }
    });
}

/// Take the regions authored during the frame.
pub fn take(ctx: &egui::Context) -> Vec<Region> {
    ctx.data_mut(|data| {
        data.remove_temp::<Registry>(registry_id())
            .unwrap_or_default()
            .0
    })
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuRegion {
    /// Normalized screen UV: left, top, right, bottom.
    rect: [f32; 4],
    /// Activity, beat phase, then padding.
    state: [f32; 4],
}

/// The separate compositor that follows the exact frame copy.
pub struct Pass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    regions: wgpu::Buffer,
    bind: Option<(wgpu::BindGroup, u64)>,
    /// A short wrapping clock for temporal phosphor grain.
    frame: u32,
}

impl Pass {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("screen_phosphor"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "screen.wgsl"
            ))),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screen_phosphor_bind_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("screen_phosphor_pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("screen_phosphor_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GpuRegion>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &ATTRIBUTES,
                })],
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
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("screen_phosphor_sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let regions = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("screen_phosphor_regions"),
            contents: &vec![0; std::mem::size_of::<GpuRegion>() * MAX_SCREENS],
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        Self {
            pipeline,
            layout,
            sampler,
            regions,
            bind: None,
            frame: 0,
        }
    }

    fn ensure_bind_group(
        &mut self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
        generation: u64,
    ) {
        if self.bind.as_ref().is_none_or(|(_, at)| *at != generation) {
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("screen_phosphor_bind"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.bind = Some((bind, generation));
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        generation: u64,
        target: &wgpu::TextureView,
        size: [u32; 2],
        _pixels_per_point: f32,
        regions: &[Region],
    ) {
        if size[0] == 0 || size[1] == 0 {
            return;
        }
        self.ensure_bind_group(device, source, generation);
        let Some((bind, _)) = &self.bind else {
            return;
        };
        // A small modulus stays exactly representable after conversion to
        // f32. The shader uses it only as a noise seed, not as displayed
        // information or musical time.
        let noise_frame = self.frame % 4096;
        self.frame = self.frame.wrapping_add(1);
        let state = aggregate_state(regions);
        let gpu = GpuRegion {
            rect: [0.0, 0.0, 1.0, 1.0],
            state: [state.activity, state.phase, noise_frame as f32, 0.0],
        };
        queue.write_buffer(&self.regions, 0, bytemuck::bytes_of(&gpu));

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("screen_phosphor_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.set_vertex_buffer(0, self.regions.slice(..));
        pass.draw(0..3, 0..1);
    }
}

/// One frame has one global phosphor state. The most active registered
/// aperture drives it; its phase follows the same probe so unrelated parked
/// displays cannot reset a moving one.
fn aggregate_state(regions: &[Region]) -> State {
    regions
        .iter()
        .take(MAX_SCREENS)
        .filter(|region| region.rect.is_positive())
        .map(|region| region.state)
        .max_by(|a, b| a.activity.total_cmp(&b.activity))
        .unwrap_or_else(|| State::new(0.0, 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shader_finds_screen_material_from_the_finished_frame() {
        let source = include_str!("screen.wgsl");
        assert!(source.contains("textureSample"));
        assert!(source.contains("screen_gate"));
        assert!(source.contains("mix(authored.rgb, crt, screen_gate)"));
        assert!(source.contains("deck_grade"));
        assert!(source.contains("tech_circuit"));
        assert!(source.contains("ancient_glyph"));
        assert!(source.contains("SHEIKAH_CYAN"));
        assert!(source.contains("emission"));
        assert!(source.contains("beam"));
        assert!(source.contains("analog_noise"));
        assert!(source.contains("interference"));
        assert!(source.contains("discard"));
        assert!(!source.contains("exp("));
        assert!(!source.contains("smoothstep"));
        assert_eq!(std::mem::size_of::<GpuRegion>(), 32);
    }

    #[test]
    fn nonsense_cannot_drive_the_phosphor() {
        let state = State::new(f32::NAN, 7.0);
        assert_eq!(state.phase, 0.0);
        assert_eq!(state.activity, 1.0);
    }

    #[test]
    fn the_most_active_aperture_drives_the_whole_black_material() {
        let regions = [
            Region {
                rect: egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(10.0, 10.0)),
                state: State::new(0.25, 0.2),
            },
            Region {
                rect: egui::Rect::from_min_max(egui::pos2(20.0, 20.0), egui::pos2(30.0, 30.0)),
                state: State::new(0.75, 0.8),
            },
        ];
        let state = aggregate_state(&regions);
        assert_eq!(state.activity, 0.8);
        assert_eq!(state.phase, 0.75);
    }
}
