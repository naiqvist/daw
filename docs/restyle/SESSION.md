# The session view

**Style reference — its FRAME and palette are right, keep them:**
https://raw.githubusercontent.com/naiqvist/daw/ui-redesign/docs/restyle/underground-v2.png

**Content reference — the session as it is today:**
https://raw.githubusercontent.com/naiqvist/daw/ui-redesign/docs/restyle/session-4-tracks.png

Dark mode only. A different VIEW of the same instrument, not a different design.

---

## What the session view is for

Not editing — **launching and arranging**. Scenes run down, tracks run across,
and a clip sits where they meet. You look at this screen to see what is
playing, what is loaded, and what you could fire next. Every design decision
follows from that.

The previous renders solved the clip editor. This is the other half, and it is
the view the user is in most of the time.

## What it really contains

| Thing | What it carries |
| --- | --- |
| A track head | Number, name, and the lane's word in the lane's own hue. Mute and solo as two pips. Armed or recording. A bar along its foot while it is sounding. |
| A scene row | Sixteen scenes to a bank, banks lettered. Numbered down the left with a graduated rule — a tick per row, a longer tick every fourth. |
| A cell | Empty, or a clip. A clip shows its TAG — `a1`, `b0`, `c2` — never a name and never a length. The playing clip carries a rail down its left edge. |
| The master | Its own column, pinned to the right edge, separate from the tracks. |
| The tray | The clip under the cursor, with its trig readout. |
| Transport | Bar, bpm, meter, state, and the five controls. |

**Absent, deliberately:** no bus meters, no log, no mixer strip, no inspector.
Those belong to other views.

---

## The prompt

```
Two references. The FIRST shows an instrument whose FRAME, PALETTE and DEPTH
are correct — the near-black faceplate, the recessed wells, the physical
chassis, the monospaced silkscreen type. The SECOND shows the view to be
redrawn.

Draw the SESSION view of this instrument: a clip-launching grid. 1280x800,
dark mode only.

This is a different VIEW of the same machine, so the frame must look like it
belongs to the first reference. What changes is what fills it.

WHAT THE SESSION VIEW IS FOR: launching and arranging, not editing. Scenes run
DOWN, tracks run ACROSS, a clip sits where they meet. Everything serves seeing
what is playing and what could fire next.

REMOVE EVERY ICON. Words and numbers only, everywhere, including the tab strip
at the top. An instrument has legends, not pictograms.

LAYOUT:

A thin top strip: scope word, track number and name, pattern number, song
name. Small monospaced caps.

Below it, the tab strip: TRIG SRC FLTR AMP LFO FX MIX SYS, words and key
numbers only, no glyphs. One active.

THE GRID fills almost everything below, and it is the point of the screen:

  A row of TRACK HEADS across the top, one per track, six tracks: KICK, BASS,
  KEYS, HAT, PERC, DUB. Each head is a recessed plate carrying:
    - its number, small and dim
    - its NAME in monospaced caps
    - the lane's word in the lane's own hue — DRUM in one colour, PLAIN
      unmarked — as a short badge, not an icon
    - two small square pips for mute and solo, lit when on
    - the word ARM or REC when armed or recording
    - a thin bar along its foot, lit, when that track is sounding now

  Beneath the heads, SIXTEEN SCENE ROWS — not eight. Numbered 01 to 16 down a
  narrow gutter at the left, with a graduated rule beside them: a short tick
  per row, a longer tick every fourth. A bank letter, A, sits above the
  numbers.

  Each CELL is a flat recessed slot. Most hold a clip; a clip shows only its
  TAG — a1, a2, b0, b1, c0, d3 — in monospaced text, coloured by the lane of
  its track, so a column reads as one hue. Empty cells hold a single small
  centred dot and nothing else. NO clip names, NO lengths, NO waveform
  thumbnails, NO rounded blocks.

  ONE cell is playing: a saturated rail down its left edge and its tag at full
  brightness. Two or three cells are selected: a slightly raised ground.

  A MASTER column pinned hard to the right edge, visually separate from the
  tracks by a wider seam.

COLOUR BY LANE, NOT BY DECORATION. Each track's clips carry that track's hue —
four or five distinct saturated colours across the grid, each one meaning a
kind of channel. This is not one tasteful accent; it is a legend. The only
non-lane colour is the playing rail, which is brighter than everything.

DENSITY. Sixteen rows and six columns of clips, plus the heads, should fill
the screen. The grid is the interface. Empty faceplate anywhere other than
inside an empty cell is a fault.

THE TRAY, a short band across the foot above the transport: the clip under the
cursor and its readout — STEP, POSITION, PITCH, LENGTH, VELOCITY, CHANCE,
NOTES, STATE — tiny grey labels over larger figures. Keep it shallow; the grid
gets the height.

THE TRANSPORT STRIP at the very bottom: bar, bpm, time signature as large
figures; rewind, stop, play, record, forward as five compact controls; a
status word. Nothing else — no sample rate, no buffer, no load, no xruns.

NOTHING ELSE ON SCREEN. No bus meters, no log, no mixer strip, no inspector,
no properties panel, no browser.

Panels meet at seams; no outer margin, no floating cards on a background.

FLAT DATA, PHYSICAL FRAME. The chassis — heads, tabs, transport — keeps the
recessed depth of the reference. The cells themselves are flat: no gradient
inside a clip cell, no gloss, no bevel. Depth belongs to the machine, not to
the data.

TYPE: one monospaced family for everything on screen. Tight, small,
unfashionable.

A screenshot of a tool in use, sharp, 1280x800, no mockup frame, no desk, no
hands, no reflections.
```

---

## How to judge what comes back

1. **Sixteen scene rows, not eight?** Density is the brief.
2. **Do clips show only their tag?** Any clip name, length or waveform means
   it drifted toward a commercial DAW.
3. **Does each column read as one hue?** Colour is a legend here, not
   decoration — if every clip is the same colour the grid tells you nothing.
4. **Any icons left anywhere?**
5. **Are the cells flat while the chassis keeps its depth?**
6. **Is the master column clearly separate from the tracks?**
7. **Is there bare faceplate anywhere outside an empty cell?**
