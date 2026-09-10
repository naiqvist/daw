# The restyle loop

How a mockup becomes the app. Written down so it is a process and not a habit.

## The division of labour

| Stage | Who | Output |
| --- | --- | --- |
| 1. Specify | Claude | A prompt naming the view's **real functionality** — every element exists in the code and carries real state |
| 2. Generate | ChatGPT | A 1280×800 render |
| 3. Judge | Both | Accept, or a note saying what to change and why |
| 4. Trace | Claude | `tools/trace.py auto` → palette, bands, cell geometry, borders, radii |
| 5. Apply | Claude | `tools/trace.py apply` → `~/Corpus/daw.tune` + `~/Corpus/daw.theme`, live |

Stage 1 is the one that cannot be delegated to the generator: it does not know
that a step carries a condition, a retrig and a sound lock, and it will draw
sixteen identical pads every time unless told. Stages 4 and 5 are mechanical.
Stage 3 is the only one that is taste, and it is the user's.

## The gates

**Aesthetic is not locked until one view passes.** Iterate freely on the FIRST
view. Once it is accepted, its palette, material rules and type treatment become
the reference every later prompt inherits verbatim — otherwise eighteen views
drift into eighteen slightly different designs and none of them agree.

Concretely, on acceptance of view one:
- its palette is traced and written to `docs/themes/`
- its material rules (recessed vs raised, where borders live, corner radius,
  what carries hue) are copied into a short **house block** pasted into every
  subsequent prompt unchanged
- only LAYOUT is open for later views

**Functional accuracy is a gate, not a preference.** Every prompt is checked
against the real model before it is sent. If a mockup shows something the app
cannot do, tracing it produces a lie.

**Acceptance needs a test, not a feeling.** Each view's brief ends with its own
questions. DECK's first one is "are the sixteen steps different from each
other?" — because if they are not, the instrument's whole point has been drawn
away, however good it looks.

## The known ceiling

**The tracer carries geometry, palette, borders and radii. It cannot carry
material.** Frosted glass, blur, inner shadow and gradient depth are not
expressible in egui's shape API — a plain linear gradient already costs a
hand-built mesh, and blur is not available at all.

So a view can be traced to the pixel and still not LOOK like its mockup until
`src/shell/panel.rs` + `panel.wgsl` exist: an SDF rounded-rect pass with
gradient, inner border and shadow, registered the way `src/shell/screen.rs`
already registers apertures and drawn before egui.

Until that pass is built, stage 5 delivers the right layout in the right
colours with flat panels. That is a real improvement and it is not the mockup.
The pass is the largest remaining piece of work in this whole effort.

## Order of views

1. **DECK** — the pages. The product thesis lives here; get it right first and
   the house block comes from it.
2. **SESSION** — the primary workspace, and the one with the most open
   questions (the desk block, the dead upper third).
3. **CLIP / the tracker** — where the six per-step states have to be fully
   legible rather than merely marked.
4. Everything else, in the order the work reaches it.

Eighteen views exist in `ui-mockups/soft-pc98-earth/`. They are the layout
reference for anything not yet regenerated.

## Artefacts

| File | What it is |
| --- | --- |
| `docs/restyle/PROMPT.md` | Brief 1 — restyle only, layout frozen |
| `docs/restyle/RELAYOUT.md` | Brief 2 — moves things, reasons recorded |
| `docs/restyle/DECK.md` | Brief 3 — one view, specified against real functionality |
| `docs/restyle/workspace-session.png` | The current session view, public raw URL |
| `tools/trace.py` | The instrument: normalise, bands, probe, cells, palette, diff, auto, render, emit, apply |
| `tools/maps/stage.json` | Which measurement is which constant — the only hardcoding |
