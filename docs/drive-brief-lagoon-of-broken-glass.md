# Drive brief — Lagoon of Broken Glass

Status: v1 composition and native TAKE completed; workflow improvements authorised
and implemented. Human listening acceptance is still pending. See the
[implementation and measured retest](lagoon-workflow-retest.md).

## The prompt

Create an original eight-track composition called **Lagoon of Broken Glass**:
an impressionist nocturne carried by a physical dancehall groove, with intricate
IDM drum programming and an evolving electronic interior. Draw inspiration from
Debussy's harmonic colour and floating voices, Aphex Twin's mixture of tenderness
and abrasive rhythmic detail, and dancehall's bass-led movement and negative
space. Do not quote an existing melody or reproduce a named recording or riddim.
The result should feel like one piece, not three genre demonstrations pasted
together. The central test: it must remain beautiful and danceable when the
listener stops paying attention to the technical tricks.

Use exactly **eight musical tracks**, 108 BPM, 4/4, and 128 bars, followed by
approximately 12–16 seconds of natural release. Shared buses and instrument-owned
FX branches do not count as additional musical tracks. No hidden ninth musical
layer in a premixed backing file. Make the complexity come from relationships,
articulation, spectral motion, and arrangement—not simply from adding notes.

Use the native instruments and the samples already in the DAW's sample folder.
Record sample identities and paths. Do not download a new sample pack. Keep the
project editable; a final bounce is a deliverable, not a replacement for the
instruments, routing, locks, and arrangement that made it.

### Harmonic and rhythmic spine

Establish a small, memorable five-note cell, initially **A–B–E–D–C**. Derive the
main melody, a complementary response, and selected percussion accents from its
contour or rhythm. Later alterations should remain recognisable as relatives.

The opening eight-bar harmonic cycle is two bars each of:

**Fmaj9(#11) → Em9 → Dm9 → G13sus4.**

Use spread voicings, shared tones, inversions, and register exchange. Do not move
every voice in parallel or keep putting the root at the bottom of every pad.
Em9 introduces F-sharp deliberately; distinguish the local chord colour from
blindly forcing every note through one global scale. Keep the ninths and sharp
elevenths away from the sub register. Let suspensions resolve across bar lines.

In the middle, develop the harmony using whole-tone fragments, brief planing,
pedal tones, and an E-Phrygian passage. Plan the return to the opening material;
do not generate unrelated chord changes. The final F harmony may leave its
sharp eleventh suspended above a quiet E in an inner voice.

Budget voices across overlapping releases. For example, rootless four-note pad
voicings can overlap within an eight-voice instrument while Glass and bass supply
the remaining harmony. Do not ask an eight-voice engine for twelve simultaneous
pad voices and quietly accept stolen tails; verify the actual voice allocation.

The bass must feel composed, not random. Start from **3 + 3 + 2 quarter-note
beats across an eight-beat/two-bar loop**: onsets at beats 0, 3, and 6, with
durations 3, 3, and 2. Use deliberate overlaps/ties for legato rather than gaps.
Develop this with occasional anticipation, held notes, and answer phrases;
retain a clearly audible two-bar return point. If the drums also use 3–3–2,
state their subdivision explicitly—3–3–2 eighth notes spans one bar, not two.

### The eight tracks

| # | Role and starting instrument | Required development |
| --- | --- | --- |
| 1 | **Undertow — Thump kick** | Deep but controlled fundamental, short physical transient, and a second softer articulation. Establish a recognisable dancehall-oriented kick pattern with meaningful rests. Vary accent and transient character without changing pitch arbitrarily. Thin it during harmonic passages; restore its weight at the return. |
| 2 | **Porcelain — Clay snare/rim** | Snappy backbeats on beats 2 and 4 whenever the full groove is present. Alternate rim, skin, and noise character through supported sound/parameter locks. Add quiet ghosts and occasional pre-backbeat flams. Fills must preserve the main backbeat, not replace it with a burst. |
| 3 | **Salt Air — Hat** | Closed and open hats from the same track, with clearly distinct envelopes and a deliberate choke relationship. Seek diffuse, non-tonal noise rather than a persistent whistle. Use velocity, decay, spectral shape, and restrained microtiming to make the groove breathe. Include brief accelerating and decelerating drills with filter sweeps; avoid constant machine-gun hats. |
| 4 | **Splinters — Sampler** | Build a small vocabulary from the DAW's existing samples: woody hits, short break fragments, shakers, and one recognisable texture. Slice, reverse, repitch, retrigger, and resample where supported. Retain attack readability. This track carries the wildest fills, but phrases land exactly back on the grid and converse with the other drums. |
| 5 | **Black Current — Spectral bass** | One Spectral instance with a stable low foundation and an independently animated upper spectrum. Move from rounded sub to hollow reed, metallic growl, and an intense but controlled neuro-like voice, then back to softness. Sequence grouped harmonic amplitudes and phases, spectral ornaments, pitch glide, and rhythmic modulation at multiple timescales. Use sensible parallel/serial FX and modulators within the implementation's real capabilities. Keep the low register centred and intelligible throughout. |
| 6 | **Water Piano — Glass** | A soft, bell/piano-like melodic voice with a rounded attack, audible body, and long enough decay to connect phrases. Play the main five-note cell, broken spread chords, and sparse upper-register responses. Use inversions, contrary motion, delayed resolutions, occasional whole-tone runs, and velocity-dependent timbre. Avoid endless straight up-and-down arpeggios. |
| 7 | **Moon Veil — Poly pad** | A slowly breathing, wide pad carrying the spread harmony. Overlap releases across chord changes so old and new voices coexist briefly without retrigger clicks or indiscriminate smearing. Use slow filter motion, restrained pitch/timbre drift, and evolving space. In the breakdown, bring inner voices forward as counterlines; later remove layers without chopping their tails. |
| 8 | **Firefly — Acid countervoice** | A warm sliding monophonic counter-melody, not a constant high-resonance lead. Answer track 6, cross registers deliberately, and yield when the harmony needs room. Use occasional subtle-to-moderate vibrato with controlled rate and depth. Across the piece include kan-swar, meend, gamak, khatka, andolan, and murki as distinct, intentional gestures—not all at once or on every note. Specify their target notes, timing, depth, and return behaviour. Use the implemented ornament controls; do not claim an authentic raga performance merely from using these gestures. |

Treat these as starting instrument assignments, not permission to silently
invent controls. First verify available parameters, routing, modulation, and
commands. If a named capability is missing, record the gap and the best audible
approximation. Seek approval for a material substitution or new instrument/kernel
outside the execution scope the user grants. Do not quietly print a capability
claim that the project cannot reproduce.

### Arrangement: contrast with consequences

| Bars | Section | Musical job |
| --- | --- | --- |
| 1–16 | **Moon on Water** | Introduce the motif and harmonic colour through Glass and the overlapping pad. Hints of percussion and distant Acid responses. Let the listener hear the space before the drums occupy it. |
| 17–32 | **The Body Arrives** | Establish the full dancehall groove and eight-beat legato bass. State the motif clearly. Introduce small timbral variations; reserve the spectacular edits. |
| 33–48 | **Refractions** | Mutate the motif, exchange melody/counter-melody roles, and introduce connected IDM fills. Open the bass's upper partials in response to specific drum accents. Keep the kick/snare skeleton unmistakable. |
| 49–64 | **Broken Current** | Push spectral sequencing, conditional percussion, phase motion, slidy pitch, and contrasting drill rates. Include one short E-Phrygian transformation. Briefly make the upper bass sound like another instrument while its low foundation remains controlled. Reach a first peak, then make a decisive subtraction. |
| 65–80 | **Suspended Room** | Remove the main drums and expose voicing, release overlap, and two independent melodic lines. Move through the planed/whole-tone colour and pedal tension. Let delayed fragments recall the groove without creating an unintended extra track. |
| 81–96 | **Return Through Glass** | Bring the rhythm back in stages. The opening motif returns with changed voicing and rhythm; spectral bass retains a trace of its transformation. Use one clearly audible question-and-answer exchange between bass and Acid. |
| 97–112 | **Luminous Machinery** | The fullest section: strongest groove, most developed harmonic motion, and the boldest fills. Reach at least one moment with all eight tracks active and still separately legible. Include a rapid ratchet sweep answered by a slowing one. Density should crest, not remain maximal for sixteen bars. |
| 113–128 | **The Tide Leaves** | Dismantle the groove gracefully. Bass simplifies and resolves; snare and hats withdraw. Leave the motif, one countervoice, and an overlapping final pad. End in a soft, intentional release, with no hanging oscillators, abrupt truncation, or stray delay hit after the music has finished. |

Make every eight-bar unit contain a meaningful development or subtraction.
Use two-, four-, and eight-bar pattern relationships rather than eight unrelated
loops. A sparse unchanged bar is welcome when it gives another change meaning.

### Programming and sound-design requirements

- Preserve the backbeat through busy passages. No probability on structural
  kick/snare anchors; reserve conditional/probabilistic behaviour for ornaments,
  ghosts, and texture. Use a fixed seed or print the selected performance through
  an official reproducible path when necessary.
- Give short fills an explicit start, endpoint, subdivision, and resolution.
  Explore 1/16, 1/32, 1/64, triplet, and quintuplet groupings only where the engine
  can represent them accurately. Unsupported divisions must be reported, not
  approximated under the same label. Make fills finish on time.
- Pair at least three ratchet gestures with shaped parameter motion: opening
  filter, closing filter, and a direction-reversing sweep. Include velocity or
  decay contours so the results sound articulated rather than simply repeated.
- Use held locks, sliding locks, envelopes, slow LFOs, tempo-related modulation,
  and bounded per-note variation where supported. Randomness should vary a
  performance within a designed identity, not replace composition.
- Give the Spectral bass at least three independently controlled timbral regions
  and two rhythmic timescales. Protect the fundamental while upper regions
  change. Do not equate simultaneous maximum parameter values with power.
- Use contrasting spaces: a short percussive room, a darker melodic delay, and a
  long pad space. Shape sends/returns around phrases, filter unnecessary low end
  from wet paths, and manage feedback. FX must deepen the composition without
  blurring every transient into the same room.
- Shared buses and instrument-owned routing should be named and inspectable.
  Avoid unnecessary duplicate chains. Reject unsafe cycles and respect current
  resource budgets. Do not add permanent UI animation or a visual score for this
  audio-only brief; keep procedural visuals Off.

### Execution, audition, and acceptance

1. Inventory the actual native commands and instrument controls. Write a short
   capability matrix: ready, awkward, missing. Plan the eight tracks, sections,
   routing, and automation before generating the drive. Never guess executable
   command syntax from the design language in this prompt.
2. Prepare an auditable native `.drive` TAKE and a new project under the DAW's
   project folder. Use canonical editing/import paths, not hand-edited project
   internals that bypass validation or undo. Preserve existing projects and
   renders. Use explicit destinations and versioned output names.
3. For important patches, follow **set parameters → audition in the app → bounce
   an excerpt → analyse → request human judgement**. Bundle focused comparisons
   rather than asking for approval after each knob. Include bass round/growl,
   snare transient, closed/open hat, pad transition, and ornamented lead examples.
4. Build a complete machine-checked v1, then provide at least three listening
   excerpts: the established groove, the densest passage/transition, and the
   breakdown into the return. Also provide the final full-resolution stereo
   bounce in the DAW render folder and an editable native project.
5. Analyse peak and true peak if a suitable tool is available, RMS/loudness with
   the measurement method identified, clipping, DC, low-frequency balance,
   stereo correlation/mono compatibility, hat spectra and decay, rhythmic
   alignment, and final-tail behaviour. Distinguish observations from taste:
   spectral flatness alone cannot establish that a hat sounds good. If a tool
   cannot make a measurement, state that instead of fabricating a result.
6. Inspect the dense arrangement in the tracker while playing, navigating,
   holding, and following. Measure relevant UI/audio load and xruns without
   concurrent compilation contaminating the result. Check save/reopen, parameter
   linkage, automation, release overlap, and an uninterrupted final render.
7. Mark the result **machine-checked / awaiting human acceptance** until the user
   listens and approves. Do not self-certify the human-in-the-loop test. If the
   user is away, finish the listening package and report the pending judgement.

## After building: make the next drive cheaper

Once v1 exists, write a separate, evidence-backed improvement report. Do not
pretend to have measured command savings while merely writing this brief.
Prioritise the friction actually encountered, including instrument and FX design,
not just note entry. Do not implement the resulting backlog until authorised.

Keep a ledger from the start: accepted native commands, key actions, typed
characters, tool calls, payload size, retries/refusals, manual interventions,
waits, and authoring/audition time. Separate one-time setup from recurring work.
A giant script/import containing hundreds of literal edits is not one unit of
creative effort just because the app accepts it with one command.

For every proposed improvement, provide:

- the repeated task and evidence from this drive;
- the existing action count and a proposed before/after sequence;
- the general operation and at least two other musical uses;
- proposed command/API syntax, explicitly labelled **not implemented**;
- parameters, defaults, deterministic behaviour, selection/addressing, preview,
  validation, refusal, undo, and persistence semantics;
- interaction with other features, and tests that would prevent regression;
- estimated recurring savings, implementation cost, and priority. Label estimates
  as estimates and distinguish them from a later measured retest.

Investigate these candidates, but earn their place from the actual ledger:

1. **Time-range and address selectors:** select by section, track role, phrase,
   beat position, register, or lock address. Reuse that selection across note,
   timing, sound, automation, and routing operations.
2. **Ratchet plus gesture:** one reusable rhythmic transform with a rate curve,
   acceleration/deceleration, exact duration, velocity contour, and one or more
   parameter sweeps. Useful for hats, snare drills, grains, and melodic stutters.
3. **Voice-leading and articulation constraints:** register-aware spread voicing,
   common-tone retention, independent voice motion, legato/overlap, and bass
   exclusions. Useful for any chord quality—not an Fmaj9(#11) button.
4. **Motif transformations:** rhythmic displacement, inversion, augmentation,
   phrase-aware ornamentation, and counterline generation with explicit range
   and collision constraints. Keep the source motif and transformations legible.
5. **Groove and variation recipes:** reusable accent/microtiming/velocity models,
   protected anchors, seeded variation, and section-level density trajectories.
   Do not build a button named after an artist or one genre.
6. **Spectral-region gestures:** address harmonic ranges or groups, preserve a
   protected fundamental, and morph amplitude/phase envelopes across musical
   time. Reuse for bass, metallic percussion, moving pads, and resynthesis.
7. **Shared modulation and routing recipes:** named envelopes/LFOs/macros and
   serial/parallel FX subgraphs with explicit mappings, units, bounds, gain
   staging, and safe topology checks. Reuse one idea across multiple sounds.
8. **Instrument-independent phrase expression:** generalise useful Acid pitch
   gestures only when authorised, with clear pitch/legato capabilities and
   note-target semantics. Also consider spectral and filter analogues without
   pretending they are the same acoustic phenomenon.
9. **Audition matrix:** automatically prepare level-matched A/B excerpts from
   parameter variations, render them, attach analysis, and collect human
   preferences. Useful for snares, hats, pads, basses, and FX chains.
10. **Arrangement-aware batch operations:** propagate a source pattern with
    explicit per-section deltas, preserve exceptions, and apply atomic edits to
    a named section. Avoid copying long strings of almost-identical commands.
11. **Render/analysis packages:** versioned full mix and excerpt exports, tail
    policy, consistent analysis, provenance, and human-acceptance state from one
    validated job description.
12. **Drive planning and preflight:** resolve addresses/units, validate resources,
    predict missing capabilities, expose meaningful state barriers, and generate
    an inspectable execution plan before touching the project.

Rank the best five by recurring effort saved and how broadly they help other
features. Identify which pieces should be shared primitives rather than separate
commands. A feature qualifies because it increases the app's general expressive
power, not because it shortens this one saved script.

For a future authorised v2, target **at least a 60% reduction in recurring native
editing commands**, with a stretch target of **400 or fewer** for an equivalent
eight-track rebuild. Keep the counting definition fixed and show the other ledger
metrics alongside it. Retest on both this composition and a different miniature
to expose overfitting. Preserve musical requirements, editability, audible
quality, and human approval; do not win the count by deleting detail, hiding work
inside opaque payloads, or omitting audition and verification.
