//! FOUR: the console EQ.
//!
//! The engineer's hand, where TONE is the DJ's: a low band that is a
//! shelf or a bell, two proportional-Q mids with their own frequency
//! and Q, and a high band that is a shelf or a bell. Fifteen dB either
//! way on each.
//!
//! The one thing that is not textbook is the INDUCTOR BUMP. A passive
//! low shelf built round an inductor resonates a little just inside its
//! corner, and that dip-then-lift is why an old EQ's bottom sounds
//! tight where a clean shelf sounds woolly. So a low SHELF here carries
//! a small bell above its corner, opposite in sign and a quarter of the
//! gain; switch the band to a BELL and the bump goes with the shelf,
//! because it was the shelf's.
//!
//! Flat, the section is a wire to the sample. The shape lives green in
//! `crate::console::four_curve` so the card draws what the core runs.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::console::four_curve::Shape;
use crate::dsp::filters::EqBand;
use crate::params::console::four as p;

/// Four bands and the inductor's bump.
const BANDS: usize = 5;

/// The three zones the readout splits its output into, which are the
/// three zones the card lights down its spine.
const ZONES: usize = 3;

pub struct FourCore {
    params: SectionParams,
    shape: Shape,
    sample_rate: f32,
    bands: [[EqBand; BANDS]; 2],
    /// Which of the five actually run this setting.
    running: [bool; BANDS],
    level_db: f32,
    /// The analysis crossover's two one-pole coefficients, at
    /// `ZONE_LOW_HZ` and `ZONE_HIGH_HZ`, baked from the sample rate.
    zone_a: [f32; 2],
    /// Those two one-poles' state, per channel. Carried across blocks so
    /// a split block measures what a whole one does.
    zone_lp: [[f32; 2]; 2],
    /// The measured level of each zone of the OUTPUT, in dBFS.
    zone_db: [f32; ZONES],
}

/// A one-pole lowpass's coefficient at `hz`. Green: called from `new`
/// only, and the crossover never moves after that.
fn one_pole(hz: f32, sample_rate: f32) -> f32 {
    let fs = sample_rate.max(1.0);
    (1.0 - (-2.0 * core::f32::consts::PI * hz / fs).exp()).clamp(0.0, 1.0)
}

/// A block peak as dBFS: the silence sentinel when the block held
/// nothing, and otherwise the level, never reported below `floor`.
fn peak_db(peak: f32, floor: f32) -> f32 {
    if peak <= p::SILENCE_PEAK {
        p::SILENCE_DB
    } else {
        (20.0 * peak.log10()).max(floor)
    }
}

impl FourCore {
    pub fn new(params: &SectionParams, sample_rate: f32, _block: usize) -> Self {
        let mut core = Self {
            params: params.clone(),
            shape: Shape::of(params),
            sample_rate,
            bands: [[EqBand::new(); BANDS], [EqBand::new(); BANDS]],
            running: [false; BANDS],
            level_db: p::SILENCE_DB,
            zone_a: [
                one_pole(p::ZONE_LOW_HZ, sample_rate),
                one_pole(p::ZONE_HIGH_HZ, sample_rate),
            ],
            zone_lp: [[0.0; 2]; 2],
            zone_db: [p::SILENCE_DB; ZONES],
        };
        core.tune();
        core
    }

    pub fn shape(&self) -> Shape {
        self.shape
    }

    fn tune(&mut self) {
        let shape = self.shape;
        self.running = [false; BANDS];
        for (index, want) in shape.all().enumerate() {
            if want.db == 0.0 {
                continue;
            }
            self.running[index] = true;
            for ch in 0..2 {
                self.bands[ch][index].prepare(
                    self.sample_rate,
                    want.hz,
                    want.q,
                    want.db,
                    want.shape,
                );
            }
        }
    }

    fn run(&mut self, ch: usize, io: &mut [f32]) {
        for (index, on) in self.running.iter().enumerate() {
            if *on {
                self.bands[ch][index].process(io);
            }
        }
    }

    /// One channel's contribution to the block's peaks: the whole
    /// signal's, and each zone's. The two one-poles are the crossover —
    /// low is the 200 Hz lowpass, mid is what lies between the two
    /// lowpasses, high is what the 2 kHz one left behind — so the three
    /// sum back to the signal sample by sample. Fixed work per sample,
    /// no branch on the data, and the states decay through the denormal
    /// range on a fading tail like every other filter here, on engine
    /// FTZ.
    fn measure(&mut self, ch: usize, io: &[f32], peak: &mut f32, zones: &mut [f32; ZONES]) {
        let (a_low, a_high) = (self.zone_a[0], self.zone_a[1]);
        let mut lp_low = self.zone_lp[ch][0];
        let mut lp_high = self.zone_lp[ch][1];
        for &sample in io {
            lp_low += a_low * (sample - lp_low);
            lp_high += a_high * (sample - lp_high);
            *peak = peak.max(sample.abs());
            zones[0] = zones[0].max(lp_low.abs());
            zones[1] = zones[1].max((lp_high - lp_low).abs());
            zones[2] = zones[2].max((sample - lp_high).abs());
        }
        self.zone_lp[ch][0] = lp_low;
        self.zone_lp[ch][1] = lp_high;
    }
}

impl SectionCore for FourCore {
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
            for band in &mut self.bands[ch] {
                band.reset();
            }
        }
        self.level_db = p::SILENCE_DB;
        self.zone_lp = [[0.0; 2]; 2];
        self.zone_db = [p::SILENCE_DB; ZONES];
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;
        if !self.shape.is_flat() {
            self.run(0, l);
            if stereo {
                self.run(1, &mut r[..n]);
            }
        }
        let mut peak = 0.0f32;
        let mut zones = [0.0f32; ZONES];
        self.measure(0, l, &mut peak, &mut zones);
        if stereo {
            self.measure(1, &r[..n], &mut peak, &mut zones);
        }
        self.level_db = peak_db(peak, p::SILENCE_DB);
        for (slot, zone) in self.zone_db.iter_mut().zip(zones) {
            *slot = peak_db(zone, p::ZONE_FLOOR_DB);
        }
    }

    /// What the block just processed sounded like. A pure copy of
    /// fields — the graph's telemetry step calls this, so every figure
    /// was measured in `process`.
    ///
    /// - `level_db`: the loudest sample of the section's OUTPUT over the
    ///   block, in dBFS. Runs from about 0.0 down to the noise, and is
    ///   exactly -120.0 when the block held nothing. A block peak: no
    ///   smoothing at all, one fresh figure per block, so its time
    ///   constant is the block itself and any easing is the card's.
    /// - `bands[0..3]`: the same block peak, in dBFS, of the OUTPUT's
    ///   three frequency zones — `bands[0]` below 200 Hz, `bands[1]`
    ///   from 200 Hz to 2 kHz, `bands[2]` above 2 kHz. Measured AFTER
    ///   the EQ, so boosting a band lifts its zone; measured on the
    ///   signal, so at a flat setting they still report whatever the
    ///   music is doing. Range about 0.0 down to a floor of -72.0, and
    ///   exactly -120.0 when the block held nothing. The split is a pair
    ///   of one-pole lowpasses (6 dB/octave, so neighbouring zones leak
    ///   into each other by design rather than by accident), whose own
    ///   time constants are 0.8 ms at 200 Hz and 0.08 ms at 2 kHz and
    ///   whose state carries across blocks; over that, the same
    ///   per-block peak with no smoothing.
    /// - `reduction_db`: always 0.0. An EQ takes nothing away as a
    ///   whole, so there is no reduction figure to give.
    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: 0.0,
            bands: self.zone_db,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::console::SectionKind;
    use crate::console::four_curve::response_db;
    use crate::params::console::four as p;

    const FS: f32 = 48_000.0;
    const BLOCK: usize = 256;

    fn clock() -> Clock {
        Clock {
            playing: true,
            beat: 0.0,
            beats_per_sample: 120.0 / 60.0 / f64::from(FS),
        }
    }

    fn core_with(edits: &[(u32, f32)]) -> FourCore {
        let mut params = SectionParams::of(SectionKind::Four);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        FourCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    /// The loudest each zone's readout reached over the settled half of
    /// a sine run. A band is a per-block peak, and a block is a fifth of
    /// a 40 Hz cycle, so the honest reading of a low tone is the biggest
    /// block peak over a run, not any single block's.
    fn zones(core: &mut FourCore, hz: f32, amp: f32) -> [f32; 3] {
        let n = FS as usize / 4;
        let mut buf = sine(hz, amp, n);
        let mut loudest = [p::SILENCE_DB; 3];
        for start in (0..n).step_by(BLOCK) {
            let end = (start + BLOCK).min(n);
            core.process(&mut buf[start..end], &mut [], &clock());
            if start * 2 < n {
                continue;
            }
            let said = core.readout();
            for (best, band) in loudest.iter_mut().zip(said.bands) {
                *best = best.max(band);
            }
        }
        loudest
    }

    fn gain_db(core: &mut FourCore, hz: f32) -> f32 {
        let n = FS as usize / 2;
        let l = sine(hz, 0.1, n);
        let mut out = l.clone();
        for start in (0..n).step_by(BLOCK) {
            let end = (start + BLOCK).min(n);
            core.process(&mut out[start..end], &mut [], &clock());
        }
        20.0 * (rms(&out[n / 2..]) / rms(&l[n / 2..])).log10()
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

    /// Each band lands on its own frequency and leaves the others.
    #[test]
    fn four_bands_four_places() {
        let mut lmf = core_with(&[(p::LMF_HZ, 400.0), (p::LMF_DB, 9.0), (p::LMF_Q, 2.0)]);
        assert!((gain_db(&mut lmf, 400.0) - 9.0).abs() < 0.5);
        assert!(gain_db(&mut lmf, 40.0).abs() < 0.5);
        assert!(gain_db(&mut lmf, 10_000.0).abs() < 0.5);

        let mut hmf = core_with(&[(p::HMF_HZ, 3_000.0), (p::HMF_DB, -12.0), (p::HMF_Q, 2.0)]);
        assert!((gain_db(&mut hmf, 3_000.0) + 12.0).abs() < 0.5);
        assert!(gain_db(&mut hmf, 200.0).abs() < 0.5);

        let mut high = core_with(&[(p::HIGH_DB, 6.0)]);
        assert!((gain_db(&mut high, 15_000.0) - 6.0).abs() < 1.0);
        assert!(gain_db(&mut high, 200.0).abs() < 0.5);
    }

    /// Q narrows the mid: at a high Q an octave off centre keeps less
    /// of the boost than at a low one.
    #[test]
    fn the_mids_q_narrows_them() {
        let share = |q: f32| -> f32 {
            let mut core = core_with(&[(p::LMF_HZ, 500.0), (p::LMF_DB, 12.0), (p::LMF_Q, q)]);
            let off = gain_db(&mut core, 1_000.0);
            let mut core = core_with(&[(p::LMF_HZ, 500.0), (p::LMF_DB, 12.0), (p::LMF_Q, q)]);
            off / gain_db(&mut core, 500.0)
        };
        assert!(
            share(0.5) > share(4.0) + 0.2,
            "{} vs {}",
            share(0.5),
            share(4.0)
        );
    }

    /// The inductor bump: a low SHELF dips just above its corner, so
    /// the boost there is less than the shelf alone would give; a low
    /// BELL has no bump at all.
    #[test]
    fn a_shelf_carries_the_inductors_bump_and_a_bell_does_not() {
        let mut shelf = core_with(&[(p::LOW_HZ, 100.0), (p::LOW_DB, 12.0)]);
        let deep = gain_db(&mut shelf, 40.0);
        let mut shelf = core_with(&[(p::LOW_HZ, 100.0), (p::LOW_DB, 12.0)]);
        let bumped = gain_db(&mut shelf, 160.0);
        assert!(
            (deep - 12.0).abs() < 1.0,
            "the shelf is not a shelf: {deep} dB"
        );
        assert!(
            bumped < deep - 2.0,
            "no bump: deep {deep} dB, at the corner {bumped} dB"
        );

        let mut bell = core_with(&[(p::LOW_HZ, 100.0), (p::LOW_DB, 12.0), (p::LOW_SHAPE, 1.0)]);
        assert!(bell.shape().inductor.is_none());
        let mut bell = core_with(&[(p::LOW_HZ, 100.0), (p::LOW_DB, 12.0), (p::LOW_SHAPE, 1.0)]);
        assert!(
            gain_db(&mut bell, 30.0) < 6.0,
            "a bell reached the bottom like a shelf"
        );
    }

    /// The curve the card draws is the curve the core runs.
    #[test]
    fn the_drawn_response_is_the_measured_one() {
        let edits = [
            (p::LOW_DB, 6.0),
            (p::LMF_DB, -8.0),
            (p::LMF_HZ, 700.0),
            (p::HMF_DB, 5.0),
            (p::HIGH_DB, -4.0),
        ];
        let mut core = core_with(&edits);
        let shape = core.shape();
        for hz in [40.0, 160.0, 700.0, 2_500.0, 12_000.0] {
            let drawn = response_db(&shape, FS, hz);
            let heard = gain_db(&mut core, hz);
            assert!(
                (drawn - heard).abs() < 0.75,
                "{hz} Hz: drawn {drawn}, heard {heard}"
            );
        }
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 0.4, 1000);
        let edits = [(p::LOW_DB, -6.0), (p::LMF_DB, 5.0), (p::HIGH_DB, 3.0)];
        let mut whole = core_with(&edits);
        let mut a = l.clone();
        for start in (0..a.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(a.len());
            whole.process(&mut a[start..end], &mut [], &clock());
        }
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

    /// The three bands are the MUSIC, not the settings. This section is
    /// at its defaults throughout — every band out of circuit, the core
    /// a wire — and the readout still moves with what is played through
    /// it: the zone the tone sits in is the loud one, every time.
    #[test]
    fn the_bands_carry_the_music_and_not_the_settings() {
        let low = zones(&mut core_with(&[]), 40.0, 0.5);
        let mid = zones(&mut core_with(&[]), 700.0, 0.5);
        let high = zones(&mut core_with(&[]), 8_000.0, 0.5);
        assert!(
            low[0] > low[1] + 6.0 && low[0] > low[2] + 6.0,
            "40 Hz did not light the low zone: {low:?}"
        );
        assert!(
            mid[1] > mid[0] + 6.0 && mid[1] > mid[2] + 6.0,
            "700 Hz did not light the mid zone: {mid:?}"
        );
        assert!(
            high[2] > high[0] + 6.0 && high[2] > high[1] + 6.0,
            "8 kHz did not light the high zone: {high:?}"
        );
        // A louder tone reads louder in its own zone, dB for dB.
        let quiet = zones(&mut core_with(&[]), 700.0, 0.05);
        assert!(
            (mid[1] - quiet[1] - 20.0).abs() < 1.5,
            "20 dB in gave {} dB out",
            mid[1] - quiet[1]
        );
    }

    /// The zones are measured AFTER the EQ, which is the whole point of
    /// measuring them: cut the low band and the low zone falls with it,
    /// boost a mid that sits above the 2 kHz split and the high zone
    /// rises with that.
    #[test]
    fn the_zones_are_measured_after_the_eq() {
        let flat = zones(&mut core_with(&[]), 40.0, 0.5)[0];
        let cut = zones(&mut core_with(&[(p::LOW_DB, -15.0)]), 40.0, 0.5)[0];
        assert!(
            cut < flat - 10.0,
            "a 15 dB low cut moved the low zone {} dB",
            cut - flat
        );

        let boost = [(p::HMF_HZ, 3_000.0), (p::HMF_DB, 12.0), (p::HMF_Q, 1.0)];
        let plain = zones(&mut core_with(&[]), 3_000.0, 0.2)[2];
        let lifted = zones(&mut core_with(&boost), 3_000.0, 0.2)[2];
        assert!(
            lifted > plain + 9.0,
            "a 12 dB mid boost moved the high zone {} dB",
            lifted - plain
        );
    }

    /// At its defaults, on silence, the section reports rest: the
    /// sentinel on every level, no reduction, and nothing left over from
    /// the last thing it heard once it is reset.
    #[test]
    fn at_rest_the_readout_is_silence() {
        let mut core = core_with(&[]);
        let mut quiet = [0.0f32; BLOCK];
        core.process(&mut quiet, &mut [], &clock());
        let said = core.readout();
        assert_eq!(said.level_db, p::SILENCE_DB);
        assert_eq!(said.bands, [p::SILENCE_DB; 3]);
        assert_eq!(said.reduction_db, 0.0);

        let loud = zones(&mut core, 700.0, 0.5);
        assert!(loud[1] > -20.0, "the run said nothing: {loud:?}");
        core.reset();
        let said = core.readout();
        assert_eq!(said.level_db, p::SILENCE_DB);
        assert_eq!(said.bands, [p::SILENCE_DB; 3]);
    }

    #[test]
    fn letters_land_clamped() {
        let mut core = core_with(&[]);
        core.set_param(p::LOW_DB, 40.0);
        assert_eq!(core.shape().bands[0].db, 15.0);
        core.set_param(p::LMF_Q, 99.0);
        assert_eq!(core.shape().bands[1].q, 4.0);
        core.set_param(99, 1.0);
        core.set_param(p::LOW_DB, 0.0);
        assert!(core.shape().is_flat());
    }
}
