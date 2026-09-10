//! Authored score, not a new beat-specific app command. Writes ordinary editable
//! Song notes/locks, an embedded Spectral patch and a native TAKE scorecard.
//! cargo run --example spectral_journey -- /tmp/spectral-journey-v1
//! Optional --render preflights the SAME native Song graph, never sample playback.
use daw::{
    audio::{
        bounce::{BounceFormat, BounceOptions, bounce},
        graph::NodeSpec,
        spectral::{SpectralParams, SpectralPatch},
        spectral_fx as fx, spectral_mod as mo,
    },
    devices::DeviceKind,
    params::spectral as p,
    sequencing::{Locator, Note, Pattern, PatternId, Song, TRACK_VOLUME},
};
use std::{fs::OpenOptions, io::Write, path::Path};

const BARS: usize = 96;
const BAR: usize = 192;
const BPM: f64 = 144.0;
fn write_new(path: &Path, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?
        .write_all(text.as_bytes())?;
    Ok(())
}
fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}
fn set(params: &mut SpectralParams, values: &[(u32, f32)]) {
    for &(id, value) in values {
        params.set(id, value);
    }
}

fn patch() -> SpectralPatch {
    let mut patch = SpectralPatch::default();
    let n = |id: &str, module| fx::Node {
        id: id.into(),
        module,
    };
    patch.fx = fx::Patch {
        version: 1,
        nodes: vec![
            n(
                "lowcut",
                fx::Module::Filter {
                    mode: fx::FilterMode::Highpass,
                    hz: 35.0,
                    q: 0.707,
                },
            ),
            n(
                "colour",
                fx::Module::Filter {
                    mode: fx::FilterMode::Lowpass,
                    hz: 600.0,
                    q: 0.85,
                },
            ),
            n(
                "fold",
                fx::Module::Shape {
                    mode: fx::ShapeMode::Fold,
                    drive: 2.1,
                    bias: 0.015,
                    mix: 0.55,
                },
            ),
            n("fold-send", fx::Module::Gain { gain: 0.0 }),
            n(
                "smear",
                fx::Module::Disperser {
                    hz: 720.0,
                    q: 0.8,
                    stages: 4,
                },
            ),
            n("body", fx::Module::Gain { gain: 2.25 }),
            n(
                "echo",
                fx::Module::Delay {
                    ms: 312.5,
                    feedback: 0.44,
                    damp_hz: 4700.0,
                },
            ),
            n("echo-return", fx::Module::Gain { gain: 0.0 }),
            n(
                "room",
                fx::Module::Reverb {
                    size: 0.83,
                    decay: 0.69,
                    damp: 0.45,
                },
            ),
            n("room-return", fx::Module::Gain { gain: 0.0 }),
            n("dc", fx::Module::DcBlock),
        ],
        routes: vec![
            fx::Route::new("input", "lowcut", 1.0),
            fx::Route::new("lowcut", "colour", 1.0),
            fx::Route::new("colour", "body", 0.93),
            fx::Route::new("colour", "fold", 1.0),
            fx::Route::new("fold", "smear", 1.0),
            fx::Route::new("smear", "fold-send", 1.0),
            fx::Route::new("fold-send", "body", 1.0),
            fx::Route::new("body", "dc", 1.0),
            fx::Route::new("body", "echo", 1.0),
            fx::Route::new("echo", "echo-return", 1.0),
            fx::Route::new("echo-return", "dc", 1.0),
            fx::Route::new("body", "room", 0.65),
            fx::Route::new("echo-return", "room", 0.4),
            fx::Route::new("room", "room-return", 1.0),
            fx::Route::new("room-return", "dc", 1.0),
            fx::Route::new("dc", "output", 1.0),
        ],
    };
    let route = |source: &str, target, depth| mo::Route {
        source: source.into(),
        target,
        depth,
        offset: 0.0,
        polarity: mo::Polarity::Native,
    };
    let target = |id: &str, control| mo::Target::Fx {
        node: id.into(),
        control,
    };
    for i in 1..=8 {
        patch.modulation.sources.push(mo::Source {
            id: format!("macro-{i}"),
            scope: mo::Scope::Shared,
            generator: mo::Generator::Macro { index: i },
        });
    }
    patch.modulation.sources.extend([
        mo::Source {
            id: "breath".into(),
            scope: mo::Scope::Shared,
            generator: mo::Generator::Lfo {
                shape: mo::Shape::Sine,
                hz: 0.073,
                phase: 0.0,
                retrigger: false,
            },
        },
        mo::Source {
            id: "shimmer".into(),
            scope: mo::Scope::Shared,
            generator: mo::Generator::Lfo {
                shape: mo::Shape::Sine,
                hz: 0.113,
                phase: 0.27,
                retrigger: false,
            },
        },
        mo::Source {
            id: "human".into(),
            scope: mo::Scope::Voice,
            generator: mo::Generator::NoteRandom { seed: 2026091001 },
        },
    ]);
    patch.modulation.routes = vec![
        route("macro-1", target("colour", fx::Control::Hz), 11000.0),
        route("macro-2", target("room-return", fx::Control::Gain), 0.95),
        route("macro-3", target("echo-return", fx::Control::Gain), 0.5),
        route("macro-4", target("fold-send", fx::Control::Gain), 0.22),
        route(
            "macro-5",
            mo::Target::HarmonicAmp {
                first: 33,
                last: 128,
            },
            0.00012,
        ),
        route("macro-6", target("lowcut", fx::Control::Hz), 8000.0),
        route(
            "macro-7",
            mo::Target::HarmonicPhase { first: 3, last: 24 },
            95.0,
        ),
        route("macro-8", target("colour", fx::Control::Q), 0.75),
        route("breath", target("colour", fx::Control::Hz), 180.0),
        route(
            "breath",
            mo::Target::HarmonicPhase { first: 2, last: 12 },
            24.0,
        ),
        route(
            "shimmer",
            mo::Target::HarmonicPhase {
                first: 13,
                last: 128,
            },
            55.0,
        ),
        route("human", mo::Target::PitchSemitones, 0.025),
    ];
    patch.params = frame(0, 0.0, 0.0);
    patch
}

/// Hand-authored spectral frames blended on a continuous drum→reed→air axis.
/// kind: 0 low skin, 1 tom, 2 slap, 3 unpitched tick, 4 bell, 5 melody, 6 pad.
fn frame(kind: usize, progress: f32, bloom: f32) -> SpectralParams {
    let mut s = SpectralParams::default();
    for h in 0..128 {
        let a = match kind {
            0 => {
                if h == 0 {
                    1.0
                } else if h == 1 {
                    0.13
                } else {
                    0.0
                }
            }
            1 => [1.0, 0.22, 0.08, 0.24, 0.0, 0.04]
                .get(h)
                .copied()
                .unwrap_or(0.0),
            2 => {
                if h < 4 {
                    0.4 / (h + 1) as f32
                } else {
                    0.0
                }
            }
            3 => 0.0,
            4 => [0.9, 0.0, 0.07, 0.0, 0.22, 0.0, 0.11]
                .get(h)
                .copied()
                .unwrap_or(0.0),
            5 => {
                if h < 32 {
                    0.9 / ((h + 1) as f32).powf(mix(1.9, 1.12, progress))
                        * if h % 2 == 0 {
                            1.0
                        } else {
                            mix(0.35, 0.85, progress)
                        }
                } else {
                    0.0
                }
            }
            _ => {
                if h < 32 {
                    0.85 / ((h + 1) as f32).powf(1.65) * if h % 2 == 0 { 1.0 } else { 0.7 }
                } else {
                    0.0
                }
            }
        };
        s.set(p::amp(h), a);
        s.set(
            p::phase(h),
            if h == 0 {
                0.0
            } else {
                ((h * 79 % 300) as f32) - 150.0
            },
        );
    }
    set(
        &mut s,
        &[
            (p::MONO, 1.0),
            (p::GLIDE, 0.0),
            (p::ATTACK, 1.0),
            (p::DECAY, 75.0),
            (p::SUSTAIN, 0.0),
            (p::RELEASE, 5.0),
            (p::LEVEL, 1.75),
            (p::MORPH, 1.5),
            (p::MACRO_1, 0.22),
            (p::MACRO_2, 0.025),
            (p::MACRO_3, 0.015),
            (p::MACRO_4, 0.05),
            (p::MACRO_5, 0.0),
            (p::MACRO_6, 0.0),
            (p::MACRO_7, 0.0),
            (p::MACRO_8, 0.0),
            (p::PITCH_ORNAMENT, 2.0),
            (p::PITCH_TIME, 35.0),
            (p::PITCH_FROM, 7.0),
            (p::PITCH_OTHER, 0.0),
            (p::ORNAMENT, 0.0),
            (p::VIBRATO_DEPTH, 0.0),
        ],
    );
    match kind {
        0 => set(
            &mut s,
            &[(p::DECAY, 130.0), (p::NOISE, 0.012), (p::MACRO_1, 0.12)],
        ),
        1 => set(
            &mut s,
            &[(p::DECAY, 90.0), (p::NOISE, 0.055), (p::PITCH_FROM, 4.0)],
        ),
        2 => set(
            &mut s,
            &[
                (p::DECAY, 53.0),
                (p::NOISE, 0.78),
                (p::MACRO_1, 0.7),
                (p::MACRO_6, 0.08),
                (p::PITCH_ORNAMENT, 0.0),
                (p::LEVEL, 1.1),
            ],
        ),
        3 => set(
            &mut s,
            &[
                (p::DECAY, 31.0),
                (p::NOISE, 1.0),
                (p::MACRO_1, 0.94),
                (p::MACRO_6, 0.63),
                (p::PITCH_ORNAMENT, 0.0),
                (p::LEVEL, 1.05),
            ],
        ),
        4 => set(
            &mut s,
            &[
                (p::DECAY, 110.0),
                (p::NOISE, 0.02),
                (p::MACRO_1, 0.75),
                (p::PITCH_ORNAMENT, 0.0),
                (p::LEVEL, 1.05),
            ],
        ),
        5 => set(
            &mut s,
            &[
                (p::ATTACK, 3.0),
                (p::DECAY, 170.0),
                (p::SUSTAIN, mix(0.45, 0.7, progress)),
                (p::RELEASE, 55.0),
                (p::NOISE, 0.003),
                (p::GLIDE, mix(75.0, 26.0, progress)),
                (p::MORPH, mix(22.0, 9.0, progress)),
                (p::LEVEL, 1.8),
                (p::MACRO_1, mix(0.11, 0.73, progress)),
                (p::MACRO_2, mix(0.08, 0.23, progress)),
                (p::MACRO_3, mix(0.10, 0.38, progress)),
                (p::MACRO_4, mix(0.05, 0.62, progress)),
                (p::MACRO_7, progress * 0.8),
                (p::MACRO_8, progress * 0.5),
                (p::PITCH_ORNAMENT, 0.0),
                (p::PITCH_TIME, 110.0),
                (p::PITCH_FROM, -1.0),
                (p::PITCH_OTHER, 2.0),
                (p::VIBRATO_SPEED, 5.3),
            ],
        ),
        6 => set(
            &mut s,
            &[
                (p::MONO, 0.0),
                (p::ATTACK, mix(140.0, 680.0, bloom)),
                (p::DECAY, 2400.0),
                (p::SUSTAIN, 0.78),
                (p::RELEASE, 2300.0),
                (p::NOISE, 0.0018),
                (p::MORPH, mix(500.0, 1900.0, bloom)),
                (p::LEVEL, 1.30),
                (p::MACRO_1, mix(0.35, 0.77, bloom)),
                (p::MACRO_2, 0.63),
                (p::MACRO_3, 0.32),
                (p::MACRO_4, 0.025),
                (p::MACRO_5, bloom),
                (p::MACRO_7, bloom * 0.48),
                (p::VIBRATO_SPEED, 4.1),
                (p::VIBRATO_DEPTH, 3.2),
                (p::PITCH_ORNAMENT, 0.0),
                (p::ORNAMENT, 0.0),
            ],
        ),
        _ => {}
    }
    s
}

fn add(
    pattern: &mut Pattern,
    tick: usize,
    pitch: u8,
    gate: usize,
    velocity: u8,
    s: &SpectralParams,
) {
    assert!(tick < BARS * BAR);
    let mut note = Note::new(pitch, gate, velocity);
    note.micro_ticks = (tick % 12) as i16;
    let trig = pattern.trig_mut(tick / 12);
    // The sequencer intentionally shares locks among notes in one 12-tick cell.
    // Rapid ornament clusters within a cell therefore share one spectral frame.
    if trig.notes.is_empty() {
        for row in p::TABLE {
            if let Some((_, true)) = p::harmonic(row.id) {
                continue;
            }
            trig.set_lock(row.id, s.get(row.id).unwrap());
        }
    }
    trig.add_tone_at(note);
}

fn compose() -> Song {
    let mut song = Song::default();
    song.set_base_bpm(BPM);
    song.rename_track(0, "ONE SPECTRAL · Skin / Coil / Sky");
    song.desk_personality.noise_enabled = false;
    song.key = daw::pitch::Key::new(
        daw::pitch::Tuning {
            reference_hz: daw::pitch::midi_to_hz(64),
            scale: daw::pitch::builtin_scale("diatonic").unwrap(),
        },
        2,
    )
    .unwrap();
    let id = song.add_device(0, DeviceKind::Spectral).unwrap();
    let patch = patch();
    daw::spectral::validate(&patch).unwrap();
    patch.install(song.device_mut(id).unwrap());
    let mut pattern = Pattern::empty(PatternId(1), "Skin → speech → coil → sky → silence".into());
    pattern.tag = "a0".into();
    pattern.extend_timeline(BARS * BAR).unwrap();
    let drum_cycle = [0, 3, 1, 2, 3, 0, 4, 1, 3, 2, 1, 3];
    let melody = [64, 65, 67, 71, 69, 67, 65, 64, 62, 64, 67, 65];
    // 0–32: a linear player. A 12-pulse hand pattern against a four-beat
    // backbone, reordered ghost strokes and short duplet/quadruplet pickups.
    for bar in 0..32 {
        for pulse in 0..12 {
            if (bar + pulse) % 11 == 4 || (pulse == 9 && bar % 4 == 0) {
                continue;
            }
            let tick = bar * BAR + pulse * 16;
            let melodic = if bar < 16 {
                bar % 4 == 3 && pulse >= 8
            } else {
                pulse >= 12 - (4 + (bar - 16) / 3).min(9)
            };
            if melodic {
                let progress = 0.1 + bar as f32 / 70.0;
                let mut s = frame(5, progress, 0.0);
                let gesture = [1, 0, 2, 0, 5, 0, 4, 6][(pulse + bar) % 8];
                set(
                    &mut s,
                    &[
                        (p::PITCH_ORNAMENT, gesture as f32),
                        (p::PITCH_FROM, if pulse % 2 == 0 { -1.0 } else { 1.0 }),
                        (p::PITCH_OTHER, -2.0),
                        (p::ORNAMENT, if pulse == 10 { 5.0 } else { 0.0 }),
                        (p::ORNAMENT_FROM, 0.18),
                        (p::ORNAMENT_OTHER, -0.1),
                        (
                            p::VIBRATO_DEPTH,
                            if pulse == 11 && bar % 8 == 7 {
                                18.0
                            } else {
                                0.0
                            },
                        ),
                    ],
                );
                let pitch = melody[(pulse + (bar / 4) * 3) % melody.len()];
                // Overlap deliberately: note-off of predecessor cannot cut a slide.
                let gate = if pulse == 11 { 11 } else { 19 };
                add(
                    &mut pattern,
                    tick,
                    pitch,
                    gate,
                    79 + ((pulse * 7 + bar) % 21) as u8,
                    &s,
                );
            } else {
                let kind = if pulse == 0 {
                    0
                } else {
                    drum_cycle[(pulse + (bar % 3) * 2) % 12]
                };
                let pitch = [40, 52 + (bar % 2) as u8 * 3, 64, 88, 83][kind];
                let s = frame(kind, 0.0, 0.0);
                let velocity = if kind == 0 {
                    108
                } else if kind == 2 {
                    91
                } else {
                    56 + ((bar * 13 + pulse * 17) % 39) as u8
                };
                add(&mut pattern, tick, pitch, 11, velocity, &s);
                if pulse == 11 && bar % 4 == 2 {
                    // Same frame and pitch at half the pulse; no stacked drum hits.
                    pattern.trig_mut(tick / 12).notes[0].length_ticks = 5;
                    add(&mut pattern, tick + 8, pitch, 5, 54, &s);
                }
            }
        }
    }
    // 32–56: the pitched material inherits the drummer's accents and gaps.
    // Intensification comes from density, register and spectral exposure,
    // not a sudden second instrument or an unrelated arpeggiator.
    let motifs: [[u8; 12]; 4] = [
        [52, 64, 65, 67, 59, 71, 69, 67, 65, 64, 62, 65],
        [53, 65, 69, 71, 72, 71, 69, 65, 64, 67, 65, 64],
        [52, 59, 64, 67, 71, 74, 72, 71, 67, 65, 64, 59],
        [57, 64, 67, 69, 71, 72, 76, 74, 71, 67, 65, 64],
    ];
    for bar in 32..64 {
        let progress = ((bar - 28) as f32 / 34.0).min(1.0);
        let spacing = if bar >= 56 { 12 } else { 16 };
        let pulses = BAR / spacing;
        for pulse in 0..pulses {
            if bar < 48 && pulse == 5 && bar % 3 == 0 {
                continue;
            }
            let tick = bar * BAR + pulse * spacing;
            let motif = &motifs[((bar - 32) / 4) % 4];
            let pitch = motif[pulse % 12] + if bar >= 52 && pulse % 4 != 0 { 12 } else { 0 };
            let mut s = frame(5, progress, 0.0);
            let accent = pulse % 3 == 0 || pulse == 7;
            // Punctuate, do not ornament every note. Every gesture is used.
            let kind = if (pulse + bar) % 7 == 0 {
                1 + (bar / 2 + pulse) % 6
            } else {
                0
            };
            set(
                &mut s,
                &[
                    (p::PITCH_ORNAMENT, kind as f32),
                    (p::PITCH_TIME, if kind == 5 { 260.0 } else { 95.0 }),
                    (p::PITCH_FROM, if kind == 3 { -0.6 } else { -1.0 }),
                    (p::PITCH_OTHER, if kind == 6 { 1.0 } else { 2.0 }),
                    (p::PITCH_SPEED, 7.0),
                    (
                        p::ORNAMENT,
                        if pulse == pulses - 2 {
                            (bar % 6 + 1) as f32
                        } else {
                            0.0
                        },
                    ),
                    (p::ORNAMENT_FROM, 0.24),
                    (p::ORNAMENT_OTHER, -0.15),
                    (p::ORNAMENT_TIME, 165.0),
                    (p::ORNAMENT_SPEED, 8.0),
                    (
                        p::MACRO_1,
                        (0.18 + progress * 0.45) * if accent { 1.0 } else { 0.7 },
                    ),
                    (
                        p::VIBRATO_DEPTH,
                        if pulse == pulses - 1 && bar % 4 == 3 {
                            23.0
                        } else {
                            0.0
                        },
                    ),
                ],
            );
            let gate = if pulse == pulses - 1 || pulse % 4 == 3 {
                spacing - 3
            } else {
                spacing + 3
            };
            add(
                &mut pattern,
                tick,
                pitch,
                gate,
                if accent {
                    104
                } else {
                    78 + (pulse % 4) as u8 * 4
                },
                &s,
            );
            if bar >= 56 && bar % 2 == 1 && pulse >= 14 {
                // Accelerating microturn: ordinary notes at 1/32 and 1/64,
                // all on the sample-stamped grid.
                add(&mut pattern, tick + 6, pitch.saturating_sub(2), 7, 73, &s);
                if pulse == 15 {
                    add(&mut pattern, tick + 9, 76, 2, 65, &s);
                }
            }
        }
    }
    // 64–88: the arpeggio stretches into held voicings, with high answering
    // phrases and a slower lower counterline inside the same eight-voice bank.
    let chords: [[u8; 4]; 6] = [
        [40, 55, 59, 62],
        [41, 57, 64, 71],
        [45, 55, 60, 64],
        [38, 53, 60, 64],
        [41, 57, 64, 71],
        [40, 55, 59, 62],
    ];
    let high = [76, 77, 79, 83, 81, 79, 77, 76];
    for group in 0..6 {
        let start_bar = 64 + group * 4;
        let bloom = (group as f32 / 3.0).min(1.0);
        let s = frame(6, 0.0, bloom);
        for pitch in chords[group] {
            add(&mut pattern, start_bar * BAR, pitch, BAR * 4 - 36, 61, &s);
        }
        for bar in start_bar..start_bar + 4 {
            let count = if group < 2 { 4 - group } else { 2 };
            for i in 0..count {
                let offset = if count == 2 {
                    [48, 132][i]
                } else {
                    24 + i * 48
                };
                let pitch = high[(bar - 64 + i * 2) % 8];
                add(
                    &mut pattern,
                    bar * BAR + offset,
                    pitch,
                    if group < 2 { 40 } else { 72 },
                    54 + (i % 2) as u8 * 8,
                    &s,
                );
            }
            if bar % 2 == 1 && group >= 2 {
                // Keep this line off the pad's held pitches: MIDI note-off is
                // pitch-addressed and must not prematurely release its unison.
                let counter = [67, 69, 65, 67][(bar / 2) % 4];
                add(&mut pattern, bar * BAR + 12, counter, 114, 43, &s);
            }
        }
    }
    // 88–96: voicing thins, the last F resolves gently to E, and the entire
    // wet field fades continuously, not with a hard transport stop.
    let mut s = frame(6, 0.0, 1.0);
    set(
        &mut s,
        &[
            (p::ATTACK, 1050.0),
            (p::RELEASE, 4500.0),
            (p::MACRO_1, 0.55),
            (p::MACRO_3, 0.20),
        ],
    );
    for pitch in [40, 55, 59, 64] {
        add(&mut pattern, 88 * BAR, pitch, BAR * 5, 52, &s);
    }
    for (bar, offset, pitch, gate, vel) in [
        (88, 96, 79, 120, 50),
        (89, 72, 77, 144, 46),
        (90, 120, 76, 168, 43),
        (92, 24, 71, 144, 39),
        (93, 48, 76, 120, 32),
    ] {
        add(&mut pattern, bar * BAR + offset, pitch, gate, vel, &s);
    }
    song.patterns = vec![pattern];
    song.tracks[0].blocks[0].length_ticks = BARS * BAR;
    song.loop_on = false;
    song.loop_brace = Some((0, BARS * BAR));
    for (bar, name) in [
        (0, "01 Skin · linear 12-pulse"),
        (16, "02 Speech · Phrygian leaks"),
        (32, "03 Coil · melodic engine"),
        (48, "04 Ascent · opening harmonics"),
        (56, "05 Flash · fast ornamental peak"),
        (64, "06 Sky · rhythm becomes air"),
        (72, "07 Counterlight · two answering lines"),
        (88, "08 Soft exit"),
    ] {
        song.locators.push(Locator {
            tick: bar * BAR,
            name: name.into(),
        });
    }
    for (bar, gain) in [
        (0, 0.85),
        (32, 0.85),
        (56, 0.79),
        (64, 0.79),
        (68, 0.95),
        (88, 0.95),
        (90, 0.80),
        (92, 0.50),
        (94, 0.17),
        (96, 0.0),
    ] {
        song.tracks[0].insert_point(TRACK_VOLUME, bar * BAR, gain);
    }
    // The desk's existing stereo hall wraps the mono instrument graph. It is
    // an effect, not another voice source. Its send opens with the sky.
    let room = song.tracks[0]
        .strip
        .iter_mut()
        .find(|d| d.kind == DeviceKind::Console(daw::console::SectionKind::Room))
        .unwrap();
    room.bypassed = false;
    use daw::params::console::room as rp;
    room.set(rp::ALGO, 1.0);
    room.set(rp::PREDELAY, 31.0);
    room.set(rp::SIZE, 72.0);
    room.set(rp::DAMP, 42.0);
    room.set(rp::MIX, 0.0);
    let target = daw::targets::device_target(room.id.0, room.kind.spec(), "Mix");
    for (bar, wet) in [
        (0, 1.5),
        (32, 4.0),
        (56, 9.0),
        (64, 15.0),
        (72, 36.0),
        (96, 40.0),
    ] {
        song.tracks[0].insert_point(&target, bar * BAR, wet);
    }
    song
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let dir = Path::new(args.get(1).ok_or("give a NEW output directory")?);
    if dir.exists() {
        return Err("destination exists; choose a fresh version".into());
    }
    let song = compose();
    assert_eq!(
        song.tracks
            .iter()
            .filter_map(|t| t.machine.as_ref())
            .filter(|d| d.is_instrument())
            .count(),
        1
    );
    for pattern in &song.patterns {
        for step in 0..pattern.step_count() {
            assert!(pattern.trig(step).sound.is_none());
        }
    }
    let note_count: usize = song
        .patterns
        .iter()
        .map(|p| {
            (0..p.step_count())
                .map(|i| p.trig(i).notes.len())
                .sum::<usize>()
        })
        .sum();
    let (spec, _) = daw::song_graph::build_song(&song);
    assert_eq!(
        spec.iter_ordered()
            .filter(|(_, node)| matches!(node, NodeSpec::Spectral { .. }))
            .count(),
        1
    );
    let started = std::time::Instant::now();
    let _checked = spec.compile_at_tempo(48000, 256, BPM)?;
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Document {
        version: u32,
        song: Song,
    }
    let text = ron::ser::to_string_pretty(
        &Document {
            version: 4,
            song: song.clone(),
        },
        ron::ser::PrettyConfig::default(),
    )?;
    let recalled: Document = ron::from_str(&text)?;
    assert_eq!(song, recalled.song);
    std::fs::create_dir(dir)?;
    write_new(&dir.join("Skin Coil Sky.stage.ron"), &text)?;
    write_new(
        &dir.join("patch.ron"),
        &ron::ser::to_string_pretty(&patch(), ron::ser::PrettyConfig::default())?,
    )?;
    println!(
        "ONE Spectral instance; {note_count} editable notes; {} bars / 160 seconds; native graph validated in {:.2}s",
        BARS,
        started.elapsed().as_secs_f32()
    );
    if args.iter().any(|s| s == "--render") {
        bounce(
            &spec,
            &BounceOptions {
                sample_rate: 48000,
                block_frames: 256,
                bpm: BPM,
                length_beats: BARS as f64 * 4.0 + 19.2,
                start_beats: 0.0,
                format: BounceFormat::Float32,
            },
            &dir.join("preflight.wav"),
        )?;
        println!(
            "Native-graph preflight complete in {:.2}s",
            started.elapsed().as_secs_f32()
        );
    }
    Ok(())
}
