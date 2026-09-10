# The plotter — the same information, drawn instead of built

**Reference image (the app today, four tracks, sequencer open on clip d0):**
https://raw.githubusercontent.com/naiqvist/daw/ui-redesign/docs/restyle/session-4-tracks.png

Dark mode only. Third sibling to [`INSTRUMENT.md`](INSTRUMENT.md) and
[`GLASS.md`](GLASS.md).

---

## How this one differs

The first two fix the layout to exact pixel bands. This one **fixes the
information and frees the arrangement**: everything in the inventory below must
be present and legible, but sizes and positions may move where the composition
is better for it.

And the material is the opposite of both siblings. The instrument face is
**opaque solids**. The light table is **emission**. This is **pure line work** —
nothing is filled, nothing glows, nothing is a panel. The whole interface is
drawn, the way a pen plotter draws on drafting film: one ink, one weight,
hairlines and hatching and measured callouts.

Nobody makes interfaces out of line work, which is exactly why it will not
come back looking like a plugin or a web app.

---

## The information that must all be present

Non-negotiable. Missing any of it means the view cannot be used.

**Identity** — scope word, track number and name, pattern number, song name.

**Page keys** — eight of them: TRIG, SRC, FLTR, AMP, LFO, FX, MIX, and an
eighth. Exactly one shown as current.

**Session** — four tracks, named KICK, BASS, KEYS and HAT, each with a column
of eight scene slots, filled or empty, and exactly one marked as playing.

**The sequencer** — sixteen steps, and each step must show whichever of these
it carries:
  - empty, or a note (name and velocity)
  - a parameter lock
  - a lock that slides to the next
  - a condition, printed as `3:4`
  - a retrig
  - a sound lock, playing through another machine
  - the playhead, on exactly one step

**The trig readout** — eight label/value pairs: STEP, POSITION, PITCH, LENGTH,
VELOCITY, CHANCE, NOTES, STATE.

**Clip header** — clip name, length in bars, step resolution, note lens.

**Transport** — bar, bpm, time signature, and five controls: rewind, stop,
play, record, forward. One health indicator.

**Absent, deliberately** — no mixer strip, no bus meters, no inspector, no log,
no sample rate, no buffer, no load, no xruns.

## What may move

The sequencer must be the largest thing on screen and the session must read as
secondary — those two are fixed. Everything else is free: the readout may sit
beside the grid rather than beneath it, the page keys may run down a side, the
transport may be a corner block. Compose it as a drawing wants to be composed.

---

## The prompt

```
Draw the main working view of a keyboard-driven music workstation — a
groovebox sequencer, 1280x800, DARK MODE ONLY.

THE ENTIRE INTERFACE IS LINE WORK. This is a technical drawing produced by a
pen plotter on dark drafting film. Not a rendering of an object, not panels,
not screens — a DRAWING of an instrument. Nothing is filled with a solid
colour. Nothing glows. Nothing has a shadow, a gradient, a bevel or a
translucency. If a region needs weight, it is HATCHED with fine parallel
lines, never filled.

SEVEN RULES:
1. ONE INK, ONE WEIGHT. A single hairline stroke draws everything. Emphasis
   comes from HATCHING DENSITY and from LINE LENGTH, never from a thicker
   stroke or a brighter colour.
2. HATCHING IS THE ONLY FILL. Fine parallel lines. Denser hatch reads as
   heavier. Cross-hatch for the heaviest. A solid block is forbidden.
3. IT IS DIMENSIONED LIKE A DRAWING. Extension lines, dimension lines with
   arrowheads at both ends, and measured callouts printed along them — the
   step grid's width, the pattern's length in bars, the tempo. Leader lines
   run from small numbered circles to annotations in the margins.
4. THE GRID IS A DRAFTING GRID. A faint ruled field underneath everything,
   with heavier lines every fourth division and tick marks numbered along the
   top and left edges, the way a drawing sheet is ruled.
5. SQUARE CORNERS ONLY. Every corner is a mitred junction of two straight
   lines. No radius anywhere. Line ends are cut square, not capped round.
6. ONE ACCENT: a single warm ink — burnt orange — used ONLY for what is live
   right now. The playhead, the playing clip, the current page. Three marks at
   most on the whole sheet. Everything else is the one pale ink.
7. DENSITY. A technical drawing fills its sheet. Empty film is wasted sheet,
   not breathing room.

PALETTE:
The film is a very dark desaturated blue-black. The ink is a pale warm
off-white, like a fine ceramic pen. The accent is burnt orange. Three colours
on the sheet and no more.

WHAT THE DRAWING CONTAINS:

A title block, bottom right or along one edge, the way a drawing sheet carries
one: scope word, track number and name, pattern number, song name — set in
small ruled boxes with printed field labels.

Eight page-key cells: TRIG, SRC, FLTR, AMP, LFO, FX, MIX, and an eighth. Each
a square outline with a small drawn glyph and its legend. The current one is
hatched in burnt orange; the rest are outline only.

A session block: four tracks, KICK, BASS, KEYS, HAT, each named in a ruled
box with a column of eight scene slots beneath it. Filled slots are hatched;
empty ones are open outline. Exactly one slot is orange — the clip playing.
This block is SECONDARY: draw it smaller and quieter than the sequencer.

THE SEQUENCER, the largest element on the sheet: sixteen steps in a row, each
a tall open rectangle on the ruled grid. Every one is different:
  - about six are empty outline
  - the filled ones carry a note name and a VELOCITY BAR drawn as a stack of
    fine horizontal rules — more rules means louder
  - two carry a small hatched square: a parameter lock
  - one has a dimension line running from its lock to the next step: the lock
    slides
  - one is annotated "3:4" on a leader line: a condition
  - one has three fine vertical filaments drawn inside it: a retrig
  - one is cross-hatched and annotated: a sound lock, another machine
  - one is hatched in burnt orange: the playhead
A ruled time base above the row, ticks numbered 1 5 9 13, with a dimension
line beneath it measuring the full sixteen.

A readout block: eight label/value pairs — STEP, POSITION, PITCH, LENGTH,
VELOCITY, CHANCE, NOTES, STATE — set in a ruled table with printed field
labels above large figures. Place it wherever the composition wants it.

A clip header: clip name, length in bars, step resolution, note lens.

A transport block: bar, bpm and time signature as large figures, and five
square outlined controls — rewind, stop, play, record, forward — the active
one hatched orange. One small circle for health.

NOTHING ELSE. No mixer, no meters, no inspector, no log, no sample rate, no
buffer, no load, no xruns.

PLACEMENT IS FREE apart from two things: the sequencer must dominate the
sheet, and the session block must read as secondary. Compose the rest as a
draughtsman would.

TYPE: one monospaced family, drawn as if stencilled. Small letter-spaced caps
for printed field labels, large figures for values. Nothing italic.

A technical drawing, sharp, 1280x800, no paper texture, no mockup frame, no
desk behind it, no hands, no window chrome.
```

---

## How to judge what comes back

1. **Is anything filled with a solid colour?** One solid block and the idea is
   gone — it becomes a flat-design UI, which is not this.
2. **Is there hatching, and does denser hatch mean heavier?** That is the whole
   tonal system. Without it the drawing has no hierarchy.
3. **Are there real dimension lines and callouts?** Arrowheads, extension
   lines, leaders to annotations. This is what makes it a drawing rather than
   an outline-style UI.
4. **Sixteen steps, all different?** Seven states specified.
5. **Three orange marks at most?**
6. **Is the sheet full?**
7. **Is every piece of information from the inventory present?** Placement was
   free; content was not.
