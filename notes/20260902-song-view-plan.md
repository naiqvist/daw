# The song view — plan, 2026-09-02

A second projection of the same song: time across, tracks down, the
clips laid where they play. Ableton's arrangement in its bones — free
placement, a loop brace, record the session into it, export it — with
an Elektron's hands: a grid of bars, patterns as the unit, every act a
key, and a row of the song readable as "which pattern on which track".
Tab turns the field from the session to the song and back.

## What exists, and is kept

- `Track.blocks: Vec<PatternBlock { pattern_id, start_tick, length_ticks }>`
  and `Track.audio_blocks: Vec<AudioBlock { source, start_tick,
  length_ticks, loop_brace }>` — the arrangement, already in the model,
  in song ticks. `Track::end_tick`, `blocks_in_time_order`, and the
  landing refusals ("BLOCK IN THE WAY") exist.
- `Song.tempo` and `Song.meter` marks, `bpm_at`, `meter_at`.
- `audio::bounce_automated(spec, opts, path, letters, progress)`: an
  offline render of any compiled graph at a tempo, with a progress
  callback.
- `NodeSpec::AudioClip`: a streamed audio block in the graph.
- The stage's transport, the session's `playing` table, and the
  sequencer tray that edits whatever pattern the cursor addresses.

## What is missing

1. A compiler for the arrangement: `song_graph::build` reads the
   session's `playing` table only.
2. A transport that runs the song rather than looping a scene.
3. The view, its cursor, and its verbs.
4. Recording the session into the arrangement.
5. A way out: export.

## Design

### Two projections, one field, one key

Tab switches the field between SESSION and SONG. Nothing else moves: the
heads stay across the top, the tray stays below, the strips stay. The
vitals strip says which is up, and the transport readout says which is
PLAYING — the two can differ, as in Ableton, and the readout is where
that is seen.

### The picture

- **Heads** across the top as in the session: the same casings, the same
  numbers, so a track is the same object in both views.
- **The ruler** under the heads: bars, numbered, with the meter's beats
  as ticks, drawn by the sequencer's own ruler rule so the two agree.
  Tempo marks and meter marks stand on the ruler as small plaques.
- **Lanes**, one per track, in the deck's lattice: bar lines as
  hairlines, beats as vias, the same ground as the session's board.
- **Pattern blocks** as chamfered casings the length they play, named
  by the pattern's number and sign, carrying a mini strip of the
  pattern's trigs (the head's miniature, laid along time) so a block
  reads as rhythm before it reads as a label. A block longer than its
  pattern repeats the strip; the seams are marked.
- **Audio blocks** as casings with the file's peaks in the three-tone
  envelope the cutting room draws, the loop brace as brackets inside.
- **The playhead**: a heavy rule in the live ink from ruler to foot,
  pulsing on the beat as the session's dashes do.
- **The loop brace**: brackets on the ruler between two locators, and a
  faint wash over the lanes inside it.
- **The cursor**: the cursor's brackets on a (track, bar) cell, or on a
  block when one stands there. Exactly one focus-bright thing.
- **Zoom**: bars per screen. At the coarsest the whole song fits; at the
  finest a bar spans the field and beats become the grid.

### The cursor and the grid

The field is a lattice of tracks × grid cells, the grid being a bar or,
zoomed in, a beat. Left and Right walk cells; Up and Down walk tracks.
Landing on a block selects it. Home is the song's start. ^Left and
^Right jump between block edges and locators.

### Verbs, all keys, the grammar's own words where they exist

| Key | Act |
|---|---|
| Enter on an empty cell | Place: the track's LAST-PLACED pattern, else the pattern its session cursor holds, one pattern-length long |
| Enter on a block | Open: the tray shows that pattern, as in the session |
| P | Pick: the pattern picker for this cell (a small list callout, the trig menu's shape) |
| Delete | Remove the block |
| D | Duplicate the block after itself |
| W then Left/Right | Nudge the block by a grid cell |
| ^R then Left/Right | Resize the block by a cell (holds, like the clip resize) |
| Shift+Left/Right | Resize by a bar |
| Q / E | Yank / put a block |
| [ / ] | Set the loop brace's start / end at the cursor |
| L | Toggle the loop brace |
| M | Drop a marker at the cursor; M on a marker names it |
| Up/Down with Shift | Zoom |
| Space | Roll the song from the playhead; Space again stops |
| Home | Playhead to the song's start |

Nothing here shadows the sequencer: the tray keeps its keys while the
cursor is inside it, exactly as in the session.

### Playing the song

The transport gets a mode: SCENE or SONG. Space in the song view sets
SONG and rolls from the playhead; Space in the session sets SCENE and
rolls the scene. The readout shows the mode as a word beside the
position. In SONG mode a launched session clip does what Ableton does:
it takes its track over until the arrangement is resumed (a key, "back
to song", on the head), and the vitals say so.

The song compiler: for each track, every pattern block becomes the
pattern's notes offset by the block's start and cut at its end,
repeated to fill it, in one timeline-locked node per track — the same
node the session uses, with `loop_len_beats = None`. Audio blocks
become `AudioClip` nodes. Locks, slices, effects: unchanged, they are
the pattern's.

### Recording the session into the song

^Space arms the arrangement. While it is armed and the scene rolls,
every launch writes a block at the playhead — the pattern that fired,
from the moment it fired until the moment something else fires on that
track or the transport stops — and the loop brace, if set, is what the
song plays on the way back. Stop writes the tails. The song view shows
the blocks arriving in the live ink as they are written. Undo takes a
recording away as one edit.

### Export

^B bounces the song — the loop brace when it is set, the whole song
when not — to `Music/daw/renders/<song>-<date>.wav` through
`bounce_automated`, offline, at the song's tempo. Progress is a gauge in
the message strip; Escape abandons. The stage asks, the host renders:
`Stage::take_export()` names the range and the path, the host runs the
bounce on a thread and reports progress back. Tempo changes: the first
pass renders at the tempo at the brace's start; the tempo map's ramp is
a later note.

### Model additions

- `Song.locators: Vec<Locator { tick, name }>` and `Song.loop_brace:
  Option<(usize, usize)>`, `serde(default)`.
- `Track.last_placed: Option<PatternId>` — not serialized; a UI memory.
- Intents on the arrangement: place, remove, nudge, resize, duplicate,
  yank/put, brace, locator — through the same applier the session's
  edits use, so undo covers them.

## Phases

1. **Compile and roll.** `song_graph::build_song`: blocks to nodes; the
   transport's SONG mode; Space rolls the arrangement from the playhead.
   Tests: a block placed at bar three sounds at bar three; a block
   longer than its pattern repeats it; audio blocks stream.
2. **The view.** Tab; heads, ruler, lanes, blocks, playhead, cursor;
   zoom. Shot poses for empty, placed, and zoomed.
3. **Editing.** Place, open, pick, remove, duplicate, nudge, resize,
   yank/put; the tray follows the block under the cursor.
4. **Brace and locators.** Loop, markers, jumps, the ruler's plaques.
5. **Export.** The host seam, the offline bounce, the progress gauge.
6. **Record.** Arm, write on launch, tails on stop, one undo.
7. **Audio blocks.** Place from the browser, peaks in the casing, the
   loop brace inside a block.

Later, each its own note: automation lanes on the timeline, tempo map
editing on the ruler, time-stretch of audio blocks, a song ROW list
(Elektron's song mode as a table) as a third projection of the same
arrangement.

## As built, 2026-09-02

Heads on the LEFT as the rows' labels (the user's call): tracks are
rows, time runs across, the view pages horizontally after the cursor
and the heads column scrolls with the cursor's row when tracks
overflow. Modules: `src/ui/stage/arrangement.rs` (state, geometry,
verbs, drawing), `song_graph::build_song` (the compiler),
`transport::Mode` (SCENE / SONG), host seam for export in
`src/bin/stage.rs` (`serve_export`).

Keys, as bound (the table above was the plan; these are the facts):

| Key | Act |
|---|---|
| Tab | Session ↔ song |
| ← → ↑ ↓ | Cell / track; the grid is a bar, or a beat at ≤ 4 bars across |
| Shift ↑ / ↓ | Zoom in / out (2 … 64 bars across) |
| ^← / ^→ | Jump to the previous / next edge: block ends, locators, the brace |
| Enter | Place the track's last pattern on an empty cell (cut to the gap); open a block in the tray |
| Delete | Remove the block (pattern or audio) |
| D | Duplicate after itself |
| W then ← → | Nudge by a cell (holds until Escape) |
| ^R then ← → | Resize by a cell (holds until Escape) |
| Shift ← / → | A bar shorter / longer |
| Q / E | Yank / put |
| P / Shift+P | Next / previous pattern the track holds in the session |
| [ / ] | Brace start / end at the cursor (end includes the cursor's cell) |
| L | Brace on / off, kept |
| M | Locator at the cursor, or lift the one there |
| Space | Roll the SONG from the playhead (mode SONG until Space in the session) |
| ^Space | Arm: launches in the session write takes; stop lands them, one undo |
| ^X | Export the brace when on, else the whole song, to `Music/daw/renders/<song>-<stamp>.wav`; Escape abandons |
| Browser Enter on a sample, audio track | An audio block at the cursor, as long as the file at the tempo there |

Export moved from ^B to ^X: ^B is the browser everywhere, and a
render is a verb that should not shadow a window.

Takes OVERWRITE the pattern blocks they land on (an arrangement
recording is the later word); they never overwrite audio blocks —
those are counted and reported in the strip.

Not yet: locator names (M1, M2 … are minted), plaques for tempo and
meter marks on the ruler, peaks inside audio blocks, a take-over of a
track by a session launch while the song plays, the tempo map's ramp
in an export.
