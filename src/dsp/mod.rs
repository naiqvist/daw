//! DSP kernel library — red zone. Leaf code with pure contracts.
//!
//! Effects and instruments are NEW WIRING, NOT NEW ARITHMETIC: Nodes do
//! wiring, kernels do arithmetic, the graph never knows kernels exist.
//!
//! Full contract: `notes/20260823-dsp-kernel-contract.md`. In short, inside
//! any `process`-like function there is no allocation, locking, syscalls,
//! I/O, logging, panicking, unbounded loops, or crate dependencies — std
//! math only. Sample rate arrives through `prepare()`, never queried.
//! Kernels process planar `&[f32]` / `&mut [f32]` blocks of any length
//! (including 0 and 1); stereo is the caller calling twice.
//!
//! Every kernel ships with five tests: reference correctness, split-block
//! bit-exact equivalence, no-alloc, edge lengths (0/1/non-power-of-two),
//! and silence/denormal-tail behavior. Merges happen on tests + review,
//! never on "sounds fine".
//!
//! Roadmap (one family per change, in dependency order):
//!   1. memory & move   ← [`mem`]
//!   2. vector x vector ← [`arith`]
//!   3. ramps & envelopes ← [`ramps`] + [`adsr`]
//!   4. LFOs & mod       ← [`lfo`]
//!   5. delay family    ← [`delay`] + [`reverb`] (the first effect built on it)
//!   6. filters         ← [`filters`] (one-pole, SVF, DC blocker,
//!      Butterworth cascade; tilt still pending)
//!   7. oscillators     ← [`osc`] + [`noise`] (mip-mapped wavetables,
//!      white/pink; polyBLEP deliberately skipped until a synth node
//!      wants sync or PWM, wavetable playback pending)
//!   8. nonlinearities  ← [`shaper`]
//!   9. dynamics        ← [`dynamics`]
//!  10. FFT/spectral    ← [`fft`] (real FFT, windows, frame cutter,
//!      overlap-add, magnitude/phase conversion)
//!
//! # The lane family
//!
//! Alongside the scalar kernels, the ones the poly synth needs have a
//! LANE-MAJOR twin that processes a whole voice group at once:
//! `LaneWhiteNoise`, `LanePinkNoise`, `LaneAdsr`, `LaneOnePole`,
//! `LaneSvf`, `LaneCascade`, `LaneOsc`, `LaneOversampler2x`, and
//! `Waveshaper::process_lanes` (a method, not a type — that curve is
//! stateless, so there is nothing per-lane to keep).
//!
//! They share one rule: what is per PATCH is shared, what is per VOICE is
//! an array. Coefficients, envelope times and waveshapes live once; phase,
//! integrator state, envelope stage and filter memory are `[f32; LANES]`.
//! Every one derives its shared half from the scalar kernel it twins —
//! coefficients are copied from a prepared instance, bands come from
//! `MipOsc::band` — so the pair cannot drift, and every lane is tested
//! BIT-IDENTICAL to the scalar result.
//!
//! On top of the usual five, each ships a sixth test: LANE INDEPENDENCE.
//! Drive one lane, silence the rest, and the rest must stay exactly zero.
//! It is the only test that catches the bug lane kernels actually have —
//! a piece of state left scalar, so all eight lanes move as one — because
//! every other test drives all lanes alike and passes regardless.
//!
//! Contract: `notes/20260825-poly-kernel-commission.md`.

#![deny(clippy::unwrap_used, clippy::expect_used)]

/// Voices processed as one group, for the lane-major kernels the poly
/// synth is built from (`LaneOsc`, `LaneAdsr`, `process_lanes`).
///
/// Eight f32 is one 256-bit `ymm`, which every x86-64-v3 machine has, so
/// the layout stays optimal on hardware without AVX-512; sixteen voices
/// is two groups. `.cargo/config.toml` pins `target-cpu=native`, so on a
/// machine with a 512-bit datapath the compiler is free to pair two
/// groups into one `zmm` where that wins.
///
/// This is ONE constant and the kernels are written as plain loops over
/// `[f32; LANES]` arrays rather than as hand-written intrinsics, so
/// retuning it is an edit here and a recompile — not a rewrite of the
/// kernels. See `notes/20260825-poly-kernel-commission.md`.
pub const LANES: usize = 8;

/// One frame of every lane: the inner element of a lane-major block.
///
/// A lane-major block is `[LaneFrame]` — OUTER index is time, INNER index
/// is voice — so one instant of the whole voice group is contiguous and
/// register-shaped. That is the entire reason for the layout.
pub type LaneFrame = [f32; LANES];

pub mod adsr;
pub mod arith;
pub mod delay;
pub mod dynamics;
pub mod fdn;
pub mod fft;
pub mod filters;
pub mod interp;
pub mod lfo;
pub mod lofi;
pub mod mem;
pub mod noise;
pub mod osc;
pub mod ramps;
pub mod reverb;
pub mod shaper;
