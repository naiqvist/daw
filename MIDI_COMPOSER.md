# MIDI Lab composer

This worktree implements the updated MIDI Lab plan in
`notes/20260909-midi-lab-plan-v2.md`. Run `cargo run --bin stage -- --midi-lab`.
New drafts open the composer; existing legacy drafts retain their original
generator. Their **Composer** action first preserves the old notes as a snapshot.
**Adopt saved notes** turns that snapshot into editable absolute motifs.

## Start a composition

1. Select **Chord**, then enter a progression such as
   `Cmaj7:4 Am7:4 Dm7:4 G7:4`. Durations here are quarter-note beats; an explicit
   tick suffix such as `Cmaj7:193t` preserves subdivision edits exactly.
2. Choose **Melody**, enable the voice, and choose its rhythm, contour, movement,
   target roles and development. Each voice has its own register and rhythm.
3. Choose **Bass**, enable it, and select a role and style. Foundation anchors the
   harmony, walking connects quarter-note arrivals, riff repeats a pattern,
   pedal holds its identity, melodic takes a more active part, and sub leaves
   longer spaces. Explicit slash bass and actual altered chord members matter.
4. Use **Play** to audition the phrase on the destination instrument. **Hear**
   isolates the selected harmony. **Send** writes the displayed composition to
   its assigned clips as one undoable operation, extending the complete timeline.

The subject inspector holds the controls; **More controls** exposes the advanced
ones. **Find** searches across subjects. Arrows move through controls;
Shift+arrows edit values; Enter opens a picker, edits text or performs the selected
action. Escape closes a picker or text edit. Numeric values also drag horizontally.
**F6** switches focus to the score. There, arrows select notes/voices,
Shift+arrows move notes, and Enter opens the note inspector.

## Chords without a vocabulary limit

Material can be a conventional name, an unnamed collection, or exact written notes:

| Entry | Meaning |
| --- | --- |
| `C7b9#9` | Simultaneous altered ninths |
| `Cmaj7/E` or `Cmaj7/E2` | Independent bass class or exact bass pitch |
| `pc:C,Eb,F#,A` | Pitch classes with deterministic register assignment |
| `notes:Cb4,B3,E#5` | Exact octaves, spellings and independent unisons |
| `int:C4:0,1,6,14` | Semitone offsets from an exact reference |
| `D\|C` | Named upper/lower polychord components |
| `rest` | Silence |

The twelve chips edit membership directly. Every nonempty 12-TET subset is legal
when the register accommodates it. Naming is optional: C–E–G–A can have C6 and Am7
readings without changing its exact notes. A key supplies Roman numerals, modes,
custom scale intervals, borrowing, cadences, substitutions and modulation. Turning
tonal context off keeps arbitrary material available.

Exact octaves stay fixed. Layouts such as open, drop, shell, rootless, quartal and
upper structure operate on the actual members; incompatible fixed pitches or
ranges produce an explanation. The voicing inspector exposes the selected
candidate, motion/spacing costs and a runner-up. Optional harmonic profiles apply
their generative policies without prohibiting explicitly written arbitrary notes.

Enable **Capture MIDI notes**, play the notes, and use **Use captured chord** to
commit the exact pitches. **Lift written clip notes** captures the primary clip as
a motif. Lifting requires exact 12-TET notes and straight, unconditional timing;
swung, rescaled, conditional or retriggered performances require a rendered note
take before import. Unsupported input is reported instead of silently converted.

## Develop melodies and bass

**Explore alternatives** constructs a bounded set of musically different proposals
from the chosen voice. Actions preserve a chosen aspect: rhythm, pitches, opening,
targets or a bass fill. Other actions simplify, increase range, develop a motif or
leave more space. Candidate descriptions report the musical differences.
**Use candidate** commits it. Changing the source invalidates old proposals.
**Save comparison**, **Compare A / B** and **Recall comparison** keep alternatives
available without hidden reseeding.

Capture a phrase as a motif, choose an absolute, chromatic, diatonic or chord-role
pitch frame, and place it along the timeline. Transforms include transposition,
inversion, retrograde, exact time scaling, rotation, fragments and sequences.
An incompatible transform is refused. The placement document is editable RON;
overlapping placements of the same voice must be resolved explicitly.

Bass uses declared arrivals and approaches, phrase-end fills, optional pedal and
open-string constraints. **Copy kick attacks** accepts a clip tag and note number,
for example `b0:36`; it snapshots that pattern's attacks. Reinforce, answer and
independent relationships use those stored attacks. **Audition drum clip** includes
the referenced clip on its separate instrument track. **Space under melody**
reduces competing attacks. Register/spacing preferences describe note relationships;
the resulting mix still depends on the chosen instruments.

The rhythm subject includes named cells, Euclidean pulses, swing, rotation,
timing and velocity offsets. Written cells use `start/length`, in ticks, with `~`
for a tie into the next same-pitch attack. A tie produces one sustain. Drag the thin
rhythm rail to write a custom cell. Drag note bodies to move pitch/time, drag their
right edges to change duration, and double-click empty piano-roll space to insert a note.
Curve handles edit contour, velocity, gate and form tension; double-click to add
a point and right-click a handle to remove it.

The note inspector reports the source, rule, target and transformations. Pitch,
timing, duration and velocity overrides are independent. Lock a note, selected
phrase or voice to protect it while exploring. An incompatible lock produces a
refusal; it is never silently dropped.

## Counterpoint and form

Counterpoint follows an explicitly selected leader. Literal canon preserves the
complete delayed tail. First through fifth species use bounded tonal construction
and final checks for their declared consonance, passing, preparation, motion and
invertibility rules. These are constrained musical profiles; infeasible ranges,
timings and locked dissonances may require changing the inputs. **Enforce rules**
controls whether applicable final violations refuse output or become findings.

Create source sections, reference them with chromatic or diatonic transposition,
choose enabled voices, and enter their names in **Form order**. Source edits flow
through references; edits made on the expanded score stay local to that occurrence.
Section boundaries must contain complete harmony and note spans. The tension curve
can influence velocity, register, density and gate through explicit amounts.

## Routing, persistence and limits

Voice destinations use stable track/clip identities. **Separate voice tracks**
clones the primary instrument for enabled voices and allocates additional tracks
for independent same-pitch overlaps; repeating the action reuses those routes.
Faithful output refuses overlaps a shared note-addressed instrument cannot preserve.
**Merge coincident notes** is an explicit conversion: its event changes are shown
in the recipe inspector. Preview uses the same conversion as Send. MIDI export
uses separate melodic channels where independent unisons require them.

Save writes a versioned RON document with its full event snapshot and comparison
bank; export writes a type-1 MIDI file with tempo and metre. Tempo at the primary
clip's arrangement position and subsequent tempo changes are used for both preview
and export. A snapshot from an unknown generator version remains playable without
being silently regenerated.

Current explicit capacities: 48 ticks per quarter note; 49,152 ticks per timeline
(256 bars of 4/4); 32,768 generated events; 128 material members; 32 comparison
snapshots; 32 MiB recipe files; and 15 independent melodic MIDI channels. The voicing
solver considers at most 32 candidates per chord with a receding two-chord horizon
and a 262,144-transition limit. Species construction uses a 16-path beam and a
262,144-transition limit. Alternatives examine 48 proposals and retain at most 12.
These searches are deterministic within their declared version and bounds; they
do not claim global optimality. Unsupported timing ratios, techniques and resource
requests produce explicit errors. No dependencies or realtime callback algorithms
were added.

## Review material

`cargo run --example midi_composer_fixtures -- /tmp/midi-fixtures` creates authored
RON, MIDI and stereo WAV examples for chord leading, three melody constructions,
and foundation/walking/riff/pedal/sub bass with harmony. These support listening
review; automated correctness tests do not substitute for musical judgment.

`cargo test --lib` includes the 4,095-mask sweep, exact-note preservation, legacy
snapshots, scoped alternatives, bass/counterpoint fixtures, small exhaustive
voicing reference, long-pattern compilation, real pointer gestures, atomic
Send/Undo, MIDI capture and audible offline rendering.
