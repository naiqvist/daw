# Lagoon: reusable workflow improvements

Implemented on the user's pushed `master` commit `68d109b`. No new DSP kernel,
instrument preset, project import shortcut, dependency, or audio callback work.

## What the drive exposed

The eight-track score was musically editable, but entering related changes took
too many separate sentences. Note gates forced repeated phrase rewrites; the
same ornament/FX moment required several lock calls; pad breathing repeated
identical sweep windows; drills and their filtering were separate edits.
`go bar` correctly moved the edit cursor, but there was no equivalent explicit
playhead command for jumping between audition passages.

The fixes are generic native palette operations:

| Repeated work | Reusable operation | Other uses |
| --- | --- | --- |
| Rewriting notes for differing durations | `rhythm`/`group`/`ratchet` gate cycles and ramps | Legato bass, articulated bells, open/closed percussion gates |
| Separate locks for one sound gesture | `lock target=value;target=value at ticks` | Drum articulations, ornament settings, simultaneous FX sends |
| Repainting several controls over repeated windows | Multi-target `sweep … every ticks` | Breathing pads, bass vowel/filter motion, recurring transition effects |
| Creating a drill then selecting/filtering it | `ratchet … sweep target=A:B;target=A:B` | Opening/closing fills, pitched bursts, delay/room rises |
| Waiting through the song to reach an audition | `seek bar`, `seek tick`, `seek cursor` | Patch comparisons, checking transitions, tracker navigation while rolling |

All editing gestures are one undo step, including any enabled console sections.
All parameters validate before commit. Bad late targets, aliases duplicating a
parameter, invalid values, unaligned windows, overlapping/overrunning repeats,
and unsupported live-lock targets are refused without partial edits. The output
is the existing editable note/lock representation, not new playback state.

See [phrase command reference](phrase-operations.md) for syntax,
time units, selection rules, limits, and exact endpoint behaviour. Units matter:
`rhythm 16` positions/gates count sixteenths; lock/sweep windows count ticks.

## Measurement and exactness

The original recipe and shortened recipe are executed from the **same empty
scaffold** through `Stage::apply_timeline_command`. The retest requires equality
of the entire resulting `Song`, not only note counts or approximate pitches.
This covers all notes, gates, microtiming, locks/slides, instrument settings,
sample references, routing, arrangement, and export configuration.

| Composition input | Original | Improved | Reduction |
| --- | ---: | ---: | ---: |
| Native palette sentences | 914 | 391 | 57.2% |
| Characters inside those sentences | 46,772 | 33,680 | 28.0% |

The improved end-to-end TAKE also uses four navigation/seek sentences around
audition and export, for **395 palette openings in total**. It contains 1,225
driver events and no fixed-frame waits. Saving commands has not made the musical
description disappear: character counts are reported to expose the remaining
literal score payload.

The equal scaffold contains eight instrument assignments, track/clip names,
64 empty clip identities and placements, section locators, and sample/routing
setup. These are one-time setup costs, excluded from both recurring scores.
The loaded Spectral patch is a 6,908-byte payload with seven FX nodes, nine FX
routes, six modulation sources and six modulation routes. Calling its load one
sentence does **not** make designing that patch one action. Authoring tool calls
and cognitive effort were not captured well enough for a numerical claim.

`examples/compact_phrase_take.rs` is an auditable verification/recipe-authoring
harness. It conservatively rewrites existing sentences into the ordinary new
syntax and refuses to emit if whole-Song equality fails. It cannot silently
quantize microtonal notes, merge chords to single notes, or alter locks to meet
a count. The app receives and executes every resulting sentence through its
normal palette in the native TAKE; it does not load the finished preflight song.

Reproduce with fresh output paths (existing directories are never overwritten):

```sh
cargo run --example compact_phrase_take -- \
  /home/naiqvist/Music/daw/lagoon-of-broken-glass-v1/empty-template.stage.ron \
  /home/naiqvist/Music/daw/lagoon-of-broken-glass-v1/preflight-commands.txt \
  /tmp/lagoon-fresh-retest
cargo build --features visuals --bin stage
python tools/run-take.py /tmp/lagoon-fresh-retest/compact.drive \
  --project /tmp/lagoon-fresh-retest/Replay.stage.ron --timeout 1200
```

The TAKE starts from the empty scaffold, enters the complete score, saves it,
auditions opening/groove/peak/breakdown passages, exercises tracker hold/follow,
captures diagnostics, exports the full song, and leaves playback at the top.
Seeks intentionally do not reconstruct previous reverb/delay history, so they
are quick checks, not substitutes for the full continuous offline render.

## Verification status

- Whole-Song equivalence: passed, 914 → 391 composition sentences.
- Full library regression suite: 3,386 passed, zero failures, four ignored.
  Includes undo, atomic refusal, parameter validation, meter-aware seeking,
  fractional-playhead realignment, and tracker performance regressions.
- Native build with `visuals`: passed (visual playback remains off).
- Native TAKE and audio comparison: pending.

## Boundaries and next opportunities

This is the first measured workflow pass, not every feature in the original
brief. There is no new chord-specific button, automatic composition judgement,
agent loop embedded in the app, generic Indian-ornament engine, or expanded
sample vocabulary. Visuals remain absent/off. The composition's sampler uses
the existing conga-loop slices; it is not a complete woody/break/shaker atlas.

The next useful work is better selection/addressing shared by notes and FX,
named reusable gestures with inspectable parameters, and a first-class
audition → bounce → analyse → human-acceptance workflow. Those are future work,
not features implied by this retest. Machine equality and clean audio metrics
do not establish that the music passes the human listening test.
