//! sCOMP — a sine, bounced through a compressor until it is a sound.
//!
//! The technique this instrument is: start from a pure fundamental,
//! drive it into a compressor and a soft clipper hard enough that the
//! peaks flatten and harmonics appear; put a resonant filter in front
//! so one region is always being pushed over the threshold, and move
//! that filter, so the dynamics never settle; bounce the result to a
//! file, pitch it, and do it again. Every pass adds grit and metal and
//! keeps the sub underneath, because a compressor cannot take away the
//! fundamental it is reacting to.
//!
//! Here the bounce happens inside the instrument. A TAKE is rendered
//! offline from the knobs — the sine, then each pass over the pass
//! before it — and the voices play the last pass like a sample, pitched
//! by the note. The passes are kept, all of them, because the picture
//! of how a sine became a sound is the instrument's own explanation of
//! itself, and the forge draws them stacked.
//!
//! Green zone, entirely: this module allocates, and is called at
//! compile and by the forge. The audio thread only ever reads a take.
//! No audio types are named, so the stage may call it.

use std::sync::Arc;

use crate::dsp::arith::{db_to_gain, gain_to_db};
use crate::dsp::delay::FeedbackDelay;
use crate::dsp::dynamics::{Ballistics, GainComputer, Mode as CompMode};
use crate::dsp::filters::{DcBlocker, Mode as FilterMode, Svf};
use crate::dsp::interp::hermite_at;
use crate::dsp::shaper::{Mode as ShapeMode, Waveshaper};
use crate::params::scomp as p;

/// The knobs, in engine units. Every row of the table, so a device's
/// overrides map onto it one to one.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ScompParams {
    pub passes: f32,
    pub take_s: f32,
    pub drop_st: f32,
    pub drop_ms: f32,
    pub decay_s: f32,
    pub filter: f32,
    pub harmonic: f32,
    pub reso: f32,
    pub sweep_oct: f32,
    pub drift_st: f32,
    pub thresh_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_db: f32,
    pub drive: f32,
    pub shift_st: f32,
    pub amp_a_ms: f32,
    pub amp_r_ms: f32,
    pub tune_st: f32,
    pub root: f32,
    pub level: f32,
}

impl Default for ScompParams {
    fn default() -> Self {
        let d = |id: u32| {
            p::TABLE
                .iter()
                .find(|def| def.id == id)
                .map(|def| def.default)
                .unwrap_or(0.0)
        };
        Self {
            passes: d(p::PASSES),
            take_s: d(p::TAKE),
            drop_st: d(p::DROP),
            drop_ms: d(p::DROP_MS),
            decay_s: d(p::DECAY),
            filter: d(p::FILTER),
            harmonic: d(p::HARMONIC),
            reso: d(p::RESO),
            sweep_oct: d(p::SWEEP),
            drift_st: d(p::DRIFT),
            thresh_db: d(p::THRESH),
            ratio: d(p::RATIO),
            attack_ms: d(p::ATTACK),
            release_ms: d(p::RELEASE),
            makeup_db: d(p::MAKEUP),
            drive: d(p::DRIVE),
            shift_st: d(p::SHIFT),
            amp_a_ms: d(p::AMP_A),
            amp_r_ms: d(p::AMP_R),
            tune_st: d(p::TUNE),
            root: d(p::ROOT),
            level: d(p::LEVEL),
        }
    }
}

impl ScompParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(def) = p::TABLE.iter().find(|def| def.id == param) else {
            return;
        };
        let value = def.clamp(value);
        match param {
            p::PASSES => self.passes = value,
            p::TAKE => self.take_s = value,
            p::DROP => self.drop_st = value,
            p::DROP_MS => self.drop_ms = value,
            p::DECAY => self.decay_s = value,
            p::FILTER => self.filter = value,
            p::HARMONIC => self.harmonic = value,
            p::RESO => self.reso = value,
            p::SWEEP => self.sweep_oct = value,
            p::DRIFT => self.drift_st = value,
            p::THRESH => self.thresh_db = value,
            p::RATIO => self.ratio = value,
            p::ATTACK => self.attack_ms = value,
            p::RELEASE => self.release_ms = value,
            p::MAKEUP => self.makeup_db = value,
            p::DRIVE => self.drive = value,
            p::SHIFT => self.shift_st = value,
            p::AMP_A => self.amp_a_ms = value,
            p::AMP_R => self.amp_r_ms = value,
            p::TUNE => self.tune_st = value,
            p::ROOT => self.root = value,
            p::LEVEL => self.level = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        Some(match param {
            p::PASSES => self.passes,
            p::TAKE => self.take_s,
            p::DROP => self.drop_st,
            p::DROP_MS => self.drop_ms,
            p::DECAY => self.decay_s,
            p::FILTER => self.filter,
            p::HARMONIC => self.harmonic,
            p::RESO => self.reso,
            p::SWEEP => self.sweep_oct,
            p::DRIFT => self.drift_st,
            p::THRESH => self.thresh_db,
            p::RATIO => self.ratio,
            p::ATTACK => self.attack_ms,
            p::RELEASE => self.release_ms,
            p::MAKEUP => self.makeup_db,
            p::DRIVE => self.drive,
            p::SHIFT => self.shift_st,
            p::AMP_A => self.amp_a_ms,
            p::AMP_R => self.amp_r_ms,
            p::TUNE => self.tune_st,
            p::ROOT => self.root,
            p::LEVEL => self.level,
            _ => return None,
        })
    }

    pub fn sanitize(&mut self) {
        for def in p::TABLE.iter() {
            if let Some(value) = self.get(def.id) {
                let fixed = if value.is_finite() {
                    value
                } else {
                    def.default
                };
                self.set(def.id, def.clamp(fixed));
            }
        }
    }

    /// How many passes the take goes through, as a count.
    pub fn pass_count(&self) -> usize {
        (self.passes.round().max(1.0) as usize).min(p::MAX_PASSES)
    }

    /// The hertz the take is rendered at: the root note.
    pub fn root_hz(&self) -> f32 {
        440.0 * ((self.root.clamp(0.0, 127.0) - 69.0) / 12.0).exp2()
    }

    /// The knobs that shape the take, and nothing else — so a change
    /// that is only a letter does not read as a new render.
    pub fn baked(&self) -> Baked {
        Baked {
            passes: self.pass_count(),
            take_s: self.take_s.to_bits(),
            drop_st: self.drop_st.to_bits(),
            drop_ms: self.drop_ms.to_bits(),
            decay_s: self.decay_s.to_bits(),
            filter: self.filter.round() as u8,
            harmonic: self.harmonic.to_bits(),
            reso: self.reso.to_bits(),
            sweep_oct: self.sweep_oct.to_bits(),
            drift_st: self.drift_st.to_bits(),
            thresh_db: self.thresh_db.to_bits(),
            ratio: self.ratio.to_bits(),
            attack_ms: self.attack_ms.to_bits(),
            release_ms: self.release_ms.to_bits(),
            makeup_db: self.makeup_db.to_bits(),
            drive: self.drive.to_bits(),
            shift_st: self.shift_st.to_bits(),
            root: self.root.to_bits(),
        }
    }
}

/// The baked knobs, comparable and hashable: the forge's cache key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Baked {
    passes: usize,
    take_s: u32,
    drop_st: u32,
    drop_ms: u32,
    decay_s: u32,
    filter: u8,
    harmonic: u32,
    reso: u32,
    sweep_oct: u32,
    drift_st: u32,
    thresh_db: u32,
    ratio: u32,
    attack_ms: u32,
    release_ms: u32,
    makeup_db: u32,
    drive: u32,
    shift_st: u32,
    root: u32,
}

/// A rendered take: the sine, then every pass over it. `passes[0]` is
/// the source; `passes.last()` is what the voices play.
#[derive(Debug, Clone)]
pub struct Take {
    pub passes: Vec<Arc<Vec<f32>>>,
    pub sample_rate: u32,
    pub root_hz: f32,
}

impl Take {
    /// A take of silence, for a voice that has nothing to play yet.
    pub fn silent(sample_rate: u32) -> Self {
        Self {
            passes: vec![Arc::new(vec![0.0; 64])],
            sample_rate,
            root_hz: 440.0,
        }
    }

    /// The pass the voices play.
    pub fn last(&self) -> Arc<Vec<f32>> {
        self.passes
            .last()
            .cloned()
            .unwrap_or_else(|| Arc::new(vec![0.0; 64]))
    }

    /// How long the played pass is, in seconds.
    pub fn seconds(&self) -> f64 {
        self.passes.last().map_or(0.0, |pass| {
            pass.len() as f64 / f64::from(self.sample_rate.max(1))
        })
    }
}

/// The source: a sine at the root, falling from `drop` semitones above
/// it over `drop_ms`, dying away over `decay`. Movement on purpose —
/// a compressor fed something that never changes has nothing to react
/// to, and the whole sound is in the reaction.
fn source(params: &ScompParams, sample_rate: f32) -> Vec<f32> {
    let frames = ((params.take_s.max(0.01) * sample_rate) as usize).max(64);
    let root = params.root_hz();
    let drop_tau = (params.drop_ms.max(1.0) / 1000.0) as f64;
    let decay = params.decay_s.max(0.01) as f64;
    let mut out = vec![0.0f32; frames];
    let mut phase = 0.0f64;
    for (i, slot) in out.iter_mut().enumerate() {
        let t = i as f64 / f64::from(sample_rate);
        let bend = f64::from(params.drop_st) * (-t / drop_tau).exp();
        let hz = f64::from(root) * (bend / 12.0).exp2();
        let amp = (-t / decay).exp();
        *slot = (phase.sin() * amp * 0.9) as f32;
        phase += std::f64::consts::TAU * hz / f64::from(sample_rate);
        if phase > std::f64::consts::TAU {
            phase -= std::f64::consts::TAU;
        }
    }
    out
}

/// The filter's centre for pass `k` (counted from one) at `t` seconds
/// into the take: the harmonic of the root, drifted a pass at a time,
/// swept across the take.
pub fn centre_hz(params: &ScompParams, k: usize, t: f32) -> f32 {
    let base = params.root_hz() * params.harmonic.max(0.01);
    let drift = (params.drift_st * (k.saturating_sub(1)) as f32 / 12.0).exp2();
    let sweep = (params.sweep_oct * (t / params.take_s.max(0.01)).clamp(0.0, 1.0)).exp2();
    base * drift * sweep
}

/// The filter's stage: a resonant peak added to the signal, a notch
/// taken out of it, or a comb through it — whichever, the point is a
/// region the dynamics will meet first.
enum Resonator {
    Band(Svf),
    Notch(Svf),
    Comb(FeedbackDelay, Vec<f32>),
}

/// One pass: filter, compress, clip, resample.
fn pass(input: &[f32], k: usize, params: &ScompParams, sample_rate: f32) -> Vec<f32> {
    let n = input.len();
    let mut buf = input.to_vec();
    let nyquist = sample_rate * 0.45;

    // FILTER, its centre recomputed every few samples so the sweep is a
    // glide rather than a staircase.
    let q = params.reso.max(0.5);
    let mut resonator = match params.filter.round() as u8 {
        1 => Resonator::Notch(Svf::new()),
        2 => {
            let max_delay = (sample_rate / 20.0).ceil() as usize + 4;
            let mut comb = FeedbackDelay::new();
            comb.prepare(sample_rate, max_delay, 9_000.0);
            comb.set_feedback((1.0 - 2.0 / (q + 2.0)).clamp(0.2, 0.96));
            comb.set_drive(1.0);
            Resonator::Comb(comb, vec![0.0; FeedbackDelay::needed_len(max_delay)])
        }
        _ => Resonator::Band(Svf::new()),
    };
    const HOP: usize = 32;
    let mut hop = vec![0.0f32; HOP];
    let mut at = 0usize;
    while at < n {
        let take = (n - at).min(HOP);
        let t = at as f32 / sample_rate;
        let hz = centre_hz(params, k, t).clamp(20.0, nyquist);
        let Some(chunk) = buf.get_mut(at..at + take) else {
            break;
        };
        match &mut resonator {
            Resonator::Band(svf) => {
                svf.prepare(sample_rate, hz, q);
                let Some(wet) = hop.get_mut(..take) else {
                    break;
                };
                wet.copy_from_slice(chunk);
                svf.process(wet, FilterMode::BandpassUnity);
                // The peak stands on the signal: the sub stays, and the
                // band is lifted toward the threshold.
                for (dst, w) in chunk.iter_mut().zip(wet.iter()) {
                    *dst += *w * (1.0 + q.sqrt() * 0.5);
                }
            }
            Resonator::Notch(svf) => {
                svf.prepare(sample_rate, hz, q);
                svf.process(chunk, FilterMode::Notch);
            }
            Resonator::Comb(comb, store) => {
                comb.set_delay(sample_rate / hz);
                let Some(wet) = hop.get_mut(..take) else {
                    break;
                };
                wet.copy_from_slice(chunk);
                comb.process(wet, store);
                for (dst, w) in chunk.iter_mut().zip(wet.iter()) {
                    *dst = 0.5 * (*dst + *w);
                }
            }
        }
        at += take;
    }

    // COMPRESS, feedforward on the peak, fast enough to chew.
    let mut computer = GainComputer::new();
    computer.configure(CompMode::Compress, params.thresh_db, params.ratio, 6.0);
    let mut ballistics = Ballistics::new();
    ballistics.prepare(sample_rate, params.attack_ms, params.release_ms);
    let makeup = db_to_gain(params.makeup_db);
    for s in buf.iter_mut() {
        let level_db = gain_to_db(s.abs().max(1.0e-6));
        let target = computer.gain_db(level_db);
        let g = ballistics.tick(target);
        *s *= db_to_gain(g) * makeup;
    }

    // CLIP: the flattened peaks are where the harmonics come from.
    let mut shaper = Waveshaper::new();
    shaper.configure(ShapeMode::SoftClip, params.drive, 0.0, 1.0);
    shaper.process(&mut buf);

    // RESAMPLE: the bounce, played back at another speed. Down is
    // longer and darker; up is shorter and brighter. The take may not
    // grow without bound, so a deep dive is cut at the limit.
    let ratio = (params.shift_st / 12.0).exp2().max(1.0e-3) as f64;
    let longest = (p::MAX_TAKE_S * sample_rate) as usize;
    let out_len = ((n as f64 / ratio).round() as usize).clamp(64, longest);
    let mut out = vec![0.0f32; out_len];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = hermite_at(&buf, i as f64 * ratio);
    }

    // A compressor leaning on an asymmetric wave leaves an offset; the
    // next pass would compress the offset. Take it out.
    let mut dc = DcBlocker::new();
    dc.prepare(sample_rate);
    dc.process(&mut out);
    for s in out.iter_mut() {
        if !s.is_finite() {
            *s = 0.0;
        }
    }
    out
}

/// Render the take: the sine, then every pass over the pass before.
pub fn render(params: &ScompParams, sample_rate: u32) -> Take {
    let mut params = *params;
    params.sanitize();
    let sr = if sample_rate == 0 {
        48_000.0
    } else {
        sample_rate as f32
    };
    let mut passes: Vec<Arc<Vec<f32>>> = Vec::with_capacity(params.pass_count() + 1);
    passes.push(Arc::new(source(&params, sr)));
    for k in 1..=params.pass_count() {
        let previous = passes.last().cloned().unwrap_or_default();
        passes.push(Arc::new(pass(&previous, k, &params, sr)));
    }
    Take {
        passes,
        sample_rate: if sample_rate == 0 {
            48_000
        } else {
            sample_rate
        },
        root_hz: params.root_hz(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(xs: &[f32]) -> f32 {
        xs.iter().fold(0.0f32, |m, x| m.max(x.abs()))
    }

    /// The nearest thing to a spectrum a test can afford: the share of
    /// the signal's energy that is not the fundamental, read by fitting
    /// the fundamental out of it.
    fn harmonic_share(xs: &[f32], hz: f32, sr: f32) -> f32 {
        let n = xs.len().min((sr * 0.5) as usize);
        let (mut a, mut b) = (0.0f64, 0.0f64);
        for (i, x) in xs.iter().take(n).enumerate() {
            let ph = std::f64::consts::TAU * f64::from(hz) * i as f64 / f64::from(sr);
            a += f64::from(*x) * ph.cos();
            b += f64::from(*x) * ph.sin();
        }
        let (a, b) = (a * 2.0 / n as f64, b * 2.0 / n as f64);
        let mut fitted = 0.0f64;
        let mut total = 0.0f64;
        for (i, x) in xs.iter().take(n).enumerate() {
            let ph = std::f64::consts::TAU * f64::from(hz) * i as f64 / f64::from(sr);
            let f = a * ph.cos() + b * ph.sin();
            fitted += (f64::from(*x) - f).powi(2);
            total += f64::from(*x).powi(2);
        }
        if total <= 0.0 {
            0.0
        } else {
            (fitted / total) as f32
        }
    }

    #[test]
    fn every_row_round_trips_and_nonsense_is_sanitized() {
        let mut params = ScompParams::default();
        for def in p::TABLE {
            params.set(def.id, def.max);
            assert_eq!(params.get(def.id), Some(def.max), "{}", def.name);
            params.set(def.id, def.min - 100.0);
            assert_eq!(params.get(def.id), Some(def.min), "{} floor", def.name);
        }
        params.take_s = f32::NAN;
        params.ratio = f32::INFINITY;
        params.sanitize();
        assert!(params.take_s.is_finite() && params.ratio.is_finite());
        assert_eq!(ScompParams::default().pass_count(), 3);
        assert!(
            (ScompParams::default().root_hz() - 65.406).abs() < 0.01,
            "C1"
        );
    }

    #[test]
    fn the_source_is_a_sine_and_the_passes_make_harmonics_of_it() {
        let mut params = ScompParams::default();
        params.drop_st = 0.0;
        params.take_s = 0.5;
        params.decay_s = 4.0;
        params.shift_st = 0.0;
        params.sweep_oct = 0.0;
        params.root = 45.0; // A2, 110 Hz
        let take = render(&params, 48_000);
        assert_eq!(take.passes.len(), 4, "the source and three passes");
        let sine = &take.passes[0];
        assert!(
            harmonic_share(sine, 110.0, 48_000.0) < 0.02,
            "the source is not a sine"
        );
        let last = take.last();
        assert_eq!(last.len(), sine.len(), "no shift, so no change of length");
        let share = harmonic_share(&last, 110.0, 48_000.0);
        assert!(share > 0.1, "three passes left the sine a sine: {share}");
        // The clipper holds the rails; the resampler's interpolation and
        // the DC blocker may lean a little past them.
        assert!(
            peak(&last) <= 1.2,
            "the clipper let it over: {}",
            peak(&last)
        );
        assert!(peak(&last) > 0.3, "the passes silenced it: {}", peak(&last));
        assert!(last.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn a_shift_down_lengthens_the_take_and_the_limit_holds() {
        let mut params = ScompParams::default();
        params.take_s = 0.5;
        params.shift_st = -12.0;
        params.passes = 1.0;
        let take = render(&params, 48_000);
        let (a, b) = (take.passes[0].len(), take.passes[1].len());
        assert!((b as f64 / a as f64 - 2.0).abs() < 0.01, "{a} -> {b}");
        params.passes = 8.0;
        params.take_s = 4.0;
        let take = render(&params, 48_000);
        let longest = take.passes.iter().map(|pass| pass.len()).max().unwrap_or(0);
        assert!(longest <= (p::MAX_TAKE_S * 48_000.0) as usize, "{longest}");
        assert!((take.seconds() - p::MAX_TAKE_S as f64).abs() < 0.01);
    }

    #[test]
    fn every_filter_renders_finite_and_the_centre_drifts_and_sweeps() {
        for filter in [p::FILTER_BAND, p::FILTER_NOTCH, p::FILTER_COMB] {
            let mut params = ScompParams::default();
            params.filter = filter;
            params.take_s = 0.25;
            let take = render(&params, 44_100);
            for pass in &take.passes {
                assert!(pass.iter().all(|x| x.is_finite()), "filter {filter}");
                assert!(peak(pass) > 0.05, "filter {filter} went silent");
            }
        }
        let params = ScompParams::default();
        let start = centre_hz(&params, 1, 0.0);
        let end = centre_hz(&params, 1, params.take_s);
        assert!((end / start - (params.sweep_oct).exp2()).abs() < 1e-3);
        let next = centre_hz(&params, 2, 0.0);
        assert!((next / start - (params.drift_st / 12.0).exp2()).abs() < 1e-3);
        assert!((start - params.root_hz() * params.harmonic).abs() < 1e-3);
    }

    #[test]
    fn only_the_baked_rows_change_the_key() {
        let mut a = ScompParams::default();
        let key = a.baked();
        a.set(p::LEVEL, 1.5);
        a.set(p::TUNE, 3.0);
        a.set(p::AMP_R, 400.0);
        assert_eq!(a.baked(), key, "a letter read as a render");
        a.set(p::DRIVE, 9.0);
        assert_ne!(a.baked(), key);
        for def in p::TABLE {
            assert_eq!(
                p::baked(def.id),
                !matches!(def.id, p::AMP_A | p::AMP_R | p::TUNE | p::ROOT | p::LEVEL)
            );
        }
        // ROOT is baked: the take is rendered at it.
        let mut b = ScompParams::default();
        b.set(p::ROOT, 48.0);
        assert_ne!(b.baked(), key);
    }
}
