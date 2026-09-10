# Tracker / Meter

The `meter` worktree is integrated with the composition tools, Spectral synth,
and optional procedural visuals. Open the full-screen tracker with
**Ctrl+Shift+R** or the palette command `meter`. `meter 7/8` still edits the
song's time signature; bare `meter` opens the tracker.

The body shows tracks side by side against one musical clock, with a fixed
now-line. Each track's own clip length folds against song time. The left panel
keeps a bounded history of answered Stage intents, with repeated verbs grouped.

This is an editable tracker, not just a meter:

- Arrows move through steps and fields; Shift+arrows extend a rectangular block.
- Page Up/Down moves a bar. Enter returns from a held position to live follow.
- `-` / `=` turn values; shifted turns are coarse.
- A–G enters named notes; Shift+A–G adds chord tones.
- Hex digits edit velocity. `T` toggles the trig; `S` toggles a lock slide.
- Delete clears the addressed field. Comma/period selects a parameter in ADD,
  allowing a parameter's first lock column to be created.
- Probability, conditional trigs, retrigger counts/rates, and sound-lock release
  have dedicated field semantics. Block edits form a single undo step.
- Escape clears a block first, then leaves the tracker. `go bar …`, `go track …`,
  and `go clip …` leave tracker mode and navigate to their stated destination.

The tracker reads the same saved patterns and device parameter tables as the
other editors. A repeating pattern is edited at its source, so every repetition
changes together. No duplicate instrument state or tracker-specific playback
engine was added. It is available in audio-only builds too.

Development entry points: `stage --meter [PROJECT]` and `stage --meter-demo`.
The core has 28 tracker-specific tests; the combined regression TAKE is
`tools/takes/take-merged-tracker-visuals.drive`.

## Redraw cost and regression checks

The draw path derives one frame-local plan and one set of rows. The body,
headings, ADD selector, and readout share them. Track starts and widths are
indexed; lock-column discovery deduplicates with a set and computes sort keys
once. ADD edits also reuse their command's plan.

Horizontal visibility is decided before cells are derived. Pattern references
are resolved once per visible track; placement lengths and device identities
are indexed once per row batch, and parameter labels once per distinct address
kind. None of this changes the audio callback or persists a second copy of the
musical state.

Text geometry uses egui's placeholder colour, with the exact fade colour
applied at painting time. This keeps layout-cache keys stable without changing
the smooth fades. The palette lock is no longer read once per cell.

Four deterministic tests protect one plan/row batch per draw, horizontal
culling plus fresh edits, column offsets/ADD agreement, and shared text geometry
with distinct unquantized paint colours. The manual benchmark includes tracker
projection, text layout, and CPU tessellation at 1920×1080, with 15 warmup and
60 measured frames. It excludes audio processing, GPU submission, and display
vsync; its milliseconds are not a measured whole-app frame rate.

Same-machine sequential before/after measurements, optimized-dev profile:

| Workload | Before median / p95 | After median / p95 | Median speedup |
| --- | --- | --- | --- |
| 12 tracks, 128 steps each, alternating notes, 16 locks/note | 15.891 / 19.271 ms | 2.263 / 3.455 ms | 7.0× |
| Skin / Coil / Sky, one dense Spectral arrangement | 402.263 / 435.342 ms | 6.721 / 8.821 ms | 59.9× |

Last-frame plan/row-batch/cell derivations fell from `66/2/768` to `1/1/126`
for the dense fixture, and `45/2/64` to `1/1/63` for Spectral. The latter keeps
the same visible track and removes the separately derived readout row.
The baseline executable was captured before the performance edits; both
versions use the identical fixture and timing loop, with no concurrent builds
during this final comparison. Full post-fix library regression: 3387 passed,
5 ignored (the manual benchmark is one of these).

```sh
cargo test --features visuals --lib meter_draw_benchmark -- --ignored --nocapture
DAW_METER_BENCH_PROJECT='/path/to/project.stage.ron' cargo test --features visuals --lib meter_draw_benchmark -- --ignored --nocapture
```

Use the same profile and project on both sides of a comparison. These fixes
target the normal optimized-dev build; they do not depend on switching to a
release build to hide repeated work. `take-merged-tracker-playing.drive` checks
native Song playback, tracker navigation/follow, return to arrangement, and Off.

Final native performance TAKE (`take-tracker-performance.drive`) completed
28 commands in 5.41 seconds, including live playback through bar 4, held cursor
navigation, follow, arrangement navigation, stopping, and reopening the tracker.
The screenshots preserve the expected colours and dense lock columns. The app
was left open with the tracker stopped. One earlier attempt refused the initial
`go bar 1` palette command before entering the tracker; the unchanged script
passed on retry. That startup-input issue was not diagnosed or masked with
fixed sleeps. Both final all-target build configurations and all 12 visual/GPU
regressions passed; the three Python tool tests passed as well.
