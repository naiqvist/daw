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

    /// Where on the strip this section stands, or `None` for a bus,
    /// mix or return section.
    pub fn strip_index(self) -> Option<usize> {
        Self::STRIP.iter().position(|kind| *kind == self)
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
    fn biquad_db(coeffs: [f32; 5], hz: f32, sample_rate: f32) -> f32 {
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
}
