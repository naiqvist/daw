# Reusable phrase operations

Open a clip, press **Ctrl+Shift+P**, type a sentence, then Enter. `:` also
opens the palette when no text field owns focus. Each sentence is one undo
step. Results are ordinary editable notes and parameter locks, saved in the
existing project format. Nothing is a beat-specific preset or driver opcode.

## Time and contours

`division` is subdivisions per whole note: 4 = quarters, 16 = sixteenths,
64 = sixty-fourths. Triplet divisions such as 12, 24, and 48 also work.
Positions are zero-based, relative to the clip; range ends are exclusive.
The engine uses 48 ticks per quarter. Clip duration is not changed.

`gate`, `vel`, and `pitch` accept a repeating comma-separated cycle or a `start:end`
ramp over generated onsets. Pitch is in semitones relative to middle C for
new notes, or relative to the source notes for ratchets and `shape pitch`.

## Generate a motif and repeat it

    rhythm 16 0,3,6 every 8 gate 2 vel 110,85,98
    group 4 3,3,2 every 8 pitch 0,-2,-5
    legato 12

`rhythm` places listed offsets. `group` turns durations into successive
onsets and gates. `at N` offsets either motif; `every N` repeats it to clip
end. `gate N`, `gate 3,3,2`, or `gate 1:4` overrides note lengths in the chosen
division (fractional contour values are rounded to that grid). Explicit gates
may cross the clip boundary; `legato` caps them at that boundary.

`legato [overlap ticks]` connects selected onsets to the next selected
onset, or all onsets when there is no selection. Chord tones remain together.

## Build drills, then sweep them

    ratchet 64 0:64 spacing 4:1 gate 1 vel 45:115
    sweep cutoff 500:5000 2

The burst accelerates from sixteenth-note spacing toward sixty-fourths,
quantized to the chosen grid. `spacing 1:4` decelerates; `spacing 2` is
constant. A source note/chord at the range start supplies pitch, velocity,
mute state, sound, probability, condition, and locks. Explicit `vel` or
`pitch` overrides/transposes that source. Implicit retriggers become explicit
notes rather than being doubled. Conflicting shared-step rules are refused.

Select the fill's time cells before `sweep` to keep other beats untouched.
Without a selection, it spans the whole clip. Parameter names and limits
come from the track's instrument. `sweep cutoff 5000:500` reverses the sweep;
optional curve 1 is linear, above 1 delays the rise, below 1 advances it.
These are sliding parameter locks using the existing playback compiler.
Instrument and same-track effect parameters use the shared parameter registry;
for example `sweep room.mix 5%:25% at 0:96`. Discrete choices can be locked, but
cannot be swept.

## Combine gestures and automate their repetition

    rhythm 16 10,17,24 gate 8,8,5 pitch 11,6,2 vel 62,70,58
    lock ornament=murki;ornament_time=125ms;room.mix=20% at 96,192
    sweep cutoff=800Hz:4kHz;room.mix=5%:25% curve 2 at 0:96 every 192
    ratchet 64 192:256 spacing 4:1 gate 1 vel 30:100 sweep cutoff=8kHz:800Hz;room.mix=5%:25%

These are ordinary palette sentences, each with one undo boundary, not script
macros. A gate cycle eliminates rewriting each note just to change its duration.
A lock bundle addresses several parameters at the same time cells; choice names
and units use the same validation as `param`. Enabling a bypassed console section
is part of that same undo step. Aliases resolving to the same parameter are
rejected within a bundle.

Lock/sweep `at` and `every` are **ticks**, unlike the generator's division units.
Lock cells are 12 ticks wide. The sweep above occupies ticks 0 through 84, reaches
its final value at 84, then repeats at 192, 384, and 576 in a four-bar clip. Every
repeat has its own exact endpoints and slide flags; the endpoint does not slide
across the gap. Repeats must not overlap or overrun the clip. Without `at`, a lock
or sweep uses selected cells, otherwise the whole clip. `every` requires `at`.

An attached ratchet sweep inherits the burst's exact time window, independently
of any standing selection. In the example, division 64 means three ticks per
unit: the burst is ticks 576–768. Its window must align to 12-tick lock cells and
contain at least two cells. Attached sweeps cannot override `at` or `every`.
Ratchet gates are capped to the burst end, including gate cycles and ramps.

Bundles allow at most 64 distinct parameters and 4096 total time cells. A bad
late target refuses the entire operation: no partial notes/locks, enabled FX,
history entry, or audio revision. This works with any registered instrument or
same-track effect that supports live locks; parameters requiring a sample bake
remain explicitly unsupported.

## Audition a place without moving the edit cursor

    seek bar 97
    seek tick 768
    seek cursor

`go` navigates the editor; `seek` moves the playhead. Bars are one-based and
meter-aware; ticks are zero-based. Seeking preserves focus (including an open
tracker), the playback mode, and whether transport is rolling. It does not edit
the project or enter undo history. It uses the existing engine discontinuity
path, so voices from the old position are silenced; it does not preroll/rebuild
the FX history before the destination. Seeking is refused during recording.

## Reshape existing notes

    voice 0,7,11,16,19

`voice` replaces each selected onset with intervals above its lowest note,
resolved in the project key; with no selection it addresses the whole clip.
It retains the source gate, velocity and other note properties, and keeps
step locks. Up to sixteen distinct integer semitone intervals are accepted.
It works on any instrument: write a root first, then choose the voicing.

    shape vel 45:110
    shape pitch 0,7,12,7
    shape gate 6,12,24

These target selected time cells, otherwise the entire clip. Gate is in
ticks here. A chord consumes one contour position, not one per tone.
Other note properties and parameter locks are preserved.

Generators replace notes at addressed onsets, leaving other onsets alone.
Invalid sentences are atomic refusals. Sentences are limited to 4096 bytes
and generation to 1024 onsets to keep UI work bounded.
