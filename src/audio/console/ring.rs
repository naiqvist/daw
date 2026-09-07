//! RING: ring modulation.
//!
//! The sound multiplied by a carrier, which is the one effect that
//! moves every partial by the same number of hertz instead of the same
//! ratio — so what comes out is inharmonic, bell-like, and nothing a
//! filter or a distortion can imitate. Four carriers: a band-limited
//! SINE, TRIANGLE and SQUARE from the oscillator's mip tables, so a
//! high carrier does not alias, and NOISE, low-passed, which turns the
//! sound to gravel rather than to a bell.
//!
//! HOLD is the thing a plain ring modulator lacks: a sample-and-hold
//! walks the carrier's pitch by up to an octave and a half, stepping on
//! its own clock, so the metal moves. MIX blends against the dry and at
//! zero the section is a wire to the sample.

use crate::audio::console::{Clock, SectionCore};
use crate::audio::graph::Readout;
use crate::console::SectionParams;
use crate::dsp::filters::OnePole;
use crate::dsp::lfo::SampleHold;
use crate::dsp::noise::WhiteNoise;
use crate::dsp::osc::{MipOsc, Waveform, build_tables, table_len};
use crate::params::console::ring as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub carrier: u32,
    pub hz: f32,
    pub hold_rate: f32,
    pub mix: f32,
}

impl Settings {
    pub fn of(params: &SectionParams) -> Self {
        let table = crate::console::SectionKind::Ring.table();
        let clamp = |id: u32| {
            let value = params.value(id);
            table
                .iter()
                .find(|def| def.id == id)
                .map_or(value, |def| def.clamp(value))
        };
        Self {
            carrier: (clamp(p::CARRIER).round().max(0.0) as u32).min(p::NOISE),
            hz: clamp(p::HZ),
            hold_rate: clamp(p::HOLD_RATE),
            mix: clamp(p::MIX) / 100.0,
        }
    }

    pub fn is_wire(&self) -> bool {
        self.mix == 0.0
    }
}

fn waveform_of(carrier: u32) -> Waveform {
    match carrier {
        p::TRIANGLE => Waveform::Triangle,
        p::SQUARE => Waveform::Square,
        _ => Waveform::Sine,
    }
}

const TONE_CARRIERS: [Waveform; 3] = [Waveform::Sine, Waveform::Triangle, Waveform::Square];

fn carrier_index(carrier: u32) -> usize {
    match carrier {
        p::TRIANGLE => 1,
        p::SQUARE => 2,
        _ => 0,
    }
}

pub struct RingCore {
    params: SectionParams,
    settings: Settings,
    sample_rate: f32,
    osc: MipOsc,
    /// Every tonal carrier's tables, built before the core can reach the
    /// callback. A carrier letter therefore selects one slice in O(1);
    /// it never performs Fourier synthesis on the audio thread.
    tables: [Vec<f32>; TONE_CARRIERS.len()],
    built: Waveform,
    noise: WhiteNoise,
    band: OnePole,
    hold: SampleHold,
    /// Compile-owned scratch: the carrier, and the hold's walk.
    carrier: Vec<f32>,
    walk: Vec<f32>,
    level_db: f32,
    /// The carrier's frequency at the last block's final sample, in Hz;
    /// zero when there is no carrier tone to name.
    carrier_hz: f32,
    /// The last block's mean |carrier|, linear 0..1.
    carrier_mean: f32,
    /// The last block's peak |dry|, linear, taken before the crossfade.
    dry_peak: f32,
}

impl RingCore {
    pub fn new(params: &SectionParams, sample_rate: f32, block: usize) -> Self {
        let settings = Settings::of(params);
        let waveform = waveform_of(settings.carrier);
        // Green zone: pay for all tonal carriers once. `set_param` runs on
        // the audio thread, and building even one of these is hundreds of
        // thousands of transcendental operations despite being allocation
        // free.
        let tables = core::array::from_fn(|index| {
            let waveform = TONE_CARRIERS[index];
            let mut data = vec![0.0; table_len(waveform)];
            build_tables(waveform, &mut data);
            data
        });
        let mut osc = MipOsc::new();
        osc.prepare(sample_rate, waveform);
        let mut noise = WhiteNoise::new();
        noise.seed(0x7269_6e67);
        let mut band = OnePole::new();
        band.prepare(sample_rate, p::NOISE_HZ);
        let mut hold = SampleHold::new();
        hold.prepare(sample_rate);
        hold.seed(0x686f_6c64);
        let mut core = Self {
            params: params.dense(),
            settings,
            sample_rate,
            osc,
            tables,
            built: waveform,
            noise,
            band,
            hold,
            carrier: vec![0.0; block.max(1)],
            walk: vec![0.0; block.max(1)],
            level_db: p::LEVEL_FLOOR_DB,
            carrier_hz: 0.0,
            carrier_mean: 0.0,
            dry_peak: 0.0,
        };
        core.tune();
        core
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    /// Red zone safe: select the already-built carrier and update its cheap
    /// scalar controls. `set_param` calls this on the audio thread.
    fn tune(&mut self) {
        let s = self.settings;
        let waveform = waveform_of(s.carrier);
        if waveform != self.built {
            self.osc.prepare(self.sample_rate, waveform);
            self.built = waveform;
        }
        self.osc.set_freq(s.hz);
        self.hold.set_rate(s.hold_rate.max(0.001));
    }
}

impl SectionCore for RingCore {
    fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        let next = Settings::of(&self.params);
        if next != self.settings {
            self.settings = next;
            self.tune();
        }
    }

    fn reset(&mut self) {
        self.noise.reset();
        self.band.reset();
        self.hold.reset();
        self.level_db = p::LEVEL_FLOOR_DB;
        self.carrier_hz = 0.0;
        self.carrier_mean = 0.0;
        self.dry_peak = 0.0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _clock: &Clock) {
        let n = l.len();
        if n == 0 || n > self.carrier.len() {
            return;
        }
        let stereo = r.len() >= n;
        let s = self.settings;
        // The DRY peak, taken before the crossfade. With `level_db`,
        // which is taken after it, the pair is the section's true gain
        // — and the proof that MIX at zero is a wire.
        self.dry_peak = l
            .iter()
            .chain(if stereo { r[..n].iter() } else { [].iter() })
            .fold(0.0f32, |peak, s| peak.max(s.abs()));
        // A shorted bridge has no carrier: no frequency, no burn.
        self.carrier_hz = 0.0;
        self.carrier_mean = 0.0;
        if !s.is_wire() {
            let carrier = &mut self.carrier[..n];
            if s.carrier == p::NOISE {
                self.noise.process(carrier);
                self.band.process_lowpass(carrier);
                // The band-limited noise is quiet; bring it back to a
                // modulator's range.
                for c in carrier.iter_mut() {
                    *c = (*c * 4.0).clamp(-1.0, 1.0);
                }
                // Noise has no frequency; `carrier_hz` stays zero so
                // the face does not draw a post that means nothing.
            } else if s.hold_rate > 0.0 {
                // The walk steps the pitch; the oscillator is retuned
                // per sample, which is what it is built for.
                self.hold.process(&mut self.walk[..n]);
                let mut hz = s.hz;
                for (c, step) in carrier.iter_mut().zip(&self.walk[..n]) {
                    hz = s.hz * 2f32.powf(p::HOLD_OCTAVES * *step);
                    self.osc.set_freq(hz);
                    let mut one = [0.0f32; 1];
                    self.osc
                        .process(&mut one, &self.tables[carrier_index(s.carrier)]);
                    *c = one[0];
                }
                // Where the walk left the carrier at the block's end.
                self.carrier_hz = hz;
            } else {
                self.osc.set_freq(s.hz);
                self.osc
                    .process(carrier, &self.tables[carrier_index(s.carrier)]);
                self.carrier_hz = s.hz;
            }
            let mut sum = 0.0f32;
            for (y, c) in l.iter_mut().zip(carrier.iter()) {
                sum += c.abs();
                *y += (*y * *c - *y) * s.mix;
            }
            // The loop above ran exactly `n` times, and `n` is not
            // zero: a divide, not a branch.
            self.carrier_mean = sum / n as f32;
            if stereo {
                for (y, c) in r[..n].iter_mut().zip(carrier.iter()) {
                    *y += (*y * *c - *y) * s.mix;
                }
            }
        }
        let peak = l
            .iter()
            .chain(if stereo { r[..n].iter() } else { [].iter() })
            .fold(0.0f32, |peak, s| peak.max(s.abs()));
        self.level_db = if peak <= p::LEVEL_SILENCE {
            p::LEVEL_FLOOR_DB
        } else {
            20.0 * peak.log10()
        };
    }

    /// What RING tells its card. Every figure is measured in
    /// [`RingCore::process`] over the block that just ran and copied
    /// here untouched: nothing is smoothed and nothing is peak-held, so
    /// each field's time constant is one block and the JUMP is the
    /// instrument. A block the core skipped (empty, or longer than the
    /// scratch) leaves the last report standing.
    ///
    /// - `level_db`: the loudest sample the section PUT OUT, post
    ///   crossfade, in dBFS over [`p::LEVEL_FLOOR_DB`]..0. The floor is
    ///   silence, not a number to draw.
    /// - `reduction_db`: always `0.0`. RING reduces nothing.
    /// - `bands[0]`: the carrier's LIVE frequency at the block's final
    ///   sample, in Hz. It is exactly HZ (20..5000) while HOLD is zero,
    ///   and while HOLD runs it steps inside
    ///   `hz / 2^HOLD_OCTAVES ..= hz * 2^HOLD_OCTAVES` — a fence three
    ///   octaves wide — moving only when the hold's own clock ticks.
    ///   `0.0` means there is no post to draw: the carrier is NOISE, or
    ///   MIX is zero and the bridge is shorted.
    /// - `bands[1]`: the block's MEAN |carrier|, linear 0..1, averaged
    ///   over that block alone. Each carrier has its own steady value,
    ///   measured: SQUARE ~0.98, SINE ~0.64 (2/pi), TRIANGLE ~0.50 —
    ///   dead still block to block — while band-limited NOISE sits
    ///   around ~0.67 and WANDERS over roughly 0.58..0.73, which is the
    ///   one carrier the face can see is unsteady. `0.0` when MIX is
    ///   zero.
    /// - `bands[2]`: the block's PEAK |dry|, LINEAR, taken BEFORE the
    ///   crossfade — 0..1 for a signal at or under full scale, and not
    ///   clamped, so a hot input reads above 1. It does not move with
    ///   MIX. `10^(level_db / 20) / bands[2]` is therefore the
    ///   section's true gain, and is exactly 1 while MIX is zero.
    fn readout(&self) -> Readout {
        Readout {
            level_db: self.level_db,
            reduction_db: 0.0,
            bands: [self.carrier_hz, self.carrier_mean, self.dry_peak],
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    const FS: f32 = 48_000.0;
    const BLOCK: usize = 256;

    fn clock() -> Clock {
        Clock {
            playing: true,
            beat: 0.0,
            beats_per_sample: 120.0 / 60.0 / f64::from(FS),
        }
    }

    fn core_with(edits: &[(u32, f32)]) -> RingCore {
        let mut params = SectionParams::of(SectionKind::Ring);
        for (id, value) in edits {
            params.set(*id, *value);
        }
        RingCore::new(&params, FS, BLOCK)
    }

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin())
            .collect()
    }

    fn run(core: &mut RingCore, l: &[f32]) -> Vec<f32> {
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

    /// The level at `hz`, in dB relative to `signal`'s loudest bin of
    /// the three tested.
    fn level_at(signal: &[f32], hz: f32) -> f32 {
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, s) in signal.iter().enumerate() {
            let w = 2.0 * core::f32::consts::PI * hz * i as f32 / FS;
            re += s * w.cos();
            im -= s * w.sin();
        }
        let mag = (re * re + im * im).sqrt() * 2.0 / signal.len() as f32;
        20.0 * mag.max(1e-9).log10()
    }

    #[test]
    fn mix_at_zero_is_a_wire_to_the_sample() {
        let mut core = core_with(&[]);
        assert!(core.settings().is_wire());
        for len in [0usize, 1, 7, BLOCK] {
            let l = sine(440.0, 0.5, len);
            let r: Vec<f32> = l.iter().map(|s| -s).collect();
            let (mut ol, mut or) = (l.clone(), r.clone());
            core.process(&mut ol, &mut or, &clock());
            assert_eq!(ol, l);
            assert_eq!(or, r);
        }
    }

    /// The sidebands are the sum and the difference, and the tone
    /// itself is gone: that is what ring modulation is.
    #[test]
    fn it_makes_the_sum_and_the_difference_and_keeps_neither() {
        let n = FS as usize / 2;
        let window = n / 2..n / 2 + 9600;
        let l = sine(1_000.0, 0.5, n);
        let mut core = core_with(&[(p::CARRIER, 0.0), (p::HZ, 300.0), (p::MIX, 100.0)]);
        let out = run(&mut core, &l);
        let low = level_at(&out[window.clone()], 700.0);
        let high = level_at(&out[window.clone()], 1_300.0);
        let tone = level_at(&out[window], 1_000.0);
        assert!(low > -20.0, "no difference sideband: {low} dBFS");
        assert!(high > -20.0, "no sum sideband: {high} dBFS");
        assert!(tone < low - 20.0, "the tone survived: {tone} against {low}");
    }

    /// The four carriers all modulate, and the noise one is not a tone.
    #[test]
    fn every_carrier_modulates() {
        let n = FS as usize / 4;
        let l = sine(1_000.0, 0.5, n);
        for carrier in 0..=3 {
            let mut core = core_with(&[
                (p::CARRIER, carrier as f32),
                (p::HZ, 400.0),
                (p::MIX, 100.0),
            ]);
            let out = run(&mut core, &l);
            let changed = rms(&out[n / 2..]
                .iter()
                .zip(&l[n / 2..])
                .map(|(a, b)| a - b)
                .collect::<Vec<_>>());
            assert!(changed > 0.05, "carrier {carrier} did nothing");
            assert!(
                out.iter().all(|s| s.abs() <= 1.0),
                "carrier {carrier} ran hot"
            );
        }
        // Noise has no sideband to find: its energy is spread.
        let mut noisy = core_with(&[(p::CARRIER, 3.0), (p::HZ, 400.0), (p::MIX, 100.0)]);
        let out = run(&mut noisy, &l);
        let window = n / 2..n / 2 + 4800;
        let sideband = level_at(&out[window], 600.0);
        assert!(
            sideband < -20.0,
            "the noise carrier made a tone: {sideband} dBFS"
        );
    }

    /// The hold walks the carrier, so the sidebands move.
    #[test]
    fn the_hold_walks_the_carrier() {
        let n = FS as usize / 2;
        let l = sine(1_000.0, 0.5, n);
        let mut still = core_with(&[(p::HZ, 300.0), (p::MIX, 100.0)]);
        let a = run(&mut still, &l);
        let mut walking = core_with(&[(p::HZ, 300.0), (p::MIX, 100.0), (p::HOLD_RATE, 8.0)]);
        let b = run(&mut walking, &l);
        let sideband_of = |out: &[f32]| level_at(&out[n / 2..n / 2 + 9600], 700.0);
        assert!(
            sideband_of(&a) > -20.0,
            "the still carrier lost its sideband"
        );
        assert!(
            sideband_of(&b) < sideband_of(&a) - 6.0,
            "the walk left the sideband put: {} against {}",
            sideband_of(&b),
            sideband_of(&a)
        );
    }

    #[test]
    fn split_blocks_are_equivalent() {
        let l = sine(330.0, 0.4, 1000);
        let edits = [(p::CARRIER, 1.0), (p::HZ, 500.0), (p::MIX, 80.0)];
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
    fn letters_land_clamped() {
        let mut core = core_with(&[]);
        core.set_param(p::CARRIER, 9.0);
        assert_eq!(core.settings().carrier, p::NOISE);
        core.set_param(p::HZ, 99_999.0);
        assert_eq!(core.settings().hz, 5_000.0);
        core.set_param(99, 1.0);
        core.set_param(p::MIX, 0.0);
        assert!(core.settings().is_wire());
    }

    #[test]
    fn switching_tonal_carriers_is_allocation_free_in_the_callback() {
        let mut core = core_with(&[(p::MIX, 100.0)]);
        let mut l = sine(440.0, 0.5, BLOCK);
        let mut r = l.clone();
        assert_no_alloc::assert_no_alloc(|| {
            for carrier in [p::SINE, p::TRIANGLE, p::SQUARE] {
                core.set_param(p::CARRIER, carrier as f32);
                core.process(&mut l, &mut r, &clock());
            }
        });
    }

    /// bands[0] is the carrier's LIVE frequency, not the HZ letter: it
    /// sits exactly on the set value while HOLD is zero, steps about
    /// inside the walk's three-octave fence while HOLD runs, and is
    /// zero for the noise carrier, which has no frequency to name.
    #[test]
    fn band_zero_is_the_carriers_live_frequency() {
        let mut still = core_with(&[(p::HZ, 300.0), (p::MIX, 100.0)]);
        let mut block = sine(1_000.0, 0.5, BLOCK);
        still.process(&mut block, &mut [], &clock());
        assert_eq!(still.readout().bands[0], 300.0, "the still post moved");

        let mut walking = core_with(&[(p::HZ, 300.0), (p::MIX, 100.0), (p::HOLD_RATE, 50.0)]);
        let mut seen = Vec::new();
        for _ in 0..24 {
            let mut block = sine(1_000.0, 0.5, BLOCK);
            walking.process(&mut block, &mut [], &clock());
            seen.push(walking.readout().bands[0]);
        }
        let low = 300.0 / 2f32.powf(p::HOLD_OCTAVES);
        let high = 300.0 * 2f32.powf(p::HOLD_OCTAVES);
        assert!(
            seen.iter().all(|hz| (low..=high).contains(hz)),
            "the walk left its fence {low}..{high}: {seen:?}"
        );
        let moves = seen.windows(2).filter(|w| w[0] != w[1]).count();
        assert!(moves >= 4, "the walk did not step: {seen:?}");
        assert!(
            seen.iter().any(|hz| *hz < 300.0) && seen.iter().any(|hz| *hz > 300.0),
            "the walk stayed on one side of the post: {seen:?}"
        );

        let mut noisy = core_with(&[(p::CARRIER, p::NOISE as f32), (p::MIX, 100.0)]);
        let mut block = sine(1_000.0, 0.5, BLOCK);
        noisy.process(&mut block, &mut [], &clock());
        assert_eq!(
            noisy.readout().bands[0],
            0.0,
            "the noise carrier named a frequency"
        );
    }

    /// bands[1] is the block's mean |carrier|, and each shape has its
    /// own steady value: measured at a carrier that fits the block
    /// whole, SINE lands on 2/pi, TRIANGLE on 1/2 and SQUARE just under
    /// 1, each dead still block to block — while the noise carrier
    /// wanders, because a block of noise is a different block of noise
    /// every time.
    #[test]
    fn band_one_is_the_carriers_mean_and_each_shape_has_its_own() {
        // 375 Hz is exactly two cycles in a 256-sample block at 48 kHz,
        // so a tone carrier's block mean has nothing to wobble on.
        let burn = |carrier: u32| {
            let mut core = core_with(&[
                (p::CARRIER, carrier as f32),
                (p::HZ, 375.0),
                (p::MIX, 100.0),
            ]);
            let mut seen = Vec::new();
            for _ in 0..16 {
                let mut block = sine(1_000.0, 0.5, BLOCK);
                core.process(&mut block, &mut [], &clock());
                seen.push(core.readout().bands[1]);
            }
            let high = seen.iter().fold(f32::MIN, |a, b| a.max(*b));
            let low = seen.iter().fold(f32::MAX, |a, b| a.min(*b));
            (seen[15], high - low)
        };
        let (sine_burn, sine_spread) = burn(p::SINE);
        let (tri_burn, tri_spread) = burn(p::TRIANGLE);
        let (square_burn, square_spread) = burn(p::SQUARE);
        let (noise_burn, noise_spread) = burn(p::NOISE);
        assert!(
            (sine_burn - 0.637).abs() < 0.02,
            "the sine carrier burned at {sine_burn}, not 2/pi"
        );
        assert!(
            (tri_burn - 0.500).abs() < 0.02,
            "the triangle carrier burned at {tri_burn}, not 1/2"
        );
        assert!(
            (square_burn - 0.985).abs() < 0.03,
            "the square carrier burned at {square_burn}, not ~1"
        );
        assert!(
            (0.5..0.85).contains(&noise_burn),
            "the noise carrier burned at {noise_burn}"
        );
        assert!(
            square_burn > sine_burn && sine_burn > tri_burn,
            "the shapes lost their order: {square_burn} {sine_burn} {tri_burn}"
        );
        // Only the noise carrier's burn is unsteady, and that is the
        // face's tell that it is the noise one.
        assert!(
            noise_spread > 0.05,
            "the noise carrier's burn sat still: spread {noise_spread}"
        );
        for (name, spread) in [
            ("sine", sine_spread),
            ("triangle", tri_spread),
            ("square", square_spread),
        ] {
            assert!(
                spread < noise_spread / 10.0,
                "the {name} carrier's burn wandered: spread {spread}"
            );
        }
    }

    /// bands[2] is the DRY peak, taken before the crossfade: it reads
    /// the same at every MIX while what leaves does not, and paired
    /// with level_db it is the section's true gain.
    #[test]
    fn band_two_is_the_dry_peak_taken_before_the_crossfade() {
        for mix in [0.0f32, 40.0, 100.0] {
            let mut core = core_with(&[(p::HZ, 375.0), (p::MIX, mix)]);
            let mut block = sine(1_000.0, 0.5, BLOCK);
            core.process(&mut block, &mut [], &clock());
            let said = core.readout();
            assert!(
                (said.bands[2] - 0.5).abs() < 1e-3,
                "the dry moved with MIX {mix}: {}",
                said.bands[2]
            );
            assert_eq!(said.reduction_db, 0.0, "RING reduced something");
        }
        // A carrier slow enough to stay near zero across the block
        // takes the output well down while the dry stands where it was:
        // the two cannot be the same measurement.
        let mut slow = core_with(&[(p::HZ, 20.0), (p::MIX, 100.0)]);
        let mut block = sine(1_000.0, 0.5, BLOCK);
        slow.process(&mut block, &mut [], &clock());
        let said = slow.readout();
        let gain = 10f32.powf(said.level_db / 20.0) / said.bands[2];
        assert!(
            gain < 0.8,
            "the dry peak followed the crossfade: gain {gain}"
        );
    }

    /// At its defaults the section is a wire, and the readout says so
    /// rather than repeating the letters: no carrier frequency, no
    /// burn, and a true gain of exactly 1 — which is the balance dot's
    /// resting size and the visual proof of the wire.
    #[test]
    fn at_its_defaults_the_section_reports_rest() {
        let mut core = core_with(&[]);
        assert!(core.settings().is_wire());
        let fresh = core.readout();
        assert_eq!(
            fresh.bands,
            [0.0, 0.0, 0.0],
            "a core at rest said something"
        );
        assert_eq!(fresh.level_db, p::LEVEL_FLOOR_DB);

        let mut block = sine(440.0, 0.5, BLOCK);
        core.process(&mut block, &mut [], &clock());
        let said = core.readout();
        assert_eq!(said.bands[0], 0.0, "a shorted bridge named a frequency");
        assert_eq!(said.bands[1], 0.0, "a shorted bridge burned");
        let gain = 10f32.powf(said.level_db / 20.0) / said.bands[2];
        assert!(
            (gain - 1.0).abs() < 1e-3,
            "the wire's gain was not 1: {gain}"
        );

        // A discontinuity puts every measured field back to rest.
        core.reset();
        let after = core.readout();
        assert_eq!(after.bands, [0.0, 0.0, 0.0]);
        assert_eq!(after.level_db, p::LEVEL_FLOOR_DB);
    }
}
