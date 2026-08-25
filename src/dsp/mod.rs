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

#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod adsr;
pub mod arith;
pub mod delay;
pub mod dynamics;
pub mod fft;
pub mod filters;
pub mod lfo;
pub mod mem;
pub mod noise;
pub mod osc;
pub mod ramps;
pub mod reverb;
pub mod shaper;
