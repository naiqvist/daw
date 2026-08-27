//! The eight-band equaliser — the effect half of `Node::Eq`.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic here belongs
//! to [`crate::dsp::filters`], and this file's whole job is to say which
//! kernel a band's TYPE selects, when its coefficients are rebuilt, and in
//! what order the eight of them run.
//!
//! # Which kernel a type selects
//!
//! | type | kernel |
//! |---|---|
//! | lo cut 12 / 48, hi cut 12 / 48 | [`Cascade`](crate::dsp::filters::Cascade) at 2 or 8 poles |
//! | lo shelf, bell, hi shelf | [`EqBand`](crate::dsp::filters::EqBand) |
//! | notch | [`Svf`](crate::dsp::filters::Svf) in [`Mode::Notch`](crate::dsp::filters::Mode::Notch) |
//!
//! All three live in every band slot rather than behind an enum. They are
//! POD and small, the unused two cost nothing to carry, and a type switch
//! is then a branch rather than a variant swap in the red zone.
//!
//! # Two rates
//!
//! Coefficients are rebuilt PER SEGMENT and only for a band whose settings
//! actually moved — a `tan` per changed band per segment, nothing for a
//! parked equaliser. What keeps a dragged handle from stepping is the
//! [`Smoother`](crate::dsp::ramps::Smoother) in front of each continuous
//! value, exactly as the kernel contract prescribes: kernels do not smooth
//! their own parameters, the caller wires a ramp in front.
//!
//! A segment is a few milliseconds, so the coefficient update rate is
//! ~187 Hz at 256 frames. That is coarse for a filter being SWEPT, which
//! is why `Node::Filter` rebuilds every sixteen samples — but an
//! equaliser band is set, not performed, and the smoother makes each step
//! small enough that the trapezoidal structure absorbs it. Everything
//! [`Svf`](crate::dsp::filters::Svf)'s header says about surviving moving
//! coefficients is the reason this is safe at all.
//!
//! # Red zone
//!
//! Everything except [`EqCore::new`] runs in the audio callback: no
//! allocation, no locks, no panic paths. The scratch the smoothers write
//! through is allocated once, at compile, and sized to the longest
//! segment the engine will ever hand over.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::filters::{BandShape, Cascade, EqBand, Mode, Svf};
use crate::dsp::ramps::Smoother;
use crate::params::eq as p;

/// How long a moved control takes to arrive, in ms. The filter's figure,
/// for the filter's reason — every device in one rack smoothing at a
/// different speed is a difference you would hear without being able to
/// name it.
const SMOOTH_MS: f32 = 15.0;

/// How far a value must move before its band's coefficients are rebuilt.
///
/// Relative for the frequency, absolute for the rest: a hertz at 30 Hz is
/// a musical step and a hertz at 16 kHz is nothing, so the threshold has
/// to scale the way hearing does.
const FREQ_EPS: f32 = 1e-4;
const GAIN_EPS: f32 = 1e-3;
const Q_EPS: f32 = 1e-4;

/// One band's settings, in ENGINE units — Hz, dB, a true Q, and the two
/// discretes as the floats every table row is.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BandParams {
    /// Above 0.5 the band runs. A float because every row of every
    /// parameter table is one, and a letter carries floats.
    pub on: f32,
    /// An index into [`crate::params::eq::TYPE_NAMES`].
    pub shape: f32,
    pub freq_hz: f32,
    pub gain_db: f32,
    pub q: f32,
}

impl Default for BandParams {
    fn default() -> Self {
        // Band 1's row, which is the only one a bare `default()` can mean.
        Self::at(0)
    }
}

impl BandParams {
    /// Band `band` at its table defaults — the frequency each band opens
    /// at is its own, so this is per band rather than one shared row.
    pub fn at(band: usize) -> Self {
        let d =
            |slot: u32| crate::params::def(p::TABLE, p::id(band.min(p::BANDS - 1), slot)).default;
        Self {
            on: d(p::ON),
            shape: d(p::TYPE),
            freq_hz: d(p::FREQ),
            gain_db: d(p::GAIN),
            q: d(p::Q),
        }
    }

    /// Whether this band contributes anything. A switched-off band is
    /// skipped entirely — no kernel runs, no coefficients are built.
    pub fn enabled(self) -> bool {
        self.on >= 0.5
    }

    /// The type index, clamped into the list. RON round-trips NaN, so a
    /// hand-edited project can smuggle one in and this is where it stops.
    pub fn shape_index(self) -> u32 {
        if self.shape.is_finite() {
            (self.shape.round().max(0.0) as u32).min(p::TYPE_NAMES.len() as u32 - 1)
        } else {
            p::TYPE_BELL
        }
    }
}

/// The whole equaliser's editable state.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct EqParams {
    pub bands: [BandParams; p::BANDS],
    /// The output trim, in dB.
    pub out_db: f32,
}

impl Default for EqParams {
    fn default() -> Self {
        let mut bands = [BandParams::at(0); p::BANDS];
        for (band, slot) in bands.iter_mut().enumerate() {
            *slot = BandParams::at(band);
        }
        Self {
            bands,
            out_db: crate::params::def(p::TABLE, p::OUT).default,
        }
    }
}

impl EqParams {
    /// This patch's value for a wire id, or `None` for one it does not
    /// have — which is how a target aimed at the wrong device is refused.
    pub fn get(&self, param: u32) -> Option<f32> {
        if param == p::OUT {
            return Some(self.out_db);
        }
        let (band, slot) = p::split(param)?;
        let band = self.bands.get(band)?;
        Some(match slot {
            p::ON => band.on,
            p::TYPE => band.shape,
            p::FREQ => band.freq_hz,
            p::GAIN => band.gain_db,
            p::Q => band.q,
            _ => return None,
        })
    }

    /// Write a wire id's value. Unknown ids are dropped, never guessed at.
    pub fn set(&mut self, param: u32, value: f32) {
        if param == p::OUT {
            self.out_db = value;
            return;
        }
        let Some((band, slot)) = p::split(param) else {
            return;
        };
        let Some(band) = self.bands.get_mut(band) else {
            return;
        };
        match slot {
            p::ON => band.on = value,
            p::TYPE => band.shape = value,
            p::FREQ => band.freq_hz = value,
            p::GAIN => band.gain_db = value,
            p::Q => band.q = value,
            _ => {}
        }
    }
}

/// The three kernels one band slot can be, and the state each keeps.
///
/// Carried together rather than as an enum: they are POD, the pair costs
/// under 200 bytes, and a type switch becomes a branch instead of a
/// variant swap with a reference to invalidate.
#[derive(Debug, Clone, Copy)]
struct BandKernels {
    cut: Cascade,
    bell: EqBand,
    notch: Svf,
}

impl BandKernels {
    fn new() -> Self {
        Self {
            cut: Cascade::new(),
            bell: EqBand::new(),
            notch: Svf::new(),
        }
    }

    fn reset(&mut self) {
        self.cut.reset();
        self.bell.reset();
        self.notch.reset();
    }

    /// Green- or red-zone: bounded pure math, a handful of `tan` calls.
    fn prepare(&mut self, sample_rate: f32, shape: u32, hz: f32, gain_db: f32, q: f32) {
        let order = p::cut_order(shape);
        if order > 0 {
            self.cut
                .prepare(sample_rate, hz, q, order, p::is_highpass(shape));
        } else if shape == p::TYPE_NOTCH {
            self.notch.prepare(sample_rate, hz, q);
        } else {
            let curve = match shape {
                p::TYPE_LO_SHELF => BandShape::LowShelf,
                p::TYPE_HI_SHELF => BandShape::HighShelf,
                _ => BandShape::Bell,
            };
            self.bell.prepare(sample_rate, hz, q, gain_db, curve);
        }
    }

    /// Red zone: run whichever kernel this band's type selected.
    fn process(&mut self, shape: u32, io: &mut [f32]) {
        if p::cut_order(shape) > 0 {
            self.cut.process(io);
        } else if shape == p::TYPE_NOTCH {
            self.notch.process(io, Mode::Notch);
        } else {
            self.bell.process(io);
        }
    }
}

/// What one band was last prepared with, so an unmoved band rebuilds
/// nothing.
#[derive(Debug, Clone, Copy)]
struct Prepared {
    shape: u32,
    hz: f32,
    gain_db: f32,
    q: f32,
}

/// One band's smoothers and both channels' kernel state.
struct Band {
    /// `[channel]`. Coefficients are identical either side; the STATE is
    /// not, and state shared between two channels is the two channels
    /// leaking into each other.
    kernels: [BandKernels; 2],
    freq: Smoother,
    gain: Smoother,
    q: Smoother,
    freq_target: f32,
    gain_target: f32,
    q_target: f32,
    shape: u32,
    on: bool,
    prepared: Prepared,
}

/// The equaliser's kernel chain, boxed so `Node` stays lean.
pub struct EqCore {
    bands: [Band; p::BANDS],
    out: Smoother,
    out_target: f32,
    /// The output trim as a LINEAR gain, where the last segment left it.
    out_gain: f32,
    /// One lane, block-long: what the smoothers write through so their
    /// segment-end value can be read. Allocated once, at compile.
    scratch: Vec<f32>,
    sample_rate: f32,
}

impl EqCore {
    /// GREEN ZONE. Builds every smoother and the one scratch lane the
    /// callback will ever need; `block_frames` is the longest segment
    /// there can be.
    pub fn new(sample_rate: f32, block_frames: usize, params: &EqParams) -> Self {
        let smoother = |value: f32| {
            let mut s = Smoother::new();
            s.prepare(sample_rate, SMOOTH_MS);
            s.set_now(value);
            s
        };
        let band_at = |band: usize| {
            let set = params.bands.get(band).copied().unwrap_or_default();
            let clamp =
                |slot: u32, v: f32| crate::params::def(p::TABLE, p::id(band, slot)).clamp(v);
            let shape = set.shape_index();
            let hz = clamp(p::FREQ, set.freq_hz);
            let gain_db = clamp(p::GAIN, set.gain_db);
            let q = clamp(p::Q, set.q);
            let mut kernels = [BandKernels::new(); 2];
            for channel in kernels.iter_mut() {
                channel.prepare(sample_rate, shape, hz, gain_db, q);
            }
            Band {
                kernels,
                freq: smoother(hz),
                gain: smoother(gain_db),
                q: smoother(q),
                freq_target: hz,
                gain_target: gain_db,
                q_target: q,
                shape,
                on: set.enabled(),
                prepared: Prepared {
                    shape,
                    hz,
                    gain_db,
                    q,
                },
            }
        };
        let out_db = crate::params::def(p::TABLE, p::OUT).clamp(params.out_db);
        Self {
            bands: std::array::from_fn(band_at),
            out: smoother(out_db),
            out_target: out_db,
            out_gain: db_to_gain(out_db),
            scratch: vec![0.0; block_frames.max(1)],
            sample_rate,
        }
    }

    /// Red zone: a letter. Retargets a smoother, or lands a discrete
    /// straight away — an on/off or a type is not a value to glide to.
    pub fn set_param(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(p::TABLE, param, value) else {
            return;
        };
        if param == p::OUT {
            self.out_target = value;
            self.out.set_target(value);
            return;
        }
        let Some((index, slot)) = p::split(param) else {
            return;
        };
        let Some(band) = self.bands.get_mut(index) else {
            return;
        };
        match slot {
            // A band arriving or leaving starts from silence rather than
            // from whatever its filters were ringing with when it was
            // last switched off — a cascade carrying old history into a
            // fresh block is a thump.
            p::ON => {
                let on = value >= 0.5;
                if on != band.on {
                    band.on = on;
                    for channel in band.kernels.iter_mut() {
                        channel.reset();
                    }
                }
            }
            // Same rule for a type switch, and the same reason: lowpass
            // history in a highpass is a thump. Forcing the prepared
            // frequency off its value is what makes the rebuild happen.
            p::TYPE => {
                let shape = if value.is_finite() {
                    (value.round().max(0.0) as u32).min(p::TYPE_NAMES.len() as u32 - 1)
                } else {
                    p::TYPE_BELL
                };
                if shape != band.shape {
                    band.shape = shape;
                    for channel in band.kernels.iter_mut() {
                        channel.reset();
                    }
                    band.prepared.hz = f32::NEG_INFINITY;
                }
            }
            p::FREQ => {
                band.freq_target = value;
                band.freq.set_target(value);
            }
            p::GAIN => {
                band.gain_target = value;
                band.gain.set_target(value);
            }
            p::Q => {
                band.q_target = value;
                band.q.set_target(value);
            }
            _ => {}
        }
    }

    /// Red zone: a seek. Clear every filter's memory and land every
    /// control on its target, so the new position starts clean and does
    /// not glide the old one's knob motion in.
    pub fn snap(&mut self) {
        for band in self.bands.iter_mut() {
            for channel in band.kernels.iter_mut() {
                channel.reset();
            }
            band.freq.set_now(band.freq_target);
            band.gain.set_now(band.gain_target);
            band.q.set_now(band.q_target);
        }
        self.out.set_now(self.out_target);
        self.out_gain = db_to_gain(self.out_target);
    }

    /// Red zone: run the whole equaliser over one segment, in place.
    ///
    /// `r` may be empty for a mono caller; every band then runs once.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(self.scratch.len());
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;
        let sample_rate = self.sample_rate;
        for band in self.bands.iter_mut() {
            // The smoothers run whether or not the band does: a band
            // switched on mid-drag must arrive at the value the control
            // is showing, not at the one it was parked on.
            let Some(lane) = self.scratch.get_mut(..n) else {
                return;
            };
            band.freq.process(lane);
            let hz = lane.last().copied().unwrap_or(band.freq_target);
            band.gain.process(lane);
            let gain_db = lane.last().copied().unwrap_or(band.gain_target);
            band.q.process(lane);
            let q = lane.last().copied().unwrap_or(band.q_target);
            if !band.on {
                continue;
            }
            let moved = (hz - band.prepared.hz).abs() > band.prepared.hz.abs() * FREQ_EPS
                || (gain_db - band.prepared.gain_db).abs() > GAIN_EPS
                || (q - band.prepared.q).abs() > Q_EPS
                || band.shape != band.prepared.shape;
            if moved {
                band.prepared = Prepared {
                    shape: band.shape,
                    hz,
                    gain_db,
                    q,
                };
                for channel in band.kernels.iter_mut() {
                    channel.prepare(sample_rate, band.shape, hz, gain_db, q);
                }
            }
            let shape = band.shape;
            if let Some(left) = band.kernels.first_mut() {
                left.process(shape, &mut l[..n]);
            }
            if stereo && let Some(right) = band.kernels.get_mut(1) {
                right.process(shape, &mut r[..n]);
            }
        }

        // The trim, ramped across the segment from wherever the last one
        // left it — a level that stepped once per segment would be a
        // click at every boundary.
        let Some(lane) = self.scratch.get_mut(..n) else {
            return;
        };
        self.out.process(lane);
        let target = db_to_gain(lane.last().copied().unwrap_or(self.out_target));
        let start = self.out_gain;
        if start != 1.0 || target != 1.0 {
            let step = (target - start) / n as f32;
            let mut g = start;
            for (i, s) in l[..n].iter_mut().enumerate() {
                g = start + step * i as f32;
                *s *= g;
            }
            if stereo {
                for (i, s) in r[..n].iter_mut().enumerate() {
                    *s *= start + step * i as f32;
                }
            }
            let _ = g;
        }
        self.out_gain = target;
    }
}

/// Decibels as a linear gain. Bounded and finite for anything a table row
/// can hold.
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

    fn flat() -> EqParams {
        EqParams::default()
    }

    /// Every id in the table round-trips through the patch, and nothing
    /// else does. A get/set pair that disagreed would send a band's Q to
    /// its neighbour's gain and nothing would fail to compile.
    #[test]
    fn every_table_row_round_trips_and_no_other_id_does() {
        let mut params = flat();
        for (i, def) in p::TABLE.iter().enumerate() {
            assert_eq!(def.id as usize, i, "ids index their own row");
            let probe = def.min + (def.max - def.min) * 0.37;
            params.set(def.id, probe);
            assert_eq!(
                params.get(def.id),
                Some(probe),
                "{} did not round-trip",
                def.name
            );
        }
        // Every OTHER row must still hold what it held: a write that
        // leaked into a neighbour is exactly what this table exists to
        // make impossible.
        for def in p::TABLE {
            let probe = def.min + (def.max - def.min) * 0.37;
            assert_eq!(
                params.get(def.id),
                Some(probe),
                "{} was overwritten",
                def.name
            );
        }
        assert_eq!(
            params.get(p::OUT.wrapping_add(1)),
            None,
            "an id past the end"
        );
        assert_eq!(params.get(u32::MAX), None);
    }

    /// A default equaliser is a WIRE. Every band opens switched off, so a
    /// freshly loaded EQ must return the signal bit for bit.
    #[test]
    fn a_default_equaliser_is_bit_exact_silence_of_opinion() {
        let mut core = EqCore::new(FS, 256, &flat());
        let input: Vec<f32> = (0..256).map(|i| (i as f32 * 0.07).sin() * 0.5).collect();
        let (mut l, mut r) = (input.clone(), input.clone());
        core.process(&mut l, &mut r);
        assert!(
            l.iter()
                .zip(&input)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "a flat EQ changed the signal"
        );
        assert!(
            r.iter()
                .zip(&input)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
    }

    /// A band that is switched on does what its row says, and switching
    /// it off puts the signal back exactly.
    #[test]
    fn a_band_only_acts_when_it_is_switched_on() {
        let hz = 1_000.0;
        let measure = |on: f32| {
            let mut params = flat();
            params.set(p::id(3, p::ON), on);
            params.set(p::id(3, p::TYPE), p::TYPE_BELL as f32);
            params.set(p::id(3, p::FREQ), hz);
            params.set(p::id(3, p::GAIN), 12.0);
            params.set(p::id(3, p::Q), 2.0);
            let mut core = EqCore::new(FS, 4096, &params);
            let n = 16_384;
            let mut l: Vec<f32> = (0..n)
                .map(|i| (i as f32 / FS * hz * core::f32::consts::TAU).sin())
                .collect();
            let mut r = vec![0.0f32; 0];
            for chunk in l.chunks_mut(4096) {
                core.process(chunk, &mut r);
            }
            let tail = &l[n / 2..];
            let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
            20.0 * (rms * core::f32::consts::SQRT_2).max(1e-9).log10()
        };
        let off = measure(0.0);
        let on = measure(1.0);
        assert!(
            off.abs() < 0.01,
            "a band switched off must pass: {off:+.3} dB"
        );
        assert!(
            (on - 12.0).abs() < 0.4,
            "a +12 dB bell measured {on:+.2} dB at its centre"
        );
    }

    /// Split-block equivalence, which the segmented transport depends on:
    /// one segment of 256 must equal 100 then 156, sample for sample.
    #[test]
    fn segmenting_a_block_changes_nothing() {
        let mut params = flat();
        for (band, shape) in [
            (0usize, p::TYPE_LO_CUT_12),
            (1, p::TYPE_LO_SHELF),
            (2, p::TYPE_BELL),
            (3, p::TYPE_NOTCH),
            (4, p::TYPE_HI_SHELF),
            (5, p::TYPE_HI_CUT_48),
        ] {
            params.set(p::id(band, p::ON), 1.0);
            params.set(p::id(band, p::TYPE), shape as f32);
            params.set(p::id(band, p::GAIN), 7.0);
        }
        params.set(p::OUT, 3.0);
        let input: Vec<f32> = (0..256).map(|i| (i as f32 * 0.11).sin()).collect();

        let mut whole_core = EqCore::new(FS, 256, &params);
        let (mut whole_l, mut whole_r) = (input.clone(), input.clone());
        whole_core.process(&mut whole_l, &mut whole_r);

        let mut split_core = EqCore::new(FS, 256, &params);
        let (mut split_l, mut split_r) = (input.clone(), input.clone());
        {
            let (l0, l1) = split_l.split_at_mut(100);
            let (r0, r1) = split_r.split_at_mut(100);
            split_core.process(l0, r0);
            split_core.process(l1, r1);
        }
        assert!(
            whole_l
                .iter()
                .zip(&split_l)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "256 must equal 100 + 156"
        );
        assert!(
            whole_r
                .iter()
                .zip(&split_r)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
    }

    /// The red-zone path allocates nothing — letters, seeks and every
    /// band type included.
    #[test]
    fn the_callback_path_does_not_allocate() {
        let mut params = flat();
        for band in 0..p::BANDS {
            params.set(p::id(band, p::ON), 1.0);
        }
        let mut core = EqCore::new(FS, 256, &params);
        let mut l = vec![0.25f32; 256];
        let mut r = vec![0.25f32; 256];

        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..64 {
                for band in 0..p::BANDS {
                    core.set_param(p::id(band, p::TYPE), (i % 8) as f32);
                    core.set_param(p::id(band, p::FREQ), 100.0 + i as f32 * 200.0);
                    core.set_param(p::id(band, p::GAIN), i as f32 * 0.25 - 8.0);
                    core.set_param(p::id(band, p::Q), 0.2 + i as f32 * 0.1);
                    core.set_param(p::id(band, p::ON), (i % 3 != 0) as u32 as f32);
                }
                core.set_param(p::OUT, i as f32 * 0.1 - 3.0);
                if i % 5 == 0 {
                    core.snap();
                }
                core.process(&mut l, &mut r);
            }
        });
    }

    /// Any block length, including none and one, and a mono caller with
    /// no right channel at all.
    #[test]
    fn any_segment_length_is_accepted() {
        let mut params = flat();
        for band in 0..p::BANDS {
            params.set(p::id(band, p::ON), 1.0);
            params.set(p::id(band, p::TYPE), (band as u32) as f32);
            params.set(p::id(band, p::GAIN), -6.0);
        }
        let mut core = EqCore::new(FS, 256, &params);
        for len in [0usize, 1, 3, 7, 63, 100, 256] {
            let mut l = vec![0.25f32; len];
            let mut r = vec![0.25f32; len];
            core.process(&mut l, &mut r);
            assert!(l.iter().all(|s| s.is_finite()), "len {len}");
            // Mono: no right channel to write.
            let mut mono = vec![0.25f32; len];
            let mut none: Vec<f32> = Vec::new();
            core.process(&mut mono, &mut none);
            assert!(mono.iter().all(|s| s.is_finite()), "mono len {len}");
        }
    }

    /// Silence in, silence out, and a tail that never becomes NaN — even
    /// with every band ringing at the settings most likely to blow up.
    #[test]
    fn silence_stays_silent_and_tails_stay_finite() {
        let mut params = flat();
        for band in 0..p::BANDS {
            params.set(p::id(band, p::ON), 1.0);
            params.set(p::id(band, p::FREQ), 25.0);
            params.set(p::id(band, p::Q), p::MAX_Q);
            params.set(p::id(band, p::GAIN), p::MAX_GAIN_DB);
        }
        let mut core = EqCore::new(FS, 256, &params);
        let mut l = vec![0.0f32; 256];
        let mut r = vec![0.0f32; 256];
        core.process(&mut l, &mut r);
        assert!(l.iter().all(|s| *s == 0.0), "silence in, silence out");

        let mut tail = vec![0.0f32; 256];
        tail[0] = 1.0;
        let mut tail_r = vec![0.0f32; 256];
        core.process(&mut tail, &mut tail_r);
        for _ in 0..1_000 {
            let mut quiet = vec![0.0f32; 256];
            let mut quiet_r = vec![0.0f32; 256];
            core.process(&mut quiet, &mut quiet_r);
            assert!(quiet.iter().all(|s| s.is_finite()), "the tail went bad");
        }
    }
}
