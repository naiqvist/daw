# Keep the depth, lose the boardroom

**Reference image — KEEP ITS DEPTH AND ITS READABILITY:**
https://raw.githubusercontent.com/naiqvist/daw/ui-redesign/docs/restyle/apple-original.png

**Second reference — what the app actually contains:**
https://raw.githubusercontent.com/naiqvist/daw/ui-redesign/docs/restyle/session-4-tracks.png

Dark mode only.

---

## The brief in one line

The reference is the most readable thing anyone has produced for this app. Its
depth, its hierarchy and its contrast are **correct and must survive**. What is
wrong with it is cultural: it reads as a corporate VST, and this is meant to be
an underground musical instrument.

So this is not a redesign. It is a change of **register**.

## What reads corporate, and what reads underground

The difference is almost never structural. It is in the signals.

| Corporate VST | Underground instrument |
| --- | --- |
| Blue and teal — the technology-company palette | Anodised black, white silkscreen, one sodium colour |
| Perfectly even, perfectly symmetrical | One thing deliberately off, and it looks intended |
| Generic labels: Cutoff, Resonance, Drive | This machine's own words, terse, assuming competence |
| Everything captioned so nobody is lost | Nothing explained; the user already knows |
| Glossy panels, decorative gradients | Matte surfaces, honest material, a little worn |
| Uniform rounded rectangles for everything | One shape for one job; edges where edges belong |
| A brand mark, product-shot polish | No logo anywhere. It is a tool, not a product |
| Wide, friendly, well-tracked sans | Type borrowed from hardware: silkscreen, stencil, label tape |

---

## The prompt

```
Two reference images. The FIRST is a UI whose DEPTH, HIERARCHY AND READABILITY
are correct — study it and keep those. The SECOND shows what this application
actually contains.

Redraw the first image's screen as a professional music instrument, 1280x800,
DARK MODE ONLY. The problem to solve: the reference reads as a CORPORATE
SOFTWARE PLUGIN, and this is meant to be an UNDERGROUND MUSICAL INSTRUMENT —
the kind of machine that gets used in a basement at 4am, not demonstrated at a
trade show.

KEEP, EXACTLY — these are the reference's strengths:
- Three levels of elevation, clearly readable: recessed wells, the flush
  surface, and raised elements. Depth is what makes it legible and it stays.
- Large confident values. A parameter's number is big enough to read at arm's
  length, with a small quiet label above it.
- Strong contrast: near-white values, mid-grey labels, dark ground.
- Colour restraint: almost everything neutral, so the few coloured things
  actually mean something.
- The picture panel showing a real curve over a real spectrum, with real
  scales on both axes.

CHANGE — these are what make it corporate:

1. THE PALETTE. Remove every blue and teal; that is the technology-company
   palette and it is the loudest corporate signal on the screen. Replace with
   ANODISED BLACK — a deep neutral with the faintest warm cast, like a
   powder-coated aluminium faceplate — WHITE SILKSCREEN for legends, and ONE
   sodium-amber for anything live. Nothing else is coloured. Ever.

2. THE TYPE. Not a user-interface font. Legends are set the way they are
   SILKSCREENED ONTO HARDWARE: narrow, letter-spaced, small, slightly
   condensed, in white. Values are large tabular figures. It should look
   printed onto a panel, not rendered by a toolkit.

3. THE LABELS. Terse. No captions explaining regions, no help text, no
   redundant words. This machine assumes its user knows what they are doing.
   Where a label can be three characters instead of a word, it is.

4. SYMMETRY. Break it exactly once, deliberately and confidently — one region
   sized or placed against the grid, so the layout looks decided by a person
   rather than divided by a machine. Everything else stays aligned.

5. WEAR. The faceplate has been used. Not dirt and not damage — the faintest
   unevenness in the finish, silkscreen very slightly imperfect, the sense of
   an object with a history. Subtle enough that you would not name it, present
   enough that the surface is not sterile.

6. NO BRANDING. No logo, no product name, no version badge, no marketing
   flourish anywhere.

7. ONE STRANGE THING. Every good instrument has a control that makes no sense
   until you use it. Include one element that is clearly deliberate and
   slightly unusual — an unexpected readout, an oddly specific unit, a
   control that is not shaped like its neighbours.

WHAT THE SCREEN CONTAINS — from the second reference, all of it:
- A thin top strip: scope, track number and name, pattern number, song name.
- Eight page tabs: TRIG, SRC, FLTR, AMP, LFO, FX, MIX and an eighth. One
  active, marked with sodium-amber.
- A narrow session panel: four tracks named KICK, BASS, KEYS, HAT, each with
  eight scene slots, filled or empty. One slot amber — the clip playing.
- THE SEQUENCER, the largest thing on screen: sixteen steps, and they MUST
  ALL DIFFER — about six empty, the rest carrying a note name and a SOLID
  VELOCITY BAR of differing height, two with a lock mark, one with a lock
  sliding to the next, one marked "3:4" for a condition, one with three fine
  bars for a retrig, one differently toned for a sound lock, and one lit
  amber where the playhead is.
- A picture panel: a filter curve over a spectrum, dB scale up the left, Hz
  along the foot.
- A readout row: STEP, POSITION, PITCH, LENGTH, VELOCITY, CHANCE, NOTES,
  STATE — small labels over large figures.
- A transport strip: bar, bpm, time signature; rewind, stop, play, record,
  forward; one status indicator. Nothing else — no sample rate, no buffer,
  no load, no xruns.
- NOTHING on the right-hand side. No mixer, no meters, no inspector, no log.

Panels meet each other; no outer margin, no floating cards on a background.

A photograph-sharp screenshot of an instrument in use, 1280x800, no mockup
frame, no desk, no hands, no reflections, no drop shadow around the window.
```

---

## How to judge what comes back

1. **Is it still as readable as the reference?** If depth or contrast was lost
   chasing character, it failed — that is the one thing that was already right.
2. **Any blue or teal left?** That single change does more than the rest
   combined.
3. **Would you believe a person decided the layout?** Find the one deliberate
   asymmetry. If everything is perfectly even, it will still read corporate
   whatever the palette.
4. **Is there a logo or product name anywhere?** Remove it.
5. **Sixteen steps, all different?**
6. **Can you find the one strange thing?** If not, it played safe, and safe is
   what corporate means.
