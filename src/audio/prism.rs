//! PRISM — three-band dynamics with an opinion.
//!
//! Wiring, not arithmetic: [`Crossover3`] splits, and each band gets the
//! same [`RmsDetector`] / [`GainComputer`] / [`Ballistics`] trio the
//! glue and the clamp are built from. What is new here is what those
//! three are asked to do.
//!
//! # One signed knob per band
//!
//! A band has a THRESHOLD and an AMOUNT. The amount's sign is the
//! direction: positive compresses downward, negative compresses UPWARD —
//! the same computer in [`Mode::Expand`], negated, which is exactly what
//! upward compression is. Its magnitude is the ratio. The alternative,
//! and what every other multiband does, is two thresholds and two ratios
//! per band; that is twenty-four numbers before a sound has moved, and
//! most of them are the same number.
//!
//! # The colour is earned
//!
//! HEAT is a saturation whose AMOUNT is the band's own gain movement,
//! sample by sample. A band doing nothing is bit-for-bit clean — the
//! promise every colour stage here makes — and a band working hard is
//! warm. That is a claim about what compression should sound like, and
//! it is the opposite of the neutral utility a multiband usually is.
//!
//! # The low band is slower, and cannot not be
//!
//! GRIP sets attack and release for the whole device, and then each band
//! floors them at a fraction of a period of its own lower corner. A
//! detector cannot measure the level of a 60 Hz tone in less than a
//! 60 Hz cycle; asked to, it tracks the waveform instead of the envelope,
//! which is not fast compression but distortion with a threshold on it.
//! So the low band is slower than the high band at the same setting, and
//! the card says so rather than offering a number it cannot honour.
//!
//! # Red zone
//!
//! Everything but [`Prism::new`] and [`Prism::prepare`] runs in the
//! callback. The band buffers are fixed-size arrays inside the struct,
//! so the split works in bounded chunks and nothing is ever allocated.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::crossover::Crossover3;
use crate::dsp::dynamics::{Ballistics, GainComputer, Mode, RmsDetector};
use crate::dsp::shaper::{self, Waveshaper};
use crate::params::prism as p;

/// How much of a block the band buffers hold at once.
///
/// The whole reason this device needs a chunk at all: three bands times
/// two channels is six buffers, and they live in the struct rather than
/// on the heap. A hundred and twenty-eight frames is 3 kB of state and
/// far more than a callback ever asks for in one go.
const CHUNK: usize = 128;

/// One band's knobs, in engine units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BandParams {
    pub threshold_db: f32,
    /// SIGNED. Positive compresses down, negative lifts up, zero does
    /// nothing at all.
    pub amount: f32,
    pub heat: f32,
    pub trim_db: f32,
}

impl Default for BandParams {
    fn default() -> Self {
        Self {
            threshold_db: -18.0,
            amount: 0.0,
            heat: 0.0,
            trim_db: 0.0,
        }
    }
}

impl BandParams {
    fn get(&self, which: u32) -> Option<f32> {
        Some(match which {
            p::band::THRESHOLD => self.threshold_db,
            p::band::AMOUNT => self.amount,
            p::band::HEAT => self.heat,
            p::band::TRIM => self.trim_db,
            _ => return None,
        })
    }

    fn set(&mut self, which: u32, value: f32) {
        match which {
            p::band::THRESHOLD => self.threshold_db = value,
            p::band::AMOUNT => self.amount = value,
            p::band::HEAT => self.heat = value,
            p::band::TRIM => self.trim_db = value,
            _ => {}
        }
    }
}

/// The whole device's knobs.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PrismParams {
    pub low_hz: f32,
    pub high_hz: f32,
    pub grip: f32,
    pub mix: f32,
    pub output_db: f32,
    pub bands: [BandParams; p::BANDS],
}

impl Default for PrismParams {
    fn default() -> Self {
        let get = |id: u32| crate::params::def(p::TABLE, id).default;
        let band = |b: usize| BandParams {
            threshold_db: get(p::param(b, p::band::THRESHOLD)),
            amount: get(p::param(b, p::band::AMOUNT)),
            heat: get(p::param(b, p::band::HEAT)),
            trim_db: get(p::param(b, p::band::TRIM)),
        };
        Self {
            low_hz: get(p::LOW_X),
            high_hz: get(p::HIGH_X),
            grip: get(p::GRIP),
            mix: get(p::MIX),
            output_db: get(p::OUTPUT),
            bands: [band(0), band(1), band(2)],
        }
    }
}

impl PrismParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(p::TABLE, param, value) else {
            return;
        };
        if let Some((band, which)) = p::split(param) {
            if let Some(slot) = self.bands.get_mut(band) {
                slot.set(which, value);
            }
            return;
        }
        match param {
            p::LOW_X => self.low_hz = value,
            p::HIGH_X => self.high_hz = value,
            p::GRIP => self.grip = value,
            p::MIX => self.mix = value,
            p::OUTPUT => self.output_db = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        if let Some((band, which)) = p::split(param) {
            return self.bands.get(band).and_then(|b| b.get(which));
        }
        Some(match param {
            p::LOW_X => self.low_hz,
            p::HIGH_X => self.high_hz,
            p::GRIP => self.grip,
            p::MIX => self.mix,
            p::OUTPUT => self.output_db,
            _ => return None,
        })
    }

    /// Every row back inside its own range, non-numbers replaced.
    ///
    /// RON round-trips NaN, and a NaN threshold makes the gain computer
    /// emit NaN, which the ballistics then hold forever.
    pub fn sanitize(&mut self) {
        for def in p::TABLE {
            let value = self.get(def.id).unwrap_or(def.default);
            self.set(
                def.id,
                if value.is_finite() {
                    value.clamp(def.min, def.max)
                } else {
                    def.default
                },
            );
        }
    }
}

/// What costs something to rebuild, resolved once a segment.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resolved {
    low_hz: f32,
    high_hz: f32,
    grip: f32,
    bands: [BandParams; p::BANDS],
}

impl Resolved {
    fn of(params: &PrismParams) -> Self {
        Self {
            low_hz: params.low_hz,
            high_hz: params.high_hz,
            grip: params.grip,
            bands: params.bands,
        }
    }
}

fn db_to_gain(db: f32) -> f32 {
    crate::dsp::arith::db_to_gain(db)
}

/// Geometric interpolation, for times where the ratio is what matters.
fn glide(t: f32, from: f32, to: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    from * (to / from).powf(t)
}

/// The times this band can actually honour, given GRIP and the corners.
///
/// See the module header: the floor is a fraction of a period of the
/// band's LOWER edge, and it is physics rather than taste.
pub fn band_times(grip: f32, lower_hz: f32) -> (f32, f32) {
    let attack = glide(grip, p::ATTACK_SLOW_MS, p::ATTACK_FAST_MS);
    let release = glide(grip, p::RELEASE_SLOW_MS, p::RELEASE_FAST_MS);
    let period_ms = 1_000.0 / lower_hz.max(1.0);
    (
        attack.max(period_ms * p::PERIODS_ATTACK),
        release.max(period_ms * p::PERIODS_RELEASE),
    )
}

/// One band's running state.
struct Band {
    detector: RmsDetector,
    computer: GainComputer,
    ballistics: Ballistics,
    heat: Waveshaper,
    /// `1/drive`, so the heat curve has unity slope at the origin and
    /// the colour does not arrive as a level change.
    heat_scale: f32,
    /// The most gain movement this band made in the last block, in dB,
    /// negative for reduction.
    reduction_db: f32,
    /// Ramped across a segment, so a trim move glides.
    trim: f32,
}

impl Band {
    fn new(sample_rate: f32, params: &BandParams, grip: f32, lower_hz: f32) -> Self {
        let mut detector = RmsDetector::new();
        detector.prepare(sample_rate, p::DETECT_MS);
        let computer = GainComputer::new();
        let mut ballistics = Ballistics::new();
        // NO AUTO RELEASE. Program dependence on one third of the
        // spectrum is a band that breathes against its neighbours.
        ballistics.set_auto(false);
        let mut heat = Waveshaper::new();
        heat.configure(shaper::Mode::SoftClip, p::HEAT_DRIVE, 0.0, 1.0);

        let mut band = Self {
            detector,
            computer,
            ballistics,
            heat,
            heat_scale: 1.0 / p::HEAT_DRIVE.max(1e-3),
            reduction_db: 0.0,
            trim: db_to_gain(params.trim_db),
        };
        band.configure(sample_rate, params, grip, lower_hz);
        band
    }

    /// Green zone: the direction, the ratio and the times.
    fn configure(&mut self, sample_rate: f32, params: &BandParams, grip: f32, lower_hz: f32) {
        let amount = if params.amount.is_finite() {
            params.amount.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        let (mode, ratio) = if amount >= 0.0 {
            (Mode::Compress, 1.0 + amount * (p::RATIO_MAX - 1.0))
        } else {
            (Mode::Expand, 1.0 + (-amount) * (p::UPWARD_RATIO_MAX - 1.0))
        };
        self.computer
            .configure(mode, params.threshold_db, ratio, p::KNEE_DB);

        let (attack, release) = band_times(grip, lower_hz);
        // UPWARD SWAPS THEM. `Ballistics` calls the direction that adds
        // reduction "attack", and upward compression adds its gain in
        // the other direction — so an unswapped knob would make the
        // fast setting the slow one. The gate crosses its times for the
        // same reason.
        if amount < 0.0 {
            self.ballistics.prepare(sample_rate, release, attack);
        } else {
            self.ballistics.prepare(sample_rate, attack, release);
        }
    }

    fn reset(&mut self) {
        self.detector.reset();
        self.ballistics.reset();
        self.reduction_db = 0.0;
    }

    /// Red zone: this band's own dynamics, in place on both channels.
    ///
    /// Returns the most gain movement made, in dB — negative for
    /// reduction, positive for lift.
    fn process(
        &mut self,
        l: &mut [f32],
        r: &mut [f32],
        params: &BandParams,
        upward: bool,
        trim_step: f32,
    ) {
        let heat = params.heat.clamp(0.0, 1.0);
        let mut worst = 0.0f32;
        for (left, right) in l.iter_mut().zip(r.iter_mut()) {
            // ONE detector over both channels: two would pull a centred
            // source toward whichever side happened to be louder, and
            // do it independently in three bands.
            let env = self.detector.tick(0.5 * (*left + *right));
            let level_db = crate::dsp::arith::gain_to_db(env.max(1e-7));
            let raw = self.computer.gain_db(level_db);
            // The computer only ever gives back attenuation. Upward
            // compression is that same curve read the other way up,
            // with a ceiling on it so a quiet band's noise floor cannot
            // become the loudest thing in the mix.
            let target = if upward {
                (-raw).min(p::LIFT_CEILING_DB)
            } else {
                raw
            };
            let moved = self.ballistics.tick(target);
            if moved.abs() > worst.abs() {
                worst = moved;
            }

            self.trim += trim_step;
            let gain = db_to_gain(moved) * self.trim;
            let (mut a, mut b) = (*left * gain, *right * gain);

            // THE COLOUR IS EARNED. The crossfade weight is this
            // sample's own gain movement, so a band doing nothing is
            // untouched to the bit and a band working hard is warm.
            if heat > 0.0 {
                let worked = (moved.abs() / p::HEAT_FULL_DB).clamp(0.0, 1.0);
                let amount = heat * worked;
                if amount > 0.0 {
                    a += amount * (self.heat.shape(a) * self.heat_scale - a);
                    b += amount * (self.heat.shape(b) * self.heat_scale - b);
                }
            }
            *left = a;
            *right = b;
        }
        self.reduction_db = worst;
    }
}

pub struct Prism {
    split: [Crossover3; 2],
    bands: [Band; p::BANDS],
    /// Band buffers: `[band][channel]`, `CHUNK` frames each.
    buf: [[[f32; CHUNK]; 2]; p::BANDS],
    /// The dry signal for the mix, one chunk at a time.
    dry_l: [f32; CHUNK],
    dry_r: [f32; CHUNK],
    prepared: Resolved,
    params: PrismParams,
    output: f32,
    mix: f32,
    said: crate::audio::graph::Readout,
    sample_rate: f32,
}

impl Prism {
    /// Green zone.
    pub fn new(sample_rate: f32, params: &PrismParams) -> Self {
        let mut params = *params;
        params.sanitize();
        let sample_rate = sample_rate.max(1.0);
        let (low, high) = Crossover3::corners(sample_rate, params.low_hz, params.high_hz);

        let mut split = [Crossover3::new(), Crossover3::new()];
        for one in split.iter_mut() {
            one.prepare(sample_rate, low, high);
        }

        let edges = Self::lower_edges(low, high);
        let band = |i: usize| {
            Band::new(
                sample_rate,
                params.bands.get(i).unwrap_or(&BandParams::default()),
                params.grip,
                edges.get(i).copied().unwrap_or(low),
            )
        };

        Self {
            split,
            bands: [band(0), band(1), band(2)],
            buf: [[[0.0; CHUNK]; 2]; p::BANDS],
            dry_l: [0.0; CHUNK],
            dry_r: [0.0; CHUNK],
            prepared: Resolved::of(&params),
            params,
            output: db_to_gain(params.output_db),
            mix: params.mix,
            said: crate::audio::graph::Readout::default(),
            sample_rate,
        }
    }

    /// The frequency each band's envelope times are floored against.
    ///
    /// The LOWER edge of each band, because that is the longest cycle
    /// the detector has to sit through. The low band has no lower
    /// corner of its own, so it uses a fraction of the first crossover
    /// — a low band split at 180 Hz still carries 40 Hz.
    fn lower_edges(low_hz: f32, high_hz: f32) -> [f32; p::BANDS] {
        [(low_hz * 0.25).max(20.0), low_hz, high_hz]
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        *self = Self::new(sample_rate, &self.params);
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
    }

    pub fn params(&self) -> &PrismParams {
        &self.params
    }

    /// What the device is doing, for the card to draw. The worst band
    /// wins — it is the one the eye goes to anyway.
    pub fn readout(&self) -> crate::audio::graph::Readout {
        self.said
    }

    /// Per-band gain movement in dB, newest block. Negative is
    /// reduction, positive is lift.
    pub fn band_reduction_db(&self) -> [f32; p::BANDS] {
        let at = |i: usize| self.bands.get(i).map_or(0.0, |b| b.reduction_db);
        [at(0), at(1), at(2)]
    }

    /// The corners actually in use, which is not always the pair asked
    /// for — see [`Crossover3::corners`].
    pub fn corners(&self) -> (f32, f32) {
        Crossover3::corners(self.sample_rate, self.params.low_hz, self.params.high_hz)
    }

    /// Red zone: everything back to rest, for a discontinuity.
    pub fn reset(&mut self) {
        for one in self.split.iter_mut() {
            one.reset();
        }
        for band in self.bands.iter_mut() {
            band.reset();
        }
    }

    pub fn latency(&self) -> usize {
        0
    }

    /// Red zone: process `l` and `r` in place.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        if n == 0 {
            return;
        }

        // --- the segment prologue: rebuild only what moved -------------
        let want = Resolved::of(&self.params);
        if want != self.prepared {
            let (low, high) = Crossover3::corners(self.sample_rate, want.low_hz, want.high_hz);
            if want.low_hz != self.prepared.low_hz || want.high_hz != self.prepared.high_hz {
                for one in self.split.iter_mut() {
                    one.prepare(self.sample_rate, low, high);
                }
            }
            let edges = Self::lower_edges(low, high);
            for (i, band) in self.bands.iter_mut().enumerate() {
                let Some(params) = want.bands.get(i) else {
                    continue;
                };
                band.configure(
                    self.sample_rate,
                    params,
                    want.grip,
                    edges.get(i).copied().unwrap_or(low),
                );
            }
            self.prepared = want;
        }

        // --- the two levels, ramped across the whole segment -----------
        let output_to = db_to_gain(self.params.output_db);
        let mix_to = self.params.mix.clamp(0.0, 1.0);
        let output_step = (output_to - self.output) / n as f32;
        let mix_step = (mix_to - self.mix) / n as f32;
        let trim_steps = {
            let mut steps = [0.0f32; p::BANDS];
            for (i, step) in steps.iter_mut().enumerate() {
                let want = self
                    .params
                    .bands
                    .get(i)
                    .map_or(1.0, |b| db_to_gain(b.trim_db));
                let now = self.bands.get(i).map_or(1.0, |b| b.trim);
                *step = (want - now) / n as f32;
            }
            steps
        };

        let mut peak = 0.0f32;
        let mut worst = 0.0f32;
        let mut done = 0usize;
        while done < n {
            let take = CHUNK.min(n - done);
            // The dry copy, for the mix — taken before anything
            // touches the signal, one chunk at a time.
            let (Some(src_l), Some(src_r)) = (l.get(done..done + take), r.get(done..done + take))
            else {
                return;
            };
            let (Some(keep_l), Some(keep_r)) =
                (self.dry_l.get_mut(..take), self.dry_r.get_mut(..take))
            else {
                return;
            };
            keep_l.copy_from_slice(src_l);
            keep_r.copy_from_slice(src_r);

            // Split both channels into the band buffers.
            for (channel, src) in [src_l, src_r].into_iter().enumerate() {
                let mut low = [0.0f32; CHUNK];
                let mut mid = [0.0f32; CHUNK];
                let mut high = [0.0f32; CHUNK];
                let Some(split) = self.split.get_mut(channel) else {
                    continue;
                };
                let (Some(lo), Some(md), Some(hi)) = (
                    low.get_mut(..take),
                    mid.get_mut(..take),
                    high.get_mut(..take),
                ) else {
                    continue;
                };
                split.process(src, lo, md, hi);
                for (band, from) in [low, mid, high].into_iter().enumerate() {
                    let Some(slot) = self
                        .buf
                        .get_mut(band)
                        .and_then(|b| b.get_mut(channel))
                        .and_then(|b| b.get_mut(..take))
                    else {
                        continue;
                    };
                    let Some(from) = from.get(..take) else {
                        continue;
                    };
                    slot.copy_from_slice(from);
                }
            }

            // Each band's own dynamics.
            for (index, band) in self.bands.iter_mut().enumerate() {
                let Some(params) = self.params.bands.get(index) else {
                    continue;
                };
                let Some(pair) = self.buf.get_mut(index) else {
                    continue;
                };
                let (left, right) = pair.split_at_mut(1);
                let (Some(left), Some(right)) = (
                    left.first_mut().and_then(|b| b.get_mut(..take)),
                    right.first_mut().and_then(|b| b.get_mut(..take)),
                ) else {
                    continue;
                };
                band.process(
                    left,
                    right,
                    params,
                    params.amount < 0.0,
                    trim_steps.get(index).copied().unwrap_or(0.0),
                );
                if band.reduction_db.abs() > worst.abs() {
                    worst = band.reduction_db;
                }
            }

            // Sum the bands back up, and cross to the dry.
            for i in 0..take {
                let mut wet_l = 0.0f32;
                let mut wet_r = 0.0f32;
                for pair in self.buf.iter() {
                    wet_l += pair.first().and_then(|b| b.get(i)).copied().unwrap_or(0.0);
                    wet_r += pair.get(1).and_then(|b| b.get(i)).copied().unwrap_or(0.0);
                }
                self.output += output_step;
                self.mix += mix_step;
                let mix = self.mix.clamp(0.0, 1.0);
                let dry_left = self.dry_l.get(i).copied().unwrap_or(0.0);
                let dry_right = self.dry_r.get(i).copied().unwrap_or(0.0);
                let out_l = dry_left + mix * (wet_l * self.output - dry_left);
                let out_r = dry_right + mix * (wet_r * self.output - dry_right);
                if let Some(slot) = l.get_mut(done + i) {
                    *slot = out_l;
                }
                if let Some(slot) = r.get_mut(done + i) {
                    *slot = out_r;
                }
                peak = peak.max(out_l.abs()).max(out_r.abs());
            }

            done += take;
        }

        self.output = output_to;
        self.mix = mix_to;
        for (i, band) in self.bands.iter_mut().enumerate() {
            band.trim = self
                .params
                .bands
                .get(i)
                .map_or(1.0, |b| db_to_gain(b.trim_db));
        }
        self.said = crate::audio::graph::Readout {
            level_db: 20.0 * peak.max(1e-7).log10(),
            reduction_db: worst.min(0.0),
            bands: self.band_reduction_db(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn prism(params: PrismParams) -> Prism {
        Prism::new(FS, &params)
    }

    fn tone(len: usize, hz: f32, amp: f32) -> Vec<f32> {
        (0..len)
            .map(|i| amp * (core::f32::consts::TAU * hz * i as f32 / FS).sin())
            .collect()
    }

    /// RMS, not peak: the device is allpass when idle, so phase has
    /// moved and a sampled peak would read that as loss.
    fn rms(block: &[f32]) -> f32 {
        if block.is_empty() {
            return 0.0;
        }
        (block.iter().map(|s| s * s).sum::<f32>() / block.len() as f32).sqrt()
    }

    fn settled(block: &[f32]) -> &[f32] {
        block.get(8_192..).unwrap_or(&[])
    }

    fn run(p: &mut Prism, input: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let mut l = input.to_vec();
        let mut r = input.to_vec();
        p.process(&mut l, &mut r);
        (l, r)
    }

    fn band(amount: f32, threshold_db: f32) -> BandParams {
        BandParams {
            threshold_db,
            amount,
            heat: 0.0,
            trim_db: 0.0,
        }
    }

    /// IDLE IS INAUDIBLE. Every band at zero, and the three come back
    /// out as loud as they went in — at the corners too, which is where
    /// a missing alignment allpass shows up.
    #[test]
    fn a_device_doing_nothing_gives_back_what_it_got() {
        for hz in [40.0, 180.0, 500.0, 2_800.0, 8_000.0] {
            let mut device = prism(PrismParams::default());
            let input = tone(16_384, hz, 0.25);
            let (out, _) = run(&mut device, &input);
            let db = 20.0 * (rms(settled(&out)) / rms(settled(&input))).log10();
            assert!(
                db.abs() < 0.4,
                "at {hz} Hz an idle prism is {db:+.2} dB off"
            );
        }
    }

    /// A POSITIVE AMOUNT HOLDS THE BAND DOWN, and only that band.
    #[test]
    fn a_positive_amount_compresses_its_own_band() {
        let loud = tone(16_384, 80.0, 0.7);
        let mut idle = prism(PrismParams::default());
        let (before, _) = run(&mut idle, &loud);

        let mut squashed = prism(PrismParams {
            bands: [
                band(1.0, -30.0),
                BandParams::default(),
                BandParams::default(),
            ],
            ..PrismParams::default()
        });
        let (after, _) = run(&mut squashed, &loud);

        let db = 20.0 * (rms(settled(&after)) / rms(settled(&before))).log10();
        assert!(db < -8.0, "the low band only came down {db:+.2} dB");

        // And a tone in the HIGH band is untouched by the low band's
        // setting — three bands that all moved together would be one
        // band with three cards.
        let high = tone(16_384, 8_000.0, 0.7);
        let mut idle = prism(PrismParams::default());
        let (before, _) = run(&mut idle, &high);
        let mut squashed = prism(PrismParams {
            bands: [
                band(1.0, -30.0),
                BandParams::default(),
                BandParams::default(),
            ],
            ..PrismParams::default()
        });
        let (after, _) = run(&mut squashed, &high);
        let db = 20.0 * (rms(settled(&after)) / rms(settled(&before))).log10();
        assert!(
            db.abs() < 0.5,
            "setting the low band moved the high band by {db:+.2} dB"
        );
    }

    /// A NEGATIVE AMOUNT LIFTS THE QUIET — the direction this device
    /// spends its one signed knob on.
    #[test]
    fn a_negative_amount_lifts_what_is_under_the_threshold() {
        let quiet = tone(16_384, 800.0, 0.01);
        let mut idle = prism(PrismParams::default());
        let (before, _) = run(&mut idle, &quiet);

        let mut lifted = prism(PrismParams {
            bands: [
                BandParams::default(),
                band(-1.0, -12.0),
                BandParams::default(),
            ],
            ..PrismParams::default()
        });
        let (after, _) = run(&mut lifted, &quiet);
        let db = 20.0 * (rms(settled(&after)) / rms(settled(&before))).log10();
        assert!(db > 6.0, "the lift only reached {db:+.2} dB");
        assert!(
            db <= p::LIFT_CEILING_DB + 1.0,
            "the lift went past its ceiling at {db:+.2} dB"
        );
    }

    /// THE COLOUR IS EARNED. A band doing no work is bit-for-bit clean
    /// whatever the heat knob says — which is the device's whole claim,
    /// and the one an ordinary saturation stage cannot make.
    #[test]
    fn heat_on_a_band_doing_nothing_changes_not_one_bit() {
        let input = tone(4_096, 800.0, 0.4);
        let clean = {
            let mut device = prism(PrismParams::default());
            run(&mut device, &input).0
        };
        let hot = {
            let mut device = prism(PrismParams {
                bands: [
                    BandParams {
                        heat: 1.0,
                        ..BandParams::default()
                    },
                    BandParams {
                        heat: 1.0,
                        ..BandParams::default()
                    },
                    BandParams {
                        heat: 1.0,
                        ..BandParams::default()
                    },
                ],
                ..PrismParams::default()
            });
            run(&mut device, &input).0
        };
        assert_eq!(clean, hot, "heat coloured a band that was doing no work");
    }

    /// ...and a band that IS working takes the colour. Measured as
    /// harmonic content, because that is what the claim is — not as a
    /// level, which the compression is already changing.
    #[test]
    fn heat_on_a_band_that_is_working_adds_harmonics() {
        let input = tone(16_384, 800.0, 0.8);
        let distortion = |heat: f32| {
            let mut device = prism(PrismParams {
                bands: [
                    BandParams::default(),
                    BandParams {
                        threshold_db: -40.0,
                        amount: 1.0,
                        heat,
                        trim_db: 0.0,
                    },
                    BandParams::default(),
                ],
                ..PrismParams::default()
            });
            let (out, _) = run(&mut device, &input);
            // Third harmonic, by correlation against a 2400 Hz tone.
            let probe = tone(input.len(), 2_400.0, 1.0);
            let tail = settled(&out);
            let probe = settled(&probe);
            let dot: f32 = tail.iter().zip(probe.iter()).map(|(a, b)| a * b).sum();
            (dot / tail.len().max(1) as f32).abs() / rms(tail).max(1e-9)
        };
        let cold = distortion(0.0);
        let warm = distortion(1.0);
        // The COLD figure is the numerical floor, not a measurement —
        // a compressor moving a gain is not a nonlinearity and puts no
        // third harmonic anywhere. The claim is the ratio between them.
        assert!(
            warm > cold * 4.0 && warm > 1e-5,
            "heat added nothing: {cold:.6} cold, {warm:.6} warm"
        );
    }

    /// THE LOW BAND CANNOT BE AS FAST AS THE HIGH ONE. Physics, and the
    /// module header's third claim.
    #[test]
    fn a_lower_band_is_slower_at_the_same_grip() {
        for grip in [0.0f32, 0.5, 1.0] {
            let (low_a, low_r) = band_times(grip, 45.0);
            let (high_a, high_r) = band_times(grip, 2_800.0);
            assert!(
                low_a >= high_a,
                "at grip {grip} the low band attacks in {low_a} ms and the high in {high_a}"
            );
            assert!(low_r >= high_r, "at grip {grip} the releases are crossed");
        }
        // And the floor really binds: at full grip the low band is held
        // well off the fast end it was asked for.
        let (attack, _) = band_times(1.0, 45.0);
        assert!(
            attack > p::ATTACK_FAST_MS * 4.0,
            "the low band honoured an attack of {attack} ms it cannot measure"
        );
    }

    /// SPLIT-BLOCK EQUIVALENCE, bit-exact — the property the segmented
    /// transport depends on.
    #[test]
    fn processing_in_pieces_is_processing_whole() {
        let params = PrismParams {
            bands: [band(0.8, -24.0), band(-0.5, -18.0), band(0.4, -30.0)],
            ..PrismParams::default()
        };
        let input = tone(700, 500.0, 0.5);

        let mut whole = prism(params);
        let (l1, r1) = run(&mut whole, &input);

        let mut pieces = prism(params);
        let mut l2 = input.clone();
        let mut r2 = input.clone();
        let (la, lb) = l2.split_at_mut(301);
        let (ra, rb) = r2.split_at_mut(301);
        pieces.process(la, ra);
        pieces.process(lb, rb);

        assert_eq!(l1, l2, "the left channel differs across a block boundary");
        assert_eq!(r1, r2, "the right channel differs across a block boundary");
    }

    #[test]
    fn processing_does_not_allocate() {
        let mut device = prism(PrismParams {
            bands: [band(1.0, -24.0), band(-1.0, -18.0), band(0.5, -30.0)],
            ..PrismParams::default()
        });
        let mut l = vec![0.2f32; 512];
        let mut r = vec![0.2f32; 512];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..50 {
                device.process(&mut l, &mut r);
            }
        });
    }

    /// EDGE LENGTHS, including one longer than the internal chunk and
    /// one that is not a multiple of it.
    #[test]
    fn odd_lengths_and_nonsense_stay_finite() {
        let mut device = prism(PrismParams {
            bands: [band(1.0, -60.0), band(-1.0, -60.0), band(1.0, -60.0)],
            ..PrismParams::default()
        });
        for len in [0usize, 1, 3, 127, 128, 129, 333] {
            let input = tone(len, 440.0, 0.9);
            let (l, r) = run(&mut device, &input);
            assert!(
                l.iter().chain(r.iter()).all(|s| s.is_finite()),
                "length {len} went non-finite"
            );
        }
        // Mismatched channels: the shorter one bounds the run.
        let mut l = vec![0.5f32; 64];
        let mut r = vec![0.5f32; 8];
        device.process(&mut l, &mut r);
        assert!(l.iter().chain(r.iter()).all(|s| s.is_finite()));
    }

    #[test]
    fn silence_stays_silent() {
        let mut device = prism(PrismParams {
            bands: [band(1.0, -60.0), band(-1.0, -60.0), band(1.0, -60.0)],
            ..PrismParams::default()
        });
        let (l, r) = run(&mut device, &vec![0.0f32; 1_024]);
        assert!(
            l.iter().chain(r.iter()).all(|s| *s == 0.0),
            "silence came out loud"
        );
    }

    /// A NaN in the patch is a defaulted knob, never a poisoned engine.
    #[test]
    fn nonsense_settings_sanitize() {
        let mut params = PrismParams {
            low_hz: f32::NAN,
            high_hz: -1.0,
            mix: 40.0,
            ..PrismParams::default()
        };
        if let Some(band) = params.bands.get_mut(0) {
            band.amount = f32::INFINITY;
            band.threshold_db = f32::NAN;
        }
        params.sanitize();
        assert!(params.low_hz.is_finite() && params.mix <= 1.0);
        for def in p::TABLE {
            let value = params.get(def.id).unwrap_or(f32::NAN);
            assert!(
                value.is_finite() && (def.min..=def.max).contains(&value),
                "{} sanitized to {value}",
                def.name
            );
        }

        let mut device = prism(params);
        let input = tone(512, 440.0, 0.5);
        let (l, _) = run(&mut device, &input);
        assert!(l.iter().all(|s| s.is_finite()));
    }

    /// Every table row reaches the field it names, and comes back.
    #[test]
    fn every_wire_id_round_trips() {
        let mut params = PrismParams::default();
        for def in p::TABLE {
            let value = def.min + (def.max - def.min) * 0.375;
            params.set(def.id, value);
            let back = params.get(def.id);
            assert!(
                back.is_some_and(|b| (b - value).abs() < (def.max - def.min) * 1e-5),
                "{} set to {value} came back as {back:?}",
                def.name
            );
        }
        // And the band arithmetic agrees with itself.
        for b in 0..p::BANDS {
            for which in 0..p::band::COUNT {
                assert_eq!(p::split(p::param(b, which)), Some((b, which)));
            }
        }
        assert_eq!(p::split(p::LOW_X), None);
        assert_eq!(p::split(p::OUTPUT), None);
    }
}
