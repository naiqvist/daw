# The light table — the same view, the opposite material

**Reference image (the app today, four tracks, sequencer open on clip d0):**
https://raw.githubusercontent.com/naiqvist/daw/ui-redesign/docs/restyle/session-4-tracks.png

Dark mode only. Sibling of [`INSTRUMENT.md`](INSTRUMENT.md).

---

## Why this exists

`INSTRUMENT.md` and this file specify **the same layout, to the same pixel
bands**, and differ only in material. That is deliberate: run both, and the
comparison is a clean one about aesthetics rather than a muddled one about
whether the sequencer got bigger.

Where the instrument face is **milled from one block** — flat, square, opaque,
separated by hairlines, depth from two ground levels — this one is **stacked
sheets of smoked glass over emitting elements**. An optical bench, a backlit
light table, a rack of cut glass with light coming through it.

The earlier glass attempt failed because "frosted glass" reads to a generator
as "dark panel with a 1px border", and it produced a competent commercial
plugin twice. This brief does not ask for frosting. It asks for **depth and
emission**, which is what glass actually is.

---

## The prompt

```
Draw the main working view of a keyboard-driven music workstation — a
groovebox, 1280x800, DARK MODE ONLY. This is a specification with exact pixel
bands; follow them.

THE REFERENCE FOR THE LOOK IS AN OPTICAL BENCH: sheets of smoked glass
suspended over emitting elements, in a black room. A backlit light table. A
rack of cut glass with light coming up through it. NOT an app, NOT a plugin,
NOT frosted-glass UI chrome.

SIX RULES, all absolute:
1. DEPTH IS THE DESIGN. Exactly three z-planes, and the viewer can tell which
   is which: a far plane, a middle plane, and one sheet closest to the eye.
   Planes cast soft shadows onto the planes behind them. Nothing is flat.
2. LIGHT IS THE MATERIAL. Live things EMIT — they glow from inside the glass
   and spill light onto the sheet beneath. Everything else is lit only by
   them. There is no ambient fill light in this room.
3. THE GLASS IS NEUTRAL AND SMOKED. Dark, faintly warm, almost colourless.
   ALL colour on screen comes from something emitting. A sheet of glass never
   has a hue of its own.
4. BLUR ONLY WHAT IS BEHIND. A plane blurs the planes behind it, and further
   means blurrier. Nothing ON a sheet is ever blurred, tinted, or reduced in
   contrast — text sits on top of the glass at full strength, always.
5. EDGES ARE CUT, NOT ROUNDED. Every sheet is a square-cornered pane, and its
   cut edge catches a bright hairline of refracted light along the top and one
   side. That lit edge is what makes it read as glass rather than as a
   rectangle of paint.
6. DENSITY SURVIVES. This is still an instrument, densely packed with
   readouts. Depth does the separating that hairlines would otherwise do —
   it is not an excuse for empty space.

PALETTE:
The room is black. The glass is a very dark neutral, barely warm. Emitting
elements are the only colour: a cold cyan-white for readouts and values, a
deep amber for held or armed state, and one saturated magenta-red for the
thing playing right now. Light falls off with distance — a glow illuminates
the sheet under it and fades.

EXACT LAYOUT, 1280x800:

y 0-36 — STATUS. On the FAR plane, dimmest, slightly out of focus. Left:
scope word, track number and name, pattern number. Right: song name.

y 36-84 — PAGE KEYS. Eight equal 160px panes, edge to edge, on the MIDDLE
plane. Each: a small glyph, its number, its word (TRIG, SRC, FLTR, AMP, LFO,
FX, MIX, and an eighth). Exactly one is lit — that one alone emits, glowing
cyan-white through its pane and casting light onto the plane behind it. The
other seven are dark glass, legends visible but unlit.

y 84-756 — THE BODY, split at x=220:

  x 0-220 — SESSION RAIL, on the FAR plane, blurred by distance. Four stacked
  track blocks about 160px tall: number and short name (KICK, BASS, KEYS,
  HAT), and beneath each a dense column of eight small scene cells. One cell
  emits magenta-red: the clip playing now, and its glow reaches the sheets in
  front of it. The rest are dark. This rail is a LOCATOR — quiet, receding.

  x 220-1280 — THE SEQUENCER, on the NEAREST sheet, sharp and dominant. It is
  the largest object on the screen.
    y 84-120: a header — clip name, length in bars, step resolution, note
      lens. Cyan-white on glass.
    y 120-620: THE STEP GRID. Sixteen steps across, each a tall pane about
      64px wide with a cut lit edge. They MUST NOT be sixteen identical
      panes — each differs:
        - about six are dark, empty glass
        - the filled ones emit: a note name and a VELOCITY BAR of light whose
          height differs from step to step
        - two carry a small bright square: they hold parameter locks
        - one has a fine trailing streak of light from its lock: it slides
        - one shows "3:4" in small figures: a condition, firing on pass three
          of every four cycles
        - one shows three fine vertical filaments: a retrig
        - one glows a different colour entirely, deep amber: a sound lock,
          playing through another machine
        - one step burns magenta-red, brightest thing on the screen, and its
          light spills onto the panes either side of it: the playhead
      A time base printed along the top edge, ticks numbered 1 5 9 13.
    y 620-756: THE TRIG READOUT, a wide band on the nearest sheet. Eight
      label/value pairs in one row: STEP, POSITION, PITCH, LENGTH, VELOCITY,
      CHANCE, NOTES, STATE. Small dim labels over large emitting values.

y 756-800 — TRANSPORT. Middle plane. Left: bar, bpm, time signature as large
figures. Centre: rewind, stop, play, record, forward as five square panes, the
active one emitting. Right: one small round indicator. Nothing else — no
sample rate, no buffer, no load, no xruns.

NO PANEL ON THE RIGHT-HAND SIDE. No mixer strip, no meters, no inspector, no
log. The sequencer owns that space.

EDGE TO EDGE. The sheets extend past the frame; there is no page margin and
no background visible around the outside.

TYPE: one monospaced family throughout. Small dim letter-spaced caps for
labels, large emitting figures for values. Always crisp, never blurred, never
tinted by the glass it sits on.

Photoreal render, sharp where it should be sharp, 1280x800, no mockup frame,
no desktop behind it, no hands, no window chrome.
```

---

## How to judge what comes back

1. **Can you tell the three planes apart?** If it is flat, the whole idea is
   absent and it will look like the plugin again.
2. **Does anything actually emit, and does its light land on something else?**
   Glow with no spill is a coloured rectangle, not light.
3. **Is the glass itself colourless?** Any hue in a pane that is not emitting
   means rule 3 was ignored, and the screen will read as tinted plastic.
4. **Are the sixteen steps different from each other?** Six states specified.
5. **Is the text crisp everywhere?** Blurred or dimmed text is the failure that
   makes real glass UIs unusable, and it is the thing Apple gets right.
6. **Is the sequencer the biggest object on screen?**

## Judging it against its sibling

Same layout, same bands, same functionality. So the only question is which
material suits an instrument you look at for hours: **a face you read**, or
**a light you watch**. Run both, put them side by side, and pick — the loop
after that is identical either way.
