//! Flint — the transient shaper, with an opinion.
//!
//! A compressor asks how loud a hit was. A transient shaper asks how
//! SUDDEN it was, which is a different question and the reason this
//! device exists next to `clamp` and `glue` rather than instead of them.
//! [`TransientSplit`](crate::dsp::dynamics::TransientSplit) answers it:
//! per sample, how much of what is arriving is strike rather than body,
//! normalised so a ghost note and an accent open it the same amount.
//!
//! # The opinion
//!
//! The gain is not neutral, and that is deliberate.
//!
//! Every transient shaper can make an attack louder. What makes a hit
//! read as sharper is not only level — it is the top end arriving first,
//! which is what an actual stick on an actual head does and what a
//! multiplier cannot fake. So when STRIKE is pushed UP, Flint lifts the
//! high end **of the strike only**, and when BODY is pushed UP it thickens
//! the low-mid **of the body only**. Neither colour touches the other half
//! of the hit, because a device that brightens the tail while sharpening
//! the attack has just turned the treble up.
//!
//! Pulling either knob DOWN is plain gain. Softening is a subtraction and
//! there is nothing to add character to; a colour that appeared on the way
//! down would be an effect nobody asked for.
//!
//! COLOUR is the amount of all that. At zero this is a competent neutral
//! transient shaper. At its default it is this one.
//!
//! # The promise
//!
//! With STRIKE and BODY at zero the device is the EXACT identity — the
//! same signal, bit for bit, whatever COLOUR and SPLIT are doing. The
//! colour is scaled by how far each knob is pushed past zero, so at zero
//! there is nothing to add, and the gain is `db_to_gain(0)`, which is
//! exactly 1.0 rather than nearly. `the_device_is_the_exact_identity_at_
//! rest` holds it to that on the bit, for the reason the sampler brief
//! gives: a colour stage that cannot be switched off cannot be measured.
//!
//! # Detection
//!
//! One detector across both channels, fed `max(|l|, |r|)` — two would pull
//! a centred hit toward whichever side happened to be louder, and would do
//! it on every snare. The detector is high-passed at
//! [`KEY_HP_HZ`]; bass smears an envelope, and a kick drum's own body
//! arriving under a hi-hat should not tell the hi-hat it has no attack.
//! The filter deafens the DETECTOR only — the audio path never sees it.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::arith::db_to_gain;
use crate::dsp::dynamics::TransientSplit;
use crate::dsp::filters::OnePole;
use crate::params::flint as p;

/// The detector's high-pass corner. Above a kick's fundamental, below a
/// snare's crack.
pub const KEY_HP_HZ: f32 = 120.0;
/// The strike colour's corner: the band that reads as "snap".
const SNAP_HZ: f32 = 2_400.0;
/// The body colour's corner: the band that reads as "weight".
const BLOOM_HZ: f32 = 320.0;
/// How much colour a fully-pushed knob buys, as a fraction of the dry
/// signal added back. Tuned so the colour is heard as the SAME hit
/// sharpened rather than as a second one layered on.
const SNAP_DEPTH: f32 = 0.55;
const BLOOM_DEPTH: f32 = 0.40;

/// The knobs, in engine units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FlintParams {
    pub strike_db: f32,
    pub body_db: f32,
    pub split_ms: f32,
    pub colour: f32,
    pub mix: f32,
    pub out: f32,
}

impl Default for FlintParams {
    fn default() -> Self {
        let d = |id: u32| {
            p::TABLE
                .iter()
                .find(|def| def.id == id)
                .map(|def| def.default)
                .unwrap_or(0.0)
        };
        Self {
            strike_db: d(p::STRIKE),
            body_db: d(p::BODY),
            split_ms: d(p::SPLIT),
            colour: d(p::COLOUR),
            mix: d(p::MIX),
            out: d(p::OUT),
        }
    }
}

impl FlintParams {
    pub fn set(&mut self, param: u32, value: f32) {
        let clamped = p::TABLE
            .iter()
            .find(|def| def.id == param)
            .map(|def| def.clamp(value));
        let Some(value) = clamped else { return };
        match param {
            p::STRIKE => self.strike_db = value,
            p::BODY => self.body_db = value,
            p::SPLIT => self.split_ms = value,
            p::COLOUR => self.colour = value,
            p::MIX => self.mix = value,
            p::OUT => self.out = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> Option<f32> {
        match param {
            p::STRIKE => Some(self.strike_db),
            p::BODY => Some(self.body_db),
            p::SPLIT => Some(self.split_ms),
            p::COLOUR => Some(self.colour),
            p::MIX => Some(self.mix),
            p::OUT => Some(self.out),
            _ => None,
        }
    }

    /// Drag every field back inside its row of the table. A patch off
    /// disk is untrusted input.
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
}

/// The settings that cost something to rebuild, resolved once a segment.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resolved {
    split_ms: f32,
}

impl Resolved {
    fn of(params: &FlintParams) -> Self {
        Self {
            split_ms: params.split_ms,
        }
    }
}

/// The device.
#[derive(Debug, Clone)]
pub struct Flint {
    params: FlintParams,
    prepared: Resolved,
    sample_rate: f32,
    split: TransientSplit,
    key_hp: OnePole,
    snap: [OnePole; 2],
    bloom: [OnePole; 2],
    /// The strike weight last seen — telemetry for the card's meter.
    weight: f32,
    /// What the last block did, for the card. Peak level, the most
    /// shaping applied (signed, so a body cut reads negative and a
    /// strike lift positive), and the peak strike weight.
    said: crate::audio::graph::Readout,
}

impl Flint {
    pub fn new(sample_rate: f32, params: &FlintParams) -> Self {
        let mut flint = Self {
            params: *params,
            prepared: Resolved::of(params),
            sample_rate: 48_000.0,
            split: TransientSplit::new(),
            key_hp: OnePole::new(),
            snap: [OnePole::new(); 2],
            bloom: [OnePole::new(); 2],
            weight: 0.0,
            said: crate::audio::graph::Readout::default(),
        };
        flint.params.sanitize();
        flint.prepare(sample_rate);
        flint
    }

    /// Green zone: the rate changed, so every coefficient is stale.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let fs = self.sample_rate;
        self.split.prepare(fs, self.params.split_ms);
        self.prepared = Resolved::of(&self.params);
        self.key_hp.prepare(fs, KEY_HP_HZ);
        for f in self.snap.iter_mut() {
            f.prepare(fs, SNAP_HZ);
        }
        for f in self.bloom.iter_mut() {
            f.prepare(fs, BLOOM_HZ);
        }
        self.reset();
    }

    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
    }

    pub fn params(&self) -> &FlintParams {
        &self.params
    }

    /// How much of the last sample read as strike, `0..=1`.
    pub fn weight(&self) -> f32 {
        self.weight
    }

    /// What the last block did — level, shaping, and how much of it read
    /// as strike. `bands[0]` carries the weight; the other two are zero
    /// because this device has one band and has already said everything.
    pub fn readout(&self) -> crate::audio::graph::Readout {
        self.said
    }

    pub fn reset(&mut self) {
        self.split.reset();
        self.key_hp.reset();
        for f in self.snap.iter_mut().chain(self.bloom.iter_mut()) {
            f.reset();
        }
        self.weight = 0.0;
        self.said = crate::audio::graph::Readout::default();
    }

    /// This device anticipates nothing, so it delays nothing.
    pub fn latency(&self) -> usize {
        0
    }

    /// Red zone: shape in place, any length.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        // Rebuild only what moved — the detector's ballistics are the
        // only thing here that costs a transcendental.
        let want = Resolved::of(&self.params);
        if want != self.prepared {
            self.split.prepare(self.sample_rate, want.split_ms);
            self.prepared = want;
        }

        let strike_db = self.params.strike_db;
        let body_db = self.params.body_db;
        let colour = self.params.colour.clamp(0.0, 1.0);
        let mix = self.params.mix.clamp(0.0, 1.0);
        let out = self.params.out;
        // Colour only ever rides a PUSH. Pulling a half down is plain
        // gain; there is nothing there to add character to.
        let snap_amount = colour * (strike_db.max(0.0) / p::SHAPE_MAX_DB) * SNAP_DEPTH;
        let bloom_amount = colour * (body_db.max(0.0) / p::SHAPE_MAX_DB) * BLOOM_DEPTH;

        let n = l.len().min(r.len());
        let (Some(l), Some(r)) = (l.get_mut(..n), r.get_mut(..n)) else {
            return;
        };

        let mut peak = 0.0f32;
        let mut most_shaping = 0.0f32;
        let mut most_weight = 0.0f32;

        for (left, right) in l.iter_mut().zip(r.iter_mut()) {
            // A non-finite sample is treated as silence rather than
            // multiplied. NaN times anything is NaN, and one of them
            // reaching the filters below would stay in their state for
            // the rest of the session — the detector already guards
            // itself this way and the audio path must match it.
            let dry_l = if left.is_finite() { *left } else { 0.0 };
            let dry_r = if right.is_finite() { *right } else { 0.0 };

            // ONE detector, on the louder side, deaf to bass.
            let key = dry_l.abs().max(dry_r.abs());
            let key = self.key_hp.tick_highpass(key).abs();
            let mut weight = [0.0f32; 1];
            self.split.process(&[key], &mut weight);
            let t = weight.first().copied().unwrap_or(0.0);
            self.weight = t;

            // The shaping gain, interpolated in DECIBELS between the two
            // halves — so both at zero is `db_to_gain(0)`, exactly 1.0.
            let shaping_db = strike_db * t + body_db * (1.0 - t);
            let gain = db_to_gain(shaping_db);
            peak = peak.max(dry_l.abs()).max(dry_r.abs());
            if shaping_db.abs() > most_shaping.abs() {
                most_shaping = shaping_db;
            }
            most_weight = most_weight.max(t);

            let mut wet_l = dry_l * gain;
            let mut wet_r = dry_r * gain;

            // The colour. Both filters run every sample whatever the
            // amounts are, so their state stays coherent and turning
            // COLOUR up mid-note does not click.
            let snap_l = self.snap[0].tick_highpass(dry_l);
            let snap_r = self.snap[1].tick_highpass(dry_r);
            let bloom_l = self.bloom[0].tick_lowpass(dry_l);
            let bloom_r = self.bloom[1].tick_lowpass(dry_r);
            if snap_amount > 0.0 {
                let a = snap_amount * t;
                wet_l += snap_l * a;
                wet_r += snap_r * a;
            }
            if bloom_amount > 0.0 {
                let a = bloom_amount * (1.0 - t);
                wet_l += bloom_l * a;
                wet_r += bloom_r * a;
            }

            *left = (dry_l + (wet_l - dry_l) * mix) * out;
            *right = (dry_r + (wet_r - dry_r) * mix) * out;
        }

        self.said = crate::audio::graph::Readout {
            level_db: crate::dsp::arith::gain_to_db(peak.max(1e-6))
                .max(crate::dsp::dynamics::FLOOR_DB),
            reduction_db: most_shaping,
            bands: [most_weight, 0.0, 0.0],
        };
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn armed(edit: impl Fn(&mut FlintParams)) -> Flint {
        let mut params = FlintParams::default();
        edit(&mut params);
        Flint::new(FS, &params)
    }

    /// A percussive hit: instant onset, exponential decay, in stereo.
    fn hit(amp: f32, decay_ms: f32, n: usize) -> (Vec<f32>, Vec<f32>) {
        let tau = decay_ms * 1e-3 * FS;
        let mono: Vec<f32> = (0..n)
            .map(|i| amp * (-(i as f32) / tau).exp() * ((i as f32) * 0.7).sin())
            .collect();
        (mono.clone(), mono)
    }

    fn peak(block: &[f32]) -> f32 {
        block.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// THE PROMISE: with both halves at zero the device is the identity,
    /// bit for bit, whatever COLOUR and SPLIT are doing.
    ///
    /// The module header claims it and the sampler brief demands the
    /// shape of it — a colour stage that cannot reach an exact bypass is
    /// a colour nobody can measure, only assert. Note the loop drives
    /// COLOUR to full: the colour must be gated by the PUSH, not merely
    /// small when the knobs are centred.
    #[test]
    fn the_device_is_the_exact_identity_at_rest() {
        for colour in [0.0f32, 0.5, 1.0] {
            for split in [1.0f32, 12.0, 60.0] {
                let mut flint = armed(|p| {
                    p.strike_db = 0.0;
                    p.body_db = 0.0;
                    p.colour = colour;
                    p.split_ms = split;
                });
                let (mut l, mut r) = hit(0.8, 30.0, 2_048);
                let (dry_l, dry_r) = (l.clone(), r.clone());
                flint.process(&mut l, &mut r);
                for (i, ((a, b), (c, d))) in l
                    .iter()
                    .zip(r.iter())
                    .zip(dry_l.iter().zip(dry_r.iter()))
                    .enumerate()
                {
                    assert_eq!(
                        a.to_bits(),
                        c.to_bits(),
                        "left sample {i} moved at colour {colour}, split {split}"
                    );
                    assert_eq!(b.to_bits(), d.to_bits(), "right sample {i} moved");
                }
            }
        }
    }

    /// The device's whole job: a strike lift makes the attack louder
    /// WITHOUT making the tail louder. A compressor cannot do this and a
    /// fader certainly cannot, so it is the claim worth testing.
    #[test]
    fn a_strike_lift_raises_the_attack_and_leaves_the_tail_alone() {
        let (dry_l, dry_r) = hit(0.5, 60.0, 9_600);
        let attack = ..480usize; // the first 10 ms
        let tail = 4_800..; // 100 ms in, well past any split

        let mut flint = armed(|p| {
            p.strike_db = 12.0;
            p.body_db = 0.0;
            p.colour = 0.0; // measure the GAIN, not the character
        });
        let (mut l, mut r) = (dry_l.clone(), dry_r.clone());
        flint.process(&mut l, &mut r);

        let dry_attack = peak(dry_l.get(attack).unwrap());
        let wet_attack = peak(l.get(attack).unwrap());
        let dry_tail = peak(dry_l.get(tail.clone()).unwrap());
        let wet_tail = peak(l.get(tail).unwrap());

        assert!(
            wet_attack > dry_attack * 1.5,
            "the attack should have grown: {dry_attack} -> {wet_attack}"
        );
        assert!(
            (wet_tail - dry_tail).abs() < dry_tail * 0.05,
            "the tail must be left alone: {dry_tail} -> {wet_tail}"
        );
    }

    /// And the other half, the other way: a body cut shortens the tail
    /// without touching the hit.
    #[test]
    fn a_body_cut_shortens_the_tail_and_leaves_the_attack_alone() {
        let (dry_l, dry_r) = hit(0.5, 60.0, 9_600);
        let mut flint = armed(|p| {
            p.strike_db = 0.0;
            p.body_db = -12.0;
            p.colour = 0.0;
        });
        let (mut l, mut r) = (dry_l.clone(), dry_r.clone());
        flint.process(&mut l, &mut r);

        let dry_attack = peak(dry_l.get(..240).unwrap());
        let wet_attack = peak(l.get(..240).unwrap());
        let dry_tail = peak(dry_l.get(4_800..).unwrap());
        let wet_tail = peak(l.get(4_800..).unwrap());

        assert!(wet_tail < dry_tail * 0.5, "the tail should have shrunk");
        assert!(
            wet_attack > dry_attack * 0.7,
            "the attack must survive: {dry_attack} -> {wet_attack}"
        );
    }

    /// The opinion, measured rather than asserted: pushing STRIKE up with
    /// COLOUR on puts MORE high end into the attack than the same lift
    /// with COLOUR off. If this ever stops being true the device has quietly
    /// become an ordinary transient shaper.
    #[test]
    fn colour_brightens_the_strike_and_only_the_strike() {
        let (dry_l, dry_r) = hit(0.5, 60.0, 9_600);
        let run = |colour: f32| {
            let mut flint = armed(|p| {
                p.strike_db = 12.0;
                p.body_db = 0.0;
                p.colour = colour;
            });
            let (mut l, mut r) = (dry_l.clone(), dry_r.clone());
            flint.process(&mut l, &mut r);
            l
        };
        // Difference between neighbouring samples is a crude but honest
        // high-frequency measure, and it needs no FFT to be trusted.
        let slew = |b: &[f32]| {
            b.windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0f32, f32::max)
        };
        let plain = run(0.0);
        let coloured = run(1.0);

        let attack_plain = slew(plain.get(..480).unwrap());
        let attack_coloured = slew(coloured.get(..480).unwrap());
        assert!(
            attack_coloured > attack_plain * 1.05,
            "colour must sharpen the attack: {attack_plain} -> {attack_coloured}"
        );

        // ...and the tail must not have been brightened along with it.
        let tail_plain = slew(plain.get(4_800..).unwrap());
        let tail_coloured = slew(coloured.get(4_800..).unwrap());
        assert!(
            (tail_coloured - tail_plain).abs() < tail_plain * 0.05 + 1e-6,
            "colour reached the tail: {tail_plain} -> {tail_coloured}"
        );
    }

    #[test]
    fn processing_in_pieces_is_processing_whole() {
        let (dry_l, dry_r) = hit(0.7, 40.0, 3_000);
        let mut whole = armed(|_| {});
        let (mut wl, mut wr) = (dry_l.clone(), dry_r.clone());
        whole.process(&mut wl, &mut wr);

        let mut split = armed(|_| {});
        let (mut sl, mut sr) = (dry_l.clone(), dry_r.clone());
        let mut at = 0;
        for cut in [1usize, 5, 64, 127, 512, 1_000] {
            let end = (at + cut).min(sl.len());
            let (Some(l), Some(r)) = (sl.get_mut(at..end), sr.get_mut(at..end)) else {
                break;
            };
            split.process(l, r);
            at = end;
        }
        if let (Some(l), Some(r)) = (sl.get_mut(at..), sr.get_mut(at..)) {
            split.process(l, r);
        }
        for (i, (a, b)) in wl.iter().zip(sl.iter()).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "left sample {i} differs");
        }
        for (i, (a, b)) in wr.iter().zip(sr.iter()).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "right sample {i} differs");
        }
    }

    #[test]
    fn no_setting_produces_a_non_finite_sample() {
        for strike in [-18.0f32, 0.0, 18.0] {
            for body in [-18.0f32, 0.0, 18.0] {
                for colour in [0.0f32, 1.0] {
                    let mut flint = armed(|p| {
                        p.strike_db = strike;
                        p.body_db = body;
                        p.colour = colour;
                    });
                    let mut l = vec![f32::NAN, f32::INFINITY, -1e30, 0.0, 0.9, -0.9, 1e-30];
                    let mut r = l.clone();
                    flint.process(&mut l, &mut r);
                    assert!(
                        l.iter().chain(r.iter()).all(|s| s.is_finite()),
                        "strike {strike} body {body} colour {colour} gave {l:?}"
                    );
                }
            }
        }
        // Silence stays silent whatever the settings.
        let mut flint = armed(|p| p.strike_db = 18.0);
        let mut l = vec![0.0f32; 512];
        let mut r = vec![0.0f32; 512];
        flint.process(&mut l, &mut r);
        assert!(l.iter().chain(r.iter()).all(|s| *s == 0.0));
    }

    #[test]
    fn odd_lengths_and_mismatched_slices_are_accepted() {
        let mut flint = armed(|_| {});
        for len in [0usize, 1, 2, 3, 17, 63, 255] {
            let (mut l, mut r) = hit(0.5, 20.0, len);
            flint.process(&mut l, &mut r);
            assert!(l.iter().all(|s| s.is_finite()), "len {len}");
        }
        // A shorter right channel truncates rather than panicking.
        let mut l = vec![0.3f32; 32];
        let mut r = vec![0.3f32; 8];
        flint.process(&mut l, &mut r);
    }

    #[test]
    fn rendering_does_not_allocate() {
        let mut flint = armed(|_| {});
        let (mut l, mut r) = hit(0.6, 30.0, 256);
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..50 {
                flint.process(&mut l, &mut r);
            }
        });
    }

    /// Every row of the table round-trips through the params struct, and
    /// nonsense off disk is dragged back inside its range.
    #[test]
    fn every_row_round_trips_and_nonsense_is_sanitized() {
        let mut params = FlintParams::default();
        for def in p::TABLE.iter() {
            params.set(def.id, def.max);
            assert_eq!(params.get(def.id), Some(def.max), "row {}", def.name);
            params.set(def.id, def.min);
            assert_eq!(params.get(def.id), Some(def.min), "row {}", def.name);
            // Out of range clamps rather than sticking.
            params.set(def.id, def.max * 100.0 + 1.0);
            let got = params.get(def.id).unwrap();
            assert!(got <= def.max && got >= def.min, "row {} = {got}", def.name);
        }
        assert_eq!(params.get(9_999), None, "an unknown id has no value");

        let mut junk = FlintParams {
            strike_db: f32::NAN,
            body_db: 1e9,
            split_ms: -5.0,
            colour: f32::INFINITY,
            mix: 40.0,
            out: f32::NAN,
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
    }
}
