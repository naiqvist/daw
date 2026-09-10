# Restyle brief — glass rave console

Reference image: [`workspace-session.png`](workspace-session.png) — the stage's
session view at 1280×800, rendered headless by `shot`. Real pixels, not a drawing
of them.

Direction: Apple's glass **readability**, none of its boardroom manners.

## The prompt

```
Restyle this interface. It is a keyboard-driven music workstation — a
groovebox console, 1280x800, dark. Keep every element exactly where it is;
change only the material, the palette and the type.

KEEP, UNCHANGED:
- the top status bar, and the row of eight function keys F1-F8 beneath it
- the track head plates across the field, and the scene grid stacked under them
- the DESK meter block and log at the right
- the clip tray across the foot, and the transport strip below it
- every readout, label and numeral in place — this is an instrument, not a poster

MATERIAL — Apple's glass, its readability specifically:
Frosted translucent panels floating over a near-black ground. Real depth: each
panel catches a single hairline of light along its top edge and falls into a
soft inner shadow at its foot. Blur what is behind a panel, never what is on
it. Text sits on glass at full contrast, never tinted by it. Crisp 1px edges,
no glow, no bloom.

ATTITUDE — bespoke technical underground, not corporate:
This is a machine somebody built for themselves, in a scene, on purpose. Think
a hand-finished hardware sequencer in a dark room — machined aluminium,
screen-printed legends, anodised panels, a little worn at the edges from use.
NOT a SaaS dashboard: no pillowy rounded cards, no drop shadows for their own
sake, no pastel, no decorative gradients, no friendly illustration. Corners are
tight (2-3px), edges are honest, nothing is softened to seem approachable.

PALETTE:
Near-black ground with a cold blue bias. Panels one step up, translucent. Hue
carries meaning and must stay legible as a system: cool blue for a field's
name, bright green for something sounding or within budget, hot orange for the
thing playing right now or holding focus, and exactly one saturated accent on
screen at a time. High chroma on the accents, near-neutral everywhere else —
the contrast between the two is the whole look.

TYPE:
Monospace throughout, tight and technical. Numerals aligned in columns.
Uppercase labels, letter-spaced, small. Legibility outranks style at every size
— if a choice costs readability, it is the wrong choice.

Photoreal UI render, sharp, 1280x800, no mockup frame, no desktop behind it,
no hands, no window chrome.
```

## What it starts from

Measured off the reference at its own raster, not chosen.

| role | value | carries |
| --- | --- | --- |
| ground | `#0a1a31` | the field |
| panel | `#19314b` | cells, pads |
| tab | `#1564c0` | the lit page |
| label | `#4ab6c3` | a field's name |
| nominal | `#1df4a2` | sounding |
| alert | `#e38344` | playing now |

| band | px |
| --- | --- |
| title row | 42 |
| key row F1–F8 | 48 |
| key cell | 151.8 × 43 |
| key margin / gap | 13 / 6 |
| lattice row pitch | 28.4 |
| clip tray | 182 |
| transport strip | 45 |

## Why the KEEP block is load-bearing

Without it an image model redesigns the layout and returns a poster rather than
something traceable. If the result comes back with the key row moved or the desk
block reshaped, that is the failure mode — regenerate rather than work from it.

When a result comes back, run `tools/trace.py auto` on it before building
anything: it reports the palette, bands, cell geometry, borders and radii it
actually contains, and whether the layout survived.
