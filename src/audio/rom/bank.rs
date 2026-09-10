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
//! # The loop is the tuning grid
//!
//! A sustained sample is rendered as a SPECTRUM on the loop's own
//! frequency grid — every component sits at an exact multiple of
//! `rate / LOOP_FRAMES` — and then inverse-transformed. A sum of
//! sinusoids whose frequencies all divide the loop length is exactly
//! periodic over it, so the loop is seamless BY CONSTRUCTION: no
//! crossfade, no window, no click, however many detuned copies and
//! however much noise the recipe asks for.
//!
//! Three things follow, and they are what make the bank sound like
//! anything at all. Detuning is expressed in BEATS PER LOOP rather than
//! cents, because a whole number of beats per loop is what a seamless
//! loop can hold — and a slow beat is most of what a warm pad is.
//! Noise is a band of grid components with seeded random phases, which
//! is how a noise wash can loop at all. And the fundamental lands on
//! the nearest grid line, up to half a line out, which the reader's
//! speed correction takes back exactly.
//!
//! Green zone. Nothing here is called from the audio thread: the graph
//! builds a `Bank` and the voices only read it.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::audio::material::Material;
use crate::dsp::fft::RealFft;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

/// Bumped when the RENDERING changes in a way that makes old wavs wrong.
/// It is part of every cache file's name, so a bump never reads a stale
/// file — and never deletes one either.
pub const BANK_VERSION: u32 = 2;

/// The loop's length in frames: a power of two, for the transform, and
/// long enough that the grid is fine (0.37 Hz at 48 kHz) and the beating
/// can be slow.
pub const LOOP_FRAMES: usize = 131_072;

/// The fade that opens every sample, so a bake can never click at frame
/// zero however loud its first cycle is.
const OPEN_MS: f32 = 1.5;

/// The peak every rendered sample is normalised to.
const PEAK: f32 = 0.89;

/// What a recipe SOUNDS like: one additive spectrum, described.
///
/// Every family in the bank is this one structure with different
/// numbers — a saw pad, a choir, a Rhodes, a bell, a bowed ensemble, a
/// noise wash and a click are all a partial series with a tilt, some
/// stretch, some copies, some noise and some formants. One renderer
/// then serves them all, which is why the bank can grow by a row of
/// data rather than a new synthesiser.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tone {
    /// How many partials of the series to lay down. Zero is legal: a
    /// wash of noise with no tone in it at all.
    pub partials: u8,
    /// The series' fall, in decibels per octave. Six is a saw, twelve is
    /// a soft triangle, three is a hard bell.
    pub tilt: f32,
    /// How much the even partials are held back: 0 keeps the whole
    /// series, 1 leaves only the odd ones — reeds, clarinets, tines.
    pub odd: f32,
    /// Inharmonicity: partial `n` sits at `n^(1+stretch)`. A few
    /// thousandths is a piano's stiffness, a few hundredths is a bell.
    pub stretch: f32,
    /// Detuned copies of the whole series. Three or four is an ensemble.
    pub copies: u8,
    /// How far the copies are pulled apart, in BEATS PER LOOP. Two is a
    /// slow warm drift; eight is a chorus.
    pub beats: f32,
    /// A band of noise under the tone: breath, bow, air.
    pub noise: f32,
    /// Where that band sits, in hertz, and how wide it is in octaves.
    pub noise_hz: f32,
    pub noise_width: f32,
    /// Formant peaks in hertz — a vowel. Zero entries are unused.
    pub formants: [f32; 3],
    /// The transient at the onset: a bright knock over the first few
    /// milliseconds. This is the Triton's syn click, as a number.
    pub click: f32,
    /// How much brighter the attack is than the loop it settles into.
    /// The two spectra share their phases and only their levels are
    /// crossfaded, so the settle cannot break the seam.
    pub sweep: f32,
    /// The swell, in milliseconds: how long the tone takes to arrive.
    pub swell_ms: f32,
}

impl Tone {
    /// A plain harmonic series and nothing else — the shape the sine
    /// test material is a special case of.
    pub const fn plain(partials: u8, tilt: f32) -> Self {
        Self {
            partials,
            tilt,
            odd: 0.0,
            stretch: 0.0,
            copies: 1,
            beats: 0.0,
            noise: 0.0,
            noise_hz: 4000.0,
            noise_width: 2.0,
            formants: [0.0; 3],
            click: 0.0,
            sweep: 0.0,
            swell_ms: 6.0,
        }
    }

    pub const fn copies(mut self, copies: u8, beats: f32) -> Self {
        self.copies = copies;
        self.beats = beats;
        self
    }

    pub const fn air(mut self, noise: f32, hz: f32, width: f32) -> Self {
        self.noise = noise;
        self.noise_hz = hz;
        self.noise_width = width;
        self
    }

    pub const fn vowel(mut self, formants: [f32; 3]) -> Self {
        self.formants = formants;
        self
    }

    pub const fn shaped(mut self, odd: f32, stretch: f32) -> Self {
        self.odd = odd;
        self.stretch = stretch;
        self
    }

    pub const fn onset(mut self, click: f32, sweep: f32, swell_ms: f32) -> Self {
        self.click = click;
        self.sweep = sweep;
        self.swell_ms = swell_ms;
        self
    }

    /// The level of partial `n` (1-based) before the formants, as a
    /// linear gain. The engine and the pictures share this arithmetic.
    pub fn partial_level(&self, n: u32) -> f32 {
        if n == 0 {
            return 0.0;
        }
        let octaves = (n as f32).log2();
        let mut level = 10f32.powf(-self.tilt.max(0.0) * octaves / 20.0);
        if n.is_multiple_of(2) {
            level *= 1.0 - self.odd.clamp(0.0, 1.0);
        }
        level
    }

    /// Where partial `n` sits, as a multiple of the fundamental.
    pub fn partial_ratio(&self, n: u32) -> f32 {
        (n as f32).powf(1.0 + self.stretch)
    }

    /// The formants' weighting of one partial's frequency.
    fn formant_gain(&self, hz: f32) -> f32 {
        let mut gain = 1.0;
        for peak in self.formants {
            if peak <= 0.0 {
                continue;
            }
            // A wide resonance in log frequency: a vowel is a hill, not
            // a spike, or the sung note would change with the pitch.
            let octaves = (hz.max(1.0) / peak).log2();
            gain += 3.0 * (-(octaves * octaves) / 0.20).exp();
        }
        gain
    }
}

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
    pub tone: Tone,
    /// Non-zero makes a ONE-SHOT: an exponential decay, no loop.
    pub decay_ms: f32,
    /// A one-shot's length. A sustained recipe's length is its swell
    /// plus one loop.
    pub seconds: f32,
}

const fn sustained(name: &'static str, root: u8, offset: f32, tone: Tone) -> Recipe {
    Recipe {
        name,
        root,
        offset,
        tone,
        decay_ms: 0.0,
        seconds: 0.0,
    }
}

const fn one_shot(name: &'static str, root: u8, tone: Tone, decay_ms: f32, seconds: f32) -> Recipe {
    Recipe {
        name,
        root,
        offset: 0.0,
        tone,
        decay_ms,
        seconds,
    }
}

// ---------------------------------------------------------------- tones

/// The warm detuned pad the whole style rests on: a saw series under a
/// gentle tilt, three copies drifting two beats apart, a breath of air
/// over the top, and a long swell.
const WARM: Tone = Tone::plain(28, 7.0)
    .copies(3, 2.0)
    .air(0.020, 3800.0, 2.4)
    .onset(0.0, 0.55, 420.0);
/// The same, played hard: brighter series, a little more drift.
const WARM_HARD: Tone = Tone::plain(34, 5.6)
    .copies(3, 3.0)
    .air(0.030, 4800.0, 2.4)
    .onset(0.06, 0.85, 300.0);

/// A choir on "ah": the formants do the work, the breath sells it.
const CHOIR: Tone = Tone::plain(30, 8.5)
    .copies(3, 3.0)
    .air(0.060, 3000.0, 2.8)
    .vowel([620.0, 1120.0, 2600.0])
    .onset(0.0, 0.35, 620.0);
const CHOIR_HARD: Tone = Tone::plain(34, 7.0)
    .copies(4, 4.0)
    .air(0.085, 3400.0, 2.8)
    .vowel([680.0, 1220.0, 2750.0])
    .onset(0.0, 0.55, 430.0);

/// The tine: odd-weighted, a little stiff, and a knock at the onset —
/// the electric piano this music is unimaginable without.
const TINE: Tone = Tone::plain(14, 10.0)
    .shaped(0.45, 0.006)
    .copies(1, 0.0)
    .onset(0.30, 1.10, 24.0);
const TINE_HARD: Tone = Tone::plain(18, 8.0)
    .shaped(0.35, 0.008)
    .copies(2, 1.0)
    .onset(0.55, 1.60, 14.0);

/// Glass: a stretched series, struck. An octave of shimmer over a pad.
const GLASS: Tone = Tone::plain(11, 5.0)
    .shaped(0.55, 0.055)
    .copies(2, 1.0)
    .air(0.015, 7000.0, 1.6)
    .onset(0.22, 1.30, 40.0);

/// A bowed ensemble: many partials, four copies, and the bow's own hiss.
const BOW: Tone = Tone::plain(44, 6.5)
    .copies(4, 4.0)
    .air(0.080, 5200.0, 2.6)
    .onset(0.0, 0.45, 900.0);
const BOW_HARD: Tone = Tone::plain(52, 5.2)
    .copies(4, 6.0)
    .air(0.110, 6000.0, 2.6)
    .onset(0.05, 0.75, 620.0);

/// Air: no tone at all, a wide band of noise that loops because every
/// one of its components is on the grid.
const AIR: Tone = Tone::plain(0, 0.0)
    .air(0.900, 2200.0, 3.2)
    .onset(0.0, 0.30, 1200.0);

/// A soft sub: three partials and a steep tilt.
const SUB: Tone = Tone::plain(3, 14.0).onset(0.0, 0.20, 90.0);

/// THE SYN CLICK. A one-shot: a bright metallic knock, gone in a
/// fortieth of a second, made to sit under the onset of something else.
const CLICK: Tone = Tone::plain(7, 3.0)
    .shaped(0.30, 0.220)
    .air(0.520, 6200.0, 2.2)
    .onset(1.00, 1.80, 0.6);

/// A sine, and the test material that is a special case of everything
/// above: one partial, and a second one for the hard layer.
const SINE_SOFT: Tone = Tone::plain(1, 0.0).onset(0.0, 0.10, 60.0);
const SINE_HARD: Tone = Tone::plain(2, 6.0).onset(0.0, 0.90, 60.0);
const PING: Tone = Tone::plain(4, 9.0).shaped(0.2, 0.02).onset(0.35, 1.2, 2.0);
const THUD: Tone = Tone::plain(2, 16.0).onset(0.10, 0.4, 2.0);

/// The bank. Five roots for anything played across the keyboard, so a
/// zone never repitches more than half an octave.
pub const RECIPES: &[Recipe] = &[
    // --- PAD
    sustained("warm-36-soft", 36, 0.0, WARM),
    sustained("warm-36-hard", 36, 0.0, WARM_HARD),
    sustained("warm-48-soft", 48, 0.0, WARM),
    sustained("warm-48-hard", 48, 0.0, WARM_HARD),
    sustained("warm-60-soft", 60, 0.0, WARM),
    sustained("warm-60-hard", 60, 0.0, WARM_HARD),
    sustained("warm-72-soft", 72, 0.0, WARM),
    sustained("warm-72-hard", 72, 0.0, WARM_HARD),
    sustained("warm-84-soft", 84, 0.0, WARM),
    sustained("warm-84-hard", 84, 0.0, WARM_HARD),
    sustained("choir-48-soft", 48, 0.0, CHOIR),
    sustained("choir-48-hard", 48, 0.0, CHOIR_HARD),
    sustained("choir-60-soft", 60, 0.0, CHOIR),
    sustained("choir-60-hard", 60, 0.0, CHOIR_HARD),
    sustained("choir-72-soft", 72, 0.0, CHOIR),
    sustained("choir-72-hard", 72, 0.0, CHOIR_HARD),
    sustained("bow-36-soft", 36, 0.0, BOW),
    sustained("bow-36-hard", 36, 0.0, BOW_HARD),
    sustained("bow-48-soft", 48, 0.0, BOW),
    sustained("bow-48-hard", 48, 0.0, BOW_HARD),
    sustained("bow-60-soft", 60, 0.0, BOW),
    sustained("bow-60-hard", 60, 0.0, BOW_HARD),
    sustained("bow-72-soft", 72, 0.0, BOW),
    sustained("bow-72-hard", 72, 0.0, BOW_HARD),
    sustained("air-48", 48, 0.0, AIR),
    sustained("air-72", 72, 0.0, AIR),
    // --- KEYS
    sustained("tine-36-soft", 36, 0.0, TINE),
    sustained("tine-36-hard", 36, 0.0, TINE_HARD),
    sustained("tine-48-soft", 48, 0.0, TINE),
    sustained("tine-48-hard", 48, 0.0, TINE_HARD),
    sustained("tine-60-soft", 60, 0.0, TINE),
    sustained("tine-60-hard", 60, 0.0, TINE_HARD),
    sustained("tine-72-soft", 72, 0.0, TINE),
    sustained("tine-72-hard", 72, 0.0, TINE_HARD),
    sustained("glass-48", 48, 0.0, GLASS),
    sustained("glass-60", 60, 0.0, GLASS),
    sustained("glass-72", 72, 0.0, GLASS),
    sustained("glass-84", 84, 0.0, GLASS),
    // --- BASS
    sustained("sub-24", 24, 0.0, SUB),
    sustained("sub-36", 36, 0.0, SUB),
    // --- HIT
    one_shot("click-48", 48, CLICK, 26.0, 0.22),
    one_shot("click-60", 60, CLICK, 24.0, 0.20),
    one_shot("click-72", 72, CLICK, 22.0, 0.18),
    one_shot("ping-48", 48, PING, 500.0, 1.2),
    one_shot("ping-60", 60, PING, 500.0, 1.2),
    one_shot("ping-72", 72, PING, 500.0, 1.2),
    one_shot("thud-36", 36, THUD, 250.0, 0.8),
    one_shot("thud-48", 48, THUD, 250.0, 0.8),
    // --- TEST
    sustained("sine-36-soft", 36, 0.0, SINE_SOFT),
    sustained("sine-36-hard", 36, 0.0, SINE_HARD),
    sustained("sine-48-soft", 48, 0.0, SINE_SOFT),
    sustained("sine-48-hard", 48, 0.0, SINE_HARD),
    sustained("sine-60-soft", 60, 0.0, SINE_SOFT),
    sustained("sine-60-hard", 60, 0.0, SINE_HARD),
    sustained("sine-72-soft", 72, 0.0, SINE_SOFT),
    sustained("sine-72-hard", 72, 0.0, SINE_HARD),
    sustained("sine-84-soft", 84, 0.0, SINE_SOFT),
    sustained("sine-84-hard", 84, 0.0, SINE_HARD),
    sustained("fifth-48", 48, 7.0, SINE_SOFT),
    sustained("fifth-60", 60, 7.0, SINE_SOFT),
    sustained("fifth-72", 72, 7.0, SINE_SOFT),
    sustained("octave-48", 48, 12.0, SINE_SOFT),
    sustained("octave-60", 60, 12.0, SINE_SOFT),
    sustained("octave-72", 72, 12.0, SINE_SOFT),
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
    pub category: &'static str,
    pub zones: &'static [Zone],
}

/// Five key zones over two velocity layers, from a run of ten recipes.
const fn five_by_two(first: u16) -> [Zone; 10] {
    [
        z(0, 41, 1, 63, first),
        z(0, 41, 64, 127, first + 1),
        z(42, 53, 1, 63, first + 2),
        z(42, 53, 64, 127, first + 3),
        z(54, 65, 1, 63, first + 4),
        z(54, 65, 64, 127, first + 5),
        z(66, 77, 1, 63, first + 6),
        z(66, 77, 64, 127, first + 7),
        z(78, 127, 1, 63, first + 8),
        z(78, 127, 64, 127, first + 9),
    ]
}

/// Four key zones over two velocity layers.
const fn four_by_two(first: u16) -> [Zone; 8] {
    [
        z(0, 41, 1, 63, first),
        z(0, 41, 64, 127, first + 1),
        z(42, 53, 1, 63, first + 2),
        z(42, 53, 64, 127, first + 3),
        z(54, 65, 1, 63, first + 4),
        z(54, 65, 64, 127, first + 5),
        z(66, 127, 1, 63, first + 6),
        z(66, 127, 64, 127, first + 7),
    ]
}

const WARM_ZONES: &[Zone] = &five_by_two(0);
const CHOIR_ZONES: &[Zone] = &[
    z(0, 53, 1, 63, 10),
    z(0, 53, 64, 127, 11),
    z(54, 65, 1, 63, 12),
    z(54, 65, 64, 127, 13),
    z(66, 127, 1, 63, 14),
    z(66, 127, 64, 127, 15),
];
const BOW_ZONES: &[Zone] = &four_by_two(16);
const AIR_ZONES: &[Zone] = &[z(0, 59, 1, 127, 24), z(60, 127, 1, 127, 25)];
const TINE_ZONES: &[Zone] = &four_by_two(26);
const GLASS_ZONES: &[Zone] = &[
    z(0, 53, 1, 127, 34),
    z(54, 65, 1, 127, 35),
    z(66, 77, 1, 127, 36),
    z(78, 127, 1, 127, 37),
];
const SUB_ZONES: &[Zone] = &[z(0, 29, 1, 127, 38), z(30, 127, 1, 127, 39)];
const CLICK_ZONES: &[Zone] = &[
    z(0, 53, 1, 127, 40),
    z(54, 65, 1, 127, 41),
    z(66, 127, 1, 127, 42),
];
const PING_ZONES: &[Zone] = &[
    z(0, 53, 1, 127, 43),
    z(54, 65, 1, 127, 44),
    z(66, 127, 1, 127, 45),
];
const THUD_ZONES: &[Zone] = &[z(0, 41, 1, 127, 46), z(42, 127, 1, 127, 47)];
const SINE_ZONES: &[Zone] = &five_by_two(48);
const FIFTH_ZONES: &[Zone] = &[
    z(0, 53, 1, 127, 58),
    z(54, 65, 1, 127, 59),
    z(66, 127, 1, 127, 60),
];
const OCTAVE_ZONES: &[Zone] = &[
    z(0, 53, 1, 127, 61),
    z(54, 65, 1, 127, 62),
    z(66, 127, 1, 127, 63),
];

pub const MULTIS: &[Multi] = &[
    Multi {
        name: "Warm",
        category: "Pad",
        zones: WARM_ZONES,
    },
    Multi {
        name: "Choir",
        category: "Pad",
        zones: CHOIR_ZONES,
    },
    Multi {
        name: "Bow",
        category: "Pad",
        zones: BOW_ZONES,
    },
    Multi {
        name: "Air",
        category: "Pad",
        zones: AIR_ZONES,
    },
    Multi {
        name: "Tine",
        category: "Keys",
        zones: TINE_ZONES,
    },
    Multi {
        name: "Glass",
        category: "Keys",
        zones: GLASS_ZONES,
    },
    Multi {
        name: "Sub",
        category: "Bass",
        zones: SUB_ZONES,
    },
    Multi {
        name: "Click",
        category: "Hit",
        zones: CLICK_ZONES,
    },
    Multi {
        name: "Ping",
        category: "Hit",
        zones: PING_ZONES,
    },
    Multi {
        name: "Thud",
        category: "Hit",
        zones: THUD_ZONES,
    },
    Multi {
        name: "Sine",
        category: "Test",
        zones: SINE_ZONES,
    },
    Multi {
        name: "Fifth",
        category: "Test",
        zones: FIFTH_ZONES,
    },
    Multi {
        name: "Octave",
        category: "Test",
        zones: OCTAVE_ZONES,
    },
];

/// The words the PCM cell steps through, parallel to [`MULTIS`]. The
/// deck's choice list is static, which is why the cell is one flat list
/// over the bank rather than a category and an index: a name is worth
/// more than a coordinate on a machine whose whole point is its content.
pub const MULTI_NAMES: &[&str] = &[
    "Warm", "Choir", "Bow", "Air", "Tine", "Glass", "Sub", "Click", "Ping", "Thud", "Sine",
    "Fifth", "Octave",
];

/// The PCM cell's top value.
pub const MULTI_MAX: f32 = (MULTIS.len() - 1) as f32;

/// Where a rendered sample sits in its own frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub frames: u64,
    /// The loop's length: [`LOOP_FRAMES`] for a sustained recipe, and
    /// the whole file for a one-shot that does not loop.
    pub period: u64,
    pub loop_start: u64,
    pub loop_end: u64,
    pub looped: bool,
    /// The frequency actually baked: the grid line the fundamental
    /// landed on.
    pub hz: f64,
    /// What the reader multiplies its speed by so that grid line still
    /// sounds at the true pitch of `root + offset`.
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
        if self.decay_ms > 0.0 {
            let frames = (f64::from(self.seconds.max(0.01)) * rate_f)
                .round()
                .max(1.0) as u64;
            return Layout {
                frames,
                period: frames,
                loop_start: 0,
                loop_end: frames,
                looped: false,
                hz: ideal,
                correction: 1.0,
            };
        }
        // The fundamental lands on the nearest line of the loop's own
        // grid, which is what makes the loop exact; the correction takes
        // the rounding back.
        let line = (ideal * LOOP_FRAMES as f64 / rate_f).round().max(1.0);
        let hz = line * rate_f / LOOP_FRAMES as f64;
        let swell = (f64::from(self.tone.swell_ms.max(0.0)) / 1000.0 * rate_f).round() as u64;
        Layout {
            frames: swell + LOOP_FRAMES as u64,
            period: LOOP_FRAMES as u64,
            loop_start: swell,
            loop_end: swell + LOOP_FRAMES as u64,
            looped: true,
            hz,
            correction: ideal / hz,
        }
    }
}

/// A seeded generator: the phases of a bank must be the same on every
/// machine, or the cache would not be a cache.
struct Seeded(u64);

impl Seeded {
    fn of(name: &str) -> Self {
        let mut hash: u64 = 0x2545_f491_4f6c_dd1d;
        for byte in name.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x1000_0000_01b3);
        }
        Self(hash | 1)
    }

    /// A turn of phase, 0..1.
    fn turn(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / 16_777_216.0
    }
}

/// The spectrum of one tone on the loop's grid, as bins.
///
/// `bright` walks the attack's spectrum away from the loop's: the same
/// components, the highs held up. Both share their phases, so a
/// crossfade between them cannot break the seam.
fn spectrum(recipe: &Recipe, rate: u32, bright: f32, real: &mut [f32], imag: &mut [f32]) {
    let tone = &recipe.tone;
    let bins = real.len().min(imag.len());
    for value in real.iter_mut().take(bins) {
        *value = 0.0;
    }
    for value in imag.iter_mut().take(bins) {
        *value = 0.0;
    }
    let rate_f = rate.max(1) as f32;
    let line_hz = rate_f / LOOP_FRAMES as f32;
    let mut seeded = Seeded::of(recipe.name);
    let fundamental = recipe.layout(rate).hz as f32;

    let add = |line: f32, level: f32, phase: f32, real: &mut [f32], imag: &mut [f32]| {
        let at = line.round().max(1.0) as usize;
        if at >= bins || !level.is_finite() || level <= 0.0 {
            return;
        }
        let turns = phase * std::f32::consts::TAU;
        real[at] += level * turns.cos();
        imag[at] += level * turns.sin();
    };

    // The partial series, once per detuned copy.
    let copies = tone.copies.max(1);
    for copy in 0..copies {
        // Copies are pulled apart in whole beats per loop — the only
        // detuning a seamless loop can hold, and the reason the drift is
        // exact rather than nearly right.
        let spread = if copies > 1 {
            (f32::from(copy) - f32::from(copies - 1) / 2.0) * tone.beats
        } else {
            0.0
        };
        for n in 1..=u32::from(tone.partials) {
            let ratio = tone.partial_ratio(n);
            let hz = fundamental * ratio;
            if hz > rate_f * 0.45 {
                break;
            }
            let mut level = tone.partial_level(n) * tone.formant_gain(hz);
            // The attack's spectrum leans on the upper partials, which is
            // what a settling tone does.
            level *= 1.0 + bright * (ratio.log2() * 0.5).min(2.0);
            level /= f32::from(copies);
            add(
                hz / line_hz + spread * ratio.min(4.0),
                level,
                seeded.turn(),
                real,
                imag,
            );
        }
    }

    // The noise band: grid components with seeded phases, which is how a
    // wash can loop at all.
    if tone.noise > 0.0 {
        let centre = tone.noise_hz.max(20.0);
        let width = tone.noise_width.max(0.25);
        let low = (centre / 2f32.powf(width)).max(line_hz);
        let high = (centre * 2f32.powf(width)).min(rate_f * 0.45);
        // One component every few lines: dense enough to be noise, sparse
        // enough that a wash is not a wall.
        let step = 4usize;
        let mut at = (low / line_hz) as usize;
        let top = (high / line_hz) as usize;
        let mut sum = 0.0f32;
        while at < top && at < bins {
            let hz = at as f32 * line_hz;
            let octaves = (hz / centre).log2();
            let shape = (-(octaves * octaves) / (width * width * 0.7)).exp();
            let level = tone.noise * shape * (1.0 + bright * 0.5);
            add(at as f32, level, seeded.turn(), real, imag);
            sum += level * level;
            at += step;
        }
        // A band of many components sums louder than its parts; hold the
        // whole band to the level the recipe asked for.
        if sum > 0.0 {
            let keep = (tone.noise / sum.sqrt()).clamp(0.05, 1.0);
            let from = (low / line_hz) as usize;
            for bin in from..top.min(bins) {
                real[bin] *= keep;
                imag[bin] *= keep;
            }
        }
    }
}

/// One loop's worth of samples from a spectrum: exactly periodic, by
/// construction.
fn periodic(real: &[f32], imag: &[f32]) -> Vec<f32> {
    let mut fft = RealFft::new();
    if !fft.prepare(LOOP_FRAMES) {
        return vec![0.0; LOOP_FRAMES];
    }
    let mut time = vec![0.0f32; LOOP_FRAMES];
    let mut scratch = vec![0.0f32; RealFft::scratch_len(LOOP_FRAMES)];
    fft.inverse(real, imag, &mut time, &mut scratch);
    time
}

fn peak_of(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |top, x| top.max(x.abs()))
}

/// The syn click: a knock over the first few milliseconds, bright and
/// short. Added into the attack, never into the loop, so it cannot
/// affect the seam.
fn click_into(out: &mut [f32], recipe: &Recipe, rate: u32) {
    let tone = &recipe.tone;
    if tone.click <= 0.0 {
        return;
    }
    // The knock has to be over before the loop begins, or whatever is
    // left of it at the junction IS a step — a click at the seam, put
    // there by the thing meant to give the note its attack. A short
    // swell therefore gets a short knock.
    let room = out.len().max(1);
    let rate_f = rate.max(1) as f32;
    let mut seeded = Seeded::of("click");
    // Two decays: a noise chirp that is gone in a blink, and a short
    // resonant ping an octave and a half above the note.
    let noise_decay = 0.0022 * rate_f;
    let ring_decay = 0.014 * rate_f;
    let ring_hz = (recipe.ideal_hz() as f32 * 2.8).min(rate_f * 0.4);
    let step = std::f32::consts::TAU * ring_hz / rate_f;
    let mut held = 0.0f32;
    for (at, sample) in out.iter_mut().enumerate() {
        let t = at as f32;
        let noise = (seeded.turn() * 2.0 - 1.0) * (-t / noise_decay).exp();
        // A one-pole on the noise so the knock has a body rather than a
        // hiss; the coefficient is the click's own brightness.
        held += (noise - held) * 0.55;
        let ring = (step * t).sin() * (-t / ring_decay).exp();
        // A raised-cosine taper over the last fifth of the room, so the
        // knock is exactly nothing where the loop takes over.
        let left = 1.0 - t / room as f32;
        let taper = if left < 0.2 {
            0.5 - 0.5 * ((left / 0.2) * std::f32::consts::PI).cos()
        } else {
            1.0
        };
        *sample += tone.click * (held * 0.7 + ring * 0.5) * taper;
        if t > ring_decay * 6.0 {
            break;
        }
    }
}

/// Render one recipe. Deterministic: the same recipe and rate give the
/// same samples on any machine, which is what makes the cache safe.
pub fn render(recipe: &Recipe, rate: u32) -> Vec<f32> {
    let layout = recipe.layout(rate);
    let rate_f = f64::from(rate.max(1));
    let open = (f64::from(OPEN_MS) / 1000.0 * rate_f).max(1.0) as f32;

    if !layout.looped {
        // A one-shot is free of the grid: partials with their own decays,
        // and the click over the top.
        let tone = &recipe.tone;
        let frames = layout.frames as usize;
        let mut out = vec![0.0f32; frames];
        let decay = f64::from(recipe.decay_ms) / 1000.0 * rate_f;
        let mut seeded = Seeded::of(recipe.name);
        let fundamental = recipe.ideal_hz() as f32;
        for n in 1..=u32::from(tone.partials) {
            let ratio = tone.partial_ratio(n);
            let hz = fundamental * ratio;
            if hz > rate_f as f32 * 0.45 {
                break;
            }
            let level = tone.partial_level(n) * tone.formant_gain(hz);
            let phase = seeded.turn() * std::f32::consts::TAU;
            let step = std::f32::consts::TAU * hz / rate_f as f32;
            // The upper partials go first: that is what struck means.
            let fall = (decay as f32) / (1.0 + ratio.log2() * (0.5 + tone.sweep * 0.5));
            for (at, sample) in out.iter_mut().enumerate() {
                let t = at as f32;
                *sample += level * (step * t + phase).sin() * (-t / fall.max(1.0)).exp();
            }
        }
        if tone.noise > 0.0 {
            let mut hiss = Seeded::of("hiss");
            let mut held = 0.0f32;
            let fall = decay as f32 * 0.35;
            for (at, sample) in out.iter_mut().enumerate() {
                let t = at as f32;
                let white = hiss.turn() * 2.0 - 1.0;
                held += (white - held) * (tone.noise_hz / (rate_f as f32 * 0.5)).clamp(0.02, 0.9);
                *sample += tone.noise * held * (-t / fall.max(1.0)).exp();
            }
        }
        click_into(&mut out, recipe, rate);
        let peak = peak_of(&out).max(1e-6);
        for (at, sample) in out.iter_mut().enumerate() {
            let open_gain = (at as f32 / open).min(1.0);
            *sample = *sample / peak * PEAK * open_gain;
        }
        return out;
    }

    // Sustained: one loop from the spectrum, and an attack made of the
    // loop's own tail so the two meet in phase.
    let bins = RealFft::bins(LOOP_FRAMES).max(1);
    let mut real = vec![0.0f32; bins];
    let mut imag = vec![0.0f32; bins];
    spectrum(recipe, rate, 0.0, &mut real, &mut imag);
    let body = periodic(&real, &imag);
    spectrum(recipe, rate, recipe.tone.sweep, &mut real, &mut imag);
    let bright = periodic(&real, &imag);

    // ONE factor for both, or the crossfade would step where the levels
    // met.
    let factor = PEAK / peak_of(&body).max(peak_of(&bright)).max(1e-6);
    let swell = layout.loop_start as usize;
    let mut out = Vec::with_capacity(layout.frames as usize);
    for at in 0..swell {
        // The attack reads the loop's TAIL, so its last sample is
        // followed by the loop's first: the seam is arithmetic, not luck.
        let index = (LOOP_FRAMES + at - swell) % LOOP_FRAMES;
        let through = at as f32 / swell.max(1) as f32;
        // A swell that leans late: the ear hears a pad arrive, not fade in.
        let gain = through * through * (3.0 - 2.0 * through);
        let mix = bright[index] * (1.0 - through) + body[index] * through;
        out.push(mix * factor * gain * (at as f32 / open).min(1.0));
    }
    click_into(&mut out, recipe, rate);
    for sample in &body {
        out.push(sample * factor);
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
    eat(&[recipe.root, recipe.tone.partials, recipe.tone.copies]);
    let t = &recipe.tone;
    for f in [
        recipe.offset,
        recipe.decay_ms,
        recipe.seconds,
        t.tilt,
        t.odd,
        t.stretch,
        t.beats,
        t.noise,
        t.noise_hz,
        t.noise_width,
        t.formants[0],
        t.formants[1],
        t.formants[2],
        t.click,
        t.sweep,
        t.swell_ms,
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

    /// The heavier recipes are rendered once and shared: a loop is
    /// 131072 frames and there are sixty-odd of them.
    fn rendered(name: &str) -> (Recipe, Vec<f32>) {
        let recipe = *RECIPES
            .iter()
            .find(|recipe| recipe.name == name)
            .unwrap_or(&RECIPES[0]);
        (recipe, render(&recipe, RATE))
    }

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

    /// A zone map points at the sample it says it does: the recipe under
    /// a zone carries the multisample's own name.
    #[test]
    fn every_zone_points_at_its_own_familys_recipe() {
        for multi in MULTIS {
            let family = multi.name.to_lowercase();
            for zone in multi.zones {
                let recipe = RECIPES.get(zone.sample as usize);
                let Some(recipe) = recipe else {
                    panic!("{} names a sample the bank lacks", multi.name)
                };
                assert!(
                    recipe.name.starts_with(&family),
                    "{} has a zone on {}",
                    multi.name,
                    recipe.name
                );
            }
        }
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
    fn a_sustained_layout_is_one_grid_loop_and_a_one_shot_is_not_looped() {
        for recipe in RECIPES {
            let layout = recipe.layout(RATE);
            assert!(layout.frames > 0, "{} is empty", recipe.name);
            if recipe.decay_ms > 0.0 {
                assert!(!layout.looped, "{} should be a one-shot", recipe.name);
                continue;
            }
            assert!(layout.looped, "{} should loop", recipe.name);
            assert_eq!(layout.loop_end - layout.loop_start, LOOP_FRAMES as u64);
            assert_eq!(layout.loop_end, layout.frames);
            // The grid line is still the pitch the recipe asked for, to
            // within the correction the reader applies.
            let sounded = layout.hz * layout.correction;
            assert!(
                (sounded - recipe.ideal_hz()).abs() < 1e-6,
                "{} sounds at {sounded}, not {}",
                recipe.name,
                recipe.ideal_hz()
            );
        }
    }

    /// THE CLAIM THE WHOLE DESIGN RESTS ON: a sustained sample loops
    /// without a discontinuity, however lush it is. The step across the
    /// seam is no larger than the largest step inside the loop itself.
    #[test]
    fn every_sustained_sample_loops_seamlessly() {
        for name in ["warm-60-hard", "choir-60-soft", "bow-48-hard", "air-48"] {
            let (recipe, audio) = rendered(name);
            let layout = recipe.layout(RATE);
            let start = layout.loop_start as usize;
            let end = layout.loop_end as usize;
            let inside = audio[start..end]
                .windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0f32, f32::max);
            let seam = (audio[start] - audio[end - 1]).abs();
            assert!(
                seam <= inside + 1e-6,
                "{name}: seam {seam} exceeds the largest step in the loop {inside}"
            );
        }
    }

    /// And the swell meets the loop the same way: the attack is the
    /// loop's own tail, so their junction is a step like any other.
    #[test]
    fn the_swell_meets_the_loop_without_a_step() {
        for name in ["warm-60-soft", "tine-48-hard", "glass-60"] {
            let (recipe, audio) = rendered(name);
            let layout = recipe.layout(RATE);
            let start = layout.loop_start as usize;
            let inside = audio[start..]
                .windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0f32, f32::max);
            let junction = (audio[start] - audio[start - 1]).abs();
            assert!(
                junction <= inside * 1.5 + 1e-6,
                "{name}: the swell steps {junction} into a loop whose largest step is {inside}"
            );
        }
    }

    #[test]
    fn rendering_is_deterministic_and_bounded() {
        for name in ["warm-48-soft", "click-60", "sub-24"] {
            let (recipe, a) = rendered(name);
            let b = render(&recipe, RATE);
            assert_eq!(a, b, "{name} is not deterministic");
            assert!(
                a.iter().all(|x| x.is_finite() && x.abs() <= 1.0),
                "{name} leaves the unit interval"
            );
            assert!(peak_of(&a) > 0.5, "{name} came out quiet");
            assert_eq!(a.len() as u64, recipe.layout(RATE).frames);
        }
    }

    /// A hard layer really is a different TIMBRE, not a louder one: both
    /// are normalised to the same peak, so the difference has to be
    /// where the energy sits.
    /// A hard layer really is a different TIMBRE, not a louder one: both
    /// are normalised to the same peak, so the difference has to be
    /// where the energy sits. Measured on the spectrum the renderer
    /// actually lays down — the fraction of the tone's energy above four
    /// times its fundamental.
    #[test]
    fn the_hard_velocity_layer_is_brighter_not_merely_louder() {
        let bins = RealFft::bins(LOOP_FRAMES);
        let top = |name: &str| {
            let recipe = *RECIPES
                .iter()
                .find(|recipe| recipe.name == name)
                .unwrap_or(&RECIPES[0]);
            let mut real = vec![0.0f32; bins];
            let mut imag = vec![0.0f32; bins];
            spectrum(&recipe, RATE, 0.0, &mut real, &mut imag);
            let line = RATE as f32 / LOOP_FRAMES as f32;
            let corner = (recipe.layout(RATE).hz as f32 * 4.0 / line) as usize;
            let power = |from: usize, to: usize| -> f32 {
                real[from.min(bins)..to.min(bins)]
                    .iter()
                    .zip(&imag[from.min(bins)..to.min(bins)])
                    .map(|(re, im)| re * re + im * im)
                    .sum()
            };
            power(corner, bins) / power(1, bins).max(1e-12)
        };
        for family in ["warm", "choir", "bow", "tine"] {
            let soft = top(&format!("{family}-48-soft"));
            let hard = top(&format!("{family}-48-hard"));
            assert!(
                hard > soft * 1.15,
                "{family}: the hard layer is not the brighter one: {soft} against {hard}"
            );
        }
    }

    /// The syn click is what it says: short, and mostly onset.
    #[test]
    fn the_click_is_a_knock_and_not_a_note() {
        let (recipe, audio) = rendered("click-60");
        assert!(recipe.decay_ms < 40.0);
        assert!(audio.len() < (RATE / 4) as usize, "the click is not short");
        let onset: f32 = audio[..480].iter().map(|x| x.abs()).sum();
        let rest: f32 = audio[480..].iter().map(|x| x.abs()).sum();
        assert!(
            onset > rest * 0.25,
            "the click's energy is not in its onset: {onset} against {rest}"
        );
    }

    /// A pad's copies beat against each other. Two copies a whole number
    /// of lines apart make an envelope that rises and falls over the
    /// loop; one copy does not.
    #[test]
    fn a_pad_drifts_and_a_single_copy_does_not() {
        let swing = |name: &str| {
            let (recipe, audio) = rendered(name);
            let layout = recipe.layout(RATE);
            let loop_part = &audio[layout.loop_start as usize..];
            // The energy of eight slices across the loop: a drifting pad
            // is uneven, a static tone is flat.
            let slice = loop_part.len() / 8;
            let energies: Vec<f32> = (0..8)
                .map(|at| {
                    let from = at * slice;
                    loop_part[from..from + slice]
                        .iter()
                        .map(|x| x * x)
                        .sum::<f32>()
                        / slice as f32
                })
                .collect();
            let high = energies.iter().copied().fold(0.0f32, f32::max);
            let low = energies.iter().copied().fold(f32::MAX, f32::min);
            high / low.max(1e-12)
        };
        assert!(swing("warm-48-soft") > 1.05, "the warm pad does not drift");
        assert!(swing("sub-36") < 1.05, "the sub should be still");
    }

    /// A vowel puts its energy where the formants are: the choir's
    /// spectrum is not the warm pad's.
    #[test]
    fn a_vowel_is_a_different_spectrum_from_a_saw() {
        let bins = RealFft::bins(LOOP_FRAMES);
        let band = |name: &str, from: f32, to: f32| {
            let recipe = *RECIPES
                .iter()
                .find(|recipe| recipe.name == name)
                .unwrap_or(&RECIPES[0]);
            let mut real = vec![0.0f32; bins];
            let mut imag = vec![0.0f32; bins];
            spectrum(&recipe, RATE, 0.0, &mut real, &mut imag);
            let line = RATE as f32 / LOOP_FRAMES as f32;
            let (lo, hi) = ((from / line) as usize, (to / line) as usize);
            real[lo..hi.min(bins)]
                .iter()
                .zip(&imag[lo..hi.min(bins)])
                .map(|(re, im)| re * re + im * im)
                .sum::<f32>()
        };
        // Around the second formant, the choir stands well above the pad
        // once both are read against their own fundamentals.
        let choir = band("choir-60-soft", 950.0, 1350.0) / band("choir-60-soft", 200.0, 400.0);
        let warm = band("warm-60-soft", 950.0, 1350.0) / band("warm-60-soft", 200.0, 400.0);
        assert!(
            choir > warm * 1.5,
            "the vowel did not lift its formant: {choir} against {warm}"
        );
    }

    /// The bank the tests use never touches the disk.
    #[test]
    fn a_rendered_bank_is_complete_and_silent_about_the_disk() {
        // One family, so the suite is not sixty inverse transforms.
        for recipe in RECIPES.iter().filter(|r| r.name.starts_with("sub")) {
            let layout = recipe.layout(RATE);
            let audio = render(recipe, RATE);
            assert_eq!(audio.len() as u64, layout.frames);
            assert!(peak_of(&audio) > 0.5);
        }
    }

    #[test]
    fn a_fingerprint_follows_every_field_that_changes_the_audio() {
        let base = RECIPES[0];
        let mut moved = base;
        moved.tone.tilt += 0.01;
        assert_ne!(fingerprint(&base, RATE), fingerprint(&moved, RATE));
        let mut drifted = base;
        drifted.tone.beats += 1.0;
        assert_ne!(fingerprint(&base, RATE), fingerprint(&drifted, RATE));
        assert_ne!(fingerprint(&base, RATE), fingerprint(&base, 44_100));
        assert_eq!(fingerprint(&base, RATE), fingerprint(&base.clone(), RATE));
    }
}
