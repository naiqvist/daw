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
//!   3. ramps & envelopes ← [`ramps`] (ADSR still pending)
//!   4. LFOs & mod
//!   5. delay family    ← [`reverb`] (the first effect built on it)
//!   6. filters         ← [`filters`] (one-pole, SVF, DC blocker,
//!      Butterworth cascade; tilt still pending)
//!   7. oscillators     ← [`osc`] (mip-mapped wavetables; noise,
//!      polyBLEP and wavetable playback pending)
//!   8. nonlinearities
//!   9. dynamics
//!  10. FFT/spectral

#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod arith;
pub mod filters;
pub mod mem;
pub mod osc;
pub mod ramps;
pub mod reverb;
