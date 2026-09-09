//! ROM's factory bank: the recipes, their key/velocity zones, and the
//! bake cache that turns one into audio.
//!
//! THE RECIPE IS THE TRUTH. A recipe is static data in this file — small,
//! diffable, versioned with the source. The wav under `~/Corpus/daw/rom`
//! is derived: [`Bank::load`] renders any file that is missing and reads
//! it back through the sampler's existing material cache, and
//! [`Bank::render`] builds the same bank in memory with no IO at all,
//! which is what the tests use.
//!
//! Green zone. Nothing here is called from the audio thread: the graph
//! builds a `Bank` and the voices only read it.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::audio::material::Material;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

/// Bumped when the RENDERING changes in a way that makes old wavs wrong.
/// It is part of every cache file's name, so a bump never reads a stale
/// file — and never deletes one either.
pub const BANK_VERSION: u32 = 1;

/// How long the looped part of a sustained recipe is, before rounding up
/// to whole cycles.
const LOOP_SECONDS: f32 = 0.35;

/// The fade that opens every sample, so a bake can never click at frame
/// zero however loud its first cycle is.
const OPEN_MS: f32 = 1.5;

/// One baked sample: what to render, and the pitch it is mapped at.
///
/// The tone is `root + offset` semitones — `offset` is the multisample's
/// baked-in character (FIFTH really is a fifth above the key it plays),
/// not a tuning error, so it moves with the note like everything else.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recipe {
    /// Stable, and part of the cache file's name. Never rename one.
    pub name: &'static str,
    /// The key this sample is mapped at: playing it reads at speed 1.
    pub root: u8,
    /// Semitones the baked tone sits above `root`.
    pub offset: f32,
    /// The second partial's level through the sustained loop.
    pub partial2: f32,
    /// The segment before the loop, where the timbre still moves.
    pub attack_ms: f32,
    /// The second partial's level at the very start of the attack. It
    /// glides to `partial2` by the time the loop begins, so the loop is
    /// exactly periodic and the attack is not.
    pub attack_partial2: f32,
    /// Non-zero makes a ONE-SHOT: an exponential decay, no loop.
    pub decay_ms: f32,
    /// A one-shot's length. Ignored by a sustained recipe, whose length
    /// is its attack plus its loop.
    pub seconds: f32,
}

const fn tone(name: &'static str, root: u8, offset: f32, partial2: f32, attack2: f32) -> Recipe {
    Recipe {
        name,
        root,
        offset,
        partial2,
        attack_ms: 60.0,
        attack_partial2: attack2,
        decay_ms: 0.0,
        seconds: 0.0,
    }
}

const fn hit(name: &'static str, root: u8, partial2: f32, decay_ms: f32, seconds: f32) -> Recipe {
    Recipe {
        name,
        root,
        offset: 0.0,
        partial2,
        attack_ms: 2.0,
        attack_partial2: partial2 * 2.0,
        decay_ms,
        seconds,
    }
}

/// The sine test bank. Sines, deliberately: a looped sine is the
/// strictest test of the read path there is — a wrong loop point, a
/// wrong rate or a wrong interpolation is a click or a beat against the
/// true frequency, where a bell would hide all three. The real recipes
/// (piano, strings, brass, …) are their own sittings; see the brief.
pub const RECIPES: &[Recipe] = &[
    // SINE — five key zones, two velocity layers. The hard layer carries
    // a second partial, so a velocity switch is a TIMBRE change and a
    // broken switch cannot pass for a level difference.
    tone("sine-36-soft", 36, 0.0, 0.02, 0.10),
    tone("sine-36-hard", 36, 0.0, 0.50, 1.20),
    tone("sine-48-soft", 48, 0.0, 0.02, 0.10),
    tone("sine-48-hard", 48, 0.0, 0.50, 1.20),
    tone("sine-60-soft", 60, 0.0, 0.02, 0.10),
    tone("sine-60-hard", 60, 0.0, 0.50, 1.20),
    tone("sine-72-soft", 72, 0.0, 0.02, 0.10),
    tone("sine-72-hard", 72, 0.0, 0.50, 1.20),
    tone("sine-84-soft", 84, 0.0, 0.02, 0.10),
    tone("sine-84-hard", 84, 0.0, 0.50, 1.20),
    // FIFTH — a fifth above the key it plays, so layering it under SINE
    // is audibly an interval: that is how the two-oscillator path is
    // checked by ear rather than by a meter.
    tone("fifth-36", 36, 7.0, 0.15, 0.40),
    tone("fifth-48", 48, 7.0, 0.15, 0.40),
    tone("fifth-60", 60, 7.0, 0.15, 0.40),
    tone("fifth-72", 72, 7.0, 0.15, 0.40),
    tone("fifth-84", 84, 7.0, 0.15, 0.40),
    // OCTAVE — the same, an octave up.
    tone("octave-36", 36, 12.0, 0.15, 0.40),
    tone("octave-48", 48, 12.0, 0.15, 0.40),
    tone("octave-60", 60, 12.0, 0.15, 0.40),
    tone("octave-72", 72, 12.0, 0.15, 0.40),
    tone("octave-84", 84, 12.0, 0.15, 0.40),
    // PING and THUD — one-shots, so LOOP off has something honest to
    // play and the amp gate has something to not hold.
    hit("ping-48", 48, 0.60, 500.0, 1.2),
    hit("ping-60", 60, 0.60, 500.0, 1.2),
    hit("ping-72", 72, 0.60, 500.0, 1.2),
    hit("thud-36", 36, 0.10, 250.0, 0.8),
    hit("thud-48", 48, 0.10, 250.0, 0.8),
];

/// One key/velocity rectangle of a multisample, naming the sample that
/// sounds inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zone {
    pub lo: u8,
    pub hi: u8,
    pub vel_lo: u8,
    pub vel_hi: u8,
    /// Index into [`RECIPES`], and into a built `Bank`'s samples.
    pub sample: u16,
}

const fn z(lo: u8, hi: u8, vel_lo: u8, vel_hi: u8, sample: u16) -> Zone {
    Zone {
        lo,
        hi,
        vel_lo,
        vel_hi,
        sample,
    }
}

/// A MULTISAMPLE — Korg's "multisound": the zones one oscillator plays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Multi {
    pub name: &'static str,
    /// What the bank sitting will file it under. Carried now so the
    /// filing is decided with the content, not bolted on after.
    pub category: &'static str,
    pub zones: &'static [Zone],
}

const SINE_ZONES: &[Zone] = &[
    z(0, 41, 1, 63, 0),
    z(0, 41, 64, 127, 1),
    z(42, 53, 1, 63, 2),
    z(42, 53, 64, 127, 3),
    z(54, 65, 1, 63, 4),
    z(54, 65, 64, 127, 5),
    z(66, 77, 1, 63, 6),
    z(66, 77, 64, 127, 7),
    z(78, 127, 1, 63, 8),
    z(78, 127, 64, 127, 9),
];
const FIFTH_ZONES: &[Zone] = &[
    z(0, 41, 1, 127, 10),
    z(42, 53, 1, 127, 11),
    z(54, 65, 1, 127, 12),
    z(66, 77, 1, 127, 13),
    z(78, 127, 1, 127, 14),
];
const OCTAVE_ZONES: &[Zone] = &[
    z(0, 41, 1, 127, 15),
    z(42, 53, 1, 127, 16),
    z(54, 65, 1, 127, 17),
    z(66, 77, 1, 127, 18),
    z(78, 127, 1, 127, 19),
];
const PING_ZONES: &[Zone] = &[
    z(0, 53, 1, 127, 20),
    z(54, 65, 1, 127, 21),
    z(66, 127, 1, 127, 22),
];
const THUD_ZONES: &[Zone] = &[z(0, 41, 1, 127, 23), z(42, 127, 1, 127, 24)];

pub const MULTIS: &[Multi] = &[
    Multi {
        name: "Sine",
        category: "Tone",
        zones: SINE_ZONES,
    },
    Multi {
        name: "Fifth",
        category: "Tone",
        zones: FIFTH_ZONES,
    },
    Multi {
        name: "Octave",
        category: "Tone",
        zones: OCTAVE_ZONES,
    },
    Multi {
        name: "Ping",
        category: "Decay",
        zones: PING_ZONES,
    },
    Multi {
        name: "Thud",
        category: "Decay",
        zones: THUD_ZONES,
    },
];

/// The words the PCM cell steps through, parallel to [`MULTIS`]. The
/// deck's choice list is static, which is why the cell is one flat list
/// over the bank rather than a category and an index: a name is worth
/// more than a coordinate on a machine whose whole point is its content.
pub const MULTI_NAMES: &[&str] = &["Sine", "Fifth", "Octave", "Ping", "Thud"];

/// The PCM cell's top value.
pub const MULTI_MAX: f32 = (MULTIS.len() - 1) as f32;

/// Where a rendered sample sits in its own frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub frames: u64,
    /// Samples in one cycle of the baked tone — a WHOLE number, which is
    /// what makes a loop of whole cycles seamless without a crossfade.
    pub period: u64,
    pub loop_start: u64,
    pub loop_end: u64,
    pub looped: bool,
    /// The frequency actually baked, `rate / period`.
    pub hz: f64,
    /// What the reader multiplies its speed by so the rounded period
    /// still sounds at the true pitch of `root + offset`.
    pub correction: f64,
}

impl Recipe {
    /// The pitch the recipe wants, in hertz.
    pub fn ideal_hz(&self) -> f64 {
        440.0 * 2f64.powf((f64::from(self.root) + f64::from(self.offset) - 69.0) / 12.0)
    }

    /// Where everything lands at `rate`. Pure arithmetic: the wav needs
    /// no `smpl` chunk and no sidecar, because the loop points are a
    /// function of the recipe and the rate.
    pub fn layout(&self, rate: u32) -> Layout {
        let rate_f = f64::from(rate.max(1));
        let ideal = self.ideal_hz();
        // A whole number of samples per cycle. The tone is up to half a
        // sample of period sharp or flat; `correction` takes it back.
        let period = (rate_f / ideal).round().max(2.0);
        let hz = rate_f / period;
        let correction = ideal / hz;
        let period_u = period as u64;
        if self.decay_ms > 0.0 {
            let frames = (f64::from(self.seconds.max(0.01)) * rate_f)
                .round()
                .max(1.0) as u64;
            return Layout {
                frames,
                period: period_u,
                loop_start: 0,
                loop_end: frames,
                looped: false,
                hz,
                correction,
            };
        }
        let cycles = |ms: f32| -> u64 {
            let want = f64::from(ms.max(0.0)) / 1000.0 * rate_f;
            ((want / period).ceil() as u64).max(1)
        };
        let attack = cycles(self.attack_ms) * period_u;
        let loop_len = cycles(LOOP_SECONDS * 1000.0) * period_u;
        Layout {
            frames: attack + loop_len,
            period: period_u,
            loop_start: attack,
            loop_end: attack + loop_len,
            looped: true,
            hz,
            correction,
        }
    }
}

/// Render one recipe. Deterministic: the same recipe and rate give the
/// same bytes on any machine, which is what makes the cache safe.
pub fn render(recipe: &Recipe, rate: u32) -> Vec<f32> {
    let layout = recipe.layout(rate);
    let rate_f = f64::from(rate.max(1));
    let period = layout.period as f64;
    let attack = layout.loop_start.max(1) as f64;
    let open = (f64::from(OPEN_MS) / 1000.0 * rate_f).max(1.0);
    let decay = f64::from(recipe.decay_ms) / 1000.0 * rate_f;
    let mut out = Vec::with_capacity(layout.frames as usize);
    for i in 0..layout.frames {
        let at = i as f64;
        // Phase from the cycle index, so it is exact at every wrap and
        // the loop's last sample is followed by the loop's first.
        let phase = (at % period) / period * std::f64::consts::TAU;
        let (amp, partial2) = if layout.looped {
            let through = (at / attack).min(1.0);
            // The second partial glides to its loop value by the time the
            // loop starts, so the looped part is exactly periodic.
            let p2 = f64::from(recipe.attack_partial2)
                + (f64::from(recipe.partial2) - f64::from(recipe.attack_partial2)) * through;
            (1.0, p2)
        } else {
            let a = if decay > 0.0 {
                (-at / decay).exp()
            } else {
                1.0
            };
            // A one-shot's partial decays faster than its fundamental:
            // that is what makes a struck sound rather than a fading tone.
            (
                a,
                f64::from(recipe.partial2) * (-at / (decay * 0.45).max(1.0)).exp(),
            )
        };
        let open_gain = (at / open).min(1.0);
        let x = (phase.sin() + partial2 * (phase * 2.0).sin()) / (1.0 + partial2.abs());
        out.push((x * amp * open_gain) as f32);
    }
    out
}

/// `~/Corpus/daw/rom` — beside the theme, the tune overrides and the
/// sound library.
pub fn cache_dir() -> PathBuf {
    crate::corpus::dir().join("daw").join("rom")
}

/// A recipe's identity as eight hex digits: every field that changes the
/// audio, plus [`BANK_VERSION`]. A changed recipe writes a new file and
/// can never read the old one.
fn fingerprint(recipe: &Recipe, rate: u32) -> u32 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
    };
    eat(recipe.name.as_bytes());
    eat(&BANK_VERSION.to_le_bytes());
    eat(&rate.to_le_bytes());
    eat(&[recipe.root]);
    for f in [
        recipe.offset,
        recipe.partial2,
        recipe.attack_ms,
        recipe.attack_partial2,
        recipe.decay_ms,
        recipe.seconds,
    ] {
        eat(&f.to_bits().to_le_bytes());
    }
    ((h >> 32) as u32) ^ (h as u32)
}

pub fn wav_path(recipe: &Recipe, rate: u32) -> PathBuf {
    cache_dir().join(format!(
        "{}-{}-{:08x}.wav",
        recipe.name,
        rate,
        fingerprint(recipe, rate)
    ))
}

/// Render `recipe` to its cache file if it is not already there. Writes
/// to a temporary beside it and renames, so an interrupted bake never
/// leaves a half file for the cache to trust.
pub fn bake(recipe: &Recipe, rate: u32) -> std::io::Result<PathBuf> {
    let path = wav_path(recipe, rate);
    if path.exists() {
        return Ok(path);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let samples = render(recipe, rate);
    let tmp = path.with_extension("wav.part");
    write_wav(&tmp, &samples, rate)?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

fn write_wav(path: &Path, samples: &[f32], rate: u32) -> std::io::Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let io = |e: hound::Error| std::io::Error::other(e.to_string());
    let mut writer = hound::WavWriter::create(path, spec).map_err(io)?;
    for x in samples {
        writer.write_sample(*x).map_err(io)?;
    }
    writer.finalize().map_err(io)
}

/// One sample of a built bank: the audio, and how to read it.
#[derive(Debug, Clone)]
pub struct Sample {
    pub material: Material,
    pub root: u8,
    pub layout: Layout,
}

/// The bank the voices read: every recipe, built once per rate.
#[derive(Debug, Clone)]
pub struct Bank {
    pub samples: Vec<Sample>,
    pub rate: u32,
}

impl Bank {
    /// Build the bank in memory. No IO, no cache, no `~/Corpus`: this is
    /// the path the tests take and the fallback when the disk says no.
    pub fn render(rate: u32) -> Self {
        let samples = RECIPES
            .iter()
            .map(|recipe| {
                let layout = recipe.layout(rate);
                let audio = render(recipe, rate);
                Sample {
                    material: material_of(audio, rate, recipe.name),
                    root: recipe.root,
                    layout,
                }
            })
            .collect();
        Self { samples, rate }
    }

    /// Build the bank from the cache, baking whatever is missing. A file
    /// that will not write or will not read falls back to the in-memory
    /// render for that sample alone: the machine always sounds.
    pub fn load(rate: u32) -> Self {
        let samples = RECIPES
            .iter()
            .map(|recipe| {
                let layout = recipe.layout(rate);
                let material = bake(recipe, rate)
                    .ok()
                    .and_then(|path| crate::audio::material::load_cached(&path, rate).ok())
                    .filter(|m| m.frames == layout.frames)
                    .unwrap_or_else(|| material_of(render(recipe, rate), rate, recipe.name));
                Sample {
                    material,
                    root: recipe.root,
                    layout,
                }
            })
            .collect();
        Self { samples, rate }
    }

    pub fn sample(&self, at: u16) -> Option<&Sample> {
        self.samples.get(at as usize)
    }
}

fn material_of(audio: Vec<f32>, rate: u32, name: &str) -> Material {
    Material {
        frames: audio.len() as u64,
        samples: Arc::new(audio),
        channels: 1,
        source: PathBuf::from(name),
        sample_rate: rate,
        original_rate: rate,
        truncated: false,
    }
}

/// The zone a note and velocity land in. Static data and a bounded scan:
/// the audio thread calls this on every note-on.
///
/// Containment first; if nothing contains the note — which the tables do
/// not allow, but a hand-edited bank might — the nearest zone by key
/// answers, so a note is never silently dropped.
pub fn zone(multi: usize, note: u8, vel: u8) -> Option<&'static Zone> {
    let zones = MULTIS.get(multi)?.zones;
    let vel = vel.max(1);
    for zone in zones {
        if note >= zone.lo && note <= zone.hi && vel >= zone.vel_lo && vel <= zone.vel_hi {
            return Some(zone);
        }
    }
    let mut best: Option<(&'static Zone, i32)> = None;
    for zone in zones {
        let middle = (i32::from(zone.lo) + i32::from(zone.hi)) / 2;
        let distance = (middle - i32::from(note)).abs();
        if best.is_none_or(|(_, d)| distance < d) {
            best = Some((zone, distance));
        }
    }
    best.map(|(zone, _)| zone)
}

/// One built bank per rate, kept for the life of the process. Green zone:
/// the graph asks for it while it builds nodes, never while it renders.
type Built = Mutex<Vec<(u32, Arc<Bank>)>>;
static BUILT: LazyLock<Built> = LazyLock::new(|| Mutex::new(Vec::new()));

pub fn bank(rate: u32) -> Arc<Bank> {
    if let Ok(mut built) = BUILT.lock() {
        if let Some((_, bank)) = built.iter().find(|(at, _)| *at == rate) {
            return Arc::clone(bank);
        }
        let bank = Arc::new(Bank::load(rate));
        built.push((rate, Arc::clone(&bank)));
        return bank;
    }
    Arc::new(Bank::load(rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    #[test]
    fn every_zone_names_a_sample_and_every_multi_has_a_name() {
        assert_eq!(MULTIS.len(), MULTI_NAMES.len());
        for (at, multi) in MULTIS.iter().enumerate() {
            assert_eq!(multi.name, MULTI_NAMES[at]);
            assert!(!multi.zones.is_empty(), "{} has no zones", multi.name);
            for zone in multi.zones {
                assert!(
                    (zone.sample as usize) < RECIPES.len(),
                    "{} names a sample the bank lacks",
                    multi.name
                );
                assert!(zone.lo <= zone.hi && zone.vel_lo <= zone.vel_hi);
            }
        }
        assert_eq!(MULTI_MAX, (MULTIS.len() - 1) as f32);
    }

    /// Every key and every velocity finds a zone in every multisample:
    /// there is no hole a note can fall through.
    #[test]
    fn the_zone_map_is_total() {
        for multi in 0..MULTIS.len() {
            for note in 0..=127u8 {
                for vel in [1u8, 63, 64, 127] {
                    assert!(
                        zone(multi, note, vel).is_some(),
                        "{multi} has no zone for note {note} vel {vel}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_sustained_layout_is_whole_cycles_and_a_one_shot_is_not_looped() {
        for recipe in RECIPES {
            let layout = recipe.layout(RATE);
            assert!(layout.frames > 0, "{} is empty", recipe.name);
            if recipe.decay_ms > 0.0 {
                assert!(!layout.looped, "{} should be a one-shot", recipe.name);
                continue;
            }
            assert!(layout.looped, "{} should loop", recipe.name);
            assert_eq!(layout.loop_start % layout.period, 0);
            assert_eq!((layout.loop_end - layout.loop_start) % layout.period, 0);
            assert_eq!(layout.loop_end, layout.frames);
            // The rounded period is still the pitch the recipe asked for,
            // to within the correction the reader applies.
            let sounded = layout.hz * layout.correction;
            assert!(
                (sounded - recipe.ideal_hz()).abs() < 1e-6,
                "{}",
                recipe.name
            );
        }
    }

    /// The claim: a sustained sample loops without a discontinuity. The
    /// step across the seam is no larger than the largest step inside one
    /// ordinary cycle.
    #[test]
    fn a_sustained_sample_loops_seamlessly() {
        for recipe in RECIPES.iter().filter(|r| r.decay_ms == 0.0) {
            let layout = recipe.layout(RATE);
            let audio = render(recipe, RATE);
            let start = layout.loop_start as usize;
            let end = layout.loop_end as usize;
            let inside = audio[start..end]
                .windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0f32, f32::max);
            let seam = (audio[start] - audio[end - 1]).abs();
            assert!(
                seam <= inside + 1e-6,
                "{}: seam {seam} exceeds the largest step in the loop {inside}",
                recipe.name
            );
        }
    }

    #[test]
    fn rendering_is_deterministic_and_bounded() {
        for recipe in RECIPES {
            let a = render(recipe, RATE);
            let b = render(recipe, RATE);
            assert_eq!(a, b, "{} is not deterministic", recipe.name);
            assert!(
                a.iter().all(|x| x.is_finite() && x.abs() <= 1.0),
                "{} leaves the unit interval",
                recipe.name
            );
            assert_eq!(a.len() as u64, recipe.layout(RATE).frames);
        }
    }

    /// A hard-layer sample really is a different TIMBRE, not a louder
    /// one: it carries more energy above its fundamental.
    #[test]
    fn the_hard_velocity_layer_is_brighter_not_merely_louder() {
        let soft = RECIPES.iter().find(|r| r.name == "sine-60-soft");
        let hard = RECIPES.iter().find(|r| r.name == "sine-60-hard");
        let (Some(soft), Some(hard)) = (soft, hard) else {
            panic!("the sine multisample lost a layer");
        };
        assert!(hard.partial2 > soft.partial2 * 4.0);
        assert!(hard.attack_partial2 > hard.partial2);
    }

    /// The bank the tests use never touches the disk.
    #[test]
    fn a_rendered_bank_is_complete_and_silent_about_the_disk() {
        let bank = Bank::render(RATE);
        assert_eq!(bank.samples.len(), RECIPES.len());
        for (at, sample) in bank.samples.iter().enumerate() {
            assert_eq!(sample.material.frames, sample.layout.frames);
            assert_eq!(sample.material.channels, 1);
            assert_eq!(sample.root, RECIPES[at].root);
            assert!(!sample.material.is_empty());
        }
    }

    #[test]
    fn a_fingerprint_follows_every_field_that_changes_the_audio() {
        let base = RECIPES[0];
        let mut moved = base;
        moved.partial2 += 0.01;
        assert_ne!(fingerprint(&base, RATE), fingerprint(&moved, RATE));
        assert_ne!(fingerprint(&base, RATE), fingerprint(&base, 44_100));
        assert_eq!(fingerprint(&base, RATE), fingerprint(&base.clone(), RATE));
    }
}
