# The instrument face — session with the sequencer open

**Reference image (the app today, four tracks, sequencer open on clip d0):**
https://raw.githubusercontent.com/naiqvist/daw/ui-redesign/docs/restyle/session-4-tracks.png

Dark mode only. No light variant, no theme switch.

---

## The problem this brief exists to fix

In the reference, **67% of the field between the key row and the tray is bare
ground**, and the sequencer occupies about 6% of the screen — a postage stamp in
the bottom-right corner while two-thirds of the display shows nothing.

The sequencer is the instrument. When a clip is open it should be the largest
object on screen. That is the single change this brief is for; everything else
follows from it.

---

## The prompt

```
Draw the main working view of a keyboard-driven music workstation — a hardware
groovebox, 1280x800, DARK MODE ONLY. This is a specification with exact pixel
bands; follow them.

THE REFERENCE FOR THE LOOK IS A JAPANESE LABORATORY INSTRUMENT — a Yokogawa or
Kikusui oscilloscope front panel, an Iwatsu signal generator, a Casio fx
scientific calculator, a PC-98 workstation. Not an app. Not a plugin. Not a
website. A measurement instrument for sound.

SEVEN RULES, all of them absolute:
1. ZERO CORNER RADIUS. Every rectangle is square. Not 2px. Zero.
2. NO CARDS. Regions are separated by hairline rules one pixel wide, and by
   sitting at a different ground level. Never by a box with a border around it.
   Nothing floats on a background.
3. THE GRID IS VISIBLE. A strict modular layout with silk-screened division
   marks and tick rules along the edges of panels, the way an instrument face
   is printed. The mathematics of the layout should be apparent.
4. NUMERALS ARE THE HERO. Tabular monospaced figures, large and confident,
   legible at arm's length across a dark room.
5. DENSITY IS A VIRTUE. Fill the panel with information. Bare ground is a
   fault, not breathing room.
6. DEPTH COMES FROM GROUND LEVEL, NOT SHADOW. Exactly two levels: recessed
   wells milled into the face, and the flush face itself. No drop shadows, no
   glow, no bloom, no frosted glass, no translucency.
7. ONE ACCENT ONLY: vermilion. Everything else is neutral. Hue means LIVE and
   nothing else.

PALETTE:
Ground: a very dark cool grey-green, the colour of an instrument case —
near-black with a faint green bias, not brown and not blue.
Face: one step lighter, the same hue.
Wells: one step darker than the ground.
Rules and legends: warm mid-grey, silk-screened, never white.
Readouts: near-white, high contrast.
Live: vermilion, the only saturated colour on the screen. At most three
vermilion things at once.

EXACT LAYOUT, 1280x800:

y 0-36 — STATUS RULE. Full width. Left: scope word, track number and name,
pattern number. Right: song name. Small caps, letter-spaced, silk-screen grey.
A hairline under it.

y 36-84 — PAGE KEYS. Eight equal cells, 160px each, edge to edge, no gaps
between them — divided by hairlines only. Each: a small technical glyph, its
number, its word (TRIG, SRC, FLTR, AMP, LFO, FX, MIX, and an eighth). Exactly
one is lit: vermilion legend on the flush face, the others silk-screen grey in
recessed wells.

y 84-756 — THE BODY, split at x=220:

  x 0-220 — SESSION RAIL. Four track columns is wrong here; instead four
  stacked track blocks, each about 160px tall: a number and a short name
  (KICK, BASS, KEYS, HAT) on the flush face, and beneath it a dense column of
  eight small scene cells, filled or empty. One cell is vermilion: the clip
  playing. This rail is a LOCATOR — dense, quiet, secondary, and it must not
  look like a list of buttons.

  x 220-1280 — THE SEQUENCER. This is the largest object on the screen and it
  must dominate.
    y 84-120: a header rule — clip name, length in bars, step resolution,
      and the note lens. Silk-screen grey, hairline under it.
    y 120-620: THE STEP GRID. Sixteen steps across, each a tall column about
      64px wide, separated by hairlines. Each step is a recessed well. They
      MUST NOT be sixteen identical pads — every one differs:
        - about six are empty wells
        - the filled ones show a note name in white and a VELOCITY BAR whose
          height differs from step to step
        - two carry a small square lock mark: they hold parameter locks
        - one shows a fine trailing arrow from its lock: the lock slides
        - one shows "3:4" in small figures: a condition, firing on pass three
          of every four cycles
        - one shows three fine vertical ticks: a retrig, repeating inside it
        - one is marked with a small badge and a different tint: a sound lock,
          this step playing through another machine entirely
        - one step is lit vermilion: the playhead is on it now
      Tick marks along the top rule every four steps, numbered 1 5 9 13, the
      way a time base is printed on an instrument.
    y 620-756: THE TRIG READOUT, a wide horizontal band under the grid. Eight
      label/value pairs laid out in one row: STEP, POSITION, PITCH, LENGTH,
      VELOCITY, CHANCE, NOTES, STATE. Labels tiny and silk-screened above
      large tabular values. This is the instrument's main readout and should
      look like one.

y 756-800 — TRANSPORT RULE. Left: bar, bpm, time signature, as large tabular
figures. Centre: rewind, stop, play, record, forward — five square recessed
wells, the active one lit vermilion. Right: one small round indicator. Nothing
else — no sample rate, no buffer, no load, no xruns.

NO PANEL ON THE RIGHT-HAND SIDE. No mixer strip, no meters, no inspector, no
log. The sequencer owns that space.

EDGE TO EDGE. No outer margin, no page gutter. Panels meet at hairlines.

TYPE: one monospaced family throughout. Legends in small letter-spaced caps.
Values in large tabular figures. Nothing italic, nothing decorative.

Photoreal render of a hardware instrument's screen, sharp, 1280x800, no mockup
frame, no desktop behind it, no hands, no window chrome, no reflections.
```

---

## How to judge what comes back

1. **Is the sequencer the biggest thing on screen?** If it is still a tray at
   the foot, nothing else matters.
2. **Are the sixteen steps different from each other?** Six states are
   specified. Identical pads means the instrument was drawn away.
3. **Any rounded corners anywhere?** Reject. Zero means zero.
4. **Any boxes with borders around regions?** The separation must be hairlines
   and ground level. A bordered box is a card and cards are the app look.
5. **How much bare ground is left?** The reference is 67%. Anything close to
   that has not solved the problem.
6. **Is more than one thing vermilion at a time?** Three at most: the playhead,
   the playing clip, the lit page key.

Then `tools/trace.py auto` on it for the numbers.
