//! `shot` — render one device card to a PNG, headlessly.
//!
//! A dev harness, like `lab`. It exists so a card can be LOOKED at without
//! launching the app, driving a mouse to the browser and dragging a device
//! onto a track — and so the picture is the card's real pixels rather than
//! a drawing of what someone hoped it looked like.
//!
//! Same egui, same theme, same fonts and the same `egui_wgpu` renderer the
//! app uses; only the surface differs, because there is no window. The
//! output is whatever the app would have put on screen, minus the CRT pass.
//!
//!     cargo run --bin shot -- flint out.png
//!
//! Not shipped.

use daw::ui::theme::Theme;
use eframe::egui;

/// A card is drawn at this scale, so the PNG is legible rather than a
/// postage stamp. A whole frame is drawn at 1:1 — it is already window
/// sized, and three times a window is a picture nothing can open.
const CARD_SCALE: f32 = 3.0;
const FRAME_SCALE: f32 = 1.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let which = args.next().unwrap_or_else(|| "flint".into());
    let out = args.next().unwrap_or_else(|| "card.png".into());

    // A whole frame asks for a window; a card asks for a card. Snug
    // around the card: it declares its own width, and its height is the
    // tall-card budget plus the frame that sits outside it. Wide enough
    // for the widest card: sibyl's nine cells.
    let frame = which.starts_with("stage");
    let (logical_w, logical_h) = if frame {
        (1280.0f32, 800.0f32)
    } else {
        (540.0f32, 248.0f32)
    };
    let scale = if frame { FRAME_SCALE } else { CARD_SCALE };
    let size = [(logical_w * scale) as u32, (logical_h * scale) as u32];

    // ---- egui: the context, the fonts and the theme the app uses ----
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(scale);
    if which.starts_with("stage") {
        daw::install_stage_fonts(&ctx);
    } else {
        daw::install_fonts(&ctx);
    }
    // A light shot needs the light runtime theme too, or the stock
    // widgets in it would be a dark window over a paper page.
    let theme = if which.ends_with("-light") {
        Theme::light()
    } else {
        Theme::dark()
    };

    // ---- wgpu: a device with no window ------------------------------
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
        ..Default::default()
    }))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("shot"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
        ..Default::default()
    }))?;

    // sRGB, the same as the app's surface, so the bytes read back are
    // already encoded the way a PNG wants them.
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("card"),
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
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let mut renderer = egui_wgpu::Renderer::new(&device, format, Default::default());
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: size,
        pixels_per_point: scale,
    };
    // ---- egui: run the card and tessellate what it drew -------------
    //
    // TWO passes, because egui settles layout on the second. Both frames'
    // texture deltas are applied: the FIRST is the one carrying the font
    // atlas, and every egui mesh samples that atlas — even a solid fill
    // takes its colour from the atlas's white pixel. Drop it and the
    // renderer silently skips every mesh, which is a blank card that
    // reports 3000 vertices tessellated.
    let mut subject = Subject::default();
    let mut jobs = Vec::new();
    for _ in 0..2 {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(logical_w, logical_h),
                )),
                ..Default::default()
            },
            |ui| {
                let bg = if frame {
                    daw::design::Alphabet::for_polarity(if which.ends_with("-light") {
                        daw::design::Polarity::Light
                    } else {
                        daw::design::Polarity::Dark
                    })
                    .ground
                    .color
                } else {
                    theme.bg
                };
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(bg))
                    .show(ui, |ui| {
                        draw(&which, ui, &theme, &mut subject);
                    });
            },
        );
        // OWNED and drained on every path — `TexturesDelta` panics in its
        // destructor if it is dropped with work still in it.
        let mut textures = std::mem::take(&mut out.textures_delta);
        for (id, deltas) in &textures.set {
            for delta in deltas {
                renderer.update_texture(&device, &queue, *id, delta);
            }
        }
        for id in std::mem::take(&mut textures.free) {
            renderer.free_texture(&id);
        }
        textures.clear();
        jobs = ctx.tessellate(std::mem::take(&mut out.shapes), scale);
    }

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("shot"),
    });
    renderer.update_buffers(&device, &queue, &mut encoder, &jobs, &screen);
    {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("card"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(srgb_clear(theme.bg)),
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

    // ---- read it back ------------------------------------------------
    // Rows in a copy destination must start on a 256-byte boundary.
    let unpadded = size[0] * 4;
    let padded = unpadded.div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded * size[1]) as u64,
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
                bytes_per_row: Some(padded),
                rows_per_image: Some(size[1]),
            },
        },
        wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::wait_indefinitely())?;
    rx.recv()??;

    let mapped = slice.get_mapped_range()?;
    let mut rgba = Vec::with_capacity((unpadded * size[1]) as usize);
    for row in 0..size[1] {
        let start = (row * padded) as usize;
        let end = start + unpadded as usize;
        rgba.extend_from_slice(mapped.get(start..end).ok_or("short readback")?);
    }
    drop(mapped);
    buffer.unmap();

    std::fs::write(&out, png(size[0], size[1], &rgba))?;
    println!("{out}: {}x{} px", size[0], size[1]);
    Ok(())
}

/// A few seconds of plausible level history, so a metering pose shows a
/// trace rather than a flat line. `hot` pushes the whole thing up.
fn posed_history(hot: f32) -> daw::ui::device::scope::History {
    use daw::ui::device::scope::{History, Reading};
    let mut h = History::default();
    for i in 0..daw::ui::device::scope::HISTORY {
        let t = i as f32 / 24.0;
        let swell = (t * 0.7).sin() * 0.5 + 0.5;
        let peak = -22.0 + swell * 20.0 + hot * 8.0;
        h.push(Reading {
            level_db: peak.min(-0.1),
            reduction_db: (peak - 9.0 + (t * 3.1).sin() * 2.0).min(-1.0),
            bands: [0.0; 3],
        });
    }
    h
}

/// Which card to draw. One arm per card that has been taught to pose.
/// The stage, driven to whatever the shot is meant to show.
///
/// Intents rather than keystrokes: the harness has no keyboard, and the
/// intent vocabulary is the same thing a key would have produced — so a
/// shot is of the surface the keys reach, not of a back door into it.
fn build_stage(which: &str) -> daw::ui::stage::Stage {
    use daw::ui::stage::{Stage, StageIntent, Step};

    let mut stage = Stage::new();
    // The palette opens with the app and would cover the very thing most
    // of these shots are of.
    stage.set_palette_open(false);
    // Set the ground the subject asks for, rather than assuming which
    // one the frame opens on — that is a frame's choice and it moves.
    let want = if which.ends_with("-light") {
        daw::design::Polarity::Light
    } else {
        daw::design::Polarity::Dark
    };
    if stage.polarity() != want {
        let _ = stage.apply(StageIntent::Ground);
    }
    if which.contains("full") {
        // A session with something in it. An empty one is easy to make
        // look tidy and tells you nothing about how the surface reads
        // when it is carrying a piece of music.
        for _ in 0..3 {
            let _ = stage.apply(StageIntent::NewInstrumentTrack);
        }
        let _ = stage.apply(StageIntent::NewAudioTrack);
        // Fill a scattering of slots: down a column, across a row, and a
        // few singles, so both axes have something to line up.
        let fills = [
            (0usize, 0usize),
            (0, 1),
            (0, 2),
            (1, 0),
            (2, 0),
            (1, 3),
            (3, 2),
            (4, 1),
            (2, 4),
        ];
        for (col, row) in fills {
            let _ = stage.apply(StageIntent::Step(Step::Up));
            for _ in 0..8 {
                let _ = stage.apply(StageIntent::Step(Step::Left));
            }
            for _ in 0..col {
                let _ = stage.apply(StageIntent::Step(Step::Right));
            }
            for _ in 0..=row {
                let _ = stage.apply(StageIntent::Step(Step::Down));
            }
            let _ = stage.apply(StageIntent::Enter);
        }
        // Rest the cursor somewhere ordinary, fire that clip, and roll:
        // the session should say what is sounding.
        for _ in 0..8 {
            let _ = stage.apply(StageIntent::Step(Step::Up));
        }
        let _ = stage.apply(StageIntent::Step(Step::Down));
        let _ = stage.apply(StageIntent::Launch);
        let _ = stage.apply(StageIntent::ToggleTransport);
    } else if which.contains("chain") {
        // A voice and two effects, with a few values moved off their
        // defaults so the band has something to distinguish.
        use daw::devices::DeviceKind;
        for kind in [DeviceKind::Poly, DeviceKind::Sat, DeviceKind::Reverb] {
            let _ = stage.song_mut().add_device(0, kind);
        }
        let _ = stage.apply(StageIntent::Devices);
        for _ in 0..3 {
            let _ = stage.apply(StageIntent::Step(Step::Down));
        }
        for _ in 0..6 {
            let _ = stage.apply(StageIntent::Param {
                up: true,
                coarse: false,
            });
        }
    } else if which.contains("mixer") {
        // Two more tracks, so the strip is a row of channels rather than
        // one column — a mixer is read across, and a mixer of one says
        // nothing about that.
        let _ = stage.apply(StageIntent::NewInstrumentTrack);
        let _ = stage.apply(StageIntent::NewAudioTrack);
        let _ = stage.apply(StageIntent::Mix);
    } else if which.contains("clip") {
        // Down onto a slot, fill it, and go in: the sequencer and the
        // trig inspector are what this shot is for. Give the new pattern
        // a small phrase before opening it, so the pose exercises seals,
        // held traces, velocity, condition and degree signs rather than
        // proving only that empty vias draw.
        let _ = stage.apply(StageIntent::Step(Step::Down));
        let _ = stage.apply(StageIntent::Enter);
        if let Some(pattern) = stage.song_mut().patterns.last_mut() {
            use daw::pitch::Pitch;
            use daw::sequencing::Note;

            pattern.set_primary(0, Note::with_pitch(Pitch::degree(0, 0), 36, 118));
            pattern.trig_mut(0).probability = 0.75;
            pattern.set_primary(4, Note::with_pitch(Pitch::degree(2, 0), 12, 82));
            let mut pushed = Note::with_pitch(Pitch::degree(4, 0), 24, 104);
            pushed.micro_ticks = 2;
            pattern.set_primary(7, pushed);
            pattern.add_tone(7, Note::with_pitch(Pitch::degree(6, 0), 24, 92));
            pattern.set_primary(12, Note::with_pitch(Pitch::degree(1, 1), 48, 126));
            pattern.set_primary(20, Note::new(54, 18, 66));
        }
        let _ = stage.apply(StageIntent::Enter);
    }
    stage
}

/// State a subject keeps BETWEEN the two passes.
///
/// The stage is a running frame rather than a pure drawing: it settles
/// layout on the second pass like everything else here, and it must be
/// the same stage both times or the shot would be of a surface in two
/// different states.
#[derive(Default)]
struct Subject {
    stage: Option<daw::ui::stage::Stage>,
}

fn draw(which: &str, ui: &mut egui::Ui, theme: &Theme, subject: &mut Subject) {
    use daw::ui::device;
    if which.starts_with("stage") {
        let stage = subject.stage.get_or_insert_with(|| build_stage(which));
        stage.show(ui);
        return;
    }
    match which {
        "flint" => {
            let mut state = device::flint::FlintUi::default();
            // A hit is arriving: the meter is lit, as it would be in use.
            device::flint::flint_card(ui, theme, &mut state, 0.82);
        }
        // The extremes, because a plot that only holds together at its
        // defaults is a plot nobody has looked at.
        "flint-hot" => {
            let mut state = device::flint::FlintUi::default();
            state.strike = device::flint::flint_norm(daw::params::flint::STRIKE, 18.0);
            state.body = device::flint::flint_norm(daw::params::flint::BODY, -14.0);
            state.split = device::flint::flint_norm(daw::params::flint::SPLIT, 45.0);
            device::flint::flint_card(ui, theme, &mut state, 0.95);
        }
        "flint-soft" => {
            let mut state = device::flint::FlintUi::default();
            state.strike = device::flint::flint_norm(daw::params::flint::STRIKE, -12.0);
            state.body = device::flint::flint_norm(daw::params::flint::BODY, 9.0);
            state.split = device::flint::flint_norm(daw::params::flint::SPLIT, 3.0);
            device::flint::flint_card(ui, theme, &mut state, 0.12);
        }
        "sibyl" => {
            let mut state = device::sibyl::SibylUi::default();
            // Following an A, which in C minor is the sixth — so the
            // harmony is not the interval a fixed +4 would have given.
            device::sibyl::sibyl_card(ui, theme, &mut state, Some(69.15));
        }
        "sibyl-two" => {
            let mut state = device::sibyl::SibylUi::default();
            state.voice_b = device::sibyl::sibyl_norm(daw::params::sibyl::VOICE_B, -3.0);
            state.shift = device::sibyl::sibyl_norm(daw::params::sibyl::SHIFT, -12.0);
            state.formant = device::sibyl::sibyl_norm(daw::params::sibyl::FORMANT, 5.0);
            device::sibyl::sibyl_card(ui, theme, &mut state, Some(64.0));
        }
        "sibyl-lost" => {
            let mut state = device::sibyl::SibylUi::default();
            device::sibyl::sibyl_card(ui, theme, &mut state, None);
        }
        "ferric" => {
            let mut state = device::ferric::FerricUi::default();
            device::ferric::ferric_card(
                ui,
                theme,
                &mut state,
                device::ferric::Transport {
                    step: 3.0,
                    reach: 1.0,
                    record_db: -9.0,
                    div_ms: 125.0,
                },
            );
        }
        "ferric-stutter" => {
            let mut state = device::ferric::FerricUi::default();
            state.pattern = device::ferric::ferric_norm(daw::params::ferric::PATTERN, 1.0);
            state.groove = device::ferric::ferric_norm(daw::params::ferric::GROOVE, 1.0);
            state.speed = device::ferric::ferric_norm(daw::params::ferric::SPEED, -5.0);
            state.age = device::ferric::ferric_norm(daw::params::ferric::AGE, 0.7);
            device::ferric::ferric_card(
                ui,
                theme,
                &mut state,
                device::ferric::Transport {
                    step: 2.0,
                    reach: 2.0,
                    record_db: -3.2,
                    div_ms: 125.0,
                },
            );
        }
        "ferric-half" => {
            let mut state = device::ferric::FerricUi::default();
            state.pattern = device::ferric::ferric_norm(daw::params::ferric::PATTERN, 2.0);
            state.groove = device::ferric::ferric_norm(daw::params::ferric::GROOVE, 0.75);
            state.division = device::ferric::ferric_norm(daw::params::ferric::DIVISION, 1.0);
            device::ferric::ferric_card(
                ui,
                theme,
                &mut state,
                device::ferric::Transport {
                    step: 5.0,
                    reach: 1.5,
                    record_db: -16.0,
                    div_ms: 250.0,
                },
            );
        }
        "umbra" => {
            let mut state = device::umbra::UmbraUi::default();
            device::umbra::umbra_card(ui, theme, &mut state, 0.58);
        }
        "umbra-deep" => {
            let mut state = device::umbra::UmbraUi::default();
            state.depth = device::umbra::umbra_norm(daw::params::umbra::DEPTH, 0.97);
            state.colour = device::umbra::umbra_norm(daw::params::umbra::COLOUR, -0.6);
            state.motion = device::umbra::umbra_norm(daw::params::umbra::MOTION, 0.85);
            state.decay = device::umbra::umbra_norm(daw::params::umbra::DECAY, 0.9);
            state.mix = device::umbra::umbra_norm(daw::params::umbra::MIX, 0.85);
            device::umbra::umbra_card(ui, theme, &mut state, 0.97);
        }
        "umbra-low" => {
            let mut state = device::umbra::UmbraUi::default();
            state.depth = device::umbra::umbra_norm(daw::params::umbra::DEPTH, 0.14);
            device::umbra::umbra_card(ui, theme, &mut state, 0.14);
        }
        "tone" => {
            let mut state = device::tone::ToneUi::default();
            device::tone::tone_card(ui, theme, &mut state);
        }
        "tone-noise" => {
            let mut state = device::tone::ToneUi::default();
            state.shape = device::tone::tone_norm(daw::params::tone::SHAPE, 5.0);
            state.level = device::tone::tone_norm(daw::params::tone::LEVEL, 0.5);
            device::tone::tone_card(ui, theme, &mut state);
        }
        "sigil" => {
            let mut state = device::sigil::SigilUi::default();
            // A square seal, well cast: the rune at its most rune-like.
            state.shape = device::sigil::sigil_norm(daw::params::sigil::SHAPE, 2.0);
            state.freq = device::sigil::sigil_norm(daw::params::sigil::FREQ, 110.0);
            state.mix = device::sigil::sigil_norm(daw::params::sigil::MIX, 0.8);
            device::sigil::sigil_card(ui, theme, &mut state);
        }
        "loom" => {
            let mut state = device::loom::LoomUi::default();
            // The wavetable map at its most wavetable: both oscillators
            // scanned well away from sine, on the OSC A page.
            state.a_morph = device::loom::loom_norm(daw::params::loom::A_MORPH, 0.8);
            state.b_morph = device::loom::loom_norm(daw::params::loom::B_MORPH, 0.4);
            state.a_oct = device::loom::loom_norm(daw::params::loom::A_OCT, 3.0);
            state.v_unison = device::loom::loom_norm(daw::params::loom::V_UNISON, 3.0);
            state.v_spread = device::loom::loom_norm(daw::params::loom::V_SPREAD, 80.0);
            device::loom::loom_card(ui, theme, &mut state);
        }
        "sigil-quiet" => {
            let mut state = device::sigil::SigilUi::default();
            state.shape = device::sigil::sigil_norm(daw::params::sigil::SHAPE, 0.0);
            state.freq = device::sigil::sigil_norm(daw::params::sigil::FREQ, 440.0);
            state.mix = device::sigil::sigil_norm(daw::params::sigil::MIX, 0.0);
            device::sigil::sigil_card(ui, theme, &mut state);
        }
        "gauge" => {
            let mut state = device::gauge::GaugeUi::default();
            let history = posed_history(0.0);
            device::gauge::gauge_card(
                ui,
                theme,
                &mut state,
                device::gauge::Reading {
                    peak_db: -3.2,
                    rms_db: -14.6,
                    correlation: 0.62,
                },
                &history,
            );
        }
        "gauge-bad" => {
            let mut state = device::gauge::GaugeUi::default();
            let history = posed_history(1.0);
            device::gauge::gauge_card(
                ui,
                theme,
                &mut state,
                device::gauge::Reading {
                    peak_db: -0.05,
                    rms_db: -4.1,
                    correlation: -0.38,
                },
                &history,
            );
        }
        "tine" => {
            let mut state = device::tine::TineUi::default();
            device::tine::tine_card(ui, theme, &mut state, 3.0);
        }
        "tine-bar" => {
            let mut state = device::tine::TineUi::default();
            state.material = device::tine::tine_norm(daw::params::tine::MATERIAL, 1.0);
            state.place = device::tine::tine_norm(daw::params::tine::PLACE, 0.22);
            state.decay = device::tine::tine_norm(daw::params::tine::DECAY, 0.85);
            state.tone = device::tine::tine_norm(daw::params::tine::TONE, 0.7);
            device::tine::tine_card(ui, theme, &mut state, 6.0);
        }
        "tine-string" => {
            let mut state = device::tine::TineUi::default();
            state.material = device::tine::tine_norm(daw::params::tine::MATERIAL, 0.0);
            state.place = device::tine::tine_norm(daw::params::tine::PLACE, 0.5);
            device::tine::tine_card(ui, theme, &mut state, 1.0);
        }
        other => {
            ui.label(format!("no pose for {other:?}"));
        }
    }
}

/// egui works in sRGB bytes; a wgpu clear colour is linear.
fn srgb_clear(c: egui::Color32) -> wgpu::Color {
    let to_linear = |b: u8| {
        let s = b as f64 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    wgpu::Color {
        r: to_linear(c.r()),
        g: to_linear(c.g()),
        b: to_linear(c.b()),
        a: 1.0,
    }
}

// ------------------------------------------------------------- png ---
//
// Written by hand rather than pulled in as a dependency: this is a dev
// harness and a PNG is a header, a CRC and a zlib stream. The stream uses
// deflate's STORED blocks — no compression, no compressor, and a file a
// few times larger than it needs to be, which for a screenshot is the
// right trade.

fn png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((h * (w * 4 + 1)) as usize);
    for row in 0..h {
        raw.push(0); // filter: none
        let start = (row * w * 4) as usize;
        let end = start + (w * 4) as usize;
        raw.extend_from_slice(&rgba[start..end]);
    }

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, RGBA
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let mut crc = crc32(0xFFFF_FFFF, kind);
    crc = crc32(crc, body);
    out.extend_from_slice(&(crc ^ 0xFFFF_FFFF).to_be_bytes());
}

/// A zlib stream of deflate STORED blocks: valid, and trivially correct.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut rest = data;
    loop {
        let take = rest.len().min(65_535);
        let last = take == rest.len();
        out.push(u8::from(last));
        out.extend_from_slice(&(take as u16).to_le_bytes());
        out.extend_from_slice(&(!(take as u16)).to_le_bytes());
        out.extend_from_slice(&rest[..take]);
        if last {
            break;
        }
        rest = &rest[take..];
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn crc32(mut crc: u32, data: &[u8]) -> u32 {
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}
