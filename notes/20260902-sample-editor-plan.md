# The cutting room — sample editor plan, 2026-09-02

A full-screen sample editor for the stage, in the spirit of the
Octatrack's audio editor: the waveform is the hero, the pages change what
the hands reach, and every act is a key. Keyboard only, like the rest of
the stage. Drawn in the deck's own hand: a chamfered screen, codex signs,
the live ink for what sounds.

## What exists, and is kept

- `audio::sampler` plays a resident `Material`: classic / one-shot /
  slice modes, start + end + loop-start as FRACTIONS of the file, reverse,
  fades, up to 64 slices from `SLICE_BASE_NOTE` upward, per-voice filter,
  drive, downsampler, plocks.
- `slice::grid` and `slice::transients` cut a file into equal or
  onset-detected slices, green side.
- `material::load_cached` reads a WAV, resamples to the device rate, and
  keeps it resident; `AuditionBuffer::from_material` plays one through
  the engine's audition path.
- The sampler's parameter table (`params::sampler`): START, END,
  LOOP_START, MODE, LOOP_MODE, REVERSE, GAIN, TUNE, ROOT, SLICES, …

## What is missing

1. Nothing in the Song stores slices. The compiler comments say so.
2. The stage cannot show a sample: it never sees audio, by rule.
3. No surface edits trim, loop, or slices by eye. No zoom. No audition.

## Design

### Model

- `Device.slices: Vec<f64>` — slice starts as FRACTIONS of the file,
  sorted, `serde(default)`. Fractions, like START/END/LOOP_START already
  are, so a document does not depend on the rate the device happened to
  open at.
- `NodeSpec::Sampler.slices` becomes `Vec<f64>` fractions too, converted
  to frames at compile against the loaded material's length. No project
  has ever carried a non-empty table, so the type change reads old files
  unchanged.
- Trim, loop, reverse, mode, loop mode, gain, root, tune: the device's
  existing overrides. Normalize is a GAIN override computed from the
  file's peak; non-destructive, undoable, and audible on every note.

### The seam between host and stage

The stage never imports `crate::audio`. So:

- `sample_peaks` (crate root, green, no audio types): a pyramid of
  min / max / RMS bins over the file, four-to-one per level, plus the
  peak magnitude. Built from plain slices of `f32`.
- `stage::SampleData { path, samples: Arc<Vec<f32>>, channels, frames,
  rate, peaks }` — the host builds it from a `Material` and hands it in;
  the stage keeps it while the editor is open. The `Arc` is the
  material's own, so nothing is copied.
- `Stage::wanted_sample()` names the path the editor needs; the host
  loads it (cached) and calls `Stage::set_sample`.
- `Stage::take_audition()` hands the host a `(path, from, to)` range to
  play through the engine's audition path; `Stage::stop_audition()`.

### The editor

State: which device, which page, cursor (fraction), view (start and
span, fractions), selected slice, transient sensitivity, snap to zero
crossing, slice count for the grid.

Pages, as tabs across the top, Tab to cycle:

- **TRIM** — start, end, loop start. S / E / L place them at the cursor.
- **SLICE** — Enter adds a slice at the cursor, Delete removes the
  nearest, G lays a grid of N, T detects transients at the sensitivity,
  C clears. + / − change N (grid) or sensitivity (transients).
- **ATTR** — N normalize, R reverse, M mode, Q loop mode; the page shows
  gain, tune, root, and the file's facts.

Everywhere: Left / Right move the cursor a 64th of the view, Shift for
an eighth; ^Left / ^Right jump between markers; Up / Down zoom about the
cursor; PageUp / PageDown scroll; Z toggles zero-crossing snap; P
auditions the slice under the cursor (or the trim region), Shift+P the
whole file; Escape leaves. Space, Home, ?, ^L stay global.

Entry: Enter on a sampler in the chain band, or ^E anywhere the cursor
addresses a track whose head is a sampler. A track with no sampler, or a
sampler with no file, refuses.

### The picture

Full field, like the codebook. A header with the file's name, length,
rate, and channels beside the page tabs. The waveform in a dark screen
recess with the house chamfer: per column the RMS band in the ink at
full strength, the min/max envelope one step dimmer, the extreme
outliers dimmer still — three tones, so density reads as brightness.
Markers: start and end as brackets, loop start dashed, slices as
hairlines with their number at the top, the cursor bold with a pad, the
audition head in the live ink while playing. Under the wave a minimap of
the whole file with the view's window drawn on it. At the foot the
page's parameters as a row of readouts, and on the SLICE page a list of
slices with positions down the right.

### Phases

1. Model and compile: slices on the device, fractions in the node spec,
   conversion at compile, tests.
2. `sample_peaks`: the pyramid, a query by view, the peak. Tests.
3. Stage state, intents, keymap scope, host seam. Tests through keys.
4. Drawing, and a shot pose with a synthesized file.
5. Host wiring: load on demand, audition.
6. Sequencer: the slice is a parameter (`SLICE`), locked per trig from
   the trig menu's slice row under a strip of the file — the note stays
   a pitch, as on the Octatrack. A cell on a slicing track wears the
   locked cut as a tag. Done.

Not in this pass: timestretch, pitch-shift retrigs, recording into the
editor, destructive edits. Each is its own note.
