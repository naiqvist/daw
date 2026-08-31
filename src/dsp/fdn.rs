//! Feedback delay network reverb: eight modulated lines through a
//! lossless mixing matrix.
//!
//! # Why this rather than more combs
//!
//! [`reverb::Reverb`](crate::dsp::reverb::Reverb) is a Schroeder/Freeverb:
//! parallel combs, then series allpasses. Combs in parallel never
//! exchange energy, so each one rings at its own pitch and the tail is
//! the sum of four resonators — which is why Freeverb needs eight combs a
//! side and still sounds metallic on a transient.
//!
//! An FDN exchanges energy between every line on every sample. The mixing
//! matrix is UNITARY, so it preserves energy exactly: nothing decays
//! because of the mixing, and everything decays because of the per-line
//! gains, which are the only place decay is allowed to come from. That
//! separation is what makes the tail smooth and the decay control mean
//! what it says.
//!
//! # Shape
//!
//! ```text
//! in ─▶ 4 series allpass diffusers ─┬─▶ line 0 ─▶ damp ─▶ gain ─┐
//!                                   ├─▶ line 1 ─▶ damp ─▶ gain ─┤
//!                                   │      ⋮                    │  HADAMARD
//!                                   └─▶ line 7 ─▶ damp ─▶ gain ─┘   8 × 8
//!                                          ▲                        │
//!                                          └────────────────────────┘
//! ```
//!
//! The diffusers are DELAY-based allpasses, not the biquad kind: a
//! second-order allpass turns phase at one frequency, which is a
//! [`Disperser`](crate::dsp::filters::Disperser). A transient becomes a
//! dense cloud only if it is smeared across a few hundred SAMPLES, and
//! that needs a delay in the loop.
//!
//! # The matrix
//!
//! An 8-point Walsh-Hadamard, scaled by `1/√8`. It is the one piece of
//! new arithmetic here, and it is cheap: three butterfly stages of adds
//! and subtracts — 24 of them — and one multiply. No general matrix
//! product, no multiplies per element. Unitary by construction, which the
//! tests check directly rather than by listening to the tail.
//!
//! # Modulation
//!
//! Each line's read position wanders by a few samples on its own slow
//! oscillator. Without it an FDN's tail settles into a fixed set of modes
//! and rings; with it the modes drift and the ring becomes a wash. The
//! oscillator is a phase accumulator here rather than [`Lfo`] because the
//! feedback loop is per-sample and `Lfo::process` fills a block — the
//! same reason [`OnePole::tick_lowpass`] exists.
//!
//! State: ~400 bytes of indices and coefficients; the delay memory is the
//! caller's, as the kernel contract requires.
//! Per-sample cost: 8 interpolated reads + 8 writes + 8 one-pole damping
//! steps + 24 adds for the matrix, plus 4 allpass read/writes — about 70
//! multiply-adds.
//! Denormal-safe: relies on engine FTZ; the feedback decays through the
//! denormal range on a long tail.
//! In-place safe: no — input and outputs are distinct by signature.
//! Latency: 0 samples. The first reflection arrives after the shortest
//! line, which is delay, not a fixed offset to compensate.

use crate::dsp::filters::OnePole;

/// How many delay lines the network runs. Eight is the smallest power of
/// two that sounds like a room rather than a set of echoes, and a power
/// of two is what lets the Hadamard be butterflies instead of a matrix
/// product.
pub const LINES: usize = 8;

/// Series allpasses on the input.
const DIFFUSERS: usize = 4;

/// Line lengths in samples at [`TUNING_RATE`], all PRIME.
///
/// Prime so no two lines share a period: lengths with a common factor
/// line their echoes up into an audible pitch, which is the failure that
/// makes a cheap reverb sound like a pipe.
const LINE_TUNING: [usize; LINES] = [1039, 1181, 1327, 1483, 1601, 1747, 1889, 2039];

/// Diffuser lengths, also prime, and short — a few milliseconds each.
/// Long enough to smear a transient, short enough not to be heard as
/// separate echoes.
const DIFFUSER_TUNING: [usize; DIFFUSERS] = [149, 211, 293, 379];

const TUNING_RATE: f32 = 44_100.0;

/// Room size as a multiplier on the line lengths. The buffer is always
/// laid out for [`SIZE_MAX`] so size can move without reallocating.
pub const SIZE_MIN: f32 = 0.35;
pub const SIZE_MAX: f32 = 1.75;

/// The most a modulated read may wander from its nominal position, in
/// samples. Headroom is reserved for this on top of the longest line, so
/// a modulated read can never step outside its own line.
const MOD_MAX_SAMPLES: f32 = 8.0;

/// The allpass coefficient the diffusers use. Diffusion shapes the
/// density of the early cloud, not the length of the tail.
const DIFFUSER_G: f32 = 0.62;

/// Decay bounds, in seconds of RT60.
pub const DECAY_MIN: f32 = 0.15;
pub const DECAY_MAX: f32 = 30.0;

/// `1/√8`, the Hadamard's normalisation.
const HADAMARD_SCALE: f32 = 0.353_553_39;

/// A length at the running sample rate, from its figure at the tuning
/// rate. At least one sample: a zero-length delay line is a wire with an
/// index that wraps to itself.
fn scaled(tuned: usize, sample_rate: f32) -> usize {
    let rate = if sample_rate.is_finite() && sample_rate > 0.0 {
        sample_rate
    } else {
        TUNING_RATE
    };
    ((tuned as f32 * rate / TUNING_RATE).round() as usize).max(1)
}

/// An 8-point Walsh-Hadamard transform, normalised.
///
/// Three butterfly stages: 24 adds and subtracts, then one multiply per
/// element. UNITARY — it preserves the sum of squares exactly — which is
/// the property the whole design rests on, because it means the mixing
/// contributes nothing to the decay and the per-line gains are the only
/// thing that does.
#[inline]
fn hadamard(v: &mut [f32; LINES]) {
    let mut stride = 1;
    while stride < LINES {
        let mut base = 0;
        while base < LINES {
            for offset in base..base + stride {
                let partner = offset + stride;
                // Both indices are provably inside an 8-element array —
                // `stride` doubles from 1 to 4 and `base + 2*stride` never
                // exceeds LINES — but the contract's rule is that a kernel
                // does not index where it can iterate, and `get` keeps it
                // a fact rather than a comment.
                let (Some(a), Some(b)) = (v.get(offset).copied(), v.get(partner).copied()) else {
                    continue;
                };
                if let Some(slot) = v.get_mut(offset) {
                    *slot = a + b;
                }
                if let Some(slot) = v.get_mut(partner) {
                    *slot = a - b;
                }
            }
            base += stride * 2;
        }
        stride *= 2;
    }
    for value in v.iter_mut() {
        *value *= HADAMARD_SCALE;
    }
}

/// A cheap smooth oscillator on a `u32` phase, in `-1..=1`.
///
/// The parabolic sine: two multiplies and no table. It is modulating a
/// delay READ POSITION by a few samples, so a fraction of a percent of
/// harmonic error is inaudible — and a table here would be memory the
/// kernel is not allowed to own.
#[inline]
fn wobble(phase: u32) -> f32 {
    // Phase to -1..1 triangle, then the parabola that approximates sine.
    let t = (phase as f32 / u32::MAX as f32) * 2.0 - 1.0;
    let parabola = 4.0 * t * (1.0 - t.abs());
    parabola.clamp(-1.0, 1.0)
}

/// One delay line's placement and state.
#[derive(Debug, Clone, Copy)]
struct Line {
    /// Where this line starts inside the caller's buffer.
    offset: usize,
    /// How many samples are laid out for it: the longest it can ever be.
    capacity: usize,
    /// How long it currently is, which `size` moves.
    len: usize,
    /// The next cell to write.
    write: usize,
    /// Feedback gain, from the decay time and this line's own length.
    gain: f32,
    damp: OnePole,
    /// Modulation phase and its increment.
    phase: u32,
    inc: u32,
}

impl Line {
    fn new() -> Self {
        Self {
            offset: 0,
            capacity: 0,
            len: 1,
            write: 0,
            gain: 0.0,
            damp: OnePole::new(),
            phase: 0,
            inc: 0,
        }
    }
}

/// The reverb.
#[derive(Debug, Clone, Copy)]
pub struct Fdn {
    sample_rate: f32,
    lines: [Line; LINES],
    /// Diffuser placement and state, laid out the same way.
    diff_offset: [usize; DIFFUSERS],
    diff_len: [usize; DIFFUSERS],
    diff_write: [usize; DIFFUSERS],
    diffusion: f32,
    /// Modulation depth in samples.
    mod_depth: f32,
    /// How much buffer the layout needs, and whether it got it.
    needed: usize,
    ready: bool,
    size: f32,
    decay_s: f32,
}

impl Default for Fdn {
    fn default() -> Self {
        Self::new()
    }
}

impl Fdn {
    pub fn new() -> Self {
        Self {
            sample_rate: TUNING_RATE,
            lines: [Line::new(); LINES],
            diff_offset: [0; DIFFUSERS],
            diff_len: [1; DIFFUSERS],
            diff_write: [0; DIFFUSERS],
            diffusion: DIFFUSER_G,
            mod_depth: 2.0,
            needed: 0,
            ready: false,
            size: 1.0,
            decay_s: 2.0,
        }
    }

    /// How many floats the caller must provide at this sample rate.
    ///
    /// Sized for the biggest room PLUS the modulation headroom, so
    /// neither size nor modulation can ever need memory that is not
    /// already there — a reverb may not allocate while it is running.
    pub fn buffer_len(sample_rate: f32) -> usize {
        let headroom = MOD_MAX_SAMPLES.ceil() as usize + 2;
        let lines: usize = LINE_TUNING
            .iter()
            .map(|t| scaled((*t as f32 * SIZE_MAX) as usize, sample_rate) + headroom)
            .sum();
        let diffusers: usize = DIFFUSER_TUNING
            .iter()
            .map(|t| scaled(*t, sample_rate))
            .sum();
        lines + diffusers
    }

    /// Green zone: lay the lines out inside `buffers`, zero them, and arm
    /// the kernel.
    ///
    /// A buffer shorter than [`buffer_len`](Self::buffer_len) leaves the
    /// kernel DISARMED and silent rather than panicking — a mis-sized
    /// buffer is a caller bug that must not take the audio thread down.
    pub fn prepare(&mut self, sample_rate: f32, buffers: &mut [f32]) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            TUNING_RATE
        };
        let headroom = MOD_MAX_SAMPLES.ceil() as usize + 2;
        let mut offset = 0;
        for (line, tuned) in self.lines.iter_mut().zip(LINE_TUNING.iter()) {
            line.capacity =
                scaled((*tuned as f32 * SIZE_MAX) as usize, self.sample_rate) + headroom;
            line.offset = offset;
            line.len = line.capacity;
            line.write = 0;
            offset += line.capacity;
        }
        for ((off, len), tuned) in self
            .diff_offset
            .iter_mut()
            .zip(self.diff_len.iter_mut())
            .zip(DIFFUSER_TUNING.iter())
        {
            *len = scaled(*tuned, self.sample_rate);
            *off = offset;
            offset += *len;
        }
        self.needed = offset;
        self.ready = buffers.len() >= offset;

        // Each line wanders at its own slow rate — between about a third
        // of a hertz and a hertz — so no two ever line up and the drift
        // never becomes a rhythm.
        for (index, line) in self.lines.iter_mut().enumerate() {
            let hz = 0.31 + index as f32 * 0.086;
            let turns = hz / self.sample_rate;
            line.inc = (turns * u32::MAX as f32) as u32;
            // Spread the starting phases around the circle.
            line.phase = (index as u32).wrapping_mul(u32::MAX / LINES as u32);
        }
        self.set_size(self.size);
        self.set_decay(self.decay_s);
        self.set_damping(self.sample_rate * 0.25);
        self.reset(buffers);
    }

    /// Green zone: room size, as a multiplier on the line lengths.
    pub fn set_size(&mut self, size: f32) {
        let size = if size.is_finite() {
            size.clamp(SIZE_MIN, SIZE_MAX)
        } else {
            1.0
        };
        self.size = size;
        let headroom = MOD_MAX_SAMPLES.ceil() as usize + 2;
        for (line, tuned) in self.lines.iter_mut().zip(LINE_TUNING.iter()) {
            let want = scaled((*tuned as f32 * size) as usize, self.sample_rate);
            // Never past the capacity, and never so short that a
            // modulated read could reach behind the write head.
            line.len = want.clamp(headroom + 2, line.capacity);
            if line.write >= line.len {
                line.write = 0;
            }
        }
        // The gains depend on the lengths, so they follow.
        self.set_decay(self.decay_s);
    }

    /// Green zone: the tail's length as RT60 in seconds.
    ///
    /// Each line's gain is derived from ITS OWN length: a sample going
    /// round a short line more often must lose less each time, or the
    /// short lines die first and the tail changes colour as it fades.
    /// `g = 10^(-3 · L / RT60)` is the textbook relation — three decades
    /// of amplitude, which is 60 dB, over the decay time.
    pub fn set_decay(&mut self, seconds: f32) {
        let rt60 = if seconds.is_finite() {
            seconds.clamp(DECAY_MIN, DECAY_MAX)
        } else {
            2.0
        };
        self.decay_s = rt60;
        for line in self.lines.iter_mut() {
            let length_s = line.len as f32 / self.sample_rate;
            // Ceilinged just under unity: a gain of exactly one is a tail
            // that never ends, and one above it is an oscillator.
            line.gain = 10f32.powf(-3.0 * length_s / rt60).clamp(0.0, 0.9995);
        }
    }

    /// Green zone: the corner of the one-pole in each feedback path.
    ///
    /// Damping is what makes a room sound like a room rather than a
    /// tiled bathroom: high frequencies are absorbed on every bounce, so
    /// the tail gets darker as it fades rather than staying bright.
    pub fn set_damping(&mut self, cutoff_hz: f32) {
        let hz = if cutoff_hz.is_finite() {
            cutoff_hz.clamp(200.0, self.sample_rate * 0.49)
        } else {
            self.sample_rate * 0.25
        };
        for line in self.lines.iter_mut() {
            line.damp.prepare(self.sample_rate, hz);
        }
    }

    /// Green zone: how dense the early cloud is, `0..=1`.
    pub fn set_diffusion(&mut self, amount: f32) {
        let amount = if amount.is_finite() {
            amount.clamp(0.0, 1.0)
        } else {
            DIFFUSER_G
        };
        self.diffusion = amount * 0.75;
    }

    /// Green zone: how far the lines wander, in samples.
    pub fn set_modulation(&mut self, depth_samples: f32) {
        self.mod_depth = if depth_samples.is_finite() {
            depth_samples.clamp(0.0, MOD_MAX_SAMPLES)
        } else {
            0.0
        };
    }

    /// Green zone: zero the tail and every filter, keeping the tuning.
    ///
    /// What a transport discontinuity calls, so a seek does not drag the
    /// old room into the new position.
    pub fn reset(&mut self, buffers: &mut [f32]) {
        for sample in buffers.iter_mut() {
            *sample = 0.0;
        }
        for line in self.lines.iter_mut() {
            line.write = 0;
            line.damp.reset();
        }
        for write in self.diff_write.iter_mut() {
            *write = 0;
        }
    }

    /// Latency: none. Stated because the contract requires an answer, not
    /// because a network of delays could have a common offset to remove.
    pub fn latency(&self) -> usize {
        0
    }

    /// Red zone: mono in, STEREO wet out, any length.
    ///
    /// Pure wet — mixing against dry is the caller's job, the same
    /// division [`reverb::Reverb`](crate::dsp::reverb::Reverb) keeps.
    ///
    /// The two outputs are different sums of the lines rather than two
    /// networks: the Hadamard has already decorrelated them, so taking
    /// alternate lines to each side gives a wide image for no extra
    /// arithmetic at all.
    pub fn process(
        &mut self,
        input: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
        buffers: &mut [f32],
    ) {
        if !self.ready || buffers.len() < self.needed {
            for sample in out_l.iter_mut().chain(out_r.iter_mut()) {
                *sample = 0.0;
            }
            return;
        }
        let diffusion = self.diffusion;
        let depth = self.mod_depth;

        for ((left, right), dry) in out_l.iter_mut().zip(out_r.iter_mut()).zip(input.iter()) {
            // ---- input diffusion: four allpasses in series ------------
            let mut x = *dry;
            for index in 0..DIFFUSERS {
                let (Some(offset), Some(len), Some(write)) = (
                    self.diff_offset.get(index).copied(),
                    self.diff_len.get(index).copied(),
                    self.diff_write.get(index).copied(),
                ) else {
                    continue;
                };
                let at = offset + write;
                let Some(cell) = buffers.get_mut(at) else {
                    continue;
                };
                let delayed = *cell;
                // The Schroeder allpass: flat magnitude, and it smears in
                // TIME, which is what a biquad allpass cannot do.
                let v = x - diffusion * delayed;
                *cell = v;
                x = diffusion * v + delayed;
                if let Some(slot) = self.diff_write.get_mut(index) {
                    *slot = if write + 1 >= len { 0 } else { write + 1 };
                }
            }

            // ---- read every line, damped and scaled -------------------
            let mut taps = [0.0f32; LINES];
            for (index, line) in self.lines.iter_mut().enumerate() {
                // The read sits `len` behind the write, wandering by up to
                // `depth` samples. The offset is always positive so the
                // read can never pass the write head.
                let wander = (wobble(line.phase) * 0.5 + 0.5) * depth;
                line.phase = line.phase.wrapping_add(line.inc);
                let back = line.len as f32 - 1.0 - wander;
                let whole = back.floor().max(1.0);
                let frac = back - whole;
                let step = whole as usize;
                let read = (line.write + line.len - step.min(line.len - 1)) % line.len;
                let next = if read + 1 >= line.len { 0 } else { read + 1 };
                let (Some(a), Some(b)) = (
                    buffers.get(line.offset + read).copied(),
                    buffers.get(line.offset + next).copied(),
                ) else {
                    continue;
                };
                // Linear interpolation between the two neighbouring cells.
                // Enough for a modulation of a few samples: the artefacts
                // of linear interpolation are a gentle treble droop, and
                // this signal is already inside a damped feedback loop.
                let value = a + (b - a) * frac;
                let damped = line.damp.tick_lowpass(value);
                if let Some(slot) = taps.get_mut(index) {
                    *slot = damped * line.gain;
                }
            }

            // ---- the lossless mix ------------------------------------
            let mut mixed = taps;
            hadamard(&mut mixed);

            // ---- write back, with the diffused input added -----------
            for (index, line) in self.lines.iter_mut().enumerate() {
                let feedback = mixed.get(index).copied().unwrap_or(0.0);
                let at = line.offset + line.write;
                if let Some(cell) = buffers.get_mut(at) {
                    *cell = x + feedback;
                }
                line.write = if line.write + 1 >= line.len {
                    0
                } else {
                    line.write + 1
                };
            }

            // ---- alternate lines to each side ------------------------
            let mut sum_l = 0.0f32;
            let mut sum_r = 0.0f32;
            for (index, tap) in taps.iter().enumerate() {
                if index % 2 == 0 {
                    sum_l += *tap;
                } else {
                    sum_r += *tap;
                }
            }
            let norm = 2.0 / LINES as f32;
            *left = sum_l * norm;
            *right = sum_r * norm;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn armed(decay: f32) -> (Fdn, Vec<f32>) {
        let mut buffers = vec![0.0f32; Fdn::buffer_len(FS)];
        let mut fdn = Fdn::new();
        fdn.prepare(FS, &mut buffers);
        fdn.set_decay(decay);
        (fdn, buffers)
    }

    fn strike(fdn: &mut Fdn, buffers: &mut [f32], frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut input = vec![0.0f32; frames];
        if let Some(first) = input.first_mut() {
            *first = 1.0;
        }
        let mut left = vec![0.0f32; frames];
        let mut right = vec![0.0f32; frames];
        fdn.process(&input, &mut left, &mut right, buffers);
        (left, right)
    }

    fn energy(buf: &[f32]) -> f64 {
        buf.iter().map(|s| f64::from(*s) * f64::from(*s)).sum()
    }

    // ---------------------------------------------------- reference ---

    /// THE MATRIX IS UNITARY, and everything else rests on it. If the
    /// mixing lost or made energy the decay control would be a lie: the
    /// tail would die at a rate that had nothing to do with the number on
    /// the knob, and a long setting could run away into oscillation.
    #[test]
    fn the_hadamard_preserves_energy_exactly() {
        let cases: [[f32; LINES]; 4] = [
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0],
            [0.3, -0.7, 0.1, 0.9, -0.4, 0.25, -0.85, 0.6],
            [0.5; LINES],
        ];
        for case in cases {
            let before: f32 = case.iter().map(|v| v * v).sum();
            let mut after = case;
            hadamard(&mut after);
            let now: f32 = after.iter().map(|v| v * v).sum();
            assert!(
                (before - now).abs() < 1e-5,
                "energy {before} became {now} for {case:?}"
            );
        }

        // Applied TWICE it is the identity — the Hadamard is its own
        // inverse when normalised, which is the strongest single check
        // that the butterflies and the scale are both right.
        let mut there = [0.31f32, -0.62, 0.17, 0.88, -0.05, 0.44, -0.93, 0.12];
        let original = there;
        hadamard(&mut there);
        hadamard(&mut there);
        for (now, was) in there.iter().zip(original.iter()) {
            assert!((now - was).abs() < 1e-5, "{now} is not {was}");
        }
    }

    /// THE DECAY KNOB MEANS SECONDS. A longer setting must ring longer,
    /// and by roughly the ratio asked for — the property that separates a
    /// reverb from a box with a feedback control.
    #[test]
    fn a_longer_decay_rings_longer() {
        // Energy in a late window, for three decay times.
        let late = |decay: f32| {
            let (mut fdn, mut buffers) = armed(decay);
            let frames = (FS * 2.0) as usize;
            let (left, right) = strike(&mut fdn, &mut buffers, frames);
            let from = (FS * 0.9) as usize;
            energy(&left[from..]) + energy(&right[from..])
        };
        let short = late(0.4);
        let medium = late(2.0);
        let long = late(8.0);
        assert!(
            medium > short * 10.0,
            "2 s should outlast 0.4 s by far: {medium} vs {short}"
        );
        assert!(
            long > medium * 2.0,
            "8 s should outlast 2 s: {long} vs {medium}"
        );

        // And it always ENDS. A tail still ringing after ten times its
        // RT60 is a network with a gain at or above unity.
        let (mut fdn, mut buffers) = armed(0.3);
        let frames = (FS * 4.0) as usize;
        let (left, right) = strike(&mut fdn, &mut buffers, frames);
        let tail = energy(&left[frames - 4_800..]) + energy(&right[frames - 4_800..]);
        assert!(tail < 1e-6, "the tail never ended: {tail}");
    }

    /// A reverb is not a delay: after the first few milliseconds there
    /// must be no silence between reflections.
    #[test]
    fn the_tail_is_dense_rather_than_a_row_of_echoes() {
        let (mut fdn, mut buffers) = armed(3.0);
        let frames = (FS * 0.5) as usize;
        let (left, _) = strike(&mut fdn, &mut buffers, frames);
        // Past 100 ms, every 10 ms window carries signal.
        let start = (FS * 0.1) as usize;
        let window = (FS * 0.01) as usize;
        let mut quiet = 0;
        let mut at = start;
        while at + window < frames {
            if energy(&left[at..at + window]) < 1e-12 {
                quiet += 1;
            }
            at += window;
        }
        assert_eq!(quiet, 0, "{quiet} silent windows in the tail");
    }

    /// The two sides are DIFFERENT, or the reverb is mono wearing a
    /// stereo signature.
    #[test]
    fn the_two_outputs_are_decorrelated() {
        let (mut fdn, mut buffers) = armed(2.0);
        let frames = (FS * 0.5) as usize;
        let (left, right) = strike(&mut fdn, &mut buffers, frames);
        let difference: f64 = left
            .iter()
            .zip(right.iter())
            .map(|(l, r)| f64::from(l - r).abs())
            .sum();
        assert!(difference > 1.0, "the sides are identical: {difference}");
        // Both carry a comparable amount of the room, though — a wide
        // reverb, not one that forgot a channel.
        let (el, er) = (energy(&left), energy(&right));
        let ratio = el.max(er) / el.min(er).max(1e-12);
        assert!(ratio < 4.0, "the sides are lopsided: {el} against {er}");
    }

    // ------------------------------------- split-block equivalence ---

    #[test]
    fn split_blocks_are_bit_exact() {
        let mut input: Vec<f32> = (0..256).map(|i| ((i as f32) * 0.07).sin() * 0.5).collect();
        if let Some(first) = input.first_mut() {
            *first = 1.0;
        }

        let (mut whole, mut whole_buf) = armed(2.0);
        let mut wl = vec![0.0f32; 256];
        let mut wr = vec![0.0f32; 256];
        whole.process(&input, &mut wl, &mut wr, &mut whole_buf);

        let (mut split, mut split_buf) = armed(2.0);
        let mut sl = vec![0.0f32; 256];
        let mut sr = vec![0.0f32; 256];
        split.process(
            &input[..100],
            &mut sl[..100],
            &mut sr[..100],
            &mut split_buf,
        );
        split.process(
            &input[100..],
            &mut sl[100..],
            &mut sr[100..],
            &mut split_buf,
        );

        assert!(
            wl.iter().zip(&sl).all(|(a, b)| a.to_bits() == b.to_bits()),
            "left: 256 must equal 100 + 156"
        );
        assert!(
            wr.iter().zip(&sr).all(|(a, b)| a.to_bits() == b.to_bits()),
            "right: 256 must equal 100 + 156"
        );
    }

    // ----------------------------------------- the remaining three ---

    #[test]
    fn edges_silence_and_nonsense_all_stay_finite() {
        // Edge lengths, including zero and a non-power-of-two.
        for len in [0usize, 1, 3, 63] {
            let (mut fdn, mut buffers) = armed(2.0);
            let input = vec![0.5f32; len];
            let mut left = vec![0.0f32; len];
            let mut right = vec![0.0f32; len];
            fdn.process(&input, &mut left, &mut right, &mut buffers);
            assert!(
                left.iter().chain(right.iter()).all(|s| s.is_finite()),
                "len {len}"
            );
        }

        // Silence in, silence out — a reverb with nothing in it is not a
        // noise source.
        let (mut fdn, mut buffers) = armed(2.0);
        let input = vec![0.0f32; 512];
        let mut left = vec![0.0f32; 512];
        let mut right = vec![0.0f32; 512];
        fdn.process(&input, &mut left, &mut right, &mut buffers);
        assert!(left.iter().chain(right.iter()).all(|s| *s == 0.0));

        // A SHORT BUFFER leaves the kernel inert and silent rather than
        // panicking: a mis-sized buffer must not take the audio thread
        // down.
        let mut short = vec![0.0f32; 16];
        let mut fdn = Fdn::new();
        fdn.prepare(FS, &mut short);
        let input = vec![1.0f32; 64];
        let mut left = vec![9.0f32; 64];
        let mut right = vec![9.0f32; 64];
        fdn.process(&input, &mut left, &mut right, &mut short);
        assert!(
            left.iter().chain(right.iter()).all(|s| *s == 0.0),
            "a disarmed reverb must be silent"
        );

        // Every setting at both stops, and past them.
        for decay in [DECAY_MIN, 1.0, DECAY_MAX, f32::NAN, -5.0, 1e9] {
            for size in [SIZE_MIN, 1.0, SIZE_MAX, f32::NAN, 1e9] {
                let (mut fdn, mut buffers) = armed(2.0);
                fdn.set_decay(decay);
                fdn.set_size(size);
                fdn.set_damping(f32::NAN);
                fdn.set_diffusion(f32::NAN);
                fdn.set_modulation(f32::NAN);
                let (left, right) = strike(&mut fdn, &mut buffers, 2_048);
                assert!(
                    left.iter().chain(right.iter()).all(|s| s.is_finite()),
                    "decay {decay} size {size}"
                );
            }
        }

        // A long decaying tail stays finite rather than grinding into
        // NaN through the denormal range.
        let (mut fdn, mut buffers) = armed(DECAY_MAX);
        let (left, right) = strike(&mut fdn, &mut buffers, (FS * 3.0) as usize);
        assert!(left.iter().chain(right.iter()).all(|s| s.is_finite()));
    }

    #[test]
    fn processing_does_not_allocate() {
        let (mut fdn, mut buffers) = armed(4.0);
        let input = vec![0.25f32; 256];
        let mut left = vec![0.0f32; 256];
        let mut right = vec![0.0f32; 256];
        assert_no_alloc::assert_no_alloc(|| {
            for i in 0..100 {
                // Knobs move while it runs, which is where a lazy
                // implementation would reach for memory.
                if i % 10 == 0 {
                    fdn.set_size(0.5 + (i % 3) as f32 * 0.4);
                    fdn.set_decay(1.0 + (i % 5) as f32);
                    fdn.set_damping(2_000.0 + (i % 7) as f32 * 900.0);
                }
                fdn.process(&input, &mut left, &mut right, &mut buffers);
            }
        });
    }

    /// Size changes the room without ever reaching outside its own
    /// memory, and without needing more of it.
    #[test]
    fn size_moves_the_lines_inside_the_buffer_it_already_has() {
        let (mut fdn, mut buffers) = armed(2.0);
        let needed = fdn.needed;
        for size in [SIZE_MIN, 0.7, 1.0, 1.3, SIZE_MAX] {
            fdn.set_size(size);
            assert_eq!(fdn.needed, needed, "size must not change the memory");
            for line in fdn.lines.iter() {
                assert!(line.len <= line.capacity, "a line outgrew its slot");
                assert!(line.offset + line.capacity <= needed);
            }
            let (left, right) = strike(&mut fdn, &mut buffers, 1_024);
            assert!(left.iter().chain(right.iter()).all(|s| s.is_finite()));
        }
        assert!(buffers.len() >= needed);
    }

    /// Modulation is what stops the tail ringing. With it off the network
    /// settles into fixed modes; with it on they drift, and the two tails
    /// are audibly different signals.
    #[test]
    fn modulation_changes_the_tail() {
        let render = |depth: f32| {
            let (mut fdn, mut buffers) = armed(4.0);
            fdn.set_modulation(depth);
            let (left, _) = strike(&mut fdn, &mut buffers, (FS * 0.5) as usize);
            left
        };
        let still = render(0.0);
        let moving = render(MOD_MAX_SAMPLES);
        let difference: f64 = still
            .iter()
            .zip(moving.iter())
            .map(|(a, b)| f64::from(a - b).abs())
            .sum();
        assert!(difference > 1.0, "modulation did nothing: {difference}");
        assert!(moving.iter().all(|s| s.is_finite()));
    }

    /// What a sample costs, in nanoseconds. Printed, not asserted: a
    /// timing threshold fails on a loaded box and tells you nothing.
    ///
    /// Run in RELEASE for the figures the header quotes — plain `cargo
    /// test` builds this crate at `opt-level = 1` and reads several
    /// times slower — and when comparing a change across two runs, keep
    /// a CONTROL row the change cannot touch: a preceding build leaves
    /// the machine hot enough to move every number here by 2x.
    #[test]
    fn report_cost_per_sample() {
        use std::time::Instant;
        const BLOCK: usize = 256;
        const REPS: usize = 8_000;
        let row = |name: &str, run: &mut dyn FnMut()| {
            for _ in 0..500 {
                run();
            }
            let t = Instant::now();
            for _ in 0..REPS {
                run();
            }
            let ns = t.elapsed().as_nanos() as f64 / (REPS * BLOCK) as f64;
            println!(
                "{name:<26} {ns:7.2} ns/sample   {:5.3}% of a core at 48k",
                ns * 48_000.0 * 1e-9 * 100.0
            );
        };
        let sig: Vec<f32> = (0..BLOCK).map(|i| (i as f32 * 0.07).sin() * 0.4).collect();

        let mut bufs = vec![0.0f32; Fdn::buffer_len(FS)];
        let mut f = Fdn::new();
        f.prepare(FS, &mut bufs);
        f.set_size(1.0);
        f.set_decay(3.0);
        f.set_damping(6_000.0);
        f.set_diffusion(0.7);
        f.set_modulation(2.0);
        let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
        row("fdn 8 lines, stereo", &mut || {
            f.process(&sig, &mut l, &mut r, &mut bufs);
            std::hint::black_box((&mut l, &mut r));
        });
    }
}
