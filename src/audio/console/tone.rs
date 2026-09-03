//! TONE: the three-band, the desk's fast hand.
//!
//! LO is a shelf at 100 Hz, HI a shelf at 8 kHz, MID a bell you can
//! sweep, each ±15 dB — and each with a KILL: the DJ's stop past the
//! end of the knob. A killed LO is a 24 dB/octave high-pass at the
//! shelf's corner and a killed HI its low-pass; a killed MID is the bell
//! driven to −36 dB and widened. The corners were chosen once, which is
//! the whole point of a three-band: three levers reached without
//! looking.
//!
//! What makes it a desk's and not a textbook's:
//!
//! - The mid's Q is PROPORTIONAL: broad at a nudge, focused at a push,
//!   and narrower again on a cut, the way passive desks cut. The curve
//!   is `crate::console::tone_curve::mid_q`, shared with the card.
//! - A KILL CROSSES OVER in 20 ms rather than switching: the killed
//!   band's filter runs beside the band and the two are blended on a
//!   linear ramp that is DONE in 20 ms, so a kill locked on a trig
//!   stutters clean and an un-kill is a small swell.
//! - A HARD BOOST is driven: past +6 dB, what a LO or MID boost ADDS
//!   goes through the preamp's iron curve, harder with every dB, so a
//!   big low boost is thick the way a transformer desk's is. HI stays
//!   clean — driving the top without oversampling would alias.
//!
//! At all three flat and no kills the section is a wire to the sample.
//! The linear shape the core runs is `crate::console::tone_curve`'s,
//! which is also what the card draws; the iron is the one thing the
//! curve cannot show, and the card shows it as heat instead.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::console::preamp_curve;
use crate::console::tone_curve::{Shape, mid_q};
use crate::dsp::filters::{BandShape, Cascade, EqBand};
use crate::dsp::ramps::LinearRamp;
use crate::params::console::tone as p;

/// One band's kill: its cut filter, per channel, and how far into the
/// kill the blend is.
struct Kill {
    mix: LinearRamp,
    /// The fade's length in samples.
    fade: u32,
    /// Whether the filter has run since the last silence: a kill that
    /// has faded fully out is reset so it starts clean next time.
    warm: bool,
}

impl Kill {
    fn new(sample_rate: f32) -> Self {
        Self {
            mix: LinearRamp::new(),
            fade: (sample_rate * p::KILL_FADE_MS / 1000.0).round().max(1.0) as u32,
            warm: false,
        }
    }

    fn aim(&mut self, on: bool) {
        self.mix.glide(if on { 1.0 } else { 0.0 }, self.fade);
    }
}

pub struct ToneCore {
    params: SectionParams,
    shape: Shape,
    sample_rate: f32,
    lo: [EqBand; 2],
    mid: [EqBand; 2],
    hi: [EqBand; 2],
    /// The mid's kill bell, apart from its lever's bell so the two can
    /// be blended.
    mid_kill: [EqBand; 2],
    kill_lo: [Cascade; 2],
    kill_hi: [Cascade; 2],
    kills: [Kill; 3],
    /// Compile-owned scratch: the dry copy a blend or a boost needs, and
    /// the kill's blend ramp.
    dry: Vec<f32>,
    ramp: Vec<f32>,
    level_db: f32,
    /// How hard the last block drove the iron, for the card's heat.
    heat: f32,
}

/// How far past the knee a boost is, 0..1.
fn drive_of(gain_db: f32) -> f32 {
    ((gain_db - p::IRON_FROM_DB) / (15.0 - p::IRON_FROM_DB)).clamp(0.0, 1.0)
}

impl ToneCore {
    /// Green zone: every band prepared for the settings in hand.
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let mut core = Self {
            params: params.dense(),
            shape: Shape::of(params),
            sample_rate,
            lo: [EqBand::new(), EqBand::new()],
            mid: [EqBand::new(), EqBand::new()],
            hi: [EqBand::new(), EqBand::new()],
            mid_kill: [EqBand::new(), EqBand::new()],
            kill_lo: [Cascade::new(), Cascade::new()],
            kill_hi: [Cascade::new(), Cascade::new()],
            kills: [
                Kill::new(sample_rate),
                Kill::new(sample_rate),
                Kill::new(sample_rate),
            ],
            dry: vec![0.0; block.max(1)],
            ramp: vec![0.0; block.max(1)],
            level_db: -120.0,
            heat: 0.0,
        };
        core.tune();
        let s = core.shape;
        for (kill, on) in core
            .kills
            .iter_mut()
            .zip([s.kill_lo, s.kill_mid, s.kill_hi])
        {
            kill.mix.set_now(if on { 1.0 } else { 0.0 });
        }
        core
    }

    pub fn shape(&self) -> Shape {
        self.shape
    }

    /// Every band's coefficients from the shape. Run from `set_param`,
    /// which is a letter arriving between blocks, not inside one.
    fn tune(&mut self) {
        let s = self.shape;
        let fs = self.sample_rate;
        for ch in 0..2 {
            self.lo[ch].prepare(fs, p::LO_HZ, p::SHELF_Q, s.lo_db, BandShape::LowShelf);
            self.hi[ch].prepare(fs, p::HI_HZ, p::SHELF_Q, s.hi_db, BandShape::HighShelf);
            self.mid[ch].prepare(fs, s.mid_hz, mid_q(s.mid_db), s.mid_db, BandShape::Bell);
            self.mid_kill[ch].prepare(fs, s.mid_hz, p::KILL_MID_Q, p::KILL_MID_DB, BandShape::Bell);
            self.kill_lo[ch].prepare(
                fs,
                p::LO_HZ,
                core::f32::consts::FRAC_1_SQRT_2,
                p::KILL_ORDER,
                true,
            );
            self.kill_hi[ch].prepare(
                fs,
                p::HI_HZ,
                core::f32::consts::FRAC_1_SQRT_2,
                p::KILL_ORDER,
                false,
            );
        }
        for (kill, on) in self
            .kills
            .iter_mut()
            .zip([s.kill_lo, s.kill_mid, s.kill_hi])
        {
            kill.aim(on);
        }
    }

    /// A lever's band on one channel: the shelf or bell, and past the
    /// knee its addition driven through the iron.
    fn lever(&mut self, which: usize, ch: usize, io: &mut [f32]) {
        let n = io.len();
        let (band, gain_db, driven) = match which {
            0 => (&mut self.lo[ch], self.shape.lo_db, true),
            1 => (&mut self.mid[ch], self.shape.mid_db, true),
            _ => (&mut self.hi[ch], self.shape.hi_db, false),
        };
        if gain_db == 0.0 {
            return;
        }
        let drive = if driven { drive_of(gain_db) } else { 0.0 };
        if drive <= 0.0 {
            band.process(io);
            return;
        }
        // The boost's addition, driven: y = x + iron(y − x).
        let dry = &mut self.dry[..n];
        dry.copy_from_slice(io);
        band.process(io);
        let k = 1.0 + p::IRON_DRIVE * drive;
        let mut hottest = 0.0f32;
        for (y, x) in io.iter_mut().zip(dry.iter()) {
            let added = *y - *x;
            hottest = hottest.max(added.abs());
            *y = *x + preamp_curve::iron(added, k);
        }
        self.heat = self.heat.max((hottest * k).min(1.0));
    }

    /// A band's kill on one channel: the cut filter beside the band,
    /// blended by the kill's ramp. Skipped, and the filter rested, once
    /// the blend has fully faded out.
    fn kill(&mut self, which: usize, ch: usize, io: &mut [f32]) {
        let n = io.len();
        let mix_now = self.kills[which].mix.current();
        let on = match which {
            0 => self.shape.kill_lo,
            1 => self.shape.kill_mid,
            _ => self.shape.kill_hi,
        };
        if !on && mix_now <= 1e-4 {
            if self.kills[which].warm {
                self.kills[which].warm = false;
                match which {
                    0 => self.kill_lo[ch].reset(),
                    1 => self.mid_kill[ch].reset(),
                    _ => self.kill_hi[ch].reset(),
                }
            }
            return;
        }
        self.kills[which].warm = true;
        let dry = &mut self.dry[..n];
        dry.copy_from_slice(io);
        match which {
            0 => self.kill_lo[ch].process(io),
            1 => self.mid_kill[ch].process(io),
            _ => self.kill_hi[ch].process(io),
        }
        // The ramp is written once for the left and replayed for the
        // right, so both sides cross over together.
        if ch == 0 {
            self.kills[which].mix.process(&mut self.ramp[..n]);
        }
        for ((y, x), m) in io.iter_mut().zip(dry.iter()).zip(&self.ramp[..n]) {
            *y = *x + (*y - *x) * *m;
        }
    }

    fn run(&mut self, ch: usize, io: &mut [f32]) {
        for band in 0..3 {
            self.lever(band, ch, io);
            self.kill(band, ch, io);
        }
    }

    /// Whether every kill has fully faded out, so the flat shape is a
    /// wire again.
    fn kills_are_cold(&self) -> bool {
        self.kills.iter().all(|kill| kill.mix.current() <= 1e-4)
    }
}

impl SectionCore for ToneCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Shape::of(&self.params);
        if next != self.shape {
            self.shape = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        for ch in 0..2 {
            self.lo[ch].reset();
            self.mid[ch].reset();
            self.hi[ch].reset();
            self.mid_kill[ch].reset();
            self.kill_lo[ch].reset();
            self.kill_hi[ch].reset();
        }
        let s = self.shape;
        for (kill, on) in self
            .kills
            .iter_mut()
            .zip([s.kill_lo, s.kill_mid, s.kill_hi])
        {
            kill.mix.set_now(if on { 1.0 } else { 0.0 });
            kill.warm = false;
        }
        self.level_db = -120.0;
        self.heat = 0.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > self.dry.len() {
            return;
        }
        let stereo = r.len() >= n;
        self.heat *= 0.8;
        if !(self.shape.is_flat() && self.kills_are_cold()) {
            self.run(0, l);
            if stereo {
                self.run(1, &mut r[..n]);
            }
        }
        let peak = l
            .iter()
            .chain(if stereo { r[..n].iter() } else { [].iter() })
            .fold(0.0f32, |peak, s| peak.max(s.abs()));
        self.level_db = if peak <= 1e-6 {
            -120.0
        } else {
            20.0 * peak.log10()
        };
    }

    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            // The heat, as the reduction figure's neighbour: how hard the
            // iron was driven this block, 0..1 — the card shows it as
            // warmth on a boosted band.
            reduction_db: -self.heat,
            bands: [self.shape.lo_db, self.shape.mid_db, self.shape.hi_db],
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::console::SectionKind;
    use crate::console::tone_curve::response_db;

    const FS: f32 = 48_000.0;
    const BLOCK: usize = 256;

    fn clock() -> Clock {
        Clock {
            playing: true,
            beat: 0.0,
            beats_per_sample: 120.0 / 60.0 / f64::from(FS),
        }
    }

    fn core_with(edits: &[(u32, f32)]) -> ToneCore {
        let mut params = SectionParams::of(SectionKind::Tone);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        ToneCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut ToneCore, l: &[f32]) -> Vec<f32> {
        let mut out = l.to_vec();
        for start in (0..out.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(out.len());
            core.process(&mut out[start..end], &mut [], &clock());
        }
        out
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    /// How much `core` changes a quiet sine at `hz`, in dB, once settled.
    fn gain_db(core: &mut ToneCore, hz: f32) -> f32 {
        let n = FS as usize / 2;
        let l = sine(hz, 0.05, n);
        let out = run(core, &l);
        20.0 * (rms(&out[n / 2..]) / rms(&l[n / 2..])).log10()
    }

    fn harmonic_db(signal: &[f32], hz: f32, h: u32) -> f32 {
        let bin = |f: f32| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, s) in signal.iter().enumerate() {
                let w = 2.0 * core::f32::consts::PI * f * i as f32 / FS;
                re += s * w.cos();
                im -= s * w.sin();
            }
            (re * re + im * im).sqrt()
        };
        20.0 * (bin(hz * h as f32) / bin(hz).max(1e-9)).log10()
    }

    #[test]
    fn flat_is_a_wire_to_the_sample() {
        let mut core = core_with(&[]);
        assert!(core.shape().is_flat());
        for len in [0usize, 1, 7, BLOCK] {
            let l = sine(440.0, 0.5, len);
            let r: Vec<f32> = l.iter().map(|s| -s).collect();
            let (mut ol, mut or) = (l.clone(), r.clone());
            core.process(&mut ol, &mut or, &clock());
            assert_eq!(ol, l);
            assert_eq!(or, r);
        }
    }

    /// Each lever moves its own band and leaves the others alone.
    #[test]
    fn each_lever_moves_its_own_band() {
        let mut lo = core_with(&[(p::LO, 5.0)]);
        assert!((gain_db(&mut lo, 40.0) - 5.0).abs() < 1.0);
        assert!(gain_db(&mut lo, 5_000.0).abs() < 0.5);

        let mut hi = core_with(&[(p::HI, -12.0)]);
        assert!((gain_db(&mut hi, 16_000.0) + 12.0).abs() < 1.5);
        assert!(gain_db(&mut hi, 300.0).abs() < 0.5);

        let mut mid = core_with(&[(p::MID, 5.0), (p::MID_HZ, 1_000.0)]);
        assert!((gain_db(&mut mid, 1_000.0) - 5.0).abs() < 0.5);
        assert!(gain_db(&mut mid, 60.0).abs() < 1.0);
        assert!(gain_db(&mut mid, 12_000.0).abs() < 1.0);
    }

    /// The mid narrows as it is pushed, and a cut is narrower than the
    /// same boost: an octave off centre, a +3 dB bell still shows most
    /// of itself, a +15 dB bell shows less of itself, and a −9 dB bell
    /// shows less of itself than a +9 dB one.
    #[test]
    fn the_mids_q_is_proportional_and_cuts_are_narrower() {
        let share = |gain: f32| -> f32 {
            let mut core = core_with(&[(p::MID, gain), (p::MID_HZ, 1_000.0)]);
            let at_centre = gain_db(&mut core, 1_000.0);
            let mut core = core_with(&[(p::MID, gain), (p::MID_HZ, 1_000.0)]);
            let an_octave_off = gain_db(&mut core, 2_000.0);
            an_octave_off / at_centre
        };
        let nudge = share(3.0);
        let push = share(15.0);
        assert!(nudge > push + 0.1, "nudge {nudge}, push {push}");
        let boost = share(9.0);
        let cut = share(-9.0);
        assert!(boost > cut + 0.05, "boost {boost}, cut {cut}");
        assert!(mid_q(-9.0) > mid_q(9.0));
        assert!(mid_q(15.0) > mid_q(3.0));
    }

    /// A kill is a kill: the band is gone, the rest is untouched.
    #[test]
    fn a_kill_takes_the_band_away() {
        let mut lo = core_with(&[(p::KILL_LO, 1.0)]);
        assert!(gain_db(&mut lo, 30.0) < -35.0);
        assert!(gain_db(&mut lo, 3_000.0).abs() < 0.5);

        let mut hi = core_with(&[(p::KILL_HI, 1.0)]);
        assert!(gain_db(&mut hi, 18_000.0) < -25.0);
        assert!(gain_db(&mut hi, 500.0).abs() < 0.5);

        let mut mid = core_with(&[(p::KILL_MID, 1.0), (p::MID_HZ, 1_000.0)]);
        assert!(gain_db(&mut mid, 1_000.0) < -30.0);
        assert!(gain_db(&mut mid, 50.0) > -3.0);
        assert!(gain_db(&mut mid, 15_000.0) > -3.0);
    }

    /// A kill thrown mid-stream crosses over rather than switching: no
    /// sample steps further than the sine itself moves, and the band is
    /// gone within a few dozen milliseconds. Un-killing fades back and
    /// leaves the section a wire again.
    #[test]
    fn a_kill_crosses_over_without_a_click() {
        let n = FS as usize / 4;
        let l = sine(30.0, 0.5, n);
        let mut core = core_with(&[]);
        let mut out = l.clone();
        let half = n / 2;
        for start in (0..half).step_by(BLOCK) {
            let end = (start + BLOCK).min(half);
            core.process(&mut out[start..end], &mut [], &clock());
        }
        core.set_param(p::KILL_LO, 1.0);
        for start in (half..n).step_by(BLOCK) {
            let end = (start + BLOCK).min(n);
            core.process(&mut out[start..end], &mut [], &clock());
        }
        let sine_step = l
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        let biggest = out
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(
            biggest < sine_step * 2.0,
            "a step of {biggest} against the sine's {sine_step}"
        );
        let tail = &out[n - 4800..];
        assert!(rms(tail) < rms(&l[..4800]) * 0.05, "the band did not go");

        core.set_param(p::KILL_LO, 0.0);
        let mut back = sine(30.0, 0.5, FS as usize / 4);
        let copy = back.clone();
        for start in (0..back.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(back.len());
            core.process(&mut back[start..end], &mut [], &clock());
        }
        let tail = &back[back.len() - 4800..];
        assert!(
            (rms(tail) / rms(&copy[..4800]) - 1.0).abs() < 0.05,
            "the band did not come back"
        );
        assert!(core.kills_are_cold());
        let mut probe = sine(440.0, 0.3, BLOCK);
        let expected = probe.clone();
        core.process(&mut probe, &mut [], &clock());
        assert_eq!(probe, expected, "a faded-out kill is not a wire");
    }

    /// A hard low boost is driven: its second harmonic rises with the
    /// boost, a small boost stays clean, and the output stays bounded.
    #[test]
    fn a_hard_boost_is_driven_through_the_iron() {
        let n = FS as usize / 2;
        let window = n / 2..n / 2 + 800 * 12;
        let l = sine(60.0, 0.4, n);
        let mut clean = core_with(&[(p::LO, 3.0)]);
        let out = run(&mut clean, &l);
        let second_clean = harmonic_db(&out[window.clone()], 60.0, 2);
        assert!(
            second_clean < -60.0,
            "a nudge is not clean: {second_clean} dB"
        );
        assert_eq!(clean.readout().reduction_db, 0.0);

        let mut hot = core_with(&[(p::LO, 14.0)]);
        let out = run(&mut hot, &l);
        let second_hot = harmonic_db(&out[window], 60.0, 2);
        assert!(second_hot > -40.0, "a push is not driven: {second_hot} dB");
        assert!(out.iter().all(|s| s.abs() <= 1.2));
        assert!(hot.readout().reduction_db < 0.0, "no heat reported");
    }

    /// The curve the card draws is the curve the core runs, at settings
    /// below the iron's knee.
    #[test]
    fn the_drawn_response_is_the_measured_one() {
        let edits = [
            (p::LO, 5.0),
            (p::MID, -8.0),
            (p::MID_HZ, 800.0),
            (p::HI, 4.0),
        ];
        let mut core = core_with(&edits);
        let shape = core.shape();
        for hz in [50.0, 300.0, 800.0, 2_500.0, 12_000.0] {
            let drawn = response_db(&shape, FS, hz);
            let heard = gain_db(&mut core, hz);
            assert!(
                (drawn - heard).abs() < 0.75,
                "{hz} Hz: drawn {drawn}, heard {heard}"
            );
        }
        let mut killed = core_with(&[(p::KILL_LO, 1.0)]);
        let shape = killed.shape();
        for hz in [40.0, 100.0, 1_000.0] {
            let drawn = response_db(&shape, FS, hz);
            let heard = gain_db(&mut killed, hz);
            assert!(
                (drawn - heard).abs() < 1.5,
                "kill at {hz} Hz: drawn {drawn}, heard {heard}"
            );
        }
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 0.4, 1000);
        let edits = [
            (p::LO, -6.0),
            (p::MID, 5.0),
            (p::HI, 3.0),
            (p::KILL_HI, 1.0),
        ];
        let mut whole = core_with(&edits);
        let a = run(&mut whole, &l);
        let mut pieces = core_with(&edits);
        let mut b = l.clone();
        let mut at = 0;
        for len in [1usize, 7, 64, 200, 128, 100, 256, 244] {
            let end = (at + len).min(b.len());
            pieces.process(&mut b[at..end], &mut [], &clock());
            at = end;
        }
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-5);
        }
    }

    #[test]
    fn letters_land_clamped_and_retune() {
        let mut core = core_with(&[]);
        core.set_param(p::LO, 40.0);
        assert_eq!(core.shape().lo_db, 15.0);
        core.set_param(p::MID_HZ, 10.0);
        assert_eq!(core.shape().mid_hz, 200.0);
        core.set_param(99, 1.0);
        core.set_param(p::LO, 0.0);
        core.set_param(p::MID_HZ, 1000.0);
        assert!(core.shape().is_flat());
        assert_eq!(core.readout().bands, [0.0, 0.0, 0.0]);
    }
}
