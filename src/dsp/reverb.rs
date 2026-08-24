//! Schroeder/Freeverb-style reverb: parallel damped combs into series
//! allpasses.
//!
//! The delay lines are the whole cost of a reverb, and a kernel may not
//! allocate — so the caller owns them. [`Reverb::buffer_len`] says how many
//! floats are needed at a sample rate, the caller provides that slice to
//! [`prepare`](Reverb::prepare) and to every [`process`](Reverb::process)
//! call, and the kernel only ever indexes inside offsets it computed
//! itself. A short slice makes the kernel INERT (silent wet output) rather
//! than panicking: a mis-sized buffer is a caller bug that must not take
//! the audio thread down.
//!
//! Output is pure WET. Mixing against dry is one job, this is another; the
//! caller crossfades.
//!
//! State: ~140 bytes (indices and coefficients; the delay memory is the
//! caller's). Per-sample cost: 4 comb reads + 4 writes + 4 one-pole
//! damping steps, then 2 allpass read/writes — about 20 multiply-adds.
//! Denormal-safe: relies on engine FTZ; the feedback path decays through
//! the denormal range on a long tail.
//! In-place safe: no — `input` and `out` are distinct by signature.
//! Latency: 0 samples (the earliest reflection appears after the shortest
//! comb, which is delay, not latency to compensate).

/// Comb and allpass lengths, in samples, at the rate they were tuned for.
/// Mutually prime so their echo patterns do not line up into a ringing
/// pitch.
const COMB_TUNING: [usize; COMBS] = [1116, 1188, 1277, 1356];
const ALLPASS_TUNING: [usize; ALLPASSES] = [556, 441];
const TUNING_RATE: f32 = 44_100.0;
const COMBS: usize = 4;
const ALLPASSES: usize = 2;

/// Fixed allpass coefficient — the classic value; it shapes diffusion, not
/// decay, so it is not a user parameter.
const ALLPASS_FEEDBACK: f32 = 0.5;
/// Feedback range — this is DECAY, the length of the tail. The floor keeps
/// a short decay from being a click; the ceiling stays under 1.0 so the
/// tail always ends.
const FEEDBACK_MIN: f32 = 0.28;
const FEEDBACK_MAX: f32 = 0.98;
/// How much of the damping control reaches the one-pole coefficient.
const DAMP_SCALE: f32 = 0.4;

/// Room SIZE scales the delay lengths — the physical dimensions of the
/// space, which is a different thing from how long it rings. The buffer is
/// always allocated for `SIZE_MAX` so size can move without reallocating.
const SIZE_MIN: f32 = 0.45;
const SIZE_MAX: f32 = 1.6;

/// Makeup applied to the wet output.
///
/// A comb with feedback `g` amplifies broadband input by `1/sqrt(1 - g*g)`,
/// and four of them sum incoherently for another `sqrt(4)`. Undoing exactly
/// that is what keeps the wet signal at roughly the level that went in —
/// otherwise a short decay is inaudible and a long one is deafening, and
/// the mix knob feels broken at both ends.
const WET_MAKEUP: f32 = 1.0;

#[derive(Debug, Clone, Copy)]
pub struct Reverb {
    comb_off: [usize; COMBS],
    comb_len: [usize; COMBS],
    comb_idx: [usize; COMBS],
    /// One-pole lowpass state inside each comb's feedback path — this is
    /// what makes the tail darken as it decays instead of ringing forever
    /// at full brightness.
    comb_store: [f32; COMBS],
    ap_off: [usize; ALLPASSES],
    ap_len: [usize; ALLPASSES],
    ap_idx: [usize; ALLPASSES],
    /// Longest each line may be — what the buffer was allocated for.
    comb_max: [usize; COMBS],
    ap_max: [usize; ALLPASSES],
    /// Kept so `set_room` can re-scale the lines without re-preparing.
    sample_rate: f32,
    feedback: f32,
    damp1: f32,
    damp2: f32,
    /// Output scaling that cancels the combs' gain, so the wet level does
    /// not swing with the decay setting.
    wet_scale: f32,
    /// Floats the delay lines need. Zero until `prepare`.
    needed: usize,
    /// False until `prepare` saw a big enough buffer.
    ready: bool,
}

impl Default for Reverb {
    fn default() -> Self {
        Self::new()
    }
}

/// Delay length for a tuning value at a sample rate — at least 1, so no
/// line can ever be zero-length.
fn scaled(tuning: usize, sample_rate: f32) -> usize {
    let r = if sample_rate.is_finite() && sample_rate > 0.0 {
        sample_rate
    } else {
        TUNING_RATE
    };
    ((tuning as f32 * r / TUNING_RATE) as usize).max(1)
}

impl Reverb {
    pub fn new() -> Self {
        Self {
            comb_off: [0; COMBS],
            comb_len: [1; COMBS],
            comb_idx: [0; COMBS],
            comb_store: [0.0; COMBS],
            comb_max: [1; COMBS],
            ap_off: [0; ALLPASSES],
            ap_len: [1; ALLPASSES],
            ap_idx: [0; ALLPASSES],
            ap_max: [1; ALLPASSES],
            sample_rate: TUNING_RATE,
            feedback: FEEDBACK_MIN,
            damp1: 0.0,
            damp2: 1.0,
            wet_scale: 1.0,
            needed: 0,
            ready: false,
        }
    }

    /// Floats of delay memory needed at `sample_rate`. Green zone: call it
    /// to size the buffer before `prepare`.
    pub fn buffer_len(sample_rate: f32) -> usize {
        // Sized for the biggest room, so `set_room` can grow the lines
        // without ever needing more memory — a reverb may not allocate
        // while it is running.
        COMB_TUNING
            .iter()
            .chain(ALLPASS_TUNING.iter())
            .map(|t| scaled((*t as f32 * SIZE_MAX) as usize, sample_rate))
            .sum()
    }

    /// Green zone: lay the delay lines out inside `buffers`, zero them, and
    /// arm the kernel. A buffer shorter than [`buffer_len`](Self::buffer_len)
    /// leaves the kernel disarmed and silent — never panicking.
    pub fn prepare(&mut self, sample_rate: f32, buffers: &mut [f32]) {
        self.sample_rate = sample_rate;
        // Lay out at the MAXIMUM length, so every line has room to grow
        // and no two lines can ever overlap when size changes.
        let mut off = 0;
        for (i, t) in COMB_TUNING.iter().enumerate() {
            self.comb_max[i] = scaled((*t as f32 * SIZE_MAX) as usize, sample_rate);
            self.comb_len[i] = self.comb_max[i];
            self.comb_off[i] = off;
            off += self.comb_max[i];
        }
        for (i, t) in ALLPASS_TUNING.iter().enumerate() {
            self.ap_max[i] = scaled((*t as f32 * SIZE_MAX) as usize, sample_rate);
            self.ap_len[i] = self.ap_max[i];
            self.ap_off[i] = off;
            off += self.ap_max[i];
        }
        self.needed = off;
        self.ready = buffers.len() >= off;
        self.reset(buffers);
    }

    /// Zero the tail and the filter memory, keeping the tuning. What a
    /// transport discontinuity calls so a seek does not drag the old room
    /// into the new position.
    pub fn reset(&mut self, buffers: &mut [f32]) {
        self.comb_idx = [0; COMBS];
        self.ap_idx = [0; ALLPASSES];
        self.comb_store = [0.0; COMBS];
        for s in buffers.iter_mut() {
            *s = 0.0;
        }
    }

    /// Green zone: the three room controls, all `0..=1`.
    ///
    /// - `size` — the space's DIMENSIONS. Scales the delay lengths, which
    ///   changes the character (a small bright room against a hall), not
    ///   how long it rings.
    /// - `decay` — how long the tail LASTS. Feedback amount.
    /// - `damp` — how fast the tail loses its highs.
    ///
    /// Size and decay are genuinely different controls: a small room can
    /// ring for a long time and a big one can be dead.
    pub fn set_room(&mut self, size: f32, decay: f32, damp: f32) {
        let unit = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        let (size, decay, damp) = (unit(size), unit(decay), unit(damp));

        // Delay lengths, never past what the buffer was laid out for.
        let factor = SIZE_MIN + size * (SIZE_MAX - SIZE_MIN);
        for (i, tuning) in COMB_TUNING.iter().enumerate() {
            let want = scaled((*tuning as f32 * factor) as usize, self.sample_rate);
            self.comb_len[i] = want.clamp(1, self.comb_max[i]);
            // A line that just shrank could leave the cursor past its end.
            if self.comb_idx[i] >= self.comb_len[i] {
                self.comb_idx[i] = 0;
            }
        }
        for (i, tuning) in ALLPASS_TUNING.iter().enumerate() {
            let want = scaled((*tuning as f32 * factor) as usize, self.sample_rate);
            self.ap_len[i] = want.clamp(1, self.ap_max[i]);
            if self.ap_idx[i] >= self.ap_len[i] {
                self.ap_idx[i] = 0;
            }
        }

        self.feedback = FEEDBACK_MIN + decay * (FEEDBACK_MAX - FEEDBACK_MIN);
        self.damp1 = damp * DAMP_SCALE;
        self.damp2 = 1.0 - self.damp1;
        // Cancel the combs' broadband gain so the wet level holds steady
        // as decay moves. See `WET_MAKEUP`.
        let g = self.feedback;
        self.wet_scale = WET_MAKEUP * (1.0 - g * g).max(1e-6).sqrt() / (COMBS as f32).sqrt();
    }

    /// Latency, for plugin delay compensation: none. A reverb's first
    /// reflection is late by design, not by processing.
    pub fn latency(&self) -> usize {
        0
    }

    /// Red zone: write the WET signal of `input` into `out`, truncating to
    /// the shorter of the two. `buffers` must be the same slice handed to
    /// `prepare`; a short one silences the output instead of panicking.
    pub fn process(&mut self, input: &[f32], out: &mut [f32], buffers: &mut [f32]) {
        if !self.ready || buffers.len() < self.needed {
            for s in out.iter_mut() {
                *s = 0.0;
            }
            return;
        }

        for (o, x) in out.iter_mut().zip(input.iter()) {
            let dry = *x;
            let mut acc = 0.0f32;

            // Parallel damped combs.
            for c in 0..COMBS {
                let at = self.comb_off[c] + self.comb_idx[c];
                // `at` is inside the slice: off + idx < off + len <= needed
                // <= buffers.len(), checked above. `get_mut` keeps that a
                // fact rather than a comment.
                let Some(cell) = buffers.get_mut(at) else {
                    continue;
                };
                let read = *cell;
                acc += read;
                // One-pole lowpass in the feedback path.
                self.comb_store[c] = read * self.damp2 + self.comb_store[c] * self.damp1;
                *cell = dry + self.comb_store[c] * self.feedback;
                self.comb_idx[c] += 1;
                if self.comb_idx[c] >= self.comb_len[c] {
                    self.comb_idx[c] = 0;
                }
            }

            // Series allpasses: diffusion, so the combs stop sounding like
            // four separate echoes.
            for a in 0..ALLPASSES {
                let at = self.ap_off[a] + self.ap_idx[a];
                let Some(cell) = buffers.get_mut(at) else {
                    continue;
                };
                let read = *cell;
                let sum = acc + read * ALLPASS_FEEDBACK;
                *cell = sum;
                acc = read - acc;
                self.ap_idx[a] += 1;
                if self.ap_idx[a] >= self.ap_len[a] {
                    self.ap_idx[a] = 0;
                }
            }

            *o = acc * self.wet_scale;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn armed(size: f32, damp: f32) -> (Reverb, Vec<f32>) {
        let mut bufs = vec![0.0f32; Reverb::buffer_len(SR)];
        let mut r = Reverb::new();
        r.prepare(SR, &mut bufs);
        r.set_room(size, size, damp);
        (r, bufs)
    }

    fn energy(s: &[f32]) -> f32 {
        s.iter().map(|x| x.abs()).sum()
    }

    // ---------------------------------------------------------- reference

    #[test]
    fn an_impulse_becomes_a_decaying_tail() {
        let (mut r, mut bufs) = armed(0.7, 0.2);
        let mut input = vec![0.0f32; 48_000];
        input[0] = 1.0;
        let mut out = vec![0.0f32; 48_000];
        r.process(&input, &mut out, &mut bufs);

        // Nothing arrives before the shortest comb: a reverb is made of
        // delays, so the very first samples are silent. The allpasses pass
        // their direct signal straight through, so the first reflection is
        // the shortest comb's echo alone.
        let first = r.comb_len[0];
        assert!(
            out[..first - 1].iter().all(|x| *x == 0.0),
            "output before the first reflection must be silence"
        );
        assert!(energy(&out[first..first + 100]) > 0.0, "then it sounds");

        // And it decays: each later window is quieter than the one before.
        let w = 8_000;
        let a = energy(&out[w..2 * w]);
        let b = energy(&out[2 * w..3 * w]);
        let c = energy(&out[4 * w..5 * w]);
        assert!(a > b && b > c, "tail must decay: {a} then {b} then {c}");
        assert!(out.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn size_sets_the_decay_time_and_damping_darkens() {
        let tail = |size: f32, damp: f32| {
            let (mut r, mut bufs) = armed(size, damp);
            let mut input = vec![0.0f32; 48_000];
            input[0] = 1.0;
            let mut out = vec![0.0f32; 48_000];
            r.process(&input, &mut out, &mut bufs);
            energy(&out[30_000..])
        };
        // A bigger room rings longer.
        assert!(
            tail(0.9, 0.0) > tail(0.2, 0.0) * 2.0,
            "size must lengthen the tail"
        );
        // Damping removes energy from the feedback path, so the late tail
        // is quieter than the same room undamped.
        assert!(
            tail(0.9, 1.0) < tail(0.9, 0.0),
            "damping must shorten the tail"
        );
    }

    // ------------------------------------------- split-block equivalence

    #[test]
    fn split_block_is_bit_exact() {
        let input: Vec<f32> = (0..256).map(|i| ((i as f32) * 0.11).sin()).collect();

        let (mut a, mut abuf) = armed(0.6, 0.3);
        let mut whole = vec![0.0f32; 256];
        a.process(&input, &mut whole, &mut abuf);

        let (mut b, mut bbuf) = armed(0.6, 0.3);
        let mut split = vec![0.0f32; 256];
        b.process(&input[..100], &mut split[..100], &mut bbuf);
        b.process(&input[100..], &mut split[100..], &mut bbuf);

        assert!(
            whole
                .iter()
                .zip(&split)
                .all(|(x, y)| x.to_bits() == y.to_bits()),
            "one 256 call must equal 100 + 156"
        );
    }

    // ------------------------------------------------------------ no-alloc

    #[test]
    fn process_does_not_allocate() {
        let (mut r, mut bufs) = armed(0.8, 0.5);
        let input = vec![0.25f32; 512];
        let mut out = vec![0.0f32; 512];
        assert_no_alloc::assert_no_alloc(|| {
            for _ in 0..100 {
                r.process(&input, &mut out, &mut bufs);
            }
        });
    }

    // -------------------------------------------------------- edge lengths

    #[test]
    fn edge_lengths_zero_one_and_non_power_of_two() {
        let (mut r, mut bufs) = armed(0.5, 0.5);
        r.process(&[], &mut [], &mut bufs);

        let mut one = [9.0f32; 1];
        r.process(&[1.0], &mut one, &mut bufs);
        assert!(one[0].is_finite());

        let mut seven = [9.0f32; 7];
        r.process(&[0.1; 7], &mut seven, &mut bufs);
        assert!(seven.iter().all(|x| x.is_finite()));

        // Mismatched lengths truncate to the shorter, never panic.
        let mut short = [0.0f32; 3];
        r.process(&[0.5; 16], &mut short, &mut bufs);
        let mut long = [0.0f32; 16];
        r.process(&[0.5; 3], &mut long, &mut bufs);
        assert!(
            long[7..].iter().all(|x| *x == 0.0),
            "untouched tail stays put"
        );
    }

    /// A caller that sizes the buffer wrong gets silence, not a crash. This
    /// is the rule that keeps a green-zone mistake off the audio thread.
    #[test]
    fn a_short_buffer_is_inert_not_fatal() {
        let mut tiny = vec![0.0f32; 10];
        let mut r = Reverb::new();
        r.prepare(SR, &mut tiny);
        r.set_room(0.9, 0.9, 0.1);
        let mut out = vec![9.0f32; 64];
        r.process(&[1.0f32; 64], &mut out, &mut tiny);
        assert!(out.iter().all(|x| *x == 0.0), "disarmed means silent");
    }

    // ------------------------------------------- silence & denormal tail

    #[test]
    fn silence_in_silence_out_and_the_tail_stays_finite() {
        let (mut r, mut bufs) = armed(0.9, 0.1);
        let mut out = vec![9.0f32; 1024];
        r.process(&[0.0f32; 1024], &mut out, &mut bufs);
        assert!(
            out.iter().all(|x| *x == 0.0),
            "silence cannot make a room ring"
        );

        // Longest possible tail, driven then left alone: finite forever,
        // and never NaN.
        let (mut r, mut bufs) = armed(1.0, 0.0);
        let mut out = vec![0.0f32; 4096];
        r.process(&[1.0f32; 4096], &mut out, &mut bufs);
        for _ in 0..200 {
            r.process(&[0.0f32; 4096], &mut out, &mut bufs);
            assert!(out.iter().all(|x| x.is_finite()), "tail went non-finite");
        }
    }

    #[test]
    fn reset_clears_the_room() {
        let (mut r, mut bufs) = armed(0.9, 0.0);
        let mut out = vec![0.0f32; 4096];
        r.process(&[1.0f32; 4096], &mut out, &mut bufs);
        r.reset(&mut bufs);
        let mut after = vec![9.0f32; 4096];
        r.process(&[0.0f32; 4096], &mut after, &mut bufs);
        assert!(
            after.iter().all(|x| *x == 0.0),
            "a reset room must be silent — a seek cannot drag the old tail along"
        );
    }

    #[test]
    fn nonsense_settings_do_not_break_it() {
        let (mut r, mut bufs) = armed(0.5, 0.5);
        r.set_room(f32::NAN, f32::NAN, f32::INFINITY);
        let mut out = vec![0.0f32; 256];
        r.process(&[0.5f32; 256], &mut out, &mut bufs);
        assert!(
            out.iter().all(|x| x.is_finite()),
            "NaN settings must not leak"
        );
    }
}
