# daw — agent instructions

Realtime DAW in Rust (edition 2024, rustc 1.98). egui/wgpu UI over a custom
audio engine on rtaudio/JACK. Full context: `notes/20260823-status.md`.

## Commands

    cargo build            # debug builds are DSP-usable (opt-level profiles set)
    cargo run
    cargo clippy
    cargo test
    cargo fmt

After ANY dependency change, verify JACK was actually compiled into rtaudio:

    nm -C target/debug/build/rtaudio-sys-*/out/build/librtaudio.a | grep -c RtApiJack
    # must be 24, not 0 — silent absence = DAW runs through Pulse compat layer

## The one rule that outranks everything else

**The audio callback (red zone) never:** allocates, takes locks, makes
syscalls, logs, panics/unwrap/expect, or runs unbounded-time paths.

- Audio thread comms: `rtrb` ring buffer (UI→audio commands) and
  `triple_buffer` (audio→UI state) ONLY.
- `parking_lot` / `crossbeam-channel` / serde / anything heap-hungry:
  green zone (UI/control) only.
- Drops of audio-thread data go through `basedrop`, never inline.
- Audio modules get `#![deny(clippy::unwrap_used, clippy::expect_used)]`
  and run under `assert_no_alloc` in debug builds.

## Escape hatch

If a change seems to require violating a red-zone rule, STOP and explain.
Do not write the unsafe version "temporarily".

## Before writing DSP kernels (src/dsp/)

Read `notes/20260823-dsp-kernel-contract.md` FIRST — it is the full contract:
no alloc/locks/syscalls/panics/unbounded loops/deps, prepare/reset/process
split, scratch via caller slices, latency reported, and the five mandatory
tests per kernel (reference, split-block equivalence, no-alloc, edge lengths,
denormal tail). Kernels touch ONLY src/dsp/. Wiring into nodes is separate
work.

## Before writing sequencer / MIDI / clip / automation code

Read `notes/20260823-sequencing-contract.md` FIRST. Five rules: sample-stamped
events, discontinuity => all-sound-off (ctx.discontinuity exists for this),
note-off before note-on on ties, sequences ride compiled immutable chunks
(never streamed from UI), every node classified free-running vs
timeline-locked.

## Project facts worth not re-deriving

- The empty `[workspace]` stanza in Cargo.toml blocks cargo's upward walk to
  an invalid manifest at `~/Projects/Cargo.toml`. Do not remove it.
- `.cargo/config.toml` sets `target-cpu=native` — SIMD/AVX assumptions rely on
  it. Do not remove it.
- `[profile.dev] opt-level = 1` + deps at opt-level 3 keeps debug builds
  xrun-free. Do not remove them.
- Backend is rtaudio 0.8.0 with `features = ["jack_linux"]` (NOT cpal — no
  duplex release). cpal/jack-crate decisions live in the status note.
- No audio-graph crate exists that fits; we write our own. Graph/buffer
  ownership model is an open human decision — do not silently pick one.
- CLAP-only via clack-host. No VST3 planned.
