# Four fixes — keep everything else

**Reference image — THE LOOK IS CORRECT. Change only what is listed below:**
https://raw.githubusercontent.com/naiqvist/daw/ui-redesign/docs/restyle/underground-v1.png

Dark mode only. This is a **targeted edit**, not a new direction.

---

## What is already right and must not change

The register landed. Do not restart, do not reinterpret, do not improve
anything not named in the four fixes.

- Anodised black faceplate, white silkscreen legends, one sodium amber for
  live. No blue anywhere. Keep this palette exactly.
- The depth: recessed wells, flush faceplate, the raised active tab.
- The tab strip, with the active tab amber and flush with the surface below.
- The session table at the left, four named tracks against eight scene rows,
  with the running clip in amber.
- The readout row across the foot: STEP, POSITION, PITCH, LENGTH, VELOCITY,
  CHANCE, NOTES, STATE, small labels over large figures.
- The transport strip and its ROLLING indicator.
- The DRIFT control with its 17 ms readout. Keep it. That is the one strange
  thing and it is doing its job.
- Every per-step marking already present: the lock, the slide arrow, the 3:4
  condition, the retrig glyph, the S for a sound lock.
- No branding, no logo, no product name.

---

## The four fixes

### 1. THE SIXTEEN STEPS ARE ONE ROW, NEVER TWO

This is the important one. The reference wraps them into two rows of eight.
A sequencer's steps are a **timeline**, and a timeline is a single line read
left to right. Wrapping it turns time into a paragraph.

Sixteen columns in one unbroken row, spanning the full width available to the
sequencer. Each column becomes narrow and tall instead of wide and short —
that is correct and expected.

### 2. THE STEPS ARE A STRIP, NOT SIXTEEN LITTLE CARDS

In the reference each step is its own rounded panel with its own outline and
its own internal padding. That is sixteen cards in a row, and it is the
corporate pattern returning by the back door.

Instead: one continuous recessed strip, divided into sixteen cells by
single-pixel seams. No outline around any individual step. The only step with
an edge is the one under the playhead.

### 3. THE PICTURE PANEL GOES ABOVE THE STEPS AND GETS BIGGER

In the reference the filter curve sits below the steps and is squeezed. It is
the largest piece of information on the screen and should sit directly beneath
the tab strip, spanning the sequencer's full width, roughly twice its current
height. The steps sit under it.

Order, top to bottom, in the sequencer region: picture panel, then the single
row of sixteen steps, then the readout row.

### 4. VELOCITY IS A BAR THAT RISES FROM THE FLOOR

The reference shows velocity as a small grey square of varying size, which is
hard to compare between neighbours. Make it a solid vertical bar rising from
the bottom of each step cell, its HEIGHT the velocity, so sixteen values can
be scanned in one glance the way a level meter is read. The number stays
beneath it.

---

## The prompt

```
Here is a screenshot of a music instrument's interface. Its look is CORRECT —
the anodised black faceplate, the white silkscreen legends, the single sodium
amber for live state, the recessed wells, the depth, the type, the session
table, the readout row, the transport, the DRIFT control. Keep ALL of it
exactly as it is.

Redraw it with FOUR changes and nothing else. Do not reinterpret the style. Do
not improve anything I have not asked for. 1280x800, dark mode only.

1. THE SIXTEEN STEPS BECOME ONE UNBROKEN ROW, not two rows of eight. A
   sequencer is a timeline and a timeline is one line read left to right.
   Sixteen columns spanning the full width of the sequencer area. Each column
   is narrow and tall rather than wide and short.

2. THE STEPS ARE ONE CONTINUOUS RECESSED STRIP divided into sixteen cells by
   single-pixel seams — NOT sixteen separate panels each with its own outline
   and padding. No individual step has a border. Only the step under the
   playhead is outlined, in amber.

3. THE FILTER CURVE PANEL MOVES ABOVE THE STEPS and roughly doubles in height.
   It spans the full width of the sequencer area and sits directly under the
   tab strip. Order top to bottom: tab strip, curve panel, the row of sixteen
   steps, the readout row, the transport.

4. VELOCITY BECOMES A SOLID VERTICAL BAR rising from the floor of each step
   cell, its height being the velocity, so all sixteen can be compared at a
   glance like a row of level meters. The numeric value stays beneath the bar.

Everything else is unchanged: the same palette, the same silkscreen type, the
same session table at the left with its amber running clip, the same per-step
markings — the lock, the slide arrow, the 3:4 condition, the retrig glyph, the
S for a sound lock — the same readout row, the same transport with ROLLING,
the same DRIFT readout, no branding.

A photograph-sharp screenshot of an instrument in use, 1280x800, no mockup
frame, no desk, no hands, no reflections.
```

---

## How to judge what comes back

1. **One row of sixteen?** If it wrapped again, everything else is moot.
2. **Is the step strip continuous, with no outline on individual steps?**
3. **Is the curve above the steps and about twice as tall?**
4. **Are the velocity bars comparable at a glance?**
5. **Did anything else change?** If the palette shifted, the type changed, or
   DRIFT disappeared, it reinterpreted instead of editing — reject and repeat
   the prompt, since the look was already right.
