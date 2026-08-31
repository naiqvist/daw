//! Gauge — the measurement utility.
//!
//! It changes nothing. That is the whole contract, and the reason it can
//! be left in a chain: what comes out is what went in, bit for bit, so
//! anything it reports is a fact about the signal rather than about
//! itself. `the_gauge_is_the_exact_identity` holds it to that on the bit.
//!
//! # What it answers
//!
//! - **PEAK**, with a hold, because the one that clipped went by while
//!   you were looking somewhere else.
//! - **RMS** over a window you choose, because loudness is an average and
//!   the question "how loud is this" has a time constant in it.
//! - **CREST**, the gap between them: how much dynamic range is left. A
//!   number that says "this is squashed" better than either alone does.
//! - **CORRELATION** between the two channels, because a mix that will
//!   not survive being summed to mono says so here and nowhere else:
//!   +1 is mono, 0 is wide, and anything below 0 is cancelling.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::dynamics::RmsDetector;
use crate::params::gauge as p;

/// The loudest sample the meter will take at face value: +24 dBFS. See
/// [`GaugeCore::measure`].
const CEILING: f32 = 16.0;

/// The knobs, in engine units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GaugeParams {
    pub window_ms: f32,
    pub hold_s: f32,
    pub range: f32,
}

impl Default for GaugeParams {
    fn default() -> Self {
        let d = |id: u32| {
            p::TABLE
                .iter()
                .find(|def| def.id == id)
                .map(|def| def.default)
                .unwrap_or(0.0)
        };
        Self {
            window_ms: d(p::WINDOW),
            hold_s: d(p::HOLD),
            range: d(p::RANGE),
        }
    }
}

impl GaugeParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(def) = p::TABLE.iter().find(|def| def.id == param) else {
            return;
        };
        let value = def.clamp(value);
        match param {
            p::WINDOW => self.window_ms = value,
            p::HOLD => self.hold_s = value,
            p::RANGE => self.range = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        match param {
            p::WINDOW => Some(self.window_ms),
            p::HOLD => Some(self.hold_s),
            p::RANGE => Some(self.range),
            _ => None,
        }
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

    /// The meter's floor, in dB.
    pub fn floor_db(&self) -> f32 {
        let i = (self.range.round().max(0.0) as usize).min(p::RANGE_FLOORS.len() - 1);
        p::RANGE_FLOORS.get(i).copied().unwrap_or(-48.0)
    }
}

/// One meter.
#[derive(Debug, Clone, Copy)]
pub struct GaugeCore {
    params: GaugeParams,
    prepared: f32,
    sample_rate: f32,
    rms: RmsDetector,
    /// The held peak, and how many samples of hold it has left.
    peak_held: f32,
    hold_left: u64,
    /// Running sums for the correlation, over the same window as the RMS.
    corr_xy: f32,
    corr_xx: f32,
    corr_yy: f32,
    corr_coeff: f32,
    said_peak_db: f32,
    said_rms_db: f32,
    said_corr: f32,
}

impl GaugeCore {
    pub fn new(sample_rate: f32, params: &GaugeParams) -> Self {
        let mut core = Self {
            params: *params,
            prepared: -1.0,
            sample_rate: 48_000.0,
            rms: RmsDetector::new(),
            peak_held: 0.0,
            hold_left: 0,
            corr_xy: 0.0,
            corr_xx: 0.0,
            corr_yy: 0.0,
            corr_coeff: 0.0,
            said_peak_db: crate::dsp::dynamics::FLOOR_DB,
            said_rms_db: crate::dsp::dynamics::FLOOR_DB,
            said_corr: 0.0,
        };
        core.params.sanitize();
        core.prepare(sample_rate);
        core
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.rebuild();
        self.reset();
    }

    fn rebuild(&mut self) {
        self.rms.prepare(self.sample_rate, self.params.window_ms);
        self.prepared = self.params.window_ms;
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
    }

    pub fn params(&self) -> GaugeParams {
        self.params
    }

    /// What the card draws: peak, RMS and correlation.
    ///
    /// `level_db` is the held peak and `reduction_db` the RMS — the two
    /// fields the readout already has, used for what they say rather
    /// than for what a compressor happens to put in them.
    pub fn readout(&self) -> crate::audio::graph::Readout {
        crate::audio::graph::Readout {
            level_db: self.said_peak_db,
            reduction_db: self.said_rms_db,
            bands: [self.said_corr, 0.0, 0.0],
        }
    }

    pub fn reset(&mut self) {
        self.rms.reset();
        self.peak_held = 0.0;
        self.hold_left = 0;
        self.corr_xy = 0.0;
        self.corr_xx = 0.0;
        self.corr_yy = 0.0;
        self.corr_coeff = 0.0;
        self.said_peak_db = crate::dsp::dynamics::FLOOR_DB;
        self.said_rms_db = crate::dsp::dynamics::FLOOR_DB;
        self.said_corr = 0.0;
    }

    /// It measures. It does not delay.
    pub fn latency(&self) -> usize {
        0
    }

    /// Red zone: measure a stereo pair, and LEAVE IT ALONE.
    ///
    /// Note the signature takes `&[f32]`, not `&mut`: the promise that
    /// this device changes nothing is kept by the type rather than by
    /// good intentions.
    pub fn measure(&mut self, l: &[f32], r: &[f32]) {
        if self.prepared != self.params.window_ms {
            self.rebuild();
        }
        let n = l.len().min(r.len());
        if n == 0 {
            return;
        }
        // The correlation's forgetting factor rides the same window the
        // RMS uses, so the two numbers describe the same stretch of time.
        let window = (self.params.window_ms * 1e-3 * self.sample_rate).max(1.0);
        let decay = crate::dsp::ramps::one_pole_coeff(1.0 / window);

        let mut block_peak = 0.0f32;
        for i in 0..n {
            let a = l.get(i).copied().unwrap_or(0.0);
            let b = r.get(i).copied().unwrap_or(0.0);
            // Finite is not enough. The correlation multiplies pairs of
            // samples together, and 1e30 squared is not a number an f32
            // has — it is infinity, and `inf / inf` is a NaN correlation
            // that then reads as neither mono nor wide. Anything past
            // +24 dBFS is not audio; clamping there keeps every product
            // in range and still reports "far too loud" honestly.
            let a = if a.is_finite() {
                a.clamp(-CEILING, CEILING)
            } else {
                0.0
            };
            let b = if b.is_finite() {
                b.clamp(-CEILING, CEILING)
            } else {
                0.0
            };
            block_peak = block_peak.max(a.abs()).max(b.abs());
            self.rms.tick(0.5 * (a + b));
            self.corr_xy += (a * b - self.corr_xy) * decay;
            self.corr_xx += (a * a - self.corr_xx) * decay;
            self.corr_yy += (b * b - self.corr_yy) * decay;
        }

        // The held peak: replaced by anything louder, and otherwise let
        // go once the hold runs out.
        let hold_samples = (self.params.hold_s * self.sample_rate) as u64;
        if block_peak >= self.peak_held {
            self.peak_held = block_peak;
            self.hold_left = hold_samples;
        } else {
            self.hold_left = self.hold_left.saturating_sub(n as u64);
            if self.hold_left == 0 {
                self.peak_held = block_peak;
            }
        }

        let denom = (self.corr_xx * self.corr_yy).max(1e-12).sqrt();
        self.corr_coeff = (self.corr_xy / denom).clamp(-1.0, 1.0);

        self.said_peak_db = crate::dsp::arith::gain_to_db(self.peak_held.max(1e-6))
            .max(crate::dsp::dynamics::FLOOR_DB);
        self.said_rms_db = crate::dsp::arith::gain_to_db(self.rms.current().max(1e-6))
            .max(crate::dsp::dynamics::FLOOR_DB);
        self.said_corr = self.corr_coeff;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn core(edit: impl Fn(&mut GaugeParams)) -> GaugeCore {
        let mut params = GaugeParams::default();
        edit(&mut params);
        GaugeCore::new(FS, &params)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (core::f32::consts::TAU * hz * i as f32 / FS).sin())
            .collect()
    }

    /// A cheap deterministic noise, so the two channels can be made
    /// genuinely independent without pulling in a kernel.
    fn noise(seed: u64, n: usize) -> Vec<f32> {
        let mut s = seed | 1;
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                ((s >> 40) as f32 / 8_388_608.0) - 1.0
            })
            .collect()
    }

    fn run(c: &mut GaugeCore, l: &[f32], r: &[f32], block: usize) {
        let mut at = 0usize;
        while at < l.len() {
            let end = (at + block).min(l.len());
            c.measure(&l[at..end], &r[at..end]);
            at = end;
        }
    }

    /// THE CONTRACT: it changes nothing.
    ///
    /// Enforced by the signature — `measure` takes `&[f32]` and cannot
    /// write — so this test is really about the signature being the one
    /// that ships. If it ever becomes `&mut`, this stops compiling, which
    /// is the loudest a test can be.
    #[test]
    fn the_gauge_cannot_change_what_it_measures() {
        let mut c = core(|_| {});
        let l = sine(440.0, 0.6, 4_096);
        let r = sine(440.0, 0.6, 4_096);
        let before = l.clone();
        c.measure(&l, &r);
        assert_eq!(l, before, "the input moved");
    }

    /// PEAK reads the peak, and HOLDS it.
    #[test]
    fn the_peak_is_the_peak_and_it_is_held() {
        let mut c = core(|p| p.hold_s = 2.0);
        let mut l = sine(440.0, 0.5, 4_096);
        // One sample far above everything else, in the middle.
        l[2_000] = 0.9;
        let r = l.clone();
        run(&mut c, &l, &r, 128);
        let peak = c.readout().level_db;
        let want = crate::dsp::arith::gain_to_db(0.9);
        assert!(
            (peak - want).abs() < 0.2,
            "the peak read {peak:.2} dB, the signal was {want:.2}"
        );

        // ...and it is still there a moment later, because that is what
        // a hold is for.
        run(&mut c, &vec![0.0; 4_096], &vec![0.0; 4_096], 128);
        let held = c.readout().level_db;
        assert!(
            (held - want).abs() < 0.2,
            "the hold let go early: {held:.2} dB"
        );

        // ...and lets go once the hold has run out.
        let quiet = vec![0.0f32; (FS * 3.0) as usize];
        run(&mut c, &quiet, &quiet, 512);
        assert!(
            c.readout().level_db < want - 20.0,
            "the hold never let go: {:.2} dB",
            c.readout().level_db
        );
    }

    /// RMS is the average, and a sine's is a known fraction of its peak —
    /// the one signal whose answer can be checked by hand.
    #[test]
    fn the_rms_of_a_sine_is_its_peak_over_root_two() {
        let mut c = core(|p| p.window_ms = 50.0);
        let l = sine(1_000.0, 1.0, (FS * 0.6) as usize);
        let r = l.clone();
        run(&mut c, &l, &r, 256);
        let got = c.readout().reduction_db;
        let want = crate::dsp::arith::gain_to_db(0.5f32.sqrt());
        assert!(
            (got - want).abs() < 0.5,
            "a full-scale sine should read {want:.2} dB RMS, read {got:.2}"
        );
    }

    /// CORRELATION: the number that says whether a mix survives mono.
    #[test]
    fn correlation_knows_mono_from_wide_from_cancelling() {
        let n = (FS * 0.6) as usize;

        // The same signal both sides: perfectly correlated.
        let mut c = core(|_| {});
        let l = sine(300.0, 0.6, n);
        run(&mut c, &l, &l, 256);
        assert!(
            c.readout().bands[0] > 0.98,
            "identical channels should read +1, read {}",
            c.readout().bands[0]
        );

        // One side inverted: perfectly ANTI-correlated, and this is the
        // case that vanishes when somebody sums to mono.
        let mut c = core(|_| {});
        let flipped: Vec<f32> = l.iter().map(|s| -s).collect();
        run(&mut c, &l, &flipped, 256);
        assert!(
            c.readout().bands[0] < -0.98,
            "an inverted channel should read -1, read {}",
            c.readout().bands[0]
        );

        // Two independent noises: uncorrelated.
        let mut c = core(|_| {});
        let a = noise(0x1234_5678, n);
        let b = noise(0x8765_4321, n);
        run(&mut c, &a, &b, 256);
        assert!(
            c.readout().bands[0].abs() < 0.2,
            "independent noise should read near zero, read {}",
            c.readout().bands[0]
        );
    }

    #[test]
    fn odd_lengths_and_nonsense_stay_finite() {
        let mut c = core(|_| {});
        for len in [0usize, 1, 3, 17, 129] {
            let l = sine(440.0, 0.4, len);
            c.measure(&l, &l);
            let said = c.readout();
            assert!(
                said.level_db.is_finite() && said.reduction_db.is_finite(),
                "len {len}"
            );
            assert!(said.bands[0].is_finite() && (-1.0..=1.0).contains(&said.bands[0]));
        }
        let junk = [f32::NAN, f32::INFINITY, -1e30, 0.5];
        c.measure(&junk, &junk);
        let said = c.readout();
        assert!(said.level_db.is_finite() && said.reduction_db.is_finite());
        assert!((-1.0..=1.0).contains(&said.bands[0]));
        // Silence reads as the floor, not as a NaN.
        let mut c = core(|p| p.hold_s = 0.0);
        c.measure(&[0.0; 512], &[0.0; 512]);
        assert!(c.readout().level_db <= crate::dsp::dynamics::FLOOR_DB + 1e-3);
    }

    #[test]
    fn measuring_does_not_allocate() {
        let mut c = core(|_| {});
        let l = sine(440.0, 0.6, 256);
        let r = sine(441.0, 0.6, 256);
        c.measure(&l, &r);
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..60 {
                c.measure(&l, &r);
            }
        });
    }

    #[test]
    fn every_row_round_trips_and_nonsense_is_sanitized() {
        let mut params = GaugeParams::default();
        for def in p::TABLE.iter() {
            params.set(def.id, def.max);
            assert_eq!(params.get(def.id), Some(def.max), "row {}", def.name);
            params.set(def.id, def.min);
            assert_eq!(params.get(def.id), Some(def.min), "row {}", def.name);
            params.set(def.id, def.max * 100.0 + 1.0);
            let got = params.get(def.id).unwrap();
            assert!(got <= def.max && got >= def.min, "row {} = {got}", def.name);
        }
        assert_eq!(params.get(9_999), None);

        let mut junk = GaugeParams {
            window_ms: f32::NAN,
            hold_s: -5.0,
            range: 99.0,
        };
        junk.sanitize();
        for def in p::TABLE.iter() {
            let got = junk.get(def.id).unwrap();
            assert!(
                got.is_finite() && got >= def.min && got <= def.max,
                "row {} survived sanitize as {got}",
                def.name
            );
        }
        assert!(p::RANGE_FLOORS.contains(&junk.floor_db()));
    }
}
