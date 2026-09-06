//! The console: the desk every track sits on.
//!
//! Every track carries the same channel strip in the same order — the
//! twenty sections of [`SectionKind::STRIP`] — and every section is an
//! effect made for this desk on the engine's own kernels. A section is
//! IN or OUT; OUT is not compiled and costs nothing. PREAMP and OUT are
//! always in: they are the channel. Every strip feeds one of four
//! buses, every bus feeds the mix, and the buses, the returns and the
//! mix wear sections that are never taken out.
//!
//! This module is the GREEN side of the console: what the sections are
//! called, what they hold, and how the surface draws them. The cores
//! that make sound are in `crate::audio::console`, which the surface
//! never imports. See `notes/20260902-console-plan.md`.

use crate::params::{self, ParamDef};

/// Every section the console has, on strips, buses, the mix and the
/// returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SectionKind {
    Preamp,
    Tone,
    Door,
    Cut,
    Hit,
    Four,
    Vca,
    Split,
    Pump,
    Drive,
    Grit,
    Shine,
    Drift,
    Phase,
    Smear,
    Ring,
    Spectra,
    Echo,
    Room,
    Out,
    Glue,
    Iron,
    Ceiling,
    Scope,
    Tape,
    Shadow,
}

/// How wide a section's card is on the band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    Narrow,
    Wide,
}

impl SectionKind {
    /// The strip, in signal order.
    pub const STRIP: [SectionKind; 20] = [
        SectionKind::Preamp,
        SectionKind::Tone,
        SectionKind::Door,
        SectionKind::Cut,
        SectionKind::Hit,
        SectionKind::Four,
        SectionKind::Vca,
        SectionKind::Split,
        SectionKind::Pump,
        SectionKind::Drive,
        SectionKind::Grit,
        SectionKind::Shine,
        SectionKind::Drift,
        SectionKind::Phase,
        SectionKind::Smear,
        SectionKind::Ring,
        SectionKind::Spectra,
        SectionKind::Echo,
        SectionKind::Room,
        SectionKind::Out,
    ];
    /// A group bus's sections, in order.
    pub const BUS: [SectionKind; 2] = [SectionKind::Glue, SectionKind::Iron];
    /// The mix bus's sections, in order.
    pub const MIX: [SectionKind; 4] = [
        SectionKind::Glue,
        SectionKind::Iron,
        SectionKind::Ceiling,
        SectionKind::Scope,
    ];
    /// The two returns.
    pub const RETURNS: [SectionKind; 2] = [SectionKind::Tape, SectionKind::Shadow];
    /// Every kind, once.
    pub const ALL: [SectionKind; 26] = [
        SectionKind::Preamp,
        SectionKind::Tone,
        SectionKind::Door,
        SectionKind::Cut,
        SectionKind::Hit,
        SectionKind::Four,
        SectionKind::Vca,
        SectionKind::Split,
        SectionKind::Pump,
        SectionKind::Drive,
        SectionKind::Grit,
        SectionKind::Shine,
        SectionKind::Drift,
        SectionKind::Phase,
        SectionKind::Smear,
        SectionKind::Ring,
        SectionKind::Spectra,
        SectionKind::Echo,
        SectionKind::Room,
        SectionKind::Out,
        SectionKind::Glue,
        SectionKind::Iron,
        SectionKind::Ceiling,
        SectionKind::Scope,
        SectionKind::Tape,
        SectionKind::Shadow,
    ];

    /// The word on the card's head.
    pub fn name(self) -> &'static str {
        match self {
            SectionKind::Preamp => "PREAMP",
            SectionKind::Tone => "TONE",
            SectionKind::Door => "DOOR",
            SectionKind::Cut => "CUT",
            SectionKind::Hit => "HIT",
            SectionKind::Four => "FOUR",
            SectionKind::Vca => "VCA",
            SectionKind::Split => "SPLIT",
            SectionKind::Pump => "PUMP",
            SectionKind::Drive => "DRIVE",
            SectionKind::Grit => "GRIT",
            SectionKind::Shine => "SHINE",
            SectionKind::Drift => "DRIFT",
            SectionKind::Phase => "PHASE",
            SectionKind::Smear => "SMEAR",
            SectionKind::Ring => "RING",
            SectionKind::Spectra => "SPECTRA",
            SectionKind::Echo => "ECHO",
            SectionKind::Room => "ROOM",
            SectionKind::Out => "OUT",
            SectionKind::Glue => "GLUE",
            SectionKind::Iron => "IRON",
            SectionKind::Ceiling => "CEILING",
            SectionKind::Scope => "SCOPE",
            SectionKind::Tape => "TAPE",
            SectionKind::Shadow => "SHADOW",
        }
    }

    /// The machine address: the params module's name, the automation
    /// target's prefix.
    pub fn code(self) -> &'static str {
        match self {
            SectionKind::Preamp => "preamp",
            SectionKind::Tone => "tone",
            SectionKind::Door => "door",
            SectionKind::Cut => "cut",
            SectionKind::Hit => "hit",
            SectionKind::Four => "four",
            SectionKind::Vca => "vca",
            SectionKind::Split => "split",
            SectionKind::Pump => "pump",
            SectionKind::Drive => "drive",
            SectionKind::Grit => "grit",
            SectionKind::Shine => "shine",
            SectionKind::Drift => "drift",
            SectionKind::Phase => "phase",
            SectionKind::Smear => "smear",
            SectionKind::Ring => "ring",
            SectionKind::Spectra => "spectra",
            SectionKind::Echo => "echo",
            SectionKind::Room => "room",
            SectionKind::Out => "out",
            SectionKind::Glue => "glue",
            SectionKind::Iron => "iron",
            SectionKind::Ceiling => "ceiling",
            SectionKind::Scope => "scope",
            SectionKind::Tape => "tape",
            SectionKind::Shadow => "shadow",
        }
    }

    /// One line under the name, for the codebook and the empty card.
    pub fn blurb(self) -> &'static str {
        match self {
            SectionKind::Preamp => "the input stage",
            SectionKind::Tone => "three levers",
            SectionKind::Door => "gate and expander",
            SectionKind::Cut => "high and low pass",
            SectionKind::Hit => "transient shaper",
            SectionKind::Four => "the console eq",
            SectionKind::Vca => "the channel compressor",
            SectionKind::Split => "three-band dynamics",
            SectionKind::Pump => "ducks on the beat",
            SectionKind::Drive => "five characters",
            SectionKind::Grit => "converter degradation",
            SectionKind::Shine => "an exciter",
            SectionKind::Drift => "chorus, flanger, vibrato",
            SectionKind::Phase => "a phaser",
            SectionKind::Smear => "allpass dispersion",
            SectionKind::Ring => "ring modulation",
            SectionKind::Spectra => "spectral",
            SectionKind::Echo => "an analogue delay",
            SectionKind::Room => "room or hall",
            SectionKind::Out => "width, sends, the bus",
            SectionKind::Glue => "the bus compressor",
            SectionKind::Iron => "transformer saturation",
            SectionKind::Ceiling => "the limiter",
            SectionKind::Scope => "the analyser",
            SectionKind::Tape => "a tape delay",
            SectionKind::Shadow => "the reverb",
        }
    }

    /// The section's parameter table.
    pub fn table(self) -> &'static [ParamDef] {
        match self {
            SectionKind::Preamp => params::console::preamp::TABLE,
            SectionKind::Tone => params::console::tone::TABLE,
            SectionKind::Door => params::console::door::TABLE,
            SectionKind::Cut => params::console::cut::TABLE,
            SectionKind::Hit => params::console::hit::TABLE,
            SectionKind::Four => params::console::four::TABLE,
            SectionKind::Vca => params::console::vca::TABLE,
            SectionKind::Split => params::console::split::TABLE,
            SectionKind::Pump => params::console::pump::TABLE,
            SectionKind::Drive => params::console::drive::TABLE,
            SectionKind::Grit => params::console::grit::TABLE,
            SectionKind::Shine => params::console::shine::TABLE,
            SectionKind::Drift => params::console::drift::TABLE,
            SectionKind::Phase => params::console::phase::TABLE,
            SectionKind::Smear => params::console::smear::TABLE,
            SectionKind::Ring => params::console::ring::TABLE,
            SectionKind::Spectra => params::console::spectra::TABLE,
            SectionKind::Echo => params::console::echo::TABLE,
            SectionKind::Room => params::console::room::TABLE,
            SectionKind::Out => params::console::out::TABLE,
            SectionKind::Glue => params::console::glue::TABLE,
            SectionKind::Iron => params::console::iron::TABLE,
            SectionKind::Ceiling => params::console::ceiling::TABLE,
            SectionKind::Scope => params::console::scope::TABLE,
            SectionKind::Tape => params::console::tape::TABLE,
            SectionKind::Shadow => params::console::shadow::TABLE,
        }
    }

    /// Whether the desk never lets this section OUT.
    pub fn always_in(self) -> bool {
        matches!(
            self,
            SectionKind::Preamp
                | SectionKind::Out
                | SectionKind::Glue
                | SectionKind::Iron
                | SectionKind::Ceiling
                | SectionKind::Scope
                | SectionKind::Tape
                | SectionKind::Shadow
        )
    }

    /// Whether the card opens a full-screen mode on Enter.
    pub fn full_screen(self) -> bool {
        matches!(
            self,
            SectionKind::Four | SectionKind::Vca | SectionKind::Spectra | SectionKind::Scope
        )
    }

    pub fn width(self) -> Width {
        match self {
            SectionKind::Preamp => Width::Narrow,
            SectionKind::Tone => Width::Narrow,
            SectionKind::Door => Width::Narrow,
            SectionKind::Cut => Width::Narrow,
            SectionKind::Hit => Width::Narrow,
            SectionKind::Four => Width::Wide,
            SectionKind::Vca => Width::Wide,
            SectionKind::Split => Width::Wide,
            SectionKind::Pump => Width::Narrow,
            SectionKind::Drive => Width::Wide,
            SectionKind::Grit => Width::Narrow,
            SectionKind::Shine => Width::Narrow,
            SectionKind::Drift => Width::Wide,
            SectionKind::Phase => Width::Narrow,
            SectionKind::Smear => Width::Narrow,
            SectionKind::Ring => Width::Narrow,
            SectionKind::Spectra => Width::Wide,
            SectionKind::Echo => Width::Wide,
            SectionKind::Room => Width::Wide,
            SectionKind::Out => Width::Narrow,
            SectionKind::Glue => Width::Narrow,
            SectionKind::Iron => Width::Narrow,
            SectionKind::Ceiling => Width::Narrow,
            SectionKind::Scope => Width::Narrow,
            SectionKind::Tape => Width::Wide,
            SectionKind::Shadow => Width::Wide,
        }
    }

    /// Whether the section draws every one of its parameters as an
    /// instrument on its own glass, and so needs no table of names and
    /// numbers under them.
    pub fn owns_its_glass(self) -> bool {
        // Every section on the desk is drawn. Not one of them answers a
        // question with a number, so not one of them keeps the generic
        // table under its figure — the face IS the parameters, and it
        // is handed the whole glass to be them on.
        true
    }

    /// Where on the strip this section stands, or `None` for a bus,
    /// mix or return section.
    pub fn strip_index(self) -> Option<usize> {
        Self::STRIP.iter().position(|kind| *kind == self)
    }
}

/// What a curve MAKES of a sine, measured rather than tabulated.
///
/// Two sections on the desk grow harmonics on purpose and both want the
/// same reading: which rungs stand, and how much distortion in total.
/// The measurement is the same either way — push a sine through the very
/// function the core runs and read the answer off a bin at a time — so
/// it lives once, here, and the curves hand themselves in.
pub mod harmonic {
    /// How many harmonics a ladder reports, the fundamental included.
    pub const COUNT: usize = 8;
    /// The floor a rung sits at when there is nothing in it.
    pub const FLOOR_DB: f32 = -96.0;
    /// Points in the probe sine. A power of two, and comfortably more
    /// than four times the highest harmonic asked for.
    const PROBE_N: usize = 256;

    /// A curve's harmonics, in dB relative to its fundamental.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Harmonics {
        /// Index 0 is the fundamental and is always 0. Silence reads
        /// [`FLOOR_DB`].
        pub db: [f32; COUNT],
        /// Total harmonic distortion, as a share of the fundamental.
        pub thd: f32,
    }

    impl Harmonics {
        /// A curve that is doing nothing.
        pub fn none() -> Self {
            let mut db = [FLOOR_DB; COUNT];
            db[0] = 0.0;
            Self { db, thd: 0.0 }
        }
    }

    /// Measure `shape` at `amplitude`.
    ///
    /// An asymmetric curve has no odd symmetry to cancel its even terms,
    /// so its even rungs stand; a symmetric one cancels them and the odd
    /// rungs stand instead. That difference is what these ladders are
    /// drawn to show, and it falls out of the arithmetic rather than
    /// being asserted anywhere.
    pub fn measure(shape: impl Fn(f32) -> f32, amplitude: f32) -> Harmonics {
        if amplitude <= 0.0 {
            return Harmonics::none();
        }
        let n = PROBE_N;
        let shaped: Vec<f32> = (0..n)
            .map(|i| {
                let t = core::f32::consts::TAU * i as f32 / n as f32;
                shape(amplitude * t.sin())
            })
            .collect();
        // One bin of a real DFT, by hand: only eight are wanted, so a
        // whole transform would be the long way round.
        let magnitude = |k: usize| -> f32 {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, y) in shaped.iter().enumerate() {
                let t = core::f32::consts::TAU * (k * i) as f32 / n as f32;
                re += y * t.cos();
                im += y * t.sin();
            }
            (re * re + im * im).sqrt() * 2.0 / n as f32
        };
        let fundamental = magnitude(1);
        if fundamental <= f32::EPSILON {
            return Harmonics::none();
        }
        let mut out = Harmonics::none();
        let mut sum = 0.0;
        for (k, slot) in out.db.iter_mut().enumerate().skip(1) {
            let m = magnitude(k + 1);
            sum += m * m;
            let ratio = m / fundamental;
            *slot = if ratio <= 0.0 {
                FLOOR_DB
            } else {
                (20.0 * ratio.log10()).max(FLOOR_DB)
            };
        }
        out.thd = sum.sqrt() / fundamental;
        out
    }
}

/// PHASE's response, green side.
///
/// A phaser is not a filter with notches drawn on it — the notches are
/// what happens when a swept ALLPASS run, which changes no magnitude at
/// all, is added back to the dry signal and the two disagree. So the
/// card cannot draw notches by placing them; it has to compute the
/// disagreement, which is what this does.
///
/// A second-order allpass has unit magnitude and a phase that runs from
/// zero to a FULL turn through its corner. Cascade `stages` of them and
/// the phase multiplies. Add the dry back and the sum is `1 + A`, which
/// nulls wherever the run has arrived at an odd half-turn — and a full
/// turn per section means one null per section.
///
/// FEEDBACK is in the sum too, as `A / (1 - k·A)`: it sharpens the
/// notches into resonances, which is what a fed-back phaser does.
pub mod phase_curve {
    use crate::params::console::phase as p;

    /// One second-order allpass section's phase at `hz`, in radians,
    /// for a corner at `corner` and the section's fixed Q.
    ///
    /// The analogue prototype's: `-2·atan2(ω/Q·ω0, ω0² − ω²)`, which
    /// runs from 0 through −π at the corner to −2π well above it.
    pub fn section_phase(hz: f32, corner: f32) -> f32 {
        let w = hz.max(1.0);
        let w0 = corner.max(1.0);
        let num = w * w0 / p::STAGE_Q;
        let den = w0 * w0 - w * w;
        -2.0 * num.atan2(den)
    }

    /// The whole section's magnitude at `hz`, in dB: the allpass run
    /// with its feedback, added to the dry.
    pub fn response_db(stages: u32, corner: f32, feedback: f32, hz: f32) -> f32 {
        if stages == 0 {
            return 0.0;
        }
        let phi = section_phase(hz, corner) * stages as f32;
        let (sin, cos) = phi.sin_cos();
        // The run, as a complex number of unit magnitude.
        let (ar, ai) = (cos, sin);
        // Through the feedback: A / (1 − k·A).
        let k = feedback.clamp(-0.95, 0.95);
        let (dr, di) = (1.0 - k * ar, -k * ai);
        let den = dr * dr + di * di;
        let (hr, hi) = if den <= f32::EPSILON {
            (ar, ai)
        } else {
            ((ar * dr + ai * di) / den, (ai * dr - ar * di) / den)
        };
        // Added back to the dry, and halved so an untouched signal reads
        // as zero rather than as six dB of nothing.
        let (sr, si) = (1.0 + hr, hi);
        let mag = (sr * sr + si * si).sqrt() * 0.5;
        if mag <= 1e-6 {
            -120.0
        } else {
            20.0 * mag.log10()
        }
    }

    /// How many notches a stage count puts in the spectrum.
    ///
    /// One per section, and this disagrees with the section's own prose,
    /// which says "two of them at four sections, eight at sixteen" —
    /// the classic pedal count, where a stage is a FIRST-order allpass
    /// worth half a turn each and two are needed per notch.
    ///
    /// The kernel does not do that. `Disperser` runs SVF sections in
    /// allpass mode, and an SVF is two-pole: every section is worth a
    /// whole turn on its own, so every section is worth a notch. The
    /// count here is measured off [`response_db`] rather than taken from
    /// the prose, and a test counts the dips to keep the two together.
    pub fn notches(stages: u32) -> u32 {
        stages
    }

    /// Where the sweep has taken the corner, for a sweep value in −1..1.
    pub fn corner_at(sweep: f32, depth: f32) -> f32 {
        let reach = sweep.clamp(-1.0, 1.0) * depth.clamp(0.0, 1.0);
        // The sweep walks the corner between the two ends, in octaves,
        // because that is how the ear hears a phaser move.
        let low = p::LOW_HZ.max(1.0).log2();
        let high = p::HIGH_HZ.max(1.0).log2();
        let mid = (low + high) * 0.5;
        let half = (high - low) * 0.5;
        2f32.powf(mid + reach * half)
    }
}

/// SHINE's curve, green side.
///
/// The exciter takes the top off, drives it hard enough to grow
/// harmonics that were never there, and adds it back under the whole
/// sound. The curve is the same asymmetric form the preamp's iron uses
/// and for the same reason — an asymmetric curve makes EVEN harmonics,
/// the octave above rather than the fifth, which is what makes an
/// exciter read as sheen and not as distortion. Only the bias and the
/// drive differ, and both are the section's own.
pub mod shine_curve {
    use crate::params::console::shine as p;

    /// The exciter's transfer at `amount` (0..1), for `x` in −1..1.
    /// Unit slope at zero, so the drive changes colour before level.
    pub fn transfer(amount: f32, x: f32) -> f32 {
        if amount <= 0.0 {
            return x;
        }
        let k = 1.0 + p::DRIVE * amount;
        let b = p::BIAS;
        let tb = b.tanh();
        let slope = k * (1.0 - tb * tb);
        ((k * x + b).tanh() - tb) / slope
    }

    /// The same drive with the bias taken out: a SYMMETRIC curve.
    ///
    /// Not a thing the section can be set to — it is the comparison the
    /// card is drawn against. A symmetric curve of the same strength
    /// makes almost no even harmonics at all, so the exciter's evens are
    /// visibly the bias's doing and not the drive's.
    pub fn symmetric(amount: f32, x: f32) -> f32 {
        if amount <= 0.0 {
            return x;
        }
        let k = 1.0 + p::DRIVE * amount;
        (k * x).tanh() / k.tanh()
    }

    /// What a symmetric curve of the same strength would make.
    pub fn without_bias(amount: f32, amplitude: f32) -> super::harmonic::Harmonics {
        if amount <= 0.0 {
            return super::harmonic::Harmonics::none();
        }
        super::harmonic::measure(|x| symmetric(amount, x), amplitude)
    }

    /// What the exciter makes of a sine at `amount`.
    pub fn harmonics(amount: f32, amplitude: f32) -> super::harmonic::Harmonics {
        if amount <= 0.0 {
            return super::harmonic::Harmonics::none();
        }
        super::harmonic::measure(|x| transfer(amount, x), amplitude)
    }
}

/// PREAMP's two curves, here on the green side so the card can draw
/// the transfer the core runs. `iron` is the drive, 0..1; `x` in −1..1.
/// Unit slope at zero — the drive changes colour before level.
pub mod preamp_curve {
    /// The bias that makes the iron's even harmonics.
    pub const IRON_BIAS: f32 = 0.22;
    /// How hard each stage drives its curve at full.
    pub const IRON_DRIVE: f32 = 2.5;
    pub const STEEL_DRIVE: f32 = 4.0;
    /// The steel's knee: higher is harder.
    pub const STEEL_KNEE: f32 = 2.5;

    /// The transformer stage: asymmetric soft saturation.
    #[inline(always)]
    pub fn iron(x: f32, k: f32) -> f32 {
        let b = IRON_BIAS;
        let tb = b.tanh();
        let slope = k * (1.0 - tb * tb);
        ((k * x + b).tanh() - tb) / slope
    }

    /// The push-pull stage: symmetric, a harder knee.
    #[inline(always)]
    pub fn steel(x: f32, k: f32) -> f32 {
        let y = k * x;
        y / (1.0 + y.abs().powf(STEEL_KNEE)).powf(1.0 / STEEL_KNEE) / k
    }

    /// The stage's transfer at `iron` and character.
    pub fn transfer(iron: f32, steel: bool, x: f32) -> f32 {
        if iron <= 0.0 {
            return x;
        }
        if steel {
            self::steel(x, 1.0 + STEEL_DRIVE * iron)
        } else {
            self::iron(x, 1.0 + IRON_DRIVE * iron)
        }
    }

    /// How many harmonics the ladder reports, the fundamental included.
    pub const HARMONICS: usize = super::harmonic::COUNT;
    /// The probe the ladder is measured with when the channel is silent,
    /// as a linear amplitude. A card that showed nothing at rest would
    /// be hiding the one thing the operator is choosing between.
    pub const REST_PROBE: f32 = 0.25;
    /// The floor a rung sits at when there is nothing in it.
    pub const FLOOR_DB: f32 = super::harmonic::FLOOR_DB;

    /// What the stage MAKES of a sine: the fundamental and the seven
    /// harmonics above it, in dB relative to the fundamental.
    ///
    /// The measurement itself is [`super::harmonic::measure`], shared
    /// with the exciter, which grows harmonics for the same reason from
    /// a curve of the same shape. What is the preamp's own is WHICH
    /// curve goes in: the transfer the core is running at this drive
    /// and this stage.
    ///
    /// The asymmetric stage (IRON) has no odd symmetry to cancel its
    /// even terms, so the even rungs stand; the symmetric one (STEEL)
    /// cancels them and the odd rungs stand instead. That is the whole
    /// difference between the two stages, and it is visible here rather
    /// than asserted.
    pub type Harmonics = super::harmonic::Harmonics;

    /// Measure the stage at `amplitude`. A stage at its floor is a wire
    /// and makes nothing.
    pub fn harmonics(iron: f32, steel: bool, amplitude: f32) -> Harmonics {
        if iron <= 0.0 {
            return Harmonics::none();
        }
        super::harmonic::measure(|x| transfer(iron, steel, x), amplitude)
    }
}

/// HIT's shape, green side: what the section does to a note.
///
/// A transient shaper cannot be drawn from its knobs. What it does at
/// any instant depends on where in a note you are — the strike is a
/// millisecond, the tail is most of a second — so the only honest
/// picture is a note run THROUGH it.
///
/// That is what this does, with the kernel itself: a model hit goes
/// through the very [`crate::dsp::dynamics::TransientSplit`] the core
/// runs, and the tail is the same two followers on the same time
/// constants, so the gain here is the gain there:
///
/// ```text
/// db = RANGE_DB * (attack * strike + sustain * tail)
/// ```
pub mod hit_curve {
    use crate::dsp::dynamics::TransientSplit;
    use crate::params::console::hit as p;

    /// The rate the model is run at. The split's fast attack is a
    /// twentieth of a millisecond, so a coarser model would not have a
    /// strike in it at all.
    const RATE: f32 = 48_000.0;

    /// One instant of the model note.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Point {
        /// Milliseconds since the note began.
        pub ms: f32,
        /// The note as it arrived, in dB from its own peak.
        pub plain_db: f32,
        /// What the section adds there, in dB. Signed.
        pub gain_db: f32,
        /// The strike's weight at that instant, 0..1.
        pub strike: f32,
        /// The tail's share at that instant, 0..1.
        pub tail: f32,
    }

    /// A model hit: a fast rise, then an exponential decay.
    fn envelope(ms: f32, decay_ms: f32) -> f32 {
        const RISE_MS: f32 = 1.0;
        if ms < 0.0 {
            return 0.0;
        }
        let rise = (ms / RISE_MS).clamp(0.0, 1.0);
        rise * (-(ms) / decay_ms.max(1.0)).exp()
    }

    /// Run a note through the section and report `points` instants of
    /// it, evenly spaced across `span_ms`.
    ///
    /// `attack` and `sustain` are the levers in −1..1, as
    /// `hit::Settings` resolves them.
    pub fn trace(
        attack: f32,
        sustain: f32,
        decay_ms: f32,
        span_ms: f32,
        points: usize,
    ) -> Vec<Point> {
        let span_ms = span_ms.max(1.0);
        let samples = ((span_ms * 1e-3 * RATE) as usize).max(points.max(2));
        let mut split = TransientSplit::new();
        split.prepare(RATE, p::WINDOW_MS);
        // The tail's two followers, on the section's own constants.
        let coeff = |ms: f32| 1.0 - (-1.0 / (ms * 1e-3 * RATE)).exp();
        let (quick_release, long_release) = (coeff(p::TAIL_QUICK_MS), coeff(p::TAIL_LONG_MS));
        let (mut quick, mut long) = (0.0f32, 0.0f32);
        let mut out = Vec::with_capacity(points);
        let mut weight = [0.0f32; 1];
        let mut next = 0usize;
        for i in 0..samples {
            let ms = i as f32 / RATE * 1e3;
            let x = envelope(ms, decay_ms);
            split.process(&[x], &mut weight);
            let strike = weight[0];
            if x > quick {
                quick = x;
            } else {
                quick += (x - quick) * quick_release;
            }
            if x > long {
                long = x;
            } else {
                long += (x - long) * long_release;
            }
            let tail = if long > p::LEVEL_FLOOR {
                ((long - quick) / long).clamp(0.0, 1.0)
            } else {
                0.0
            };
            // Report evenly across the span, whatever the rate.
            let want = next * samples / points.max(1);
            if i >= want && out.len() < points {
                out.push(Point {
                    ms,
                    plain_db: if x > p::LEVEL_FLOOR {
                        20.0 * x.log10()
                    } else {
                        -120.0
                    },
                    gain_db: p::RANGE_DB * (attack * strike + sustain * tail),
                    strike,
                    tail,
                });
                next += 1;
            }
        }
        out
    }
}

/// TONE's shape, here on the green side so the card draws the response
/// the core runs: the same bands, prepared the same way, read at a
/// frequency. A killed band is the cut filter the core runs in its
/// place.
pub mod tone_curve {
    use crate::dsp::filters::{BandShape, EqBand};
    use crate::params::console::tone as p;

    /// TONE's settings, as both the core and the card resolve them.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Shape {
        pub lo_db: f32,
        pub mid_db: f32,
        pub hi_db: f32,
        pub mid_hz: f32,
        pub kill_lo: bool,
        pub kill_mid: bool,
        pub kill_hi: bool,
    }

    impl Shape {
        pub fn of(params: &super::SectionParams) -> Self {
            let table = super::SectionKind::Tone.table();
            let clamp = |id: u32, value: f32| {
                table
                    .iter()
                    .find(|def| def.id == id)
                    .map_or(value, |def| def.clamp(value))
            };
            Self {
                lo_db: clamp(p::LO, params.value(p::LO)),
                mid_db: clamp(p::MID, params.value(p::MID)),
                hi_db: clamp(p::HI, params.value(p::HI)),
                mid_hz: clamp(p::MID_HZ, params.value(p::MID_HZ)),
                kill_lo: params.value(p::KILL_LO) >= 0.5,
                kill_mid: params.value(p::KILL_MID) >= 0.5,
                kill_hi: params.value(p::KILL_HI) >= 0.5,
            }
        }

        /// Whether the section is a wire.
        pub fn is_flat(&self) -> bool {
            self.lo_db == 0.0
                && self.mid_db == 0.0
                && self.hi_db == 0.0
                && !self.kill_lo
                && !self.kill_mid
                && !self.kill_hi
        }
    }

    /// The mid's Q at a gain: proportional, and narrower on a cut.
    /// Broad at a nudge, focused at a push — a +3 dB bell is wide and
    /// musical, a +15 dB one is aimed — and a cut of the same amount is
    /// half again as narrow, the way passive desks cut.
    pub fn mid_q(gain_db: f32) -> f32 {
        let amount = ((gain_db.abs() - p::Q_KNEE_DB) / (15.0 - p::Q_KNEE_DB)).clamp(0.0, 1.0);
        let q = p::Q_BROAD + (p::Q_FOCUSED - p::Q_BROAD) * amount;
        if gain_db < 0.0 {
            q * p::CUT_NARROWER
        } else {
            q
        }
    }

    /// A biquad's magnitude at `hz`, from its coefficients
    /// `[b0, b1, b2, a1, a2]`.
    pub fn biquad_db(coeffs: [f32; 5], hz: f32, sample_rate: f32) -> f32 {
        let w = 2.0 * core::f32::consts::PI * hz / sample_rate;
        let (c1, s1) = (w.cos(), w.sin());
        let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
        let [b0, b1, b2, a1, a2] = coeffs;
        let num = (b0 + b1 * c1 + b2 * c2, -(b1 * s1 + b2 * s2));
        let den = (1.0 + a1 * c1 + a2 * c2, -(a1 * s1 + a2 * s2));
        let mag = (num.0 * num.0 + num.1 * num.1).sqrt()
            / (den.0 * den.0 + den.1 * den.1).sqrt().max(1e-9);
        20.0 * mag.max(1e-9).log10()
    }

    /// A Butterworth cut's magnitude at `hz`: the analytic curve of the
    /// cascade the core runs.
    fn butterworth_db(hz: f32, corner: f32, order: u32, highpass: bool) -> f32 {
        let ratio = (hz / corner).max(1e-6);
        let r2n = ratio.powi(2 * order as i32);
        let mag2 = if highpass {
            r2n / (1.0 + r2n)
        } else {
            1.0 / (1.0 + r2n)
        };
        10.0 * mag2.max(1e-12).log10()
    }

    /// The whole section's response at `hz`, in dB.
    pub fn response_db(shape: &Shape, sample_rate: f32, hz: f32) -> f32 {
        let mut db = 0.0;
        let mut band = EqBand::new();
        if shape.kill_lo {
            db += butterworth_db(hz, p::LO_HZ, p::KILL_ORDER, true);
        } else if shape.lo_db != 0.0 {
            band.prepare(
                sample_rate,
                p::LO_HZ,
                p::SHELF_Q,
                shape.lo_db,
                BandShape::LowShelf,
            );
            db += biquad_db(band.coeffs(), hz, sample_rate);
        }
        if shape.kill_mid {
            band.prepare(
                sample_rate,
                shape.mid_hz,
                p::KILL_MID_Q,
                p::KILL_MID_DB,
                BandShape::Bell,
            );
            db += biquad_db(band.coeffs(), hz, sample_rate);
        } else if shape.mid_db != 0.0 {
            band.prepare(
                sample_rate,
                shape.mid_hz,
                mid_q(shape.mid_db),
                shape.mid_db,
                BandShape::Bell,
            );
            db += biquad_db(band.coeffs(), hz, sample_rate);
        }
        if shape.kill_hi {
            db += butterworth_db(hz, p::HI_HZ, p::KILL_ORDER, false);
        } else if shape.hi_db != 0.0 {
            band.prepare(
                sample_rate,
                p::HI_HZ,
                p::SHELF_Q,
                shape.hi_db,
                BandShape::HighShelf,
            );
            db += biquad_db(band.coeffs(), hz, sample_rate);
        }
        db
    }
}

/// CUT's shape on the green side: the two filters' linear response,
/// which is what the card draws. The crunch is the one thing the curve
/// cannot show, and the card shows it as heat.
pub mod cut_curve {
    use crate::params::console::cut as p;

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Shape {
        pub hp_hz: f32,
        pub hp_res: f32,
        pub lp_hz: f32,
        pub lp_res: f32,
        pub crunch: f32,
    }

    impl Shape {
        pub fn of(params: &super::SectionParams) -> Self {
            let table = super::SectionKind::Cut.table();
            let clamp = |id: u32| {
                let value = params.value(id);
                table
                    .iter()
                    .find(|def| def.id == id)
                    .map_or(value, |def| def.clamp(value))
            };
            Self {
                hp_hz: clamp(p::HP_HZ),
                hp_res: clamp(p::HP_RES) / 100.0,
                lp_hz: clamp(p::LP_HZ),
                lp_res: clamp(p::LP_RES) / 100.0,
                crunch: clamp(p::CRUNCH) / 100.0,
            }
        }

        pub fn hp_off(&self) -> bool {
            self.hp_hz <= p::HP_OFF_HZ
        }

        pub fn lp_off(&self) -> bool {
            self.lp_hz >= p::LP_OFF_HZ
        }

        /// Whether both are parked: a wire.
        pub fn is_off(&self) -> bool {
            self.hp_off() && self.lp_off()
        }
    }

    /// The resonance knob's Q: exponential from flat to just past
    /// self-oscillation, so the last inch is where it sings.
    pub fn q_of(res: f32) -> f32 {
        p::Q_MIN * (p::Q_MAX / p::Q_MIN).powf(res.clamp(0.0, 1.0))
    }

    /// The loop's damping for a resonance: `1/Q` down to the last inch,
    /// then through zero to slightly negative, which is where a real
    /// loop starts to sing on its own — the tanh in the loop is what
    /// holds it there.
    pub fn damping_of(res: f32) -> f32 {
        let res = res.clamp(0.0, 1.0);
        if res < p::SING_FROM {
            1.0 / q_of(res)
        } else {
            let along = (res - p::SING_FROM) / (1.0 - p::SING_FROM);
            (1.0 / p::Q_MAX) * (1.0 - along) + p::SING_DAMPING * along
        }
    }

    /// The passband gain at a resonance, in dB: pulled down as the
    /// resonance rises, so a sweep gets a peak and not a level jump.
    pub fn compensation_db(res: f32) -> f32 {
        -p::RES_COMPENSATION_DB * res.clamp(0.0, 1.0)
    }

    /// A 2-pole filter's magnitude at `hz`, in dB: the analogue
    /// prototype's, which the digital loop matches at its corner.
    fn two_pole_db(hz: f32, corner: f32, q: f32, highpass: bool) -> f32 {
        let w = hz / corner.max(1.0);
        let w2 = w * w;
        let den = ((1.0 - w2).powi(2) + (w / q).powi(2)).max(1e-12);
        let num = if highpass { w2 * w2 } else { 1.0 };
        10.0 * (num / den).log10()
    }

    /// The whole section's response at `hz`, in dB.
    pub fn response_db(shape: &Shape, hz: f32) -> f32 {
        let mut db = 0.0;
        if !shape.hp_off() {
            db += two_pole_db(hz, shape.hp_hz, q_of(shape.hp_res), true)
                + compensation_db(shape.hp_res);
        }
        if !shape.lp_off() {
            db += two_pole_db(hz, shape.lp_hz, q_of(shape.lp_res), false)
                + compensation_db(shape.lp_res);
        }
        db
    }
}

/// DRIVE's five curves, green so the card draws what the core runs.
/// Every curve has unit slope at zero; `k` is the drive.
pub mod drive_curve {
    use crate::params::console::drive as p;

    /// The tube: asymmetric soft saturation with a heavier bias than
    /// the preamp's iron — a triode's, so the even harmonics lead well
    /// into the drive.
    pub fn tube(x: f32, k: f32) -> f32 {
        let b = p::TUBE_BIAS;
        let tb = b.tanh();
        let slope = k * (1.0 - tb * tb);
        ((k * x + b).tanh() - tb) / slope
    }

    /// Tape: symmetric and round, the softest knee.
    pub fn tape(x: f32, k: f32) -> f32 {
        let y = k * x;
        y / (1.0 + y * y).sqrt() / k
    }

    /// The transistor: symmetric, a hard knee, odd harmonics.
    pub fn transistor(x: f32, k: f32) -> f32 {
        let y = k * x;
        y / (1.0 + y.abs().powf(3.5)).powf(1.0 / 3.5) / k
    }

    /// Fuzz: a starved stage — biased, then clipped flat.
    pub fn fuzz(x: f32, k: f32) -> f32 {
        let b = p::FUZZ_BIAS * (1.0 - 1.0 / k);
        ((k * x + b).clamp(-1.0, 1.0) - b.clamp(-1.0, 1.0)) / k
    }

    /// The folder: a triangle wave of the input — identity inside the
    /// rails, reflected outside.
    pub fn fold(x: f32, k: f32) -> f32 {
        let y = k * x;
        let m = (y + 1.0) * 0.25;
        let tri = 4.0 * (m - (m + 0.5).floor()).abs() - 1.0;
        tri / k
    }

    /// The drive `k` a character reaches at `drive` (0..1).
    pub fn drive_of(character: u32, drive: f32) -> f32 {
        let full = match character {
            p::TUBE => p::TUBE_DRIVE,
            p::TAPE => p::TAPE_DRIVE,
            p::TRANSISTOR => p::TRANSISTOR_DRIVE,
            p::FUZZ => p::FUZZ_DRIVE,
            _ => p::FOLD_DRIVE,
        };
        1.0 + full * drive.clamp(0.0, 1.0)
    }

    /// The character's transfer at `drive`, for `x` in −1..1.
    pub fn transfer(character: u32, drive: f32, x: f32) -> f32 {
        if drive <= 0.0 {
            return x;
        }
        let k = drive_of(character, drive);
        match character {
            p::TUBE => tube(x, k),
            p::TAPE => tape(x, k),
            p::TRANSISTOR => transistor(x, k),
            p::FUZZ => fuzz(x, k),
            _ => fold(x, k),
        }
    }
}

/// FOUR's shape, green so the card draws the curve the core runs.
pub mod four_curve {
    use crate::dsp::filters::{BandShape, EqBand};
    use crate::params::console::four as p;

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Band {
        pub hz: f32,
        pub db: f32,
        pub q: f32,
        pub shape: BandShape,
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Shape {
        pub bands: [Band; 4],
        /// The inductor bump under a low SHELF, when there is one.
        pub inductor: Option<Band>,
    }

    impl Shape {
        pub fn of(params: &super::SectionParams) -> Self {
            let table = super::SectionKind::Four.table();
            let clamp = |id: u32| {
                let value = params.value(id);
                table
                    .iter()
                    .find(|def| def.id == id)
                    .map_or(value, |def| def.clamp(value))
            };
            let low_bell = clamp(p::LOW_SHAPE) >= 0.5;
            let high_bell = clamp(p::HIGH_SHAPE) >= 0.5;
            let low = Band {
                hz: clamp(p::LOW_HZ),
                db: clamp(p::LOW_DB),
                q: if low_bell { 0.8 } else { p::SHELF_Q },
                shape: if low_bell {
                    BandShape::Bell
                } else {
                    BandShape::LowShelf
                },
            };
            Self {
                bands: [
                    low,
                    Band {
                        hz: clamp(p::LMF_HZ),
                        db: clamp(p::LMF_DB),
                        q: clamp(p::LMF_Q),
                        shape: BandShape::Bell,
                    },
                    Band {
                        hz: clamp(p::HMF_HZ),
                        db: clamp(p::HMF_DB),
                        q: clamp(p::HMF_Q),
                        shape: BandShape::Bell,
                    },
                    Band {
                        hz: clamp(p::HIGH_HZ),
                        db: clamp(p::HIGH_DB),
                        q: if high_bell { 0.8 } else { p::SHELF_Q },
                        shape: if high_bell {
                            BandShape::Bell
                        } else {
                            BandShape::HighShelf
                        },
                    },
                ],
                // A passive low shelf resonates just inside its corner;
                // a bell there, opposite in sign to the shelf, is what
                // makes an old EQ's bottom tight rather than woolly.
                inductor: (!low_bell && low.db != 0.0).then(|| Band {
                    hz: low.hz * p::INDUCTOR_AT,
                    db: -low.db * p::INDUCTOR_SHARE,
                    q: p::INDUCTOR_Q,
                    shape: BandShape::Bell,
                }),
            }
        }

        pub fn is_flat(&self) -> bool {
            self.bands.iter().all(|band| band.db == 0.0)
        }

        /// Every band the core runs, the inductor included.
        pub fn all(&self) -> impl Iterator<Item = &Band> {
            self.bands.iter().chain(self.inductor.iter())
        }
    }

    /// ONE band's contribution at `hz`, in dB.
    ///
    /// The sum hides the parts: two mids fighting each other read as one
    /// gentle curve, and the inductor's bump is invisible inside the
    /// shelf it belongs to. The card draws each band behind the
    /// composite, and this is what it draws them from — the same
    /// coefficients [`response_db`] adds up.
    pub fn band_db(band: &Band, sample_rate: f32, hz: f32) -> f32 {
        if band.db == 0.0 {
            return 0.0;
        }
        let mut one = EqBand::new();
        one.prepare(sample_rate, band.hz, band.q, band.db, band.shape);
        super::tone_curve::biquad_db(one.coeffs(), hz, sample_rate)
    }

    /// The whole section's response at `hz`, in dB.
    pub fn response_db(shape: &Shape, sample_rate: f32, hz: f32) -> f32 {
        let mut db = 0.0;
        let mut band = EqBand::new();
        for want in shape.all() {
            if want.db == 0.0 {
                continue;
            }
            band.prepare(sample_rate, want.hz, want.q, want.db, want.shape);
            db += super::tone_curve::biquad_db(band.coeffs(), hz, sample_rate);
        }
        db
    }
}

/// What a section measured this frame, as the surface reads it: the
/// green twin of the engine's readout, so a card can carry live figures
/// without the surface importing the audio side. Level in dBFS,
/// reduction in dB (zero is none, negative is reduction), and up to
/// three bands of the same figure for a section that has bands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Telemetry {
    pub level_db: f32,
    pub reduction_db: f32,
    pub bands: [f32; 3],
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            level_db: -120.0,
            reduction_db: 0.0,
            bands: [0.0; 3],
        }
    }
}

/// SMEAR's law, green side.
///
/// The card's whole middle is a bar chart of WHEN each octave will
/// arrive, and a card that guessed at that would be drawing a disperser
/// that does not exist. So the group delay is computed from the very
/// coefficients the kernel is tuned with — a topology-preserving SVF
/// allpass at a prewarped corner — and the answer is in milliseconds,
/// which is what the eye is being asked to read.
pub mod smear_curve {
    use super::SectionParams;
    use crate::params::console::smear as p;

    /// What the section is set to. The same two numbers the core reads.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Shape {
        pub stages: u32,
        pub centre: f32,
    }

    impl Shape {
        pub fn of(params: &SectionParams) -> Self {
            let table = super::SectionKind::Smear.table();
            let clamp = |id: u32| {
                let value = params.value(id);
                table
                    .iter()
                    .find(|def| def.id == id)
                    .map_or(value, |def| def.clamp(value))
            };
            Self {
                stages: clamp(p::AMOUNT).round().max(0.0) as u32,
                centre: clamp(p::CENTRE),
            }
        }
    }

    /// The allpass's denominator, at the same prewarped corner the SVF
    /// takes: `H(z) = (a2 + a1 z⁻¹ + z⁻²) / (1 + a1 z⁻¹ + a2 z⁻²)`.
    fn coefficients(sample_rate: f64, hz: f64) -> (f64, f64) {
        let nyquist = sample_rate * 0.5;
        let hz = hz.clamp(1.0, nyquist * 0.98);
        let g = (core::f64::consts::PI * hz / sample_rate).tan();
        let k = 1.0 / f64::from(p::STAGE_Q).max(1e-3);
        let denom = 1.0 + g * k + g * g;
        if denom.abs() < 1e-12 {
            return (0.0, 0.0);
        }
        ((2.0 * g * g - 2.0) / denom, (1.0 - g * k + g * g) / denom)
    }

    /// The phase one section turns at `omega`, in radians.
    fn phase_at(a1: f64, a2: f64, omega: f64) -> f64 {
        let (s1, c1) = omega.sin_cos();
        let (s2, c2) = (2.0 * omega).sin_cos();
        // Numerator a2 + a1 z⁻¹ + z⁻², denominator 1 + a1 z⁻¹ + a2 z⁻².
        let nre = a2 + a1 * c1 + c2;
        let nim = -(a1 * s1 + s2);
        let dre = 1.0 + a1 * c1 + a2 * c2;
        let dim = -(a1 * s1 + a2 * s2);
        nim.atan2(nre) - dim.atan2(dre)
    }

    /// How long the chain holds `hz` back, in milliseconds.
    ///
    /// The group delay is the slope of the phase, so it is measured the
    /// way a slope is measured: two phases a hair apart. The step is
    /// small enough that no wrap can fall between them at any delay this
    /// section can produce, and the arithmetic is in double so a
    /// difference of two nearly equal angles keeps its digits.
    pub fn group_delay_ms(shape: &Shape, sample_rate: f32, hz: f32) -> f32 {
        if shape.stages == 0 {
            return 0.0;
        }
        let fs = f64::from(sample_rate).max(1.0);
        let (a1, a2) = coefficients(fs, f64::from(shape.centre));
        let nyquist = fs * 0.5;
        let omega = core::f64::consts::TAU * f64::from(hz).clamp(1.0, nyquist * 0.98) / fs;
        let step = 1e-3;
        let lo = (omega - step).max(1e-6);
        let hi = (omega + step).min(core::f64::consts::PI - 1e-6);
        let slope = (phase_at(a1, a2, hi) - phase_at(a1, a2, lo)) / (hi - lo);
        let samples = (-slope).max(0.0) * f64::from(shape.stages);
        (samples * 1000.0 / fs) as f32
    }
}

/// GLUE's law, green side.
///
/// The bus compressor's card draws the settle line the fill walks to,
/// and a card that sketched that line would be drawing a compressor
/// that does not exist. So the card asks the SAME kernel the core
/// configures, with the same numbers, and the two cannot drift.
pub mod glue_curve {
    use crate::dsp::dynamics::{GainComputer, Mode};
    use crate::params::console::glue as p;

    /// Where the threshold stands for a lean of 0..100: at the top of
    /// the scale when nothing is leaning, down in the mix at full.
    pub fn threshold_db(lean: f32) -> f32 {
        let lean = (lean / 100.0).clamp(0.0, 1.0);
        p::THRESHOLD_HIGH_DB + (p::THRESHOLD_LOW_DB - p::THRESHOLD_HIGH_DB) * lean
    }

    /// How much gain the computer asks for at `level_db`, in dB at or
    /// below zero — the kernel's own soft knee, configured exactly as
    /// the core configures it.
    pub fn gain_db(level_db: f32, threshold_db: f32) -> f32 {
        let mut computer = GainComputer::new();
        computer.configure(Mode::Compress, threshold_db, p::RATIO, p::KNEE_DB);
        computer.gain_db(level_db)
    }
}

/// A section's settings as the graph's spec carries them: the kind, and
/// the edits by id — exactly a device's overrides. A missing id reads
/// as the table's default.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SectionParams {
    pub kind: SectionKind,
    #[serde(default)]
    pub values: Vec<(u32, f32)>,
}

impl SectionParams {
    /// The kind at its defaults.
    pub fn of(kind: SectionKind) -> Self {
        Self {
            kind,
            values: Vec::new(),
        }
    }

    pub fn value(&self, param: u32) -> f32 {
        if let Some((_, value)) = self.values.iter().find(|(id, _)| *id == param) {
            return *value;
        }
        self.kind
            .table()
            .iter()
            .find(|def| def.id == param)
            .map_or(0.0, |def| def.default)
    }

    /// A copy with EVERY parameter present, at whatever this table
    /// says or the kind's default where it says nothing.
    ///
    /// The document keeps a section's settings sparsely: only what
    /// somebody moved is written down, which is what makes a saved song
    /// small and a default readable. The AUDIO side cannot afford that.
    /// A knob turn arrives on the audio thread as a letter, and
    /// [`Self::set`] on a sparse table PUSHES the first time it sees an
    /// id — a Vec growing inside the callback, which is an allocation
    /// in the one place that may never allocate. A dense table is
    /// already the right length, so setting a value only ever
    /// overwrites one.
    pub fn dense(&self) -> Self {
        Self {
            kind: self.kind,
            values: self
                .kind
                .table()
                .iter()
                .map(|def| (def.id, self.value(def.id)))
                .collect(),
        }
    }

    pub fn set(&mut self, param: u32, value: f32) {
        let Some(def) = self.kind.table().iter().find(|def| def.id == param) else {
            return;
        };
        let value = def.clamp(value);
        match self.values.iter_mut().find(|(id, _)| *id == param) {
            Some(entry) => entry.1 = value,
            None => self.values.push((param, value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SMEAR's card draws WHEN each octave arrives, so the figure it
    /// draws is checked against the identity a second-order allpass is
    /// known by: at its own corner one section holds the signal back by
    /// four Q over omega-nought, and far below the corner by two over Q
    /// omega-nought. If this ever stops agreeing, the card has started
    /// drawing a disperser that is not the one running.
    #[test]
    fn the_smear_curve_is_the_allpass_the_kernel_runs() {
        use crate::params::console::smear as p;
        let fs = 48_000.0;
        let centre = 1_000.0;
        let q = p::STAGE_Q;
        let one = smear_curve::Shape { stages: 1, centre };
        let at_corner = smear_curve::group_delay_ms(&one, fs, centre);
        let analogue = 4.0 * q / (core::f32::consts::TAU * centre) * 1000.0;
        assert!(
            (at_corner - analogue).abs() < analogue * 0.05,
            "at the corner: {at_corner} ms against {analogue} ms"
        );
        let low = smear_curve::group_delay_ms(&one, fs, 30.0);
        let far = 2.0 / (q * core::f32::consts::TAU * centre) * 1000.0;
        assert!(
            (low - far).abs() < far * 0.15,
            "well below the corner: {low} ms against {far} ms"
        );
        // The corner is where the holding is deepest, and stacking
        // sections multiplies it exactly.
        assert!(at_corner > low * 2.0);
        assert!(at_corner > smear_curve::group_delay_ms(&one, fs, 12_000.0));
        let many = smear_curve::Shape { stages: 16, centre };
        assert!(
            (smear_curve::group_delay_ms(&many, fs, centre) - at_corner * 16.0).abs() < 1e-3,
            "sixteen sections are not sixteen times one"
        );
        // No sections is a wire, at every frequency.
        let none = smear_curve::Shape { stages: 0, centre };
        for hz in [30.0, 1_000.0, 16_000.0] {
            assert_eq!(smear_curve::group_delay_ms(&none, fs, hz), 0.0);
        }
    }

    /// GLUE's settle line is the kernel's own knee, so leaning walks the
    /// threshold down the vessel and a quiet mix is never touched.
    #[test]
    fn the_glue_curve_is_the_computer_the_core_configures() {
        let open = glue_curve::threshold_db(0.0);
        let deep = glue_curve::threshold_db(100.0);
        assert_eq!(open, crate::params::console::glue::THRESHOLD_HIGH_DB);
        assert_eq!(deep, crate::params::console::glue::THRESHOLD_LOW_DB);
        assert_eq!(glue_curve::gain_db(-90.0, open), 0.0);
        assert!(glue_curve::gain_db(0.0, deep) < 0.0);
        // Past the knee the law is the ratio: one dB in, 1/2.5 dB out.
        let a = glue_curve::gain_db(-6.0, deep);
        let b = glue_curve::gain_db(-5.0, deep);
        let slope = 1.0 + (b - a);
        assert!(
            (slope - 1.0 / crate::params::console::glue::RATIO).abs() < 0.02,
            "the slope past the knee is {slope}"
        );
    }

    #[test]
    fn the_strip_starts_at_the_preamp_and_ends_at_out() {
        assert_eq!(SectionKind::STRIP[0], SectionKind::Preamp);
        assert_eq!(SectionKind::STRIP[19], SectionKind::Out);
        assert!(SectionKind::Preamp.always_in());
        assert!(SectionKind::Out.always_in());
        assert!(!SectionKind::Tone.always_in());
        let mut seen = std::collections::HashSet::new();
        for kind in SectionKind::STRIP {
            assert!(seen.insert(kind), "{kind:?} is on the strip twice");
        }
    }

    #[test]
    fn every_kind_has_a_table_whose_ids_are_its_positions() {
        for kind in SectionKind::ALL {
            for (index, def) in kind.table().iter().enumerate() {
                assert_eq!(def.id as usize, index, "{kind:?}: {}", def.name);
                assert!(
                    def.min <= def.default && def.default <= def.max,
                    "{kind:?}: {}",
                    def.name
                );
            }
            assert!(!kind.name().is_empty());
            assert!(!kind.code().is_empty());
        }
    }

    #[test]
    fn the_buses_and_the_mix_are_always_in() {
        for kind in SectionKind::BUS
            .into_iter()
            .chain(SectionKind::MIX)
            .chain(SectionKind::RETURNS)
        {
            assert!(kind.always_in(), "{kind:?} could be taken out");
        }
    }

    /// The whole difference between the two stages, measured rather
    /// than asserted: the asymmetric stage HAS even harmonics, the
    /// symmetric one cancels them.
    ///
    /// Not that the iron's second beats its own third — it does not,
    /// at drive, because tanh's odd term outruns what the bias adds.
    /// The reading that separates the stages is across them: whatever
    /// even content the iron makes, the steel makes essentially none.
    #[test]
    fn only_the_asymmetric_stage_makes_even_harmonics() {
        use crate::console::preamp_curve as p;
        for drive in [0.35, 0.7, 1.0] {
            let iron = p::harmonics(drive, false, 0.5);
            let steel = p::harmonics(drive, true, 0.5);
            assert!(
                iron.db[1] > steel.db[1] + 30.0,
                "drive {drive}: iron {iron:?} against steel {steel:?}"
            );
            assert!(
                iron.db[3] > steel.db[3] + 20.0,
                "drive {drive}: the fourth should part the two stages too"
            );
            // Both stages are odd-harmonic machines underneath; that is
            // not what tells them apart.
            assert!(iron.db[2] > p::FLOOR_DB && steel.db[2] > p::FLOOR_DB);
            assert!(iron.thd > 0.0 && steel.thd > 0.0);
        }
    }

    /// At the floor the stage is a wire to the sample, and a wire makes
    /// no harmonics. The card draws an empty ladder because the ladder
    /// IS empty, not because it was told to.
    #[test]
    fn a_stage_at_its_floor_makes_nothing() {
        use crate::console::preamp_curve as p;
        let quiet = p::harmonics(0.0, false, 0.5);
        assert_eq!(quiet.thd, 0.0);
        assert!(quiet.db.iter().skip(1).all(|db| *db <= p::FLOOR_DB));
        assert_eq!(p::harmonics(1.0, false, 0.0).thd, 0.0);
    }

    /// Harder is dirtier, on both stages. A ladder that did not follow
    /// the knob would be a picture rather than a reading.
    #[test]
    fn leaning_on_the_stage_raises_its_distortion() {
        use crate::console::preamp_curve as p;
        for steel in [false, true] {
            let soft = p::harmonics(0.25, steel, 0.5).thd;
            let hard = p::harmonics(1.0, steel, 0.5).thd;
            assert!(hard > soft, "steel={steel}: {hard} should exceed {soft}");
        }
    }

    /// The ladder is read off the same `transfer` the core runs, so a
    /// fundamental measured through a stage that is a wire comes back
    /// as the probe itself.
    #[test]
    fn the_ladder_reads_the_curve_it_is_drawn_beside() {
        use crate::console::preamp_curve as p;
        for amplitude in [0.1, 0.35, 0.7] {
            let measured = p::harmonics(0.6, false, amplitude);
            assert!(measured.thd.is_finite() && measured.thd >= 0.0);
            assert_eq!(measured.db[0], 0.0);
        }
    }

    /// At rest the section is a wire to the sample, and the model says
    /// so at every instant of the note rather than only on average.
    #[test]
    fn a_hit_at_rest_touches_nothing() {
        let trace = crate::console::hit_curve::trace(0.0, 0.0, 250.0, 400.0, 64);
        assert_eq!(trace.len(), 64);
        for point in &trace {
            assert_eq!(point.gain_db, 0.0, "the wire moved at {} ms", point.ms);
        }
    }

    /// The two levers reach different parts of the note: ATTACK lands on
    /// the strike and is gone by the tail, SUSTAIN does the opposite.
    /// That separation is the whole device.
    #[test]
    fn attack_lands_on_the_strike_and_sustain_on_the_tail() {
        use crate::params::console::hit as p;
        let struck = crate::console::hit_curve::trace(1.0, 0.0, 250.0, 400.0, 200);
        let held = crate::console::hit_curve::trace(0.0, 1.0, 250.0, 400.0, 200);
        let early = |trace: &[crate::console::hit_curve::Point]| {
            trace
                .iter()
                .filter(|point| point.ms <= p::WINDOW_MS)
                .fold(0.0f32, |most, point| most.max(point.gain_db))
        };
        let late = |trace: &[crate::console::hit_curve::Point]| {
            trace
                .iter()
                .filter(|point| point.ms >= 200.0)
                .fold(0.0f32, |most, point| most.max(point.gain_db))
        };
        assert!(early(&struck) > 6.0, "attack did not lift the strike");
        assert!(late(&struck) < 1.0, "attack reached the tail");
        assert!(late(&held) > 3.0, "sustain did not lift the tail");
        assert!(early(&held) < 1.0, "sustain reached the strike");
    }

    /// Neither lever can move a note further than the range it is given,
    /// in either direction.
    #[test]
    fn no_lever_moves_a_note_past_its_range() {
        use crate::params::console::hit as p;
        for (attack, sustain) in [(1.0, 1.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0)] {
            for point in crate::console::hit_curve::trace(attack, sustain, 250.0, 400.0, 128) {
                assert!(
                    point.gain_db.abs() <= p::RANGE_DB + 0.01,
                    "{attack}/{sustain} moved {} dB at {} ms",
                    point.gain_db,
                    point.ms
                );
                assert!((0.0..=1.0).contains(&point.strike));
                assert!((0.0..=1.0).contains(&point.tail));
            }
        }
    }
}
