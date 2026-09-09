//! Depth-tested, four-sample wgpu render of the physical drum. The heads
//! use the simulation's displacement field; no 2D painter projection.
use crate::kiln::{Patch, indices::*, membrane::Animation};
use egui_wgpu::{CallbackResources, CallbackTrait, ScreenDescriptor};
use glam::Vec3;
use std::{collections::HashMap, sync::Arc};
use wgpu::util::DeviceExt;
const RINGS: usize = 48;
const SPOKES: usize = 96;
/// Metres of simulation become visible deformation, relative to head radius.
/// @tune 2..100
const VIEW_GAIN: f32 = 8.0;
/// Vertical perspective angle of the lab camera.
/// @tune 30..70 deg
const FIELD_OF_VIEW: f32 = 48.0;
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    material: f32,
    displacement: f32,
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Camera {
    vp: [[f32; 4]; 4],
    eye: [f32; 4],
    background: [f32; 4],
}
#[derive(Default)]
struct Mesh {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}
impl Mesh {
    fn vertex(&mut self, p: Vec3, n: Vec3, uv: [f32; 2], material: f32, d: f32) -> u32 {
        let i = self.vertices.len() as u32;
        self.vertices.push(Vertex {
            position: p.to_array(),
            normal: n.to_array(),
            uv,
            material,
            displacement: d,
        });
        i
    }
    fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.indices.extend_from_slice(&[a, b, c, a, c, d]);
    }
    fn head(&mut self, field: &[f32], radius: f32, y: f32, bottom: bool) {
        let base = self.vertices.len() as u32;
        for r in 0..RINGS {
            for s in 0..SPOKES {
                let a = s as f32 * std::f32::consts::TAU / SPOKES as f32;
                let rr = r as f32 / (RINGS - 1) as f32;
                let z = field.get(r * SPOKES + s).copied().unwrap_or(0.0) * crate::tune!(VIEW_GAIN);
                self.vertex(
                    Vec3::new(radius * rr * a.cos(), y - z, radius * rr * a.sin()),
                    if bottom { -Vec3::Y } else { Vec3::Y },
                    [rr * a.cos(), rr * a.sin()],
                    0.0,
                    z,
                );
            }
        }
        for r in 0..RINGS - 1 {
            for s in 0..SPOKES {
                let a = base + (r * SPOKES + s) as u32;
                let b = base + (r * SPOKES + (s + 1) % SPOKES) as u32;
                let c = base + ((r + 1) * SPOKES + (s + 1) % SPOKES) as u32;
                let d = base + ((r + 1) * SPOKES + s) as u32;
                if bottom {
                    self.quad(a, d, c, b);
                } else {
                    self.quad(a, b, c, d);
                }
            }
        }
        // Area-weighted normals from the actual deformed triangles.
        let end = self.vertices.len();
        let mut normals = vec![Vec3::ZERO; end - base as usize];
        for tri in self.indices.chunks_exact(3).filter(|t| t[0] >= base) {
            let [a, b, c] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
            let n = (Vec3::from(self.vertices[b].position) - Vec3::from(self.vertices[a].position))
                .cross(
                    Vec3::from(self.vertices[c].position) - Vec3::from(self.vertices[a].position),
                );
            for i in [a, b, c] {
                normals[i - base as usize] += n;
            }
        }
        for (v, n) in self.vertices[base as usize..end].iter_mut().zip(normals) {
            v.normal = n
                .try_normalize()
                .unwrap_or(if bottom { -Vec3::Y } else { Vec3::Y })
                .to_array();
        }
    }
    fn tube(&mut self, a: Vec3, b: Vec3, radius: f32, material: f32) {
        let axis = (b - a).normalize_or_zero();
        let u = axis
            .cross(if axis.y.abs() > 0.9 { Vec3::X } else { Vec3::Y })
            .normalize_or_zero();
        let v = axis.cross(u);
        let base = self.vertices.len() as u32;
        for p in [a, b] {
            for s in 0..8 {
                let angle = s as f32 * std::f32::consts::TAU / 8.;
                let n = u * angle.cos() + v * angle.sin();
                self.vertex(p + n * radius, n, [0., 0.], material, 0.);
            }
        }
        for s in 0..8 {
            self.quad(
                base + s,
                base + (s + 1) % 8,
                base + 8 + (s + 1) % 8,
                base + 8 + s,
            );
        }
    }
    fn hoop(&mut self, radius: f32, y: f32, thickness: f32) {
        for i in 0..96 {
            let a = i as f32 * std::f32::consts::TAU / 96.;
            let b = (i + 1) as f32 * std::f32::consts::TAU / 96.;
            self.tube(
                Vec3::new(radius * a.cos(), y, radius * a.sin()),
                Vec3::new(radius * b.cos(), y, radius * b.sin()),
                thickness,
                2.,
            );
        }
    }
    fn sphere(&mut self, p: Vec3, r: f32) {
        let base = self.vertices.len() as u32;
        for i in 0..=12 {
            for j in 0..24 {
                let a = i as f32 * std::f32::consts::PI / 12.;
                let b = j as f32 * std::f32::consts::TAU / 24.;
                let n = Vec3::new(a.sin() * b.cos(), a.cos(), a.sin() * b.sin());
                self.vertex(p + n * r, n, [0., 0.], 3., 0.);
            }
        }
        for i in 0..12 {
            for j in 0..24 {
                let a = base + i * 24 + j;
                let b = base + i * 24 + (j + 1) % 24;
                self.quad(a, a + 24, b + 24, b);
            }
        }
    }
}

pub struct Scene {
    pub id: usize,
    /// A pitch helix shares the depth-tested lab renderer and camera.
    pub chord: Option<Vec<crate::theory::harmony::Tone>>,
    pub patch: Patch,
    pub animation: Option<Arc<Animation>>,
    pub time: Option<f32>,
    pub standing: usize,
    pub camera: [f32; 3],
    pub background: [f32; 4],
    pub size: [f32; 2],
}
fn geometry(scene: &Scene) -> Mesh {
    if let Some(tones) = &scene.chord {
        return chord_geometry(tones);
    }
    let mut mesh = Mesh::default();
    let radius = scene.patch.get(BATTER_RADIUS) as f32;
    let bottom_radius = scene.patch.get(RESONANT_RADIUS) as f32 / radius;
    let height = (scene.patch.get(SHELL_SHELL_HEIGHT) as f32 / radius).clamp(0.2, 2.5);
    let (top, bottom) = if let Some(animation) = &scene.animation {
        if let Some(t) = scene.time {
            (
                animation.field(t, RINGS, SPOKES, false),
                animation.field(t, RINGS, SPOKES, true),
            )
        } else {
            let mode = scene.standing.min(animation.top.len().saturating_sub(1));
            let mut q = vec![0.; animation.top.len()];
            if let Some(v) = q.get_mut(mode) {
                *v = 0.0015;
            }
            (
                Animation::mesh_field(&animation.top, &q, RINGS, SPOKES),
                vec![0.; RINGS * SPOKES],
            )
        }
    } else {
        (vec![0.; RINGS * SPOKES], vec![0.; RINGS * SPOKES])
    };
    // Physical amplitude is in metres; world units use the batter radius.
    let top: Vec<_> = top.iter().map(|v| v / radius).collect();
    let bottom: Vec<_> = bottom.iter().map(|v| v / radius).collect();
    mesh.head(&top, 1., height * 0.5, false);
    mesh.head(&bottom, bottom_radius, -height * 0.5, true);
    for (r, y) in [(1.02, height * 0.5), (bottom_radius * 1.02, -height * 0.5)] {
        mesh.hoop(r, y, 0.032);
        mesh.hoop(r, y + if y > 0. { 0.055 } else { -0.055 }, 0.016);
    }
    // A fixed front cutaway makes the second head and contacts visible.
    for i in 0..96 {
        let a = i as f32 * std::f32::consts::TAU / 96.;
        let b = (i + 1) as f32 * std::f32::consts::TAU / 96.;
        if a.sin() > 0.25 {
            continue;
        }
        let n = Vec3::new(a.cos(), 0., a.sin());
        let nn = Vec3::new(b.cos(), 0., b.sin());
        let v0 = mesh.vertex(n * 1.01 + Vec3::Y * height * 0.5, n, [0., 0.], 1., 0.);
        let v1 = mesh.vertex(
            n * bottom_radius * 1.01 - Vec3::Y * height * 0.5,
            n,
            [0., 1.],
            1.,
            0.,
        );
        let v2 = mesh.vertex(
            nn * bottom_radius * 1.01 - Vec3::Y * height * 0.5,
            nn,
            [1., 1.],
            1.,
            0.,
        );
        let v3 = mesh.vertex(nn * 1.01 + Vec3::Y * height * 0.5, nn, [1., 0.], 1., 0.);
        mesh.quad(v0, v1, v2, v3);
    }
    for i in 0..10 {
        let a = i as f32 * std::f32::consts::TAU / 10.;
        let n = Vec3::new(a.cos(), 0., a.sin());
        mesh.tube(
            n * 1.065 + Vec3::Y * height * 0.5,
            n * bottom_radius * 1.065 - Vec3::Y * height * 0.5,
            0.019,
            2.,
        );
    }
    let count = scene.patch.get(WIRES_COUNT).round() as usize;
    for i in 0..count {
        let x =
            (i as f32 - (count as f32 - 1.) * 0.5) * scene.patch.get(WIRES_SPACING) as f32 / radius;
        let limit = (bottom_radius * bottom_radius - x * x).max(0.).sqrt();
        let gap = scene.patch.get(WIRES_REST_GAP) as f32 / radius;
        let motion = scene
            .animation
            .as_ref()
            .and_then(|a| {
                a.wire_frames
                    .get((scene.time.unwrap_or(0.) * 240.) as usize)
            })
            .and_then(|f| f.get(i))
            .copied()
            .unwrap_or(0.)
            / radius
            * crate::tune!(VIEW_GAIN);
        for j in 0..32 {
            let z = |k: usize| -limit + 2. * limit * k as f32 / 32.;
            let y = |k: usize| {
                -height * 0.5 - 0.045 - gap - motion * (std::f32::consts::PI * k as f32 / 32.).sin()
            };
            mesh.tube(
                Vec3::new(x, y(j), z(j)),
                Vec3::new(x, y(j + 1), z(j + 1)),
                0.005,
                4.,
            );
        }
    }
    let strike = scene.patch.get(STRIKE_POSITION) as f32;
    let a = scene.patch.get(STRIKE_ANGLE) as f32 * std::f32::consts::PI / 180.;
    let lift = scene
        .time
        .and_then(|t| {
            scene
                .animation
                .as_ref()
                .and_then(|a| a.mallet.get((t * 240.0) as usize))
        })
        .map_or(0.35, |q| 0.075 - (*q / radius).clamp(-1.3, 0.04));
    let mallet = Vec3::new(strike * a.cos(), height * 0.5 + lift, strike * a.sin());
    mesh.sphere(mallet, 0.075);
    mesh.tube(
        mallet + Vec3::new(0., 0.04, 0.),
        mallet + Vec3::new(0.35, 0.85, -0.2),
        0.019,
        4.,
    );
    let distance = scene.patch.get(MIC_MIC_DISTANCE) as f32 / radius;
    let angle = (scene.patch.get(MIC_MIC_ANGLE) as f32).to_radians();
    let mic = Vec3::new(
        distance * angle.sin(),
        height * 0.5 + scene.patch.get(MIC_MIC_HEIGHT) as f32 / radius,
        distance * angle.cos(),
    );
    mesh.tube(mic, mic + Vec3::new(0.26, 0.13, 0.10), 0.065, 1.);
    mesh.sphere(mic, 0.066);
    let floor = -height * 0.5 - 0.23;
    let mut ids = Vec::new();
    for (x, z) in [(-8., -8.), (-8., 8.), (8., 8.), (8., -8.)] {
        ids.push(mesh.vertex(Vec3::new(x, floor, z), Vec3::Y, [x, z], 5., 0.));
    }
    mesh.quad(ids[0], ids[1], ids[2], ids[3]);
    mesh
}

/// Absolute pitch class fixes the angle; actual register fixes height. A
/// common centre translation only frames the object, never changes spacing.
pub fn chord_positions(tones: &[crate::theory::harmony::Tone]) -> Vec<Vec3> {
    let center = tones.first().zip(tones.last()).map_or(60., |(a, b)| {
        (f32::from(a.pitch) + f32::from(b.pitch)) * 0.5
    });
    tones
        .iter()
        .map(|t| {
            let a =
                f32::from(t.pitch % 12) * std::f32::consts::TAU / 12. - std::f32::consts::FRAC_PI_2;
            Vec3::new(
                a.cos(),
                (f32::from(t.pitch) - center) * 0.12 + 0.275,
                a.sin(),
            )
        })
        .collect()
}
fn chord_geometry(tones: &[crate::theory::harmony::Tone]) -> Mesh {
    let mut mesh = Mesh::default();
    let positions = chord_positions(tones);
    let floor = positions.first().map_or(-0.7, |p| p.y - 0.35);
    let top = positions.last().map_or(0.7, |p| p.y + 0.18);
    // Quiet pitch-class ring, radial ticks, and a vertical register spine.
    for i in 0..96 {
        let a = i as f32 * std::f32::consts::TAU / 96.;
        let b = (i + 1) as f32 * std::f32::consts::TAU / 96.;
        mesh.tube(
            Vec3::new(a.cos() * 1.16, floor, a.sin() * 1.16),
            Vec3::new(b.cos() * 1.16, floor, b.sin() * 1.16),
            0.004,
            7.,
        );
    }
    mesh.tube(Vec3::new(0., floor, 0.), Vec3::new(0., top, 0.), 0.004, 7.);
    for (i, p) in positions.iter().enumerate() {
        let tone = &tones[i];
        let material = if tone.degree == 1 {
            6.
        } else if tone.extension {
            8.
        } else {
            7.
        };
        let first = mesh.vertices.len();
        mesh.sphere(*p, if tone.degree == 1 { 0.105 } else { 0.077 });
        for v in &mut mesh.vertices[first..] {
            v.material = material;
        }
        mesh.tube(*p, Vec3::new(p.x, floor, p.z), 0.003, 7.);
        if i > 0 {
            mesh.tube(positions[i - 1], *p, 0.016, material);
        }
        if i > 1 {
            let a = positions[0];
            let b = positions[i - 1];
            let c = *p;
            let n = (b - a).cross(c - a).normalize_or_zero();
            let va = mesh.vertex(a, n, [0., 0.], 9., 0.);
            let vb = mesh.vertex(b, n, [0., 0.], 9., 0.);
            let vc = mesh.vertex(c, n, [0., 0.], 9., 0.);
            mesh.indices.extend([va, vb, vc]);
        }
    }
    mesh
}

struct Surface {
    size: [u32; 2],
    colour: wgpu::TextureView,
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    bind: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    camera: wgpu::BindGroup,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    capacity: [u64; 2],
    seen: std::time::Instant,
}
struct Renderer {
    format: wgpu::TextureFormat,
    pipeline: Option<wgpu::RenderPipeline>,
    blit: Option<wgpu::RenderPipeline>,
    surfaces: HashMap<usize, Surface>,
}
pub fn install(renderer: &mut egui_wgpu::Renderer, format: wgpu::TextureFormat) {
    renderer.callback_resources.insert(Renderer {
        format,
        pipeline: None,
        blit: None,
        surfaces: HashMap::new(),
    });
}
fn pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    blit: bool,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("membrane shader"),
        source: wgpu::ShaderSource::Wgsl(
            if blit {
                include_str!("kiln_blit.wgsl")
            } else {
                include_str!("kiln.wgsl")
            }
            .into(),
        ),
    });
    const ATTRS: [wgpu::VertexAttribute; 5] =
        wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2,3=>Float32,4=>Float32];
    let buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRS,
    })];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("membrane pipeline"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            buffers: if blit { &[] } else { &buffers },
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: if blit {
            None
        } else {
            Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            })
        },
        multisample: wgpu::MultisampleState {
            count: if blit { 1 } else { 4 },
            ..Default::default()
        },
        multiview_mask: None,
        cache: None,
    })
}
fn texture(
    device: &wgpu::Device,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("kiln attachment"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | if samples == 1 {
                    wgpu::TextureUsages::TEXTURE_BINDING
                } else {
                    wgpu::TextureUsages::empty()
                },
            view_formats: &[],
        })
        .create_view(&Default::default())
}
impl CallbackTrait for Scene {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(renderer) = resources.get_mut::<Renderer>() else {
            return vec![];
        };
        if renderer.pipeline.is_none() {
            renderer.pipeline = Some(pipeline(device, wgpu::TextureFormat::Rgba8UnormSrgb, false));
            renderer.blit = Some(pipeline(device, renderer.format, true));
        }
        let (Some(pipeline), Some(blit)) = (&renderer.pipeline, &renderer.blit) else {
            return vec![];
        };
        let size = self
            .size
            .map(|v| (v * screen.pixels_per_point).round().clamp(1., 2048.) as u32);
        let mesh = geometry(self);
        let vb = bytemuck::cast_slice(&mesh.vertices);
        let ib = bytemuck::cast_slice(&mesh.indices);
        renderer
            .surfaces
            .retain(|_, s| s.seen.elapsed().as_secs() < 10);
        let rebuild = renderer.surfaces.get(&self.id).is_none_or(|s| {
            s.size != size || s.capacity[0] < vb.len() as u64 || s.capacity[1] < ib.len() as u64
        });
        if rebuild {
            let colour = texture(device, size, wgpu::TextureFormat::Rgba8UnormSrgb, 1);
            let msaa = texture(device, size, wgpu::TextureFormat::Rgba8UnormSrgb, 4);
            let depth = texture(device, size, wgpu::TextureFormat::Depth32Float, 4);
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &blit.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&colour),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });
            let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: std::mem::size_of::<Camera>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let camera = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                }],
            });
            let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: vb,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: ib,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            });
            renderer.surfaces.insert(
                self.id,
                Surface {
                    size,
                    colour,
                    msaa,
                    depth,
                    bind,
                    uniform,
                    camera,
                    vertices,
                    indices,
                    capacity: [vb.len() as u64, ib.len() as u64],
                    seen: std::time::Instant::now(),
                },
            );
        }
        let Some(s) = renderer.surfaces.get_mut(&self.id) else {
            return vec![];
        };
        s.seen = std::time::Instant::now();
        queue.write_buffer(&s.vertices, 0, vb);
        queue.write_buffer(&s.indices, 0, ib);
        let [yaw, pitch, distance] = self.camera;
        let aspect = size[0] as f32 / size[1] as f32;
        let distance = distance * (0.9 / aspect).max(1.0);
        let eye = Vec3::new(
            yaw.sin() * pitch.cos(),
            pitch.sin(),
            yaw.cos() * pitch.cos(),
        ) * distance;
        let view = glam::camera::rh::view::look_at_mat4(eye, Vec3::new(0., 0.10, 0.), Vec3::Y);
        let projection = glam::camera::rh::proj::directx::perspective(
            crate::tune!(FIELD_OF_VIEW).to_radians(),
            aspect,
            0.05,
            40.,
        );
        let camera = Camera {
            vp: (projection * view).to_cols_array_2d(),
            eye: eye.extend(1.).to_array(),
            background: self.background,
        };
        queue.write_buffer(&s.uniform, 0, bytemuck::bytes_of(&camera));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("membrane 3D"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &s.msaa,
                resolve_target: Some(&s.colour),
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: self.background[0] as f64,
                        g: self.background[1] as f64,
                        b: self.background[2] as f64,
                        a: 1.,
                    }),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &s.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &s.camera, &[]);
        pass.set_vertex_buffer(0, s.vertices.slice(..));
        pass.set_index_buffer(s.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
        vec![]
    }
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let Some(r) = resources.get::<Renderer>() else {
            return;
        };
        if let (Some(p), Some(s)) = (&r.blit, r.surfaces.get(&self.id)) {
            pass.set_pipeline(p);
            pass.set_bind_group(0, &s.bind, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
