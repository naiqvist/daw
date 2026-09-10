# Restyle brief 2 — re-layout, not just re-skin

**Reference image (the current UI, what to change FROM):**
https://raw.githubusercontent.com/naiqvist/daw/ui-redesign/docs/restyle/workspace-session.png

---

## What to do

The first brief kept the layout frozen and changed only the surface. This one
does the opposite: **move things**. The reference shows where the app is today;
the prompt below describes where it should be. Follow the new arrangement
exactly — it is a specification, not a suggestion.

Return a 1280×800 image, and a link openable without an account.

---

## The prompt

```
Redraw this interface as a keyboard-driven music workstation — a groovebox
console, 1280x800, dark. Do NOT preserve the reference's arrangement. Rebuild
it to the layout below, in the material below.

THE NEW LAYOUT, top to bottom:

1. STATUS BAR, full width, thin (about 40px). Scope, track / scene / pattern
   numbers at the left; song name at the right. Nothing else.

2. PAGE KEYS, full width, about 48px. Eight cells edge to edge: F1 TRIG,
   F2 SRC, F3 FLTR, F4 AMP, F5 LFO, F6 FX, F7 MIX, F8. Each carries a small
   technical glyph and its word. Exactly one is lit.

3. THE BODY, everything between the keys and the foot, split two ways:

   LEFT RAIL, narrow — about 180px. The session: a stack of track plates, each
   with a number and a short name, and beneath each a column of small scene
   cells. This is a LOCATOR, not the main event. Dense, quiet, secondary.

   CENTRE, everything else — THE INSTRUMENT, and the largest thing on screen.
   A wide picture panel across the top of it: a waveform, a filter curve, a
   spectrum — an image of the sound being shaped. Beneath it, EIGHT PARAMETER
   CELLS in one row, each a numbered slab showing a name, a large value, and a
   fill bar. These eight cells are the focal point of the whole screen.

   THERE IS NO PANEL ON THE RIGHT. No mixer strip, no bus meters, no log, no
   inspector. That space belongs to the centre. This is the most important
   instruction here — the reference has a meter block top-right and it must be
   gone.

4. CLIP TRAY across the foot: a step grid, sixteen positions, some filled.

5. TRANSPORT STRIP, bottom, about 45px. Bar and bpm and time signature at the
   left; rewind / stop / play / record / forward centred as five square cells;
   ONE small round health indicator at the far right and nothing else. No
   sample-rate, buffer, load or xrun readouts.

EDGE TO EDGE. No outer margin, no page gutter, no floating cards on a
background. Panels meet each other at seams. This is hardware, not a document.

MATERIAL — Apple's glass, its readability specifically:
Frosted translucent panels over a near-black ground. Real depth: each panel
catches a single hairline of light along its top edge and falls into a soft
inner shadow at its foot. Blur what is behind a panel, never what is on it.
Text sits on glass at full contrast, never tinted by it. Crisp 1px edges, no
glow, no bloom.

ATTITUDE — bespoke technical underground, not corporate:
A machine somebody built for themselves, in a scene, on purpose. A
hand-finished hardware sequencer in a dark room: machined aluminium,
screen-printed legends, anodised panels, a little worn at the edges from use.
NOT a SaaS dashboard — no pillowy rounded cards, no drop shadows for their own
sake, no pastel, no decorative gradients, no illustration. Corners tight
(2-3px). Nothing softened to seem approachable.

COLOUR — quiet ground, loud state:
Near-black with a cold blue bias. Panels one step up, translucent. Field names
and static labels are NEUTRAL GREY, not coloured. Hue is reserved for things
that are LIVE: bright green for something sounding, hot orange for the thing
playing right now or holding focus, one saturated blue for the lit page key.
At most two or three coloured things on the whole screen. The screen should
read as quiet, with the live things leaping out of it.

TYPE:
Monospace throughout, tight and technical. Numerals aligned in columns.
Uppercase labels, letter-spaced, small. Parameter values large enough to read
across a room. Legibility outranks style at every size.

Photoreal UI render, sharp, 1280x800, no mockup frame, no desktop behind it,
no hands, no window chrome.
```

---

## Why each move

For judging the result, not for the generator.

| Move | Reason |
| --- | --- |
| Desk block deleted | It occupies the best real estate on screen and it is machine telemetry, not music. Bus meters are for when something is wrong, not while playing. |
| Session demoted to a rail | The pages are the instrument. Today the deck floats *over* the session and hides it; the most important surface should not be the one covering something else. |
| Engine readouts → one pip | `rate -- buf -- load -- xruns --` reads `--` almost always and sits on the same row as the transport. |
| Margins to zero | A 21px field margin reads as a document with a gutter. Hardware has panels that meet. Biggest change of character for the least work. |
| Labels neutral, hue for state | `label` blue is on every field name at once, so the one live thing competes with fifty. Reserve hue and the screen goes quiet. |
| Centre must fill | Today, when a clip takes focus the grid collapses and the middle of the screen is dead space. Something has to win it. |
