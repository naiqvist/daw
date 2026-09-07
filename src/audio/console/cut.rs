//! CUT: two DJ filters in series — the Xone:92's, twice.
//!
//! A high-pass into a low-pass, each a 2-pole state-variable filter
//! descended from the Oberheim SEM, which is what the Allen & Heath
//! Xone:92's VCF is and why a slow sweep across a whole track stays
//! musical: twelve dB an octave is a lever, not a switch. Each has a
//! FREQ and a RES; parked at its resting end — 20 Hz, 20 kHz — a filter
//! is off, and with both parked the section is a wire behind the fixed
//! antialias round-trip latency. The round trip remains warm while the
//! filters are parked so moving a live frequency control never moves the
//! channel in time.
//!
//! The opinions, baked in:
//!
//! - TWELVE dB ONLY. No slope switch; FOUR is the surgeon.
//! - The RESONANCE LOOP SATURATES. The 92's resonance is an OTA that
//!   crunches as it is pushed, so a peak gets loud and then breaks up
//!   instead of screaming. The loop here is a zero-delay-feedback
//!   state-variable filter of the core's own with a tanh in the
//!   feedback path, at 2× through the shaper's oversampler so it holds
//!   at self-oscillation. CRUNCH is how hard that loop leans.
//! - RESONANCE COMPENSATION. As RES rises the passband is pulled down
//!   by up to six dB, so a resonant sweep gets a peak, not a level
//!   jump.
//! - The FREQUENCY LAW is the knob's: the table is linear in hertz and
//!   the surface's steps are what they are, but the filter's own
//!   response is exponential, so the middle of the range is where the
//!   sweep lives.
//!
//! The linear shape the card draws is `crate::console::cut_curve`'s.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::console::cut_curve::{Shape, compensation_db, damping_of};
use crate::dsp::shaper::Oversampler2x;
use crate::params::console::cut as p;

/// A 2-pole state-variable filter solved without a unit delay in its
/// loop (Zavalishin's form), with the resonance feedback bent through
/// a tanh so a hard peak saturates the way an OTA does. Runs at the
/// oversampled rate.
#[derive(Clone, Copy, Debug, Default)]
struct Loop {
    ic1: f32,
    ic2: f32,
    g: f32,
    k: f32,
    /// The knee of the loop's tanh: larger is cleaner.
    knee: f32,
}

impl Loop {
    fn tune(&mut self, sample_rate: f32, hz: f32, damping: f32, crunch: f32) {
        let hz = hz.clamp(5.0, sample_rate * 0.45);
        self.g = (core::f32::consts::PI * hz / sample_rate).tan();
        self.k = damping;
        // The tanh's scale: small, and the loop is a line until it is
        // driven hard; large, and it rounds early — the OTA's knee.
        self.knee = p::CRUNCH_FLOOR + p::CRUNCH_DRIVE * crunch * crunch;
    }

    fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    /// One sample in, the low-pass and high-pass outputs.
    #[inline(always)]
    fn tick(&mut self, x: f32) -> (f32, f32) {
        let (g, k) = (self.g, self.k);
        // The resonance path is the band-pass fed back through k; it
        // is the band-pass that is bent, so the passband stays clean
        // and only the peak crunches.
        // Damping may sit just under zero at full resonance; the
        // solve stays sound while `1 + g(g + k)` does, which it does for
        // any corner the tune allows.
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        // The bend: the band-pass state through the tanh, scaled back,
        // so a quiet loop is exact and a loud one rounds.
        let bent = (v1 * self.knee).tanh() / self.knee;
        self.ic1 = 2.0 * bent - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        let low = v2;
        let high = x - k * bent - low;
        (low, high)
    }
}

pub struct CutCore {
    params: SectionParams,
    shape: Shape,
    sample_rate: f32,
    hp: [Loop; 2],
    lp: [Loop; 2],
    over: [Oversampler2x; 2],
    lane: Vec<f32>,
    /// The passband gains, linear, from the compensation.
    hp_gain: f32,
    lp_gain: f32,
    level_db: f32,
    heat: f32,
    /// Each loop's own ring: the block peak of its band-pass state,
    /// scaled and held, so the card can light the filter that is
    /// actually resonating rather than the one that is merely set to.
    hp_ring: f32,
    lp_ring: f32,
    /// The input peak, in dBFS, measured before the filters run.
    in_db: f32,
}

/// The loudest sample in a block, over one channel or two.
fn peak_of(l: &[f32], r: &[f32], n: usize, stereo: bool) -> f32 {
    l.iter()
        .chain(if stereo { r[..n].iter() } else { [].iter() })
        .fold(0.0f32, |peak, s| peak.max(s.abs()))
}

/// A block peak as dBFS, floored so silence is a number and not a log
/// of zero.
fn peak_db(peak: f32) -> f32 {
    if peak <= p::SILENCE_PEAK {
        p::SILENT_DB
    } else {
        20.0 * peak.log10()
    }
}

impl CutCore {
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let mut core = Self {
            params: params.dense(),
            shape: Shape::of(params),
            sample_rate,
            hp: [Loop::default(); 2],
            lp: [Loop::default(); 2],
            over: [Oversampler2x::new(), Oversampler2x::new()],
            lane: vec![0.0; Oversampler2x::scratch_len(block.max(1))],
            hp_gain: 1.0,
            lp_gain: 1.0,
            level_db: p::SILENT_DB,
            heat: 0.0,
            hp_ring: 0.0,
            lp_ring: 0.0,
            in_db: p::SILENT_DB,
        };
        for over in &mut core.over {
            over.prepare();
        }
        core.tune();
        core
    }

    pub fn shape(&self) -> Shape {
        self.shape
    }

    fn tune(&mut self) {
        let s = self.shape;
        let fs2 = self.sample_rate * 2.0;
        for ch in 0..2 {
            self.hp[ch].tune(fs2, s.hp_hz, damping_of(s.hp_res), s.crunch);
            self.lp[ch].tune(fs2, s.lp_hz, damping_of(s.lp_res), s.crunch);
        }
        self.hp_gain = 10f32.powf(compensation_db(s.hp_res) / 20.0);
        self.lp_gain = 10f32.powf(compensation_db(s.lp_res) / 20.0);
    }

    fn run(&mut self, ch: usize, io: &mut [f32]) {
        let n = io.len();
        let s = self.shape;
        let lane = &mut self.lane[..n * 2];
        self.over[ch].up(io, lane);
        let (hp_on, lp_on) = (!s.hp_off(), !s.lp_off());
        let mut hottest = 0.0f32;
        // The two loops' resonance currents, kept APART: heat is how
        // hard either one is leaning on its tanh, but a ring belongs to
        // one filter and says which of the two is singing.
        let mut hp_ring = 0.0f32;
        let mut lp_ring = 0.0f32;
        for x in lane.iter_mut() {
            let mut y = *x;
            if hp_on {
                let (_, high) = self.hp[ch].tick(y);
                y = high * self.hp_gain;
                let ic1 = self.hp[ch].ic1.abs();
                hp_ring = hp_ring.max(ic1);
                hottest = hottest.max(ic1 * self.hp[ch].knee);
            }
            if lp_on {
                let (low, _) = self.lp[ch].tick(y);
                y = low * self.lp_gain;
                let ic1 = self.lp[ch].ic1.abs();
                lp_ring = lp_ring.max(ic1);
                hottest = hottest.max(ic1 * self.lp[ch].knee);
            }
            *x = y;
        }
        self.over[ch].down(lane, io);
        self.heat = self.heat.max((hottest / p::HEAT_FULL).min(1.0));
        // `max` across the call, so the pair's two channels fold into
        // one figure per filter.
        self.hp_ring = self.hp_ring.max((hp_ring / p::RING_FULL).min(1.0));
        self.lp_ring = self.lp_ring.max((lp_ring / p::RING_FULL).min(1.0));
    }
}

impl SectionCore for CutCore {
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
            self.hp[ch].reset();
            self.lp[ch].reset();
            self.over[ch].reset();
        }
        self.level_db = p::SILENT_DB;
        self.heat = 0.0;
        self.hp_ring = 0.0;
        self.lp_ring = 0.0;
        // A dB field's zero is silence, not full scale.
        self.in_db = p::SILENT_DB;
    }

    fn latency(&self) -> usize {
        self.over[0].latency()
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n * 2 > self.lane.len() {
            return;
        }
        let stereo = r.len() >= n;
        self.heat *= p::READOUT_DECAY;
        self.hp_ring *= p::READOUT_DECAY;
        self.lp_ring *= p::READOUT_DECAY;
        // The input peak is taken BEFORE the filters run over the same
        // buffers, so `in_db - level_db` is this section's own loss and
        // nothing else's.
        self.in_db = peak_db(peak_of(l, r, n, stereo));
        // Even a parked CUT traverses the identity antialias round trip.
        // Its parameters arrive as live letters, so skipping the filters at
        // rest would make the channel jump by their 35-sample group delay as
        // soon as either blade moved.
        self.run(0, l);
        if stereo {
            self.run(1, &mut r[..n]);
        }
        self.level_db = peak_db(peak_of(l, r, n, stereo));
    }

    /// What the CUT card is drawn from. Every figure is measured in
    /// `process`; this is a copy of five floats and no work.
    ///
    /// - `level_db`: the loudest OUTPUT sample of the block, in dBFS,
    ///   from 0 down to a floor of −120 for silence. One block's peak,
    ///   no smoothing and no hold — it follows the material exactly.
    /// - `reduction_db`: not a gain reduction but the LOOP HEAT,
    ///   negated: 0 at rest, −1 when a resonance loop is as deep into
    ///   its tanh as the crunch knee allows. A block peak, held and
    ///   multiplied by 0.8 each block (≈10 blocks to a tenth).
    /// - `bands[0]`: the HP loop's RING, 0..1 and unitless — the block
    ///   peak of |ic1|, the band-pass state that IS the resonance
    ///   current in the loop, taken over both channels, divided by 1.5
    ///   and clamped, then held and decayed 0.8 per block like the
    ///   heat. 0 while the filter is parked, while the section is off,
    ///   or while the signal sits away from that corner; 1 when the
    ///   loop is fully ringing or singing.
    /// - `bands[1]`: the same figure for the LP loop, on the same scale
    ///   and the same time constant, so the two are comparable.
    /// - `bands[2]`: the loudest INPUT sample of the block in dBFS, 0
    ///   down to the same −120 floor, taken before either filter runs;
    ///   one block's peak, no smoothing, no hold. `bands[2] −
    ///   level_db` is therefore the section's true loss in dB, 0 when
    ///   both blades are parked.
    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: -self.heat,
            bands: [self.hp_ring, self.lp_ring, self.in_db],
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::console::SectionKind;
    use crate::console::cut_curve::response_db;

    const FS: f32 = 48_000.0;
    const BLOCK: usize = 256;

    fn clock() -> Clock {
        Clock {
            playing: true,
            beat: 0.0,
            beats_per_sample: 120.0 / 60.0 / f64::from(FS),
        }
    }

    fn core_with(edits: &[(u32, f32)]) -> CutCore {
        let mut params = SectionParams::of(SectionKind::Cut);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        CutCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut CutCore, l: &[f32]) -> Vec<f32> {
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
    fn gain_db(core: &mut CutCore, hz: f32) -> f32 {
        let n = FS as usize / 2;
        let l = sine(hz, 0.02, n);
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
    fn both_parked_are_a_wire_behind_the_reported_latency() {
        let mut core = core_with(&[(p::HP_RES, 80.0), (p::LP_RES, 80.0)]);
        assert!(core.shape().is_off());
        let ahead = core.latency();
        assert_eq!(ahead, Oversampler2x::new().latency());
        let mut l = vec![0.0f32; BLOCK];
        let mut r = vec![0.0f32; BLOCK];
        l[0] = 1.0;
        r[0] = -1.0;
        core.process(&mut l, &mut r, &clock());
        let peak = l
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .expect("an impulse has a peak");
        assert_eq!(peak.0, ahead);
        assert!(peak.1.abs() > 0.9, "the round trip lost the impulse");
        assert!(
            l.iter()
                .zip(&r)
                .all(|(a, b)| (*a + *b).abs() <= f32::EPSILON),
            "the stereo lanes must remain equal and opposite"
        );
    }

    /// Each filter is twelve dB an octave from its corner, and leaves
    /// the far side alone.
    #[test]
    fn twelve_db_an_octave_each_way() {
        let mut hp = core_with(&[(p::HP_HZ, 400.0), (p::HP_RES, 0.0)]);
        let at_corner = gain_db(&mut hp, 400.0);
        let octave_under = gain_db(&mut hp, 200.0);
        let two_under = gain_db(&mut hp, 100.0);
        assert!((at_corner + 3.0).abs() < 1.0, "corner {at_corner}");
        assert!(
            (octave_under - two_under - 12.0).abs() < 1.5,
            "{octave_under} then {two_under}"
        );
        assert!(gain_db(&mut hp, 8_000.0).abs() < 0.3);

        let mut lp = core_with(&[(p::LP_HZ, 1_000.0), (p::LP_RES, 0.0)]);
        let octave_over = gain_db(&mut lp, 2_000.0);
        let two_over = gain_db(&mut lp, 4_000.0);
        assert!(
            (octave_over - two_over - 12.0).abs() < 1.5,
            "{octave_over} then {two_over}"
        );
        assert!(gain_db(&mut lp, 60.0).abs() < 0.3);
    }

    /// Resonance raises a peak at the corner and pulls the passband
    /// down to make room for it.
    #[test]
    fn resonance_is_a_peak_over_a_lowered_passband() {
        let mut flat = core_with(&[(p::LP_HZ, 1_000.0), (p::LP_RES, 0.0)]);
        let mut peaked = core_with(&[(p::LP_HZ, 1_000.0), (p::LP_RES, 70.0)]);
        let flat_corner = gain_db(&mut flat, 1_000.0);
        let peaked_corner = gain_db(&mut peaked, 1_000.0);
        assert!(
            peaked_corner > flat_corner + 8.0,
            "no peak: {flat_corner} vs {peaked_corner}"
        );
        let flat_pass = gain_db(&mut flat, 100.0);
        let peaked_pass = gain_db(&mut peaked, 100.0);
        assert!(
            peaked_pass < flat_pass - 3.0,
            "passband not pulled down: {flat_pass} vs {peaked_pass}"
        );
    }

    /// The loop crunches: a loud note at the corner with resonance and
    /// crunch grows harmonics; without crunch it stays clean.
    #[test]
    fn the_resonance_loop_crunches_when_pushed() {
        let n = FS as usize / 2;
        let window = n / 2..n / 2 + 96 * 100;
        // A modest note: loud enough for the peak to lean on the
        // loop, quiet enough that a clean loop is still a line.
        let l = sine(500.0, 0.1, n);
        let mut clean = core_with(&[(p::LP_HZ, 500.0), (p::LP_RES, 60.0), (p::CRUNCH, 0.0)]);
        let out = run(&mut clean, &l);
        let third_clean = harmonic_db(&out[window.clone()], 500.0, 3);
        let mut crunchy = core_with(&[(p::LP_HZ, 500.0), (p::LP_RES, 60.0), (p::CRUNCH, 100.0)]);
        let out = run(&mut crunchy, &l);
        let third_crunchy = harmonic_db(&out[window], 500.0, 3);
        assert!(
            third_crunchy > third_clean + 12.0,
            "clean {third_clean}, crunchy {third_crunchy}"
        );
        assert!(out.iter().all(|s| s.abs() < 4.0), "the loop ran away");
        assert!(crunchy.readout().reduction_db < 0.0, "no heat reported");
    }

    /// At full resonance the loop sings on its own and does not blow up.
    #[test]
    fn full_resonance_sings_and_stays_bounded() {
        let n = FS as usize / 2;
        let mut l = vec![0.0f32; n];
        l[0] = 0.5;
        let mut core = core_with(&[(p::LP_HZ, 800.0), (p::LP_RES, 100.0), (p::CRUNCH, 50.0)]);
        let out = run(&mut core, &l);
        let tail = &out[n - 4800..];
        assert!(rms(tail) > 0.01, "the loop did not sing: {}", rms(tail));
        assert!(out.iter().all(|s| s.abs() < 4.0));
    }

    /// The curve the card draws is the curve the core runs, at modest
    /// resonance where the loop is linear.
    #[test]
    fn the_drawn_response_is_the_measured_one() {
        let edits = [
            (p::HP_HZ, 150.0),
            (p::HP_RES, 30.0),
            (p::LP_HZ, 3_000.0),
            (p::LP_RES, 40.0),
            (p::CRUNCH, 0.0),
        ];
        let mut core = core_with(&edits);
        let shape = core.shape();
        for hz in [60.0, 150.0, 500.0, 1_500.0, 3_000.0, 8_000.0] {
            let drawn = response_db(&shape, hz);
            let heard = gain_db(&mut core, hz);
            assert!(
                (drawn - heard).abs() < 1.5,
                "{hz} Hz: drawn {drawn}, heard {heard}"
            );
        }
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 0.4, 1000);
        let edits = [(p::HP_HZ, 120.0), (p::LP_HZ, 2_000.0), (p::LP_RES, 50.0)];
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
            assert!((x - y).abs() < 1e-4);
        }
    }

    /// At its defaults the section is a delayed wire, and says so: both
    /// rings dark, and the input peak equal to the output level, which
    /// is a loss of nothing. On silence the whole readout is at rest.
    #[test]
    fn at_its_defaults_the_bands_report_rest() {
        let mut core = core_with(&[]);
        let mut quiet = vec![0.0f32; BLOCK];
        core.process(&mut quiet, &mut [], &clock());
        let said = core.readout();
        assert_eq!(said.bands, [0.0, 0.0, p::SILENT_DB]);
        assert_eq!(said.reduction_db, 0.0);

        let l = sine(440.0, 0.5, BLOCK * 4);
        let out = run(&mut core, &l);
        let ahead = core.latency();
        for i in ahead..out.len() {
            assert!(
                (out[i] - l[i - ahead]).abs() < 0.01,
                "the parked round trip moved sample {i} by {}",
                out[i] - l[i - ahead]
            );
        }
        let said = core.readout();
        assert_eq!(said.bands[0], 0.0, "a parked HP rang");
        assert_eq!(said.bands[1], 0.0, "a parked LP rang");
        assert!(
            (said.bands[2] - said.level_db).abs() < 1e-3,
            "a wire lost {} dB",
            said.bands[2] - said.level_db
        );
    }

    /// The rings are the loops RINGING, not the loops' settings: with
    /// both filters set to the same resonance, only the one the note
    /// sits on lights up.
    #[test]
    fn each_ring_lights_only_for_the_loop_that_is_ringing() {
        let edits = [
            (p::HP_HZ, 300.0),
            (p::HP_RES, 85.0),
            (p::LP_HZ, 6_000.0),
            (p::LP_RES, 85.0),
            (p::CRUNCH, 0.0),
        ];
        let n = FS as usize / 4;
        let mut low = core_with(&edits);
        run(&mut low, &sine(300.0, 0.3, n));
        let at_hp = low.readout();
        let mut high = core_with(&edits);
        run(&mut high, &sine(6_000.0, 0.3, n));
        let at_lp = high.readout();
        assert!(
            at_hp.bands[0] > at_hp.bands[1] + 0.3,
            "a note on the HP corner lit {} / {}",
            at_hp.bands[0],
            at_hp.bands[1]
        );
        assert!(
            at_lp.bands[1] > at_lp.bands[0] + 0.3,
            "a note on the LP corner lit {} / {}",
            at_lp.bands[0],
            at_lp.bands[1]
        );
        for said in [at_hp, at_lp] {
            assert!(said.bands[0] >= 0.0 && said.bands[0] <= 1.0);
            assert!(said.bands[1] >= 0.0 && said.bands[1] <= 1.0);
        }
    }

    /// A ring is live in both directions: it grows with the resonance
    /// that made it, and it falls away, block by block, when the note
    /// stops — no wall clock, just the hold's 0.8.
    #[test]
    fn the_ring_grows_with_resonance_and_dies_into_silence() {
        let note = sine(800.0, 0.2, FS as usize / 8);
        let mut flat = core_with(&[(p::LP_HZ, 800.0), (p::LP_RES, 0.0), (p::CRUNCH, 0.0)]);
        run(&mut flat, &note);
        let dull = flat.readout().bands[1];
        let mut peaked = core_with(&[(p::LP_HZ, 800.0), (p::LP_RES, 75.0), (p::CRUNCH, 0.0)]);
        run(&mut peaked, &note);
        let ringing = peaked.readout().bands[1];
        assert!(
            ringing > dull + 0.3,
            "res 0 rang {dull}, res 75 rang {ringing}"
        );

        let mut before = ringing;
        for block in 0..24 {
            let mut quiet = vec![0.0f32; BLOCK];
            peaked.process(&mut quiet, &mut [], &clock());
            let now = peaked.readout().bands[1];
            assert!(now <= before + 1e-6, "block {block}: {before} then {now}");
            before = now;
        }
        assert!(before < 0.05, "the ring never went dark: {before}");
    }

    /// `bands[2]` is the INPUT peak: it holds still while the blades
    /// close on the signal, so the card's `bands[2] - level_db` is this
    /// section's own loss.
    #[test]
    fn the_third_band_is_the_input_peak_so_the_loss_is_readable() {
        let l = sine(100.0, 0.5, FS as usize / 8);
        let mut wire = core_with(&[]);
        run(&mut wire, &l);
        let open = wire.readout();
        let half_db = 20.0 * 0.5f32.log10();
        assert!(
            (open.bands[2] - half_db).abs() < 0.5,
            "a half-scale sine read {} dBFS",
            open.bands[2]
        );
        assert!(
            (open.bands[2] - open.level_db).abs() < 0.2,
            "a wire lost level"
        );

        let mut shut = core_with(&[(p::HP_HZ, 4_000.0), (p::HP_RES, 0.0)]);
        run(&mut shut, &l);
        let closed = shut.readout();
        assert!(
            (closed.bands[2] - open.bands[2]).abs() < 0.5,
            "the input peak moved with the setting: {} vs {}",
            closed.bands[2],
            open.bands[2]
        );
        assert!(
            closed.bands[2] - closed.level_db > 24.0,
            "a shut blade lost only {} dB",
            closed.bands[2] - closed.level_db
        );
    }

    #[test]
    fn letters_land_clamped_and_retune() {
        let mut core = core_with(&[]);
        core.set_param(p::HP_HZ, 99_999.0);
        assert_eq!(core.shape().hp_hz, 4_000.0);
        core.set_param(p::LP_RES, -5.0);
        assert_eq!(core.shape().lp_res, 0.0);
        core.set_param(99, 1.0);
        core.set_param(p::HP_HZ, 20.0);
        assert!(core.shape().is_off());
    }
}
