# Hardware frame, tracker guts

**Reference — the current result. Its FRAME is right; its DATA is too polite:**
https://raw.githubusercontent.com/naiqvist/daw/ui-redesign/docs/restyle/underground-v2.png

Dark mode only.

---

## Why the last one still reads "pro audio"

Not a failure of execution. A failure of reference.

A dark faceplate with one amber accent, silkscreen legends and a soft gradient
on every well **is** the 2020s boutique-synth look — Elektron, Erica, Make
Noise, Arturia. It is underground-adjacent, and it is still a **product**:
something designed to photograph well on a retailer's page.

Four things keep it there:

- **Icons on the tabs.** A software convention. Instruments have words.
- **A gradient on every surface.** Premium injection-moulded plastic. That is
  product design, and it is doing no work.
- **Perfect regularity.** Every cell the same, every gap identical.
- **Politeness.** Nothing on the screen is dense, odd, or unwelcoming.

## The way out

Stop referencing hardware products. Reference **the software the scene
actually uses: trackers** — Renoise, Impulse Tracker, Sunvox, Furnace.

Trackers look like nothing a product designer made. Dense monospaced grids.
Hexadecimal. No icons anywhere. Saturated colour used per data type rather
than one tasteful accent. Rows of numbers that assume you already know. They
are unfashionable, information-dense, hostile to newcomers, and completely
beloved — which is what underground actually means.

**The hybrid this brief specifies:** keep the hardware faceplate for the
FRAME — tabs, transport, session, the physical depth that made it readable —
and make the SEQUENCER a tracker. A machine whose chassis is an instrument and
whose screen is a tracker. That combination does not exist, and it is honest
to what this app is.

**The trade, stated plainly:** trackers are flat and text-first. Some of the
depth that made the reference readable goes away inside the data region. In
exchange the screen holds several times more information, which is the thing
this app actually needs.

---

## The prompt

```
Here is a music instrument's interface. Its FRAME is right — the dark faceplate,
the recessed depth, the transport, the overall shape. Keep the frame.

Its DATA region is wrong: it reads as a commercial hardware synth, and this is
meant to be an underground tool. Replace the sequencer with a TRACKER.

1280x800, dark mode only.

THE REFERENCE FOR THE DATA IS A TRACKER: Renoise, Impulse Tracker, Sunvox,
Furnace. Dense monospaced grids. Hexadecimal. No icons. Rows of numbers that
assume the user already knows what they mean. Not a product; a tool that
nobody tried to make friendly.

KEEP FROM THE FRAME:
- The dark, nearly black faceplate and its physical recessed depth.
- The tab strip across the top — BUT REMOVE EVERY ICON. Words and numbers
  only: TRIG, SRC, FLTR, AMP, LFO, FX, MIX, SYS, each with its key number.
- The transport strip at the foot, with its large figures.
- The session panel at the left.
- No branding anywhere.

REPLACE THE SEQUENCER WITH A TRACKER GRID:

Instead of sixteen step cells, a VERTICAL TRACKER: steps run DOWNWARD as rows,
four track columns run across, and every row is a line of monospaced text. This
is the single biggest change and it is the point of the brief.

Roughly thirty-two rows visible at once, numbered down the left in hexadecimal
— 00 01 02 03 ... 0F 10 11 ... 1F — in dim grey, with every fourth row's number
brighter and the row itself very slightly lighter, the way a tracker marks its
beats.

Each track column carries fixed-width fields, printed as text and aligned:

    NOTE  INST  VOL  FX
    C-4    01   40   C08
    ---    --   --   ---
    D#4    01   3A   ---
    OFF    --   --   ---

Empty values are dashes. A note-off is OFF. Nothing is drawn as a shape.

COLOUR BY DATA TYPE, NOT BY STATE. This is how a tracker reads and it is the
opposite of one tasteful accent. Each FIELD has its own saturated hue, the
same hue everywhere it appears:
  - note names: pale cyan
  - instrument numbers: dim green
  - volume: amber
  - effect commands: magenta
  - row numbers: grey, brighter every fourth
  - the row under the playhead: a full-width saturated band with the text
    reading through it
Five or six saturated colours on screen at once, all of them meaning something.
This will look garish beside the reference, and that is correct.

NO GRADIENTS INSIDE THE TRACKER. The grid is flat. Depth belongs to the frame
around it, not to the data.

DENSITY IS THE POINT. The grid should be tight — small type, close leading,
no padding inside cells, columns separated by a single space rather than a
rule. It should look like it is holding as much information as it possibly
can, because that is what these tools do.

A header line above the columns naming the tracks: KICK BASS KEYS HAT, in the
same monospaced type, with the fields labelled beneath in tiny grey caps.

KEEP A SMALL PICTURE PANEL — the filter curve over its spectrum — but shrink
it to a narrow strip along the top of the data region, no taller than about a
sixth of the screen. In a tracker the numbers matter more than the picture.

KEEP the readout row of label/value pairs at the foot: STEP, POSITION, PITCH,
LENGTH, VELOCITY, CHANCE, NOTES, STATE.

TYPE: one monospaced family for absolutely everything, including the frame.
Bitmap-like, tight, unfashionable. No proportional type anywhere on screen.

A screenshot of a tool in use, sharp, 1280x800, no mockup frame, no desk, no
hands, no reflections.
```

---

## How to judge what comes back

1. **Is the sequencer vertical, with steps running downward as text rows?**
   If it is still sixteen cells across, the brief was not followed.
2. **Are there five or six saturated colours, each meaning a data type?** One
   tasteful accent means it stayed a product.
3. **Any icons left on the tabs?**
4. **Is the grid flat?** Gradients inside the data region mean the
   boutique-synth reference crept back.
5. **Does it look like it is holding as much information as it can?**
6. **Does the frame still have its physical depth?** That was the good part and
   it should survive around the outside.
7. **Would a product manager ask you to soften it?** If yes, it is right.
