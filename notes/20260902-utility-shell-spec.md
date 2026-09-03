# Stage utility shell

## Purpose

The stage needs the unglamorous half of being a dependable DAW: getting into
the right project, keeping it safe, configuring the machine, rendering a
deliverable, and explaining the machine when something is wrong. These are
one system because they all sit between the musical document and the host.

This work belongs to the `stage` binary. It does not alter DSP and it does not
put machine-local choices into a song.

## Design law

The utility shell is a cyberpunk terminal built from the stage's existing
alphabet, circuit traces, dark display material, and global vector cursor. It
is not a collection of stock desktop dialogs.

- Keyboard first: arrows move, Enter acts, Escape backs out, printable input
  edits the active field. Mouse hit targets mirror every keyboard target.
- One modal authority: while a utility surface is open no key leaks into the
  song, transport, browser, palette, or device rack.
- One cursor: utility rows claim the same animated global cursor as the rest
  of the app, at a higher compositional layer.
- Recoverable by default: opening or replacing a dirty song requires an
  explicit Save / Discard / Cancel decision. Missing recents remain visible
  until deliberately forgotten.
- Honest controls only: a setting is offered only when the host can apply it.
  Requested audio settings and the negotiated running stream are shown
  separately.
- Green-zone work only: filesystem access, device enumeration, persistence,
  and bounce setup remain outside the audio callback.

## Surfaces

### 1. Project deck / startup splash

Shown on ordinary launch unless `skip_splash` is set. A command-line project
opens directly and suppresses the splash. It is also summonable anywhere.

The deck contains:

- `NEW`: start an empty song.
- `OPEN PATH`: edit or paste a `.stage.ron` / compatible `.daw.ron` path.
- `SAVE`: save the current song to its known path, or the next safe untitled
  name in the project folder.
- `SAVE AS`: write to the entered path without overwriting by accident unless
  overwrite is explicitly confirmed.
- recent projects, newest first, with file name, parent, modified age, size,
  dirty/missing status, and keyboard selection;
- forget one missing entry and clean all missing entries;
- the current project and recovery status;
- a `SHOW AT STARTUP` toggle.

Opening, saving, and Save As update the bounded deduplicated recent list.

### 2. Unsaved-change interlock

`NEW`, `OPEN`, and opening a recent project pass through one interlock when
the current song is dirty:

- Save and continue;
- Discard and continue;
- Cancel.

No destructive project transition may bypass this state.

### 3. Preferences console

Four pages share one navigation model.

#### Audio

- backend: JACK / ALSA / PulseAudio;
- output device by stable name, including backend default;
- supported sample rates from the selected device;
- buffer size with calculated one-way block time;
- explicit rescan;
- requested configuration beside the actual running backend, rate, buffer,
  channel count, reported latency, engine health, load, and xruns;
- `APPLY + RESTART`, performed by the host after the modal yields a request.

#### Projects

- default projects folder;
- autosave/recovery interval: off / 1 / 2 / 5 / 10 / 15 minutes;
- keep timestamped backup before overwriting an existing project;
- confirm before replacing a dirty project;
- show project deck at startup;
- clear missing recents.

#### Library

- user-library path;
- additional sample roots;
- add/remove roots with validation;
- rescan and index status: assets, scales, lenses, warnings.

#### Interface

- dark/light display ground;
- comfortable/compact density;
- motion: full/reduced;
- cursor energy: quiet/normal/high;
- tooltips on/off.

Motion and cursor energy are machine preferences and feed the shared cursor;
they never enter project data.

### 4. Export console

The existing offline bounce becomes an explicit job form:

- range: whole song or active loop brace;
- destination path derived from the project and still editable;
- WAV encoding: 32-bit float, 24-bit integer, or 16-bit integer;
- rate: running device, 44.1, 48, 88.2, or 96 kHz;
- tail: 0, 1, 2, 5, or 10 seconds, added after the musical range;
- exact start/end ticks, duration, frame estimate, and approximate file size;
- one render at a time, cancellable with Escape;
- successful exports join a bounded destination history; failures and
  cancellations remain visible in the console status and leave no partial WAV.

The render uses the existing deterministic offline graph. It never uses the
live callback and never silently substitutes a different range.

### 5. System / diagnostics console

A fifth utility surface covers the support work a DAW inevitably needs:

- current project path, dirty state, track/pattern/device counts;
- actual audio stream, health, load, xruns, and latency;
- library generation and indexed counts;
- current recovery file and most recent export;
- version and build target;
- copy a concise diagnostics report to the clipboard;
- rescan audio devices and library from the place diagnosing them.

## Host boundary

The stage owns presentation, keyboard focus, document transitions, recent
history, and preference editing. It emits bounded host requests:

- enumerate audio devices for one backend;
- restart audio with one complete requested configuration;
- render one complete export request.

The `stage` binary performs those operations and returns immutable status.
No UI module imports the audio engine.

## Persistence and recovery

- `UiPrefs` remains backward-compatible through `#[serde(default)]` fields.
- Recents, utility preferences, project folder, export defaults, library
  roots, and presentation settings are machine-local shell storage.
- The current song remains the only project document.
- While dirty, periodic recovery writes an atomic sidecar under the project
  folder's `recovery` directory. A successful save removes its recovery file.
- Before overwriting an existing project, an optional timestamped copy is
  placed under `backups`.
- Writes use temporary siblings plus rename so interruption cannot leave the
  named project half-written.

## Keyboard vocabulary

- `Ctrl+O`: project deck
- `Ctrl+,`: preferences
- `Ctrl+Shift+E`: export console
- `Ctrl+Shift+D`: diagnostics
- `Up/Down` or `J/K`: move rows
- `Left/Right` or `H/L`: change a finite value or page
- `Enter`: activate / edit
- `Escape`: stop editing, close a subdialog, cancel a render, or leave
- `Tab` / `Shift+Tab`: next / previous target

These commands are also present in the command palette and help vocabulary.

## Acceptance criteria

1. First ordinary launch opens the project deck; a CLI project opens directly.
2. New/open cannot destroy dirty work without the interlock.
3. Save/open/recent management operates on real project files and persists.
4. Audio preference changes restart the engine through the host and report
   the negotiated result or the exact failure.
5. Export options reach the actual bounce options and produce the requested
   WAV encoding, rate, range, and tail.
6. Utility modals consume their keys and use the shared animated cursor.
7. Recovery and backup paths are deterministic, bounded, and tested.
8. Old preference blobs still load; no musical data appears in preferences.
9. Headless poses cover project, preferences, export, and diagnostics screens.
10. `cargo fmt`, focused utility tests, and the `stage` build pass.
