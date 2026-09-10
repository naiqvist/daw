# Dark industrial — a professional application

**Reference image (the app today, four tracks, sequencer open on clip d0):**
https://raw.githubusercontent.com/naiqvist/daw/ui-redesign/docs/restyle/session-4-tracks.png

Dark mode only.

---

## The stance

This one should look like **a screenshot of a shipping professional
application at version 3.0** — something people sit in front of for eight
hours a day and have stopped noticing. Not a concept, not a pitch, not a
render of an object. A tool.

The reference world is the dark professional toolchain: Nuke, Houdini, DaVinci
Resolve, Bitwig, and the industrial control and data-acquisition software they
grew out of. Very dark neutral grey. Panels that tile edge to edge. Dense small
type. One muted accent used sparingly. Nothing decorative anywhere.

Practical note: this is also the only one of these directions the app can
render today, since it needs no translucency, no blur and no line work.

## What the three previous attempts got wrong

Baked in below as explicit rules, because each was produced by a generator
following a brief that failed to forbid it.

- **Cards.** Every attempt reconstructed regions as outlined boxes with printed
  title captions — SESSION, SEQUENCER, TRANSPORT. That is the single most
  application-shaped mistake available and it must be banned outright.
- **Hatching or texture for values.** Velocity drawn as stacked rules is
  impossible to compare at a glance. A value is a bar whose HEIGHT is the
  value.
- **Uniform weight.** One ink at one brightness leaves the eye nowhere to land.
- **Emptiness.** The app today wastes 67% of its field.

---

## The prompt

```
Draw the main working view of a professional music production application —
a groovebox sequencer, 1280x800, DARK MODE ONLY.

IT MUST LOOK LIKE A SHIPPING PROFESSIONAL TOOL AT VERSION 3.0. The reference
world is the dark professional toolchain — Nuke, Houdini, DaVinci Resolve,
Bitwig — and the industrial control software behind it. Something an engineer
uses for eight hours a day and has stopped noticing. NOT a concept render, NOT
a product shot, NOT a landing page, NOT a plugin, NOT a technical drawing.

EIGHT RULES:

1. PANELS TILE, THEY DO NOT FLOAT. Every region meets its neighbours directly.
   No gaps between panels, no margin around the outside, nothing sitting on a
   background. The window is completely filled.

2. NO CARD HEADERS. Regions are NEVER announced by a printed title in a box —
   no "SESSION", no "SEQUENCER", no "TRANSPORT" captions. A region is known by
   where it is and what it is made of. Section labels, where genuinely needed,
   are tiny grey text in a corner, not a header bar.

3. SEPARATION IS A ONE-PIXEL SEAM. A single dark rule between panels, and a
   single light rule where a panel's edge catches. Never a box outline around
   a region.

4. THREE GROUND LEVELS AND NO MORE. Chrome, darkest, for toolbars and rules.
   Panel, one step up, the working surface. Field wells, recessed below the
   panel, for anything holding a value. That is the whole depth system: no
   drop shadows, no glow, no gradients beyond a barely perceptible top-to-
   bottom shade on a panel.

5. VALUES ARE BARS, NOT TEXTURES. Anything showing a magnitude is a solid
   filled bar whose LENGTH OR HEIGHT is the magnitude, comparable at a glance
   across neighbours. Never hatching, never stippling, never a stack of rules.

6. ONE MUTED ACCENT: amber. Used only for what is live or selected right now,
   and desaturated enough to sit in the same room as the greys. Everything
   else is neutral. Semantic red exists for a fault and appears nowhere on
   this screen.

7. DENSE, SMALL TYPE. One monospaced or narrow technical sans, around 11 to
   13 pixels for labels and 13 to 16 for values, with a few large figures for
   the transport only. Labels are grey and quiet; values are near-white. A
   professional tool is packed with information and trusts its user to read
   it.

8. SQUARE. Two pixels of corner radius at most, and zero is better.

PALETTE:
Chrome #1b1d1f. Panel #232629. Well #17191b. Seams: #101112 dark and #35393d
light. Labels #8a9199. Values #e8ecef. Accent: amber #d4922f, and nothing else
saturated on screen.

LAYOUT — approximate bands, adjust by up to about forty pixels where the
composition is better for it:

y 0-32 — A THIN TOOLBAR. Left: scope word, track number and name, pattern
number. Right: song name. Small grey text on chrome. One seam beneath.

y 32-76 — A TAB STRIP: eight tabs, TRIG SRC FLTR AMP LFO FX MIX SET, each
with a small monochrome glyph and its key number. The active tab is panel-
coloured and flush with the panel below it, with a two-pixel amber underline;
the inactive tabs are chrome-coloured and recessed. This is how a professional
tool shows a tab, and it must read that way.

y 76-756 — THE WORK AREA, split at about x=230:

  x 0-230 — THE SESSION PANEL. A compact table: four track rows named KICK,
  BASS, KEYS, HAT, each with eight scene slots. Filled slots carry a short
  clip name on a slightly raised ground; empty slots are wells. One slot is
  amber with a small play triangle: the clip running. Row striping, alternate
  rows a hair lighter. This panel is SECONDARY — quiet, dense, narrow.

  x 230-1280 — THE SEQUENCER. The largest region on screen by a wide margin.
    A thin strip along its top: clip name, length in bars, step resolution,
    note lens, as label/value pairs in small text. No header bar.
    Below it, SIXTEEN STEP COLUMNS filling the full width and most of the
    height, each about 65 pixels wide, separated by one-pixel seams. Each step
    is a recessed well and they ALL DIFFER:
      - about six are empty wells, showing only their step number
      - filled ones show a note name and a SOLID VERTICAL VELOCITY BAR rising
        from the bottom of the well, each a different height
      - two carry a small filled square in the corner: a parameter lock
      - one has a short horizontal line running to the next step: the lock
        slides
      - one shows "3:4" in small figures: a condition
      - one shows three narrow vertical bars: a retrig
      - one has a differently-toned well and a small badge: a sound lock
      - one column is amber-tinted end to end: the playhead is there
    A step ruler along the top numbered 1 5 9 13.
    At the foot of the sequencer, a single row of eight label/value pairs —
    STEP, POSITION, PITCH, LENGTH, VELOCITY, CHANCE, NOTES, STATE — labels
    tiny and grey above near-white values, each pair in its own well.

y 756-800 — THE TRANSPORT BAR. Chrome-coloured. Left: bar, bpm, time
signature as large near-white figures with tiny grey labels. Centre: five
compact square buttons — rewind, stop, play, record, forward — the active one
amber. Right: one small round status dot. Nothing else: no sample rate, no
buffer, no load, no xrun counters.

NO PANEL ON THE RIGHT-HAND SIDE. No mixer strip, no meters, no inspector, no
log, no properties panel. The sequencer owns that space.

Fill the window completely. Bare background is a fault.

A screenshot of a professional application, sharp, 1280x800, no mockup frame,
no desktop behind it, no browser chrome, no hands, no reflections, no drop
shadow around the window.
```

---

## How to judge what comes back

1. **Would you believe it is a screenshot of shipping software?** That is the
   whole test. If it looks like a concept, it failed.
2. **Any region announced by a title caption in a box?** Cards. Reject.
3. **Are the velocity bars comparable at a glance?** Solid bars of differing
   height, not texture.
4. **Do the panels tile with no gaps and no outer margin?**
5. **Is the active tab flush with the panel below it?** That single detail is
   what separates a real tab strip from eight buttons in a row.
6. **Sixteen steps, all different?**
7. **Is amber the only saturated colour, and on at most three things?**
8. **How much bare ground is left?** The reference wastes 67%.
