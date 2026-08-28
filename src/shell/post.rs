//! The pass that runs after everything: the frame, read back and
//! resettled.
//!
//! # What it is for
//!
//! Not a CRT costume. Three effects, each doing one thing, and all of
//! them motionless — the design guide bans idle animation and a flicker
//! would break that rule harder than anything else in the app.
//!
//! - **SOFTEN.** A three-tap horizontal filter. This interface is built
//!   almost entirely from one-pixel hairlines, and a hairline on a
//!   modern flat panel is a maximally aliased edge: the sharpest thing
//!   the display can show, on every divider, every rule, every cell
//!   border, all at once. A small amount of lateral bleed takes the
//!   ring off those edges. It is the one effect here that genuinely
//!   argues for itself on legibility rather than on feel.
//!
//! - **SCANLINE.** A luminance modulation on alternate PHYSICAL pixel
//!   rows, at an amplitude of about a percent. Individually invisible;
//!   collectively it gives large flat surfaces a texture, so a panel
//!   reads as a surface rather than as an absence. It is measured in
//!   device pixels, not points, so it does not grow into a stripe on a
//!   scaled display.
//!
//! - **VIGNETTE.** A few percent at the extreme corners. The window is
//!   a dark rectangle on a dark desktop; a corner that falls off gives
//!   the frame an edge without drawing one.
//!
//! # Exact bypass
//!
//! [`Amounts::NONE`] returns the sampled texel unchanged, bit for bit —
//! the promise every colour stage in this codebase makes, and the one
//! that matters most here because this is the only stage that cannot be
//! taken out of the chain to hear what it was doing. The shader's own
//! arithmetic is arranged around it: every term is a multiply-add
//! against an amount, so zero is identity by construction rather than
//! by a branch that might be wrong.

/// How hard each term is applied, `0..=1` throughout.
///
/// The shape of this is [`crt-screen.frag`]'s: a master [`intensity`]
/// dial with the individual terms hanging off it, so "turn the tube
/// down" is one number and "I want less of the grille specifically" is
/// still available.
///
/// [`crt-screen.frag`]: ../../../.config/hypr/shaders/crt-screen.frag
/// [`intensity`]: Amounts::intensity
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Amounts {
    /// The master. Zero is a clean screen, one is the tube as tuned.
    pub intensity: f32,
    /// Barrel distortion. OFF by default and deliberately so — see the
    /// header of `post.wgsl`. It moves pixels away from where the
    /// pointer is, which a desktop can absorb and a card full of drag
    /// handles cannot.
    pub curve: f32,
    pub scanline: f32,
    pub grille: f32,
    pub bloom: f32,
    pub chroma: f32,
    pub vignette: f32,
    pub grain: f32,
    /// The saturation-and-contrast grade from `cyberpunk.glsl`.
    pub neon: f32,
    /// Ours, not theirs: the banding fix. See `post.wgsl`.
    pub dither: f32,
}

impl Amounts {
    /// The identity. Present as a named value because "off" has to be a
    /// setting, not the absence of one.
    pub const NONE: Self = Self {
        intensity: 0.0,
        curve: 0.0,
        scanline: 0.0,
        grille: 0.0,
        bloom: 0.0,
        chroma: 0.0,
        vignette: 0.0,
        grain: 0.0,
        neon: 0.0,
        dither: 0.0,
    };
}

impl Default for Amounts {
    /// THEIR TUNING, at full. The constants in `post.wgsl` are carried
    /// over from the screen shader unchanged, so every term here is
    /// simply "all of it" — the place to disagree with the look is the
    /// constant, not a fraction of it applied here.
    ///
    /// Two exceptions, both argued in `post.wgsl`'s header: the curve
    /// is off, and the dither is ours and runs at full because it is
    /// smaller than one code value by construction.
    fn default() -> Self {
        Self {
            intensity: 1.0,
            curve: 0.0,
            scanline: 1.0,
            grille: 1.0,
            bloom: 1.0,
            chroma: 1.0,
            vignette: 1.0,
            grain: 1.0,
            neon: 1.0,
            dither: 1.0,
        }
    }
}

/// What the shader reads. `repr(C)` and padded to sixty-four bytes,
/// because a uniform buffer's layout is a contract with the GPU and
/// getting it wrong is silent until the first draw call.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    /// The target's size in PHYSICAL pixels — everything in the shader
    /// is a pixel grid effect and has to know the real grid.
    resolution: [f32; 2],
    intensity: f32,
    curve: f32,
    scanline: f32,
    grille: f32,
    bloom: f32,
    chroma: f32,
    vignette: f32,
    grain: f32,
    neon: f32,
    dither: f32,
    /// Non-zero puts the shader in TEST PATTERN mode — see
    /// [`Post::set_test`].
    test: f32,
    /// To sixty-four bytes. The two sides have to agree byte for byte
    /// and nothing but a draw call will say so if they do not.
    _pad: [f32; 3],
}

/// The full-screen pass: a triangle, a sampler, and the frame.
pub struct Post {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    bind: Option<(wgpu::BindGroup, u64)>,
    amounts: Amounts,
    test: bool,
}

impl Post {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, amounts: Amounts) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!("post.wgsl"))),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post_bind_layout"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post_pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("post_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                buffers: &[],
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
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        // NEAREST on the outside, because the shader does its own
        // sampling by texel and a linear filter under it would blur
        // twice — once where we asked and once where we did not.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post_sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("post_uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            sampler,
            uniforms,
            bind: None,
            amounts,
            test: false,
        }
    }

    /// TEST PATTERN: make the pass impossible to miss.
    ///
    /// A diagnostic, not a feature. A treatment tuned to be subtle and
    /// a treatment that is not running look identical from a chair, and
    /// the difference matters a great deal to whoever is tuning it —
    /// so there is a switch that answers the question outright. It also
    /// answers a second one for free: if the bands come out soft or at
    /// the wrong pitch, something between this pass and the glass is
    /// resampling the frame, and no amount of tuning here would ever
    /// have fixed that.
    pub fn set_test(&mut self, test: bool) {
        self.test = test;
    }

    /// What the treatment is set to, for the shell to write down.
    pub fn amounts(&self) -> Amounts {
        self.amounts
    }

    /// The bind group is rebuilt only when the view it points at
    /// changes, which is on resize — `generation` is what says so.
    fn ensure_bind_group(
        &mut self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        generation: u64,
    ) {
        if self.bind.as_ref().is_none_or(|(_, at)| *at != generation) {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post_bind"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.uniforms.as_entire_binding(),
                    },
                ],
            });
            self.bind = Some((group, generation));
        }
    }

    /// Draw the offscreen frame onto `target`.
    #[expect(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        generation: u64,
        target: &wgpu::TextureView,
        size: [u32; 2],
    ) {
        queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::cast_slice(&[Uniforms {
                resolution: [size[0].max(1) as f32, size[1].max(1) as f32],
                intensity: self.amounts.intensity.clamp(0.0, 1.0),
                curve: self.amounts.curve.clamp(0.0, 1.0),
                scanline: self.amounts.scanline.clamp(0.0, 1.0),
                grille: self.amounts.grille.clamp(0.0, 1.0),
                bloom: self.amounts.bloom.clamp(0.0, 1.0),
                chroma: self.amounts.chroma.clamp(0.0, 1.0),
                vignette: self.amounts.vignette.clamp(0.0, 1.0),
                grain: self.amounts.grain.clamp(0.0, 1.0),
                neon: self.amounts.neon.clamp(0.0, 1.0),
                dither: self.amounts.dither.clamp(0.0, 1.0),
                test: f32::from(u8::from(self.test)),
                _pad: [0.0; 3],
            }]),
        );
        self.ensure_bind_group(device, source, generation);
        let Some((bind, _)) = &self.bind else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("post_pass"),
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
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind, &[]);
        // One oversized triangle rather than two: no seam down the
        // diagonal, and three vertices generated in the shader instead
        // of a vertex buffer nobody would ever change.
        pass.draw(0..3, 0..1);
    }
}

/// Where the shell keeps the treatment's settings.
pub const STORAGE_KEY: &str = "post";

/// The order the terms are written down in, and read back.
const FIELDS: usize = 10;

impl serde::Serialize for Amounts {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        [
            self.intensity,
            self.curve,
            self.scanline,
            self.grille,
            self.bloom,
            self.chroma,
            self.vignette,
            self.grain,
            self.neon,
            self.dither,
        ]
        .serialize(s)
    }
}

impl<'de> serde::Deserialize<'de> for Amounts {
    /// A setting stored by an earlier shape of this struct fails to
    /// parse and the caller falls back to [`Amounts::default`] — which
    /// is the right answer for a visual preference, and the reason the
    /// caller was written to fall back rather than to fail.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = <[f32; FIELDS]>::deserialize(d)?;
        let at = |i: usize| v.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
        Ok(Self {
            intensity: at(0),
            curve: at(1),
            scanline: at(2),
            grille: at(3),
            bloom: at(4),
            chroma: at(5),
            vignette: at(6),
            grain: at(7),
            neon: at(8),
            dither: at(9),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Their constants, mirrored from the top of `post.wgsl`. They live
    // here because nothing in Rust reads them — the GPU does — and what
    // they are FOR is stating the claims below as arithmetic. The
    // agreement between these and the shader's own copies is held by
    // review; the alternative was a build script that parses WGSL.
    const MASK_PITCH: usize = 3;
    const MASK_DEPTH: f32 = 0.45;
    const SCAN_PITCH: f32 = 2.0;
    const SCAN_DEPTH: f32 = 0.22;
    const GAIN: f32 = 1.0 / 0.53;
    const CONTRAST: f32 = 0.18;
    const CONTRAST_PIVOT: f32 = 0.14;
    const RELIEF: f32 = 0.88;
    const TINT: [f32; 3] = [1.00, 0.97, 0.91];
    const GRAIN: f32 = 4.0;
    const DITHER_CEILING: f32 = 0.5 / 255.0;

    /// The interface's real grounds, in encoded units. The treatment has
    /// to be judged against THESE, not a mid-grey test card — two
    /// earlier versions of this shader were tuned against nothing in
    /// particular and both came out invisible on the app.
    const GROUND: f32 = 21.0 / 255.0;
    const INK: f32 = 228.0 / 255.0;

    /// A CPU model of the shader's POINTWISE terms: mask, scanline,
    /// gain, tint. Bloom, chroma and the curve are neighbourhood or
    /// geometry effects and need a neighbourhood; they are left out on
    /// purpose rather than modelled badly.
    ///
    /// Encoded units throughout, the same space the shader works in.
    /// `relief` is `0.0` in the middle of a flat surface and `1.0` on
    /// the edge of a glyph — see `post.wgsl`. It gates everything that
    /// models the tube.
    fn model(rgb: [f32; 3], row: usize, column: usize, relief: f32, a: Amounts) -> [f32; 3] {
        let tube = a.intensity * (1.0 - relief.clamp(0.0, 1.0) * RELIEF);
        let line = 0.5 + 0.5 * (std::f32::consts::TAU * row as f32 / SCAN_PITCH).cos();
        let stripe = column % MASK_PITCH;
        let lit = a.grille * a.scanline * tube;
        let mut out = [0.0f32; 3];
        for (channel, slot) in out.iter_mut().enumerate() {
            let mask = if channel == stripe { 1.0 } else { MASK_DEPTH };
            let masked = rgb[channel] * (1.0 + (mask - 1.0) * a.grille * tube);
            let scanned = masked * (1.0 - SCAN_DEPTH * a.scanline * tube * line);
            // The soft gain — see `post.wgsl`. Same slope at the
            // bottom, asymptotically unity at the top, so it cannot
            // clip.
            let g = 1.0 + (GAIN - 1.0) * lit;
            let gained = scanned * g / (1.0 + (g - 1.0) * scanned);
            let tinted = gained * (1.0 + (TINT[channel] - 1.0) * lit);
            // The grade, pivoted where this interface's tones are, and
            // relaxing to nothing as it approaches white — see
            // `post.wgsl`.
            let grade = a.neon * tube;
            let t = ((tinted - 0.75) / 0.25).clamp(0.0, 1.0);
            let headroom = 1.0 - t * t * (3.0 - 2.0 * t);
            *slot = ((tinted - CONTRAST_PIVOT) * (1.0 + CONTRAST * grade * headroom)
                + CONTRAST_PIVOT)
                .clamp(0.0, 1.0);
        }
        out
    }

    fn hash12(px: f32, py: f32) -> f32 {
        let mut p3 = [
            (px * 0.1031).fract(),
            (py * 0.1031).fract(),
            (px * 0.1031).fract(),
        ];
        let d = p3[0] * (p3[1] + 33.33) + p3[1] * (p3[2] + 33.33) + p3[2] * (p3[0] + 33.33);
        for v in p3.iter_mut() {
            *v += d;
        }
        ((p3[0] + p3[1]) * p3[2]).fract()
    }

    fn bayer(x: usize, y: usize) -> f32 {
        const M: [f32; 16] = [
            0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0,
        ];
        M[(y & 3) * 4 + (x & 3)] / 16.0 - 0.46875
    }

    /// THE UNIFORM BLOCK IS THE SIZE THE SHADER EXPECTS, and 16-aligned.
    ///
    /// A layout that disagrees with `post.wgsl` is not a compile error
    /// in either language — it is a validation error at the first draw,
    /// which is to say a crash on launch. That happened once already.
    #[test]
    fn the_uniform_block_is_the_size_the_shader_expects() {
        assert_eq!(std::mem::size_of::<Uniforms>(), 64);
        assert_eq!(std::mem::size_of::<Uniforms>() % 16, 0);
    }

    /// THE SHADER AND THE STRUCT AGREE, checked against the shader's own
    /// text rather than against a number somebody remembered.
    ///
    /// This has now been the crash-on-launch twice: once when a
    /// `vec3<f32>` of padding aligned to 16 and pushed the block from 32
    /// to 48, and again when the same field pushed it from 64 to 80.
    /// Neither is a compile error in either language, and neither shows
    /// up until a draw call validates the bind group — so "held by
    /// review" was tried, and review missed it both times.
    ///
    /// So the test parses `post.wgsl`'s `Uniforms` and applies WGSL's
    /// uniform address space rules: `f32` is align 4 size 4, `vec2` is
    /// align 8 size 8, `vec3` is align 16 size 12, `vec4` is align 16
    /// size 16, and the struct rounds up to its largest member's
    /// alignment — a minimum of 16 for a uniform.
    #[test]
    fn the_shader_and_the_struct_agree_on_the_layout() {
        let source = include_str!("post.wgsl");
        let body = source
            .split_once("struct Uniforms {")
            .and_then(|(_, rest)| rest.split_once("};"))
            .map(|(body, _)| body)
            .expect("post.wgsl declares a Uniforms struct");

        let mut offset = 0usize;
        let mut align = 16usize; // a uniform struct is at least 16-aligned
        let mut fields = 0usize;
        for line in body.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") {
                continue;
            }
            let ty = line
                .split_once(':')
                .map(|(_, ty)| ty.trim().trim_end_matches(','))
                .expect("a field is `name: type,`");
            let (a, size) = match ty {
                "f32" | "i32" | "u32" => (4, 4),
                "vec2<f32>" => (8, 8),
                "vec3<f32>" => (16, 12),
                "vec4<f32>" => (16, 16),
                other => panic!("the layout rules do not cover {other}"),
            };
            offset = offset.div_ceil(a) * a;
            offset += size;
            align = align.max(a);
            fields += 1;
        }
        let wgsl = offset.div_ceil(align) * align;

        assert!(fields > 4, "the struct did not parse: {fields} fields");
        assert_eq!(
            wgsl,
            std::mem::size_of::<Uniforms>(),
            "post.wgsl lays Uniforms out at {wgsl} bytes, the struct at {}",
            std::mem::size_of::<Uniforms>()
        );
    }

    /// ZERO INTENSITY IS AN EXACT BYPASS. The master dial gates every
    /// term, so one number turns the whole tube off — and it has to
    /// leave the frame untouched to the bit, or "off" is a look of its
    /// own.
    #[test]
    fn zero_intensity_is_an_exact_bypass() {
        for value in [0.0f32, GROUND, 0.5, INK, 1.0] {
            for row in 0..4 {
                for column in 0..6 {
                    let rgb = [value; 3];
                    assert_eq!(
                        model(rgb, row, column, 0.0, Amounts::NONE),
                        rgb,
                        "row {row} column {column} moved at zero intensity"
                    );
                    // And the master alone is enough, with every other
                    // term left at full.
                    let master_off = Amounts {
                        intensity: 0.0,
                        ..Amounts::default()
                    };
                    assert_eq!(
                        model(rgb, row, column, 0.0, master_off),
                        rgb,
                        "the master leaked"
                    );
                }
            }
        }
    }

    /// THE MASK AND THE SCANLINES ARE PURE ATTENUATION, and the gain is
    /// what makes that affordable.
    ///
    /// This is the structural difference from what was here before. The
    /// old version modulated around the mean so it cost no light and
    /// could never be strong; theirs throws most of the light away and
    /// puts it back with `GAIN`, which is why a real tube reads as a
    /// tube. The test is that the two halves actually balance: a lit
    /// phosphor on a bright line comes back to roughly where it started.
    #[test]
    fn the_gain_puts_back_what_the_mask_and_the_lines_take() {
        let a = Amounts::default();
        // `line` is a raised cosine, so it peaks — and therefore
        // ATTENUATES most — on EVEN rows. Row 1 is the gap between two
        // beam passes and takes no scanline attenuation at all; column
        // 0 is the red phosphor's stripe.
        let best = model([GROUND; 3], 1, 0, 0.0, a)[0];
        let worst = model([GROUND; 3], 0, 1, 0.0, a)[0];
        assert!(
            best > GROUND * 0.9,
            "a lit phosphor between beam passes fell to {best} from {GROUND}"
        );
        assert!(
            worst < GROUND * 0.6,
            "an off phosphor under the beam line is {worst} — the mask does nothing"
        );
    }

    /// AND IT IS VISIBLE ON THE GROUNDS THE APP ACTUALLY USES.
    ///
    /// The test that would have caught the two attempts before this
    /// one. Several 8-bit levels between neighbouring rows on the DARK
    /// surfaces, because that is what almost all of this interface is.
    #[test]
    fn the_texture_is_there_on_the_real_grounds() {
        let a = Amounts::default();
        // Odd rows sit between beam passes and are the brighter of the
        // pair — see `the_gain_puts_back_what_the_mask_and_the_lines_take`.
        let lit = model([GROUND; 3], 1, 0, 0.0, a)[0] * 255.0;
        let dark = model([GROUND; 3], 0, 0, 0.0, a)[0] * 255.0;
        let levels = lit - dark;
        assert!(
            levels >= 4.0,
            "adjacent rows differ by {levels:.1}/255 — invisible on the app's own ground"
        );
    }

    /// NOTHING IS PUSHED THROUGH THE TOP. The gain is a multiply of
    /// nearly one and a half, and every value readout in this app is
    /// near white — so the one thing that could go wrong here is that
    /// text saturates into a solid block.
    #[test]
    fn bright_ink_survives_the_gain() {
        let a = Amounts::default();
        for row in 0..4 {
            for column in 0..MASK_PITCH {
                let out = model([INK; 3], row, column, 0.0, a);
                for channel in out {
                    assert!((0.0..=1.0).contains(&channel));
                }
                // The lit phosphor between beam passes is the
                // brightest case there is; if that clips, one column in
                // three of every glyph goes flat. Their straight
                // multiply DID clip here — the soft gain is why this
                // holds.
                if row % 2 == 1 {
                    let lit = out[column];
                    assert!(lit < 1.0, "ink clipped to {lit} at column {column}");
                    // Against the channel's OWN tinted level: the
                    // amber cast deliberately pulls blue down, so
                    // "brighter than it went in" is only true for red.
                    // What must hold everywhere is that the gain more
                    // than covers what the mask and the lines took.
                    assert!(
                        lit > INK * TINT[column],
                        "the gain failed to cover the loss on channel {column}"
                    );
                }
            }
        }
    }

    /// THE SURFACE LADDER SURVIVES, and this is the test that would
    /// have caught the port.
    ///
    /// The house style is a ladder of near-black grounds — app, card,
    /// well, sub-well — and every one of them lives between 13 and 42
    /// out of 255. Ported verbatim, the tube put SUNKEN AND BG BOTH AT
    /// ZERO and the raised card at 8: four distinct surfaces arriving
    /// as one black rectangle, and with them the whole grouping
    /// grammar the interface is built on. A contrast boost pivoted at
    /// 0.5 does that to a UI whose tones are all below 0.17.
    ///
    /// So: the surfaces stay apart, and stay in order.
    #[test]
    fn the_surface_ladder_survives_the_tube() {
        let a = Amounts::default();
        // A flat surface is exactly where relief does NOT help — this
        // is the middle of a panel, not the edge of a letter.
        let through = |v: f32| {
            let mut sum = 0.0;
            for row in 0..2 {
                for column in 0..MASK_PITCH {
                    for channel in model([v; 3], row, column, 0.0, a) {
                        sum += channel;
                    }
                }
            }
            sum / (2 * MASK_PITCH * 3) as f32 * 255.0
        };
        let sunken = through(13.0 / 255.0);
        let bg = through(21.0 / 255.0);
        let surface = through(29.0 / 255.0);
        let raised = through(42.0 / 255.0);

        assert!(
            sunken < bg && bg < surface && surface < raised,
            "the ladder lost its order: {sunken:.1} {bg:.1} {surface:.1} {raised:.1}"
        );
        // Each rung has to be TELLABLE from the one below it. Two code
        // values is about where a large flat area stops being
        // distinguishable from its neighbour.
        for (lo, hi, name) in [
            (sunken, bg, "sunken/bg"),
            (bg, surface, "bg/surface"),
            (surface, raised, "surface/raised"),
        ] {
            assert!(
                hi - lo >= 2.0,
                "{name} are {:.1} code values apart — one surface, not two",
                hi - lo
            );
        }
        // And the ladder as a whole keeps its span: the original is 29
        // code values from sunken to raised.
        assert!(
            raised - sunken >= 22.0,
            "the ladder compressed to {:.1} of 29 code values",
            raised - sunken
        );
    }

    /// TEXT KEEPS ITS BRIGHTNESS, because the tube gets out of its way.
    ///
    /// The relief measure is the readability fix: the mask, the lines
    /// and the convergence error are there to make a flat SURFACE feel
    /// like a tube, and none of them has a job on a glyph. Where local
    /// contrast is high they fade out and the pixel arrives as egui
    /// drew it.
    #[test]
    fn the_tube_gets_out_of_the_way_of_detail() {
        let a = Amounts::default();
        let ink_at = |relief: f32| {
            let mut sum = 0.0;
            for row in 0..2 {
                for column in 0..MASK_PITCH {
                    for channel in model([INK; 3], row, column, relief, a) {
                        sum += channel;
                    }
                }
            }
            sum / (2 * MASK_PITCH * 3) as f32
        };
        let flat = ink_at(0.0);
        let edge = ink_at(1.0);
        assert!(
            edge > flat,
            "the relief did nothing: {flat:.3} flat, {edge:.3} on an edge"
        );
        // On the edge of a glyph the treatment is almost gone, so the
        // ink arrives close to where it started.
        assert!(
            (edge - INK).abs() < 0.06,
            "ink on an edge came through at {edge:.3}, not {INK:.3}"
        );
        // And in the middle of a flat white block — where there is no
        // detail to protect — it is dimmed, but not by half.
        assert!(flat > INK * 0.75, "a flat white block fell to {flat:.3}");
    }

    /// THE DITHER IS SMALLER THAN THE SMALLEST THING THE DISPLAY CAN
    /// SHOW, which is why it is allowed to run at full: its job is to
    /// break the banding every smooth ramp above produces, and it
    /// cannot be seen doing it.
    #[test]
    fn the_dither_never_reaches_one_code_value() {
        let mut worst = 0.0f32;
        for y in 0..4 {
            for x in 0..4 {
                worst = worst.max((bayer(x, y) * DITHER_CEILING).abs());
            }
        }
        assert!(
            worst * 255.0 <= 0.5,
            "the dither swings {:.2} code values",
            worst * 255.0
        );
        let sum: f32 = (0..4).flat_map(|y| (0..4).map(move |x| bayer(x, y))).sum();
        assert!(sum.abs() < 1e-3, "the dither matrix is biased by {sum}");
    }

    /// THEIR HASH IS A GOOD ONE, and this is worth asserting because
    /// the obvious alternative is not.
    ///
    /// `fract(sin(dot(p, k)) * 43758.5453)` — the hash every shader on
    /// the internet uses, and the one that was here before this port —
    /// measures a mean of -0.500 and an adjacent-pixel correlation of
    /// +0.27 in f32, which is what a GPU runs. That is not grain, it is
    /// a constant darkening with streaks in it, and by eye it would
    /// only ever have looked like weak grain.
    #[test]
    fn the_grain_is_fixed_to_the_glass_and_is_actually_noise() {
        assert_eq!(hash12(17.0, 42.0), hash12(17.0, 42.0), "not deterministic");
        let grid: Vec<f32> = (0..256)
            .flat_map(|y| (0..256).map(move |x| hash12(x as f32, y as f32) - 0.5))
            .collect();
        let mean = grid.iter().sum::<f32>() / grid.len() as f32;
        let variance = grid.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / grid.len() as f32;
        let adjacent = grid.windows(2).map(|w| w[0] * w[1]).sum::<f32>() / grid.len() as f32;
        assert!(mean.abs() < 0.01, "the grain is biased by {mean}");
        // Uniform on ±0.5 has a variance of exactly 1/12.
        assert!(
            (variance - 1.0 / 12.0).abs() < 0.01,
            "the grain is not uniform: variance {variance}"
        );
        assert!(
            adjacent.abs() < 0.01,
            "neighbouring pixels correlate at {adjacent} — the grain streaks"
        );
        let worst = grid.iter().fold(0.0f32, |m, v| m.max(v.abs())) * GRAIN;
        assert!(worst <= 2.1, "the grain swings {worst:.2} code values");
    }

    /// THE CURVE IS OFF BY DEFAULT, and that is a decision rather than
    /// an oversight.
    ///
    /// Barrel distortion moves a pixel away from where the pointer
    /// thinks it is — their own note says so. A desktop absorbs a few
    /// pixels at the corners; a device card whose handles are dragged
    /// cannot, because the handle would not be where it is drawn.
    #[test]
    fn the_curve_is_off_by_default() {
        assert_eq!(
            Amounts::default().curve,
            0.0,
            "the curve is on, and the pointer no longer lands where it looks"
        );
    }

    /// Settings survive a round trip through the preferences file, and
    /// nonsense in that file cannot make an out-of-range treatment.
    #[test]
    fn amounts_round_trip_and_nonsense_clamps() {
        let text = ron::ser::to_string(&Amounts::default()).expect("serialize");
        let back: Amounts = ron::from_str(&text).expect("deserialize");
        assert_eq!(back, Amounts::default());

        let wild: Amounts = ron::from_str("(9.0, -4.0, 0.5, 2.0, -0.1, 0.5, 0.5, 0.5, 0.5, 0.5)")
            .expect("deserialize");
        assert_eq!(wild.intensity, 1.0);
        assert_eq!(wild.curve, 0.0);
        assert_eq!(wild.scanline, 0.5);
        assert_eq!(wild.grille, 1.0);
        assert_eq!(wild.bloom, 0.0);

        // A setting stored by an earlier shape of this struct is
        // refused rather than misread, and the caller falls back.
        assert!(ron::from_str::<Amounts>("(0.3, 0.3, 0.4)").is_err());
    }
}
