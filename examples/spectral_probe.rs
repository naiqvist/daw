//! Offline ENGINE proof, not a native TAKE or a human-approved sound.
//! cargo run --example spectral_probe -- /tmp/spectral-prototype-v1
//! Writes new-only patch.ron, score.ron and audio.wav. Never overwrites a take.
use daw::audio::{
    spectral::{SpectralPatch, SpectralVoices},
    spectral_fx as fx, spectral_mod as modulation,
};
use daw::params::spectral as p;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    time::Instant,
};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
enum Action {
    On { pitch: u8, velocity: u8 },
    Off { pitch: u8 },
    Lock { id: u32, value: f32 },
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Event {
    frame: usize,
    action: Action,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Score {
    sample_rate: u32,
    frames: usize,
    events: Vec<Event>,
}

fn file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let destination = std::env::args()
        .nth(1)
        .ok_or("give a NEW output directory")?;
    let directory = Path::new(&destination);
    if directory.exists() {
        return Err("output directory already exists; choose a fresh version".into());
    }
    let mut patch = SpectralPatch::default();
    patch.params.set(p::ATTACK, 30.0);
    patch.params.set(p::RELEASE, 700.0);
    patch.params.set(p::SUSTAIN, 0.85);
    patch.params.set(p::LEVEL, 1.0);
    patch.params.set(p::MORPH, 250.0);
    patch.params.set(p::ORNAMENT_TIME, 950.0);
    patch.params.set(p::ORNAMENT_SPEED, 3.2);
    patch.params.set(p::ORNAMENT_FROM, 3.0);
    patch.params.set(p::ORNAMENT_OTHER, -1.5);
    for h in 0..128 {
        patch.params.set(
            p::amp(h),
            if h < 24 {
                0.65 / ((h + 1) as f32).sqrt()
            } else {
                0.0
            },
        );
    }
    let node = |id: &str, module| fx::Node {
        id: id.into(),
        module,
    };
    patch.fx = fx::Patch {
        version: 1,
        nodes: vec![
            node(
                "filter",
                fx::Module::Filter {
                    mode: fx::FilterMode::Lowpass,
                    hz: 2600.0,
                    q: 0.8,
                },
            ),
            node(
                "fold",
                fx::Module::Shape {
                    mode: fx::ShapeMode::Fold,
                    drive: 2.3,
                    bias: 0.06,
                    mix: 0.5,
                },
            ),
            node(
                "smear",
                fx::Module::Disperser {
                    hz: 1200.0,
                    q: 0.8,
                    stages: 5,
                },
            ),
            node("mix", fx::Module::Gain { gain: 0.9 }),
            node(
                "echo",
                fx::Module::Delay {
                    ms: 375.0,
                    feedback: 0.38,
                    damp_hz: 5500.0,
                },
            ),
            node(
                "room",
                fx::Module::Reverb {
                    size: 0.7,
                    decay: 0.5,
                    damp: 0.5,
                },
            ),
            node("dc", fx::Module::DcBlock),
        ],
        routes: vec![
            fx::Route::new("input", "filter", 1.0),
            fx::Route::new("filter", "mix", 0.8),
            fx::Route::new("filter", "fold", 1.0),
            fx::Route::new("fold", "smear", 1.0),
            fx::Route::new("smear", "mix", 0.2),
            fx::Route::new("mix", "dc", 0.9),
            fx::Route::new("mix", "echo", 1.0),
            fx::Route::new("mix", "room", 0.5),
            fx::Route::new("echo", "room", 0.5),
            fx::Route::new("echo", "dc", 0.18),
            fx::Route::new("room", "dc", 0.35),
            fx::Route::new("dc", "output", 1.0),
        ],
    };
    let source = |id: &str, scope, generator| modulation::Source {
        id: id.into(),
        scope,
        generator,
    };
    let route = |source: &str, target, depth, offset| modulation::Route {
        source: source.into(),
        target,
        depth,
        offset,
        polarity: modulation::Polarity::Native,
    };
    patch.modulation = modulation::Patch {
        sources: vec![
            source(
                "breath",
                modulation::Scope::Shared,
                modulation::Generator::Lfo {
                    shape: modulation::Shape::Sine,
                    hz: 0.19,
                    phase: 0.0,
                    retrigger: false,
                },
            ),
            source(
                "pitch-rise",
                modulation::Scope::Voice,
                modulation::Generator::Envelope {
                    attack_ms: 180.0,
                    decay_ms: 450.0,
                    sustain: 0.0,
                    release_ms: 300.0,
                },
            ),
            source(
                "amp-shape",
                modulation::Scope::Voice,
                modulation::Generator::Envelope {
                    attack_ms: 180.0,
                    decay_ms: 300.0,
                    sustain: 0.8,
                    release_ms: 700.0,
                },
            ),
            source(
                "human",
                modulation::Scope::Voice,
                modulation::Generator::NoteRandom { seed: 20260910 },
            ),
        ],
        routes: vec![
            route(
                "breath",
                modulation::Target::Fx {
                    node: "filter".into(),
                    control: fx::Control::Hz,
                },
                1800.0,
                0.0,
            ),
            route(
                "breath",
                modulation::Target::HarmonicPhase { first: 2, last: 24 },
                70.0,
                0.0,
            ),
            route("pitch-rise", modulation::Target::PitchSemitones, 0.24, 0.0),
            route("human", modulation::Target::PitchSemitones, 0.055, 0.0),
            route("amp-shape", modulation::Target::Amplitude, 1.0, -1.0),
        ],
    };
    let mut events = Vec::new();
    for (i, pitch) in [48, 55, 59, 52, 57, 60].into_iter().enumerate() {
        let frame = i * 96_000;
        events.push(Event {
            frame,
            action: Action::Lock {
                id: p::ORNAMENT,
                value: (i + 1) as f32,
            },
        });
        events.push(Event {
            frame,
            action: Action::Lock {
                id: p::phase(4),
                value: if i % 2 == 0 { 140.0 } else { -140.0 },
            },
        });
        events.push(Event {
            frame,
            action: Action::On {
                pitch,
                velocity: 96,
            },
        });
        events.push(Event {
            frame: frame + 62_400,
            action: Action::Off { pitch },
        });
    }
    let score = Score {
        sample_rate: 48_000,
        frames: 720_000,
        events,
    };
    // Exercise persistence before sounding anything: the artifact is what plays.
    let pretty = ron::ser::PrettyConfig::default();
    let patch_text = ron::ser::to_string_pretty(&patch, pretty.clone())?;
    let score_text = ron::ser::to_string_pretty(&score, pretty)?;
    let recalled: SpectralPatch = ron::from_str(&patch_text)?;
    let score: Score = ron::from_str(&score_text)?;
    let mut voice = SpectralVoices::prepare(48_000.0, 256, &recalled, 32 * 1024 * 1024)?;
    let mut audio = vec![0.0; score.frames];
    let mut cursor = 0;
    let mut age = 0;
    let started = Instant::now();
    for event in &score.events {
        assert_no_alloc::assert_no_alloc(|| voice.render_audio(&mut audio[cursor..event.frame]));
        cursor = event.frame;
        match event.action {
            Action::Lock { id, value } => voice.plock(id, Some(value)),
            Action::On { pitch, velocity } => {
                voice.note_on(pitch, velocity, age);
                age += 1;
            }
            Action::Off { pitch } => voice.note_off(pitch),
        }
    }
    assert_no_alloc::assert_no_alloc(|| voice.render_audio(&mut audio[cursor..]));
    let render_seconds = started.elapsed().as_secs_f64();
    if voice.fx_faulted() || audio.iter().any(|s| !s.is_finite()) {
        return Err("render faulted; no audio artifact written".into());
    }
    let peak = audio.iter().fold(0.0f32, |a, b| a.max(b.abs()));
    if peak >= 1.0 {
        return Err(format!("prototype exceeds unity: {peak}; revise gain before audition").into());
    }
    let rms = (audio.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / audio.len() as f64).sqrt();
    std::fs::create_dir_all(directory)?;
    file(&directory.join("patch.ron"))?.write_all(patch_text.as_bytes())?;
    file(&directory.join("score.ron"))?.write_all(score_text.as_bytes())?;
    let mut wav = hound::WavWriter::new(
        file(&directory.join("audio.wav"))?,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for sample in audio {
        wav.write_sample((sample * 8_388_607.0).round() as i32)?;
    }
    wav.finalize()?;
    println!(
        "15-second engine proof; render {render_seconds:.3}s; peak {:.2} dBFS; RMS {:.2} dBFS; no native TAKE/human approval",
        20.0 * peak.log10(),
        20.0 * rms.log10()
    );
    println!("{}", directory.display());
    Ok(())
}
