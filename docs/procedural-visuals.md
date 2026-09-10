# Procedural visual scores — first implementation

Build the native Stage with `cargo build --features visuals --bin stage`.
The default/audio-only build has no visual renderer, compiler, exporter, or
per-frame visual hook. It preserves an opaque `Song.visuals` payload on save.
This is optional music-video authoring, not a header animation or audio effect.

## Runtime contract

- Starts Off; opening a project never starts visual rendering automatically.
- `visual on` compiles a validated score and opens a GPU preview. While rolling,
  it reads the host's fractional musical clock. When parked in Song view, it
  scrubs at the arrangement cursor (`go bar 41`, for example).
- `visual off` drops the preview and requests cancellation of any export. It
  reports STOPPING until the worker exits, then OFF. There are no visual
  evaluations, GPU submissions, audio taps, timers, or workers while Off.
  In-flight preview handles retire with the last submitted UI frame. A tiny
  inactive-state branch remains in visual-enabled builds; this is not a claim
  that the entire DAW process uses 0% CPU. The default build removes that hook.
- `visual lock` protects score edits, not playback. Unlock explicitly to edit.
  Lock is saved; runtime On/Off is not. Undo/document replacement turns preview
  Off and cancels the old snapshot's export. Visual-only edits and undo do not
  increment the audio graph revision.
- Every frame is a pure function of absolute musical time and a fixed seed.
  Skipped frames, playback direction, and seeks cannot alter the result.
  Same-GPU re-renders are tested; bit-identical output across GPU vendors is
  not promised. No feedback simulation or frame-history accumulator is hidden
  in a primitive.

Audio callback code does not reference this subsystem. Offline export owns its
own immutable Song snapshot and uses the native song graph/tempo-aware bounce.

## Score and commands

Open the palette with Ctrl+Shift+P. Times are integer ticks: 48 per quarter note.
Clip keys are local; placements are on the song timeline. Placement intervals
are half-open. Overlap clips to crossfade their opacity lanes. Repeating clips
restart their keys and modulators at each loop boundary.

```text
visual new
visual clip pulse 192
visual layer pulse halo rings
visual set pulse halo hue=0.55; scale=0.9; softness=0.05
visual key pulse halo brightness 0 1
visual key pulse halo brightness 12 0.25
visual ramp pulse halo rotation 0:191 0:360
visual pulse pulse halo scale 48 8 0.15 0
visual lfo pulse halo hue 0.0625 0.08 0
visual random pulse halo x 24 0.1 42
visual place pulse 0 1536 repeat
visual on
visual lock
visual inspect
visual off
```

Primitives: `disc`, `rings`, `ribbon`, `field`, `noise`.
Layer operations: transform → warp → primitive → colour → blend.
Blend modes: `over`, `add`, `multiply`, `screen`.

Every layer exposes the same addressable parameter bank: `x`, `y`, `scale`,
`rotation` (degrees), `hue` (cycles), `saturation`, `brightness`, `opacity`,
`frequency`, `warp`, `softness`, `phase`. `visual params` lists bounds.
The preview's expandable inspector lists clips, placements, layers, parameter
values, lock counts, and modulation routes from the canonical compiled score.

```text
visual set CLIP LAYER name=value; name=value
visual blend CLIP LAYER MODE
visual key CLIP LAYER PARAM TICK VALUE [slide]
visual ramp CLIP LAYER PARAM START:END FROM:TO
visual lfo CLIP LAYER PARAM CYCLES_PER_BEAT DEPTH PHASE_CYCLES
visual pulse CLIP LAYER PARAM PERIOD_TICKS DECAY_TICKS DEPTH PHASE_TICKS
visual random CLIP LAYER PARAM STEP_TICKS DEPTH SEED
visual unmod CLIP LAYER INDEX
visual place CLIP START_TICK LENGTH_TICKS once|repeat
visual unplace INDEX
visual seed INTEGER
visual background R G B
visual load FILE.visual.ron
visual save NEW-FILE.visual.ron
visual unlock
visual clear
```

Indices are 1-based. A key replaces an existing key at its timestamp; a ramp
inserts two keys without deleting intermediate ones. `slide` interpolates from
that key to the next; otherwise the value holds. Before the first key, the base
parameter applies. Modulators add after the locks, then clamp to parameter
bounds. Commands validate a candidate and commit atomically as one undo step.
Saving the Stage project embeds the entire score. Standalone save refuses to
overwrite; load refuses files above 4 MiB and invalid graphs/values.

## Video export

```text
visual export 1280 720 30 /path/to/new-music-video.mp4
visual cancel
```

Requires system FFmpeg with libx264/AAC, plus a working wgpu adapter. Output is
H.264, yuv420p, CRF 18, AAC 256 kb/s stereo at 48 kHz. The render ends at the later
of the song and visual arrangement ends; extend a visual placement for a tail.
Frame `n` maps directly to sample `floor(n * 48000 / fps)`, then through the
same tempo map as audio. No accumulated floating-point frame clock.

Publication uses an atomic no-clobber hard link on the destination filesystem.
Cancellation kills only the owned encoder, reaps the worker, and removes only
its unique temporary files. No partial output is published. GPU readback has a
timeout. Export progress stays visible without running a preview.

Current deliberate budgets: 16 simultaneously active layers, 256 clips,
4096 placements, 32768 keys, 64 modulator routes per layer; even dimensions
16–3840, 24/25/30/60 fps, and up to one hour. Invalid scores are refused, never
silently truncated. This is layered 2D v1, not yet an arbitrary visual routing
graph, a draggable visual timeline, feedback/particle simulation, or baked
video playback inside the DAW.

## Demonstration and verification

`examples/visual_score.rs` authors Skin / Coil / Sky as three overlapping clips:
rhythmic rings, accelerating ribbons, then slow fields and counter-lights.
It generates an editable native project without changing the audio Song, and
can render six diagnostic frames with a same-time pixel repeatability check.

Run unit and real-GPU tests:

```sh
cargo check --no-default-features --all-targets
cargo test --no-default-features --lib visual
cargo test --features visuals --lib visual -- --include-ignored
```

Native TAKE recipes are in `tools/takes/`. They exercise palette editing,
locking, saving, cursor scrubbing, playback across Off, cancellation, and full
audio/video export. The first completed TAKE used 69 commands, zero fixed waits
and zero mid-run interventions, finishing in 98.08 seconds. Its output had 5100
frames at 1280×720/30 fps; audio and video both measured 170.000 seconds. Decoded
AAC: peak −4.70 dBFS, RMS −17.03 dBFS, zero clipped samples. Screenshot review
then found and corrected native BGRA capture order and preview gamma handling;
the latter has an explicit GPU comparison against the sRGB exporter.

Future improvements should remain reusable: visual routing graphs and reusable
subgraphs, shared modulator definitions, note-to-visual event extraction, a
visual arrangement editor, and optional baked playback. Keep the agentic
prompt → audition → analyse → human acceptance loop as a future app feature.
