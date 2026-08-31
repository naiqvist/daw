//! The bus compressor — the effect half of `Node::Glue`.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic here belongs
//! to [`crate::dsp::dynamics`], [`crate::dsp::filters`] and
//! [`crate::dsp::shaper`]. This file's whole job is to say what feeds
//! what, and in which direction.
//!
//! # The direction is the point
//!
//! This is a FEEDBACK compressor, and that is the single decision that
//! makes it sound like the console unit it is modelled on rather than
//! like a textbook compressor with the same numbers on it.
//!
//! ```text
//!            ┌──────────────────────────────────────┐
//!  in ───────┤ x  gain                              ├──── x makeup ──── mix ─── clip ─── out
//!            └───▲──────────────────────────────┬───┘                    ▲
//!                │                              │                        │
//!             ballistics ◄─ gain computer ◄─ detector ◄── sc high-pass ───┘ (dry)
//! ```
//!
//! The detector reads the compressor's OWN OUTPUT, not its input. Two
//! consequences, and they are the whole character:
//!
//! - **The effective ratio softens.** Reducing the signal reduces what
//!   the detector sees, which reduces the reduction. The loop settles
//!   somewhere gentler than the ratio nominally asks for, and it does so
//!   more at high levels than at low ones — a compression curve that
//!   bends instead of hinging.
//! - **The attack becomes level dependent.** How fast the loop converges
//!   depends on how far over threshold it started, which is what a diode
//!   in a real sidechain does and what "it breathes" means when people
//!   say it about these units.
//!
//! The cost is that it cannot be written with block kernels. Sample `n`'s
//! gain depends on sample `n-1`'s output, so the loop is closed here, one
//! sample at a time, through the per-sample doors those kernels expose
//! ([`RmsDetector::tick`], [`Ballistics::tick`],
//! [`GainComputer::gain_db`], [`OnePole::tick_highpass`],
//! [`Waveshaper::shape`]). A block-only kernel set would have forced a
//! feedforward topology — a different compressor wearing this one's
//! panel.
//!
//! # What is read per segment and what is read per sample
//!
//! Per SEGMENT: everything that costs a transcendental to rebuild — the
//! ballistics coefficients, the sidechain corner, the gain computer's
//! threshold/ratio/knee. A segment is a few milliseconds and these are
//! settings, not signals.
//!
//! Per SAMPLE: the loop itself, and the two levels that must not step —
//! the makeup gain and the dry/wet mix, both ramped across the block the
//! way [`Node::Pan`](crate::audio::graph::Node)'s fader is.
//!
//! # Red zone
//!
//! Everything except [`GlueCore::new`] runs in the audio callback: no
//! allocation, no locks, no panic paths, no unbounded loops.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::dynamics::{Ballistics, GainComputer, Mode, RmsDetector};
use crate::dsp::filters::OnePole;
use crate::dsp::shaper::{Mode as ShapeMode, Waveshaper};
use crate::params::glue as p;

/// The detector's RMS window, in ms.
///
/// Short enough to follow a mix's shape, long enough that it reads
/// LOUDNESS rather than waveform — a window under a few ms starts
/// tracking individual cycles of a bass note and the compressor
/// modulates at the bass frequency.
const WINDOW_MS: f32 = 10.0;

/// Where the peak clipper's rails sit, as a linear amplitude.
///
/// Just under full scale: the clipper exists to stop a transient
/// arriving at 0 dBFS from becoming an integer overshoot downstream, and
/// a rail exactly at 1.0 leaves nothing for the soft knee to bend in.
const CLIP_CEILING: f32 = 0.99;

/// How hard the soft clipper is driven at the rails. The waveshaper's
/// drive is input gain into `tanh`, so this is how sharply the curve
/// turns over — gentle, because this is a safety net and not a tone.
const CLIP_DRIVE: f32 = 1.0;

/// The compressor's editable state, in ENGINE units — dB, percentages,
/// Hz, and the three switches as the float indices every table row is.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GlueParams {
    pub threshold_db: f32,
    /// An index into [`crate::params::glue::RATIO_NAMES`].
    pub ratio: f32,
    /// An index into [`crate::params::glue::ATTACK_NAMES`].
    pub attack: f32,
    /// An index into [`crate::params::glue::RELEASE_NAMES`]; the last
    /// position is AUTO and is not a time.
    pub release: f32,
    pub makeup_db: f32,
    pub dry_wet: f32,
    pub range_db: f32,
    /// Above 0.5 the peak clipper runs.
    pub clip: f32,
    pub sc_hp_hz: f32,
}

impl Default for GlueParams {
    fn default() -> Self {
        let d = |id: u32| crate::params::def(p::TABLE, id).default;
        Self {
            threshold_db: d(p::THRESHOLD),
            ratio: d(p::RATIO),
            attack: d(p::ATTACK),
            release: d(p::RELEASE),
            makeup_db: d(p::MAKEUP),
            dry_wet: d(p::DRY_WET),
            range_db: d(p::RANGE),
            clip: d(p::CLIP),
            sc_hp_hz: d(p::SC_HP),
        }
    }
}

impl GlueParams {
    /// This patch's value for a wire id, or `None` for one it does not
    /// have — how a target aimed at the wrong device is refused.
    pub fn get(&self, param: u32) -> Option<f32> {
        Some(match param {
            p::THRESHOLD => self.threshold_db,
            p::RATIO => self.ratio,
            p::ATTACK => self.attack,
            p::RELEASE => self.release,
            p::MAKEUP => self.makeup_db,
            p::DRY_WET => self.dry_wet,
            p::RANGE => self.range_db,
            p::CLIP => self.clip,
            p::SC_HP => self.sc_hp_hz,
            _ => return None,
        })
    }

    /// Write a wire id's value. Unknown ids are dropped, never guessed.
    pub fn set(&mut self, param: u32, value: f32) {
        match param {
            p::THRESHOLD => self.threshold_db = value,
            p::RATIO => self.ratio = value,
            p::ATTACK => self.attack = value,
            p::RELEASE => self.release = value,
            p::MAKEUP => self.makeup_db = value,
            p::DRY_WET => self.dry_wet = value,
            p::RANGE => self.range_db = value,
            p::CLIP => self.clip = value,
            p::SC_HP => self.sc_hp_hz = value,
            _ => {}
        }
    }

    /// A switch position, clamped into its list. RON round-trips NaN, so
    /// a hand-edited project can smuggle one in and this is where it
    /// stops.
    fn index(value: f32, count: usize) -> u32 {
        if value.is_finite() {
            (value.round().max(0.0) as u32).min(count.saturating_sub(1) as u32)
        } else {
            0
        }
    }

    pub fn ratio_index(self) -> u32 {
        Self::index(self.ratio, p::RATIO_NAMES.len())
    }

    pub fn attack_index(self) -> u32 {
        Self::index(self.attack, p::ATTACK_NAMES.len())
    }

    pub fn release_index(self) -> u32 {
        Self::index(self.release, p::RELEASE_NAMES.len())
    }

    pub fn clipping(self) -> bool {
        self.clip >= 0.5
    }
}

/// What the settings resolve to once the switches have been read. Held
/// so the segment prologue can tell whether anything actually moved.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resolved {
    threshold_db: f32,
    ratio: f32,
    knee_db: f32,
    attack_ms: f32,
    release_ms: f32,
    auto: bool,
    sc_hp_hz: f32,
    range_db: f32,
}

impl Resolved {
    fn of(params: &GlueParams) -> Self {
        // NOT `ParamDef::clamp` alone: `f32::clamp` returns NaN for a NaN
        // input, so a poisoned value walks straight through a range check
        // and into the loop. RON round-trips NaN literals, so a
        // hand-edited or corrupt project is a real way to get one.
        let clamp = |id: u32, v: f32| {
            let def = crate::params::def(p::TABLE, id);
            if v.is_finite() {
                def.clamp(v)
            } else {
                def.default
            }
        };
        let ratio_index = params.ratio_index();
        let release_index = params.release_index();
        Self {
            threshold_db: clamp(p::THRESHOLD, params.threshold_db),
            ratio: p::ratio(ratio_index),
            // NOT a control: the knee follows the ratio, and the figure
            // comes from `params` so the drawn curve reads the same one.
            knee_db: p::knee_db(ratio_index),
            attack_ms: p::attack_ms(params.attack_index()),
            release_ms: p::release_ms(release_index),
            auto: p::is_auto(release_index),
            sc_hp_hz: clamp(p::SC_HP, params.sc_hp_hz),
            range_db: clamp(p::RANGE, params.range_db),
        }
    }
}

/// The compressor's kernel chain, boxed so `Node` stays lean.
pub struct GlueCore {
    /// ONE detector, ONE ballistics, ONE gain: the two channels are
    /// LINKED, which is what stops the image wandering when a bass note
    /// on the left ducks only the left.
    detector: RmsDetector,
    sc_hp: OnePole,
    computer: GainComputer,
    ballistics: Ballistics,
    clipper: Waveshaper,
    /// The last thing `prepare` was called with, so a parked compressor
    /// rebuilds nothing.
    prepared: Resolved,
    params: GlueParams,
    /// Makeup and mix as LINEAR values, where the last segment left them
    /// — both ramp across the block rather than stepping at its edge.
    makeup: f32,
    mix: f32,
    /// Gain reduction as of the last sample, in dB (≤ 0). The loop's own
    /// state — what the NEXT sample's gain is computed from.
    reduction_db: f32,
    /// The extremes of the segment just processed: the loudest the
    /// detector heard and the most the loop reduced.
    ///
    /// Separate from `reduction_db` because they answer different
    /// questions. The loop needs the newest value; a display needs the
    /// worst one, since it reads at frame rate and would otherwise miss
    /// every transient that happened between two repaints.
    said: crate::audio::graph::Readout,
    sample_rate: f32,
}

impl GlueCore {
    /// GREEN ZONE. Builds every kernel the callback will use; nothing
    /// here is called again from the audio thread.
    pub fn new(sample_rate: f32, params: &GlueParams) -> Self {
        let resolved = Resolved::of(params);
        let mut detector = RmsDetector::new();
        detector.prepare(sample_rate, WINDOW_MS);
        let mut sc_hp = OnePole::new();
        sc_hp.prepare(sample_rate, resolved.sc_hp_hz);
        let mut computer = GainComputer::new();
        computer.configure(
            Mode::Compress,
            resolved.threshold_db,
            resolved.ratio,
            resolved.knee_db,
        );
        let mut ballistics = Ballistics::new();
        ballistics.prepare(sample_rate, resolved.attack_ms, resolved.release_ms);
        ballistics.set_auto(resolved.auto);
        let mut clipper = Waveshaper::new();
        clipper.configure(ShapeMode::SoftClip, CLIP_DRIVE, 0.0, 1.0);
        Self {
            detector,
            sc_hp,
            computer,
            ballistics,
            clipper,
            prepared: resolved,
            params: *params,
            makeup: db_to_gain(params.makeup_db),
            mix: mix_of(params.dry_wet),
            reduction_db: 0.0,
            said: crate::audio::graph::Readout::default(),
            sample_rate,
        }
    }

    /// Red zone: a letter. Stored now, resolved at the top of the next
    /// segment — rebuilding a coefficient per letter would mean doing it
    /// several times for one mouse move.
    pub fn set_param(&mut self, param: u32, value: f32) {
        // The same rule `Node::apply` applies before any arm sees a
        // letter, restated because this door can also be reached
        // directly: a non-finite value is not a setting, it is a bug
        // somewhere upstream, and one of them in a FEEDBACK loop poisons
        // every sample from here on rather than one.
        if !value.is_finite() {
            return;
        }
        let Some(value) = crate::params::clamp(p::TABLE, param, value) else {
            return;
        };
        self.params.set(param, value);
    }

    /// Red zone: a seek. Forget the signal's history and land every
    /// control, so the new position starts clean rather than gliding the
    /// old one's compression in.
    pub fn snap(&mut self) {
        self.detector.reset();
        self.sc_hp.reset();
        self.ballistics.reset();
        self.reduction_db = 0.0;
        self.said = crate::audio::graph::Readout::default();
        self.makeup = db_to_gain(self.params.makeup_db);
        self.mix = mix_of(self.params.dry_wet);
    }

    /// The gain reduction as of the last sample processed, in dB (≤ 0).
    pub fn reduction_db(&self) -> f32 {
        self.reduction_db
    }

    /// What this compressor has to say about the segment just processed.
    pub fn readout(&self) -> crate::audio::graph::Readout {
        self.said
    }

    /// Red zone: one segment, in place. `r` may be empty for a mono
    /// caller; the detector then hears the one channel.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;

        // --- the segment prologue: settings, only if they moved --------
        let want = Resolved::of(&self.params);
        if want != self.prepared {
            if want.threshold_db != self.prepared.threshold_db
                || want.ratio != self.prepared.ratio
                || want.knee_db != self.prepared.knee_db
            {
                self.computer.configure(
                    Mode::Compress,
                    want.threshold_db,
                    want.ratio,
                    want.knee_db,
                );
            }
            if want.attack_ms != self.prepared.attack_ms
                || want.release_ms != self.prepared.release_ms
            {
                self.ballistics
                    .prepare(self.sample_rate, want.attack_ms, want.release_ms);
            }
            if want.auto != self.prepared.auto {
                self.ballistics.set_auto(want.auto);
            }
            if want.sc_hp_hz != self.prepared.sc_hp_hz {
                self.sc_hp.prepare(self.sample_rate, want.sc_hp_hz);
            }
            self.prepared = want;
        }
        // The RANGE cap, as a floor on the gain: a reduction may not go
        // past it, whatever the computer asks for. Zero range is a
        // compressor switched off in all but name, which is what the
        // control is for.
        let floor_db = -want.range_db.clamp(0.0, p::RANGE_MAX_DB);
        let clipping = self.params.clipping();

        // --- the two levels, ramped across the segment -----------------
        let makeup_to = db_to_gain(self.params.makeup_db);
        let mix_to = mix_of(self.params.dry_wet);
        let makeup_step = (makeup_to - self.makeup) / n as f32;
        let mix_step = (mix_to - self.mix) / n as f32;
        let (makeup_from, mix_from) = (self.makeup, self.mix);

        // This segment's extremes start from nothing: the schedule keeps
        // the BLOCK's, across however many segments a block was split
        // into, so accumulating here as well would report a stale peak
        // forever.
        self.said = crate::audio::graph::Readout::default();

        // --- the loop ---------------------------------------------------
        for i in 0..n {
            let Some(left) = l.get_mut(i) else { break };
            let dry_l = *left;
            let dry_r = if stereo {
                r.get(i).copied().unwrap_or(dry_l)
            } else {
                dry_l
            };

            // The gain from the PREVIOUS sample's detector reading. This
            // one line is the feedback topology: the detector below is
            // fed from the output, so what it saw last sample is what
            // decides this one.
            let gain = db_to_gain(self.reduction_db);
            let wet_l = dry_l * gain;
            let wet_r = dry_r * gain;

            // The detector hears the OUTPUT, pre-makeup: makeup is a
            // level after the gain stage, and a detector that heard it
            // would make the makeup knob a second threshold.
            //
            // Linked by the larger of the two sides rather than their
            // sum, which cancels on out-of-phase material and would let
            // a wide mix walk straight past the threshold.
            let side = wet_l.abs().max(wet_r.abs());
            let side = self.sc_hp.tick_highpass(side);
            let level = self.detector.tick(side);
            let level_db = crate::dsp::arith::gain_to_db(level.max(1e-6));
            let target = self.computer.gain_db(level_db).max(floor_db);
            self.reduction_db = self.ballistics.tick(target);
            self.said.level_db = self.said.level_db.max(level_db);
            self.said.reduction_db = self.said.reduction_db.min(self.reduction_db);

            let makeup = makeup_from + makeup_step * i as f32;
            let mix = mix_from + mix_step * i as f32;
            let out_l = blend(dry_l, wet_l * makeup, mix);
            *left = if clipping { self.clip(out_l) } else { out_l };
            if stereo && let Some(right) = r.get_mut(i) {
                let out_r = blend(dry_r, wet_r * makeup, mix);
                *right = if clipping { self.clip(out_r) } else { out_r };
            }
        }
        self.makeup = makeup_to;
        self.mix = mix_to;
    }

    /// The peak clipper: soft, and at a rail just under full scale.
    #[inline(always)]
    fn clip(&self, x: f32) -> f32 {
        self.clipper.shape(x * (1.0 / CLIP_CEILING)) * CLIP_CEILING
    }
}

/// Wet against dry. A plain crossfade, in phase, because both sides are
/// the same signal at different gains — there is nothing to align.
#[inline(always)]
fn blend(dry: f32, wet: f32, mix: f32) -> f32 {
    dry + (wet - dry) * mix
}

/// The dry/wet mix as a fraction, and never a NaN — see `Resolved::of`
/// for why a clamp alone is not enough.
#[inline(always)]
fn mix_of(dry_wet: f32) -> f32 {
    if dry_wet.is_finite() {
        (dry_wet * 0.01).clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// Decibels as a linear gain, bounded and finite for anything a table
/// row can hold.
#[inline(always)]
fn db_to_gain(db: f32) -> f32 {
    if !db.is_finite() {
        return 1.0;
    }
    let g = (db * (1.0 / 20.0) * core::f32::consts::LOG2_10).exp2();
    if g.is_finite() {
        g.clamp(0.0, 64.0)
    } else {
        1.0
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn patch() -> GlueParams {
        GlueParams::default()
    }

    /// A sine at `db` dBFS, `secs` long, through the compressor — and
    /// the level that comes out, in dB.
    fn through(params: &GlueParams, in_db: f32, secs: f32) -> f32 {
        let mut core = GlueCore::new(FS, params);
        let n = (FS * secs) as usize;
        let amp = db_to_gain(in_db);
        let mut l: Vec<f32> = (0..n)
            .map(|i| (i as f32 / FS * 220.0 * core::f32::consts::TAU).sin() * amp)
            .collect();
        let mut r = l.clone();
        for (lc, rc) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            core.process(lc, rc);
        }
        // The settled tail, so the attack is not in the measurement.
        let tail = &l[n / 2..];
        let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
        20.0 * (rms * core::f32::consts::SQRT_2).max(1e-9).log10()
    }

    /// Every table row round-trips through the patch, and nothing else
    /// does.
    #[test]
    fn every_table_row_round_trips_and_no_other_id_does() {
        let mut params = patch();
        for (i, def) in p::TABLE.iter().enumerate() {
            assert_eq!(def.id as usize, i, "ids index their own row");
            let probe = def.min + (def.max - def.min) * 0.37;
            params.set(def.id, probe);
            assert_eq!(params.get(def.id), Some(probe), "{}", def.name);
        }
        for def in p::TABLE {
            let probe = def.min + (def.max - def.min) * 0.37;
            assert_eq!(
                params.get(def.id),
                Some(probe),
                "{} was overwritten",
                def.name
            );
        }
        assert_eq!(params.get(u32::MAX), None);
    }

    /// At its defaults the threshold sits at 0 dBFS, so ordinary
    /// material passes very nearly untouched — a compressor you have not
    /// dialled in must not be quietly squashing the mix.
    #[test]
    fn a_default_compressor_barely_touches_the_signal() {
        let out = through(&patch(), -20.0, 0.5);
        assert!(
            (out + 20.0).abs() < 0.5,
            "a -20 dBFS sine came out at {out:+.2} dBFS"
        );
    }

    /// IT COMPRESSES: the same input, over the threshold, comes out
    /// quieter — and further over, proportionally quieter still.
    #[test]
    fn over_the_threshold_it_reduces_and_keeps_reducing() {
        let mut params = patch();
        params.set(p::THRESHOLD, -30.0);
        params.set(p::ATTACK, 0.0); // land at once, so the tail is settled
        params.set(p::RELEASE, 0.0);

        let quiet = through(&params, -40.0, 0.5);
        assert!(
            (quiet + 40.0).abs() < 0.5,
            "below the threshold must pass: {quiet:+.2}"
        );

        let at = through(&params, -20.0, 0.5);
        let hot = through(&params, -10.0, 0.5);
        assert!(at < -20.0 + 0.5, "over the threshold must reduce: {at:+.2}");
        // Stated over the REDUCTION, not over the output level: more
        // input is always more output, just not proportionally so. The
        // first draft of this compared output levels and demanded the
        // louder input come out quieter, which is not compression — it
        // is a compressor wired backwards.
        assert!(
            (-10.0 - hot) > (-20.0 - at),
            "further over must reduce further: {:.2} dB against {:.2} dB",
            -10.0 - hot,
            -20.0 - at
        );
        // And 10 dB more in must arrive as clearly less than 10 dB more
        // out, which is the compression itself.
        let grew = hot - at;
        assert!(
            grew < 8.0,
            "10 dB in became {grew:.2} dB out — that is barely compressing"
        );
        assert!(grew > 0.0, "and the output must not go backwards");
    }

    /// THE FEEDBACK TOPOLOGY, measured: the loop settles gentler than the
    /// ratio nominally asks for, and more so the harder it is driven.
    ///
    /// A feedforward compressor at 10:1 with a 30 dB overshoot reduces by
    /// 27 dB. This one reads the result of its own reduction, so it
    /// settles well short of that — the curve bends instead of hinging,
    /// which is the whole reason to wire it this way.
    #[test]
    fn the_detector_reads_the_output_so_the_curve_bends() {
        let mut params = patch();
        params.set(p::THRESHOLD, -40.0);
        params.set(p::RATIO, 2.0); // 10:1, the hardest position
        params.set(p::ATTACK, 0.0);
        params.set(p::RELEASE, 0.0);
        params.set(p::RANGE, p::RANGE_MAX_DB);

        let out = through(&params, -10.0, 0.5);
        let reduction = -10.0 - out;
        // Feedforward at 10:1 would take 30 dB of overshoot down to 3 —
        // a 27 dB reduction. Feedback cannot reach that: reducing the
        // signal reduces what the detector sees.
        assert!(
            reduction > 3.0,
            "it must still compress hard: {reduction:.1} dB of reduction"
        );
        assert!(
            reduction < 24.0,
            "feedback must settle short of the feedforward figure, \
             got {reduction:.1} dB"
        );
    }

    /// RANGE caps the reduction, and zero range is a bypass in all but
    /// name.
    #[test]
    fn range_caps_the_reduction() {
        let mut params = patch();
        params.set(p::THRESHOLD, -40.0);
        params.set(p::RATIO, 2.0);
        params.set(p::ATTACK, 0.0);
        params.set(p::RELEASE, 0.0);

        let uncapped = -10.0 - through(&params, -10.0, 0.5);
        params.set(p::RANGE, 3.0);
        let capped = -10.0 - through(&params, -10.0, 0.5);
        assert!(
            capped < 3.5 && capped > 2.0,
            "a 3 dB range must cap near 3 dB, got {capped:.2}"
        );
        assert!(uncapped > capped + 2.0, "and uncapped must go further");

        params.set(p::RANGE, 0.0);
        let none = through(&params, -10.0, 0.5);
        assert!(
            (none + 10.0).abs() < 0.3,
            "zero range must pass the signal: {none:+.2}"
        );
    }

    /// Makeup and dry/wet are levels, not opinions: makeup scales the
    /// output, and a fully dry mix is the input back.
    #[test]
    fn makeup_lifts_and_a_dry_mix_is_the_input() {
        let mut params = patch();
        params.set(p::THRESHOLD, -40.0);
        params.set(p::ATTACK, 0.0);
        params.set(p::RELEASE, 0.0);
        let compressed = through(&params, -12.0, 0.4);

        params.set(p::MAKEUP, 6.0);
        let lifted = through(&params, -12.0, 0.4);
        assert!(
            (lifted - compressed - 6.0).abs() < 0.3,
            "6 dB of makeup moved it {:.2} dB",
            lifted - compressed
        );

        params.set(p::MAKEUP, 0.0);
        params.set(p::DRY_WET, 0.0);
        let dry = through(&params, -12.0, 0.4);
        assert!(
            (dry + 12.0).abs() < 0.1,
            "a fully dry mix must be the input: {dry:+.2}"
        );
    }

    /// The sidechain high-pass keeps bass out of the DETECTOR without
    /// taking it out of the signal.
    #[test]
    fn the_sidechain_filter_deafens_the_detector_to_bass() {
        let bass_through = |hp: f32| {
            let mut params = patch();
            params.set(p::THRESHOLD, -30.0);
            params.set(p::ATTACK, 0.0);
            params.set(p::RELEASE, 0.0);
            params.set(p::SC_HP, hp);
            let mut core = GlueCore::new(FS, &params);
            let n = (FS * 0.5) as usize;
            let amp = db_to_gain(-6.0);
            // 40 Hz, well under a 500 Hz sidechain corner.
            let mut l: Vec<f32> = (0..n)
                .map(|i| (i as f32 / FS * 40.0 * core::f32::consts::TAU).sin() * amp)
                .collect();
            let mut r = l.clone();
            for (lc, rc) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
                core.process(lc, rc);
            }
            core.reduction_db()
        };
        let open = bass_through(p::SC_HP_OFF_HZ);
        let filtered = bass_through(500.0);
        assert!(
            open < -3.0,
            "with the filter off, bass must compress: {open:.2}"
        );
        assert!(
            filtered > open + 2.0,
            "a 500 Hz sidechain corner must let the bass past: \
             {filtered:.2} against {open:.2}"
        );
    }

    /// Auto release reaches the compressor: the same burst recovers
    /// faster than the same hold, through the whole node rather than
    /// only in the kernel.
    #[test]
    fn auto_release_reaches_the_node() {
        let left_after = |release: f32, hold_secs: f32| {
            let mut params = patch();
            params.set(p::THRESHOLD, -30.0);
            params.set(p::ATTACK, 0.0);
            params.set(p::RELEASE, release);
            let mut core = GlueCore::new(FS, &params);
            let hold = (FS * hold_secs) as usize;
            let amp = db_to_gain(-6.0);
            let mut loud: Vec<f32> = (0..hold)
                .map(|i| (i as f32 / FS * 800.0 * core::f32::consts::TAU).sin() * amp)
                .collect();
            let mut r = loud.clone();
            for (lc, rc) in loud.chunks_mut(256).zip(r.chunks_mut(256)) {
                core.process(lc, rc);
            }
            let quiet_n = (FS * 0.4) as usize;
            let mut quiet = vec![0.0f32; quiet_n];
            let mut qr = vec![0.0f32; quiet_n];
            for (lc, rc) in quiet.chunks_mut(256).zip(qr.chunks_mut(256)) {
                core.process(lc, rc);
            }
            core.reduction_db()
        };
        let auto = p::RELEASE_AUTO as f32;
        let burst = left_after(auto, 0.03);
        let held = left_after(auto, 2.0);
        assert!(
            burst > held + 1.0,
            "auto must let go faster after a burst ({burst:.2} dB left) \
             than after a hold ({held:.2} dB left)"
        );
    }

    /// The peak clipper holds the rails, and leaves the signal alone
    /// when it is switched off.
    #[test]
    fn the_peak_clipper_holds_the_rails() {
        let peak = |clip: f32| {
            let mut params = patch();
            params.set(p::THRESHOLD, 10.0); // out of the way
            params.set(p::MAKEUP, 12.0);
            params.set(p::CLIP, clip);
            let mut core = GlueCore::new(FS, &params);
            let mut l: Vec<f32> = (0..512)
                .map(|i| (i as f32 / FS * 200.0 * core::f32::consts::TAU).sin() * 0.9)
                .collect();
            let mut r = l.clone();
            core.process(&mut l, &mut r);
            l.iter().fold(0.0f32, |m, s| m.max(s.abs()))
        };
        assert!(peak(0.0) > 1.5, "switched off, it must overshoot freely");
        assert!(peak(1.0) <= 1.0, "switched on, it must hold the rails");
    }

    /// Split-block equivalence, which the segmented transport depends
    /// on: 256 must equal 100 then 156, sample for sample.
    #[test]
    fn segmenting_a_block_changes_nothing() {
        let mut params = patch();
        params.set(p::THRESHOLD, -24.0);
        params.set(p::MAKEUP, 3.0);
        params.set(p::DRY_WET, 70.0);
        params.set(p::CLIP, 1.0);
        let input: Vec<f32> = (0..256).map(|i| ((i as f32) * 0.11).sin() * 0.6).collect();

        let mut whole = GlueCore::new(FS, &params);
        let (mut wl, mut wr) = (input.clone(), input.clone());
        whole.process(&mut wl, &mut wr);

        let mut split = GlueCore::new(FS, &params);
        let (mut sl, mut sr) = (input.clone(), input.clone());
        {
            let (l0, l1) = sl.split_at_mut(100);
            let (r0, r1) = sr.split_at_mut(100);
            split.process(l0, r0);
            split.process(l1, r1);
        }
        assert!(
            wl.iter().zip(&sl).all(|(a, b)| a.to_bits() == b.to_bits()),
            "256 must equal 100 + 156"
        );
        assert!(wr.iter().zip(&sr).all(|(a, b)| a.to_bits() == b.to_bits()));
    }

    /// The callback path allocates nothing — letters, seeks and every
    /// switch position included.
    #[test]
    fn the_callback_path_does_not_allocate() {
        let mut core = GlueCore::new(FS, &patch());
        let mut l = vec![0.3f32; 256];
        let mut r = vec![0.3f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..64 {
                core.set_param(p::THRESHOLD, -40.0 + i as f32);
                core.set_param(p::RATIO, (i % 3) as f32);
                core.set_param(p::ATTACK, (i % 7) as f32);
                core.set_param(p::RELEASE, (i % 7) as f32);
                core.set_param(p::MAKEUP, (i % 12) as f32);
                core.set_param(p::DRY_WET, (i % 100) as f32);
                core.set_param(p::RANGE, (i % 60) as f32);
                core.set_param(p::CLIP, (i % 2) as f32);
                core.set_param(p::SC_HP, 20.0 + i as f32 * 10.0);
                if i % 5 == 0 {
                    core.snap();
                }
                core.process(&mut l, &mut r);
            }
        });
    }

    /// Any segment length, including none and one, and a mono caller
    /// with no right channel at all.
    #[test]
    fn any_segment_length_is_accepted() {
        let mut params = patch();
        params.set(p::THRESHOLD, -30.0);
        params.set(p::CLIP, 1.0);
        let mut core = GlueCore::new(FS, &params);
        for len in [0usize, 1, 3, 7, 63, 100, 256] {
            let mut l = vec![0.4f32; len];
            let mut r = vec![0.4f32; len];
            core.process(&mut l, &mut r);
            assert!(l.iter().all(|s| s.is_finite()), "len {len}");
            let mut mono = vec![0.4f32; len];
            let mut none: Vec<f32> = Vec::new();
            core.process(&mut mono, &mut none);
            assert!(mono.iter().all(|s| s.is_finite()), "mono len {len}");
        }
    }

    /// Silence in, silence out; a tail that stays finite; and no setting
    /// a caller can reach turns the loop into a NaN that poisons every
    /// sample after it.
    #[test]
    fn silence_stays_silent_and_nothing_poisons_the_loop() {
        let mut params = patch();
        params.set(p::THRESHOLD, -60.0);
        params.set(p::RATIO, 2.0);
        params.set(p::MAKEUP, 24.0);
        let mut core = GlueCore::new(FS, &params);
        let mut l = vec![0.0f32; 512];
        let mut r = vec![0.0f32; 512];
        core.process(&mut l, &mut r);
        assert!(l.iter().all(|s| *s == 0.0), "silence in, silence out");

        for (threshold, ratio, attack, release, makeup, mix, range, hp) in [
            (
                -60.0f32, 0.0f32, 0.0f32, 0.0f32, 24.0f32, 0.0f32, 0.0f32, 20.0f32,
            ),
            (10.0, 2.0, 6.0, 6.0, -12.0, 100.0, 60.0, 2_000.0),
            (
                f32::NAN,
                f32::NAN,
                f32::NAN,
                f32::NAN,
                f32::NAN,
                f32::NAN,
                f32::NAN,
                f32::NAN,
            ),
            (f32::INFINITY, 99.0, 99.0, 99.0, 1e9, 1e9, 1e9, 1e9),
        ] {
            let mut core = GlueCore::new(FS, &patch());
            for (id, v) in [
                (p::THRESHOLD, threshold),
                (p::RATIO, ratio),
                (p::ATTACK, attack),
                (p::RELEASE, release),
                (p::MAKEUP, makeup),
                (p::DRY_WET, mix),
                (p::RANGE, range),
                (p::SC_HP, hp),
            ] {
                core.set_param(id, v);
            }
            let mut l: Vec<f32> = (0..256).map(|i| (i as f32 * 0.3).sin() * 0.5).collect();
            let mut r = l.clone();
            for _ in 0..8 {
                core.process(&mut l, &mut r);
                assert!(
                    l.iter().all(|s| s.is_finite()),
                    "threshold {threshold} ratio {ratio} makeup {makeup}"
                );
            }
            assert!(core.reduction_db().is_finite());
        }
    }
}
