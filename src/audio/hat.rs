//! The 808 hi-hat's voice — the instrument half of `Node::Hat`.
//!
//! Node-side wiring, not a kernel: every piece of arithmetic here belongs
//! to `src/dsp/`, and this file's whole job is to say which kernel feeds
//! which, and when.
//!
//! # The path, in order
//!
//! ```text
//! six squares at 205.3, 304.4, 369.6, 522.7, 540.0, 800.0 Hz
//!        └─▶ sum ─▶ BANDPASS ─▶ VCA (closed | open env) ─▶ HIGHPASS
//!            ─▶ SATURATOR ─▶ out
//! ```
//!
//! # Why six squares and not filtered noise
//!
//! Because that is what the machine does, and the two do not sound
//! alike. The TR-808's hi-hat contains NO noise source: it sums six
//! square-wave oscillators running at fixed, mutually inharmonic
//! frequencies and filters the result. The metallic clang everybody
//! recognises is those six squares beating against one another — a dense
//! but *deterministic* comb of intermodulation products. Filtered white
//! noise gives a "tss" with no pitch in it at all, which is why the
//! sample-based imitations of this sound are recognisably not it.
//!
//! The six frequencies are the machine's own; see
//! [`params::hat::RATIOS`](crate::params::hat::RATIOS). They are not
//! adjustable individually, and deliberately: the ratios ARE the
//! instrument. `tune` scales all six together, so the bank can be moved
//! without being detuned into some other machine.
//!
//! # Why the oscillators free-run
//!
//! They are NOT reset on the strike, and this is the second half of the
//! accuracy. The 808's oscillators run continuously and the envelope
//! simply opens a gate onto them, so each hit catches the bank at a
//! different phase and therefore a slightly different timbre. A hat that
//! restarts its phase every time is identical hit after hit — the
//! machine-gun quality that gives away a sampled hat on a fast pattern.
//!
//! This costs nothing in reproducibility: the node renders every block
//! whether or not it is sounding, so the phase at any sample is a
//! function of transport position, and a seek cuts the voice through
//! `all_sound_off` like every other timeline-locked node.
//!
//! # Open and closed are ONE voice
//!
//! The 808 has two buttons and one oscillator bank, and a closed hat
//! played over a ringing open one cuts it. That is what a single voice
//! with a retriggering envelope does for free, so that is what this is.
//! Which envelope a note gets is decided by its PITCH:
//! [`OPEN_NOTE`](crate::params::hat::OPEN_NOTE) and above is open,
//! anything below is closed — GM's F#1/G#1/A#1, so an ordinary drum-map
//! pattern plays the right button without anybody configuring anything.

use crate::dsp::adsr::ExpDecay;
use crate::dsp::filters::{Mode, Svf};
use crate::dsp::osc::{self, MipOsc, Waveform};
use crate::dsp::shaper::{Mode as ShapeMode, Waveshaper};
use crate::params::hat as hp;

/// How many samples one run of the render loop covers.
///
/// Unlike the kick and snare this is not a control-rate boundary — a hat
/// has nothing that needs rebuilding mid-note. It is here to BOUND THE
/// SCRATCH: the six oscillators need somewhere to render before they are
/// summed, and a fixed-size array is the only kind of scratch a red-zone
/// path may have. The caller's block length is not bounded; this is.
pub const CHUNK: usize = 32;

/// The oscillators in the bank. The machine's own count.
pub const OSCILLATORS: usize = 6;

/// A hi-hat's settings, in engine units. Parallel to
/// [`params::hat::TABLE`](crate::params::hat::TABLE).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct HatParams {
    pub tune: f32,
    pub closed_ms: f32,
    pub open_ms: f32,
    pub band_hz: f32,
    pub band_q: f32,
    pub hp_hz: f32,
    pub drive: f32,
    pub gain: f32,
}

impl Default for HatParams {
    /// Every default read from the one table — and the defaults ARE the
    /// machine: tune 1.0 puts the bank on its original six frequencies.
    fn default() -> Self {
        let at = |id: u32| crate::params::def(hp::TABLE, id).default;
        Self {
            tune: at(hp::TUNE),
            closed_ms: at(hp::CLOSED_DECAY),
            open_ms: at(hp::OPEN_DECAY),
            band_hz: at(hp::BP_HZ),
            band_q: at(hp::BP_Q),
            hp_hz: at(hp::HP_HZ),
            drive: at(hp::DRIVE),
            gain: at(hp::GAIN),
        }
    }
}

impl HatParams {
    /// Write one parameter by table id, clamped through the table.
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = crate::params::clamp(hp::TABLE, param, value) else {
            return;
        };
        match param {
            hp::TUNE => self.tune = value,
            hp::CLOSED_DECAY => self.closed_ms = value,
            hp::OPEN_DECAY => self.open_ms = value,
            hp::BP_HZ => self.band_hz = value,
            hp::BP_Q => self.band_q = value,
            hp::HP_HZ => self.hp_hz = value,
            hp::DRIVE => self.drive = value,
            hp::GAIN => self.gain = value,
            _ => {}
        }
    }

    pub fn get(&self, param: u32) -> f32 {
        match param {
            hp::TUNE => self.tune,
            hp::CLOSED_DECAY => self.closed_ms,
            hp::OPEN_DECAY => self.open_ms,
            hp::BP_HZ => self.band_hz,
            hp::BP_Q => self.band_q,
            hp::HP_HZ => self.hp_hz,
            hp::DRIVE => self.drive,
            hp::GAIN => self.gain,
            _ => 0.0,
        }
    }
}

/// The hi-hat's single voice: one oscillator bank, one gate.
pub struct HatVoice {
    sample_rate: f32,
    params: HatParams,
    /// The knobs as letters last set them: what a lock's restore
    /// returns to. `params` is the LIVE patch, which a lock may hold
    /// elsewhere for one hit.
    base: HatParams,

    /// The bank. One table set between all six — the tables are read-only
    /// once built, so sharing costs nothing and allocates once.
    bank: [MipOsc; OSCILLATORS],
    tables: Vec<f32>,

    band: Svf,
    high: Svf,

    /// ONE envelope, not two. Open and closed are the same gate opened
    /// for different lengths, which is why a closed hat cuts an open one.
    env: ExpDecay,

    shaper: Waveshaper,
    velocity: f32,

    /// What the two filters are currently tuned to, so a block only pays
    /// for a re-tune when something actually moved.
    tuned_band_hz: f32,
    tuned_band_q: f32,
    tuned_hp_hz: f32,
    /// What the bank is currently tuned to, likewise.
    tuned_scale: f32,

    /// Scratch for one run. Sized at compile time; nothing here allocates
    /// while running.
    voice_scratch: [f32; CHUNK],
    env_scratch: [f32; CHUNK],
}

impl Default for HatVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl HatVoice {
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            params: HatParams::default(),
            base: HatParams::default(),
            bank: [MipOsc::new(); OSCILLATORS],
            tables: Vec::new(),
            band: Svf::new(),
            high: Svf::new(),
            env: ExpDecay::new(),
            shaper: Waveshaper::new(),
            velocity: 1.0,
            tuned_band_hz: 0.0,
            tuned_band_q: 0.0,
            tuned_hp_hz: 0.0,
            tuned_scale: 0.0,
            voice_scratch: [0.0; CHUNK],
            env_scratch: [0.0; CHUNK],
        }
    }

    /// Green zone: build the square tables and settle every kernel.
    ///
    /// The ONE allocation in this file, and it happens at compile.
    pub fn prepare(&mut self, sample_rate: f32, params: HatParams) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.tables.resize(osc::table_len(Waveform::Square), 0.0);
        osc::build_tables(Waveform::Square, &mut self.tables);
        for osc in &mut self.bank {
            osc.prepare(self.sample_rate, Waveform::Square);
        }
        self.params = params;
        self.env.prepare(self.sample_rate, params.closed_ms);
        self.retune_bank(true);
        self.retune_filters(true);
        self.reset();
        // The bank is spread across its cycle at reset rather than left
        // in phase. Six oscillators all starting at zero sum to one loud
        // edge on the very first sample — a click that is not part of the
        // instrument, and one the free-running phase would otherwise take
        // a while to disperse.
        self.spread_phases();
    }

    /// Give each oscillator a different starting phase.
    ///
    /// An irrational-ish spread rather than even sixths: evenly spaced
    /// phases on frequencies that are nearly rational still line their
    /// edges up periodically, which is heard as a low buzz under the
    /// metal.
    fn spread_phases(&mut self) {
        for (i, osc) in self.bank.iter_mut().enumerate() {
            osc.set_phase((i as f32 * 0.618_034) % 1.0);
        }
    }

    /// The bank's six frequencies, rebuilt when `tune` moves.
    ///
    /// Every one clamped below Nyquist independently, so a hostile tune
    /// on a low sample rate folds nothing back down into the band.
    fn retune_bank(&mut self, force: bool) {
        let scale = if self.params.tune.is_finite() {
            self.params.tune.clamp(hp::TUNE_MIN, hp::TUNE_MAX)
        } else {
            1.0
        };
        if !force && (scale - self.tuned_scale).abs() <= 1e-6 {
            return;
        }
        self.tuned_scale = scale;
        let ceiling = self.sample_rate * 0.45;
        for (osc, ratio) in self.bank.iter_mut().zip(hp::RATIOS.iter()) {
            osc.set_freq((*ratio * scale).clamp(1.0, ceiling));
        }
    }

    /// The band and the output highpass, rebuilt only when something
    /// moved. Both cost a transcendental; neither is worth paying for a
    /// number that has not changed.
    fn retune_filters(&mut self, force: bool) {
        let ceiling = self.sample_rate * 0.45;
        let band = self.params.band_hz.clamp(20.0, ceiling);
        let q = self.params.band_q;
        let high = self.params.hp_hz.clamp(20.0, ceiling);
        if force
            || (band - self.tuned_band_hz).abs() > self.tuned_band_hz.max(1.0) * 1e-4
            || (q - self.tuned_band_q).abs() > 1e-4
        {
            self.tuned_band_hz = band;
            self.tuned_band_q = q;
            self.band.prepare(self.sample_rate, band, q);
        }
        if force || (high - self.tuned_hp_hz).abs() > self.tuned_hp_hz.max(1.0) * 1e-4 {
            self.tuned_hp_hz = high;
            // Butterworth: the output stage shapes the band, it does not
            // add a resonance of its own.
            self.high
                .prepare(self.sample_rate, high, core::f32::consts::FRAC_1_SQRT_2);
        }
    }

    /// Green zone: silence the gate and the filters.
    ///
    /// The oscillators are reset too, because this is what a transport
    /// discontinuity calls: after a seek nothing from before the jump may
    /// still be ringing, and that includes the bank's phase.
    pub fn reset(&mut self) {
        for osc in &mut self.bank {
            osc.reset();
        }
        self.spread_phases();
        self.band.reset();
        self.high.reset();
        self.env.reset();
    }

    /// A letter: the knob moves, and the live patch with it.
    pub fn set_param(&mut self, param: u32, value: f32) {
        self.base.set(param, value);
        self.apply_param(param, value);
    }

    /// A parameter LOCK at a note boundary: `Some` holds the live patch
    /// at the note's own value, `None` returns it to the knob. The knob
    /// itself never moves, so a lock is heard on its hit and no other.
    pub fn plock(&mut self, param: u32, value: Option<f32>) {
        let value = value.unwrap_or_else(|| self.base.get(param));
        self.apply_param(param, value);
    }

    pub fn plock_glide(&mut self, param: u32, alpha: f32) {
        let live = self.params.get(param);
        let base = self.base.get(param);
        self.apply_param(param, live + (base - live) * alpha.clamp(0.0, 1.0));
    }

    fn apply_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
        match param {
            hp::TUNE => self.retune_bank(false),
            hp::BP_HZ | hp::BP_Q | hp::HP_HZ => self.retune_filters(false),
            // The decays are read at the strike, so nothing to rebuild
            // here: a decay turned mid-pattern is heard on the next hit.
            _ => {}
        }
    }

    pub fn params(&self) -> HatParams {
        self.params
    }

    pub fn active(&self) -> bool {
        self.env.active()
    }

    /// Strike. The PITCH picks the button: open at and above
    /// [`OPEN_NOTE`](crate::params::hat::OPEN_NOTE), closed below it.
    ///
    /// The oscillators are deliberately NOT reset — see the module note.
    pub fn trigger(&mut self, pitch: u8, velocity: u8) {
        self.velocity = f32::from(velocity.max(1)) / 127.0;
        let decay = if pitch >= hp::OPEN_NOTE {
            self.params.open_ms
        } else {
            self.params.closed_ms
        };
        self.env.prepare(self.sample_rate, decay);
        // Retriggering REPLACES the tail, which is the choke: a closed
        // hat over a ringing open one cuts it, exactly as the machine's
        // shared oscillator bank does.
        self.env.trigger(1.0);
    }

    /// Red zone: render into `out`, ADDING to what is there.
    pub fn render_add(&mut self, out: &mut [f32], gain: f32) {
        if self.tables.is_empty() {
            return;
        }
        let mut written = 0usize;
        while written < out.len() {
            let take = CHUNK.min(out.len() - written);
            let Some(block) = out.get_mut(written..written + take) else {
                return;
            };
            self.render_run(block, gain);
            written += take;
        }
    }

    /// One run of samples. Fixed-size scratch, so `take` never exceeds
    /// [`CHUNK`].
    fn render_run(&mut self, out: &mut [f32], gain: f32) {
        let n = out.len();
        let (Some(mix), Some(env)) = (
            self.voice_scratch.get_mut(..n),
            self.env_scratch.get_mut(..n),
        ) else {
            return;
        };

        // --- the bank -------------------------------------------------
        //
        // The six run WHETHER OR NOT the gate is open — that is what
        // "free-running" means, and skipping them while silent would put
        // the phase back under the envelope's control.
        for sample in mix.iter_mut() {
            *sample = 0.0;
        }
        for osc in &mut self.bank {
            osc.process(env, &self.tables);
            for (sum, square) in mix.iter_mut().zip(env.iter()) {
                *sum += *square;
            }
        }
        // Averaged, so the bank peaks where one square would: six squares
        // summed raw is six times the headroom of one, and the saturator
        // downstream would then be a different effect at the same drive.
        let norm = 1.0 / OSCILLATORS as f32;
        for sample in mix.iter_mut() {
            *sample *= norm;
        }

        if !self.env.active() {
            // Silent, but the bank has still advanced and the filters
            // still need their state settled by the signal that passed —
            // which it just did, above. Nothing to add to `out`.
            return;
        }

        // --- band, gate, output stage ---------------------------------
        self.band.process(mix, Mode::BandpassUnity);
        self.env.process(env);
        let velocity = self.velocity;
        for (sample, level) in mix.iter_mut().zip(env.iter()) {
            *sample *= *level * velocity;
        }
        self.high.process(mix, Mode::Highpass);

        self.shaper
            .configure(ShapeMode::SoftClip, self.params.drive, 0.0, 1.0);
        self.shaper.process(mix);

        let level = self.params.gain * gain;
        for (sample, voice) in out.iter_mut().zip(mix.iter()) {
            *sample += *voice * level;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn voice(params: HatParams) -> HatVoice {
        let mut v = HatVoice::new();
        v.prepare(FS, params);
        v
    }

    fn render(v: &mut HatVoice, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; frames];
        v.render_add(&mut out, 1.0);
        out
    }

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0f32, |peak, s| peak.max(s.abs()))
    }

    fn rms(buf: &[f32]) -> f32 {
        if buf.is_empty() {
            return 0.0;
        }
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    #[test]
    fn a_fresh_hat_is_silent_until_it_is_struck() {
        let mut v = voice(HatParams::default());
        let quiet = render(&mut v, 512);
        assert!(quiet.iter().all(|s| *s == 0.0), "silent before the strike");
        assert!(!v.active());

        v.trigger(42, 100);
        assert!(v.active());
        let hit = render(&mut v, 512);
        assert!(peak(&hit) > 0.01, "the strike made sound: {}", peak(&hit));
    }

    /// THE BANK IS THE MACHINE'S. Six oscillators, on the TR-808's own
    /// six frequencies, scaled together by `tune` and never against each
    /// other. Changing one of these numbers is changing which machine
    /// this is, so the numbers are the test.
    #[test]
    fn the_oscillator_bank_is_the_808s_own_six_frequencies() {
        assert_eq!(hp::RATIOS.len(), OSCILLATORS);
        assert_eq!(
            hp::RATIOS,
            [205.3, 304.4, 369.6, 522.7, 540.0, 800.0],
            "these are the machine's measured oscillator frequencies"
        );

        // Mutually INHARMONIC: no pair is a simple integer ratio, which is
        // what makes six squares metal instead of a buzzy saw.
        for (i, a) in hp::RATIOS.iter().enumerate() {
            for b in hp::RATIOS.iter().skip(i + 1) {
                let ratio = b / a;
                let nearest = ratio.round();
                assert!(
                    nearest < 1.5 || (ratio - nearest).abs() > 0.02,
                    "{a} and {b} are very nearly harmonic ({ratio})"
                );
            }
        }

        // And the default patch IS the machine: tune at exactly unity.
        assert_eq!(HatParams::default().tune, 1.0);
    }

    /// OPEN AND CLOSED COME FROM THE NOTE, and open really is longer. A
    /// hat whose two buttons sounded the same length would have one.
    #[test]
    fn the_note_picks_the_button_and_open_outlasts_closed() {
        let params = HatParams {
            closed_ms: 50.0,
            open_ms: 800.0,
            ..HatParams::default()
        };
        // 300 ms in: the closed hat is long gone, the open one is not.
        let late = (FS * 0.3) as usize;
        let window = 2_400usize;

        let mut closed = voice(params);
        closed.trigger(42, 127); // GM closed hi-hat
        let short = render(&mut closed, late + window);

        let mut open = voice(params);
        open.trigger(hp::OPEN_NOTE, 127); // GM open hi-hat
        let long = render(&mut open, late + window);

        let closed_late = rms(&short[late..]);
        let open_late = rms(&long[late..]);
        assert!(
            closed_late < 1e-4,
            "the closed hat should be gone at 300 ms: {closed_late}"
        );
        assert!(
            open_late > closed_late * 50.0 && open_late > 1e-4,
            "the open hat should still be ringing: {open_late}"
        );

        // The pedal hat, GM 44, is below the open note and so is closed.
        // A fact about the constant, checked where the constant is.
        const _: () = assert!(44 < hp::OPEN_NOTE);
    }

    /// A CLOSED HAT CHOKES A RINGING OPEN ONE. One bank, one gate — the
    /// machine's behaviour, and the reason this is a single voice.
    #[test]
    fn a_closed_hat_chokes_the_open_one() {
        let params = HatParams {
            closed_ms: 40.0,
            open_ms: 1_500.0,
            ..HatParams::default()
        };
        let mut v = voice(params);
        v.trigger(hp::OPEN_NOTE, 127);
        let _ = render(&mut v, 2_400); // 50 ms of open hat

        // Now close it, and 200 ms later there must be nothing left.
        v.trigger(42, 127);
        let after = render(&mut v, (FS * 0.25) as usize);
        let tail = rms(&after[after.len() - 2_400..]);
        assert!(tail < 1e-4, "the open tail survived the choke: {tail}");

        // Where an uninterrupted open hat would still be going strong.
        let mut uncut = voice(params);
        uncut.trigger(hp::OPEN_NOTE, 127);
        let free = render(&mut uncut, 2_400 + (FS * 0.25) as usize);
        let still = rms(&free[free.len() - 2_400..]);
        assert!(still > tail * 50.0, "the control case died too: {still}");
    }

    /// THE OSCILLATORS FREE-RUN, so no two hits are identical. A hat that
    /// restarted its phase every strike is the machine-gun sound that
    /// gives a sampled hat away on a fast pattern.
    #[test]
    fn no_two_hits_are_bit_identical() {
        let mut v = voice(HatParams::default());
        v.trigger(42, 127);
        let first = render(&mut v, 1_024);
        // A gap that is not a whole number of any oscillator's period.
        let _ = render(&mut v, 3_571);
        v.trigger(42, 127);
        let second = render(&mut v, 1_024);

        assert!(
            first
                .iter()
                .zip(&second)
                .any(|(a, b)| a.to_bits() != b.to_bits()),
            "the two hits were identical — the bank is not free-running"
        );
        // But they are the same INSTRUMENT: comparable level, not noise.
        let (a, b) = (rms(&first), rms(&second));
        assert!(
            a > 0.0 && b > 0.0 && (a / b) > 0.5 && (a / b) < 2.0,
            "the two hits are not the same sound: {a} against {b}"
        );
    }

    /// The band knob actually moves the band — a filter tuned once at
    /// prepare and never again would leave the knob doing nothing.
    #[test]
    fn the_band_knob_moves_where_the_energy_sits() {
        let take = |band_hz: f32| {
            let mut v = voice(HatParams {
                band_hz,
                hp_hz: 1_000.0,
                band_q: 4.0,
                drive: 1.0,
                ..HatParams::default()
            });
            v.trigger(42, 127);
            let buf = render(&mut v, 2_048);
            buf.windows(2)
                .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
                .count()
        };
        assert!(
            take(14_000.0) > take(2_500.0),
            "a higher band must give faster crossings"
        );
    }

    /// Split-block equivalence — the property the segmented transport
    /// depends on.
    #[test]
    fn rendering_is_the_same_however_the_block_is_split() {
        let params = HatParams::default();
        let mut whole = voice(params);
        whole.trigger(42, 100);
        let mut a = vec![0.0f32; 256];
        whole.render_add(&mut a, 1.0);

        let mut split = voice(params);
        split.trigger(42, 100);
        let mut b = vec![0.0f32; 256];
        split.render_add(&mut b[..100], 1.0);
        split.render_add(&mut b[100..], 1.0);

        assert!(
            a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()),
            "256 must equal 100 + 156"
        );
    }

    /// The render path allocates nothing — including the silent path,
    /// where the bank still turns.
    #[test]
    fn rendering_does_not_allocate() {
        let mut v = voice(HatParams {
            drive: 8.0,
            band_q: 12.0,
            ..HatParams::default()
        });
        let mut out = vec![0.0f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..100 {
                if i % 16 == 0 {
                    v.trigger(if i % 32 == 0 { 42 } else { hp::OPEN_NOTE }, 100);
                }
                v.render_add(&mut out, 0.8);
            }
        });
    }

    /// Nonsense in, finite out — at every extreme the table allows.
    #[test]
    fn no_setting_produces_a_non_finite_sample() {
        for def in hp::TABLE {
            for value in [def.min, def.default, def.max] {
                let mut params = HatParams::default();
                params.set(def.id, value);
                let mut v = voice(params);
                for pitch in [0u8, 42, hp::OPEN_NOTE, 127] {
                    v.trigger(pitch, 127);
                    let out = render(&mut v, 1_024);
                    assert!(
                        out.iter().all(|s| s.is_finite()),
                        "{} at {value}, pitch {pitch}",
                        def.name
                    );
                }
            }
        }

        // A low sample rate must not fold the bank back into the band.
        let mut slow = HatVoice::new();
        slow.prepare(
            8_000.0,
            HatParams {
                tune: 2.0,
                ..HatParams::default()
            },
        );
        slow.trigger(42, 127);
        let mut out = vec![0.0f32; 1_024];
        slow.render_add(&mut out, 1.0);
        assert!(out.iter().all(|s| s.is_finite()));

        // And a voice never prepared renders silence.
        let mut bare = HatVoice::new();
        bare.trigger(42, 127);
        let mut none = vec![0.0f32; 128];
        bare.render_add(&mut none, 1.0);
        assert!(none.iter().all(|s| *s == 0.0));
    }

    /// Every table id round-trips, and an unknown id is ignored.
    #[test]
    fn every_parameter_reaches_its_field() {
        let mut params = HatParams::default();
        for def in hp::TABLE {
            let want = (def.min + def.max) * 0.5;
            params.set(def.id, want);
            assert!(
                (params.get(def.id) - want).abs() < 1e-4,
                "{} did not round-trip",
                def.name
            );
        }
        let before = params;
        params.set(9_999, 1.0);
        assert_eq!(params, before, "an unknown id must change nothing");

        params.set(hp::TUNE, 1e9);
        assert!((params.tune - hp::TUNE_MAX).abs() < 1e-3);
        params.set(hp::TUNE, -1e9);
        assert!((params.tune - hp::TUNE_MIN).abs() < 1e-3);
    }
}
