# The DECK view, optimally

One view, specified against what the app actually does. Layout may change —
it should — but every element here exists in the code today and carries real
state. Nothing below is invented.

**Reference (the current UI):**
https://raw.githubusercontent.com/naiqvist/daw/ui-redesign/docs/restyle/workspace-session.png

---

## Why this view

The product thesis is that **the pages are the instrument**: a track is designed
and sequenced on the same eight cells, and a parameter lock is simply a
parameter edit that belongs to one step. The DECK is where that happens. It is
the view worth getting right first, and it is the view both previous attempts
circled without landing on.

## What the app really has here

| Thing | What it carries |
| --- | --- |
| Page keys F1–F8 | Words are declared **by the machine**, not fixed. ROM puts `ROM` on F8; a machine that declares nothing is not offered at all. |
| Sub-pages | Unbounded. Tapping the lit key steps and wraps; Shift steps back. Position shows as `n/count`. |
| Hero | The machine's own picture. Two heights: a band, or tall. Series, marks, axis labels, an optional unity diagonal, or a waveform. ROM and the sampler show a browsable list with a selected row instead. |
| Hero tools | A tall hero declares a key legend — `Z ZOOM  G GRID  C DETECT  S SPLIT  M MERGE  F FIND LOOP`. Keys shown where the work is. |
| The eight cells | Each: name, value, unit, a 0..1 fill, and three states — **locked** (this step holds a lock on this parameter), **slide** (the lock glides to the next lock), **out** (the section is switched out, so the knob moves the document and nothing sounds). |
| A step | Six states, not on/off: **enabled**, **notes** (pitch, velocity, length, micro-timing), **probability**, **condition** (fire on pass A of every B cycles), **retrig** (repeats inside the step), **sound lock** (this step plays through a different machine entirely). |
| Pattern windows | A pattern is longer than sixteen steps; the visible window is one of several, and which one must be legible. |

**The step row is where this app is unlike anything else, and both mockups so
far drew it as sixteen identical pads.** That is the single biggest functional
miss. If a step's condition, retrig and sound-lock are not visible, the view has
thrown away the instrument's whole point.

---

## The prompt

```
Draw the main editing view of a keyboard-driven music workstation — a groovebox
console, 1280x800, dark. This is a SPECIFICATION; follow the arrangement and the
state markings exactly.

TOP, thin (about 40px): a status row. Scope word DECK, then track number and
name, pattern number, and the machine's name. Nothing else.

UNDER IT (about 48px): eight page keys edge to edge — TRIG, SRC, FLTR, AMP,
LFO, FX, MIX, and an eighth carrying a machine-specific word. Each has a small
technical glyph. Exactly one is lit. The lit one shows small position pips
reading 2/5, because a page has several sub-pages.

THE HERO, a wide panel across the upper middle, the largest picture on screen:
a filter response curve over a faint spectrum, with a dB scale up the left and
a Hz scale along the foot, and one draggable handle on the curve. Beneath the
hero, a single row of key legends in small type: "Z ZOOM   G GRID   C DETECT
S SPLIT   M MERGE   F FIND LOOP" — the keys shown where the work is.

THE EIGHT CELLS, one row, directly under the hero. Each is a numbered slab, 1
to 8, showing a parameter name, its value with its unit, and a horizontal fill
bar. They are the focal point. Three of them are marked, differently:
  - cell 2 has a small filled square in its corner: a LOCK, meaning this step
    holds its own value for that parameter
  - cell 5 has a lock mark with a short arrow trailing to the right: a SLIDING
    lock, gliding toward the next
  - cell 7 is dimmed with the word OUT small in its corner: that section is
    switched out, so the knob moves the document but nothing sounds
The other five are plain. Only the selected cell has an outline.

THE STEP ROW, across the foot, sixteen steps. THIS IS THE MOST IMPORTANT PART
AND MUST NOT BE SIXTEEN IDENTICAL PADS. Every step is a narrow vertical slab
and they differ:
  - most are empty, just a recessed well
  - about seven are filled, and each filled one shows a small note name and a
    velocity bar whose HEIGHT varies from step to step
  - two filled steps carry a tiny lock glyph, meaning they hold parameter locks
  - one shows a small "3:4" — a condition, firing on pass 3 of every 4
  - one shows three fine vertical ticks — a retrig, repeating inside the step
  - one is tinted a different hue entirely and marked with a small badge — a
    SOUND LOCK, this step playing through a different machine
  - one step is brighter than all the others: the playhead, on it right now
To the right of the row, four small dots with the second one lit: which window
of a longer pattern is on screen.

BOTTOM (about 45px): bar, bpm, time signature at the left; rewind, stop, play,
record, forward as five square cells centred; one small round health indicator
at the far right. Nothing else — no sample rate, no buffer, no load, no xruns.

EDGE TO EDGE. No outer margin, no page gutter, no floating cards on a
background. Panels meet at seams. Hardware, not a document.

MATERIAL: machined, not glassy. The eight cells read as RECESSED into a
faceplate — milled wells with a dark interior and a bright lip catching light
along their top edge — rather than raised cards sitting on top. Anodised dark
metal, screen-printed legends, a little worn at the edges from use. No pillowy
rounded cards, no drop shadows for their own sake, no pastel, no decorative
gradients, no illustration. Corners tight, 2px.

BORDERS: only the selected cell and the lit page key have an outline. Nothing
else does. Separation comes from the material and the seams.

COLOUR: near-black, committed cold — a blue-black, not a neutral grey-black.
Names and static labels are neutral grey. Hue belongs only to live state: hot
orange for the playhead and the selected cell, bright green for a sounding
value inside its budget, one saturated blue for the lit page key, and one
distinct hue for the sound-locked step. Three or four coloured things on the
entire screen; everything else grey. The screen reads quiet and the live
things leap out.

BREAK THE SYMMETRY ONCE: the hero panel runs slightly wider than the row of
cells beneath it, overhanging on the right. Everything else lines up.

TYPE: monospace throughout. Numerals aligned in columns. Uppercase labels,
letter-spaced, small. Parameter values large. Legibility outranks style.

Photoreal UI render, sharp, 1280x800, no mockup frame, no desktop behind it,
no hands, no window chrome.
```

---

## How to judge what comes back

1. **Are the sixteen steps different from each other?** If they are identical
   pads, reject it — that is the instrument's whole point discarded.
2. **Are lock, slide and out visible on the cells?** Three marks, three
   meanings. Without them the cells are a plugin's knob row.
3. **Is the right-hand side still empty?** No mixer strip, no inspector, no
   meters crept back in.
4. **Is anything coloured that is not live?** If labels are blue, the colour
   rule was ignored and the live things will not read.
5. **Does it look machined or does it look like a plugin?** The previous
   attempt produced a competent commercial plugin. Recessed cells, one
   asymmetry, no uniform borders — those are the three levers against it.

Then run `tools/trace.py auto` on it: palette, bands, cell geometry, borders and
radii, and whether the layout is a grid or was placed by hand.
