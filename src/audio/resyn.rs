//! The resynthesiser — the spectrum taken apart and put back together.
//!
//! The first device to run `dsp::fft` in the AUDIO PATH. Wiring again:
//! [`FrameCutter`](crate::dsp::fft::FrameCutter) cuts the stream into
//! overlapping frames, [`RealFft`](crate::dsp::fft::RealFft) transforms
//! them, [`OverlapAdd`](crate::dsp::fft::OverlapAdd) puts them back. All
//! three were written for exactly this and had never been called.
//!
//! # What happens to a frame
//!
//! ```text
//! window -> FFT -> magnitude
//!        -> per-bin attack/release
//!        -> formant  (the envelope moves, the fine structure stays)
//!        -> band gains
//!        -> warm     (tilt + per-bin compression)
//!        -> shift    (the whole magnitude spectrum slides, in Hz)
//!        -> IFFT with the ORIGINAL phase -> window -> overlap-add
//! ```
//!
//! The phase is left where it was found. That is what makes this a
//! RESYNTHESIS rather than a clean shifter: it smears, and the smearing
//! is the sound the device is for. Hiding it would produce a worse
//! version of a different device.
//!
//! The shift comes last so the band gains act on the spectrum where the
//! user put them, not on wherever the shift has moved it to.
//!
//! # Latency, and why the dry is delayed too
//!
//! [`LATENCY`] samples, constant, and reported to the graph so PDC can
//! compensate. `the_reported_latency_is_the_real_one` measures where an
//! impulse actually leaves rather than trusting this comment.
//!
//! The MIX blend therefore runs against a delayed copy of the input. A
//! resynthesis summed with an undelayed dry would be a comb filter with a
//! twenty-millisecond tooth spacing, which is not a mix control, it is a
//! fault.
//!
//! # Red zone
//!
//! `process` allocates nothing and cannot panic. Every buffer is born in
//! [`ResynCore::new`] and sized from the block length the graph compiled
//! for; the frame loop is bounded by the input it was handed; the FIFO
//! and the dry delay are fixed rings. Per-bin work is a fixed count of
//! multiplies plus one `powf` in warm mode.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::dsp::fft::{FrameCutter, OverlapAdd, RealFft, Window};
use crate::params::resyn as p;
use crate::params::{clamp as clamp_param, def};

/// The transform size. About 21 ms at 48 kHz — long enough to resolve a
/// bass partial, short enough that a transient survives recognisably.
pub const SIZE: usize = 1024;

/// The hop. Four to one overlap, which is what a Hann pair needs for a
/// flat reconstruction and what keeps the smear from turning into a
/// flutter at the frame rate.
pub const HOP: usize = 256;

/// Non-redundant bins.
pub const BINS: usize = SIZE / 2 + 1;

/// The delay from input to output, in samples.
///
/// `SIZE - HOP` of it is the frame overlap the cutter builds in; the
/// remaining `HOP` is the output FIFO's priming, which is what makes the
/// figure CONSTANT rather than jittering by up to a hop depending on
/// where a block boundary fell. Together: exactly `SIZE`.
pub const LATENCY: usize = SIZE;

/// How many bins either side the envelope averages over, for the formant
/// shift. Wide enough to smooth past individual partials, narrow enough
/// to keep a formant's shape.
const ENV_SPAN: usize = 8;

/// How far the formant gain is allowed to lift or cut, as a ratio. A
/// spectral envelope can have deep valleys, and an unbounded ratio
/// between two of them is a very loud noise.
const FORMANT_CLAMP: f32 = 8.0;

/// A resynthesiser's editable values, in ENGINE units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ResynParams {
    pub formant_st: f32,
    pub shift_hz: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    /// The switch as its wire INDEX, kept as f32 like every other engine
    /// value here.
    pub warm: f32,
    pub mix: f32,
    pub bands: [f32; p::BAND_COUNT],
}

impl Default for ResynParams {
    fn default() -> Self {
        Self {
            formant_st: def(p::TABLE, p::FORMANT).default,
            shift_hz: def(p::TABLE, p::SHIFT).default,
            attack_ms: def(p::TABLE, p::ATTACK).default,
            release_ms: def(p::TABLE, p::RELEASE).default,
            warm: def(p::TABLE, p::WARM).default,
            mix: def(p::TABLE, p::MIX).default,
            bands: [0.0; p::BAND_COUNT],
        }
    }
}

impl ResynParams {
    /// This state's value for a wire id. One place an id becomes a field.
    pub fn get(&self, param: u32) -> Option<f32> {
        Some(match param {
            p::FORMANT => self.formant_st,
            p::SHIFT => self.shift_hz,
            p::ATTACK => self.attack_ms,
            p::RELEASE => self.release_ms,
            p::WARM => self.warm,
            p::MIX => self.mix,
            _ => *self.bands.get(band_index(param)?)?,
        })
    }

    /// Set by wire id, ignoring anything the table does not describe.
    pub fn set(&mut self, param: u32, value: f32) {
        let Some(value) = clamp_param(p::TABLE, param, value) else {
            return;
        };
        match param {
            p::FORMANT => self.formant_st = value,
            p::SHIFT => self.shift_hz = value,
            p::ATTACK => self.attack_ms = value,
            p::RELEASE => self.release_ms = value,
            p::WARM => self.warm = value,
            p::MIX => self.mix = value,
            _ => {
                if let Some(index) = band_index(param)
                    && let Some(slot) = self.bands.get_mut(index)
                {
                    *slot = value;
                }
            }
        }
    }

    /// Whether the warm mode is engaged.
    pub fn warm_on(&self) -> bool {
        self.warm.round() as u32 == p::WARM_ON
    }
}

/// The band a per-band gain id names, if it is one.
pub fn band_index(param: u32) -> Option<usize> {
    let index = param.checked_sub(p::BAND0)? as usize;
    (index < p::BAND_COUNT).then_some(index)
}

/// One channel's streaming state. Everything here is per-channel because
/// it is memory, and memory shared between two channels is the two
/// channels leaking into each other.
#[derive(Debug, Clone)]
struct Channel {
    cutter: FrameCutter,
    cut_store: Vec<f32>,
    adder: OverlapAdd,
    add_store: Vec<f32>,
    /// One frame at a time: the cutter stops when this is full, which is
    /// what bounds the loop without needing to size for a worst-case
    /// block.
    frame: Vec<f32>,
    /// The hop the adder writes per frame.
    hop_out: Vec<f32>,
    /// Per-bin magnitude envelope — the attack/release state.
    smooth: Vec<f32>,
    /// Output FIFO, primed with [`HOP`] zeros so the delay is constant.
    fifo: Vec<f32>,
    fifo_head: usize,
    fifo_len: usize,
    /// The dry, delayed by [`LATENCY`], so the mix blends things that
    /// happened at the same time.
    dry: Vec<f32>,
    dry_write: usize,
}

impl Channel {
    fn new(sample_rate: f32) -> Self {
        let _ = sample_rate;
        let mut cutter = FrameCutter::new();
        cutter.prepare(SIZE, HOP);
        let mut adder = OverlapAdd::new();
        adder.prepare(SIZE, HOP);
        let mut ch = Self {
            cutter,
            cut_store: vec![0.0; SIZE],
            adder,
            add_store: vec![0.0; SIZE],
            frame: vec![0.0; SIZE],
            hop_out: vec![0.0; HOP],
            smooth: vec![0.0; BINS],
            // Room for the priming plus anything a block can add before
            // it is drained.
            fifo: vec![0.0; SIZE * 4],
            fifo_head: 0,
            fifo_len: 0,
            dry: vec![0.0; LATENCY],
            dry_write: 0,
        };
        ch.prime();
        ch
    }

    /// The FIFO's priming: [`HOP`] zeros, which is what turns a delay
    /// that jitters with the block boundary into a constant one.
    fn prime(&mut self) {
        self.fifo_head = 0;
        self.fifo_len = HOP;
        for s in self.fifo.iter_mut() {
            *s = 0.0;
        }
    }

    fn reset(&mut self) {
        self.cutter.reset();
        self.adder.reset();
        for s in self.cut_store.iter_mut() {
            *s = 0.0;
        }
        for s in self.add_store.iter_mut() {
            *s = 0.0;
        }
        for s in self.smooth.iter_mut() {
            *s = 0.0;
        }
        for s in self.dry.iter_mut() {
            *s = 0.0;
        }
        self.dry_write = 0;
        self.prime();
    }

    fn push_fifo(&mut self, samples: &[f32]) {
        let cap = self.fifo.len();
        for s in samples {
            if self.fifo_len == cap {
                // Cannot happen with a block no longer than the graph
                // compiled for; dropping is the fail-open, never a panic.
                break;
            }
            let at = (self.fifo_head + self.fifo_len) % cap;
            if let Some(slot) = self.fifo.get_mut(at) {
                *slot = *s;
            }
            self.fifo_len += 1;
        }
    }

    fn pop_fifo(&mut self) -> f32 {
        if self.fifo_len == 0 {
            return 0.0;
        }
        let cap = self.fifo.len();
        let out = self.fifo.get(self.fifo_head).copied().unwrap_or(0.0);
        self.fifo_head = (self.fifo_head + 1) % cap;
        self.fifo_len -= 1;
        out
    }

    /// Push one input sample into the dry delay and take the one that is
    /// [`LATENCY`] samples old.
    fn dry_tick(&mut self, x: f32) -> f32 {
        let cap = self.dry.len();
        let out = self.dry.get(self.dry_write).copied().unwrap_or(0.0);
        if let Some(slot) = self.dry.get_mut(self.dry_write) {
            *slot = x;
        }
        self.dry_write += 1;
        if self.dry_write >= cap {
            self.dry_write = 0;
        }
        out
    }
}

/// One resynthesiser.
#[derive(Debug, Clone)]
pub struct ResynCore {
    params: ResynParams,
    sample_rate: f32,
    fft: RealFft,
    analysis: Vec<f32>,
    synthesis: Vec<f32>,
    ch: [Channel; 2],
    // Frame scratch, shared: one frame is processed at a time.
    work: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    mag: Vec<f32>,
    phase: Vec<f32>,
    out_mag: Vec<f32>,
    shifted: Vec<f32>,
    env: Vec<f32>,
    fft_scratch: Vec<f32>,
    /// Which band each bin belongs to. Precomputed, so the per-bin loop
    /// is a lookup rather than a search.
    band_of: Vec<u8>,
}

impl ResynCore {
    /// Green zone: every buffer the callback will use is born here.
    pub fn new(sample_rate: f32, params: &ResynParams) -> Self {
        let mut params = *params;
        for row in p::TABLE {
            if let Some(value) = params.get(row.id) {
                params.set(row.id, value);
            }
        }

        let mut fft = RealFft::new();
        fft.prepare(SIZE);

        let mut analysis = vec![0.0; SIZE];
        crate::dsp::fft::fill_window(Window::Hann, &mut analysis);
        let mut synthesis = analysis.clone();
        let mut phase_sums = vec![0.0; HOP];
        // Unity overlap-add gain for THIS analysis/synthesis pair, rather
        // than a constant somebody worked out once for a different one.
        crate::dsp::fft::normalize_overlap(&analysis, &mut synthesis, HOP, &mut phase_sums);

        let bin_hz = sample_rate / SIZE as f32;
        let band_of = (0..BINS)
            .map(|k| {
                let hz = k as f32 * bin_hz;
                let mut band = 0u8;
                for (index, edge) in p::BAND_EDGES.iter().enumerate().skip(1) {
                    if hz >= *edge && index < p::BAND_COUNT {
                        band = index as u8;
                    }
                }
                band
            })
            .collect();

        Self {
            params,
            sample_rate,
            fft,
            analysis,
            synthesis,
            ch: [Channel::new(sample_rate), Channel::new(sample_rate)],
            work: vec![0.0; SIZE],
            re: vec![0.0; BINS],
            im: vec![0.0; BINS],
            mag: vec![0.0; BINS],
            phase: vec![0.0; BINS],
            out_mag: vec![0.0; BINS],
            shifted: vec![0.0; BINS],
            env: vec![0.0; BINS],
            fft_scratch: vec![0.0; RealFft::scratch_len(SIZE)],
            band_of,
        }
    }

    /// Red zone: one door for every letter.
    pub fn set_param(&mut self, param: u32, value: f32) {
        self.params.set(param, value);
    }

    /// This device's values, for the app to read back.
    pub fn params(&self) -> ResynParams {
        self.params
    }

    /// Green zone: forget the history, keep the settings.
    pub fn reset(&mut self) {
        for ch in self.ch.iter_mut() {
            ch.reset();
        }
    }

    /// Stated by the device itself, so the figure the graph compensates
    /// for and the delay it actually holds cannot drift apart.
    pub fn latency(&self) -> usize {
        LATENCY
    }

    /// One frame of spectral work, in `self.ch[channel].frame`, in place.
    fn transform(&mut self, channel: usize) {
        let Some(ch) = self.ch.get_mut(channel) else {
            return;
        };
        // --- analysis -------------------------------------------------
        self.work.copy_from_slice(&ch.frame);
        crate::dsp::fft::apply_window_in_place(&mut self.work, &self.analysis);
        self.fft.forward(
            &self.work,
            &mut self.re,
            &mut self.im,
            &mut self.fft_scratch,
        );
        crate::dsp::fft::magnitude_phase(&self.re, &self.im, &mut self.mag, &mut self.phase);

        // --- the per-bin envelope -------------------------------------
        //
        // The frame rate, not the sample rate: this state advances once
        // per hop, and a coefficient computed for samples would make both
        // knobs wrong by the overlap factor.
        let per_frame = HOP as f32 / self.sample_rate.max(1.0);
        let coeff = |ms: f32| 1.0 - (-per_frame / (ms * 1e-3).max(1e-6)).exp();
        let attack = coeff(self.params.attack_ms);
        let release = coeff(self.params.release_ms);
        for (state, target) in ch.smooth.iter_mut().zip(self.mag.iter()) {
            let c = if *target > *state { attack } else { release };
            *state += (*target - *state) * c;
        }

        // --- the spectral envelope, for the formant -------------------
        //
        // A box average over the smoothed magnitudes: wide enough to
        // smooth past individual partials, which is what makes it an
        // envelope rather than a copy.
        let mut running = 0.0f32;
        for k in 0..BINS {
            running += ch.smooth.get(k).copied().unwrap_or(0.0);
            if k > 2 * ENV_SPAN {
                running -= ch
                    .smooth
                    .get(k - (2 * ENV_SPAN + 1))
                    .copied()
                    .unwrap_or(0.0);
            }
            let span = (2 * ENV_SPAN + 1).min(k + 1) as f32;
            let at = k.saturating_sub(ENV_SPAN);
            if let Some(slot) = self.env.get_mut(at) {
                *slot = running / span;
            }
        }
        // The tail the box could not centre.
        for k in BINS.saturating_sub(ENV_SPAN)..BINS {
            let last = self.env.get(BINS - ENV_SPAN - 1).copied().unwrap_or(0.0);
            if let Some(slot) = self.env.get_mut(k) {
                *slot = last;
            }
        }

        // --- formant, bands, warm -------------------------------------
        let ratio = (self.params.formant_st / 12.0).exp2();
        let warm = self.params.warm_on();
        let mut band_gain = [1.0f32; p::BAND_COUNT];
        for (slot, db) in band_gain.iter_mut().zip(self.params.bands.iter()) {
            *slot = 10.0f32.powf(*db / 20.0);
        }
        let peak = ch.smooth.iter().fold(0.0f32, |a, m| a.max(*m)).max(1e-9);

        for k in 0..BINS {
            let mut m = ch.smooth.get(k).copied().unwrap_or(0.0);

            // FORMANT: the envelope moves, the fine structure stays. The
            // gain is the ratio between the envelope where it would be
            // and where it is, so the partials keep their own levels
            // relative to each other while the shape over them slides.
            if ratio != 1.0 {
                let from = k as f32 / ratio;
                let want = lerp_at(&self.env, from);
                let have = self.env.get(k).copied().unwrap_or(0.0);
                if have > 1e-9 {
                    m *= (want / have).clamp(1.0 / FORMANT_CLAMP, FORMANT_CLAMP);
                }
            }

            // BANDS.
            let band = self.band_of.get(k).copied().unwrap_or(0) as usize;
            m *= band_gain.get(band).copied().unwrap_or(1.0);

            // WARM: a tilt down toward the top, and a compression that
            // lifts quiet partials toward the loud ones. The second half
            // is the one an EQ cannot do, and the reason this is a mode
            // rather than another shelf.
            if warm {
                let t = k as f32 / (BINS - 1).max(1) as f32;
                m *= 10.0f32.powf(p::WARM_TILT_DB * t / 20.0);
                m = peak * (m / peak).max(0.0).powf(p::WARM_EXPONENT);
            }

            if let Some(slot) = self.out_mag.get_mut(k) {
                *slot = m;
            }
        }

        // --- SHIFT, last ----------------------------------------------
        //
        // Additive in hertz, so every partial moves the same distance and
        // their ratios change: inharmonic by construction, which is what
        // a spectral shift is and what separates it from a pitch shift.
        let bin_hz = self.sample_rate / SIZE as f32;
        let shift = self.params.shift_hz / bin_hz.max(1e-6);
        if shift != 0.0 {
            for k in 0..BINS {
                let from = k as f32 - shift;
                let value = if from < 0.0 {
                    0.0
                } else {
                    lerp_at(&self.out_mag, from)
                };
                if let Some(slot) = self.shifted.get_mut(k) {
                    *slot = value;
                }
            }
            self.out_mag.copy_from_slice(&self.shifted);
        }

        // --- synthesis ------------------------------------------------
        //
        // The ORIGINAL phase. See the module header: this is the line
        // that makes the device a resynthesiser.
        crate::dsp::fft::polar_to_cartesian(&self.out_mag, &self.phase, &mut self.re, &mut self.im);
        self.fft
            .inverse(&self.re, &self.im, &mut self.work, &mut self.fft_scratch);
        crate::dsp::fft::apply_window_in_place(&mut self.work, &self.synthesis);
        if let Some(ch) = self.ch.get_mut(channel) {
            ch.frame.copy_from_slice(&self.work);
        }
    }

    /// Red zone: run a stereo pair through the resynthesiser, in place.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let stereo = r.len() >= n;
        let mix = self.params.mix.clamp(0.0, 1.0);

        for channel in 0..2 {
            if channel == 1 && !stereo {
                break;
            }
            // Split so the borrow of the buffer and of `self` do not
            // overlap: the frame loop needs both.
            let io: &mut [f32] = if channel == 0 { l } else { &mut r[..n] };

            // Feed the cutter one frame at a time. The loop is bounded by
            // the input: `consumed` is always positive while there is
            // input left, so it cannot spin.
            let mut fed = 0usize;
            while fed < n {
                let (consumed, produced) = {
                    let Some(ch) = self.ch.get_mut(channel) else {
                        break;
                    };
                    let (a, b) = (&mut ch.frame, &mut ch.cut_store);
                    ch.cutter.process(&io[fed..], a, b)
                };
                if consumed == 0 && produced == 0 {
                    break;
                }
                fed += consumed;
                if produced > 0 {
                    self.transform(channel);
                    let Some(ch) = self.ch.get_mut(channel) else {
                        break;
                    };
                    let (frame, out, store) = (&ch.frame, &mut ch.hop_out, &mut ch.add_store);
                    let (_, written) = ch.adder.process(frame, out, store);
                    let hop: [f32; HOP] = {
                        let mut copy = [0.0f32; HOP];
                        copy[..written.min(HOP)].copy_from_slice(&ch.hop_out[..written.min(HOP)]);
                        copy
                    };
                    ch.push_fifo(&hop[..written.min(HOP)]);
                }
            }

            // Drain: one output sample per input sample, so the block
            // that came in is the block that goes out.
            let Some(ch) = self.ch.get_mut(channel) else {
                break;
            };
            for sample in io.iter_mut().take(n) {
                let dry = ch.dry_tick(*sample);
                let wet = ch.pop_fifo();
                *sample = dry + (wet - dry) * mix;
            }
        }

        // A mono node still has to leave the right channel alone rather
        // than half-written.
        if !stereo {
            for (i, sample) in r.iter_mut().enumerate() {
                if let Some(v) = l.get(i) {
                    *sample = *v;
                }
            }
        }
    }
}

/// Read `table` at a fractional index, linearly. Out of range reads zero,
/// which is the right answer for a magnitude spectrum.
fn lerp_at(table: &[f32], at: f32) -> f32 {
    if at < 0.0 || table.is_empty() {
        return 0.0;
    }
    let lo = at.floor() as usize;
    if lo + 1 >= table.len() {
        return table.last().copied().unwrap_or(0.0);
    }
    let f = at - lo as f32;
    let a = table.get(lo).copied().unwrap_or(0.0);
    let b = table.get(lo + 1).copied().unwrap_or(0.0);
    a + (b - a) * f
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn core(params: ResynParams) -> ResynCore {
        ResynCore::new(FS, &params)
    }

    fn sine(hz: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (i as f32 / FS * hz * std::f32::consts::TAU).sin() * 0.5)
            .collect()
    }

    /// Run a signal through in `block`-sized pieces, as the graph would.
    fn run(mut c: ResynCore, signal: &[f32], block: usize) -> Vec<f32> {
        let mut out = signal.to_vec();
        let mut r = signal.to_vec();
        let mut at = 0;
        while at < out.len() {
            let end = (at + block).min(out.len());
            let (a, b) = (&mut out[at..end], &mut r[at..end]);
            c.process(a, b);
            at = end;
        }
        out
    }

    /// THE ONE THAT MATTERS MOST: the delay the device reports is the
    /// delay it has. PDC subtracts this figure from every other path, so
    /// a device that lied about it would drag the whole track out of
    /// time — silently, and in a way no listening test localises.
    #[test]
    fn the_reported_latency_is_the_real_one() {
        let mut signal = vec![0.0f32; SIZE * 8];
        signal[SIZE] = 1.0;
        let out = run(core(ResynParams::default()), &signal, 256);
        let peak = out
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap())
            .map(|(i, _)| i)
            .unwrap();
        assert_eq!(
            peak,
            SIZE + LATENCY,
            "the impulse went in at {SIZE} and came out at {peak}, so the real \
             latency is {} and not the {LATENCY} reported",
            peak - SIZE
        );
    }

    /// And it does not depend on how the blocks were cut, which is what
    /// the FIFO's priming buys.
    #[test]
    fn the_latency_does_not_move_with_the_block_size() {
        for block in [1usize, 17, 64, 256, 1024] {
            let mut signal = vec![0.0f32; SIZE * 8];
            signal[SIZE] = 1.0;
            let out = run(core(ResynParams::default()), &signal, block);
            let peak = out
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap())
                .map(|(i, _)| i)
                .unwrap();
            assert_eq!(peak, SIZE + LATENCY, "block {block} moved the latency");
        }
    }

    /// At rest the device is a delay and very little else: the overlap-add
    /// reconstructs what went in. Not bit-exact — a transform round trip
    /// never is — but close enough that "flat" is a fair description.
    #[test]
    fn at_rest_it_reconstructs_its_input() {
        let signal = sine(440.0, SIZE * 8);
        let out = run(core(ResynParams::default()), &signal, 256);
        // Compare the settled middle against the input, delayed.
        let from = SIZE * 3;
        let to = SIZE * 6;
        let mut worst = 0.0f32;
        for i in from..to {
            worst = worst.max((out[i] - signal[i - LATENCY]).abs());
        }
        assert!(
            worst < 0.02,
            "reconstruction was off by {worst:.4}, which is not a round trip"
        );
    }

    /// Silence in, silence out — a spectral device that idles into noise
    /// is one people switch off.
    #[test]
    fn silence_stays_silent() {
        let out = run(core(ResynParams::default()), &vec![0.0f32; SIZE * 4], 256);
        assert!(out.iter().all(|s| s.abs() < 1e-9), "the resynth hissed");
    }

    /// A band gain does what it says, in its own band and not its
    /// neighbour's.
    #[test]
    fn a_band_gain_moves_its_own_band() {
        // Band 4 is 1 k - 2.2 k by `BAND_EDGES`.
        let cut = ResynParams {
            bands: {
                let mut b = [0.0f32; p::BAND_COUNT];
                b[4] = -24.0;
                b
            },
            ..ResynParams::default()
        };
        let level = |params: ResynParams, hz: f32| {
            let signal = sine(hz, SIZE * 8);
            let out = run(core(params), &signal, 256);
            let tail = &out[SIZE * 5..SIZE * 7];
            (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt()
        };
        let flat = ResynParams::default();
        assert!(
            level(cut, 1_500.0) < level(flat, 1_500.0) * 0.25,
            "cutting band 4 left 1.5 kHz at {:.4} against {:.4}",
            level(cut, 1_500.0),
            level(flat, 1_500.0)
        );
        assert!(
            level(cut, 300.0) > level(flat, 300.0) * 0.8,
            "cutting band 4 also took 300 Hz down"
        );
    }

    /// The shift is ADDITIVE: a partial comes out where it went in plus
    /// the knob, in hertz.
    #[test]
    fn the_shift_moves_a_partial_by_its_own_hertz() {
        let params = ResynParams {
            shift_hz: 200.0,
            ..ResynParams::default()
        };
        let signal = sine(1_000.0, SIZE * 8);
        let out = run(core(params), &signal, 256);
        // Where the energy ended up, by a coarse bin search.
        let tail: Vec<f32> = out[SIZE * 5..SIZE * 5 + SIZE].to_vec();
        let mut fft = RealFft::new();
        fft.prepare(SIZE);
        let (mut re, mut im) = (vec![0.0; BINS], vec![0.0; BINS]);
        let mut scratch = vec![0.0; RealFft::scratch_len(SIZE)];
        fft.forward(&tail, &mut re, &mut im, &mut scratch);
        let (mut mag, mut ph) = (vec![0.0; BINS], vec![0.0; BINS]);
        crate::dsp::fft::magnitude_phase(&re, &im, &mut mag, &mut ph);
        let loudest = mag
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(k, _)| k)
            .unwrap();
        let hz = loudest as f32 * FS / SIZE as f32;
        assert!(
            (hz - 1_200.0).abs() < 100.0,
            "a 1 kHz tone shifted by 200 Hz came out at {hz:.0} Hz"
        );
    }

    /// Warm mode changes the spectrum rather than only the level: it
    /// tilts down toward the top, which a level control cannot do.
    #[test]
    fn warm_tilts_the_spectrum_down() {
        let hot = |warm: bool| {
            let params = ResynParams {
                warm: if warm { p::WARM_ON as f32 } else { 0.0 },
                ..ResynParams::default()
            };
            let level = |hz: f32| {
                let signal = sine(hz, SIZE * 8);
                let out = run(core(params), &signal, 256);
                let tail = &out[SIZE * 5..SIZE * 7];
                (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt()
            };
            level(8_000.0) / level(200.0)
        };
        assert!(
            hot(true) < hot(false) * 0.9,
            "warm left the balance at {:.3} against {:.3}",
            hot(true),
            hot(false)
        );
    }

    /// Every table row reaches a field and comes back, the eight band
    /// gains included.
    #[test]
    fn every_row_round_trips() {
        let mut params = ResynParams::default();
        for row in p::TABLE {
            let mid = (row.min + row.max) * 0.5;
            params.set(row.id, mid);
            assert_eq!(
                params.get(row.id),
                Some(mid),
                "`{}` did not come back",
                row.name
            );
        }
    }

    /// The bands cover the spectrum without a gap or an overlap.
    #[test]
    fn every_bin_lands_in_exactly_one_band() {
        let c = core(ResynParams::default());
        assert_eq!(c.band_of.len(), BINS);
        assert!(c.band_of.iter().all(|b| (*b as usize) < p::BAND_COUNT));
        // And they ascend: a bin's band never goes backwards with
        // frequency, which is what makes the picker's order meaningful.
        for pair in c.band_of.windows(2) {
            assert!(pair[1] >= pair[0], "the bands are out of order");
        }
    }
}
