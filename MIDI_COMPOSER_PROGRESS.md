# MIDI composer delivery

Worktree: `/home/naiqvist/Work/daw/.claude/worktrees/midi-composer`.
Branch: `codex/midi-composer`.
Plan: `notes/20260909-midi-lab-plan-v2.md` (the separate local notebook).
Baseline: `5555414`, preserving main-next's working tree. Main-next is untouched.
Usage and declared capacities: [MIDI_COMPOSER.md](MIDI_COMPOSER.md).

## Delivered

- Universal 12-TET material with exact octaves, spelling, independent unisons,
  named polychord components, optional interpretation and functional harmony.
- Versioned deterministic composition, provenance, saved comparisons, legacy
  snapshots, scoped alternatives, independent locks and source/instance edits.
- Bounded voicing search with displayed costs, independent rhythmic cells,
  metre/grouping, ties, gate/swing/offsets and expression curves.
- Melodic contours, movements, target roles, ornaments, development and editable
  motif frames, transformations and placements.
- Six bass roles, style presets, actual chord/slash-bass arrivals, kick patterns,
  fills, approaches, melodic space and declared tuning constraints.
- Complete delayed canons, bounded species construction, final musical checks,
  source sections, references, form expansion and explicit tension mappings.
- Native subject inspector, searchable catalogues/palette, pitch chips, aligned
  timeline, editable notes/gates/curves, MIDI capture and checked clip lifting.
- Long patterns and compiled events, stable destinations, separate unison tracks,
  atomic Send/Undo, explicit conversion diffs, tempo-aware audition and MIDI export.

## Verification

- Final library suite: **3,184 passed, 3 ignored, 0 failed**. It includes all 4,095
  nonempty pitch-class sets, exact/legacy sound preservation, snapshot cache
  identity, small exhaustive voicing reference, bass and species fixtures,
  local/source lock precedence, long-pattern compilation, Send/Undo and offline
  audio, as well as the existing sequencer and realtime regression suite.
- Host integration suites: **82 passed**, covering instrument/document/graph
  round-trips and session pointer/keyboard behavior.
- Real pointer tests cover numeric drag accumulation, rhythm release editing,
  note body/release separation and curve handles crossing neighboring points.
- All application binaries and the fixture example build. The shared shell's
  existing default-binary import issue is resolved by exposing daw::ui at the
  binary root. Dependencies and realtime callback algorithms are unchanged.
- Native screenshots reviewed at 1280×800 and 720×480, including a twelve-note
  unnamed collection in the light theme, melody and expanded form. The compact
  layout reserves space for the piano roll beneath the five voice lanes.
- Nine authored fixtures render to RON, MIDI and finite stereo WAV files using
  the real destination instrument: chord leading; three melodic developments;
  foundation, walking, riff, pedal and sub bass with harmony. Generate them with
  `cargo run --example midi_composer_fixtures -- /tmp/midi-fixtures`.

The recovered sequencing and device UI contracts were read before host/editor
changes and remain in this worktree's local notes directory. Search limits and
unsupported inputs produce explicit results; no global-optimality or listening
sign-off is claimed. The fixture bank is material for the musician's listening
review, separate from automated correctness checks.
