//! Parameter tables — the wire contract between widget, engine and app.
//!
//! Every knob a node exposes used to be numbered in three places that could
//! not see each other: the device widget's `P_*` consts, the bare literals in
//! `Node::apply` in `src/audio/graph.rs`, and the bare literals in the app's
//! `apply_*_edits`. Nothing connected them, and the failure mode was silent —
//! a renumbering sends a reverb's mix to a synth's gain and nothing fails to
//! compile. (The old `assert_eq!(P_MIX, 0)` test in the reverb widget was
//! this file trying to exist.)
//!
//! Now a device declares its knobs ONCE: id, name, engine-facing range,
//! default. All three sites read this table — the widget for names, ranges
//! and defaults; the engine for its red-zone clamp; the app for its match
//! arms. Adding a device means adding a module here and using its consts
//! everywhere; using a literal id anywhere else is the bug.
//!
//! Invariants (enforced by tests below): within a table, ids are dense and
//! equal to their index — so `TABLE[FOO as usize]` is the definition of
//! `FOO` — names are unique, and every default lies inside its range.

/// One knob of one device, in ENGINE units (Hz, ms, linear gain — never
/// normalized widget positions).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamDef {
    /// The wire id carried by `ParamChange` letters and `ParamEdit`s.
    pub id: u32,
    /// Display name, also the automation-facing name later.
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

impl ParamDef {
    /// Clamp an engine-facing value into this knob's range.
    pub fn clamp(self, value: f32) -> f32 {
        value.clamp(self.min, self.max)
    }
}

/// Red-zone lookup: clamp `value` into the range of `id`, or `None` for an
/// id the table does not know (a stale or misrouted letter — drop it).
/// Linear scan over a static table of at most a handful of entries; no
/// allocation, no panic.
pub fn clamp(table: &'static [ParamDef], id: u32, value: f32) -> Option<f32> {
    table
        .iter()
        .find(|def| def.id == id)
        .map(|def| def.clamp(value))
}

/// Green-zone lookup by id. Panics on an unknown id, which is a compile-time
/// mistake when callers use the named consts — never call this with a
/// runtime-provided id (that is what [`clamp`] is for).
pub fn def(table: &'static [ParamDef], id: u32) -> &'static ParamDef {
    &table[id as usize]
}

/// STAB: the house chord synth. One key is a chord; the chord, its
/// inversion and its voicing are knobs, so a pattern of single notes
/// plays a progression. See [`crate::audio::stab`].
pub mod stab {
    use super::ParamDef;

    pub const CHORD: u32 = 0;
    pub const INVERSION: u32 = 1;
    pub const OPEN: u32 = 2;
    pub const OCTAVE: u32 = 3;
    pub const STRUM: u32 = 4;
    pub const TONE: u32 = 5;
    pub const DETUNE: u32 = 6;
    pub const WIDTH: u32 = 7;
    pub const ATTACK: u32 = 8;
    pub const DECAY: u32 = 9;
    pub const SUSTAIN: u32 = 10;
    pub const RELEASE: u32 = 11;
    pub const CUTOFF: u32 = 12;
    pub const RESO: u32 = 13;
    pub const ENV: u32 = 14;
    pub const FENV: u32 = 15;
    pub const DRIVE: u32 = 16;
    pub const CRUSH: u32 = 17;
    pub const RATE: u32 = 18;
    pub const LEVEL: u32 = 19;
    /// A chord tone left out: a rootless voicing over a bass, a bare
    /// fifth, the third dropped so the chord stops saying major or
    /// minor. Counted by chord tone, not by pitch, so it means the same
    /// thing in every chord that has that tone.
    pub const OMIT: u32 = 20;

    pub const OMIT_NAMES: &[&str] = &["none", "root", "3rd", "5th", "7th", "9th"];

    /// The most notes a chord has.
    pub const NOTES: usize = 5;

    /// The chords, as semitones above the root. The list a house record
    /// needs: triads, sevenths, ninths, the suspensions, the sixths.
    pub const CHORDS: &[(&str, &[i32])] = &[
        ("maj", &[0, 4, 7]),
        ("min", &[0, 3, 7]),
        ("7", &[0, 4, 7, 10]),
        ("m7", &[0, 3, 7, 10]),
        ("maj7", &[0, 4, 7, 11]),
        ("9", &[0, 4, 7, 10, 14]),
        ("m9", &[0, 3, 7, 10, 14]),
        ("maj9", &[0, 4, 7, 11, 14]),
        ("add9", &[0, 4, 7, 14]),
        ("sus2", &[0, 2, 7]),
        ("sus4", &[0, 5, 7]),
        ("6", &[0, 4, 7, 9]),
        ("m6", &[0, 3, 7, 9]),
        ("dim", &[0, 3, 6]),
    ];
    pub const CHORD_NAMES: &[&str] = &[
        "maj", "min", "7", "m7", "maj7", "9", "m9", "maj9", "add9", "sus2", "sus4", "6", "m6",
        "dim",
    ];
    pub const INVERSION_NAMES: &[&str] = &["root", "1st", "2nd", "3rd", "4th"];
    pub const OPEN_NAMES: &[&str] = &["close", "open"];
    pub const OCTAVE_NAMES: &[&str] = &["-2", "-1", "0", "+1", "+2"];
    pub const LEVEL_MAX: f32 = 2.0;

    /// The chord's notes as semitones above the played key, voiced:
    /// inverted by carrying the lowest notes up an octave, and opened
    /// by dropping the second note from the top an octave — the
    /// drop-two voicing, which is how a stab gets wide without getting
    /// muddy. Returns the notes and how many there are; the rest of the
    /// array is unused. No allocation: the audio thread calls this.
    pub fn voicing(
        chord: usize,
        inversion: usize,
        open: bool,
        omit: usize,
    ) -> ([i32; NOTES], usize) {
        let (_, intervals) = CHORDS.get(chord).copied().unwrap_or(CHORDS[0]);
        let mut notes = [0i32; NOTES];
        let mut count = 0usize;
        // The chord tones in the table are root, third, fifth, seventh,
        // ninth, in that order; an omission names one by its place. A
        // chord with fewer tones than the omission asks for keeps them
        // all — and a chord is never omitted down to a single note.
        for (place, interval) in intervals.iter().enumerate().take(NOTES) {
            if omit > 0 && place + 1 == omit && intervals.len() > 2 {
                continue;
            }
            notes[count] = *interval;
            count += 1;
        }
        // Inversion: the lowest note goes up an octave, `inversion`
        // times. A ninth sits above the raised root, so the notes are
        // put back in order each time rather than assumed to be.
        for _ in 0..inversion.min(count.saturating_sub(1)) {
            notes[0] += 12;
            sort(&mut notes[..count]);
        }
        if open && count >= 3 {
            notes[count - 2] -= 12;
            sort(&mut notes[..count]);
        }
        (notes, count)
    }

    /// An insertion sort: five notes at most, and no allocation.
    fn sort(notes: &mut [i32]) {
        for i in 1..notes.len() {
            let mut j = i;
            while j > 0 && notes[j - 1] > notes[j] {
                notes.swap(j - 1, j);
                j -= 1;
            }
        }
    }

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: CHORD,
            name: "chord",
            min: 0.0,
            max: 13.0,
            default: 3.0,
        },
        ParamDef {
            id: INVERSION,
            name: "inversion",
            min: 0.0,
            max: 4.0,
            default: 0.0,
        },
        ParamDef {
            id: OPEN,
            name: "voicing",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: OCTAVE,
            name: "octave",
            min: -2.0,
            max: 2.0,
            default: 0.0,
        },
        ParamDef {
            id: STRUM,
            name: "strum",
            min: 0.0,
            max: 60.0,
            default: 0.0,
        },
        ParamDef {
            id: TONE,
            name: "tone",
            min: 0.0,
            max: 1.0,
            default: 0.35,
        },
        ParamDef {
            id: DETUNE,
            name: "detune",
            min: 0.0,
            max: 50.0,
            default: 8.0,
        },
        ParamDef {
            id: WIDTH,
            name: "width",
            min: 0.0,
            max: 1.0,
            default: 0.6,
        },
        ParamDef {
            id: ATTACK,
            name: "attack",
            min: 0.0,
            max: 500.0,
            default: 2.0,
        },
        ParamDef {
            id: DECAY,
            name: "decay",
            min: 5.0,
            max: 3_000.0,
            default: 350.0,
        },
        ParamDef {
            id: SUSTAIN,
            name: "sustain",
            min: 0.0,
            max: 1.0,
            default: 0.2,
        },
        ParamDef {
            id: RELEASE,
            name: "release",
            min: 5.0,
            max: 3_000.0,
            default: 180.0,
        },
        ParamDef {
            id: CUTOFF,
            name: "cutoff",
            min: 100.0,
            max: 18_000.0,
            default: 6_000.0,
        },
        ParamDef {
            id: RESO,
            name: "reso",
            min: 0.5,
            max: 12.0,
            default: 0.9,
        },
        ParamDef {
            id: ENV,
            name: "env",
            min: 0.0,
            max: 6.0,
            default: 2.5,
        },
        ParamDef {
            id: FENV,
            name: "env decay",
            min: 5.0,
            max: 2_000.0,
            default: 220.0,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: 1.0,
            max: 32.0,
            default: 1.5,
        },
        ParamDef {
            id: CRUSH,
            name: "crush",
            min: 2.0,
            max: 16.0,
            default: 16.0,
        },
        ParamDef {
            id: RATE,
            name: "rate",
            min: 1_000.0,
            max: 48_000.0,
            default: 48_000.0,
        },
        ParamDef {
            id: LEVEL,
            name: "level",
            min: 0.0,
            max: LEVEL_MAX,
            default: 0.8,
        },
        ParamDef {
            id: OMIT,
            name: "omit",
            min: 0.0,
            max: 5.0,
            default: 0.0,
        },
    ];

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_chord_list_and_its_names_agree_and_voicings_stay_sorted() {
            assert_eq!(CHORDS.len(), CHORD_NAMES.len());
            for ((name, intervals), listed) in CHORDS.iter().zip(CHORD_NAMES) {
                assert_eq!(name, listed);
                assert!(intervals.len() <= NOTES && intervals[0] == 0);
                assert!(intervals.windows(2).all(|w| w[0] < w[1]));
            }
            let (notes, count) = voicing(0, 0, false, 0);
            assert_eq!((&notes[..count], count), (&[0, 4, 7][..], 3));
            let (notes, count) = voicing(0, 1, false, 0);
            assert_eq!(&notes[..count], &[4, 7, 12], "first inversion");
            let (notes, count) = voicing(0, 2, false, 0);
            assert_eq!(&notes[..count], &[7, 12, 16], "second inversion");
            let (notes, count) = voicing(4, 3, false, 0);
            assert_eq!(&notes[..count], &[11, 12, 16, 19], "maj7 third inversion");
            let (notes, count) = voicing(0, 4, false, 0);
            assert_eq!(
                &notes[..count],
                &[7, 12, 16],
                "a triad has no fourth inversion"
            );
            let (notes, count) = voicing(4, 0, true, 0);
            assert_eq!(
                &notes[..count],
                &[-5, 0, 4, 11],
                "drop-two: the fifth goes under"
            );
            for chord in 0..CHORDS.len() {
                for inversion in 0..5 {
                    for open in [false, true] {
                        for omit in 0..OMIT_NAMES.len() {
                            let (notes, count) = voicing(chord, inversion, open, omit);
                            assert!(count >= 2, "{chord} {inversion} {open} {omit} left {count}");
                            assert!(
                                notes[..count].windows(2).all(|w| w[0] < w[1]),
                                "{chord} {inversion} {open} {omit}: {notes:?}"
                            );
                        }
                    }
                }
            }
            // Omissions, by chord tone.
            let (notes, count) = voicing(3, 0, false, 1);
            assert_eq!(&notes[..count], &[3, 7, 10], "m7 without its root");
            let (notes, count) = voicing(0, 0, false, 2);
            assert_eq!(&notes[..count], &[0, 7], "a bare fifth");
            let (notes, count) = voicing(6, 0, false, 3);
            assert_eq!(&notes[..count], &[0, 3, 10, 14], "m9 without its fifth");
            let (notes, count) = voicing(0, 0, false, 4);
            assert_eq!(
                &notes[..count],
                &[0, 4, 7],
                "a triad has no seventh to omit"
            );
            let (notes, count) = voicing(6, 1, false, 1);
            assert_eq!(&notes[..count], &[7, 10, 14, 15], "rootless, then inverted");
        }
    }
}

/// QUAD: the four-operator FM workhorse. See [`crate::audio::quad`].
pub mod quad {
    use super::ParamDef;

    /// The operators' rows: twelve per operator, in this order, so an
    /// operator's parameter id is `op * PER_OP + field`.
    pub const OPS: usize = 4;
    pub const PER_OP: u32 = 12;
    pub const RATIO: u32 = 0;
    pub const FINE: u32 = 1;
    pub const LEVEL_OP: u32 = 2;
    pub const ATTACK: u32 = 3;
    pub const DECAY: u32 = 4;
    pub const SUSTAIN: u32 = 5;
    pub const RELEASE: u32 = 6;
    pub const WAVE: u32 = 7;
    pub const FIXED: u32 = 8;
    pub const HZ: u32 = 9;
    pub const VEL: u32 = 10;
    pub const KEYSCALE: u32 = 11;
    /// Operator `op` (from zero), field `field`.
    pub const fn op_param(op: usize, field: u32) -> u32 {
        op as u32 * PER_OP + field
    }
    /// Which operator and field a row is, if it is an operator's.
    pub fn op_of(param: u32) -> Option<(usize, u32)> {
        (param < OPS as u32 * PER_OP).then(|| ((param / PER_OP) as usize, param % PER_OP))
    }

    pub const ALGO: u32 = 48;
    pub const FEEDBACK: u32 = 49;
    pub const PITCH1: u32 = 50;
    pub const PITCH1_RISE: u32 = 51;
    pub const PITCH1_FALL: u32 = 52;
    pub const PITCH2: u32 = 53;
    pub const PITCH2_RISE: u32 = 54;
    pub const PITCH2_FALL: u32 = 55;
    pub const FMODE: u32 = 56;
    pub const CUTOFF: u32 = 57;
    pub const RESO: u32 = 58;
    pub const FENV: u32 = 59;
    pub const FENV_ATT: u32 = 60;
    pub const FENV_DEC: u32 = 61;
    pub const KEYTRACK: u32 = 62;
    pub const DIST: u32 = 63;
    pub const DRIVE: u32 = 64;
    pub const VELOCITY: u32 = 65;
    pub const LEVEL: u32 = 66;
    pub const KEY_RATE: u32 = 67;
    pub const UNISON: u32 = 68;
    pub const UDETUNE: u32 = 69;
    pub const WIDTH: u32 = 70;
    pub const MONO: u32 = 71;
    pub const GLIDE: u32 = 72;
    pub const FB_OP: u32 = 73;
    pub const LFO1_RATE: u32 = 74;
    pub const LFO1_SHAPE: u32 = 75;
    pub const LFO1_DELAY: u32 = 76;
    pub const LFO1_FADE: u32 = 77;
    pub const LFO1_PITCH: u32 = 78;
    pub const LFO1_MOD: u32 = 79;
    pub const LFO1_AMP: u32 = 80;
    pub const LFO1_FILTER: u32 = 81;
    pub const LFO2_RATE: u32 = 82;
    pub const LFO2_SHAPE: u32 = 83;
    pub const LFO2_DELAY: u32 = 84;
    pub const LFO2_FADE: u32 = 85;
    pub const LFO2_PITCH: u32 = 86;
    pub const LFO2_MOD: u32 = 87;
    pub const LFO2_AMP: u32 = 88;
    pub const LFO2_FILTER: u32 = 89;

    /// The two LFOs' rows are laid out alike: `LFO2_RATE - LFO1_RATE`
    /// apart.
    pub const LFOS: usize = 2;
    pub const PER_LFO: u32 = LFO2_RATE - LFO1_RATE;
    pub const fn lfo_param(lfo: usize, field: u32) -> u32 {
        LFO1_RATE + lfo as u32 * PER_LFO + field
    }
    pub const LFO_RATE: u32 = 0;
    pub const LFO_SHAPE: u32 = 1;
    pub const LFO_DELAY: u32 = 2;
    pub const LFO_FADE: u32 = 3;
    pub const LFO_PITCH: u32 = 4;
    pub const LFO_MOD: u32 = 5;
    pub const LFO_AMP: u32 = 6;
    pub const LFO_FILTER: u32 = 7;

    pub const ALGO_NAMES: &[&str] = &[
        "4>3>2>1",
        "4>3>1 2>1",
        "4>2 3>2>1",
        "4>3>2 +1",
        "4>3 2>1",
        "4,3,2>1",
        "4>3 +2 +1",
        "1+2+3+4",
    ];
    pub const FMODE_NAMES: &[&str] = &["lp", "hp", "bp", "notch"];
    pub const DIST_NAMES: &[&str] = &["off", "soft", "hard", "fold"];
    /// An operator's shape: the sine, the TX81Z's half and rectified
    /// sines, a soft square, and noise — which is a modulator's grit and
    /// a carrier's breath.
    pub const WAVE_NAMES: &[&str] = &["sine", "half", "rect", "square", "noise"];
    pub const WAVE_NOISE: f32 = 4.0;
    pub const FIXED_NAMES: &[&str] = &["ratio", "fixed"];
    pub const UNISON_NAMES: &[&str] = &["1", "2", "3", "4"];
    pub const UNISON_MAX: usize = 4;
    pub const MONO_NAMES: &[&str] = &["poly", "mono", "legato"];
    pub const MONO_POLY: f32 = 0.0;
    pub const MONO_MONO: f32 = 1.0;
    pub const MONO_LEGATO: f32 = 2.0;
    pub const FB_OP_NAMES: &[&str] = &["op 1", "op 2", "op 3", "op 4"];
    pub const LFO_SHAPE_NAMES: &[&str] = &["sine", "tri", "square", "s&h"];
    pub const LEVEL_MAX: f32 = 2.0;

    /// A routing shape: which operator modulates which, and which
    /// operators are heard. Operators are numbered from one as on the
    /// panel; the arrays index from zero.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Algorithm {
        /// `(modulator, carrier)` pairs, modulator into carrier.
        pub edges: &'static [(usize, usize)],
        /// The operators that reach the output.
        pub carriers: &'static [usize],
    }

    pub const ALGORITHMS: [Algorithm; 8] = [
        Algorithm {
            edges: &[(3, 2), (2, 1), (1, 0)],
            carriers: &[0],
        },
        Algorithm {
            edges: &[(3, 2), (2, 0), (1, 0)],
            carriers: &[0],
        },
        Algorithm {
            edges: &[(3, 1), (2, 1), (1, 0)],
            carriers: &[0],
        },
        Algorithm {
            edges: &[(3, 2), (2, 1)],
            carriers: &[1, 0],
        },
        Algorithm {
            edges: &[(3, 2), (1, 0)],
            carriers: &[2, 0],
        },
        Algorithm {
            edges: &[(3, 0), (2, 0), (1, 0)],
            carriers: &[0],
        },
        Algorithm {
            edges: &[(3, 2)],
            carriers: &[2, 1, 0],
        },
        Algorithm {
            edges: &[],
            carriers: &[3, 2, 1, 0],
        },
    ];

    pub fn algorithm(index: f32) -> Algorithm {
        ALGORITHMS[(index.round().max(0.0) as usize).min(ALGORITHMS.len() - 1)]
    }

    /// Whether operator `op` is a modulator in `algo` — the ones the
    /// second pitch envelope bends.
    pub fn is_modulator(algo: Algorithm, op: usize) -> bool {
        algo.edges.iter().any(|(m, _)| *m == op)
    }

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: 0,
            name: "op1 ratio",
            min: 0.25,
            max: 16.00,
            default: 1.00,
        },
        ParamDef {
            id: 1,
            name: "op1 fine",
            min: -100.00,
            max: 100.00,
            default: 0.00,
        },
        ParamDef {
            id: 2,
            name: "op1 level",
            min: 0.00,
            max: 1.00,
            default: 1.00,
        },
        ParamDef {
            id: 3,
            name: "op1 attack",
            min: 0.00,
            max: 2000.00,
            default: 1.00,
        },
        ParamDef {
            id: 4,
            name: "op1 decay",
            min: 1.00,
            max: 4000.00,
            default: 800.00,
        },
        ParamDef {
            id: 5,
            name: "op1 sustain",
            min: 0.00,
            max: 1.00,
            default: 0.70,
        },
        ParamDef {
            id: 6,
            name: "op1 release",
            min: 1.00,
            max: 4000.00,
            default: 250.00,
        },
        ParamDef {
            id: 7,
            name: "op1 wave",
            min: 0.00,
            max: 4.00,
            default: 0.00,
        },
        ParamDef {
            id: 8,
            name: "op1 fixed",
            min: 0.00,
            max: 1.00,
            default: 0.00,
        },
        ParamDef {
            id: 9,
            name: "op1 hz",
            min: 1.00,
            max: 10000.00,
            default: 440.00,
        },
        ParamDef {
            id: 10,
            name: "op1 vel",
            min: 0.00,
            max: 1.00,
            default: 0.50,
        },
        ParamDef {
            id: 11,
            name: "op1 keyscale",
            min: -12.00,
            max: 12.00,
            default: 0.00,
        },
        ParamDef {
            id: 12,
            name: "op2 ratio",
            min: 0.25,
            max: 16.00,
            default: 2.00,
        },
        ParamDef {
            id: 13,
            name: "op2 fine",
            min: -100.00,
            max: 100.00,
            default: 0.00,
        },
        ParamDef {
            id: 14,
            name: "op2 level",
            min: 0.00,
            max: 1.00,
            default: 0.55,
        },
        ParamDef {
            id: 15,
            name: "op2 attack",
            min: 0.00,
            max: 2000.00,
            default: 1.00,
        },
        ParamDef {
            id: 16,
            name: "op2 decay",
            min: 1.00,
            max: 4000.00,
            default: 400.00,
        },
        ParamDef {
            id: 17,
            name: "op2 sustain",
            min: 0.00,
            max: 1.00,
            default: 0.30,
        },
        ParamDef {
            id: 18,
            name: "op2 release",
            min: 1.00,
            max: 4000.00,
            default: 250.00,
        },
        ParamDef {
            id: 19,
            name: "op2 wave",
            min: 0.00,
            max: 4.00,
            default: 0.00,
        },
        ParamDef {
            id: 20,
            name: "op2 fixed",
            min: 0.00,
            max: 1.00,
            default: 0.00,
        },
        ParamDef {
            id: 21,
            name: "op2 hz",
            min: 1.00,
            max: 10000.00,
            default: 440.00,
        },
        ParamDef {
            id: 22,
            name: "op2 vel",
            min: 0.00,
            max: 1.00,
            default: 0.50,
        },
        ParamDef {
            id: 23,
            name: "op2 keyscale",
            min: -12.00,
            max: 12.00,
            default: 0.00,
        },
        ParamDef {
            id: 24,
            name: "op3 ratio",
            min: 0.25,
            max: 16.00,
            default: 1.00,
        },
        ParamDef {
            id: 25,
            name: "op3 fine",
            min: -100.00,
            max: 100.00,
            default: 0.00,
        },
        ParamDef {
            id: 26,
            name: "op3 level",
            min: 0.00,
            max: 1.00,
            default: 0.00,
        },
        ParamDef {
            id: 27,
            name: "op3 attack",
            min: 0.00,
            max: 2000.00,
            default: 1.00,
        },
        ParamDef {
            id: 28,
            name: "op3 decay",
            min: 1.00,
            max: 4000.00,
            default: 400.00,
        },
        ParamDef {
            id: 29,
            name: "op3 sustain",
            min: 0.00,
            max: 1.00,
            default: 0.30,
        },
        ParamDef {
            id: 30,
            name: "op3 release",
            min: 1.00,
            max: 4000.00,
            default: 250.00,
        },
        ParamDef {
            id: 31,
            name: "op3 wave",
            min: 0.00,
            max: 4.00,
            default: 0.00,
        },
        ParamDef {
            id: 32,
            name: "op3 fixed",
            min: 0.00,
            max: 1.00,
            default: 0.00,
        },
        ParamDef {
            id: 33,
            name: "op3 hz",
            min: 1.00,
            max: 10000.00,
            default: 440.00,
        },
        ParamDef {
            id: 34,
            name: "op3 vel",
            min: 0.00,
            max: 1.00,
            default: 0.50,
        },
        ParamDef {
            id: 35,
            name: "op3 keyscale",
            min: -12.00,
            max: 12.00,
            default: 0.00,
        },
        ParamDef {
            id: 36,
            name: "op4 ratio",
            min: 0.25,
            max: 16.00,
            default: 1.00,
        },
        ParamDef {
            id: 37,
            name: "op4 fine",
            min: -100.00,
            max: 100.00,
            default: 0.00,
        },
        ParamDef {
            id: 38,
            name: "op4 level",
            min: 0.00,
            max: 1.00,
            default: 0.00,
        },
        ParamDef {
            id: 39,
            name: "op4 attack",
            min: 0.00,
            max: 2000.00,
            default: 1.00,
        },
        ParamDef {
            id: 40,
            name: "op4 decay",
            min: 1.00,
            max: 4000.00,
            default: 400.00,
        },
        ParamDef {
            id: 41,
            name: "op4 sustain",
            min: 0.00,
            max: 1.00,
            default: 0.30,
        },
        ParamDef {
            id: 42,
            name: "op4 release",
            min: 1.00,
            max: 4000.00,
            default: 250.00,
        },
        ParamDef {
            id: 43,
            name: "op4 wave",
            min: 0.00,
            max: 4.00,
            default: 0.00,
        },
        ParamDef {
            id: 44,
            name: "op4 fixed",
            min: 0.00,
            max: 1.00,
            default: 0.00,
        },
        ParamDef {
            id: 45,
            name: "op4 hz",
            min: 1.00,
            max: 10000.00,
            default: 440.00,
        },
        ParamDef {
            id: 46,
            name: "op4 vel",
            min: 0.00,
            max: 1.00,
            default: 0.50,
        },
        ParamDef {
            id: 47,
            name: "op4 keyscale",
            min: -12.00,
            max: 12.00,
            default: 0.00,
        },
        ParamDef {
            id: 48,
            name: "algo",
            min: 0.00,
            max: 7.00,
            default: 0.00,
        },
        ParamDef {
            id: 49,
            name: "feedback",
            min: -1.00,
            max: 1.00,
            default: 0.00,
        },
        ParamDef {
            id: 50,
            name: "pitch 1",
            min: -48.00,
            max: 48.00,
            default: 0.00,
        },
        ParamDef {
            id: 51,
            name: "p1 rise",
            min: 0.00,
            max: 2000.00,
            default: 0.00,
        },
        ParamDef {
            id: 52,
            name: "p1 fall",
            min: 1.00,
            max: 4000.00,
            default: 200.00,
        },
        ParamDef {
            id: 53,
            name: "pitch 2",
            min: -48.00,
            max: 48.00,
            default: 0.00,
        },
        ParamDef {
            id: 54,
            name: "p2 rise",
            min: 0.00,
            max: 2000.00,
            default: 0.00,
        },
        ParamDef {
            id: 55,
            name: "p2 fall",
            min: 1.00,
            max: 4000.00,
            default: 200.00,
        },
        ParamDef {
            id: 56,
            name: "filter",
            min: 0.00,
            max: 3.00,
            default: 0.00,
        },
        ParamDef {
            id: 57,
            name: "cutoff",
            min: 20.00,
            max: 20000.00,
            default: 12000.00,
        },
        ParamDef {
            id: 58,
            name: "reso",
            min: 0.50,
            max: 20.00,
            default: 0.80,
        },
        ParamDef {
            id: 59,
            name: "f env",
            min: -6.00,
            max: 6.00,
            default: 0.00,
        },
        ParamDef {
            id: 60,
            name: "f attack",
            min: 0.00,
            max: 2000.00,
            default: 0.00,
        },
        ParamDef {
            id: 61,
            name: "f decay",
            min: 1.00,
            max: 4000.00,
            default: 300.00,
        },
        ParamDef {
            id: 62,
            name: "keytrack",
            min: 0.00,
            max: 1.00,
            default: 0.50,
        },
        ParamDef {
            id: 63,
            name: "dist",
            min: 0.00,
            max: 3.00,
            default: 0.00,
        },
        ParamDef {
            id: 64,
            name: "drive",
            min: 1.00,
            max: 32.00,
            default: 1.00,
        },
        ParamDef {
            id: 65,
            name: "velocity",
            min: 0.00,
            max: 1.00,
            default: 0.60,
        },
        ParamDef {
            id: 66,
            name: "level",
            min: 0.00,
            max: 2.00,
            default: 0.80,
        },
        ParamDef {
            id: 67,
            name: "key rate",
            min: 0.00,
            max: 1.00,
            default: 0.30,
        },
        ParamDef {
            id: 68,
            name: "unison",
            min: 1.00,
            max: 4.00,
            default: 1.00,
        },
        ParamDef {
            id: 69,
            name: "udetune",
            min: 0.00,
            max: 50.00,
            default: 10.00,
        },
        ParamDef {
            id: 70,
            name: "width",
            min: 0.00,
            max: 1.00,
            default: 0.50,
        },
        ParamDef {
            id: 71,
            name: "mode",
            min: 0.00,
            max: 2.00,
            default: 0.00,
        },
        ParamDef {
            id: 72,
            name: "glide",
            min: 0.00,
            max: 2000.00,
            default: 0.00,
        },
        ParamDef {
            id: 73,
            name: "fb op",
            min: 0.00,
            max: 3.00,
            default: 3.00,
        },
        ParamDef {
            id: 74,
            name: "lfo1 rate",
            min: 0.05,
            max: 20.00,
            default: 5.00,
        },
        ParamDef {
            id: 75,
            name: "lfo1 shape",
            min: 0.00,
            max: 3.00,
            default: 0.00,
        },
        ParamDef {
            id: 76,
            name: "lfo1 delay",
            min: 0.00,
            max: 4000.00,
            default: 0.00,
        },
        ParamDef {
            id: 77,
            name: "lfo1 fade",
            min: 0.00,
            max: 4000.00,
            default: 0.00,
        },
        ParamDef {
            id: 78,
            name: "lfo1 pitch",
            min: 0.00,
            max: 12.00,
            default: 0.00,
        },
        ParamDef {
            id: 79,
            name: "lfo1 mod",
            min: 0.00,
            max: 1.00,
            default: 0.00,
        },
        ParamDef {
            id: 80,
            name: "lfo1 amp",
            min: 0.00,
            max: 1.00,
            default: 0.00,
        },
        ParamDef {
            id: 81,
            name: "lfo1 filter",
            min: -4.00,
            max: 4.00,
            default: 0.00,
        },
        ParamDef {
            id: 82,
            name: "lfo2 rate",
            min: 0.05,
            max: 20.00,
            default: 5.00,
        },
        ParamDef {
            id: 83,
            name: "lfo2 shape",
            min: 0.00,
            max: 3.00,
            default: 0.00,
        },
        ParamDef {
            id: 84,
            name: "lfo2 delay",
            min: 0.00,
            max: 4000.00,
            default: 0.00,
        },
        ParamDef {
            id: 85,
            name: "lfo2 fade",
            min: 0.00,
            max: 4000.00,
            default: 0.00,
        },
        ParamDef {
            id: 86,
            name: "lfo2 pitch",
            min: 0.00,
            max: 12.00,
            default: 0.00,
        },
        ParamDef {
            id: 87,
            name: "lfo2 mod",
            min: 0.00,
            max: 1.00,
            default: 0.00,
        },
        ParamDef {
            id: 88,
            name: "lfo2 amp",
            min: 0.00,
            max: 1.00,
            default: 0.00,
        },
        ParamDef {
            id: 89,
            name: "lfo2 filter",
            min: -4.00,
            max: 4.00,
            default: 0.00,
        },
    ];

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn ids_are_positions_and_algorithms_are_well_formed() {
            for (i, def) in TABLE.iter().enumerate() {
                assert_eq!(def.id, i as u32, "{}", def.name);
            }
            assert_eq!(TABLE.len(), OPS * PER_OP as usize + 42);
            assert_eq!(op_of(op_param(2, SUSTAIN)), Some((2, SUSTAIN)));
            assert_eq!(op_of(op_param(3, KEYSCALE)), Some((3, KEYSCALE)));
            assert_eq!(op_of(ALGO), None);
            assert_eq!(lfo_param(1, LFO_FILTER), LFO2_FILTER);
            assert_eq!(lfo_param(0, LFO_SHAPE), LFO1_SHAPE);
            assert_eq!(ALGO_NAMES.len(), ALGORITHMS.len());
            assert_eq!(WAVE_NAMES.len(), WAVE_NOISE as usize + 1);
            for algo in ALGORITHMS {
                assert!(!algo.carriers.is_empty());
                for (m, c) in algo.edges {
                    assert!(*m > *c, "modulators sit above their carriers: {m}>{c}");
                    assert!(*m < OPS && *c < OPS);
                }
                for op in 0..OPS {
                    assert!(
                        algo.carriers.contains(&op) || algo.edges.iter().any(|(m, _)| *m == op),
                        "{algo:?}: op {op} goes nowhere"
                    );
                }
            }
            assert!(is_modulator(ALGORITHMS[0], 3) && !is_modulator(ALGORITHMS[0], 0));
            assert!(!is_modulator(ALGORITHMS[7], 3));
        }
    }
}

/// sCOMP: a sine, squashed into a sound. See [`crate::scomp`].
pub mod scomp {
    use super::ParamDef;

    pub const PASSES: u32 = 0;
    pub const TAKE: u32 = 1;
    pub const DROP: u32 = 2;
    pub const DROP_MS: u32 = 3;
    pub const DECAY: u32 = 4;
    pub const FILTER: u32 = 5;
    pub const HARMONIC: u32 = 6;
    pub const RESO: u32 = 7;
    pub const SWEEP: u32 = 8;
    pub const DRIFT: u32 = 9;
    pub const THRESH: u32 = 10;
    pub const RATIO: u32 = 11;
    pub const ATTACK: u32 = 12;
    pub const RELEASE: u32 = 13;
    pub const MAKEUP: u32 = 14;
    pub const DRIVE: u32 = 15;
    pub const SHIFT: u32 = 16;
    pub const AMP_A: u32 = 17;
    pub const AMP_R: u32 = 18;
    pub const TUNE: u32 = 19;
    pub const ROOT: u32 = 20;
    pub const LEVEL: u32 = 21;
    /// A door, not a knob: turning it up opens the forge. It reads as a
    /// switch so the band's grammar reaches it like any other row, and
    /// it never stays on — the stage answers it and puts it back.
    pub const OPEN: u32 = 22;

    pub const OPEN_NAMES: &[&str] = &["-", "open"];
    pub const MAX_PASSES: usize = 8;
    pub const PASS_NAMES: &[&str] = &["1", "2", "3", "4", "5", "6", "7", "8"];
    pub const FILTER_BAND: f32 = 0.0;
    pub const FILTER_NOTCH: f32 = 1.0;
    pub const FILTER_COMB: f32 = 2.0;
    pub const FILTER_NAMES: &[&str] = &["band", "notch", "comb"];
    /// The longest a take may grow to, in seconds, however far down the
    /// passes pitch it.
    pub const MAX_TAKE_S: f32 = 8.0;
    pub const LEVEL_MAX: f32 = 2.0;

    /// The rows that are BAKED into the take: a change to any of them is
    /// a new render, not a letter. The engine never re-renders on the
    /// audio thread, so the host rebuilds the graph instead, exactly as
    /// it does for a slice table.
    pub fn baked(param: u32) -> bool {
        !matches!(param, AMP_A | AMP_R | TUNE | ROOT | LEVEL | OPEN)
    }

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: PASSES,
            name: "passes",
            min: 1.0,
            max: 8.0,
            default: 3.0,
        },
        ParamDef {
            id: TAKE,
            name: "take",
            min: 0.25,
            max: 4.0,
            default: 1.5,
        },
        ParamDef {
            id: DROP,
            name: "drop",
            min: 0.0,
            max: 36.0,
            default: 12.0,
        },
        ParamDef {
            id: DROP_MS,
            name: "dropms",
            min: 5.0,
            max: 2_000.0,
            default: 120.0,
        },
        ParamDef {
            id: DECAY,
            name: "decay",
            min: 0.05,
            max: 4.0,
            default: 1.2,
        },
        ParamDef {
            id: FILTER,
            name: "filter",
            min: 0.0,
            max: 2.0,
            default: FILTER_BAND,
        },
        ParamDef {
            id: HARMONIC,
            name: "harmonic",
            min: 0.5,
            max: 32.0,
            default: 3.0,
        },
        ParamDef {
            id: RESO,
            name: "reso",
            min: 0.5,
            max: 40.0,
            default: 8.0,
        },
        ParamDef {
            id: SWEEP,
            name: "sweep",
            min: -4.0,
            max: 4.0,
            default: -1.5,
        },
        ParamDef {
            id: DRIFT,
            name: "drift",
            min: -24.0,
            max: 24.0,
            default: 7.0,
        },
        ParamDef {
            id: THRESH,
            name: "thresh",
            min: -60.0,
            max: 0.0,
            default: -30.0,
        },
        ParamDef {
            id: RATIO,
            name: "ratio",
            min: 1.0,
            max: 100.0,
            default: 20.0,
        },
        ParamDef {
            id: ATTACK,
            name: "attack",
            min: 0.1,
            max: 100.0,
            default: 1.0,
        },
        ParamDef {
            id: RELEASE,
            name: "release",
            min: 5.0,
            max: 1_000.0,
            default: 60.0,
        },
        ParamDef {
            id: MAKEUP,
            name: "makeup",
            min: 0.0,
            max: 36.0,
            default: 18.0,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: 1.0,
            max: 32.0,
            default: 6.0,
        },
        ParamDef {
            id: SHIFT,
            name: "shift",
            min: -24.0,
            max: 24.0,
            default: -5.0,
        },
        ParamDef {
            id: AMP_A,
            name: "amp a",
            min: 0.0,
            max: 500.0,
            default: 2.0,
        },
        ParamDef {
            id: AMP_R,
            name: "amp r",
            min: 5.0,
            max: 2_000.0,
            default: 120.0,
        },
        ParamDef {
            id: TUNE,
            name: "tune",
            min: -24.0,
            max: 24.0,
            default: 0.0,
        },
        ParamDef {
            id: ROOT,
            name: "root",
            min: 0.0,
            max: 127.0,
            default: 36.0,
        },
        ParamDef {
            id: LEVEL,
            name: "level",
            min: 0.0,
            max: LEVEL_MAX,
            default: 0.8,
        },
        ParamDef {
            id: OPEN,
            name: "forge",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
    ];
}

/// The built-in sequencer synth (`Node::Seq`, the SineSynth device).
pub mod seq {
    use super::ParamDef;

    pub const GAIN: u32 = 0;
    pub const ATTACK: u32 = 1;
    pub const RELEASE: u32 = 2;

    /// Defaults match the synth's original hardcoded voice: unity gain,
    /// ~1 ms attack, ~640 ms release.
    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: GAIN,
            name: "gain",
            min: 0.0,
            max: 2.0,
            default: 1.0,
        },
        ParamDef {
            id: ATTACK,
            name: "attack",
            min: 0.05,
            max: 5_000.0,
            default: 1.0,
        },
        ParamDef {
            id: RELEASE,
            name: "release",
            min: 1.0,
            max: 30_000.0,
            default: 640.0,
        },
    ];
}

/// `Node::Reverb`.
/// The reverb: a feedback delay network, `dsp::fdn::Fdn`.
///
/// Nine rows, and the split is deliberate. The first five describe the
/// SPACE — where it starts, how big it is, how long it rings, how dark
/// it gets, and what the tail keeps of the low end. The last four
/// describe how that space is PRESENTED: how dense, how much it moves,
/// how wide, and how much of it you hear.
///
/// The old three-row table (mix, size, damp) is gone with the Freeverb
/// it drove. It had no pre-delay, so the tail started on top of the
/// source and every setting sounded like a wash; and it was mono, which
/// is the one thing a reverb cannot be.
pub mod reverb {
    use super::ParamDef;

    pub const PREDELAY: u32 = 0;
    pub const SIZE: u32 = 1;
    pub const DECAY: u32 = 2;
    pub const DAMP: u32 = 3;
    pub const LOWCUT: u32 = 4;
    pub const DIFFUSION: u32 = 5;
    pub const MODULATION: u32 = 6;
    pub const WIDTH: u32 = 7;
    pub const MIX: u32 = 8;

    /// The longest gap between the source and its room, in ms.
    ///
    /// Pre-delay is what separates a reverb from a wash: a few tens of
    /// milliseconds of silence lets the dry transient through before the
    /// room answers, which is why a vocal stays intelligible in a hall.
    pub const PREDELAY_MAX: f32 = 200.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: PREDELAY,
            name: "predelay",
            min: 0.0,
            max: PREDELAY_MAX,
            default: 20.0,
        },
        ParamDef {
            id: SIZE,
            name: "size",
            min: crate::dsp::fdn::SIZE_MIN,
            max: crate::dsp::fdn::SIZE_MAX,
            default: 1.0,
        },
        ParamDef {
            id: DECAY,
            name: "decay",
            min: crate::dsp::fdn::DECAY_MIN,
            max: crate::dsp::fdn::DECAY_MAX,
            // A medium room. Loading a reverb must not drown the track
            // it lands on.
            default: 1.8,
        },
        ParamDef {
            id: DAMP,
            name: "damp",
            // The corner of the one-pole in every feedback path. Named
            // in HERTZ rather than as a fraction, because "the tail goes
            // dark above 4 kHz" is a thing a person can picture.
            min: 400.0,
            max: 18_000.0,
            default: 5_000.0,
        },
        ParamDef {
            id: LOWCUT,
            name: "lowcut",
            // Off at the bottom. A reverb tail carrying the fundamental
            // of everything it is fed is how a mix turns to mud, and
            // this is the control that stops it.
            min: 20.0,
            max: 800.0,
            default: 120.0,
        },
        ParamDef {
            id: DIFFUSION,
            name: "diffusion",
            min: 0.0,
            max: 1.0,
            default: 0.8,
        },
        ParamDef {
            id: MODULATION,
            name: "modulation",
            // In samples of wander. Zero is a still network, which rings;
            // the top is a chorused tail.
            min: 0.0,
            max: 8.0,
            default: 2.0,
        },
        ParamDef {
            id: WIDTH,
            name: "width",
            // 0 is mono, 1 is the network's own spread, 2 over-widens.
            min: 0.0,
            max: 2.0,
            default: 1.0,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            default: 0.25,
        },
    ];
}

/// `Node::Sine`, the lab's tone generator.
pub mod sine {
    use super::ParamDef;

    pub const FREQ: u32 = 0;
    pub const AMP: u32 = 1;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: FREQ,
            name: "freq",
            min: 1.0,
            max: 20_000.0,
            default: 440.0,
        },
        ParamDef {
            id: AMP,
            name: "amp",
            min: 0.0,
            max: 1.0,
            default: 0.5,
        },
    ];
}

/// `Node::Mixer`.
pub mod mixer {
    use super::ParamDef;

    pub const GAIN: u32 = 0;

    pub const TABLE: &[ParamDef] = &[ParamDef {
        id: GAIN,
        name: "gain",
        min: 0.0,
        max: 2.0,
        default: 1.0,
    }];
}

/// `Node::Pan` — the per-track output stage: placement AND level.
///
/// Volume lives here rather than in a node of its own for the reason pan
/// does: every track already has this node, so a fader move is a param
/// letter to a permanent address instead of a schedule swap. The ceiling is
/// +6 dB in linear terms (1.995), the range every console's fader has above
/// unity.
pub mod pan {
    use super::ParamDef;

    pub const PAN: u32 = 0;
    pub const GAIN: u32 = 1;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: PAN,
            name: "pan",
            min: -1.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: GAIN,
            name: "volume",
            min: 0.0,
            max: 1.995_262_3,
            default: 1.0,
        },
    ];
}

/// `Node::AudioClip`.
pub mod clip {
    use super::ParamDef;

    pub const GAIN: u32 = 0;
    pub const FADE_IN: u32 = 1;
    pub const FADE_OUT: u32 = 2;
    pub const FADE_IN_CURVE: u32 = 3;
    pub const FADE_OUT_CURVE: u32 = 4;

    /// The longest a fade may be, in FRAMES.
    ///
    /// Frames, not seconds, because every other length a clip carries is
    /// in frames and a fade has to be compared against them — and because
    /// the node has no sample rate of its own to convert with. The
    /// ceiling is generous rather than meaningful: what actually bounds a
    /// fade is the clip it lives on, which the app clamps against.
    pub const FADE_MAX_FRAMES: f32 = 4_800_000.0;

    /// A fade's SHAPE, as one continuous family rather than a menu.
    ///
    /// ```text
    /// y(x) = x * (1 + k) / (1 + k * x)      k > -1
    /// ```
    ///
    /// It passes through both endpoints, it is monotonic for every legal
    /// `k`, and it costs one multiply and one divide per sample. `k = 0`
    /// is exactly linear; above zero it rises fast then flattens, below
    /// zero the reverse.
    ///
    /// `powf` would give the same shapes and is neither unsafe nor
    /// unbounded — it is simply a transcendental per sample per channel
    /// on every clip that has a fade, which is a real cost for no
    /// difference anyone can hear. Hence the rational curve.
    ///
    /// Here rather than in the renderer or the node because BOTH need it
    /// and they must agree: a destructive fade and a clip fade of the
    /// same shape have to produce the same samples, or committing one
    /// would change the sound.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Curve {
        /// The shape, in `-1..=1`. Zero is linear.
        pub shape: f32,
    }

    impl Default for Curve {
        fn default() -> Self {
            Self::LINEAR
        }
    }

    impl Curve {
        pub const LINEAR: Self = Self { shape: 0.0 };

        pub fn new(shape: f32) -> Self {
            Self {
                shape: if shape.is_finite() {
                    shape.clamp(-1.0, 1.0)
                } else {
                    0.0
                },
            }
        }

        /// The curve's `k`, which must stay ABOVE −1.
        ///
        /// The two halves are not the same formula, and that is not an
        /// accident. Inverting this family — reflecting the curve about
        /// the diagonal, which is what "the opposite shape" means — sends
        /// `k` to `-k / (1 + k)`. Put `k₊ = a / (1 - a)` through that and
        /// it comes back as exactly `-a`. So the negative half IS the
        /// mirror of the positive half, and it is also the only mapping
        /// that keeps `k` above −1.
        ///
        /// The obvious symmetric-looking guess, `shape / (1 - |shape|)`
        /// on both sides, sends `k` to −4 at a shape of −0.8: the
        /// denominator crosses zero a quarter of the way along, the
        /// "fade" goes to infinity and then negative, and the shape is
        /// not a fade at all.
        ///
        /// The shape is clamped just inside ±1 so the mapping stays
        /// finite — at exactly ±1 it would be a vertical step.
        fn k(self) -> f32 {
            let shape = self.shape.clamp(-0.999, 0.999);
            if shape >= 0.0 {
                shape / (1.0 - shape)
            } else {
                shape
            }
        }

        /// `y` for an `x` in `0..=1`, both ends exact.
        pub fn at(self, x: f32) -> f32 {
            let x = x.clamp(0.0, 1.0);
            let k = self.k();
            if k == 0.0 {
                return x;
            }
            x * (1.0 + k) / (1.0 + k * x)
        }
    }

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: GAIN,
            name: "gain",
            min: 0.0,
            max: 2.0,
            default: 1.0,
        },
        ParamDef {
            id: FADE_IN,
            name: "fadein",
            min: 0.0,
            max: FADE_MAX_FRAMES,
            default: 0.0,
        },
        ParamDef {
            id: FADE_OUT,
            name: "fadeout",
            min: 0.0,
            max: FADE_MAX_FRAMES,
            default: 0.0,
        },
        // The SHAPES ride letters beside the lengths, so dragging a
        // curve is heard while it is dragged rather than when the next
        // recompile catches up.
        ParamDef {
            id: FADE_IN_CURVE,
            name: "fadeincurve",
            min: -1.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: FADE_OUT_CURVE,
            name: "fadeoutcurve",
            min: -1.0,
            max: 1.0,
            default: 0.0,
        },
    ];
}

/// `Node::Filter` — the first kernel-backed effect. Beyond the knob table
/// this module owns the numbers BOTH the drawn curve and the audio path
/// derive from, so the display cannot drift from the sound: the resonance
/// mapping, the drive squash, and the slope list.
pub mod filter {
    use super::ParamDef;

    pub const MODE: u32 = 0;
    pub const SLOPE: u32 = 1;
    pub const CUTOFF: u32 = 2;
    pub const RES: u32 = 3;
    pub const DRIVE: u32 = 4;
    pub const CHARACTER: u32 = 5;
    pub const SPREAD: u32 = 6;

    /// Mode indices on the wire. Same order as the widget's mode strip.
    pub const MODE_LP: u32 = 0;
    pub const MODE_HP: u32 = 1;
    pub const MODE_BP: u32 = 2;
    pub const MODE_NOTCH: u32 = 3;

    /// Butterworth-flat: no peak at the corner. The resonance knob's
    /// audible floor — below this the resonant section stays flat, in the
    /// drawing and in the audio alike.
    pub const FLAT_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

    /// How hard drive squashes resonance in the CLEAN character. The
    /// saturation lives in the feedback path, so at full drive a peak
    /// that would be `1 + x` above flat is squashed to
    /// `1 + x / (1 + DRIVE_SQUASH)`.
    ///
    /// Every other character scales this — see [`CHARACTERS`].
    pub const DRIVE_SQUASH: f32 = 3.0;

    /// Character indices on the wire.
    pub const CHAR_CLEAN: u32 = 0;
    pub const CHAR_LADDER: u32 = 1;
    pub const CHAR_OTA: u32 = 2;
    pub const CHAR_DIODE: u32 = 3;
    pub const CHAR_MAX: u32 = CHAR_DIODE;

    /// WHAT KIND OF FILTER THIS IS, past the coefficients.
    ///
    /// Two filters with the same corner and the same slope can sound
    /// nothing alike, and the difference is almost never the response
    /// curve — it is where the nonlinearity sits, what shape it is, and
    /// what the resonance does when you lean on it. That is what a
    /// character is here: not a preset over the other knobs, but the
    /// handful of numbers the curve cannot show.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Character {
        pub name: &'static str,
        /// Multiplier on [`DRIVE_SQUASH`]. Higher means the resonance
        /// gives way sooner as the drive comes up — a ladder's peak
        /// collapses under drive, an OTA's holds on.
        pub squash: f32,
        /// Asymmetry fed to the shaper. Non-zero makes EVEN harmonics,
        /// which is what separates a buzz from a growl.
        pub bias: f32,
        /// Multiplier on the drive the shaper is given.
        pub drive_scale: f32,
        /// Which `dsp::shaper` curve, as an index into
        /// [`sat::MODE_*`](super::sat).
        pub shape: u32,
        /// How much low end the resonance eats, `0..=1` at full
        /// resonance. A real ladder thins out as it is turned up because
        /// the feedback subtracts the input; an OTA does not.
        pub bass_loss: f32,
    }

    /// The four, in wire order.
    ///
    /// These are voicings, not emulations of any particular circuit — the
    /// names say which tradition each one is reaching for, and the numbers
    /// are chosen so that the four are clearly different from one another
    /// at the same settings. That last part is the actual requirement: a
    /// character switch whose positions you cannot tell apart is a switch
    /// nobody will ever move twice.
    pub const CHARACTERS: &[Character] = &[
        // The filter as it was before there were characters: a symmetric
        // soft clip after the poles, moderate squash, no bass loss. The
        // reference, and the one to pick when the filter should get out
        // of the way.
        Character {
            name: "clean",
            squash: 1.0,
            bias: 0.0,
            drive_scale: 1.0,
            shape: super::sat::MODE_SOFT,
            bass_loss: 0.0,
        },
        // Fat, and it gives way. The peak collapses under drive and the
        // bottom thins as the resonance comes up, which together are why
        // a ladder sweep sounds like it is being played rather than set.
        Character {
            name: "ladder",
            squash: 1.7,
            bias: 0.0,
            drive_scale: 1.2,
            shape: super::sat::MODE_SOFT,
            bass_loss: 0.35,
        },
        // Bright and stubborn. Least squash of the four, so the peak
        // stays put however hard it is driven, and a cubic curve rather
        // than a tanh — a harder knee, more of the odd harmonics that
        // read as glassy.
        Character {
            name: "ota",
            squash: 0.6,
            bias: 0.0,
            drive_scale: 1.0,
            shape: super::sat::MODE_CUBIC,
            bass_loss: 0.0,
        },
        // The nasty one. Asymmetric, hardest squash, most drive: the
        // resonance folds over almost immediately and what is left is
        // buzz with a corner in it. Nobody reaches for this to be
        // tasteful.
        Character {
            name: "diode",
            squash: 2.4,
            bias: 0.30,
            drive_scale: 1.6,
            shape: super::sat::MODE_SOFT,
            bass_loss: 0.20,
        },
    ];

    pub const CHARACTER_NAMES: &[&str] = &["clean", "ladder", "ota", "diode"];

    /// The character a wire index names.
    ///
    /// Out of range gives `clean` — the FIRST, not the nearest. A stale
    /// or corrupt index must still filter, and the position it lands on
    /// should be the one that gets out of the way rather than the one
    /// that screams. Clamping to the last instead would make a truncated
    /// project file open sounding like a fuzz box.
    pub fn character(index: u32) -> &'static Character {
        CHARACTERS.get(index as usize).unwrap_or(&CHARACTERS[0])
    }

    /// The widest the two channels' corners may be pushed apart, in
    /// SEMITONES.
    ///
    /// Semitones rather than hertz, because the useful amount of spread
    /// is a musical interval and not a fixed distance: half an octave
    /// apart at 200 Hz and half an octave apart at 8 kHz are the same
    /// gesture, and 400 Hz apart is a different one at each.
    pub const SPREAD_MAX_ST: f32 = 12.0;

    /// One channel's corner, given the knob and how far to lean.
    ///
    /// `side` is -1 for left and +1 for right, so the two move in
    /// OPPOSITE directions around the cutoff the knob names — the corner
    /// you set stays the centre of what you hear, and turning spread up
    /// widens rather than detunes.
    pub fn spread_cutoff(cutoff_hz: f32, spread_st: f32, side: f32) -> f32 {
        let half = spread_st.clamp(0.0, SPREAD_MAX_ST) * 0.5 * side;
        (cutoff_hz * (half / 12.0).exp2()).clamp(20.0, 20_000.0)
    }

    /// Slope index -> filter order (poles). 6 dB per octave per pole.
    pub const SLOPE_ORDERS: &[u32] = &[1, 2, 3, 4, 6, 8];

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: MODE,
            name: "mode",
            min: 0.0,
            max: 3.0,
            default: 0.0, // lowpass
        },
        ParamDef {
            id: SLOPE,
            name: "slope",
            min: 0.0,
            max: 5.0,
            default: 3.0, // 24 dB/octave
        },
        ParamDef {
            id: CUTOFF,
            name: "cutoff",
            min: 20.0,
            max: 20_000.0,
            // Parked out of the way: loading a filter is transparent, and
            // the first knob touch is the first audible change — the
            // reverb's never-destroy-the-mix rule, applied to a filter.
            default: 20_000.0,
        },
        ParamDef {
            id: RES,
            name: "res",
            min: 0.3,
            // Just below scream: +24 dB or so of peak, matching the top of
            // the display window. Self-oscillation is a feature for a
            // later version that has a limiter behind it.
            max: 24.0,
            default: FLAT_Q,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: CHARACTER,
            name: "character",
            min: 0.0,
            max: CHAR_MAX as f32,
            // LADDER, not clean. The cutoff is parked open so the device
            // is silent on load whatever this says — and when the first
            // sweep does happen, it should sound like something rather
            // than like arithmetic. Clean is one position away for
            // anybody who wants the filter to get out of the way.
            default: CHAR_LADDER as f32,
        },
        ParamDef {
            id: SPREAD,
            name: "spread",
            min: 0.0,
            max: SPREAD_MAX_ST,
            // Zero: a stereo width effect nobody asked for is the one
            // thing worse than a mono filter, and `spread` is the knob
            // that says you did ask.
            default: 0.0,
        },
    ];

    /// The resonant section's effective Q after drive has had its say —
    /// the ONE resonance mapping. Drive squashes only the part of the
    /// resonance above flat, because that is the part living in the
    /// feedback path where the saturation is; a gentle filter driven hard
    /// does not lose its corner. Sub-flat requests floor at flat, which
    /// keeps the drawn curve and the audio identical there too.
    ///
    /// The engine feeds this straight to its resonant section; the display
    /// scales it by the section's Butterworth base via [`resonant_q`].
    /// Same function, so agreement is by construction, not by test alone.
    pub fn effective_q(q: f32, drive: f32) -> f32 {
        effective_q_for(q, drive, CHAR_CLEAN)
    }

    /// [`effective_q`] for a given character.
    ///
    /// The character scales how hard the drive squashes: a ladder's peak
    /// collapses under drive and an OTA's holds on, and that difference
    /// is most of what makes them recognisable. The clean character
    /// scales by one, so it is exactly the old behaviour and the
    /// pre-character projects that reach this path still sound the way
    /// they did.
    pub fn effective_q_for(q: f32, drive: f32, model: u32) -> f32 {
        let excess = (q.max(0.05) - FLAT_Q).max(0.0);
        let squash = DRIVE_SQUASH * character(model).squash;
        FLAT_Q + excess / (1.0 + drive.clamp(0.0, 1.0) * squash)
    }

    /// [`effective_q`] expressed against a cascade section's Butterworth
    /// base — the form the display's per-section math consumes.
    pub fn resonant_q(q: f32, base: f32, drive: f32) -> f32 {
        resonant_q_for(q, base, drive, CHAR_CLEAN)
    }

    /// [`resonant_q`] for a given character.
    pub fn resonant_q_for(q: f32, base: f32, drive: f32, model: u32) -> f32 {
        base * (effective_q_for(q, drive, model) / FLAT_Q)
    }

    /// Drive knob (0..=1) -> waveshaper drive. Full drive pushes ~24 dB
    /// into the soft clipper — growl territory, not bitcrush.
    pub fn shaper_drive(drive: f32) -> f32 {
        1.0 + drive.clamp(0.0, 1.0) * 15.0
    }

    /// [`shaper_drive`] scaled by the character.
    pub fn shaper_drive_for(drive: f32, model: u32) -> f32 {
        let scale = character(model).drive_scale;
        (1.0 + drive.clamp(0.0, 1.0) * 15.0 * scale)
            .clamp(crate::dsp::shaper::DRIVE_MIN, crate::dsp::shaper::DRIVE_MAX)
    }

    /// Slope index -> order, clamped to the steepest available.
    pub fn slope_order(index: u32) -> u32 {
        SLOPE_ORDERS[(index as usize).min(SLOPE_ORDERS.len() - 1)]
    }

    /// The level a resonant filter GIVES UP as the resonance comes up, as
    /// a linear gain — [`Character::bass_loss`] with the knob applied.
    ///
    /// A ladder thins out as its feedback subtracts the input, an OTA
    /// does not; only a lowpass has a bottom end to lose, so the other
    /// three modes come back at unity.
    ///
    /// The one place this arithmetic lives: the node multiplies its
    /// filtered block by it and the card's drawn curve shifts by the same
    /// number, so the picture and the audio agree by construction rather
    /// than by inspection. Before it was shared, a ladder at Q 8 measured
    /// a decibel under what the display drew and nothing in the app could
    /// have told you which one was lying.
    pub fn resonance_loss(res: f32, model: u32, mode: u32) -> f32 {
        let loss = character(model).bass_loss;
        if loss <= 0.0 || mode != MODE_LP {
            return 1.0;
        }
        // Against the knob's own top, so "full resonance" means the
        // stop and not an arbitrary Q.
        let span = TABLE[RES as usize].max;
        let excess = ((res.max(0.05) - FLAT_Q) / span).clamp(0.0, 1.0);
        1.0 - loss * excess
    }
}

/// `Node::Eq` — the eight-band equaliser.
///
/// # Why the table is eight identical groups and not eight of anything else
///
/// The band count is FIXED. Every target string, every modulation wire and
/// `shape_hash` itself are built on a static parameter table, so a band
/// that could come and go would be a parameter that could come and go —
/// and an automation lane pointed at a band that no longer exists is the
/// kind of silent failure this codebase spends its comments avoiding.
/// Eight permanent bands, each with an ON switch, costs one filter pair
/// per ENABLED band and nothing at all for the rest.
///
/// Ids are `band * PER_BAND + slot`, in table order, so `TABLE[id]` is
/// that id's row — the same property `echo` and `sat` rely on.
pub mod eq {
    use super::ParamDef;

    /// How many bands the equaliser has. Not a maximum: they all exist,
    /// all the time, and an unused one is switched off rather than absent.
    pub const BANDS: usize = 8;
    /// How many parameters each band carries.
    pub const PER_BAND: u32 = 5;

    // The slot within a band.
    pub const ON: u32 = 0;
    pub const TYPE: u32 = 1;
    pub const FREQ: u32 = 2;
    pub const GAIN: u32 = 3;
    pub const Q: u32 = 4;

    /// The `ParamChange` id of one band's one slot.
    pub const fn id(band: usize, slot: u32) -> u32 {
        band as u32 * PER_BAND + slot
    }

    /// Which band an id belongs to, and which slot of it. `None` for the
    /// output trim, which belongs to no band.
    pub const fn split(id: u32) -> Option<(usize, u32)> {
        if id >= OUT {
            return None;
        }
        Some(((id / PER_BAND) as usize, id % PER_BAND))
    }

    /// The output trim, after every band. The one row that is not a band.
    pub const OUT: u32 = BANDS as u32 * PER_BAND;

    // The shapes a band can take, as indices into `TYPE_NAMES`.
    pub const TYPE_LO_CUT_12: u32 = 0;
    pub const TYPE_LO_CUT_48: u32 = 1;
    pub const TYPE_LO_SHELF: u32 = 2;
    pub const TYPE_BELL: u32 = 3;
    pub const TYPE_NOTCH: u32 = 4;
    pub const TYPE_HI_SHELF: u32 = 5;
    pub const TYPE_HI_CUT_12: u32 = 6;
    pub const TYPE_HI_CUT_48: u32 = 7;

    /// The eight shapes, in the order a band's type cell steps through
    /// them: low end first, high end last, so stepping the control walks
    /// up the spectrum the way the picture does.
    pub const TYPE_NAMES: &[&str] = &[
        "lo cut 12",
        "lo cut 48",
        "lo shelf",
        "bell",
        "notch",
        "hi shelf",
        "hi cut 12",
        "hi cut 48",
    ];

    /// Whether this shape applies a GAIN. The cuts and the notch do not —
    /// they take away what they take away — so their gain cell is dead
    /// and their curve handle only moves sideways.
    pub const fn has_gain(shape: u32) -> bool {
        matches!(shape, TYPE_LO_SHELF | TYPE_BELL | TYPE_HI_SHELF)
    }

    /// The order of the cut this shape asks for, in poles. Zero for
    /// everything that is not a cut.
    pub const fn cut_order(shape: u32) -> u32 {
        match shape {
            TYPE_LO_CUT_12 | TYPE_HI_CUT_12 => 2,
            TYPE_LO_CUT_48 | TYPE_HI_CUT_48 => 8,
            _ => 0,
        }
    }

    /// Whether this shape is a cut that keeps the HIGH end.
    pub const fn is_highpass(shape: u32) -> bool {
        matches!(shape, TYPE_LO_CUT_12 | TYPE_LO_CUT_48)
    }

    /// The lowest and highest corner a band will accept. The audible band
    /// with a little either side, which is also exactly what the display
    /// draws — a band you cannot see is a band you cannot get back.
    pub const MIN_HZ: f32 = 20.0;
    pub const MAX_HZ: f32 = 20_000.0;

    /// The most a band will boost or cut, in dB.
    ///
    /// Eighteen rather than a wilder figure: past this an EQ band is
    /// being used as a filter or a fader, and both of those exist.
    pub const MAX_GAIN_DB: f32 = 18.0;

    /// The Q range. The bottom is a very wide, gentle shape; the top
    /// rings hard enough to use as a surgical notch.
    pub const MIN_Q: f32 = 0.1;
    pub const MAX_Q: f32 = 18.0;

    /// Butterworth, the Q that peaks at nothing. Every band's default,
    /// so a freshly switched-on band is the plain shape its name says.
    pub const FLAT_Q: f32 = core::f32::consts::FRAC_1_SQRT_2;

    /// The most the output trim will move, in dB.
    pub const MAX_OUT_DB: f32 = 24.0;

    pub const TABLE: &[ParamDef] = &[
        // ---- band 1: a low cut, out of the way until you want it. ----
        ParamDef {
            id: id(0, ON),
            name: "on1",
            min: 0.0,
            max: 1.0,
            // OFF. A fresh equaliser is a wire: eight bands running flat
            // would cost sixteen filters to change nothing.
            default: 0.0,
        },
        ParamDef {
            id: id(0, TYPE),
            name: "type1",
            min: 0.0,
            max: (TYPE_NAMES.len() - 1) as f32,
            default: TYPE_LO_CUT_12 as f32,
        },
        ParamDef {
            id: id(0, FREQ),
            name: "freq1",
            min: MIN_HZ,
            max: MAX_HZ,
            default: 80.0,
        },
        ParamDef {
            id: id(0, GAIN),
            name: "gain1",
            min: -MAX_GAIN_DB,
            max: MAX_GAIN_DB,
            default: 0.0,
        },
        ParamDef {
            id: id(0, Q),
            name: "q1",
            min: MIN_Q,
            max: MAX_Q,
            default: FLAT_Q,
        },
        // ---- band 2: the low shelf: weight, or the lack of it. ----
        ParamDef {
            id: id(1, ON),
            name: "on2",
            min: 0.0,
            max: 1.0,
            // OFF. A fresh equaliser is a wire: eight bands running flat
            // would cost sixteen filters to change nothing.
            default: 0.0,
        },
        ParamDef {
            id: id(1, TYPE),
            name: "type2",
            min: 0.0,
            max: (TYPE_NAMES.len() - 1) as f32,
            default: TYPE_LO_SHELF as f32,
        },
        ParamDef {
            id: id(1, FREQ),
            name: "freq2",
            min: MIN_HZ,
            max: MAX_HZ,
            default: 150.0,
        },
        ParamDef {
            id: id(1, GAIN),
            name: "gain2",
            min: -MAX_GAIN_DB,
            max: MAX_GAIN_DB,
            default: 0.0,
        },
        ParamDef {
            id: id(1, Q),
            name: "q2",
            min: MIN_Q,
            max: MAX_Q,
            default: FLAT_Q,
        },
        // ---- band 3: the low mids, where a mix goes muddy. ----
        ParamDef {
            id: id(2, ON),
            name: "on3",
            min: 0.0,
            max: 1.0,
            // OFF. A fresh equaliser is a wire: eight bands running flat
            // would cost sixteen filters to change nothing.
            default: 0.0,
        },
        ParamDef {
            id: id(2, TYPE),
            name: "type3",
            min: 0.0,
            max: (TYPE_NAMES.len() - 1) as f32,
            default: TYPE_BELL as f32,
        },
        ParamDef {
            id: id(2, FREQ),
            name: "freq3",
            min: MIN_HZ,
            max: MAX_HZ,
            default: 350.0,
        },
        ParamDef {
            id: id(2, GAIN),
            name: "gain3",
            min: -MAX_GAIN_DB,
            max: MAX_GAIN_DB,
            default: 0.0,
        },
        ParamDef {
            id: id(2, Q),
            name: "q3",
            min: MIN_Q,
            max: MAX_Q,
            default: FLAT_Q,
        },
        // ---- band 4: the box. ----
        ParamDef {
            id: id(3, ON),
            name: "on4",
            min: 0.0,
            max: 1.0,
            // OFF. A fresh equaliser is a wire: eight bands running flat
            // would cost sixteen filters to change nothing.
            default: 0.0,
        },
        ParamDef {
            id: id(3, TYPE),
            name: "type4",
            min: 0.0,
            max: (TYPE_NAMES.len() - 1) as f32,
            default: TYPE_BELL as f32,
        },
        ParamDef {
            id: id(3, FREQ),
            name: "freq4",
            min: MIN_HZ,
            max: MAX_HZ,
            default: 800.0,
        },
        ParamDef {
            id: id(3, GAIN),
            name: "gain4",
            min: -MAX_GAIN_DB,
            max: MAX_GAIN_DB,
            default: 0.0,
        },
        ParamDef {
            id: id(3, Q),
            name: "q4",
            min: MIN_Q,
            max: MAX_Q,
            default: FLAT_Q,
        },
        // ---- band 5: presence. ----
        ParamDef {
            id: id(4, ON),
            name: "on5",
            min: 0.0,
            max: 1.0,
            // OFF. A fresh equaliser is a wire: eight bands running flat
            // would cost sixteen filters to change nothing.
            default: 0.0,
        },
        ParamDef {
            id: id(4, TYPE),
            name: "type5",
            min: 0.0,
            max: (TYPE_NAMES.len() - 1) as f32,
            default: TYPE_BELL as f32,
        },
        ParamDef {
            id: id(4, FREQ),
            name: "freq5",
            min: MIN_HZ,
            max: MAX_HZ,
            default: 2000.0,
        },
        ParamDef {
            id: id(4, GAIN),
            name: "gain5",
            min: -MAX_GAIN_DB,
            max: MAX_GAIN_DB,
            default: 0.0,
        },
        ParamDef {
            id: id(4, Q),
            name: "q5",
            min: MIN_Q,
            max: MAX_Q,
            default: FLAT_Q,
        },
        // ---- band 6: edge. ----
        ParamDef {
            id: id(5, ON),
            name: "on6",
            min: 0.0,
            max: 1.0,
            // OFF. A fresh equaliser is a wire: eight bands running flat
            // would cost sixteen filters to change nothing.
            default: 0.0,
        },
        ParamDef {
            id: id(5, TYPE),
            name: "type6",
            min: 0.0,
            max: (TYPE_NAMES.len() - 1) as f32,
            default: TYPE_BELL as f32,
        },
        ParamDef {
            id: id(5, FREQ),
            name: "freq6",
            min: MIN_HZ,
            max: MAX_HZ,
            default: 5000.0,
        },
        ParamDef {
            id: id(5, GAIN),
            name: "gain6",
            min: -MAX_GAIN_DB,
            max: MAX_GAIN_DB,
            default: 0.0,
        },
        ParamDef {
            id: id(5, Q),
            name: "q6",
            min: MIN_Q,
            max: MAX_Q,
            default: FLAT_Q,
        },
        // ---- band 7: air, as a shelf rather than a bump. ----
        ParamDef {
            id: id(6, ON),
            name: "on7",
            min: 0.0,
            max: 1.0,
            // OFF. A fresh equaliser is a wire: eight bands running flat
            // would cost sixteen filters to change nothing.
            default: 0.0,
        },
        ParamDef {
            id: id(6, TYPE),
            name: "type7",
            min: 0.0,
            max: (TYPE_NAMES.len() - 1) as f32,
            default: TYPE_HI_SHELF as f32,
        },
        ParamDef {
            id: id(6, FREQ),
            name: "freq7",
            min: MIN_HZ,
            max: MAX_HZ,
            default: 10000.0,
        },
        ParamDef {
            id: id(6, GAIN),
            name: "gain7",
            min: -MAX_GAIN_DB,
            max: MAX_GAIN_DB,
            default: 0.0,
        },
        ParamDef {
            id: id(6, Q),
            name: "q7",
            min: MIN_Q,
            max: MAX_Q,
            default: FLAT_Q,
        },
        // ---- band 8: a high cut at the top of the band. ----
        ParamDef {
            id: id(7, ON),
            name: "on8",
            min: 0.0,
            max: 1.0,
            // OFF. A fresh equaliser is a wire: eight bands running flat
            // would cost sixteen filters to change nothing.
            default: 0.0,
        },
        ParamDef {
            id: id(7, TYPE),
            name: "type8",
            min: 0.0,
            max: (TYPE_NAMES.len() - 1) as f32,
            default: TYPE_HI_CUT_12 as f32,
        },
        ParamDef {
            id: id(7, FREQ),
            name: "freq8",
            min: MIN_HZ,
            max: MAX_HZ,
            default: 16000.0,
        },
        ParamDef {
            id: id(7, GAIN),
            name: "gain8",
            min: -MAX_GAIN_DB,
            max: MAX_GAIN_DB,
            default: 0.0,
        },
        ParamDef {
            id: id(7, Q),
            name: "q8",
            min: MIN_Q,
            max: MAX_Q,
            default: FLAT_Q,
        },
        // ---- and the one row that belongs to no band. ----
        ParamDef {
            id: OUT,
            name: "out",
            min: -MAX_OUT_DB,
            max: MAX_OUT_DB,
            // Unity. An equaliser that changed the level before you
            // touched it would make every A/B a lie.
            default: 0.0,
        },
    ];
}

/// `Node::Glue` — the bus compressor.
///
/// Beyond the knob table this module owns the numbers BOTH the drawn
/// transfer curve and the audio path derive from, so the display cannot
/// drift from the sound: the switch positions, and the KNEE, which is not
/// a control at all but a consequence of the ratio.
///
/// # Why attack and release are switches
///
/// Because the unit this models has switches. A bus compressor's attack
/// is not a continuous search — it is a choice between "let the transient
/// through" and "catch it", and the six or seven positions on the panel
/// are the ones that have been useful for forty years. A continuous knob
/// here would be more freedom and less help: every position between 10
/// and 30 ms is a position nobody can hear the point of, and the ones
/// that matter would be harder to land on.
pub mod glue {
    use super::ParamDef;

    pub const THRESHOLD: u32 = 0;
    pub const RATIO: u32 = 1;
    pub const ATTACK: u32 = 2;
    pub const RELEASE: u32 = 3;
    pub const MAKEUP: u32 = 4;
    pub const DRY_WET: u32 = 5;
    pub const RANGE: u32 = 6;
    pub const CLIP: u32 = 7;
    pub const SC_HP: u32 = 8;

    /// The three ratios, and their names. A bus compressor offers a
    /// choice, not a sweep: 2:1 glues, 4:1 controls, 10:1 limits.
    pub const RATIO_VALUES: &[f32] = &[2.0, 4.0, 10.0];
    pub const RATIO_NAMES: &[&str] = &["2:1", "4:1", "10:1"];

    /// The attack positions, in MILLISECONDS.
    ///
    /// The fastest is half a sample at 48 kHz, which is not a mistake:
    /// it is what "instant" costs to write down, and the ballistics
    /// kernel treats any sub-sample time as instant.
    pub const ATTACK_MS: &[f32] = &[0.01, 0.1, 0.3, 1.0, 3.0, 10.0, 30.0];
    /// Bare numbers: the unit rides the RAIL'S LABEL ("attack ms"), not
    /// every segment. Seven segments each carrying "0.01 ms" is a rail
    /// half again as wide, for a unit that cannot change between
    /// positions — and the card has two of these to fit side by side.
    /// With their unit, because a CELL prints the selected one ONCE.
    ///
    /// These were briefly bare, back when the card drew all seven at
    /// once on a rail and the unit would have been printed seven times
    /// for a figure that cannot change between positions. A cell shows
    /// only the position you are on, so it can afford to say what it is
    /// — and a bare "0.3" on a compressor is ambiguous in a way
    /// "0.3 ms" is not.
    pub const ATTACK_NAMES: &[&str] = &[
        "0.01 ms", "0.1 ms", "0.3 ms", "1 ms", "3 ms", "10 ms", "30 ms",
    ];

    /// The release positions, in SECONDS — and then AUTO, which is not a
    /// time at all.
    ///
    /// Indexed together because they are one switch on the panel, and
    /// the last position is the one that matters most: see
    /// [`is_auto`] and `dsp::dynamics::Ballistics`.
    pub const RELEASE_S: &[f32] = &[0.1, 0.2, 0.4, 0.6, 0.8, 1.2];
    /// Bare numbers for the reason [`ATTACK_NAMES`] gives — except the
    /// last, which is not a number and says so.
    /// With their unit, for the reason [`ATTACK_NAMES`] gives — except
    /// the last, which is not a time and says so.
    pub const RELEASE_NAMES: &[&str] =
        &["0.1 s", "0.2 s", "0.4 s", "0.6 s", "0.8 s", "1.2 s", "auto"];

    /// Which release position means "program dependent".
    pub const RELEASE_AUTO: u32 = 6;

    /// Whether this release position is AUTO rather than a time.
    pub const fn is_auto(index: u32) -> bool {
        index >= RELEASE_AUTO
    }

    /// The release time a position asks for, in ms. Auto has no time of
    /// its own — the kernel's two poles are the answer — so it reports
    /// the middle of the range, which is what the auto poles bracket and
    /// what a display should print if it prints anything.
    pub fn release_ms(index: u32) -> f32 {
        let i = (index as usize).min(RELEASE_S.len() - 1);
        RELEASE_S.get(i).copied().unwrap_or(0.4) * 1_000.0
    }

    /// The attack time a position asks for, in ms.
    pub fn attack_ms(index: u32) -> f32 {
        let i = (index as usize).min(ATTACK_MS.len() - 1);
        ATTACK_MS.get(i).copied().unwrap_or(10.0)
    }

    /// The ratio a position asks for.
    pub fn ratio(index: u32) -> f32 {
        let i = (index as usize).min(RATIO_VALUES.len() - 1);
        RATIO_VALUES.get(i).copied().unwrap_or(4.0)
    }

    /// THE KNEE IS NOT A CONTROL. It follows the ratio.
    ///
    /// A gentle ratio wants a wide, soft knee — that combination is what
    /// "glue" means, a compressor that is always slightly working and
    /// never announces itself. A hard ratio wants a narrow one, because
    /// at 10:1 the point IS the corner: you are asking it to stop the
    /// signal, and a soft knee would start stopping it long before the
    /// threshold you set.
    ///
    /// One switch changing two things is the unit's own behaviour, not a
    /// simplification of it — and it lives here rather than in the node
    /// so the curve on screen is drawn from the same figure the audio
    /// uses. A display that quietly disagreed about the knee would be
    /// wrong exactly where a compressor is hardest to hear.
    pub fn knee_db(ratio_index: u32) -> f32 {
        match ratio_index {
            0 => 18.0,
            1 => 10.0,
            _ => 4.0,
        }
    }

    /// The most gain reduction [`RANGE`] will allow, in dB. At the top
    /// the cap is off the end of anything audible; at 0 the compressor
    /// is switched off in all but name.
    pub const RANGE_MAX_DB: f32 = 60.0;

    /// The sidechain high-pass corner at which the filter is OFF.
    ///
    /// A 6 dB/octave corner at 20 Hz takes nothing audible off a
    /// detector, so the bottom of the range IS the off position and the
    /// control needs no separate switch beside it.
    pub const SC_HP_OFF_HZ: f32 = 20.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: THRESHOLD,
            name: "threshold",
            min: -60.0,
            max: 10.0,
            // ZERO, so a freshly loaded compressor is very nearly a wire:
            // it costs the CPU but it does not change the mix until you
            // ask it to. The same rule the equaliser's off bands follow.
            default: 0.0,
        },
        ParamDef {
            id: RATIO,
            name: "ratio",
            min: 0.0,
            max: (RATIO_VALUES.len() - 1) as f32,
            // 4:1, the middle position and the one an SSL sits at.
            default: 1.0,
        },
        ParamDef {
            id: ATTACK,
            name: "attack",
            min: 0.0,
            max: (ATTACK_MS.len() - 1) as f32,
            // 10 ms: slow enough to let a transient through, which is
            // what a BUS compressor is for. A fast attack on a mix bus
            // flattens the drums and everyone blames the compressor.
            default: 5.0,
        },
        ParamDef {
            id: RELEASE,
            name: "release",
            min: 0.0,
            max: (RELEASE_NAMES.len() - 1) as f32,
            // AUTO, the classic starting point: it is the setting that
            // makes the unit forgiving of material it has never heard.
            default: RELEASE_AUTO as f32,
        },
        ParamDef {
            id: MAKEUP,
            name: "makeup",
            // Negative is allowed, unlike the unit this models, because
            // there is no console fader after it here — a device that
            // can only go up leaves you leaving the card to come back
            // down.
            min: -12.0,
            max: 24.0,
            default: 0.0,
        },
        ParamDef {
            id: DRY_WET,
            name: "drywet",
            min: 0.0,
            max: 100.0,
            // Fully wet: parallel compression is a thing you ask for.
            default: 100.0,
        },
        ParamDef {
            id: RANGE,
            name: "range",
            min: 0.0,
            max: RANGE_MAX_DB,
            // Uncapped. The cap is how the unit's character is dialled
            // BACK, so it starts out of the way.
            default: RANGE_MAX_DB,
        },
        ParamDef {
            id: CLIP,
            name: "clip",
            min: 0.0,
            max: 1.0,
            // Off. A soft clipper that arrived switched on would change
            // the sound of every project that loads one.
            default: 0.0,
        },
        ParamDef {
            id: SC_HP,
            name: "schp",
            min: SC_HP_OFF_HZ,
            max: 2_000.0,
            // Off, at the bottom of its own range.
            default: SC_HP_OFF_HZ,
        },
    ];
}

/// `Node::Sat` — the saturator. A transfer curve, oversampled.
///
/// # The table speaks the KERNEL's units
///
/// Drive is `1..32` and bias `-0.9..0.9` because those are
/// [`crate::dsp::shaper::DRIVE_MIN`]/[`DRIVE_MAX`](crate::dsp::shaper::DRIVE_MAX)
/// and [`BIAS_MAX`](crate::dsp::shaper::BIAS_MAX) — the same numbers the
/// widget draws its curve with. The filter's drive knob is `0..1` and
/// converts through [`filter::shaper_drive`] because it is a filter
/// SEASONING; this device IS the shaper, so a second mapping between the
/// knob and the curve would be one more place for the drawing and the
/// audio to disagree. The agreement test depends on the clamps agreeing,
/// not merely the arithmetic.
/// The kick drum synth.
///
/// One voice, one shot, fixed order:
///
/// ```text
/// pitch env A (fast) ─┐
/// pitch env B (slow) ─┴─▶ sine ──▶ amp env ─┐
/// white noise ── click env ─────────────────┴─▶ DISPERSER ─▶ SATURATOR ─▶ out
/// ```
///
/// TWO pitch envelopes because a kick needs two different drops at once:
/// a very fast one, a few milliseconds, that is heard as the beater
/// hitting the skin, and a slow one, a few dozen, that is heard as the
/// body falling to its tuned note. One envelope can be either but not
/// both — set it fast and the body has no weight, set it slow and the
/// attack turns to a woolly swoop.
///
/// The DISPERSER is last before the saturator and tuned to a HARMONIC of
/// the note, so the phase smear it adds sits on the drum rather than
/// beside it. See [`dsp::filters::Disperser`](crate::dsp::filters::Disperser).
pub mod kick {
    use super::ParamDef;

    pub const TUNE: u32 = 0;
    pub const AMP_DECAY: u32 = 1;
    pub const PITCH_A_DEPTH: u32 = 2;
    pub const PITCH_A_DECAY: u32 = 3;
    pub const PITCH_B_DEPTH: u32 = 4;
    pub const PITCH_B_DECAY: u32 = 5;
    pub const CLICK_LEVEL: u32 = 6;
    pub const CLICK_DECAY: u32 = 7;
    pub const DISP_STAGES: u32 = 8;
    pub const DISP_HARMONIC: u32 = 9;
    pub const DISP_Q: u32 = 10;
    pub const DRIVE: u32 = 11;
    pub const GAIN: u32 = 12;

    /// The tuned fundamental's window, in Hz. The bottom is below what
    /// most systems reproduce and the top is where a kick stops being one
    /// — a range wide enough for an 808 and a techno thump both.
    pub const TUNE_MIN: f32 = 20.0;
    pub const TUNE_MAX: f32 = 200.0;

    /// How far a pitch envelope can throw the fundamental, in semitones.
    ///
    /// The brief's kick recipe asks for ±48, and the fast envelope wants
    /// most of it: a 36-semitone drop from 50 Hz reaches 400 Hz, which is
    /// the click of a beater rather than a pitch.
    pub const PITCH_DEPTH_MAX: f32 = 48.0;

    /// The most allpass sections the disperser will run, as a float for
    /// the table. Mirrors the kernel's own cap so the knob cannot ask for
    /// a stage that does not exist.
    pub const DISP_STAGES_MAX: f32 = crate::dsp::filters::DISPERSER_MAX_STAGES as f32;

    /// Which harmonic of the tuned note the disperser sits on.
    ///
    /// 1 is the fundamental, where the smear is longest and reads as a
    /// pitch drop; higher harmonics move it up into the click, where it
    /// reads as a metallic zip. Integer harmonics rather than a free
    /// frequency because the point is to stay MUSICALLY attached to the
    /// drum — a free knob is a knob you have to re-tune every time the
    /// kick moves.
    pub const DISP_HARMONIC_MAX: f32 = 8.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: TUNE,
            name: "tune",
            min: TUNE_MIN,
            max: TUNE_MAX,
            // Around the low G that most kicks sit near.
            default: 50.0,
        },
        ParamDef {
            id: AMP_DECAY,
            name: "decay",
            min: 20.0,
            max: 2_000.0,
            default: 320.0,
        },
        ParamDef {
            id: PITCH_A_DEPTH,
            name: "punchdepth",
            min: 0.0,
            max: PITCH_DEPTH_MAX,
            default: 32.0,
        },
        ParamDef {
            id: PITCH_A_DECAY,
            name: "punchtime",
            // Down to a quarter of a millisecond: at the bottom this is a
            // click rather than a pitch drop, which is the point.
            min: 0.25,
            max: 60.0,
            default: 6.0,
        },
        ParamDef {
            id: PITCH_B_DEPTH,
            name: "sweepdepth",
            min: 0.0,
            max: PITCH_DEPTH_MAX,
            default: 12.0,
        },
        ParamDef {
            id: PITCH_B_DECAY,
            name: "sweeptime",
            min: 5.0,
            max: 500.0,
            default: 55.0,
        },
        ParamDef {
            id: CLICK_LEVEL,
            name: "click",
            min: 0.0,
            max: 1.0,
            default: 0.35,
        },
        ParamDef {
            id: CLICK_DECAY,
            // VERY short: the top of the range is still under a fiftieth
            // of a second. A noise burst longer than that stops being a
            // click and starts being a snare.
            name: "clicktime",
            min: 0.2,
            max: 20.0,
            default: 2.0,
        },
        ParamDef {
            id: DISP_STAGES,
            name: "disperse",
            min: 0.0,
            max: DISP_STAGES_MAX,
            // Off by default: the disperser is the flavour, not the drum.
            default: 0.0,
        },
        ParamDef {
            id: DISP_HARMONIC,
            name: "harmonic",
            min: 1.0,
            max: DISP_HARMONIC_MAX,
            default: 2.0,
        },
        ParamDef {
            id: DISP_Q,
            name: "spread",
            min: 0.3,
            max: 12.0,
            default: 2.0,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: crate::dsp::shaper::DRIVE_MIN,
            max: crate::dsp::shaper::DRIVE_MAX,
            default: 1.0,
        },
        ParamDef {
            id: GAIN,
            name: "gain",
            min: 0.0,
            max: 2.0,
            default: 0.9,
        },
    ];
}

/// The snare drum synth (`Node::Snare`).
///
/// A snare is TWO instruments struck at once and mixed: a tuned shell,
/// which is two detuned membrane modes, and the wire snares underneath,
/// which are band-passed noise. Every row here belongs to one or the
/// other, and the SNAP row is the balance between them — the single knob
/// that walks a drum from a tom to a rimshot.
pub mod snare {
    use super::ParamDef;

    pub const TUNE: u32 = 0;
    pub const RATIO: u32 = 1;
    pub const TONE_DECAY: u32 = 2;
    pub const BEND: u32 = 3;
    pub const BEND_TIME: u32 = 4;
    pub const SNAP: u32 = 5;
    pub const SNAP_DECAY: u32 = 6;
    pub const NOISE_TONE: u32 = 7;
    pub const NOISE_Q: u32 = 8;
    pub const DRIVE: u32 = 9;
    pub const GAIN: u32 = 10;

    /// The shell's fundamental, in Hz. A 14" snare sits near 180 and a
    /// piccolo near 300; the bottom of the range is a floor tom's
    /// territory and the top is a rim.
    pub const TUNE_MIN: f32 = 90.0;
    pub const TUNE_MAX: f32 = 400.0;

    /// How far above the fundamental the SECOND shell mode sits.
    ///
    /// A real drumhead's modes are not harmonic — the second circular
    /// mode of an ideal membrane is 1.59 times the first, not 2. That
    /// inharmonicity is why a snare reads as a drum rather than a pitch,
    /// and why this is a free ratio instead of a harmonic count. The
    /// default is the 180/330 pair most drum machines shipped.
    pub const RATIO_MIN: f32 = 1.0;
    pub const RATIO_MAX: f32 = 3.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: TUNE,
            name: "tune",
            min: TUNE_MIN,
            max: TUNE_MAX,
            default: 180.0,
        },
        ParamDef {
            id: RATIO,
            name: "ratio",
            min: RATIO_MIN,
            max: RATIO_MAX,
            default: 1.833,
        },
        ParamDef {
            id: TONE_DECAY,
            name: "shell",
            min: 20.0,
            max: 800.0,
            default: 120.0,
        },
        ParamDef {
            id: BEND,
            name: "bend",
            min: 0.0,
            max: 24.0,
            default: 6.0,
        },
        ParamDef {
            id: BEND_TIME,
            name: "bendtime",
            min: 1.0,
            max: 100.0,
            default: 20.0,
        },
        ParamDef {
            id: SNAP,
            name: "snap",
            min: 0.0,
            max: 1.0,
            default: 0.7,
        },
        ParamDef {
            id: SNAP_DECAY,
            // LONGER than the shell by default, and that is the sound: a
            // snare's wires rattle on after the head has stopped, and a
            // noise decay shorter than the shell's is a tom with a hiss
            // on the front.
            name: "snaptime",
            min: 20.0,
            max: 1_200.0,
            default: 180.0,
        },
        ParamDef {
            id: NOISE_TONE,
            name: "noise",
            min: 300.0,
            max: 8_000.0,
            default: 1_800.0,
        },
        ParamDef {
            id: NOISE_Q,
            name: "width",
            min: 0.3,
            max: 8.0,
            default: 0.9,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: crate::dsp::shaper::DRIVE_MIN,
            max: crate::dsp::shaper::DRIVE_MAX,
            default: 1.0,
        },
        ParamDef {
            id: GAIN,
            name: "gain",
            min: 0.0,
            max: 2.0,
            default: 0.9,
        },
    ];
}

/// The tom synth (`Node::Tom`).
///
/// The simplest drum in the rack, and deliberately: one sine, one pitch
/// bend, one short noise attack for the stick, one lowpass for the skin.
/// A tom that needs more rows than this is a kick with the wrong name.
pub mod tom {
    use super::ParamDef;

    pub const TUNE: u32 = 0;
    pub const DECAY: u32 = 1;
    pub const BEND: u32 = 2;
    pub const BEND_TIME: u32 = 3;
    pub const STICK: u32 = 4;
    pub const STICK_DECAY: u32 = 5;
    pub const TONE: u32 = 6;
    pub const DRIVE: u32 = 7;
    pub const GAIN: u32 = 8;

    /// The whole tom family in one range: a 16" floor tom at the bottom,
    /// a rack tom in the middle, a high timbale at the top.
    pub const TUNE_MIN: f32 = 40.0;
    pub const TUNE_MAX: f32 = 400.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: TUNE,
            name: "tune",
            min: TUNE_MIN,
            max: TUNE_MAX,
            default: 120.0,
        },
        ParamDef {
            id: DECAY,
            name: "decay",
            min: 40.0,
            max: 2_000.0,
            default: 400.0,
        },
        ParamDef {
            id: BEND,
            // SMALL by default. A tom bends — every struck drum does —
            // but a tom with a kick's 32 semitones on it is a kick.
            name: "bend",
            min: 0.0,
            max: 24.0,
            default: 5.0,
        },
        ParamDef {
            id: BEND_TIME,
            name: "bendtime",
            min: 5.0,
            max: 200.0,
            default: 40.0,
        },
        ParamDef {
            id: STICK,
            name: "stick",
            min: 0.0,
            max: 1.0,
            default: 0.15,
        },
        ParamDef {
            id: STICK_DECAY,
            name: "sticktime",
            min: 1.0,
            max: 80.0,
            default: 8.0,
        },
        ParamDef {
            id: TONE,
            name: "tone",
            min: 200.0,
            max: 12_000.0,
            default: 4_000.0,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: crate::dsp::shaper::DRIVE_MIN,
            max: crate::dsp::shaper::DRIVE_MAX,
            default: 1.0,
        },
        ParamDef {
            id: GAIN,
            name: "gain",
            min: 0.0,
            max: 2.0,
            default: 0.9,
        },
    ];
}

/// The 808 hi-hat (`Node::Hat`).
///
/// Modelled on the TR-808's actual circuit rather than on the usual
/// filtered-noise approximation, because the two do not sound alike. The
/// 808 has no noise source in its hat at all: it sums SIX SQUARE
/// OSCILLATORS at fixed, mutually inharmonic frequencies, and the metallic
/// clang everyone recognises is those six squares beating against each
/// other. Filtered white noise gives a "tss" with no pitch in it; this
/// gives the 808's.
///
/// The six frequencies are the machine's own, set by its oscillator
/// bank's timing components: see [`RATIOS`].
pub mod hat {
    use super::ParamDef;

    pub const TUNE: u32 = 0;
    pub const CLOSED_DECAY: u32 = 1;
    pub const OPEN_DECAY: u32 = 2;
    pub const BP_HZ: u32 = 3;
    pub const BP_Q: u32 = 4;
    pub const HP_HZ: u32 = 5;
    pub const DRIVE: u32 = 6;
    pub const GAIN: u32 = 7;

    /// The TR-808's six hi-hat oscillator frequencies, in Hz.
    ///
    /// These are the measured free-running frequencies of the machine's
    /// six square-wave oscillators. They are NOT harmonically related and
    /// that is the entire point — six harmonics would sum to a buzzy saw,
    /// while six inharmonic squares sum to metal. Changing one of these
    /// numbers is changing which machine this is.
    pub const RATIOS: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

    /// The tune knob, as a multiplier on the whole bank.
    ///
    /// A multiplier rather than six knobs, and rather than a frequency:
    /// the six ratios ARE the instrument, and anything that can move them
    /// against each other is a knob that can turn an 808 into something
    /// else. 1.0 is the machine exactly, and it is the default.
    pub const TUNE_MIN: f32 = 0.5;
    pub const TUNE_MAX: f32 = 2.0;

    /// The note that opens the hat: GM's A#1, open hi-hat.
    ///
    /// Below it — F#1 closed, G#1 pedal — the short envelope plays. The
    /// 808's own panel had two buttons and one voice; the note is how a
    /// sequence says which button.
    pub const OPEN_NOTE: u8 = 46;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: TUNE,
            name: "tune",
            min: TUNE_MIN,
            max: TUNE_MAX,
            default: 1.0,
        },
        ParamDef {
            id: CLOSED_DECAY,
            name: "closed",
            min: 10.0,
            max: 400.0,
            default: 55.0,
        },
        ParamDef {
            id: OPEN_DECAY,
            name: "open",
            min: 50.0,
            max: 3_000.0,
            default: 500.0,
        },
        ParamDef {
            id: BP_HZ,
            // The 808's hat band sits around 10 kHz. Below about 6 the
            // squares stop being metal and start being a buzz.
            name: "band",
            min: 2_000.0,
            max: 16_000.0,
            default: 10_000.0,
        },
        ParamDef {
            id: BP_Q,
            name: "width",
            min: 0.5,
            max: 12.0,
            default: 2.0,
        },
        ParamDef {
            id: HP_HZ,
            name: "hp",
            min: 1_000.0,
            max: 12_000.0,
            default: 7_000.0,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: crate::dsp::shaper::DRIVE_MIN,
            max: crate::dsp::shaper::DRIVE_MAX,
            default: 1.0,
        },
        ParamDef {
            id: GAIN,
            name: "gain",
            min: 0.0,
            max: 2.0,
            default: 0.9,
        },
    ];
}

/// The hand clap (`Node::Handclap`).
///
/// `handclap`, not `clap`, and deliberately: CLAP in this codebase is the
/// PLUGIN FORMAT the host loads through clack. A module named `clap` next
/// to a plugin host that scans for CLAP plugins is a name that costs
/// somebody an afternoon.
///
/// A clap is not one sound. It is several hands not quite together,
/// followed by the room they are in — and that is exactly how it is
/// built: a short burst of band-passed noise retriggered a few times a
/// few milliseconds apart, over one longer decaying tail of the same
/// noise. The unevenness of the burst spacing is the whole realism.
pub mod handclap {
    use super::ParamDef;

    pub const BURSTS: u32 = 0;
    pub const SPREAD: u32 = 1;
    pub const BURST_DECAY: u32 = 2;
    pub const BODY: u32 = 3;
    pub const BODY_DECAY: u32 = 4;
    pub const TONE: u32 = 5;
    pub const WIDTH: u32 = 6;
    pub const HP_HZ: u32 = 7;
    pub const DRIVE: u32 = 8;
    pub const GAIN: u32 = 9;

    /// The most hands the clap will stack.
    pub const BURSTS_MAX: f32 = 4.0;

    /// WHERE each burst falls, as a multiple of the spread.
    ///
    /// Not `0, 1, 2, 3`. Evenly spaced bursts sum to a flam — a machine
    /// gun, audibly periodic — because the ear hears equal intervals as a
    /// rhythm however short they are. Real hands are progressively closer
    /// together as they converge, so the gaps SHRINK: these offsets are
    /// the classic uneven pattern, and they are why this reads as one
    /// clap rather than four taps.
    pub const OFFSETS: [f32; 4] = [0.0, 1.0, 1.9, 2.7];

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: BURSTS,
            name: "hands",
            min: 1.0,
            max: BURSTS_MAX,
            default: 3.0,
        },
        ParamDef {
            id: SPREAD,
            name: "spread",
            min: 2.0,
            max: 40.0,
            default: 10.0,
        },
        ParamDef {
            id: BURST_DECAY,
            name: "snap",
            min: 1.0,
            max: 60.0,
            default: 6.0,
        },
        ParamDef {
            id: BODY,
            name: "body",
            min: 0.0,
            max: 1.0,
            default: 0.5,
        },
        ParamDef {
            id: BODY_DECAY,
            name: "tail",
            min: 50.0,
            max: 1_500.0,
            default: 280.0,
        },
        ParamDef {
            id: TONE,
            // The clap band: around 1 kHz, which is where a hand's slap
            // actually lives. Higher reads as a rimshot, lower as a thud.
            name: "tone",
            min: 300.0,
            max: 4_000.0,
            default: 1_000.0,
        },
        ParamDef {
            id: WIDTH,
            name: "width",
            min: 0.3,
            max: 8.0,
            default: 1.1,
        },
        ParamDef {
            id: HP_HZ,
            name: "hp",
            min: 100.0,
            max: 2_000.0,
            default: 500.0,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: crate::dsp::shaper::DRIVE_MIN,
            max: crate::dsp::shaper::DRIVE_MAX,
            default: 1.0,
        },
        ParamDef {
            id: GAIN,
            name: "gain",
            min: 0.0,
            max: 2.0,
            default: 0.9,
        },
    ];
}

/// Modulato: chorus, flanger and vibrato, which are one effect.
///
/// See `audio::modulato` for why they are one, and for what `mode`
/// actually does — it picks the base delay's WINDOW, because a single
/// knob spanning a flanger's tenth of a millisecond and a chorus's
/// thirty is a knob that can tune neither.
pub mod modulato {
    use super::ParamDef;

    pub const MODE: u32 = 0;
    pub const RATE: u32 = 1;
    pub const DEPTH: u32 = 2;
    pub const DELAY: u32 = 3;
    pub const FEEDBACK: u32 = 4;
    pub const SPREAD: u32 = 5;
    pub const MIX: u32 = 6;

    pub const MODE_CHORUS: f32 = 0.0;
    pub const MODE_FLANGER: f32 = 1.0;
    pub const MODE_VIBRATO: f32 = 2.0;
    pub const MODE_MAX: f32 = MODE_VIBRATO;

    /// The names the card's mode strip shows, indexed by the wire value.
    pub const MODE_NAMES: &[&str] = &["chorus", "flanger", "vibrato"];

    /// The deepest swing and the longest base delay, in milliseconds.
    /// The delay lines are sized for their sum.
    pub const DEPTH_MAX_MS: f32 = 10.0;
    pub const DELAY_MAX_MS: f32 = 40.0;

    /// The base delay's window for a mode, in milliseconds.
    ///
    /// UNDER TEN is where a delayed copy comb-filters the dry one, which
    /// is a flanger; past ten it is heard as a second voice, which is a
    /// chorus. Vibrato sits in between because it has no dry signal to
    /// beat against and only wants to stay one voice.
    pub fn window(mode: f32) -> (f32, f32) {
        match mode.round() {
            m if m == MODE_FLANGER => (0.2, 10.0),
            m if m == MODE_VIBRATO => (1.0, 15.0),
            _ => (8.0, DELAY_MAX_MS),
        }
    }

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: MODE,
            name: "mode",
            min: 0.0,
            max: MODE_MAX,
            default: MODE_CHORUS,
        },
        ParamDef {
            id: RATE,
            name: "rate",
            // Down to a cycle every fifty seconds, which is a slow drift
            // rather than an effect you can hear moving; up to where it
            // stops being modulation and starts being a sideband.
            min: 0.02,
            max: 20.0,
            default: 0.8,
        },
        ParamDef {
            id: DEPTH,
            name: "depth",
            min: 0.0,
            max: DEPTH_MAX_MS,
            default: 2.5,
        },
        ParamDef {
            id: DELAY,
            // A POSITION IN THE MODE'S WINDOW, `0..=1`, not a time — see
            // `window` above. The card prints the millisecond figure it
            // resolves to, so the reading is still honest.
            name: "delay",
            min: 0.0,
            max: 1.0,
            default: 0.35,
        },
        ParamDef {
            id: FEEDBACK,
            // BIPOLAR: negative feedback inverts the comb, so the
            // notches land where the peaks were. That is the difference
            // between the two flanger sounds everyone knows, and it
            // costs one sign.
            name: "feedback",
            min: -0.9,
            max: 0.9,
            default: 0.0,
        },
        ParamDef {
            id: SPREAD,
            // The right oscillator's phase offset, in TURNS. Half a turn
            // is opposition, which is as wide as it goes.
            name: "spread",
            min: 0.0,
            max: 0.5,
            default: 0.25,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            default: 0.5,
        },
    ];
}

/// The character limiter (`Node::Limiter`).
///
/// NOT a transparent one, and that is the whole design. A limiter that
/// only prevents peaks is a safety device, and a safety device is
/// something you put on and forget; this is something you put on because
/// you want what it does. It makes things louder, it stops them getting
/// loud, and on the way it adds a little warmth, a little less high-end
/// fuzz, and a slew-driven lift that puts back the edge the gain
/// reduction takes off.
///
/// # Why there is no output trim
///
/// Because the CEILING is the output. A trim after the clipper would be
/// the one control able to push the signal back over the ceiling, which
/// would turn the device's one absolute promise — nothing leaves here
/// above the ceiling — into a promise with an asterisk. Drive with
/// [`PUSH`], land with [`CEILING`], and the guarantee holds at every
/// setting.
pub mod limiter {
    use super::ParamDef;

    pub const PUSH: u32 = 0;
    pub const CEILING: u32 = 1;
    pub const STYLE: u32 = 2;
    pub const RELEASE: u32 = 3;
    pub const WARMTH: u32 = 4;
    pub const FUZZ: u32 = 5;
    pub const BRIGHTEN: u32 = 6;

    /// How far into the ceiling the input can be driven, in dB.
    ///
    /// 24 dB is a lot, deliberately: this is the loudness control, and
    /// the interesting settings on a character limiter are the ones where
    /// it is working. A 6 dB range would make it a safety device again.
    pub const PUSH_MAX_DB: f32 = 24.0;

    /// The ceiling's window, in dBFS. Never above 0 — the point of a
    /// ceiling is that it is one.
    pub const CEILING_MIN_DB: f32 = -12.0;
    pub const CEILING_MAX_DB: f32 = 0.0;

    /// The styles, in wire order. Indices, as every other switch here is.
    pub const STYLE_WARM: u32 = 0;
    pub const STYLE_PUNCH: u32 = 1;
    pub const STYLE_SMASH: u32 = 2;
    pub const STYLE_MAX: u32 = STYLE_SMASH;

    /// What each style does to the release, as a MULTIPLIER on the knob.
    ///
    /// The style changes the recovery and nothing else, because the
    /// attack cannot move: it rides the lookahead, and the lookahead is
    /// the device's reported latency. A style that changed the latency
    /// would slide the track in time when you picked it off a menu.
    ///
    /// - `warm` recovers slowly, so the gain sits still and the device
    ///   reads as level rather than as movement.
    /// - `punch` is the knob as written.
    /// - `smash` recovers fast enough to pump audibly, which on the right
    ///   material is the effect people reach for a limiter to get.
    pub const STYLE_RELEASE_SCALE: &[f32] = &[2.5, 1.0, 0.35];
    pub const STYLE_NAMES: &[&str] = &["warm", "punch", "smash"];

    /// The style a wire index names. Out of range gives `punch`, because
    /// a stale index must still limit.
    pub fn style_release_scale(index: u32) -> f32 {
        STYLE_RELEASE_SCALE
            .get(index as usize)
            .copied()
            .unwrap_or(1.0)
    }

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: PUSH,
            name: "push",
            min: 0.0,
            max: PUSH_MAX_DB,
            // Already working when you load it. The saturator's rule, for
            // the saturator's reason: you add a character limiter because
            // you want the character, and one that does nothing until you
            // turn a knob is a different kind of surprise.
            default: 3.0,
        },
        ParamDef {
            id: CEILING,
            name: "ceiling",
            min: CEILING_MIN_DB,
            max: CEILING_MAX_DB,
            // Not 0.0: a true-peak reconstruction of a signal sitting
            // exactly at full scale can overshoot a converter, and a
            // third of a dB is the cheapest insurance in audio.
            default: -0.3,
        },
        ParamDef {
            id: STYLE,
            name: "style",
            min: 0.0,
            max: STYLE_MAX as f32,
            default: STYLE_PUNCH as f32,
        },
        ParamDef {
            id: RELEASE,
            name: "release",
            min: 20.0,
            max: 1_000.0,
            default: 180.0,
        },
        ParamDef {
            id: WARMTH,
            name: "warmth",
            min: 0.0,
            max: 1.0,
            // SUBTLE, and the brief's word. This is the asymmetry that
            // makes even harmonics; past about half it stops being warmth
            // and starts being a fuzz box, so the default sits below.
            default: 0.35,
        },
        ParamDef {
            id: FUZZ,
            name: "fuzz",
            min: 0.0,
            max: 1.0,
            // Subtler again than the warmth, and deliberately: this one
            // lives in the top octaves where the ear is least forgiving.
            default: 0.15,
        },
        ParamDef {
            id: BRIGHTEN,
            name: "brighten",
            min: 0.0,
            max: 1.0,
            default: 0.3,
        },
    ];
}

pub mod sat {
    use super::ParamDef;

    pub const MODE: u32 = 0;
    pub const DRIVE: u32 = 1;
    pub const BIAS: u32 = 2;
    pub const MIX: u32 = 3;
    pub const OUT: u32 = 4;

    /// Mode indices on the wire — the order of `dsp::shaper::Mode`'s five
    /// shapes, and of the widget's mode strip. Indices, as
    /// [`filter::MODE`](super::filter::MODE) is.
    pub const MODE_HARD: u32 = 0;
    pub const MODE_SOFT: u32 = 1;
    pub const MODE_CUBIC: u32 = 2;
    pub const MODE_FOLD: u32 = 3;
    pub const MODE_CRUSH: u32 = 4;
    pub const MODE_MAX: u32 = MODE_CRUSH;

    /// The output trim's window, in dB, and the same figures as linear
    /// gain. Both forms are written down because the TABLE is in linear
    /// gain and the KNOB is in dB, and a widget deriving one from the
    /// other by hand is how the two ends of one range drift apart.
    pub const OUT_MIN_DB: f32 = -24.0;
    pub const OUT_MAX_DB: f32 = 12.0;
    /// `10^(-24/20)` and `10^(12/20)`, to f32 precision.
    pub const OUT_MIN: f32 = 0.063_095_73;
    pub const OUT_MAX: f32 = 3.981_072;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: MODE,
            name: "mode",
            min: 0.0,
            max: MODE_MAX as f32,
            default: MODE_SOFT as f32,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: crate::dsp::shaper::DRIVE_MIN,
            max: crate::dsp::shaper::DRIVE_MAX,
            // Unity gain into the curve — which is NOT the identity for
            // four of the five shapes (`SoftClip` at drive 1 is still
            // `tanh`, about 3% down at a normal level). Deliberately: the
            // reverb parks its default out of the way because a reverb
            // you did not ask for drowns a mix, and a gentle tanh does
            // not. You add a saturator because you want the character;
            // loading one that does nothing at all until you turn a knob
            // is a different kind of surprise.
            //
            // The identity case still EXISTS and is still tested —
            // `MODE_HARD` at this drive is a wire inside the rails, which
            // is what proves the oversampler round trip is unity gain and
            // phase-clean.
            default: crate::dsp::shaper::DRIVE_MIN,
        },
        ParamDef {
            id: BIAS,
            name: "bias",
            // Symmetric, because the kernel clamps symmetrically: bias
            // shifts the curve either way off centre, and which way is a
            // taste, not a magnitude.
            min: -crate::dsp::shaper::BIAS_MAX,
            max: crate::dsp::shaper::BIAS_MAX,
            default: 0.0,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            // Fully wet. A parallel blend is a choice you reach for —
            // it is how you keep a transient while crushing everything
            // under it — not the state you load into; a device that
            // arrives half-bypassed reads as broken before it reads as
            // subtle.
            default: 1.0,
        },
        ParamDef {
            id: OUT,
            name: "out",
            // Linear gain over exactly [`OUT_MIN_DB`]..[`OUT_MAX_DB`].
            // Saturation is a level change as much as a timbre change,
            // and the trim is how you A/B the two without reaching for
            // the fader behind it.
            //
            // The floor is -24 dB and NOT silence, deliberately: this row
            // is a trim, the track already has a fader and a mute, and a
            // range whose bottom is an infinite drop spends most of a
            // knob's travel — and most of an automation lane's useful
            // resolution — on the last inaudible decibel.
            min: OUT_MIN,
            max: OUT_MAX,
            default: 1.0,
        },
    ];

    /// Mode index -> the kernel's shape, clamped. The ONE place the wire
    /// index becomes a curve — the widget's mode strip is the same list
    /// in the same order, so the drawing and the audio pick the same
    /// shape from the same number.
    pub fn mode(index: u32) -> crate::dsp::shaper::Mode {
        use crate::dsp::shaper::Mode;
        match index.min(MODE_MAX) {
            MODE_HARD => Mode::HardClip,
            MODE_CUBIC => Mode::Cubic,
            MODE_FOLD => Mode::Fold,
            MODE_CRUSH => Mode::Crush,
            _ => Mode::SoftClip,
        }
    }
}

/// `Node::Echo` — the analogue delay.
///
/// # Why the node is `Echo` and the device is "delay"
///
/// [`NodeSpec::Delay`](crate::audio::graph::NodeSpec::Delay) was taken
/// years-of-commits ago by plugin delay compensation: a pure wire that
/// arrives late, inserted by COMPILE and never by a user. Two different
/// things called Delay in one graph is a bug waiting for a careless
/// match arm, so the musical one is `Echo` in the engine and "delay" on
/// its face — the user-facing word is the widget's business.
///
/// # Time is either a division or a number, never both
///
/// [`SYNC`] picks which. At index 0 the echo is FREE and [`TIME`] is
/// read as milliseconds; at any other index the echo is locked to the
/// transport and [`TIME`] is ignored — the division is converted against
/// the tempo the segment is actually playing at, so a tempo ramp drags
/// the repeats with it.
pub mod echo {
    use super::ParamDef;

    pub const SYNC: u32 = 0;
    pub const TIME: u32 = 1;
    pub const FEEDBACK: u32 = 2;
    pub const TONE: u32 = 3;
    pub const DRIVE: u32 = 4;
    pub const WOW: u32 = 5;
    pub const SPREAD: u32 = 6;
    pub const MIX: u32 = 7;
    pub const SEND: u32 = 8;

    /// The longest echo the buffer is built for, in milliseconds.
    ///
    /// A hard ceiling and not a suggestion: the delay memory is
    /// allocated once at compile, in the green zone, and nothing in the
    /// red zone may ask for more than it. Four seconds covers a free
    /// time at its maximum and a whole bar at any tempo down to 60 BPM;
    /// slower than that, a synced whole-note clamps and the repeats come
    /// back early rather than the callback reaching for memory.
    pub const MAX_MS: f32 = 4_000.0;

    /// Sync divisions, in BEATS. Index 0 is free-running and has no
    /// division, which is why this list is indexed from `SYNC - 1`.
    ///
    /// Beats, not note names, because that is the unit the transport
    /// speaks: `ctx.beats_per_sample` converts one of these into samples
    /// with a single divide and no table of tempo maths.
    pub const DIVISION_BEATS: &[f32] = &[
        4.0,       // 1/1
        2.0,       // 1/2
        1.0,       // 1/4
        0.5,       // 1/8
        0.25,      // 1/16
        2.0 / 3.0, // 1/4T
        1.0 / 3.0, // 1/8T
        1.5,       // 1/4.
        0.75,      // 1/8.
    ];

    /// What the sync switch says. Index 0 is free; the rest line up with
    /// [`DIVISION_BEATS`], so the strip and the arithmetic cannot name
    /// different divisions.
    pub const SYNC_NAMES: &[&str] = &[
        "free", "1/1", "1/2", "1/4", "1/8", "1/16", "1/4t", "1/8t", "1/4.", "1/8.",
    ];

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: SYNC,
            name: "sync",
            min: 0.0,
            max: (SYNC_NAMES.len() - 1) as f32,
            // An eighth: the division a delay is reached for most, and
            // audibly a delay rather than a doubler the moment it loads.
            default: 4.0,
        },
        ParamDef {
            id: TIME,
            name: "time",
            // Two milliseconds is where a delay stops being an echo and
            // starts being a comb filter — worth having, and the floor
            // the kernel's own fractional read enforces anyway.
            min: 2.0,
            max: MAX_MS,
            default: 350.0,
        },
        ParamDef {
            id: FEEDBACK,
            name: "feedback",
            min: 0.0,
            // The kernel's ceiling, as a percentage. At 99% a repeat is
            // "almost forever" and still provably decays; 100% would
            // integrate the buffer into the rails.
            max: 99.0,
            default: 35.0,
        },
        ParamDef {
            id: TONE,
            name: "tone",
            // The damping corner INSIDE the loop, so each trip loses
            // everything above it again. Down at 200 Hz the third repeat
            // is a thud; wide open it is a digital delay.
            min: 200.0,
            max: 20_000.0,
            default: 4_500.0,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: 0.0,
            max: 100.0,
            // Clean on arrival: drive 0 is bit-exact bypass in the
            // kernel, so a delay you have not dirtied is arithmetically
            // the plain one.
            default: 0.0,
        },
        ParamDef {
            id: WOW,
            name: "wow",
            min: 0.0,
            max: 100.0,
            // A trace of movement by default. Dead-still repeats are the
            // one thing that never sounds like tape, and at this depth it
            // reads as warmth rather than as an effect.
            default: 12.0,
        },
        ParamDef {
            id: SPREAD,
            name: "spread",
            min: 0.0,
            max: 100.0,
            // How much later the right channel repeats than the left, as
            // a percentage of the echo time. A stereo picture from one
            // control, and 0 is a mono-compatible delay.
            default: 0.0,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 100.0,
            // Audible, never a wash — the reverb's rule. A delay that
            // arrives louder than the track it is on is a delay nobody
            // asked for.
            default: 30.0,
        },
        ParamDef {
            id: SEND,
            name: "send",
            min: 0.0,
            max: 100.0,
            // ZERO IS A TOPOLOGY, not just a quiet send. At 0 the delay
            // is an ordinary insert: the whole track runs through it and
            // `mix` blends. Above 0 it leaves the chain and becomes an
            // aux — the track is tapped at this level, post-fader, and
            // the delay's output returns to the master beside the dry.
            //
            // 0 is therefore also the only default that leaves every
            // project written before this parameter existed sounding
            // exactly as it did.
            default: 0.0,
        },
    ];

    /// The echo time in SAMPLES for this segment.
    ///
    /// One function, called by the node and by nothing else, so the
    /// free and synced cases cannot disagree about what "time" means.
    /// `beats_per_sample` comes from the segment being rendered, so a
    /// tempo change moves the repeats with it.
    ///
    /// Clamped to what the buffer was built for at both ends. A synced
    /// whole-note at a very slow tempo asks for more than [`MAX_MS`],
    /// and the honest answer in the red zone is an echo that comes back
    /// early — never a read past the end of the memory.
    pub fn time_samples(sync: u32, time_ms: f32, sample_rate: f32, beats_per_sample: f64) -> f32 {
        let max = MAX_MS * 1e-3 * sample_rate;
        let samples = if sync == 0 {
            time_ms * 1e-3 * sample_rate
        } else {
            let beats = DIVISION_BEATS
                .get((sync - 1) as usize)
                .copied()
                .unwrap_or(1.0);
            // beats / (beats per sample) = samples. A stopped transport
            // reports zero beats per sample; fall back to the free time
            // rather than dividing by zero.
            if beats_per_sample > 0.0 {
                (f64::from(beats) / beats_per_sample) as f32
            } else {
                time_ms * 1e-3 * sample_rate
            }
        };
        if samples.is_finite() {
            samples.clamp(2.0, max)
        } else {
            2.0
        }
    }
}

/// `Node::Poly` — the workhorse synth of `notes/20260825-synth-brief.md`.
///
/// # Naming convention
///
/// A name here is `<group> <label>`: the group is the section of the
/// voice path the parameter belongs to, the label is what the knob says.
/// Names must be unique across a device, but a knob standing in a well
/// already titled "osc a" must not repeat itself — so the table carries
/// the full, automation-facing name and the widget shows everything after
/// the first space. One string, both jobs, and `poly::label` is the only
/// place that knows the split.
///
/// # The table speaks the unit the knob shows
///
/// Percent parameters run `0..100`, times are ms, frequencies Hz,
/// resonance is a Q on the Filter node's scale, gain is linear. A widget
/// `Param` built from a row uses the row's range AS its mapping, so
/// `Param::value()` is always the engine-facing number and there is no
/// per-parameter bridging step to get wrong. Where a kernel wants a
/// different form — `shaper_drive` wants `0..=1`, not percent — the node
/// converts, once, at the point of use.
///
/// # Discrete parameters are INDICES
///
/// Wave, octave, mode, slope, drive position, voice mode and unison ride
/// the wire as indices into the widget's choice list, exactly as
/// [`filter::MODE`] and [`filter::SLOPE`] do. Where the index is not the
/// value, the offset is written down beside it — there is no second
/// convention to remember, only the one arithmetic step.
pub mod poly {
    use super::ParamDef;

    // osc a
    pub const A_WAVE: u32 = 0;
    pub const A_OCT: u32 = 1;
    pub const A_SEMI: u32 = 2;
    pub const A_FINE: u32 = 3;
    pub const A_LEVEL: u32 = 4;
    pub const A_PENV: u32 = 5;
    // osc b
    pub const B_WAVE: u32 = 6;
    pub const B_OCT: u32 = 7;
    pub const B_SEMI: u32 = 8;
    pub const B_FINE: u32 = 9;
    pub const B_LEVEL: u32 = 10;
    pub const B_PENV: u32 = 11;
    // noise
    pub const N_COLOR: u32 = 12;
    pub const N_LEVEL: u32 = 13;
    pub const N_DECAY: u32 = 14;
    // filter
    pub const F_MODE: u32 = 15;
    pub const F_SLOPE: u32 = 16;
    pub const F_CUTOFF: u32 = 17;
    pub const F_RES: u32 = 18;
    pub const F_ENV: u32 = 19;
    pub const F_KEY: u32 = 20;
    pub const F_DRIVE: u32 = 21;
    pub const F_POS: u32 = 22;
    // amp
    pub const AMP_A: u32 = 23;
    pub const AMP_D: u32 = 24;
    pub const AMP_S: u32 = 25;
    pub const AMP_R: u32 = 26;
    pub const GAIN: u32 = 27;
    pub const VEL: u32 = 28;
    // voices
    pub const V_MODE: u32 = 29;
    pub const V_GLIDE: u32 = 30;
    pub const V_UNISON: u32 = 31;
    pub const V_DETUNE: u32 = 32;
    pub const V_SPREAD: u32 = 33;
    // filter envelope — its OWN times, not the amp's
    pub const FENV_A: u32 = 34;
    pub const FENV_D: u32 = 35;
    pub const FENV_S: u32 = 36;
    pub const FENV_R: u32 = 37;
    // pitch envelope — a one-shot decay, the kick's clock
    pub const PENV_D: u32 = 38;
    // the audio-rate mod matrix: four wires of (source, dest, depth)
    pub const W1_SRC: u32 = 39;
    pub const W1_DST: u32 = 40;
    pub const W1_AMT: u32 = 41;
    pub const W2_SRC: u32 = 42;
    pub const W2_DST: u32 = 43;
    pub const W2_AMT: u32 = 44;
    pub const W3_SRC: u32 = 45;
    pub const W3_DST: u32 = 46;
    pub const W3_AMT: u32 = 47;

    /// The four `MipOsc` shapes, then the four sparse spectral tables.
    /// Index order IS the wire order of [`A_WAVE`] and [`B_WAVE`].
    pub const WAVES: &[&str] = &[
        "sine", "tri", "saw", "square", "bell", "glass", "metal", "air",
    ];
    pub const NOISE_COLORS: &[&str] = &["white", "pink"];
    /// Whether the drive stage sits before or after the filter.
    pub const DRIVE_POS: &[&str] = &["pre", "post"];
    pub const VOICE_MODES: &[&str] = &["poly", "mono", "legato"];
    /// Octave transpose choices. An octave is a discrete musical fact, so
    /// it rides as an index rather than as a continuous number a knob
    /// could land between: `octave = index - OCT_CENTER`.
    pub const OCTAVES: &[&str] = &["-4", "-3", "-2", "-1", "0", "+1", "+2", "+3", "+4"];
    /// Index of "no transpose" in [`OCTAVES`].
    pub const OCT_CENTER: u32 = 4;
    /// Unison voices per note: `voices = index + 1`.
    pub const UNISON: &[&str] = &["1", "2", "3", "4", "5", "6", "7", "8"];

    /// Wires the matrix holds. Fixed, because the wire rows are param
    /// rows: automation, projects and letters all address them the same
    /// way they address a knob, which is the entire reason the matrix is
    /// in the TABLE rather than in a side structure only the UI knows.
    ///
    /// THREE, down from four (2026-08-26): the matrix presents as a 3×3
    /// grid — three wires of (src, dst, depth) — and a fourth wire the
    /// grid could not show would be a knob without a face. A project
    /// saved with four loads with its fourth wire dropped.
    pub const WIRES: usize = 3;
    /// `(src, dst, depth)` row ids of each wire, in wire order.
    pub const WIRE_IDS: [(u32, u32, u32); WIRES] = [
        (W1_SRC, W1_DST, W1_AMT),
        (W2_SRC, W2_DST, W2_AMT),
        (W3_SRC, W3_DST, W3_AMT),
    ];

    /// What a wire can listen to. Index order IS the wire value of the
    /// `W*_SRC` rows. "off" first, so a defaulted wire is silent.
    pub const MOD_SRC: &[&str] = &[
        "off", "osc a", "osc b", "noise", "amp", "fenv", "penv", "vel",
    ];
    pub const SRC_OFF: u32 = 0;
    pub const SRC_OSC_A: u32 = 1;
    pub const SRC_OSC_B: u32 = 2;
    pub const SRC_NOISE: u32 = 3;
    pub const SRC_AMP: u32 = 4;
    pub const SRC_FENV: u32 = 5;
    pub const SRC_PENV: u32 = 6;
    pub const SRC_VEL: u32 = 7;

    /// What a wire can move. Phase and level are the audio-rate pair —
    /// PM IS FM here (phase is the integral of frequency, and the phase
    /// accumulator makes it an add); cutoff, res and pan ride the
    /// control chunk.
    pub const MOD_DST: &[&str] = &[
        "off",
        "a phase",
        "a level",
        "b phase",
        "b level",
        "noise lvl",
        "cutoff",
        "res",
        "pan",
    ];
    pub const DST_OFF: u32 = 0;
    pub const DST_A_PHASE: u32 = 1;
    pub const DST_A_LEVEL: u32 = 2;
    pub const DST_B_PHASE: u32 = 3;
    pub const DST_B_LEVEL: u32 = 4;
    pub const DST_N_LEVEL: u32 = 5;
    pub const DST_CUTOFF: u32 = 6;
    pub const DST_RES: u32 = 7;
    pub const DST_PAN: u32 = 8;

    /// Total voices the allocator owns, before unison multiplies them.
    /// Two lane groups of eight — one zmm register each.
    pub const VOICES: usize = 16;

    /// The part of a name a knob shows: everything after the group word.
    ///
    /// A subslice of a `&'static str` is still `&'static`, so this is the
    /// whole implementation — no second table of short names to drift
    /// against the first.
    pub fn label(name: &'static str) -> &'static str {
        name.split_once(' ').map_or(name, |(_group, rest)| rest)
    }

    /// Defaults are ONE audible oscillator and nothing else: osc A at
    /// full level, osc B and the noise source silent, the filter parked
    /// open, unity gain out. Loading the synth makes a sound the moment a
    /// note arrives, and it makes exactly one — the reverb table's
    /// never-destroy-what-it-lands-on rule, aimed at a patch the user
    /// starts by ADDING to rather than by subtracting from.
    pub const TABLE: &[ParamDef] = &[
        // ---------------------------------------------------- osc a ---
        ParamDef {
            id: A_WAVE,
            name: "a wave",
            min: 0.0,
            max: 7.0,
            default: 0.0,
        },
        ParamDef {
            id: A_OCT,
            name: "a oct",
            min: 0.0,
            max: 8.0,
            default: 4.0,
        },
        ParamDef {
            id: A_SEMI,
            name: "a semi",
            min: -12.0,
            max: 12.0,
            default: 0.0,
        },
        ParamDef {
            id: A_FINE,
            name: "a fine",
            min: -100.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: A_LEVEL,
            name: "a level",
            min: 0.0,
            max: 100.0,
            default: 100.0,
        },
        // +-48 semitones: the brief's kick recipe sweeps the whole range.
        ParamDef {
            id: A_PENV,
            name: "a p.env",
            min: -48.0,
            max: 48.0,
            default: 0.0,
        },
        // ---------------------------------------------------- osc b ---
        ParamDef {
            id: B_WAVE,
            name: "b wave",
            min: 0.0,
            max: 7.0,
            default: 0.0,
        },
        ParamDef {
            id: B_OCT,
            name: "b oct",
            min: 0.0,
            max: 8.0,
            default: 4.0,
        },
        ParamDef {
            id: B_SEMI,
            name: "b semi",
            min: -12.0,
            max: 12.0,
            default: 0.0,
        },
        ParamDef {
            id: B_FINE,
            name: "b fine",
            min: -100.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: B_LEVEL,
            name: "b level",
            min: 0.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: B_PENV,
            name: "b p.env",
            min: -48.0,
            max: 48.0,
            default: 0.0,
        },
        // ---------------------------------------------------- noise ---
        ParamDef {
            id: N_COLOR,
            name: "noise color",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: N_LEVEL,
            name: "noise level",
            min: 0.0,
            max: 100.0,
            default: 0.0,
        },
        // The noise source's OWN decay, so a kick's click and a hat's
        // body are two settings rather than two devices.
        ParamDef {
            id: N_DECAY,
            name: "noise decay",
            min: 1.0,
            max: 4_000.0,
            default: 120.0,
        },
        // --------------------------------------------------- filter ---
        ParamDef {
            id: F_MODE,
            name: "filter mode",
            min: 0.0,
            max: 3.0,
            default: 0.0,
        },
        ParamDef {
            id: F_SLOPE,
            name: "filter slope",
            min: 0.0,
            max: 5.0,
            default: 3.0,
        },
        // Down to 20 Hz: a sub bass has to keep its fundamental.
        ParamDef {
            id: F_CUTOFF,
            name: "filter cutoff",
            min: 20.0,
            max: 20_000.0,
            default: 20_000.0,
        },
        // Q, on the same scale the Filter node uses — one resonance
        // meaning across the app, so `filter::effective_q` applies here
        // unchanged.
        ParamDef {
            id: F_RES,
            name: "filter res",
            min: 0.3,
            max: 24.0,
            default: super::filter::FLAT_Q,
        },
        // Bipolar, in percent of the cutoff sweep.
        ParamDef {
            id: F_ENV,
            name: "filter env",
            min: -100.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: F_KEY,
            name: "filter key",
            min: 0.0,
            max: 200.0,
            default: 0.0,
        },
        ParamDef {
            id: F_DRIVE,
            name: "filter drive",
            min: 0.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: F_POS,
            name: "filter pos",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        // ------------------------------------------------------ amp ---
        // Attacks down to 1 ms and below, per the brief: a click is an
        // envelope, not a special case.
        ParamDef {
            id: AMP_A,
            name: "amp attack",
            min: 0.05,
            max: 5_000.0,
            default: 1.0,
        },
        ParamDef {
            id: AMP_D,
            name: "amp decay",
            min: 1.0,
            max: 10_000.0,
            default: 200.0,
        },
        ParamDef {
            id: AMP_S,
            name: "amp sustain",
            min: 0.0,
            max: 100.0,
            default: 70.0,
        },
        ParamDef {
            id: AMP_R,
            name: "amp release",
            min: 1.0,
            max: 30_000.0,
            default: 640.0,
        },
        // Linear gain, matching `seq::GAIN` — the table speaks amplitude
        // and the widget decides how to show it.
        ParamDef {
            id: GAIN,
            name: "amp gain",
            min: 0.0,
            max: 2.0,
            default: 1.0,
        },
        ParamDef {
            id: VEL,
            name: "amp vel",
            min: 0.0,
            max: 100.0,
            default: 100.0,
        },
        // --------------------------------------------------- voices ---
        ParamDef {
            id: V_MODE,
            name: "voice mode",
            min: 0.0,
            max: 2.0,
            default: 0.0,
        },
        ParamDef {
            id: V_GLIDE,
            name: "voice glide",
            min: 1.0,
            max: 2_000.0,
            default: 1.0,
        },
        ParamDef {
            id: V_UNISON,
            name: "voice unison",
            min: 0.0,
            max: 7.0,
            default: 0.0,
        },
        ParamDef {
            id: V_DETUNE,
            name: "voice detune",
            min: 0.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: V_SPREAD,
            name: "voice spread",
            min: 0.0,
            max: 100.0,
            default: 0.0,
        },
        // ------------------------------------------- filter envelope ---
        // Its own ADSR: two voices of one patch may share a shape, but
        // the filter's contour and the amp's are different musical ideas
        // and borrowing one's times for the other made them one knob.
        ParamDef {
            id: FENV_A,
            name: "fenv attack",
            min: 0.05,
            max: 5_000.0,
            default: 1.0,
        },
        ParamDef {
            id: FENV_D,
            name: "fenv decay",
            min: 1.0,
            max: 10_000.0,
            default: 200.0,
        },
        ParamDef {
            id: FENV_S,
            name: "fenv sustain",
            min: 0.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: FENV_R,
            name: "fenv release",
            min: 1.0,
            max: 30_000.0,
            default: 300.0,
        },
        // -------------------------------------------- pitch envelope ---
        // One-shot decay. ~50 ms is the brief's kick recipe, which is
        // what this envelope exists for.
        ParamDef {
            id: PENV_D,
            name: "penv decay",
            min: 1.0,
            max: 2_000.0,
            default: 50.0,
        },
        // ------------------------------------------------ mod matrix ---
        // Four wires, each three rows. Depth is bipolar percent; at 100%
        // a phase wire swings a full turn and a level wire doubles or
        // silences. Src/dst are indices into MOD_SRC / MOD_DST, "off"
        // first so a defaulted wire is silent.
        ParamDef {
            id: W1_SRC,
            name: "wire1 src",
            min: 0.0,
            max: 7.0,
            default: 0.0,
        },
        ParamDef {
            id: W1_DST,
            name: "wire1 dest",
            min: 0.0,
            max: 8.0,
            default: 0.0,
        },
        ParamDef {
            id: W1_AMT,
            name: "wire1 depth",
            min: -100.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: W2_SRC,
            name: "wire2 src",
            min: 0.0,
            max: 7.0,
            default: 0.0,
        },
        ParamDef {
            id: W2_DST,
            name: "wire2 dest",
            min: 0.0,
            max: 8.0,
            default: 0.0,
        },
        ParamDef {
            id: W2_AMT,
            name: "wire2 depth",
            min: -100.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: W3_SRC,
            name: "wire3 src",
            min: 0.0,
            max: 7.0,
            default: 0.0,
        },
        ParamDef {
            id: W3_DST,
            name: "wire3 dest",
            min: 0.0,
            max: 8.0,
            default: 0.0,
        },
        ParamDef {
            id: W3_AMT,
            name: "wire3 depth",
            min: -100.0,
            max: 100.0,
            default: 0.0,
        },
    ];

    /// Octave transpose of a wire index.
    pub fn octave(index: u32) -> i32 {
        index as i32 - OCT_CENTER as i32
    }

    /// Unison voice count of a wire index.
    pub fn unison(index: u32) -> u32 {
        index + 1
    }
}

/// `Node::Utility` — gain, placement and the stereo field.
///
/// The device with no tone of its own. Everything here is a thing you
/// reach for when a track is RIGHT but sits wrong: it is three dB too
/// loud, it is centred when it should lean, its stereo is too wide for
/// the mix or too narrow for the part, its bass is smeared across the
/// image, one side arrived with its phase flipped, or the file has an
/// offset in it.
///
/// # The order the stages run in is a decision, not an accident
///
/// ```text
///  in ── dc ── channel ── phase ── width ── bass mono ── pan ── gain ── out
/// ```
///
/// - **DC first.** An offset is damage to be cleaned before anything
///   reads the signal; left in, it rides into `mid` and the width stage
///   spreads it across both sides.
/// - **Channel, then phase.** Swapping the sides and inverting one are
///   both routing, and doing them the other way round would make "ø L"
///   mean whichever channel `swap` had just moved there.
/// - **Width, then bass mono.** Both are mid/side, and bass mono is a
///   correction ON the width: it is the low end you keep centred no
///   matter how wide the rest is told to be.
/// - **Pan and gain last.** They are level, and level is what the stages
///   above must not have to compensate for.
///
/// # The discretes are INDICES
///
/// [`PHASE`], [`CHANNEL`] and [`DC`] ride the wire as indices into their
/// name lists, exactly as [`filter::MODE`](super::filter::MODE) does.
pub mod utility {
    use super::ParamDef;

    pub const GAIN: u32 = 0;
    pub const PAN: u32 = 1;
    pub const WIDTH: u32 = 2;
    pub const MONO_HZ: u32 = 3;
    pub const PHASE: u32 = 4;
    pub const CHANNEL: u32 = 5;
    pub const DC: u32 = 6;

    /// The trim's window, in dB. Wider than the saturator's output trim
    /// on purpose: that one is a make-up for a curve, and this is the
    /// row you reach for when a stem arrives at the wrong level
    /// entirely. Still not silence at the bottom, for the reason
    /// [`sat::OUT`](super::sat::OUT) gives — the track has a fader and a
    /// mute already, and a range whose floor is an infinite drop spends
    /// most of its travel on the last inaudible decibel.
    pub const GAIN_MIN_DB: f32 = -36.0;
    pub const GAIN_MAX_DB: f32 = 36.0;

    /// Full width is 1.0 and the ceiling is twice that. Zero is mono —
    /// the side signal gone entirely — and it is a real setting rather
    /// than an edge case: "make this mono" is half of why the device
    /// exists.
    pub const WIDTH_MAX: f32 = 2.0;

    /// The bass-mono crossover's window. At [`MONO_MIN_HZ`] the stage is
    /// OFF: the corner has walked below the band, there is nothing under
    /// it to centre, and paying for a filter that does nothing is worse
    /// than saying so — see [`mono_off`].
    pub const MONO_MIN_HZ: f32 = 20.0;
    pub const MONO_MAX_HZ: f32 = 500.0;

    pub const PHASE_NONE: u32 = 0;
    pub const PHASE_L: u32 = 1;
    pub const PHASE_R: u32 = 2;
    pub const PHASE_BOTH: u32 = 3;
    /// What the cell prints, indexed by [`PHASE`]. `ø` is the console
    /// marking, and it is worth the non-ascii: "inv L" and "ø L" cost
    /// the same width and only one of them is what the desk says.
    pub const PHASE_NAMES: &[&str] = &["off", "ø L", "ø R", "ø L+R"];

    pub const CHANNEL_STEREO: u32 = 0;
    pub const CHANNEL_SWAP: u32 = 1;
    pub const CHANNEL_LEFT: u32 = 2;
    pub const CHANNEL_RIGHT: u32 = 3;
    /// `left` and `right` mean "this side, on both outputs" — a mono
    /// FOLD to one source, not a mute of the other. Soloing one side of
    /// a stereo file to check it is the ordinary use, and hearing it out
    /// of one speaker while you do is not.
    pub const CHANNEL_NAMES: &[&str] = &["stereo", "swap", "left", "right"];

    /// The two-position switches' faces. One list, because a switch that
    /// printed `on`/`off` in one place and `in`/`out` in another would be
    /// two grammars for one control.
    pub const SWITCH_NAMES: &[&str] = &["off", "on"];

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: GAIN,
            name: "gain",
            // dB, not linear gain: this row IS the number the user is
            // thinking in, and every readout, automation lane and
            // modulation sweep over it should move in decibels because
            // that is what "3 dB down" means.
            min: GAIN_MIN_DB,
            max: GAIN_MAX_DB,
            default: 0.0,
        },
        ParamDef {
            id: PAN,
            name: "pan",
            min: -1.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: WIDTH,
            name: "width",
            min: 0.0,
            max: WIDTH_MAX,
            // Untouched. A utility that opened anywhere but unity would
            // change a mix by being inserted, which is the one thing this
            // device must never do.
            default: 1.0,
        },
        ParamDef {
            id: MONO_HZ,
            name: "mono",
            min: MONO_MIN_HZ,
            max: MONO_MAX_HZ,
            // Off, by the same rule: inserted and untouched, this device
            // is a wire.
            default: MONO_MIN_HZ,
        },
        ParamDef {
            id: PHASE,
            name: "phase",
            min: 0.0,
            max: PHASE_BOTH as f32,
            default: PHASE_NONE as f32,
        },
        ParamDef {
            id: CHANNEL,
            name: "channel",
            min: 0.0,
            max: CHANNEL_RIGHT as f32,
            default: CHANNEL_STEREO as f32,
        },
        ParamDef {
            id: DC,
            name: "dc",
            min: 0.0,
            max: 1.0,
            // Off. A DC blocker is a highpass at 5 Hz, and one that
            // arrived switched on would quietly change the bottom of
            // every track it was dropped onto.
            default: 0.0,
        },
    ];

    /// A switch position, clamped into its list. RON round-trips NaN, so
    /// a hand-edited project can smuggle one in and this is where it
    /// stops — the same door [`glue`](super::glue) uses, for the same
    /// reason.
    pub fn index(value: f32, count: usize) -> u32 {
        if value.is_finite() {
            (value.round().max(0.0) as u32).min(count.saturating_sub(1) as u32)
        } else {
            0
        }
    }

    /// The sign each side is multiplied by, for a [`PHASE`] index. The
    /// ONE place the wire index becomes arithmetic — the card's cell
    /// reads the same list in the same order, so the drawing and the
    /// audio flip the same channel.
    pub fn phase_signs(index: u32) -> (f32, f32) {
        match index.min(PHASE_BOTH) {
            PHASE_L => (-1.0, 1.0),
            PHASE_R => (1.0, -1.0),
            PHASE_BOTH => (-1.0, -1.0),
            _ => (1.0, 1.0),
        }
    }

    /// Whether the bass-mono crossover does anything at this corner.
    ///
    /// A single place, because the node skipping the filter and the card
    /// printing "off" have to agree: a stage that is drawn as bypassed
    /// and still running is how a null test stops nulling.
    pub fn mono_off(hz: f32) -> bool {
        !hz.is_finite() || hz <= MONO_MIN_HZ
    }
}

/// The sampler — the device that plays a file, and has an opinion about
/// how it should sound coming back out.
///
/// Thirty-six rows across five pages. Design and reasoning:
/// `notes/20260827-sampler-brief.md`.
///
/// Everything here is in ENGINE units — hertz, milliseconds, semitones,
/// decibels — except the four positions ([`START`], [`END`],
/// [`LOOP_START`], and the `start` modulation destination), which are
/// FRACTIONS OF THE FILE. That is deliberate: a position stored in frames
/// would point somewhere else the moment a different sample was dropped
/// on the card, and a sampler is a device you drop samples on.
pub mod sampler {
    use super::ParamDef;

    pub const MODE: u32 = 0;
    pub const START: u32 = 1;
    pub const END: u32 = 2;
    pub const REVERSE: u32 = 3;
    pub const FADE_IN: u32 = 4;
    pub const FADE_OUT: u32 = 5;
    pub const ROOT: u32 = 6;
    pub const TUNE: u32 = 7;
    pub const FINE: u32 = 8;

    pub const LOOP_MODE: u32 = 9;
    pub const LOOP_START: u32 = 10;
    pub const LOOP_XFADE: u32 = 11;
    pub const SLICES: u32 = 12;
    pub const SLICE_SOURCE: u32 = 13;
    pub const CHOKE: u32 = 14;

    pub const AMP_A: u32 = 15;
    pub const AMP_D: u32 = 16;
    pub const AMP_S: u32 = 17;
    pub const AMP_R: u32 = 18;
    pub const FILT_MODE: u32 = 19;
    pub const CUTOFF: u32 = 20;
    pub const RES: u32 = 21;
    pub const KEYTRACK: u32 = 22;

    pub const MOD_A: u32 = 23;
    pub const MOD_D: u32 = 24;
    pub const MOD_S: u32 = 25;
    pub const MOD_R: u32 = 26;
    pub const MOD_DEST: u32 = 27;
    pub const MOD_DEPTH: u32 = 28;
    pub const VELOCITY: u32 = 29;

    pub const DRIVE: u32 = 30;
    pub const RATE: u32 = 31;
    pub const BITS: u32 = 32;
    pub const PREAMP: u32 = 33;
    pub const GAIN: u32 = 34;
    pub const PAN: u32 = 35;
    /// Which slice a note plays, in slice mode, counted from one. A
    /// parameter rather than an address in the keyboard, so a trig locks
    /// it the way it locks anything else and the note is free to be a
    /// pitch — the Octatrack's SLIC, exactly.
    pub const SLICE: u32 = 36;

    // ------------------------------------------------------- discretes ---

    /// What a note MEANS. See the brief's mode table.
    pub const MODE_CLASSIC: f32 = 0.0;
    pub const MODE_ONE_SHOT: f32 = 1.0;
    pub const MODE_SLICE: f32 = 2.0;
    pub const MODE_MAX: f32 = MODE_SLICE;
    pub const MODE_NAMES: &[&str] = &["classic", "1-shot", "slice"];

    pub const LOOP_OFF: f32 = 0.0;
    pub const LOOP_FORWARD: f32 = 1.0;
    pub const LOOP_PINGPONG: f32 = 2.0;
    pub const LOOP_MODE_MAX: f32 = LOOP_PINGPONG;
    pub const LOOP_NAMES: &[&str] = &["off", "fwd", "ping"];

    pub const SLICE_GRID: f32 = 0.0;
    pub const SLICE_TRANSIENTS: f32 = 1.0;
    /// The table was authored BY HAND and nothing may re-cut it.
    ///
    /// Not a third way of slicing — a latch. Dragging a marker sets it,
    /// because a grid that is re-derived from the count every frame would
    /// otherwise walk over the drag on the very next one. Making that
    /// visible in a cell is the point: when the count knob stops re-cutting
    /// the file, the card can say why.
    pub const SLICE_CUSTOM: f32 = 2.0;
    pub const SLICE_SOURCE_MAX: f32 = SLICE_CUSTOM;
    pub const SLICE_SOURCE_NAMES: &[&str] = &["grid", "onset", "custom"];

    pub const OFF_ON_NAMES: &[&str] = &["off", "on"];

    /// Filter shapes, in the order [`crate::dsp::filters::Mode`] wants
    /// them. One filter, four faces — the brief asks for a single filter
    /// and this is what "single" costs.
    pub const FILT_LOWPASS: f32 = 0.0;
    pub const FILT_HIGHPASS: f32 = 1.0;
    pub const FILT_BANDPASS: f32 = 2.0;
    pub const FILT_NOTCH: f32 = 3.0;
    pub const FILT_MODE_MAX: f32 = FILT_NOTCH;
    pub const FILT_NAMES: &[&str] = &["lp", "hp", "bp", "notch"];

    /// Where the one modulation envelope goes.
    pub const DEST_CUTOFF: f32 = 0.0;
    pub const DEST_PITCH: f32 = 1.0;
    pub const DEST_START: f32 = 2.0;
    pub const DEST_RATE: f32 = 3.0;
    pub const DEST_DRIVE: f32 = 4.0;
    pub const DEST_PAN: f32 = 5.0;
    pub const DEST_MAX: f32 = DEST_PAN;
    pub const DEST_NAMES: &[&str] = &["cutoff", "pitch", "start", "rate", "drive", "pan"];

    // ------------------------------------------------------------ spans ---

    /// The most slices one file can be cut into. Sixty-four is the
    /// Octatrack's number and it is the right one: past that the notes
    /// run off the top of a five-octave keyboard.
    pub const SLICES_MAX: f32 = 64.0;

    /// How far the pitch knobs reach. Two octaves either way covers
    /// re-tuning a one-shot into a bassline without turning the range
    /// into a place you cannot find zero in.
    pub const TUNE_MAX: f32 = 24.0;

    /// Full-scale throw of each modulation destination, at `depth = 1`.
    /// The card prints these, so they live here rather than inside the
    /// voice where the card could not see them.
    pub const DEST_CUTOFF_OCTAVES: f32 = 5.0;
    pub const DEST_PITCH_SEMITONES: f32 = 24.0;
    pub const DEST_START_FRACTION: f32 = 0.25;
    pub const DEST_RATE_OCTAVES: f32 = 4.0;

    /// The MIDI note that plays slice 0, and every semitone above it the
    /// next slice.
    pub const SLICE_BASE_NOTE: u8 = 36;

    /// Which page each row belongs to, for the card's tab strip. Parallel
    /// to [`TABLE`] and checked against it by a test, because a row that
    /// no page claims is a row nobody can reach.
    pub const PAGES: &[&str] = &["sample", "loop", "shape", "mod", "dirt"];

    pub fn page_of(id: u32) -> usize {
        match id {
            MODE..=FINE | SLICE => 0,
            LOOP_MODE..=CHOKE => 1,
            AMP_A..=KEYTRACK => 2,
            MOD_A..=VELOCITY => 3,
            _ => 4,
        }
    }

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: MODE,
            name: "mode",
            min: 0.0,
            max: MODE_MAX,
            default: MODE_CLASSIC,
        },
        ParamDef {
            id: START,
            name: "start",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: END,
            name: "end",
            min: 0.0,
            max: 1.0,
            default: 1.0,
        },
        ParamDef {
            id: REVERSE,
            name: "reverse",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: FADE_IN,
            // NOT zero by default. A sample cut mid-cycle clicks, and a
            // device that clicks out of the box is a broken device.
            name: "fadein",
            min: 0.0,
            max: 2_000.0,
            default: 2.0,
        },
        ParamDef {
            id: FADE_OUT,
            // Also the choke ramp and the voice-stealing ramp, which is
            // why it is longer than the fade in: what it mostly does is
            // get out of the way of the next note.
            name: "fadeout",
            min: 0.0,
            max: 2_000.0,
            default: 5.0,
        },
        ParamDef {
            id: ROOT,
            name: "root",
            min: 0.0,
            max: 127.0,
            default: 60.0,
        },
        ParamDef {
            id: TUNE,
            name: "tune",
            min: -TUNE_MAX,
            max: TUNE_MAX,
            default: 0.0,
        },
        ParamDef {
            id: FINE,
            name: "fine",
            min: -100.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: LOOP_MODE,
            name: "loop",
            min: 0.0,
            max: LOOP_MODE_MAX,
            default: LOOP_OFF,
        },
        ParamDef {
            id: LOOP_START,
            name: "loopstart",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: LOOP_XFADE,
            name: "xfade",
            min: 0.0,
            max: 500.0,
            default: 10.0,
        },
        ParamDef {
            id: SLICES,
            name: "slices",
            min: 1.0,
            max: SLICES_MAX,
            default: 16.0,
        },
        ParamDef {
            id: SLICE_SOURCE,
            name: "slicefrom",
            min: 0.0,
            max: SLICE_SOURCE_MAX,
            default: SLICE_GRID,
        },
        ParamDef {
            id: CHOKE,
            name: "choke",
            min: 0.0,
            max: 1.0,
            // On: a new slice cuts the last one. That is what makes a
            // chopped break sit still instead of smearing.
            default: 1.0,
        },
        ParamDef {
            id: AMP_A,
            name: "attack",
            min: 0.1,
            max: 8_000.0,
            default: 1.0,
        },
        ParamDef {
            id: AMP_D,
            name: "decay",
            min: 1.0,
            max: 16_000.0,
            default: 800.0,
        },
        ParamDef {
            id: AMP_S,
            name: "sustain",
            min: 0.0,
            max: 1.0,
            // Held open, so a fresh sampler plays the file rather than an
            // envelope's opinion of it.
            default: 1.0,
        },
        ParamDef {
            id: AMP_R,
            name: "release",
            min: 1.0,
            max: 16_000.0,
            default: 40.0,
        },
        ParamDef {
            id: FILT_MODE,
            name: "filter",
            min: 0.0,
            max: FILT_MODE_MAX,
            default: FILT_LOWPASS,
        },
        ParamDef {
            id: CUTOFF,
            name: "cutoff",
            min: 20.0,
            max: 20_000.0,
            default: 20_000.0,
        },
        ParamDef {
            id: RES,
            name: "res",
            min: 0.5,
            max: 20.0,
            default: std::f32::consts::FRAC_1_SQRT_2,
        },
        ParamDef {
            id: KEYTRACK,
            name: "keytrack",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: MOD_A,
            name: "modatk",
            min: 0.1,
            max: 8_000.0,
            default: 1.0,
        },
        ParamDef {
            id: MOD_D,
            name: "moddec",
            min: 1.0,
            max: 16_000.0,
            default: 200.0,
        },
        ParamDef {
            id: MOD_S,
            name: "modsus",
            min: 0.0,
            max: 1.0,
            // Zero: this is an envelope, not a level. A mod envelope that
            // sustains at full is a knob offset wearing a disguise.
            default: 0.0,
        },
        ParamDef {
            id: MOD_R,
            name: "modrel",
            min: 1.0,
            max: 16_000.0,
            default: 100.0,
        },
        ParamDef {
            id: MOD_DEST,
            name: "dest",
            min: 0.0,
            max: DEST_MAX,
            default: DEST_CUTOFF,
        },
        ParamDef {
            id: MOD_DEPTH,
            name: "depth",
            min: -1.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: VELOCITY,
            name: "vel",
            min: 0.0,
            max: 1.0,
            default: 0.5,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: 0.0,
            max: 1.0,
            // On by default, and only just. Enough to round a transient's
            // tip; not enough to hear as distortion on a pad.
            default: 0.15,
        },
        ParamDef {
            id: RATE,
            name: "rate",
            min: crate::dsp::lofi::RATE_MIN,
            max: 48_000.0,
            // The S900's neighbourhood. Subtle, not crushed: what it
            // mostly does is take the air off.
            default: 32_000.0,
        },
        ParamDef {
            id: BITS,
            name: "bits",
            min: crate::dsp::lofi::BITS_MIN,
            max: crate::dsp::lofi::BITS_MAX,
            // Twelve, because that is what the machines this borrows from
            // had. Continuous, because thirteen and a half is a perfectly
            // good lattice and sometimes the right one.
            default: 12.0,
        },
        ParamDef {
            id: PREAMP,
            name: "preamp",
            min: 0.0,
            max: 1.0,
            default: 0.25,
        },
        ParamDef {
            id: GAIN,
            name: "gain",
            min: -60.0,
            max: 12.0,
            default: 0.0,
        },
        ParamDef {
            id: PAN,
            name: "pan",
            min: -1.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: SLICE,
            name: "slice",
            min: 1.0,
            max: SLICES_MAX,
            default: 1.0,
        },
    ];
}

/// The lo-fi converter — an early sampler's front end, as a device.
///
/// Two knobs of real character and two of housekeeping. `RATE` is the
/// converter's own clock and `BITS` its word length, and both are the
/// kernel's: `dsp::lofi::Downsampler` owns the tracking anti-alias
/// filter, the zero-order hold and the quantiser, and this table only
/// says what a musician is allowed to ask for.
///
/// # The top of each range is OFF, and exactly off
///
/// The kernel promises an EXACT identity at `rate >= sample_rate` and at
/// `bits >= 16`, checked per block rather than per sample. Both ends are
/// reachable here on purpose, so "how much of this is the device?" is
/// answerable by turning two knobs to their stops rather than by
/// bypassing the card. A colour you cannot remove is a colour you cannot
/// measure.
///
/// [`RATE_MAX`] is 48 kHz rather than the running sample rate because a
/// TABLE is `&'static` — `params::clamp` scans it inside the callback —
/// and a bound that moved with the device's preparation could not be.
/// At any session rate at or below 48 kHz the top of the knob is at or
/// above it, so the off switch stays reachable.
/// The sheen — a slew-driven brightener, as a device.
///
/// `dsp::dynamics::SlewBrighten` owns the whole idea: a high-frequency
/// lift whose size is driven by how fast the signal is actually MOVING,
/// so a snare edge gets it and a held pad does not. This table only says
/// what a musician is allowed to ask for.
///
/// # The bottom of `AMOUNT` is off, and exactly off
///
/// The kernel promises that amount 0 is a bit-exact wire rather than
/// merely a quiet one, which is what lets a device leave the stage
/// permanently in its path. That end is reachable here on purpose, for
/// the reason [`lofi`](super::lofi)'s two off switches are: a colour you
/// cannot remove is a colour you cannot measure.
///
/// # What is NOT here
///
/// The envelope's attack and release, and the knee that decides where
/// "fast" starts, are the kernel's own constants and stay there. They are
/// what make this a brightener rather than a general-purpose exciter, and
/// the kernel's doc argues each figure. A device that exposed them would
/// be offering the user a way to turn it into the harshness it exists to
/// avoid.
/// The disperser — a chain of allpasses, as a device.
///
/// `dsp::filters::Disperser` owns the whole idea: sections that pass every
/// frequency at unity gain and do nothing but delay each one differently,
/// so a transient stops arriving all at once and a click becomes a
/// descending chirp. This table only says what a musician may ask for.
///
/// Modelled on Kilohearts' Disperser, which is the reference everyone has
/// heard, and whose three controls map exactly onto what our kernel
/// already takes: how many sections, where they are tuned, and how
/// tightly the phase turns there.
///
/// # There is no MIX, and that is the device
///
/// Every other effect here blends against the dry. This one must not.
/// The kernel's load-bearing property is that its magnitude response is
/// FLAT at every setting — that is what makes it safe across a drum — and
/// flatness is a property of the wet signal alone. Sum it with the dry
/// and the two disagree in phase, which is a comb filter: notches that
/// move with the frequency knob. That is a phaser, and a good one, but it
/// is a different device and it does not keep this one's promise.
///
/// # Zero stages is the off switch
///
/// The kernel says so plainly, and the range starts there on purpose, for
/// the reason [`lofi`](super::lofi) and [`sheen`](super::sheen) both give:
/// a colour you cannot remove is a colour you cannot measure. The
/// reference starts at one section; we start at none, because ours can.
/// The tilt — a see-saw around a pivot, as a device.
///
/// `dsp::filters::Tilt` owns the whole idea: highs up and lows down by
/// the same amount, or the reverse, and unity exactly AT the pivot. One
/// gesture for "brighter" or "darker" that keeps the level where it was.
///
/// The kernel has been in the tree since the sampler's preamp, which uses
/// it with the pivot nailed to 1 kHz and the direction fixed. This table
/// is what it takes to hand both to the user.
///
/// # Two rows, and why there is no third
///
/// No output trim, unlike every other colour device here. A see-saw is
/// unity at its pivot and moves the two ends in opposite directions, so
/// broadband material comes out at about the level it went in — there is
/// no make-up to make up. A third row would be a third thing to set on a
/// device whose entire appeal is that it is one gesture.
///
/// # Flat is the DEFAULT, which is the opposite of the lo-fi's argument
///
/// [`lofi`](super::lofi) opens audibly coloured, because a colour device
/// that does nothing until you turn a knob is a surprise. This one opens
/// FLAT, and the difference is what the device is for. You reach for a
/// tilt to make a decision about a track; a tilt that arrived having
/// already made one would be making it on your behalf, and you would
/// have to undo it before you could start. Corrective devices open
/// neutral. Colouring devices open coloured.
///
/// Flat here is the kernel's exact wire — zero tilt is `g_hi = 1` and
/// `g_delta = 0`, which is `x` — so the off setting is exact in the way
/// [`lofi`](super::lofi) and [`sheen`](super::sheen) both insist on.
/// The phaser — the disperser's allpass chain, swept and MIXED.
///
/// [`disperser`](super::disperser) refuses a mix knob, and its module
/// says exactly why: summing a phase-shifted copy with the dry is a comb
/// filter whose notches move with the frequency, which breaks the flat
/// magnitude that device promises. "That is a phaser, and a good one, but
/// it is a different device."
///
/// This is that device. Same kernel, same sections; the mix is the point
/// rather than the mistake, and an LFO walks the corner so the notches
/// sweep.
///
/// # What it does not expose
///
/// The sections' Q. The disperser offers it as `pinch` because a
/// disperser is a tuning instrument — you aim it at a harmonic. A phaser
/// is a sweep, and a narrow Q turns the notches into a ringing pitch that
/// fights the sweep rather than riding it. It is fixed at
/// `audio::graph`'s `PHASER_Q`.
///
/// # Zero stages is the off switch
///
/// With no sections the wet path IS the dry path, so the blend has
/// nothing to cancel against and the device passes through whatever the
/// mix says. The same exact-off the rest of these devices insist on,
/// arriving for free out of the kernel's own wire.
/// The gate — downward expansion, as a device.
///
/// `dsp::dynamics::Mode::Expand` has existed since the dynamics family
/// landed, and its own doc calls it "a gate at high ratio". Until now
/// nothing in the audio path used it: `audio::glue` is a compressor, and
/// the only caller of `Expand` anywhere was the display widget that draws
/// its curve. This table is what it takes to make it a device.
///
/// # Attack and release mean the opposite of what they mean on the glue
///
/// A compressor ATTACKS by pulling the gain DOWN. A gate attacks by
/// letting it UP — the signal crossed the threshold and the gate opens.
/// `dsp::dynamics::Ballistics` uses the compressor's convention (attack
/// is whichever direction adds reduction), so `audio::gate` hands it the
/// two times SWAPPED. See `GateCore::process`; a test measures the
/// opening and closing times so the swap cannot quietly come undone.
///
/// # And it listens to its INPUT, not its output
///
/// The glue is a feedback compressor: its detector reads its own output,
/// which is what gives it its character. A gate must not be. Once a
/// feedback gate closed, its detector would hear the silence it had just
/// made, decide the signal was still below the threshold, and stay shut
/// forever. Feed-forward is not a preference here, it is the only
/// topology that reopens.
/// The strip — a mini channel: two shelves, an output stage, and a
/// switch.
///
/// Three things that already exist, in one device and in one order:
/// `dsp::filters::EqBand` for the tone, `audio::preamp::Preamp` for the
/// colour, and a second pair of fixed shelves behind [`WARM`].
///
/// `preamp.rs`'s own header predicted this device — "it lives in its own
/// file rather than inside `sampler.rs` because it is a stage a future
/// effect device would want whole" — and this is that effect device. Its
/// character is not invented here: a tilt at 1 kHz, an asymmetric soft
/// clip reached through headroom so the second harmonic sits above the
/// third, a DC blocker, and hiss gated by the programme. All measured,
/// all already tested.
///
/// # The character: EVERYTHING IS PRE-DRIVE
///
/// This is the one thing that makes the strip more than its three parts
/// bolted together, and it is deliberate. The shelves and the warm switch
/// all sit BEFORE the output stage, so they do not merely shape the tone
/// — they change what the non-linearity is fed. Lift the low shelf and
/// the bottom end drives the clip harder, which is more second harmonic
/// ON THE BASS specifically. That is what a console does and why its EQ
/// sounds different from the same curve applied afterwards.
///
/// It also means the two halves are not independent, and the card says so
/// rather than hiding it: at zero drive the strip IS just an equaliser,
/// and every dB of shelf is worth more the further the drive is up.
///
/// # What [`WARM`] is, and what it is not
///
/// A fixed pair of shelves — a lift at the bottom, a gentle softening at
/// the very top — engaged before the output stage. The transformer curve,
/// roughly.
///
/// It is NOT a second copy of the preamp's own tilt, which is a see-saw
/// across the whole band pivoting at 1 kHz. This touches only the
/// extremes and leaves the mids alone, so the two stack rather than
/// duplicate: the tilt changes the balance, the switch changes the ends.
///
/// And because it is pre-drive, the button does more than an EQ curve
/// could. With the drive down it is a couple of dB at the edges; with the
/// drive up it is also a louder bottom end arriving at the clip.
///
/// # Zero drive is exactly off
///
/// `Preamp` promises a bit-exact bypass at `amount == 0`, checked per
/// block. The strip's drive reaches that floor, so "how much of this is
/// the colour?" is answerable by turning one knob down rather than by
/// bypassing the card — the same promise [`lofi`](super::lofi) and
/// [`sheen`](super::sheen) make. The shelves keep working, which is the
/// point: at zero drive this is a clean two-band EQ.
/// The resynthesiser — the spectrum taken apart and put back together.
///
/// The first device in the tree to run `dsp::fft` in the AUDIO PATH.
/// Until the tilt card measured a curve with it, nothing outside
/// `src/dsp/` had called that module at all; this is the other half of
/// paying it off.
///
/// Sound goes in, is cut into overlapping frames, transformed, altered as
/// a MAGNITUDE SPECTRUM, and rebuilt. Everything the device does happens
/// to that spectrum, which is why the controls are the ones they are: a
/// gain per band, a formant shift, a spectral shift, and an envelope with
/// its own attack and release.
///
/// # It is a resynthesiser, and that is a promise about the sound
///
/// The phase is left where it was found and only the magnitudes are
/// moved. That is what makes this a resynthesis rather than a clean
/// pitch or frequency shifter: it smears, and the smearing is the sound.
/// A device that hid it would be a worse version of a different device.
///
/// # Latency
///
/// Real, constant, and reported: `SIZE` samples — the frame overlap plus
/// the hop the output is buffered by. The graph's PDC compensates it, and
/// `audio::resyn`'s `the_reported_latency_is_the_real_one` measures where
/// an impulse actually comes out rather than trusting the arithmetic.
/// The acid mono — a classic hardware monosynth, emulated.
///
/// One oscillator, an eighteen-decibel resonant lowpass, one envelope
/// pointed at the filter, and a sequencer that can slide and accent. The
/// machine everybody means by "acid".
///
/// # Why this one, and why it is MONO on purpose
///
/// The rack already has a sixteen-voice workhorse. What it did not have
/// is an instrument whose character comes from having exactly one voice:
/// on this synth a slide is what happens when two notes overlap, and an
/// accent is what happens when one is louder than its neighbours. Both
/// are properties of a single voice being handed a queue of notes, and
/// neither survives being made polyphonic.
///
/// Our `Note` already carries `vel` and a length that can run past the
/// next note's start, so both gestures were already expressible before
/// this device existed. That is the whole reason it is worth building.
///
/// # The slope
///
/// Eighteen decibels per octave, which is the machine's own and is not a
/// number a Butterworth cascade offers: `dsp::filters::Svf` gives twelve
/// with the resonance, and `dsp::filters::OnePole` adds the other six.
/// Two kernels, no new arithmetic, the right slope.
///
/// # What is deliberately NOT a knob
///
/// The amplifier's envelope. The original has none — the VCA opens fast
/// when the gate does and shuts fast when it lets go, and the knob
/// labelled DECAY is the FILTER's. Exposing an amp ADSR would make this a
/// generic monosynth wearing the name.
pub mod acid {
    use super::ParamDef;

    pub const WAVE: u32 = 0;
    pub const TUNE: u32 = 1;
    pub const CUTOFF: u32 = 2;
    pub const RESONANCE: u32 = 3;
    pub const ENV_MOD: u32 = 4;
    pub const DECAY: u32 = 5;
    pub const ACCENT: u32 = 6;
    pub const GLIDE: u32 = 7;
    pub const DRIVE: u32 = 8;
    pub const LEVEL: u32 = 9;

    pub const WAVE_SAW: u32 = 0;
    pub const WAVE_SQUARE: u32 = 1;
    /// The two shapes, in wire order. The machine had exactly these and
    /// a switch between them.
    pub const WAVE_NAMES: &[&str] = &["saw", "square"];

    /// Coarse tuning, in semitones.
    pub const TUNE_MAX_ST: f32 = 24.0;

    /// The filter's window. The floor is low enough to close the sound
    /// almost entirely, which is half of what the knob is for.
    pub const CUTOFF_MIN_HZ: f32 = 60.0;
    pub const CUTOFF_MAX_HZ: f32 = 12_000.0;

    /// How far a full envelope at full depth opens the filter, in
    /// octaves. Five, which is a sweep from a closed thump to an open
    /// buzz — the range the squelch lives in.
    pub const ENV_OCTAVES: f32 = 5.0;

    /// The filter envelope's decay, in ms.
    pub const DECAY_MIN_MS: f32 = 30.0;
    pub const DECAY_MAX_MS: f32 = 2_000.0;

    /// The slide's time, in ms. Only a SLID note glides — see
    /// `audio::acid`; a note arriving on its own snaps to pitch, because
    /// a portamento that applied to every note would be a different
    /// instrument.
    pub const GLIDE_MIN_MS: f32 = 1.0;
    pub const GLIDE_MAX_MS: f32 = 300.0;

    /// Above this velocity a note is ACCENTED. The original had a switch
    /// per step rather than a continuum; a velocity threshold is the same
    /// gesture in a sequencer that already stores one.
    pub const ACCENT_VEL: u8 = 100;

    /// The amplifier's fixed edges, in ms. Not knobs — see the module
    /// header.
    pub const AMP_ATTACK_MS: f32 = 3.0;
    pub const AMP_RELEASE_MS: f32 = 12.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: WAVE,
            name: "wave",
            min: WAVE_SAW as f32,
            max: WAVE_SQUARE as f32,
            // The saw. Both are canonical, and the saw is the one on the
            // records.
            default: WAVE_SAW as f32,
        },
        ParamDef {
            id: TUNE,
            name: "tune",
            min: -TUNE_MAX_ST,
            max: TUNE_MAX_ST,
            default: 0.0,
        },
        ParamDef {
            id: CUTOFF,
            name: "cutoff",
            min: CUTOFF_MIN_HZ,
            max: CUTOFF_MAX_HZ,
            // Low, because the envelope opens it. A cutoff parked high
            // leaves the env mod nothing to do, which is the commonest
            // way this instrument gets set up to sound like nothing.
            default: 400.0,
        },
        ParamDef {
            id: RESONANCE,
            name: "resonance",
            min: 0.0,
            max: 1.0,
            // High. The squelch IS the resonance, and a cautious default
            // here would be a cautious default on the one control the
            // device exists for.
            default: 0.65,
        },
        ParamDef {
            id: ENV_MOD,
            name: "env mod",
            min: 0.0,
            max: 1.0,
            default: 0.55,
        },
        ParamDef {
            id: DECAY,
            name: "decay",
            min: DECAY_MIN_MS,
            max: DECAY_MAX_MS,
            default: 300.0,
        },
        ParamDef {
            id: ACCENT,
            name: "accent",
            min: 0.0,
            max: 1.0,
            default: 0.5,
        },
        ParamDef {
            id: GLIDE,
            name: "glide",
            min: GLIDE_MIN_MS,
            max: GLIDE_MAX_MS,
            default: 60.0,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: 0.0,
            max: 1.0,
            // A little. The filter's own saturation is part of the
            // character; the rack's saturator is where a lot of it lives.
            default: 0.15,
        },
        ParamDef {
            id: LEVEL,
            name: "level",
            min: 0.0,
            max: 2.0,
            default: 0.8,
        },
    ];
}

pub mod resyn {
    use super::ParamDef;

    pub const FORMANT: u32 = 0;
    pub const SHIFT: u32 = 1;
    pub const ATTACK: u32 = 2;
    pub const RELEASE: u32 = 3;
    pub const WARM: u32 = 4;
    pub const MIX: u32 = 5;
    /// The first of [`BAND_COUNT`] consecutive per-band gain ids.
    pub const BAND0: u32 = 6;

    /// How many bands the spectrum is divided into for the gain rows.
    ///
    /// Eight, log-spaced. Enough to be a tone control with real reach and
    /// few enough that one row of the card can pick between them, which
    /// is the trade `eq.rs` already made and argued.
    pub const BAND_COUNT: usize = 8;

    /// The band edges, in Hz: `BAND_COUNT + 1` of them, log-spaced.
    pub const BAND_EDGES: [f32; BAND_COUNT + 1] = [
        20.0, 80.0, 200.0, 450.0, 1_000.0, 2_200.0, 4_800.0, 10_000.0, 20_000.0,
    ];

    /// A band gain's window, in dB. The floor is a real cut rather than
    /// silence: a band switched entirely off is a hole a resynthesis
    /// cannot fill, and the result reads as a fault rather than a
    /// setting.
    pub const BAND_MIN_DB: f32 = -24.0;
    pub const BAND_MAX_DB: f32 = 12.0;

    /// The formant shift, in semitones. The spectral ENVELOPE moves and
    /// the fine structure stays, which is what separates a voice's
    /// character from its pitch.
    pub const FORMANT_MAX_ST: f32 = 12.0;

    /// The spectral shift, in Hz. ADDITIVE, not a ratio — every partial
    /// moves by the same number of hertz, so their ratios change and the
    /// result is inharmonic. That is the classic spectral shift and it is
    /// deliberately not a pitch shift.
    pub const SHIFT_MAX_HZ: f32 = 500.0;

    /// The per-bin envelope's times, in ms. The attack is how fast a
    /// partial is allowed to appear and the release how fast it may
    /// leave; long releases are what turn programme into a pad.
    pub const ATTACK_MIN_MS: f32 = 1.0;
    pub const ATTACK_MAX_MS: f32 = 500.0;
    pub const RELEASE_MIN_MS: f32 = 1.0;
    pub const RELEASE_MAX_MS: f32 = 4_000.0;

    pub const WARM_OFF: u32 = 0;
    pub const WARM_ON: u32 = 1;
    pub const WARM_NAMES: &[&str] = &["off", "on"];

    /// The warm mode's spectral tilt, in dB at the top of the band.
    ///
    /// Distinct from [`strip`](super::strip)'s warm switch, which is a
    /// pair of shelves in the time domain. This one is a tilt applied to
    /// the MAGNITUDES, and it comes with the half a time-domain filter
    /// cannot do: a gentle compression of each bin, which lifts quiet
    /// partials toward the loud ones. That is what thickens a
    /// resynthesis, and it has no equivalent as an EQ curve.
    pub const WARM_TILT_DB: f32 = -6.0;
    /// The exponent each magnitude is raised to in warm mode. Below one,
    /// so quiet partials come up.
    pub const WARM_EXPONENT: f32 = 0.85;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: FORMANT,
            name: "formant",
            min: -FORMANT_MAX_ST,
            max: FORMANT_MAX_ST,
            default: 0.0,
        },
        ParamDef {
            id: SHIFT,
            name: "shift",
            min: -SHIFT_MAX_HZ,
            max: SHIFT_MAX_HZ,
            default: 0.0,
        },
        ParamDef {
            id: ATTACK,
            name: "attack",
            min: ATTACK_MIN_MS,
            max: ATTACK_MAX_MS,
            // Fast enough to keep a transient recognisable. The device is
            // already smearing; an attack that smeared further by default
            // would make it sound broken rather than spectral.
            default: 5.0,
        },
        ParamDef {
            id: RELEASE,
            name: "release",
            min: RELEASE_MIN_MS,
            max: RELEASE_MAX_MS,
            default: 80.0,
        },
        ParamDef {
            id: WARM,
            name: "warm",
            min: WARM_OFF as f32,
            max: WARM_ON as f32,
            default: WARM_OFF as f32,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            // Fully wet. A resynthesis blended with its own dry is a comb
            // filter on everything the two still agree about, which is
            // most of the signal at rest — audible, unwanted, and the
            // reason this opens at the top rather than in the middle.
            default: 1.0,
        },
        ParamDef {
            id: BAND0 + 0,
            name: "band 0",
            min: -BAND_MIN_DB.abs(),
            max: BAND_MAX_DB,
            default: 0.0,
        },
        ParamDef {
            id: BAND0 + 1,
            name: "band 1",
            min: -BAND_MIN_DB.abs(),
            max: BAND_MAX_DB,
            default: 0.0,
        },
        ParamDef {
            id: BAND0 + 2,
            name: "band 2",
            min: -BAND_MIN_DB.abs(),
            max: BAND_MAX_DB,
            default: 0.0,
        },
        ParamDef {
            id: BAND0 + 3,
            name: "band 3",
            min: -BAND_MIN_DB.abs(),
            max: BAND_MAX_DB,
            default: 0.0,
        },
        ParamDef {
            id: BAND0 + 4,
            name: "band 4",
            min: -BAND_MIN_DB.abs(),
            max: BAND_MAX_DB,
            default: 0.0,
        },
        ParamDef {
            id: BAND0 + 5,
            name: "band 5",
            min: -BAND_MIN_DB.abs(),
            max: BAND_MAX_DB,
            default: 0.0,
        },
        ParamDef {
            id: BAND0 + 6,
            name: "band 6",
            min: -BAND_MIN_DB.abs(),
            max: BAND_MAX_DB,
            default: 0.0,
        },
        ParamDef {
            id: BAND0 + 7,
            name: "band 7",
            min: -BAND_MIN_DB.abs(),
            max: BAND_MAX_DB,
            default: 0.0,
        },
    ];
}

pub mod strip {
    use super::ParamDef;

    pub const LOW: u32 = 0;
    pub const HIGH: u32 = 1;
    pub const DRIVE: u32 = 2;
    pub const WARM: u32 = 3;
    pub const OUT: u32 = 4;

    /// The shelves' window, in dB. Modest on purpose: a channel strip's
    /// tone controls are for leaning on a source, and the eight-band EQ
    /// next door is the device for surgery.
    pub const SHELF_MAX_DB: f32 = 12.0;

    /// Where the user's shelves sit. Fixed, because two knobs that also
    /// chose their own corners would be four knobs.
    pub const LOW_HZ: f32 = 120.0;
    pub const HIGH_HZ: f32 = 8_000.0;

    /// The shelves' Q. Gentle — a shelf that resonates at its corner is a
    /// bell wearing a shelf's name.
    pub const SHELF_Q: f32 = 0.7;

    /// The warm switch's own curve: a lift at the bottom and a softening
    /// at the top, both fixed. Small figures, because the button is meant
    /// to be reached for and left on rather than auditioned.
    pub const WARM_LOW_HZ: f32 = 100.0;
    pub const WARM_LOW_DB: f32 = 2.0;
    pub const WARM_HIGH_HZ: f32 = 10_000.0;
    pub const WARM_HIGH_DB: f32 = -1.5;

    /// The output trim's window, in dB, and the same figures as linear
    /// gain — both forms written down for the reason
    /// [`sat::OUT_MIN_DB`](super::sat::OUT_MIN_DB) gives.
    pub const OUT_MIN_DB: f32 = -24.0;
    pub const OUT_MAX_DB: f32 = 12.0;
    /// `10^(-24/20)` and `10^(12/20)`, to f32 precision.
    pub const OUT_MIN: f32 = 0.063_095_73;
    pub const OUT_MAX: f32 = 3.981_072;

    pub const WARM_OFF: u32 = 0;
    pub const WARM_ON: u32 = 1;
    /// What the switch prints, indexed by [`WARM`].
    pub const WARM_NAMES: &[&str] = &["off", "on"];

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: LOW,
            name: "low",
            min: -SHELF_MAX_DB,
            max: SHELF_MAX_DB,
            default: 0.0,
        },
        ParamDef {
            id: HIGH,
            name: "high",
            min: -SHELF_MAX_DB,
            max: SHELF_MAX_DB,
            default: 0.0,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: 0.0,
            max: 1.0,
            // Coloured on arrival, for the reason `lofi`'s table gives:
            // you add an analogue strip because you want the analogue,
            // and one that does nothing until you turn a knob is the
            // surprise. A third of the way up is where the preamp's
            // second harmonic is audible and its hiss is not.
            default: 0.35,
        },
        ParamDef {
            id: WARM,
            name: "warm",
            min: WARM_OFF as f32,
            max: WARM_ON as f32,
            // OFF. The shelves default flat and so does this: the strip
            // arrives with a colour (the drive) and no TONE decision,
            // because a tone decision is about the source and the device
            // has not heard it yet.
            default: WARM_OFF as f32,
        },
        ParamDef {
            id: OUT,
            name: "out",
            min: OUT_MIN,
            max: OUT_MAX,
            default: 1.0,
        },
    ];
}

pub mod gate {
    use super::ParamDef;

    pub const THRESHOLD: u32 = 0;
    pub const RATIO: u32 = 1;
    pub const ATTACK: u32 = 2;
    pub const RELEASE: u32 = 3;
    pub const RANGE: u32 = 4;

    /// Where the gate decides, in dBFS.
    ///
    /// # These two ranges are the DISPLAY's, deliberately
    ///
    /// `ui::device::dynamics` draws its transfer curve across
    /// `VIEW_MIN_DB..VIEW_MAX_DB` with a ratio axis of
    /// `RATIO_MIN..RATIO_MAX`, and on the gate's card that curve is not
    /// an illustration — it is the control, dragged to set both. A knob
    /// that reached past the axis it is drawn on would put the threshold
    /// cell and the threshold handle in disagreement, and the card's
    /// whole premise is that they cannot disagree.
    ///
    /// So the table takes the widget's figures rather than the kernel's
    /// wider ones. Nothing is lost that a gate wants: a threshold under
    /// -60 dBFS is below the noise it would be gating, and 60:1 is
    /// already a closed door — `dsp::dynamics`' own doc says past about a
    /// hundred the curve is "a gate to more digits than a float holds".
    pub const THRESHOLD_MIN_DB: f32 = crate::ui::device::dynamics::VIEW_MIN_DB;
    pub const THRESHOLD_MAX_DB: f32 = crate::ui::device::dynamics::VIEW_MAX_DB;

    /// How steeply it expands below the threshold. The display's axis —
    /// see [`THRESHOLD_MIN_DB`].
    pub const RATIO_MIN: f32 = crate::ui::device::dynamics::RATIO_MIN;
    pub const RATIO_MAX: f32 = crate::ui::device::dynamics::RATIO_MAX;

    /// How fast it OPENS, in ms. Fast at the bottom, because a gate that
    /// takes even a millisecond to open has already eaten the transient
    /// it was let through for.
    pub const ATTACK_MIN_MS: f32 = 0.05;
    pub const ATTACK_MAX_MS: f32 = 100.0;

    /// How fast it CLOSES, in ms.
    pub const RELEASE_MIN_MS: f32 = 5.0;
    pub const RELEASE_MAX_MS: f32 = 2_000.0;

    /// The most it will ever shut, in dB.
    ///
    /// A gate that closes completely is the special case, not the
    /// default: leaving a little of the room in is what makes gated drums
    /// sound gated rather than chopped. Zero range is the device switched
    /// off in all but name, which is what the bottom of the control is
    /// for — and it is exact, because a range of nothing is a gain of
    /// one.
    pub const RANGE_MAX_DB: f32 = 80.0;

    /// The detector's window, in ms. Not a knob: a gate is a decision
    /// about whether a sound has started, and a slow window blurs the one
    /// question it exists to answer. Fast enough to catch a stick, slow
    /// enough not to chatter on a waveform's own zero crossings.
    pub const WINDOW_MS: f32 = 3.0;

    /// The knee, in dB, centred on the threshold. Not a knob either —
    /// five rows is already the biggest of the small devices, and a
    /// gate's knee is a refinement rather than a decision. Soft enough
    /// that programme sitting near the threshold breathes instead of
    /// stuttering.
    pub const KNEE_DB: f32 = 6.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: THRESHOLD,
            name: "threshold",
            min: THRESHOLD_MIN_DB,
            max: THRESHOLD_MAX_DB,
            default: -40.0,
        },
        ParamDef {
            id: RATIO,
            name: "ratio",
            min: RATIO_MIN,
            max: RATIO_MAX,
            // Eight to one. Unmistakably a gate and still an EXPANDER —
            // it leans on quiet material rather than deleting it, which
            // is the setting that flatters most sources. The top of the
            // range is there for when you want the chop.
            default: 8.0,
        },
        ParamDef {
            id: ATTACK,
            name: "attack",
            min: ATTACK_MIN_MS,
            max: ATTACK_MAX_MS,
            default: 1.0,
        },
        ParamDef {
            id: RELEASE,
            name: "release",
            min: RELEASE_MIN_MS,
            max: RELEASE_MAX_MS,
            default: 150.0,
        },
        ParamDef {
            id: RANGE,
            name: "range",
            min: 0.0,
            max: RANGE_MAX_DB,
            // Sixty dB down is closed to any ear, and still short of the
            // silence that makes a gate sound like an edit.
            default: 60.0,
        },
    ];
}

pub mod phaser {
    use super::ParamDef;

    pub const AMOUNT: u32 = 0;
    pub const CENTRE: u32 = 1;
    pub const DEPTH: u32 = 2;
    pub const RATE: u32 = 3;
    pub const MIX: u32 = 4;

    /// The most sections a phaser runs.
    ///
    /// Sixteen, not the disperser's thirty-two. Past this the notches are
    /// packed closer than a sweep can separate them and the effect stops
    /// being a phaser and starts being the disperser next door — which is
    /// available, and better at it.
    pub const AMOUNT_MAX: f32 = 16.0;

    /// Where the sweep is centred.
    pub const CENTRE_MIN_HZ: f32 = 100.0;
    pub const CENTRE_MAX_HZ: f32 = 4_000.0;

    /// How far the corner travels either side of centre, in OCTAVES.
    ///
    /// Octaves and not hertz, because that is what the ear hears and what
    /// the log-mapped centre knob already speaks. A depth in hertz would
    /// mean a different sweep at every centre setting.
    pub const DEPTH_MAX_OCT: f32 = 4.0;

    /// The sweep's speed. Slow enough at the bottom to take half a minute
    /// over one pass, fast enough at the top to wobble.
    pub const RATE_MIN_HZ: f32 = 0.02;
    pub const RATE_MAX_HZ: f32 = 8.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: AMOUNT,
            name: "amount",
            min: 0.0,
            max: AMOUNT_MAX,
            // Four sections: two notches, which is the classic phaser and
            // the one everybody has heard.
            default: 4.0,
        },
        ParamDef {
            id: CENTRE,
            name: "centre",
            min: CENTRE_MIN_HZ,
            max: CENTRE_MAX_HZ,
            default: 800.0,
        },
        ParamDef {
            id: DEPTH,
            name: "depth",
            min: 0.0,
            max: DEPTH_MAX_OCT,
            // Two octaves either side. Wide enough to hear the notches
            // travel, narrow enough that they stay in the band the centre
            // knob was pointed at.
            default: 2.0,
        },
        ParamDef {
            id: RATE,
            name: "rate",
            min: RATE_MIN_HZ,
            max: RATE_MAX_HZ,
            default: 0.4,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            // HALF, and this one is not a shrug. A phaser's notch is the
            // dry and the wet cancelling, and cancellation is deepest
            // when the two are equal. Fully wet is an allpass — flat, and
            // silent as an effect. The knob's TOP is the setting that
            // does nothing here, which is the reverse of every other mix
            // in the rack and worth knowing before you reach for it.
            default: 0.5,
        },
    ];
}

pub mod tilt {
    use super::ParamDef;

    pub const TILT: u32 = 0;
    pub const PIVOT: u32 = 1;

    /// How far the see-saw leans, in dB at the HIGH extreme; the low end
    /// mirrors it.
    ///
    /// TWELVE, and not the kernel's own twenty-four. This is the one
    /// place in the rack where the device's range is deliberately
    /// narrower than the kernel's, and the reason is measured rather
    /// than tasteful — see [`PIVOT_MAX_HZ`].
    pub const TILT_MAX_DB: f32 = 12.0;

    /// Where the plank balances, and the other half of one decision.
    ///
    /// # Why both ranges stop short of the kernel's
    ///
    /// `Tilt::prepare` puts the unity crossing on the pivot by placing
    /// the one-pole's corner at `pivot × g_hi`. That correction is what
    /// makes the pivot mean anything — without it the crossing slides an
    /// octave at ±6 dB — but it walks the corner UP as the lean
    /// increases, and a bilinear corner loses accuracy as it approaches
    /// Nyquist. At a high pivot and a big positive lean the two multiply
    /// and the crossing comes off the pivot after all.
    ///
    /// Measured, at 48 kHz, as the crossing's error in dB:
    ///
    /// ```text
    ///   pivot:    200    500   1000   2000   3000   4000   8000
    ///  +24 dB:  -0.12  -0.86  -4.22 -20.82 -18.69 -16.60 -10.58
    ///  +12 dB:  -0.01  -0.07  -0.18  -0.70  -1.83  -3.55 -11.65
    ///   +6 dB:  -0.00  -0.03  -0.03  -0.09  -0.29  -0.37  -2.00
    /// ```
    ///
    /// So the ranges are drawn around the region where the promise
    /// HOLDS: ±12 dB and a pivot up to 2 kHz keeps the crossing within
    /// 0.7 dB of where the knob points, and within 0.2 dB over most of
    /// it. That is a conventional tilt anyway — most are ±6 — and the
    /// alternative was shipping two knobs that combine into a lie.
    ///
    /// The failure is not the kernel's: its doc already says a
    /// first-order see-saw is the wrong tool past its ceiling, and this
    /// is the same argument arriving one stage earlier. Nothing here
    /// clamps behind the user's back, which would be the other way to
    /// hide it and a worse one — a knob that silently stops moving is
    /// harder to diagnose than one that was never offered.
    pub const PIVOT_MIN_HZ: f32 = 100.0;
    pub const PIVOT_MAX_HZ: f32 = 2_000.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: TILT,
            name: "tilt",
            min: -TILT_MAX_DB,
            max: TILT_MAX_DB,
            // FLAT. See the module header: this one is corrective, and a
            // corrective device that opens with an opinion is a device
            // you have to argue with before you can use it.
            default: 0.0,
        },
        ParamDef {
            id: PIVOT,
            name: "pivot",
            min: PIVOT_MIN_HZ,
            max: PIVOT_MAX_HZ,
            // A kilohertz: the preamp's own figure, and where a tilt
            // pivot conventionally sits — near enough the middle of the
            // band by ear that leaning either way reads as "brighter" or
            // "darker" rather than as a bass or a treble control.
            default: 1_000.0,
        },
    ];
}

pub mod disperser {
    use super::ParamDef;

    pub const AMOUNT: u32 = 0;
    pub const FREQ: u32 = 1;
    pub const PINCH: u32 = 2;

    /// The most sections on offer, and the kernel's own ceiling —
    /// `dsp::filters::DISPERSER_MAX_STAGES`, not a second opinion about
    /// it. Restated as an f32 because a TABLE row is f32.
    pub const AMOUNT_MAX: f32 = crate::dsp::filters::DISPERSER_MAX_STAGES as f32;

    /// Where the sections are tuned. The full audible span, because the
    /// whole gesture with this device is sweeping the smear from a
    /// sub-bass boing up to a metallic tick.
    pub const FREQ_MIN_HZ: f32 = 20.0;
    pub const FREQ_MAX_HZ: f32 = 20_000.0;

    /// How tightly the phase turns at the corner — the sections' Q.
    ///
    /// Low spreads the group delay over octaves and reads as a soft
    /// smear; high packs it into a narrow band and reads as a ringing
    /// pitch. The floor stays clear of the filter module's own `MIN_Q` so
    /// no setting here lands on a clamp.
    pub const PINCH_MIN: f32 = 0.1;
    pub const PINCH_MAX: f32 = 8.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: AMOUNT,
            name: "amount",
            min: 0.0,
            max: AMOUNT_MAX,
            // Eight sections: sixteen poles of phase. Unmistakably the
            // effect and still a smeared transient rather than the
            // pitched "pew" the top of the range gives, which is a sound
            // you go looking for rather than one you want on load.
            default: 8.0,
        },
        ParamDef {
            id: FREQ,
            name: "freq",
            min: FREQ_MIN_HZ,
            max: FREQ_MAX_HZ,
            // Low-mid, where a kick's body lives — the case the kernel's
            // own doc is written around.
            default: 500.0,
        },
        ParamDef {
            id: PINCH,
            name: "pinch",
            min: PINCH_MIN,
            max: PINCH_MAX,
            // The neutral turn. Butterworth-ish, and the setting at which
            // the smear reads as a softened transient rather than as a
            // note of its own.
            default: 1.0,
        },
    ];
}

pub mod sheen {
    use super::ParamDef;

    pub const AMOUNT: u32 = 0;
    pub const EDGE: u32 = 1;
    pub const MIX: u32 = 2;
    pub const OUT: u32 = 3;

    /// The kernel's own ceiling on how much a fully-triggered lift adds.
    /// Taken from `SlewBrighten::prepare`'s clamp rather than invented
    /// here, so the top of the knob is the top of the kernel.
    pub const AMOUNT_MAX: f32 = 4.0;

    /// The edge band's corner: everything above it is what gets lifted.
    ///
    /// The floor is well clear of the kernel's own 20 Hz minimum because
    /// a brightener whose band starts in the bass is a volume knob with
    /// extra steps.
    pub const EDGE_MIN_HZ: f32 = 200.0;
    pub const EDGE_MAX_HZ: f32 = 8_000.0;

    /// The output trim's window, in dB, and the same figures as linear
    /// gain — both forms written down for the reason
    /// [`sat::OUT_MIN_DB`](super::sat::OUT_MIN_DB) gives.
    pub const OUT_MIN_DB: f32 = -24.0;
    pub const OUT_MAX_DB: f32 = 12.0;
    /// `10^(-24/20)` and `10^(12/20)`, to f32 precision.
    pub const OUT_MIN: f32 = 0.063_095_73;
    pub const OUT_MAX: f32 = 3.981_072;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: AMOUNT,
            name: "amount",
            min: 0.0,
            max: AMOUNT_MAX,
            // The figure the limiter's own brighten knob reaches at full
            // travel (`audio::limiter::BRIGHTEN_AMOUNT`). Known-good and
            // clearly audible, and well short of the kernel's ceiling —
            // a device that opens at its maximum leaves nowhere to go.
            default: 1.2,
        },
        ParamDef {
            id: EDGE,
            name: "edge",
            min: EDGE_MIN_HZ,
            max: EDGE_MAX_HZ,
            // `SlewBrighten`'s own default corner, and near the 1.8 kHz
            // the limiter picked: presence rather than air.
            default: 1_500.0,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            // Fully wet. The amount knob is already the "how much" — a
            // second one at less than full would mean the device opens
            // quieter than either control admits.
            default: 1.0,
        },
        ParamDef {
            id: OUT,
            name: "out",
            min: OUT_MIN,
            max: OUT_MAX,
            default: 1.0,
        },
    ];
}

/// HAZE — the pad synth's table.
///
/// Nineteen knobs, and the count is a decision rather than an accident.
/// The synth brief's thesis is that deep sound design should be a
/// NAVIGATION problem and not a construction one, and a pad is the case
/// where that bites hardest: the sound is slow, so every knob is a slow
/// experiment, and sixty of them is an afternoon spent finding out that
/// fifty-five did not matter.
///
/// So the axes here are the ones a pad is actually made of, and several
/// knobs drive more than one thing underneath — `GRAIN` moves bit depth
/// and sample rate together because "how lo-fi" is one question, and
/// splitting it in two would be the construction problem wearing a
/// disguise.
/// CLAMP — the surgical compressor's table.
///
/// The other compressor in the box is `glue`, and every difference here
/// is a difference from that one rather than a preference. Glue is a bus
/// compressor: three ratio detents, an auto release, a peak clipper at
/// the rails, and a character you turn on. This one is the opposite
/// device — continuous ratio, a knee you can set, an attack that reaches
/// under a tenth of a millisecond, and a colour that is deliberately
/// slight.
///
/// Two compressors is not duplication when they are these two: "hold a
/// mix together" and "stop that one syllable" are different jobs, and a
/// single device wide enough to do both is a device with a bad default
/// for each.
pub mod clamp {
    use super::ParamDef;

    pub const THRESHOLD: u32 = 0;
    pub const RATIO: u32 = 1;
    pub const KNEE: u32 = 2;
    pub const ATTACK: u32 = 3;
    pub const RELEASE: u32 = 4;
    pub const MAKEUP: u32 = 5;
    pub const SC_HP: u32 = 6;
    pub const WARMTH: u32 = 7;
    pub const MIX: u32 = 8;

    pub const THRESHOLD_MIN_DB: f32 = -60.0;
    pub const THRESHOLD_MAX_DB: f32 = 0.0;
    /// Continuous, unlike glue's three detents.
    ///
    /// A bus compressor wants a small set of known ratios because the
    /// job is repeated and the answer is usually one of three. Surgical
    /// work is the opposite: the ratio IS the decision, it changes per
    /// source, and 3.5:1 is a real answer that a detented knob cannot
    /// give.
    pub const RATIO_MIN: f32 = 1.0;
    pub const RATIO_MAX: f32 = 20.0;
    pub const KNEE_MAX_DB: f32 = 24.0;
    /// Under a tenth of a millisecond at the fast end.
    ///
    /// This is the whole point of the device. Glue's fastest is 10 µs on
    /// paper but its ballistics are program-dependent and its ratios
    /// start at 2:1 — it is built to move slowly and be unnoticed. This
    /// one is built to catch one transient and let go, so its attack has
    /// to be able to be shorter than the transient.
    pub const ATTACK_MIN_MS: f32 = 0.05;
    pub const ATTACK_MAX_MS: f32 = 50.0;
    pub const RELEASE_MIN_MS: f32 = 5.0;
    pub const RELEASE_MAX_MS: f32 = 500.0;
    pub const MAKEUP_MIN_DB: f32 = -12.0;
    pub const MAKEUP_MAX_DB: f32 = 24.0;
    pub const SC_HP_MIN_HZ: f32 = 20.0;
    pub const SC_HP_MAX_HZ: f32 = 500.0;

    /// How far the warmth knob drives the output stage.
    ///
    /// A THIRD of what the stage can do, and that ceiling is the device
    /// saying what it is. `preamp` is the same colour the sampler and
    /// the strip run, and at full amount it is a character — tilt, a
    /// second harmonic, a hiss floor. A surgical compressor that
    /// coloured that much would be a second glue with a different card,
    /// and the reason to reach for this one is that it does not change
    /// the sound it is controlling.
    ///
    /// So the knob spans this device's OWN range, top to bottom, and
    /// that range stops early. A knob that reached the same place as
    /// another device's would be the two devices being the same device.
    pub const WARMTH_CEILING: f32 = 0.33;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: THRESHOLD,
            name: "threshold",
            min: THRESHOLD_MIN_DB,
            max: THRESHOLD_MAX_DB,
            // High enough to do nothing on arrival. A compressor that
            // starts compressing is a compressor whose first act is to
            // change a sound nobody asked it to.
            default: -12.0,
        },
        ParamDef {
            id: RATIO,
            name: "ratio",
            min: RATIO_MIN,
            max: RATIO_MAX,
            default: 4.0,
        },
        ParamDef {
            id: KNEE,
            name: "knee",
            min: 0.0,
            max: KNEE_MAX_DB,
            // Soft, because a hard corner is audible as a click on
            // material sitting exactly at the threshold — which is
            // where surgical work puts it by definition.
            default: 6.0,
        },
        ParamDef {
            id: ATTACK,
            name: "attack",
            min: ATTACK_MIN_MS,
            max: ATTACK_MAX_MS,
            // Fast enough to catch a consonant, slow enough to leave a
            // drum its stick.
            default: 3.0,
        },
        ParamDef {
            id: RELEASE,
            name: "release",
            min: RELEASE_MIN_MS,
            max: RELEASE_MAX_MS,
            default: 80.0,
        },
        ParamDef {
            id: MAKEUP,
            name: "makeup",
            min: MAKEUP_MIN_DB,
            max: MAKEUP_MAX_DB,
            default: 0.0,
        },
        ParamDef {
            id: SC_HP,
            name: "sc hp",
            min: SC_HP_MIN_HZ,
            max: SC_HP_MAX_HZ,
            // Off, at the bottom of its range. Glue defaults its
            // detector deaf to bass because a mix bus always has some;
            // one source might be a bass, and deafening the detector to
            // the thing it was pointed at is the wrong default here.
            default: SC_HP_MIN_HZ,
        },
        ParamDef {
            id: WARMTH,
            name: "warmth",
            min: 0.0,
            max: 1.0,
            // A seasoning, and on by a little. Exact bypass at zero, the
            // promise every colour stage here makes.
            default: 0.25,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            // All wet. Parallel compression is a deliberate move and
            // this device's job is the direct one; the knob is here
            // because the move is worth having, not because it is the
            // default way to use a compressor.
            default: 1.0,
        },
    ];
}

/// PRISM — three-band dynamics, and where it disagrees with everyone
/// else's.
///
/// Two disagreements, and they are the reason to build a fourth
/// multiband rather than a third:
///
/// 1. **One knob per band, signed.** Every multiband gives a band a
///    threshold, a ratio, an attack and a release, and then does it
///    again for the upward direction — twenty-four decisions before a
///    sound has moved. Here a band has a THRESHOLD and an AMOUNT, and
///    the amount's SIGN chooses the direction: positive holds peaks
///    down, negative lifts the quiet up. The ratio comes off the same
///    knob, because "how much" and "how hard" are the same question
///    asked twice.
///
/// 2. **The colour is earned, not dialled.** [`HEAT`](band::HEAT) is a
///    saturation whose amount is the band's own gain reduction. A band
///    doing nothing is bit-for-bit clean; a band working hard is warm.
///    That is a claim about what compression should sound like, and it
///    is the opposite of the neutral utility every other multiband
///    aims to be.
///
/// And one thing that is physics rather than opinion: the per-band
/// envelope times cannot go faster than the band itself. See
/// [`GRIP`] and [`PERIODS_ATTACK`].
pub mod prism {
    use super::ParamDef;

    pub const LOW_X: u32 = 0;
    pub const HIGH_X: u32 = 1;
    pub const GRIP: u32 = 2;
    pub const MIX: u32 = 3;
    pub const OUTPUT: u32 = 4;

    /// How many bands, and it is three on purpose.
    ///
    /// Four is where a multiband stops being a tool you can hear and
    /// starts being one you reason about — and the third and fourth
    /// bands of a four-band are almost always doing one job between
    /// them. Three is low, middle, top: the way anyone describes a
    /// sound out loud.
    pub const BANDS: usize = 3;

    /// The first per-band id, and the stride between bands.
    ///
    /// Wire ids are flat because the engine's `set_param` is flat, and
    /// the arithmetic lives here so that a card, a node and a p-lock
    /// all derive the same id from the same two numbers.
    pub const BAND_BASE: u32 = 5;
    pub const BAND_STRIDE: u32 = 4;

    /// Offsets within one band, for [`param`].
    pub mod band {
        pub const THRESHOLD: u32 = 0;
        pub const AMOUNT: u32 = 1;
        pub const HEAT: u32 = 2;
        pub const TRIM: u32 = 3;
        pub const COUNT: u32 = 4;
    }

    /// The wire id of one band's one control.
    ///
    /// Out-of-range asks fold back into band 0 rather than colliding
    /// with a global — a bad index should be a wrong knob, never
    /// someone else's knob.
    pub const fn param(band: usize, which: u32) -> u32 {
        let band = if band < BANDS { band } else { 0 };
        let which = if which < band::COUNT { which } else { 0 };
        BAND_BASE + band as u32 * BAND_STRIDE + which
    }

    /// Which band a wire id belongs to, and which control — `None` for
    /// the globals.
    pub const fn split(param: u32) -> Option<(usize, u32)> {
        if param < BAND_BASE {
            return None;
        }
        let offset = param - BAND_BASE;
        let band = (offset / BAND_STRIDE) as usize;
        if band >= BANDS {
            return None;
        }
        Some((band, offset % BAND_STRIDE))
    }

    pub const LOW_X_MIN_HZ: f32 = 40.0;
    pub const LOW_X_MAX_HZ: f32 = 800.0;
    pub const HIGH_X_MIN_HZ: f32 = 800.0;
    pub const HIGH_X_MAX_HZ: f32 = 12_000.0;

    pub const THRESHOLD_MIN_DB: f32 = -60.0;
    pub const THRESHOLD_MAX_DB: f32 = 0.0;
    pub const TRIM_MIN_DB: f32 = -18.0;
    pub const TRIM_MAX_DB: f32 = 18.0;
    pub const OUTPUT_MIN_DB: f32 = -24.0;
    pub const OUTPUT_MAX_DB: f32 = 24.0;

    /// The steepest the AMOUNT knob gets, pushed all the way down.
    ///
    /// Ten to one and not twenty: a band of a mix is not a source, and
    /// a limiter on one third of the spectrum is a sound nobody wants
    /// by accident. The clamp is where twenty lives.
    pub const RATIO_MAX: f32 = 10.0;

    /// And pulled all the way up. Upward compression is far more
    /// dangerous than downward — it lifts noise, room and bleed with
    /// the signal — so the same knob travel buys much less of it.
    pub const UPWARD_RATIO_MAX: f32 = 3.0;

    /// However far the upward direction is pushed, it stops here.
    ///
    /// An upward compressor with no ceiling turns its own noise floor
    /// into the loudest thing in the band, and does it slowly enough
    /// that it sounds like the room rather than like a bug.
    pub const LIFT_CEILING_DB: f32 = 18.0;

    /// The knee, fixed and not on the face.
    ///
    /// Every band is crossing its threshold constantly — that is what a
    /// band of music does — and a hard corner on that is audible as
    /// grain. There is no setting of this worth the cell it would cost.
    pub const KNEE_DB: f32 = 6.0;

    /// The two ends of [`GRIP`], in milliseconds, before the band's own
    /// floor is applied.
    pub const ATTACK_SLOW_MS: f32 = 40.0;
    pub const ATTACK_FAST_MS: f32 = 0.3;
    pub const RELEASE_SLOW_MS: f32 = 500.0;
    pub const RELEASE_FAST_MS: f32 = 25.0;

    /// THE FLOOR UNDER GRIP, in cycles of the band's own lowest content.
    ///
    /// This is physics, not taste. A detector cannot measure the level
    /// of a 60 Hz tone in less than a 60 Hz cycle — ask it to and it
    /// tracks the waveform instead of the envelope, which is not fast
    /// compression but distortion with a threshold on it. So each
    /// band's attack floors at half a period of its lower corner and
    /// its release at four, and the low band is SLOWER than the high
    /// one at the same grip setting.
    ///
    /// Every multiband has this problem. Most hide it by letting you
    /// set an attack the low band cannot honour.
    pub const PERIODS_ATTACK: f32 = 0.5;
    pub const PERIODS_RELEASE: f32 = 4.0;

    /// How hard the heat curve is driven, and how much reduction counts
    /// as "working hard".
    ///
    /// The drive is fixed because HEAT is an amount, not a flavour —
    /// one curve, faded in. The dB figure is the scale of the claim: at
    /// twelve decibels of reduction a band at full heat is fully
    /// coloured, and at none it is untouched to the bit.
    pub const HEAT_DRIVE: f32 = 2.5;
    pub const HEAT_FULL_DB: f32 = 12.0;

    /// The detector's window. Short — the ballistics are the part with
    /// a knob on them.
    pub const DETECT_MS: f32 = 2.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: LOW_X,
            name: "low x",
            min: LOW_X_MIN_HZ,
            max: LOW_X_MAX_HZ,
            // Under the voice, over the kick's body: the seam most
            // mixes already have whether anyone drew it or not.
            default: 180.0,
        },
        ParamDef {
            id: HIGH_X,
            name: "high x",
            min: HIGH_X_MIN_HZ,
            max: HIGH_X_MAX_HZ,
            // Where presence stops and air starts.
            default: 2_800.0,
        },
        ParamDef {
            id: GRIP,
            name: "grip",
            min: 0.0,
            max: 1.0,
            // Middle: the times a multiband is usually set to anyway,
            // and the one place the knob says nothing about itself.
            default: 0.5,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            default: 1.0,
        },
        ParamDef {
            id: OUTPUT,
            name: "out",
            min: OUTPUT_MIN_DB,
            max: OUTPUT_MAX_DB,
            default: 0.0,
        },
        // --- low ---------------------------------------------------
        ParamDef {
            id: param(0, band::THRESHOLD),
            name: "low thresh",
            min: THRESHOLD_MIN_DB,
            max: THRESHOLD_MAX_DB,
            default: -18.0,
        },
        ParamDef {
            id: param(0, band::AMOUNT),
            name: "low amount",
            min: -1.0,
            max: 1.0,
            // ZERO. Every band opens doing nothing, which is what makes
            // the signed knob safe to have: there is no direction to
            // arrive in.
            default: 0.0,
        },
        ParamDef {
            id: param(0, band::HEAT),
            name: "low heat",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: param(0, band::TRIM),
            name: "low trim",
            min: TRIM_MIN_DB,
            max: TRIM_MAX_DB,
            default: 0.0,
        },
        // --- mid ---------------------------------------------------
        ParamDef {
            id: param(1, band::THRESHOLD),
            name: "mid thresh",
            min: THRESHOLD_MIN_DB,
            max: THRESHOLD_MAX_DB,
            default: -18.0,
        },
        ParamDef {
            id: param(1, band::AMOUNT),
            name: "mid amount",
            min: -1.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: param(1, band::HEAT),
            name: "mid heat",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: param(1, band::TRIM),
            name: "mid trim",
            min: TRIM_MIN_DB,
            max: TRIM_MAX_DB,
            default: 0.0,
        },
        // --- high --------------------------------------------------
        ParamDef {
            id: param(2, band::THRESHOLD),
            name: "high thresh",
            min: THRESHOLD_MIN_DB,
            max: THRESHOLD_MAX_DB,
            default: -18.0,
        },
        ParamDef {
            id: param(2, band::AMOUNT),
            name: "high amount",
            min: -1.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: param(2, band::HEAT),
            name: "high heat",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: param(2, band::TRIM),
            name: "high trim",
            min: TRIM_MIN_DB,
            max: TRIM_MAX_DB,
            default: 0.0,
        },
    ];
}

pub mod haze {
    use super::ParamDef;

    pub const SPREAD: u32 = 0;
    pub const SHAPE: u32 = 1;
    pub const SUB: u32 = 2;
    pub const DRIFT: u32 = 3;
    pub const CUTOFF: u32 = 4;
    pub const RESONANCE: u32 = 5;
    pub const TRACK: u32 = 6;
    pub const ENV: u32 = 7;
    pub const ATTACK: u32 = 8;
    pub const DECAY: u32 = 9;
    pub const SUSTAIN: u32 = 10;
    pub const RELEASE: u32 = 11;
    pub const FILTER_ATTACK: u32 = 12;
    pub const FILTER_DECAY: u32 = 13;
    pub const ENSEMBLE: u32 = 14;
    pub const WOW: u32 = 15;
    pub const GRAIN: u32 = 16;
    pub const WARMTH: u32 = 17;
    pub const LEVEL: u32 = 18;

    /// Voices. Two lane groups, which is where the SoA layout wants to
    /// land, and enough that a held eight-note chord can be played over
    /// without stealing from itself while the first one is still
    /// releasing — a pad releases for seconds, so the release IS the
    /// polyphony requirement.
    pub const VOICES: usize = 16;
    /// The detuned copies each note is played by.
    pub const STACK: usize = 3;

    /// The longest attack and release, in seconds.
    ///
    /// A pad synth whose attack stops at a second is not a pad synth.
    /// Eight seconds in and sixteen out is the range the string machines
    /// had, and the reason theirs went that far is that a chord change
    /// under a long release is the sound the whole instrument exists for.
    /// The SHORTEST a stage can be, in seconds.
    ///
    /// A millisecond, and not zero. The knob is logarithmic — a linear
    /// one would spend its first third between two and five seconds and
    /// cross everything short in its last few degrees — and a log scale
    /// has no zero to reach. Writing the floor into the TABLE rather
    /// than clamping it in the widget is what keeps the two ends of the
    /// range agreeing: a table saying zero and a knob unable to express
    /// it is a round trip that does not close, which is what
    /// `engine_values_survive_the_trip_through_the_knobs` found.
    pub const TIME_MIN: f32 = 0.001;
    pub const ATTACK_MAX: f32 = 8.0;
    pub const RELEASE_MAX: f32 = 16.0;

    /// Detune between the stacked copies, in cents at full spread.
    ///
    /// Fifty is wide. At the top this is an ensemble rather than a
    /// unison, which is what a pad wants — the beating between copies is
    /// the movement, and a synth that only ever detunes by five cents
    /// makes a chorus pedal necessary.
    pub const SPREAD_MAX: f32 = 50.0;

    pub const CUTOFF_MIN: f32 = 20.0;
    pub const CUTOFF_MAX: f32 = 20_000.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: SPREAD,
            name: "spread",
            min: 0.0,
            max: SPREAD_MAX,
            // Open, because the stack IS the instrument. At zero the
            // three copies are one oscillator and the whole analog
            // argument collapses to a saw.
            default: 14.0,
        },
        ParamDef {
            id: SHAPE,
            name: "shape",
            min: 0.0,
            max: 1.0,
            // Between triangle and saw rather than at either: a pad
            // wants more than a sine's nothing and less than a saw's
            // everything, and the interesting half of this knob is the
            // middle.
            default: 0.55,
        },
        ParamDef {
            id: SUB,
            name: "sub",
            min: 0.0,
            max: 1.0,
            // Present but under. A pad without weight sits on top of a
            // mix instead of under it, and the sub octave is the
            // cheapest weight there is.
            default: 0.3,
        },
        ParamDef {
            id: DRIFT,
            name: "drift",
            min: 0.0,
            max: 1.0,
            // THE ANALOG KNOB, and it defaults ON. Voices that are never
            // quite in tune with each other and never quite steady is
            // the whole of what "analog" means to an ear; a digital
            // polysynth is one where every voice is identical, and that
            // is exactly what it sounds like.
            default: 0.35,
        },
        ParamDef {
            id: CUTOFF,
            name: "cutoff",
            min: CUTOFF_MIN,
            max: CUTOFF_MAX,
            // Closed enough to be a pad on the first note. An
            // instrument that opens fully bright has thrown away the
            // gesture it is for.
            default: 2_200.0,
        },
        ParamDef {
            id: RESONANCE,
            name: "reso",
            min: 0.0,
            max: 1.0,
            // A touch, not a whistle. Resonance on a slow chord is a
            // formant; past about a third it is a sine playing over the
            // top of the pad.
            default: 0.15,
        },
        ParamDef {
            id: TRACK,
            name: "track",
            min: 0.0,
            max: 1.0,
            // Most of the way, and this is the pad-specific choice on
            // this knob. Without keytracking a wide chord has its top
            // notes filtered into nothing while the bottom ones stay
            // bright, which is how a pad turns to mud as it climbs.
            default: 0.7,
        },
        ParamDef {
            id: ENV,
            name: "env",
            min: -1.0,
            max: 1.0,
            // Opening, gently. The slow filter rise under a long attack
            // is the pad gesture; making it the default is making the
            // instrument what it says it is.
            default: 0.35,
        },
        ParamDef {
            id: ATTACK,
            name: "attack",
            min: TIME_MIN,
            max: ATTACK_MAX,
            default: 0.9,
        },
        ParamDef {
            id: DECAY,
            name: "decay",
            min: TIME_MIN,
            max: ATTACK_MAX,
            default: 1.5,
        },
        ParamDef {
            id: SUSTAIN,
            name: "sustain",
            min: 0.0,
            max: 1.0,
            // High. A pad is a held sound; a low sustain makes it a
            // plucked one, and there are other devices for that.
            default: 0.8,
        },
        ParamDef {
            id: RELEASE,
            name: "release",
            min: TIME_MIN,
            max: RELEASE_MAX,
            // Long enough that a chord change overlaps itself, which is
            // the sound the instrument exists for.
            default: 2.4,
        },
        ParamDef {
            id: FILTER_ATTACK,
            name: "f.atk",
            min: TIME_MIN,
            max: ATTACK_MAX,
            // SLOWER than the amp's, deliberately. The note arrives and
            // then opens: that lag is the whole gesture, and equal times
            // would hide it.
            default: 1.8,
        },
        ParamDef {
            id: FILTER_DECAY,
            name: "f.dec",
            min: TIME_MIN,
            max: ATTACK_MAX,
            default: 2.5,
        },
        ParamDef {
            id: ENSEMBLE,
            name: "ens",
            min: 0.0,
            max: 1.0,
            // On. The string machines this instrument is descended from
            // had the chorus wired in and no way to switch it off,
            // because without it they were thin — and the reason a
            // three-tap ensemble reads as "lush" is that it is three
            // more detunings on top of the stack's own.
            default: 0.45,
        },
        ParamDef {
            id: WOW,
            name: "wow",
            min: 0.0,
            max: 1.0,
            // A little. Wow is the tape half of the lo-fi story and the
            // half that works on slow material: a pitch that is never
            // quite still is what separates a recording from a render.
            default: 0.2,
        },
        ParamDef {
            id: GRAIN,
            name: "grain",
            min: 0.0,
            max: 1.0,
            // ONE knob over bit depth and sample rate together, because
            // "how lo-fi" is one question. Bypasses EXACTLY at zero, the
            // promise every colour stage in this codebase makes.
            default: 0.25,
        },
        ParamDef {
            id: WARMTH,
            name: "warmth",
            min: 0.0,
            max: 1.0,
            // The output stage: tilt, soft clip, a gated hiss. Exact
            // bypass at zero, same promise.
            default: 0.35,
        },
        ParamDef {
            id: LEVEL,
            name: "level",
            min: 0.0,
            max: 2.0,
            default: 0.8,
        },
    ];
}

/// Flint: the transient shaper.
///
/// Two knobs that mean what they say — how hard the STRIKE hits and how
/// much BODY follows it — over a detector that does not care how loud you
/// played. See `audio::flint` for why the gain is not neutral.
pub mod flint {
    use super::ParamDef;

    pub const STRIKE: u32 = 0;
    pub const BODY: u32 = 1;
    pub const SPLIT: u32 = 2;
    pub const COLOUR: u32 = 3;
    pub const MIX: u32 = 4;
    pub const OUT: u32 = 5;

    /// How far either half can be pushed, in dB. Eighteen is enough to
    /// rebuild a hit that was recorded badly and far enough past "tasteful"
    /// to be an effect rather than a correction.
    pub const SHAPE_MAX_DB: f32 = 18.0;

    /// The output trim, both ways round, for the reason `lofi::OUT_MIN_DB`
    /// gives: the TABLE is linear gain and the KNOB is dB, and deriving one
    /// from the other by hand is how the two ends drift apart.
    pub const OUT_MIN_DB: f32 = -24.0;
    pub const OUT_MAX_DB: f32 = 12.0;
    pub const OUT_MIN: f32 = 0.063_095_73;
    pub const OUT_MAX: f32 = 3.981_072;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: STRIKE,
            name: "strike",
            min: -SHAPE_MAX_DB,
            max: SHAPE_MAX_DB,
            // Up, and audibly. A transient shaper that arrives neutral is
            // asking the user to guess what it does; this one arrives
            // having already done it, which is the argument `lofi::TABLE`
            // makes about defaults and it applies double here.
            default: 4.5,
        },
        ParamDef {
            id: BODY,
            name: "body",
            min: -SHAPE_MAX_DB,
            max: SHAPE_MAX_DB,
            // Down a little. Up on the strike and down on the body is the
            // move people actually reach for — it tightens a loop without
            // touching its level — so it is the position the device opens
            // in rather than one the manual mentions.
            default: -1.5,
        },
        ParamDef {
            id: SPLIT,
            name: "split",
            min: crate::dsp::dynamics::SPLIT_WINDOW_MIN_MS,
            max: crate::dsp::dynamics::SPLIT_WINDOW_MAX_MS,
            // Twelve milliseconds: long enough to hold a kick's whole
            // click, short enough that a snare's body is still body.
            default: 12.0,
        },
        ParamDef {
            id: COLOUR,
            name: "colour",
            min: 0.0,
            max: 1.0,
            // On, and most of the way. The colour IS the device's opinion
            // — at zero this is a competent neutral transient shaper and
            // there are several of those. Turning it down is the choice;
            // leaving it up is the default.
            default: 0.65,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            // Fully wet. Parallel transient shaping is a real technique
            // and a rare one; the common case is in the path.
            default: 1.0,
        },
        ParamDef {
            id: OUT,
            name: "out",
            min: OUT_MIN,
            max: OUT_MAX,
            default: 1.0,
        },
    ];
}

/// Sibyl: the pitch shifter that arrives as a harmoniser.
///
/// A transpose, a formant control that is independent of it, and two
/// harmony voices measured in SCALE STEPS rather than semitones — so a
/// third stays a third whether the passage is major or minor. See
/// `audio::sibyl`.
pub mod sibyl {
    use super::ParamDef;

    pub const SHIFT: u32 = 0;
    pub const FORMANT: u32 = 1;
    pub const VOICE_A: u32 = 2;
    pub const VOICE_B: u32 = 3;
    pub const KEY: u32 = 4;
    pub const SCALE: u32 = 5;
    pub const BLEND: u32 = 6;
    pub const MIX: u32 = 7;
    pub const OUT: u32 = 8;

    /// Two octaves either way. Past that a phase vocoder is a texture
    /// rather than a transposition, and the device says so by stopping.
    pub const SHIFT_MAX_ST: f32 = 24.0;
    /// One octave of formant either way — the whole usable range of a
    /// vocal tract, and well past the whole tasteful one.
    pub const FORMANT_MAX_ST: f32 = 12.0;
    /// Scale steps a harmony voice may sit at. Seven is an octave in a
    /// seven-note scale and more than an octave in a pentatonic, which
    /// is the right kind of wrong: the step is the SCALE's, not the
    /// chromatic ladder's.
    pub const VOICE_MAX_STEPS: f32 = 7.0;

    pub const KEY_NAMES: &[&str] = &[
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    pub const SCALE_NAMES: &[&str] = &["major", "minor", "dorian", "mixo", "pent"];

    pub const OUT_MIN_DB: f32 = -24.0;
    pub const OUT_MAX_DB: f32 = 12.0;
    pub const OUT_MIN: f32 = 0.063_095_73;
    pub const OUT_MAX: f32 = 3.981_072;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: SHIFT,
            name: "shift",
            min: -SHIFT_MAX_ST,
            max: SHIFT_MAX_ST,
            // ZERO, and that is the opinion rather than the absence of
            // one. Sibyl arrives as a HARMONISER — the voice you already
            // have, joined by one you did not — and a device that opened
            // a fifth up would be a transposer that happens to harmonise.
            default: 0.0,
        },
        ParamDef {
            id: FORMANT,
            name: "formant",
            min: -FORMANT_MAX_ST,
            max: FORMANT_MAX_ST,
            // Zero means HELD, not "moves with the pitch". The envelope
            // is put back where the source had it, which is what stops
            // an octave down sounding like a monster — and it is the
            // reason this control exists as its own knob rather than as
            // a switch nobody finds.
            default: 0.0,
        },
        ParamDef {
            id: VOICE_A,
            name: "voice a",
            min: -VOICE_MAX_STEPS,
            max: VOICE_MAX_STEPS,
            // A third above, in whatever key is set. The device makes a
            // sound the moment it is added, and the sound it makes is
            // the one it is for.
            default: 2.0,
        },
        ParamDef {
            id: VOICE_B,
            name: "voice b",
            min: -VOICE_MAX_STEPS,
            max: VOICE_MAX_STEPS,
            // OFF. Two harmonies at once is a choice; one is a default.
            default: 0.0,
        },
        ParamDef {
            id: KEY,
            name: "key",
            min: 0.0,
            max: 11.0,
            default: 0.0,
        },
        ParamDef {
            id: SCALE,
            name: "scale",
            min: 0.0,
            max: 4.0,
            // MINOR. A harmoniser has to pick one, and the third that
            // surprises people in a good way is the flat one.
            default: 1.0,
        },
        ParamDef {
            id: BLEND,
            name: "blend",
            min: 0.0,
            max: 1.0,
            // The harmony stands behind the voice it is answering, not
            // beside it.
            default: 0.55,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            // Fully wet: the dry is already in the harmony, because the
            // main voice at SHIFT 0 IS the dry.
            default: 1.0,
        },
        ParamDef {
            id: OUT,
            name: "out",
            min: OUT_MIN,
            max: OUT_MAX,
            default: 1.0,
        },
    ];
}

/// Ferric: the tape looper.
///
/// A loop of tape that is ALWAYS recording, a read head the grid moves
/// around on it, and the wear that a real one would have. The groove
/// patterns live here rather than in the node so the card draws the same
/// table the engine plays — see `audio::ferric`.
pub mod ferric {
    use super::ParamDef;

    pub const SPEED: u32 = 0;
    pub const DIVISION: u32 = 1;
    pub const PATTERN: u32 = 2;
    pub const GROOVE: u32 = 3;
    pub const DRIVE: u32 = 4;
    pub const WOW: u32 = 5;
    pub const AGE: u32 = 6;
    pub const MIX: u32 = 7;
    pub const OUT: u32 = 8;

    /// An octave either way of varispeed. A tape machine's pitch control
    /// is a SPEED control, so this moves time with it — which is the
    /// whole difference between this and a pitch shifter.
    pub const SPEED_MAX_ST: f32 = 12.0;

    /// The grid the head is moved on, in beats.
    pub const DIVISION_BEATS: [f32; 6] = [
        1.0,       // 1/4
        0.5,       // 1/8
        1.0 / 3.0, // 1/8T
        0.25,      // 1/16
        1.0 / 6.0, // 1/16T
        0.125,     // 1/32
    ];
    pub const DIVISION_NAMES: &[&str] = &["1/4", "1/8", "1/8T", "1/16", "1/16T", "1/32"];

    /// How many steps a groove pattern cycles over.
    pub const PATTERN_STEPS: usize = 8;

    /// What each pattern does, as how many DIVISIONS back the read head
    /// is placed at the start of each step.
    ///
    /// Displacement, not a note list: the head is put somewhere and the
    /// tape does the rest. Zero is "wherever the write head is", which is
    /// the tape passing through — so `run` is the identity and every
    /// other pattern is a departure from it that can be measured against
    /// it.
    pub const PATTERNS: [[u8; PATTERN_STEPS]; 6] = [
        // RUN — straight through. The timbre and the speed, nothing else.
        [0, 0, 0, 0, 0, 0, 0, 0],
        // STUTTER — four steps all playing the first one's tape.
        [0, 1, 2, 3, 0, 1, 2, 3],
        // HALFTIME — every step played twice, so the bar drags at half
        // pace without the tempo moving.
        [0, 0, 1, 1, 2, 2, 3, 3],
        // SKIP — a bounce: every other step reaches two back.
        [0, 2, 0, 2, 0, 2, 0, 2],
        // REVERSE — one division back, played backwards. The pattern is
        // flat because the DIRECTION is the figure here.
        [1, 1, 1, 1, 1, 1, 1, 1],
        // DRAG — a pull-back on the last step of each half. The subtle
        // one, and the reason the device opens on it.
        [0, 0, 0, 1, 0, 0, 0, 1],
    ];
    pub const PATTERN_NAMES: &[&str] = &["run", "stutter", "half", "skip", "rev", "drag"];
    /// Which pattern plays its division backwards.
    pub const REVERSE_PATTERN: usize = 4;

    /// Where the top of the band ends up as the tape wears: fresh, then
    /// worn. Here rather than in the node because the CARD prints the
    /// figure the AGE knob is actually buying, and a second copy of these
    /// two numbers is a second answer to that question.
    pub const LOSS_FRESH_HZ: f32 = 19_000.0;
    pub const LOSS_WORN_HZ: f32 = 4_200.0;

    /// The band edge at a given age.
    pub fn top_hz(age: f32) -> f32 {
        let age = age.clamp(0.0, 1.0);
        LOSS_FRESH_HZ + (LOSS_WORN_HZ - LOSS_FRESH_HZ) * age
    }

    pub const OUT_MIN_DB: f32 = -24.0;
    pub const OUT_MAX_DB: f32 = 12.0;
    pub const OUT_MIN: f32 = 0.063_095_73;
    pub const OUT_MAX: f32 = 3.981_072;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: SPEED,
            name: "speed",
            min: -SPEED_MAX_ST,
            max: SPEED_MAX_ST,
            // Nominal. The varispeed is a thing you reach for, and a
            // looper that arrived detuned would be a looper nobody
            // trusted with a whole mix.
            default: 0.0,
        },
        ParamDef {
            id: DIVISION,
            name: "div",
            min: 0.0,
            max: (DIVISION_BEATS.len() - 1) as f32,
            // A sixteenth: small enough to be a groove, large enough to
            // still be a piece of the performance rather than a grain.
            default: 3.0,
        },
        ParamDef {
            id: PATTERN,
            name: "pattern",
            min: 0.0,
            max: (PATTERNS.len() - 1) as f32,
            // DRAG. The device has to do something when it is added, and
            // of the six this is the one that can sit under a whole take
            // without announcing itself.
            default: 5.0,
        },
        ParamDef {
            id: GROOVE,
            name: "groove",
            min: 0.0,
            max: 1.0,
            // Half. At zero the head never leaves the write position and
            // this is a tape saturator; the groove is the device.
            default: 0.5,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: 0.0,
            max: 1.0,
            default: 0.35,
        },
        ParamDef {
            id: WOW,
            name: "wow",
            min: 0.0,
            max: 1.0,
            // Present but not seasick. Enough that two passes of the same
            // bar are not identical, which is the whole reason tape
            // sounds like tape.
            default: 0.25,
        },
        ParamDef {
            id: AGE,
            name: "age",
            min: 0.0,
            max: 1.0,
            // One knob for the whole decay: the top comes off, the head
            // bump comes up, and the hiss arrives. They happen together
            // on a real reel and there is no musical reason to take them
            // apart.
            default: 0.3,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            default: 1.0,
        },
        ParamDef {
            id: OUT,
            name: "out",
            min: OUT_MIN,
            max: OUT_MAX,
            default: 1.0,
        },
    ];
}

/// Umbra: the signal's shadow.
///
/// Four MACROS over a ten-link chain. No link has a control of its own —
/// each knob reaches across several at once, and the REACH matrix says
/// which. It lives here so the card draws the same wiring the engine
/// plays. See `audio::umbra`.
pub mod umbra {
    use super::ParamDef;

    pub const DEPTH: u32 = 0;
    pub const MOTION: u32 = 1;
    pub const DECAY: u32 = 2;
    pub const COLOUR: u32 = 3;
    pub const MIX: u32 = 4;
    pub const OUT: u32 = 5;

    /// One link in the chain: what it is called, and the DEPTH at which
    /// it begins to arrive.
    pub struct Stage {
        pub name: &'static str,
        pub at: f32,
    }

    /// The chain, in the order the signal meets it.
    ///
    /// Ordered by how far each one takes the sound from what came in, so
    /// DEPTH is a journey rather than a switchboard: tone, then dirt,
    /// then time, then space, then things that were never in the
    /// recording at all.
    pub const STAGES: &[Stage] = &[
        Stage {
            name: "tilt",
            at: 0.00,
        },
        Stage {
            name: "drive",
            at: 0.08,
        },
        Stage {
            name: "grain",
            at: 0.20,
        },
        Stage {
            name: "smear",
            at: 0.32,
        },
        Stage {
            name: "sweep",
            at: 0.42,
        },
        Stage {
            name: "echo",
            at: 0.52,
        },
        Stage {
            name: "room",
            at: 0.62,
        },
        Stage {
            name: "bloom",
            at: 0.72,
        },
        Stage {
            name: "shimmer",
            at: 0.82,
        },
        Stage {
            name: "haze",
            at: 0.90,
        },
    ];

    /// The delay the chain imposes, in samples: the shimmer's frame.
    /// Constant whatever DEPTH is doing, and here rather than in the node
    /// because the CARD prints it — a device that quietly costs a
    /// twenty-first of a second should say so on its face.
    pub const LATENCY_SAMPLES: usize = 1024;

    /// How many links there are. A compile-time figure so the engine can
    /// hold the stage amounts in an ARRAY rather than a `Vec` — a
    /// `collect()` once a chunk is an allocation in the audio callback,
    /// which is the one thing the red zone will not have.
    pub const STAGE_COUNT: usize = 10;

    /// The four macros, in the order they sit on the face.
    pub const MACRO_NAMES: [&str; 4] = ["depth", "motion", "decay", "colour"];
    pub const MACRO_COUNT: usize = 4;
    /// Which knob each macro is.
    pub const MACRO_PARAMS: [u32; MACRO_COUNT] = [DEPTH, MOTION, DECAY, COLOUR];

    /// How strongly each macro reaches each link, `0..=1`.
    ///
    /// This is the device. No stage has a knob; every knob has stages,
    /// and a control that moved exactly one thing would just be that
    /// thing's parameter wearing a costume. Reading a column tells you
    /// what shapes a link; reading a row tells you what a knob will do.
    ///
    /// DEPTH reaches everything because it is the gate — it decides
    /// whether a link is in the signal at all. The other three decide
    /// what it is like once it is.
    pub const REACH: [[f32; STAGE_COUNT]; MACRO_COUNT] = [
        //     tilt drive grain smear sweep echo  room bloom shim  haze
        /* depth  */
        [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        /* motion */ [0.0, 0.0, 0.3, 0.8, 1.0, 0.4, 0.0, 0.6, 0.2, 0.9],
        /* decay  */ [0.0, 0.0, 0.0, 0.2, 0.0, 1.0, 0.9, 1.0, 0.7, 0.3],
        /* colour */ [1.0, 0.9, 0.8, 0.0, 0.6, 0.3, 0.5, 0.5, 0.4, 0.6],
    ];

    /// How much DEPTH a stage takes to arrive fully. The LAST stage's
    /// threshold plus this must not exceed 1.0, or the end of the chain
    /// can never fully engage — haze sat at 0.92 and topped out at four
    /// fifths of itself with the knob against the stop.
    ///
    /// Stages overlap, and that is the point: the chain crossfades
    /// rather than switching, so there is no position of the knob where
    /// something clicks on.
    pub const STAGE_FADE: f32 = 0.10;

    /// How far into a stage the depth has travelled, `0..=1`.
    pub fn stage_amount(depth: f32, index: usize) -> f32 {
        let Some(stage) = STAGES.get(index) else {
            return 0.0;
        };
        ((depth - stage.at) / STAGE_FADE).clamp(0.0, 1.0)
    }

    /// How hard macro `m` is currently pulling on link `s`.
    ///
    /// The macro's own position times its reach, and then GATED by
    /// whether the link is in the signal at all — a decay setting on a
    /// reverb that is not running is not doing anything, and a matrix
    /// that showed it lit would be describing the wiring rather than the
    /// sound.
    pub fn pull(values: [f32; MACRO_COUNT], depth: f32, m: usize, s: usize) -> f32 {
        let Some(reach) = REACH.get(m).and_then(|row| row.get(s)).copied() else {
            return 0.0;
        };
        let value = values.get(m).copied().unwrap_or(0.0).abs().clamp(0.0, 1.0);
        let engaged = stage_amount(depth, s);
        if m == 0 {
            engaged * reach
        } else {
            value * reach * engaged
        }
    }

    pub const OUT_MIN_DB: f32 = -24.0;
    pub const OUT_MAX_DB: f32 = 12.0;
    pub const OUT_MIN: f32 = 0.063_095_73;
    pub const OUT_MAX: f32 = 3.981_072;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: DEPTH,
            name: "depth",
            min: 0.0,
            max: 1.0,
            // Far enough in to be past the dirt and into the time, so the
            // device arrives sounding like what it is rather than like a
            // tone control somebody forgot to turn up.
            default: 0.58,
        },
        ParamDef {
            id: MOTION,
            name: "motion",
            min: 0.0,
            max: 1.0,
            // Everything that WANDERS: the filter sweep, the smear, the
            // reel's wobble, the ring under the haze. Present, because a
            // shadow that stood perfectly still would read as a copy.
            default: 0.45,
        },
        ParamDef {
            id: DECAY,
            name: "decay",
            min: 0.0,
            max: 1.0,
            // Everything that LASTS: the echo's time and feedback, the
            // room, the bloom's tail, how long the shimmer holds.
            default: 0.5,
        },
        ParamDef {
            id: COLOUR,
            name: "colour",
            min: -1.0,
            max: 1.0,
            // Everything that SHAPES: the tilt, the drive, the grain, the
            // damping of both reverbs. Bipolar, and slightly dark — a
            // shadow is darker than the thing casting it.
            default: -0.15,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            // A shadow stands BEHIND what casts it. This is the one knob
            // on the card that is deliberately not at its top.
            default: 0.4,
        },
        ParamDef {
            id: OUT,
            name: "out",
            min: OUT_MIN,
            max: OUT_MAX,
            default: 1.0,
        },
    ];
}

/// Tone: the test-signal generator.
///
/// A utility, and the one every studio has on the wall: a known signal to
/// push through a chain when you want to know what the chain is doing
/// rather than what the music is doing.
pub mod tone {
    use super::ParamDef;

    pub const SHAPE: u32 = 0;
    pub const FREQ: u32 = 1;
    pub const LEVEL: u32 = 2;
    pub const MIX: u32 = 3;

    /// The six signals worth having on tap. The four waveforms are the
    /// oscillator's own; the two noises are the noise kernel's.
    pub const SHAPE_NAMES: &[&str] = &["sine", "tri", "saw", "square", "white", "pink"];
    /// Which shapes are NOISE, and therefore have no frequency.
    pub const FIRST_NOISE: usize = 4;

    pub const FREQ_MIN: f32 = 20.0;
    pub const FREQ_MAX: f32 = 20_000.0;
    pub const LEVEL_MIN_DB: f32 = -60.0;
    pub const LEVEL_MAX_DB: f32 = 0.0;
    /// `10^(-60/20)` and `10^(0/20)`, to f32 precision.
    pub const LEVEL_MIN: f32 = 0.001;
    pub const LEVEL_MAX: f32 = 1.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: SHAPE,
            name: "shape",
            min: 0.0,
            max: (SHAPE_NAMES.len() - 1) as f32,
            default: 0.0,
        },
        ParamDef {
            id: FREQ,
            name: "freq",
            min: FREQ_MIN,
            max: FREQ_MAX,
            // A440. The number every musician can hear the rightness of,
            // and the one a tuner is checked against.
            default: 440.0,
        },
        ParamDef {
            id: LEVEL,
            name: "level",
            min: LEVEL_MIN,
            max: LEVEL_MAX,
            // −18 dBFS: the level a calibrated chain is set up around,
            // and quiet enough that plugging this into a monitor chain
            // by accident is a surprise rather than an injury.
            default: 0.125_892_54,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            // FULLY WET, and that is the point of the device: you insert
            // a tone generator to hear the tone. Turning it down blends
            // the tone under what is already there, which is the rarer
            // case and so the one you have to ask for.
            default: 1.0,
        },
    ];
}

/// LOOM: the wavetable synth.
///
/// Two morphing wavetable oscillators — the position knob walks the
/// whole table set, sine to metal, crossfading equal-power between
/// neighbours so a scan is one continuous timbre — plus noise, a
/// multimode filter and two ADSRs. The table rows are ordinary rows:
/// automation, projects and letters address them like any knob.
pub mod loom {
    use super::ParamDef;

    // wavetable osc A
    pub const A_MORPH: u32 = 0;
    pub const A_OCT: u32 = 1;
    pub const A_SEMI: u32 = 2;
    pub const A_LEVEL: u32 = 3;
    // wavetable osc B
    pub const B_MORPH: u32 = 4;
    pub const B_OCT: u32 = 5;
    pub const B_SEMI: u32 = 6;
    pub const B_LEVEL: u32 = 7;
    // noise
    pub const N_LEVEL: u32 = 8;
    pub const N_DECAY: u32 = 9;
    // filter
    pub const F_MODE: u32 = 10;
    pub const F_CUTOFF: u32 = 11;
    pub const F_RES: u32 = 12;
    pub const F_ENV: u32 = 13;
    // amp
    pub const AMP_A: u32 = 14;
    pub const AMP_D: u32 = 15;
    pub const AMP_S: u32 = 16;
    pub const AMP_R: u32 = 17;
    pub const GAIN: u32 = 18;
    pub const VEL: u32 = 19;
    // filter envelope — its OWN times, not the amp's
    pub const FENV_A: u32 = 20;
    pub const FENV_D: u32 = 21;
    pub const FENV_S: u32 = 22;
    pub const FENV_R: u32 = 23;
    // voices
    pub const V_UNISON: u32 = 24;
    pub const V_DETUNE: u32 = 25;
    pub const V_SPREAD: u32 = 26;
    pub const V_GLIDE: u32 = 27;
    // the workhorse extras: the pitch envelope (kick drop, snare crack)
    // and the internal LFO (dub sirens, vibrato)
    pub const PENV_D: u32 = 28;
    pub const PENV: u32 = 29;
    pub const LFO_RATE: u32 = 30;
    pub const LFO_PITCH: u32 = 31;

    /// Voices the synth owns, before unison multiplies what one note
    /// costs. Sixteen = two groups of eight lanes.
    pub const VOICES: usize = 16;

    pub const FILTER_MODES: &[&str] = &["lp", "hp", "bp", "notch"];
    /// Octave transpose choices, index minus `OCT_CENTER` is the octave.
    pub const OCTAVES: &[&str] = &["-4", "-3", "-2", "-1", "0", "+1", "+2", "+3", "+4"];
    pub const OCT_CENTER: u32 = 4;
    /// Unison voices per note: `voices = index + 1`.
    pub const UNISON: &[&str] = &["1", "2", "3", "4", "5", "6", "7", "8"];

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: A_MORPH,
            name: "a morph",
            min: 0.0,
            max: 1.0,
            default: 0.35,
        },
        ParamDef {
            id: A_OCT,
            name: "a octave",
            min: 0.0,
            max: (OCTAVES.len() - 1) as f32,
            default: OCT_CENTER as f32,
        },
        ParamDef {
            id: A_SEMI,
            name: "a semi",
            min: -12.0,
            max: 12.0,
            default: 0.0,
        },
        ParamDef {
            id: A_LEVEL,
            name: "a level",
            min: 0.0,
            max: 100.0,
            default: 100.0,
        },
        ParamDef {
            id: B_MORPH,
            name: "b morph",
            min: 0.0,
            max: 1.0,
            default: 0.35,
        },
        ParamDef {
            id: B_OCT,
            name: "b octave",
            min: 0.0,
            max: (OCTAVES.len() - 1) as f32,
            default: OCT_CENTER as f32,
        },
        ParamDef {
            id: B_SEMI,
            name: "b semi",
            min: -12.0,
            max: 12.0,
            default: 0.0,
        },
        ParamDef {
            id: B_LEVEL,
            name: "b level",
            min: 0.0,
            max: 100.0,
            default: 50.0,
        },
        ParamDef {
            id: N_LEVEL,
            name: "noise",
            min: 0.0,
            max: 100.0,
            default: 0.0,
        },
        ParamDef {
            id: N_DECAY,
            name: "n decay",
            min: 1.0,
            max: 2_000.0,
            default: 40.0,
        },
        ParamDef {
            id: F_MODE,
            name: "mode",
            min: 0.0,
            max: (FILTER_MODES.len() - 1) as f32,
            default: 0.0,
        },
        ParamDef {
            id: F_CUTOFF,
            name: "cutoff",
            min: 20.0,
            max: 20_000.0,
            default: 12_000.0,
        },
        ParamDef {
            id: F_RES,
            name: "res",
            min: 0.0,
            max: 100.0,
            default: 30.0,
        },
        ParamDef {
            id: F_ENV,
            name: "f env",
            min: -1.0,
            max: 1.0,
            default: 0.5,
        },
        ParamDef {
            id: AMP_A,
            name: "attack",
            min: 0.1,
            max: 5_000.0,
            default: 2.0,
        },
        ParamDef {
            id: AMP_D,
            name: "decay",
            min: 1.0,
            max: 30_000.0,
            default: 300.0,
        },
        ParamDef {
            id: AMP_S,
            name: "sustain",
            min: 0.0,
            max: 1.0,
            default: 0.7,
        },
        ParamDef {
            id: AMP_R,
            name: "release",
            min: 1.0,
            max: 30_000.0,
            default: 300.0,
        },
        ParamDef {
            id: GAIN,
            name: "gain",
            min: 0.0,
            max: 2.0,
            default: 0.5,
        },
        ParamDef {
            id: VEL,
            name: "velocity",
            min: 0.0,
            max: 100.0,
            default: 100.0,
        },
        ParamDef {
            id: FENV_A,
            name: "f attack",
            min: 0.1,
            max: 5_000.0,
            default: 2.0,
        },
        ParamDef {
            id: FENV_D,
            name: "f decay",
            min: 1.0,
            max: 30_000.0,
            default: 400.0,
        },
        ParamDef {
            id: FENV_S,
            name: "f sustain",
            min: 0.0,
            max: 1.0,
            default: 0.4,
        },
        ParamDef {
            id: FENV_R,
            name: "f release",
            min: 1.0,
            max: 30_000.0,
            default: 200.0,
        },
        ParamDef {
            id: V_UNISON,
            name: "unison",
            min: 0.0,
            max: (UNISON.len() - 1) as f32,
            default: 0.0,
        },
        ParamDef {
            id: V_DETUNE,
            name: "detune",
            min: 0.0,
            max: 100.0,
            default: 6.0,
        },
        ParamDef {
            id: V_SPREAD,
            name: "spread",
            min: 0.0,
            max: 100.0,
            default: 50.0,
        },
        ParamDef {
            id: V_GLIDE,
            name: "glide",
            // One millisecond is the floor: a glide shorter than a
            // sample is no glide, and a LOG mapping needs a positive
            // floor — log(0) is not a number a knob can show.
            min: 1.0,
            max: 2_000.0,
            default: 1.0,
        },
        ParamDef {
            id: PENV_D,
            name: "p decay",
            min: 1.0,
            max: 500.0,
            default: 1.0,
        },
        ParamDef {
            id: PENV,
            name: "p env",
            min: -48.0,
            max: 48.0,
            default: 0.0,
        },
        ParamDef {
            id: LFO_RATE,
            name: "lfo rate",
            min: 0.05,
            max: 30.0,
            default: 3.0,
        },
        ParamDef {
            id: LFO_PITCH,
            name: "lfo pitch",
            min: 0.0,
            max: 2.0,
            default: 0.0,
        },
    ];
}

/// SIGIL: the ring modulator.
///
/// The carrier frequency is chosen in octaves like any other pitched
/// thing, and the mix reaches an EXACT bypass at zero — the one promise
/// every colour stage keeps, so a sigil can sit in a chain and be
/// measured rather than merely asserted.
pub mod sigil {
    use super::ParamDef;

    pub const SHAPE: u32 = 0;
    pub const FREQ: u32 = 1;
    pub const MIX: u32 = 2;

    /// The three forms the seal can be cast in. Sine is the classic
    /// spectral mirror; triangle is the gentler one; square is the
    /// metallic one that turns every input into an anvil.
    pub const SHAPE_NAMES: &[&str] = &["sine", "tri", "square"];

    /// Below a hertz is where the seal stops being a tone and starts
    /// being a slow throb — tremolo territory, and it belongs here
    /// because a ring modulator that cannot be parked there is missing
    /// half its range.
    pub const FREQ_MIN: f32 = 0.25;
    pub const FREQ_MAX: f32 = 8_000.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: SHAPE,
            name: "shape",
            min: 0.0,
            max: (SHAPE_NAMES.len() - 1) as f32,
            default: 0.0,
        },
        ParamDef {
            id: FREQ,
            name: "freq",
            min: FREQ_MIN,
            max: FREQ_MAX,
            // A440, the same A the tone generator defaults to — a sigil
            // dropped on a track in A meets the music it finds there.
            default: 440.0,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            // Half cast: the ring is clearly there, and clearly
            // bypassable both ways from where you start.
            default: 0.5,
        },
    ];
}

/// Gauge: the measurement utility.
///
/// It changes nothing. That is its whole contract, and the reason it can
/// be left in a chain: what it reports is what was there, and what comes
/// out is what went in, bit for bit.
pub mod gauge {
    use super::ParamDef;

    pub const WINDOW: u32 = 0;
    pub const HOLD: u32 = 1;
    pub const RANGE: u32 = 2;

    /// The meter's floor options, in dB. A meter with the wrong range
    /// shows either a solid bar or nothing at all.
    pub const RANGE_NAMES: &[&str] = &["-24", "-48", "-72"];
    pub const RANGE_FLOORS: [f32; 3] = [-24.0, -48.0, -72.0];

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: WINDOW,
            name: "window",
            min: 1.0,
            max: 1_000.0,
            // 300 ms: the standard integration for a programme meter, and
            // long enough that the number stops dancing.
            default: 300.0,
        },
        ParamDef {
            id: HOLD,
            name: "hold",
            min: 0.0,
            max: 10.0,
            // Two seconds. Long enough to read a peak that went by while
            // you were looking at something else.
            default: 2.0,
        },
        ParamDef {
            id: RANGE,
            name: "range",
            min: 0.0,
            max: (RANGE_FLOORS.len() - 1) as f32,
            default: 1.0,
        },
    ];
}

/// Tine: the struck-resonator synth.
///
/// Excite a structure and let it ring. The MODE TABLES are the structure
/// — the ratios a material's partials fall on — and they live here so the
/// card can draw the same spectrum the voices sound. See `audio::tine`.
pub mod tine {
    use super::ParamDef;

    pub const MATERIAL: u32 = 0;
    pub const STRIKE: u32 = 1;
    pub const PLACE: u32 = 2;
    pub const DECAY: u32 = 3;
    pub const BODY: u32 = 4;
    pub const TONE: u32 = 5;
    pub const SPREAD: u32 = 6;
    pub const TUNE: u32 = 7;
    pub const LEVEL: u32 = 8;

    /// How many partials a voice rings on.
    pub const MODES: usize = 8;

    /// A STRING's partials: whole multiples of the fundamental. What a
    /// plucked or struck string does, and what the ear hears as "a note".
    pub const HARMONIC: [f32; MODES] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];

    /// A BAR's partials, free at both ends. Not multiples of anything —
    /// which is why a marimba or a glockenspiel has a pitch you can name
    /// and a sound you would never call a note.
    ///
    /// These are the real ratios, from the bar's own equation. They are
    /// the reason MATERIAL is one knob and not a menu: everything between
    /// a string and a bar is a position on this morph.
    pub const INHARMONIC: [f32; MODES] = [1.0, 2.756, 5.404, 8.933, 13.34, 18.64, 24.82, 31.87];

    /// Where a partial's ratio sits at a given MATERIAL, `0` string to
    /// `1` bar.
    pub fn ratio(material: f32, mode: usize) -> f32 {
        let t = material.clamp(0.0, 1.0);
        let a = HARMONIC.get(mode).copied().unwrap_or(1.0);
        let b = INHARMONIC.get(mode).copied().unwrap_or(1.0);
        a + (b - a) * t
    }

    /// How loud a partial is excited when the structure is struck at
    /// `place`, `0`..`1` along it.
    ///
    /// `|sin(pi * n * place)|` — not a curve chosen to sound nice, but
    /// what actually happens: a partial with a node where you hit it
    /// cannot be excited at all. Striking at a half kills the even
    /// partials, at a third kills every third, and at the very end
    /// excites everything, which is why picking near the bridge is
    /// bright and thin.
    pub fn strike_gain(place: f32, mode: usize) -> f32 {
        let p = place.clamp(0.0, 1.0);
        (core::f32::consts::PI * (mode + 1) as f32 * p).sin().abs()
    }

    /// Ring time at the bottom and top of DECAY, in seconds, for the
    /// first partial at the reference pitch.
    pub const RING_SHORT_S: f32 = 0.18;
    pub const RING_LONG_S: f32 = 9.0;
    /// The pitch the ring times are quoted at. A shared resonator Q makes
    /// higher notes decay faster, which is what a real bar does.
    pub const RING_REF_HZ: f32 = 220.0;

    /// How long partial `mode` rings, in seconds.
    ///
    /// Higher partials always die first, and they die FASTER on a string
    /// than on a bar — which is most of what tells wood from metal. Here
    /// rather than in the node because the card draws these lengths, and
    /// a second copy of the falloff would be a second opinion about what
    /// the instrument is made of.
    pub fn ring_seconds(material: f32, decay: f32, mode: usize) -> f32 {
        let decay = decay.clamp(0.0, 1.0);
        let base = RING_SHORT_S + (RING_LONG_S - RING_SHORT_S) * decay * decay;
        let falloff = 0.62 + 0.33 * material.clamp(0.0, 1.0);
        base * falloff.powi(mode as i32)
    }

    /// Past this, STRIKE stops being a mallet and becomes a BOW: the
    /// excitation sustains for as long as the note is held instead of
    /// being a burst.
    pub const BOW_AT: f32 = 0.78;

    pub const TUNE_MAX_ST: f32 = 24.0;
    pub const LEVEL_MIN: f32 = 0.0;
    pub const LEVEL_MAX: f32 = 2.0;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: MATERIAL,
            name: "material",
            min: 0.0,
            max: 1.0,
            // A third of the way to the bar: enough inharmonicity to
            // sound struck rather than plucked, and not so much that it
            // stops having a pitch. This is the knob the instrument is
            // for, and it opens where the instrument is most itself.
            default: 0.34,
        },
        ParamDef {
            id: STRIKE,
            name: "strike",
            min: 0.0,
            max: 1.0,
            // A firm mallet. Soft enough to have a body, hard enough to
            // have an attack.
            default: 0.45,
        },
        ParamDef {
            id: PLACE,
            name: "place",
            min: 0.02,
            max: 0.5,
            // A fifth of the way along — where a piano's hammers hit,
            // and for the reason they do: it suppresses the seventh
            // partial, which is the one that sounds sour.
            default: 0.2,
        },
        ParamDef {
            id: DECAY,
            name: "decay",
            min: 0.0,
            max: 1.0,
            default: 0.5,
        },
        ParamDef {
            id: BODY,
            name: "body",
            min: 0.0,
            max: 1.0,
            // The cabinet the thing is mounted in. Present, because an
            // unmounted resonator sounds like a test tone.
            default: 0.35,
        },
        ParamDef {
            id: TONE,
            name: "tone",
            min: -1.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: SPREAD,
            name: "spread",
            min: 0.0,
            max: 1.0,
            // Voices fanned across the field by their own index, so a
            // chord arrives as a chord rather than as a stack.
            default: 0.45,
        },
        ParamDef {
            id: TUNE,
            name: "tune",
            min: -TUNE_MAX_ST,
            max: TUNE_MAX_ST,
            default: 0.0,
        },
        ParamDef {
            id: LEVEL,
            name: "level",
            min: LEVEL_MIN,
            max: LEVEL_MAX,
            default: 0.8,
        },
    ];
}

/// LENS — the analog poly, bent.
///
/// Two oscillators, a sub and noise into a 24 dB ladder, which is the
/// classic machine exactly. What makes it this one is the LENS between
/// them: a single warp axis the whole oscillator section passes through.
///
/// # The lens
///
/// [`WARP`] is how much, [`BEND`] is which — and BEND is a morph, not a
/// menu, because the three regimes it crosses are the same operation
/// aimed at different places in the signal:
///
/// | bend | regime | what bends |
/// |---|---|---|
/// | 0.0 | phase | the phase, before the table read — self-PM |
/// | 0.5 | fold  | the amplitude, after it — a triangle folder |
/// | 1.0 | ring  | the envelope, by a sine at a ratio of the note |
///
/// All three run at every setting; the weights are a triangle over BEND,
/// so the middle of the knob is genuinely half of two regimes rather
/// than a crossfade between two rendered voices. At `WARP = 0` all three
/// depths are zero and the path is EXACTLY the unwarped one — the same
/// promise the sampler's colour stages make, and for the same reason:
/// the bend has to be measurable, not merely asserted.
///
/// [`WARP`]: WARP
/// [`BEND`]: BEND
pub mod lens {
    use super::ParamDef;

    // The tone row.
    pub const MIX: u32 = 0;
    pub const DETUNE: u32 = 1;
    pub const WIDTH: u32 = 2;
    pub const SUB: u32 = 3;
    pub const NOISE: u32 = 4;
    pub const WARP: u32 = 5;
    pub const BEND: u32 = 6;
    pub const DRIFT: u32 = 7;

    // The shape row.
    pub const CUTOFF: u32 = 8;
    pub const RESO: u32 = 9;
    pub const ENV: u32 = 10;
    pub const ATTACK: u32 = 11;
    pub const DECAY: u32 = 12;
    pub const SUSTAIN: u32 = 13;
    pub const RELEASE: u32 = 14;
    pub const ENSEMBLE: u32 = 15;
    pub const LEVEL: u32 = 16;

    /// The three things BEND crosses, in the order it crosses them.
    pub const REGIMES: [&str; 3] = ["phase", "fold", "ring"];

    /// How much of each regime is in the signal at a given BEND.
    ///
    /// A triangle per regime, centred on its own position and one full
    /// span wide, so the weights sum to one everywhere and each regime
    /// reaches its own peak alone. Shared because the card prints the
    /// mix and the voice multiplies by it: two triangles would be two
    /// opinions about what the knob is pointing at.
    pub fn regime_mix(bend: f32) -> [f32; REGIMES.len()] {
        let b = bend.clamp(0.0, 1.0) * (REGIMES.len() - 1) as f32;
        let mut out = [0.0; REGIMES.len()];
        for (i, w) in out.iter_mut().enumerate() {
            *w = (1.0 - (b - i as f32).abs()).max(0.0);
        }
        out
    }

    /// What to call where BEND is pointing.
    pub fn regime_name(bend: f32) -> &'static str {
        let mix = regime_mix(bend);
        let mut best = 0;
        for (i, w) in mix.iter().enumerate() {
            if *w > mix[best] {
                best = i;
            }
        }
        REGIMES[best]
    }

    /// How deep the self-phase-modulation goes, in turns of phase.
    ///
    /// Fed as a PM input from a sine locked to the oscillator's own
    /// pitch and phase — which is self-PM written feed-forward, so the
    /// no-feedback rule in the synth brief holds without costing the
    /// sound anything. A third of a turn is where a saw stops being a
    /// saw and starts being the hollow, formant-ish thing a phase
    /// distortion synth is known for.
    pub const PM_MAX_TURNS: f32 = 0.34;

    pub fn pm_depth(warp: f32, bend: f32) -> f32 {
        warp.clamp(0.0, 1.0) * regime_mix(bend)[0] * PM_MAX_TURNS
    }

    /// Input gain into the triangle folder. ONE at the bottom, which is
    /// the folder's exact identity — a fold that cannot be switched off
    /// is a colour you can never measure.
    pub const FOLD_MAX_DRIVE: f32 = 7.0;

    pub fn fold_drive(warp: f32, bend: f32) -> f32 {
        1.0 + (FOLD_MAX_DRIVE - 1.0) * warp.clamp(0.0, 1.0) * regime_mix(bend)[1]
    }

    /// How much of the ring modulator replaces the dry signal, `0..=1`.
    pub fn ring_depth(warp: f32, bend: f32) -> f32 {
        warp.clamp(0.0, 1.0) * regime_mix(bend)[2]
    }

    /// The ring modulator's pitch, as a ratio of the note.
    ///
    /// Not a whole number, and that is the point: a whole ratio ring-mods
    /// back onto the harmonic series and just sounds like a filter. This
    /// one lands between partials and makes the metallic, bell-adjacent
    /// clang the regime is there for.
    pub const RING_RATIO: f32 = 2.717;

    /// The duty cycle WIDTH asks for, `0.5` (square) down to a sliver.
    ///
    /// The pulse is built as the difference of two saws a fraction of a
    /// cycle apart, which is the analog trick and needs no new kernel —
    /// the phase offset IS the duty cycle.
    pub const WIDTH_MIN_DUTY: f32 = 0.06;

    pub fn duty(width: f32) -> f32 {
        0.5 - (0.5 - WIDTH_MIN_DUTY) * width.clamp(0.0, 1.0)
    }

    /// How far DRIFT pulls one voice off the others.
    ///
    /// Three destinations from one per-voice random walk, because that is
    /// what the analog fault actually was: one drifting reference per
    /// card moved everything on it together. Independent noise on three
    /// destinations sounds like three effects; this sounds like a voice.
    pub const DRIFT_CENTS: f32 = 11.0;
    pub const DRIFT_CUTOFF: f32 = 0.22;
    pub const DRIFT_WARP: f32 = 0.18;

    /// Below this, ENSEMBLE leaves the signal alone.
    ///
    /// The classic string chorus smears the bass into porridge because
    /// it choruses everything. An LR4 split at the bottom of the cello's
    /// range keeps the low end mono and solid and lets the top swim,
    /// which is what the effect was always for.
    pub const ENSEMBLE_SPLIT_HZ: f32 = 180.0;
    pub const ENSEMBLE_BASE_MS: f32 = 9.0;
    pub const ENSEMBLE_SWING_MS: f32 = 5.2;
    pub const ENSEMBLE_HZ: f32 = 0.62;

    pub const CUTOFF_MIN_HZ: f32 = 20.0;
    pub const CUTOFF_MAX_HZ: f32 = 18_000.0;
    pub const TIME_MIN_MS: f32 = 1.0;
    pub const TIME_MAX_MS: f32 = 8_000.0;
    pub const ATTACK_MIN_MS: f32 = 0.5;
    pub const ATTACK_MAX_MS: f32 = 4_000.0;
    pub const DETUNE_MAX_CENTS: f32 = 50.0;
    pub const LEVEL_MAX: f32 = 2.0;

    /// The knob positions the card draws a cycle from — natural units,
    /// exactly as the engine holds them.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Shape {
        pub mix: f32,
        pub width: f32,
        pub sub: f32,
        pub noise: f32,
        pub warp: f32,
        pub bend: f32,
    }

    /// One cycle of the voice as it leaves the lens, into `out`.
    ///
    /// THE SAME ARITHMETIC THE VOICE RUNS, in the same order: mix, then
    /// phase warp, then fold, then ring. What differs is that the engine
    /// reads band-limited tables and oversamples the folder, so this is
    /// the IDEAL shape rather than the rendered one — which is what a
    /// display should show, since the anti-aliasing is a thing the
    /// instrument does to avoid an artefact, not a thing it sounds like.
    ///
    /// Noise is deliberately absent: it has no cycle, and drawing one
    /// frozen instance of it as though it repeated would be a lie about
    /// the shape. The card states the noise level in text instead.
    pub fn cycle(shape: &Shape, out: &mut [f32]) {
        let n = out.len();
        if n == 0 {
            return;
        }
        let pm = pm_depth(shape.warp, shape.bend);
        let drive = fold_drive(shape.warp, shape.bend);
        let ring = ring_depth(shape.warp, shape.bend);
        let duty = duty(shape.width);
        let mix = shape.mix.clamp(0.0, 1.0);
        let sub = shape.sub.clamp(0.0, 1.0);
        let tau = core::f32::consts::TAU;

        for (i, s) in out.iter_mut().enumerate() {
            let t = i as f32 / n as f32;
            // The warp is on the PHASE, which is why it survives the
            // filter: it changes which harmonics exist, not how loud the
            // ones already there are.
            let p = t + pm * (tau * t).sin();
            let saw = |x: f32| {
                let f = x - x.floor();
                f * 2.0 - 1.0
            };
            // Two saws a duty cycle apart IS a pulse — the analog trick,
            // and the reason WIDTH needs no waveform of its own.
            // The difference of two saws a duty apart is ALREADY dc-free
            // and already peak-to-peak two — the offset a naive pulse
            // needs is the wrap itself. Nothing to correct.
            let pulse = saw(p) - saw(p + duty);
            let osc = saw(p) * (1.0 - mix) + pulse * mix;
            let square = if (p - p.floor()) < 0.5 { 1.0 } else { -1.0 };
            let mut v = osc + square * sub * 0.7;
            v = fold(v * drive) / drive.max(1.0).sqrt();
            let modulator = (tau * t * RING_RATIO).sin();
            *s = v * (1.0 - ring) + v * modulator * ring;
        }
    }

    /// The folder's transfer curve, stated once. The kernel's closed-form
    /// triangle, repeated here because the card cannot reach into
    /// `dsp::shaper` without crossing the layer the UI is not allowed to
    /// cross — and asserted equal to it by a test in `audio::lens`.
    pub fn fold(x: f32) -> f32 {
        let m = (x + 1.0) * 0.25;
        let t = (m - m.floor()) * 4.0;
        1.0 - (t - 2.0).abs()
    }

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            // Mostly saw with the pulse under it: the sound every classic
            // poly preset starts from.
            default: 0.35,
        },
        ParamDef {
            id: DETUNE,
            name: "detune",
            min: 0.0,
            max: DETUNE_MAX_CENTS,
            // Nine cents. Enough beating to be two oscillators, little
            // enough to still be one note.
            default: 9.0,
        },
        ParamDef {
            id: WIDTH,
            name: "width",
            min: 0.0,
            max: 1.0,
            default: 0.34,
        },
        ParamDef {
            id: SUB,
            name: "sub",
            min: 0.0,
            max: 1.0,
            default: 0.22,
        },
        ParamDef {
            id: NOISE,
            name: "noise",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: WARP,
            name: "warp",
            min: 0.0,
            max: 1.0,
            // Open with the lens actually in the path. A synth whose one
            // idea is off by default teaches nobody what it is.
            default: 0.26,
        },
        ParamDef {
            id: BEND,
            name: "bend",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
        ParamDef {
            id: DRIFT,
            name: "drift",
            min: 0.0,
            max: 1.0,
            default: 0.24,
        },
        ParamDef {
            id: CUTOFF,
            name: "cutoff",
            min: CUTOFF_MIN_HZ,
            max: CUTOFF_MAX_HZ,
            default: 2_400.0,
        },
        ParamDef {
            id: RESO,
            name: "reso",
            min: 0.0,
            max: 1.0,
            default: 0.26,
        },
        ParamDef {
            id: ENV,
            name: "env",
            min: -1.0,
            max: 1.0,
            default: 0.45,
        },
        ParamDef {
            id: ATTACK,
            name: "attack",
            min: ATTACK_MIN_MS,
            max: ATTACK_MAX_MS,
            default: 4.0,
        },
        ParamDef {
            id: DECAY,
            name: "decay",
            min: TIME_MIN_MS,
            max: TIME_MAX_MS,
            default: 900.0,
        },
        ParamDef {
            id: SUSTAIN,
            name: "sustain",
            min: 0.0,
            max: 1.0,
            default: 0.55,
        },
        ParamDef {
            id: RELEASE,
            name: "release",
            min: TIME_MIN_MS,
            max: TIME_MAX_MS,
            default: 320.0,
        },
        ParamDef {
            id: ENSEMBLE,
            name: "ensemble",
            min: 0.0,
            max: 1.0,
            default: 0.3,
        },
        ParamDef {
            id: LEVEL,
            name: "level",
            min: 0.0,
            max: LEVEL_MAX,
            default: 0.4,
        },
    ];
}

pub mod lofi {
    use super::ParamDef;

    pub const RATE: u32 = 0;
    pub const BITS: u32 = 1;
    pub const MIX: u32 = 2;
    pub const OUT: u32 = 3;

    /// The converter clock's window. The floor is the kernel's own
    /// [`RATE_MIN`](crate::dsp::lofi::RATE_MIN) — below about a kilohertz
    /// the hold period is heard as a buzz at its own pitch rather than as
    /// a texture — and the ceiling is where the hold switches off.
    pub const RATE_MAX: f32 = 48_000.0;

    /// The output trim's window, in dB, and the same figures as linear
    /// gain. Both forms are written down for the reason
    /// [`sat::OUT_MIN_DB`](super::sat::OUT_MIN_DB) gives: the TABLE is in
    /// linear gain and the KNOB is in dB, and a widget deriving one from
    /// the other by hand is how the two ends of one range drift apart.
    pub const OUT_MIN_DB: f32 = -24.0;
    pub const OUT_MAX_DB: f32 = 12.0;
    /// `10^(-24/20)` and `10^(12/20)`, to f32 precision.
    pub const OUT_MIN: f32 = 0.063_095_73;
    pub const OUT_MAX: f32 = 3.981_072;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: RATE,
            name: "rate",
            min: crate::dsp::lofi::RATE_MIN,
            max: RATE_MAX,
            // Half of CD, which is the rate the twelve-bit machines
            // people mean by "lo-fi" actually ran near. Audibly the
            // device rather than a polite nod at it: you add this
            // because you want the grain, and one that does nothing
            // until you turn a knob is the surprise the saturator's
            // table already argues against.
            default: 22_050.0,
        },
        ParamDef {
            id: BITS,
            name: "bits",
            min: crate::dsp::lofi::BITS_MIN,
            max: crate::dsp::lofi::BITS_MAX,
            // TWELVE. The number in every advert for the machines this
            // models, and far enough from the kernel's sixteen-bit off
            // switch to be heard.
            default: 12.0,
        },
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            // Fully wet. A converter is a thing signal goes THROUGH, and
            // parallel lo-fi is the special case rather than the default
            // — unlike the reverb, which parks itself out of the way
            // because a reverb nobody asked for drowns a mix.
            default: 1.0,
        },
        ParamDef {
            id: OUT,
            name: "out",
            min: OUT_MIN,
            max: OUT_MAX,
            default: 1.0,
        },
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[(&str, &[ParamDef])] = &[
        ("seq", seq::TABLE),
        ("reverb", reverb::TABLE),
        ("sine", sine::TABLE),
        ("mixer", mixer::TABLE),
        ("pan", pan::TABLE),
        ("clip", clip::TABLE),
        ("filter", filter::TABLE),
        ("sat", sat::TABLE),
        ("echo", echo::TABLE),
        ("poly", poly::TABLE),
        ("haze", haze::TABLE),
        ("clamp", clamp::TABLE),
        ("prism", prism::TABLE),
        ("eq", eq::TABLE),
        ("glue", glue::TABLE),
        ("kick", kick::TABLE),
        ("snare", snare::TABLE),
        ("tom", tom::TABLE),
        ("hat", hat::TABLE),
        ("handclap", handclap::TABLE),
        ("limiter", limiter::TABLE),
        ("lofi", lofi::TABLE),
        ("sheen", sheen::TABLE),
        ("disperser", disperser::TABLE),
        ("tilt", tilt::TABLE),
        ("phaser", phaser::TABLE),
        ("gate", gate::TABLE),
        ("strip", strip::TABLE),
        ("resyn", resyn::TABLE),
        ("acid", acid::TABLE),
    ];

    /// The invariant `def()` and every `TABLE[FOO as usize]` rely on.
    /// The bus compressor's switches: every position has a name, every
    /// name has a position, and the table's range covers exactly the
    /// list it indexes.
    ///
    /// A names list one longer than its values list is how a switch ends
    /// up with a position that reads "30 ms" and sets 10 — nothing fails
    /// to compile, and nothing sounds obviously wrong either.
    #[test]
    fn the_glue_switches_and_their_names_are_the_same_length() {
        use glue as g;
        assert_eq!(g::RATIO_VALUES.len(), g::RATIO_NAMES.len());
        assert_eq!(g::ATTACK_MS.len(), g::ATTACK_NAMES.len());
        // The release is the odd one: one more NAME than time, because
        // the last position is auto and has no time of its own.
        assert_eq!(g::RELEASE_S.len() + 1, g::RELEASE_NAMES.len());
        assert_eq!(g::RELEASE_AUTO as usize, g::RELEASE_S.len());

        // Each switch's row spans exactly its list.
        for (id, count) in [
            (g::RATIO, g::RATIO_NAMES.len()),
            (g::ATTACK, g::ATTACK_NAMES.len()),
            (g::RELEASE, g::RELEASE_NAMES.len()),
        ] {
            let def = def(g::TABLE, id);
            assert_eq!(def.min, 0.0, "{}: switches start at 0", def.name);
            assert_eq!(
                def.max,
                (count - 1) as f32,
                "{}: the row does not cover its list",
                def.name
            );
        }

        // Every position resolves, and out-of-range indices clamp to the
        // end rather than panicking or wrapping to the start.
        for i in 0..g::ATTACK_MS.len() as u32 {
            assert_eq!(g::attack_ms(i), g::ATTACK_MS[i as usize]);
        }
        assert_eq!(g::attack_ms(999), *g::ATTACK_MS.last().unwrap());
        for i in 0..g::RATIO_VALUES.len() as u32 {
            assert_eq!(g::ratio(i), g::RATIO_VALUES[i as usize]);
        }
        assert_eq!(g::ratio(999), *g::RATIO_VALUES.last().unwrap());

        // Auto is the last position and nothing before it.
        for i in 0..g::RELEASE_AUTO {
            assert!(!g::is_auto(i), "position {i} must be a time");
            assert_eq!(g::release_ms(i), g::RELEASE_S[i as usize] * 1_000.0);
        }
        assert!(g::is_auto(g::RELEASE_AUTO));
        assert!(g::is_auto(999), "past the end is still auto, never a time");

        // The knee follows the ratio, and does so MONOTONICALLY: gentler
        // ratio, wider knee. This is the unit's "one switch, two things"
        // behaviour and the display reads the same function.
        let knees: Vec<f32> = (0..g::RATIO_VALUES.len() as u32).map(g::knee_db).collect();
        assert!(
            knees.windows(2).all(|w| w[0] > w[1]),
            "a harder ratio must not have a wider knee: {knees:?}"
        );
        assert!(
            knees.iter().all(|k| *k > 0.0),
            "a zero knee is a hard corner"
        );
        assert_eq!(
            g::knee_db(999),
            *knees.last().unwrap(),
            "past the end clamps"
        );
    }

    #[test]
    fn ids_are_dense_and_equal_to_their_index() {
        for (device, table) in ALL {
            for (i, p) in table.iter().enumerate() {
                assert_eq!(p.id, i as u32, "{device}:{}", p.name);
            }
        }
    }

    #[test]
    fn names_are_unique_within_a_device() {
        for (device, table) in ALL {
            for (i, a) in table.iter().enumerate() {
                for b in &table[i + 1..] {
                    assert_ne!(a.name, b.name, "{device}");
                }
            }
        }
    }

    #[test]
    fn ranges_are_ordered_and_defaults_lie_inside_them() {
        for (device, table) in ALL {
            for p in *table {
                assert!(p.min < p.max, "{device}:{}", p.name);
                assert!(
                    (p.min..=p.max).contains(&p.default),
                    "{device}:{} default {} outside [{}, {}]",
                    p.name,
                    p.default,
                    p.min,
                    p.max
                );
            }
        }
    }

    /// The one resonance mapping: flat stays flat at any drive, drive
    /// squashes the excess by exactly 1 + DRIVE_SQUASH at full tilt, and
    /// sub-flat requests floor at flat.
    #[test]
    fn effective_q_squashes_excess_and_floors_at_flat() {
        use filter::{DRIVE_SQUASH, FLAT_Q, effective_q};
        assert_eq!(effective_q(FLAT_Q, 0.0), FLAT_Q);
        assert_eq!(effective_q(FLAT_Q, 1.0), FLAT_Q);
        assert_eq!(effective_q(0.3, 0.7), FLAT_Q);
        let clean = effective_q(8.0, 0.0) - FLAT_Q;
        let driven = effective_q(8.0, 1.0) - FLAT_Q;
        assert!((clean / driven - (1.0 + DRIVE_SQUASH)).abs() < 1e-5);
    }

    /// The `<group> <label>` split the poly card leans on: every name
    /// has a group, and what is left is short enough to sit under a knob
    /// in a well that already carries the group as its title.
    #[test]
    fn poly_names_split_into_a_group_and_a_short_label() {
        for p in poly::TABLE {
            let label = poly::label(p.name);
            assert_ne!(label, p.name, "{} has no group word", p.name);
            assert!(!label.contains(' '), "{} has a two-word label", p.name);
            assert!(!label.is_empty(), "{} has an empty label", p.name);
        }
    }

    /// The two places a poly index is not the value it means. Both are
    /// written down beside their tables; these pin the arithmetic.
    #[test]
    fn poly_indices_convert_to_what_they_mean() {
        use poly::{OCT_CENTER, OCTAVES, UNISON, octave, unison};
        assert_eq!(octave(0), -4);
        assert_eq!(octave(OCT_CENTER), 0);
        assert_eq!(octave(OCTAVES.len() as u32 - 1), 4);
        assert_eq!(unison(0), 1);
        assert_eq!(unison(UNISON.len() as u32 - 1), 8);
    }

    /// The matrix rows and their vocabularies agree: every wire's three
    /// ids exist, src/dst ranges are exactly their name lists, and depth
    /// is bipolar. A list that outgrew its range is a choice the switch
    /// offers and the engine clamps away.
    #[test]
    fn poly_wire_rows_match_their_vocabularies() {
        use poly::{MOD_DST, MOD_SRC, TABLE, WIRE_IDS};
        for (src, dst, amt) in WIRE_IDS {
            let s = def(TABLE, src);
            let d = def(TABLE, dst);
            let a = def(TABLE, amt);
            assert_eq!(s.max as usize + 1, MOD_SRC.len(), "{}", s.name);
            assert_eq!(d.max as usize + 1, MOD_DST.len(), "{}", d.name);
            assert_eq!((s.min, d.min, s.default, d.default), (0.0, 0.0, 0.0, 0.0));
            assert!(a.min < 0.0 && a.max > 0.0, "{} is not bipolar", a.name);
        }
    }

    /// The red-zone lookup: known ids clamp, unknown ids drop.
    #[test]
    fn clamp_bounds_known_ids_and_refuses_unknown_ones() {
        assert_eq!(clamp(reverb::TABLE, reverb::MIX, 2.0), Some(1.0));
        assert_eq!(clamp(reverb::TABLE, reverb::MIX, -1.0), Some(0.0));
        assert_eq!(clamp(pan::TABLE, pan::PAN, 0.3), Some(0.3));
        assert_eq!(clamp(reverb::TABLE, 99, 0.5), None);
    }

    /// THE FADE CURVE IS MONOTONIC AND HITS BOTH ENDS, at every shape.
    ///
    /// Both properties are what makes it a fade rather than a shape: one
    /// that dipped would get louder halfway through a fade out, and one
    /// that missed an endpoint would either click at the start or never
    /// reach silence at the end.
    #[test]
    fn the_fade_curve_is_monotonic_and_hits_both_endpoints() {
        use clip::Curve;
        for step in -20..=20 {
            let shape = step as f32 / 20.0;
            let curve = Curve::new(shape);
            assert_eq!(curve.at(0.0), 0.0, "shape {shape} left endpoint");
            assert!(
                (curve.at(1.0) - 1.0).abs() < 1e-5,
                "shape {shape} right endpoint: {}",
                curve.at(1.0)
            );
            let mut previous = 0.0;
            for i in 0..=200 {
                let value = curve.at(i as f32 / 200.0);
                assert!(value.is_finite(), "shape {shape} at {i} is {value}");
                assert!(
                    (-1e-6..=1.0 + 1e-6).contains(&value),
                    "shape {shape} at {i} left the unit interval: {value}"
                );
                assert!(
                    value >= previous - 1e-6,
                    "shape {shape} fell at {i}: {value} after {previous}"
                );
                previous = value;
            }
        }
    }

    /// A shape of zero is EXACTLY linear, so a curve that has never been
    /// touched cannot change a fade that already sounded right.
    #[test]
    fn a_shape_of_zero_is_exactly_linear() {
        let curve = clip::Curve::LINEAR;
        for i in 0..=100 {
            let x = i as f32 / 100.0;
            assert_eq!(curve.at(x), x);
        }
        assert_eq!(clip::Curve::default(), clip::Curve::LINEAR);
    }

    /// Opposite shapes are mirror images about the diagonal. This is the
    /// property the two-branch `k` mapping exists to keep, and the reason
    /// the obvious one-line version of it is wrong.
    #[test]
    fn opposite_shapes_are_mirror_images() {
        use clip::Curve;
        for step in 1..=18 {
            let shape = step as f32 / 20.0;
            let up = Curve::new(shape);
            let down = Curve::new(-shape);
            for i in 0..=50 {
                let x = i as f32 / 50.0;
                // Reflecting about the diagonal: y = f(x) becomes
                // x = g(y), so g(f(x)) is x again.
                let back = down.at(up.at(x));
                assert!(
                    (back - x).abs() < 1e-4,
                    "shape {shape} at {x} came back as {back}"
                );
            }
        }
    }

    /// A nonsense shape is a linear fade, not a NaN that silences a clip.
    #[test]
    fn a_nonsense_shape_falls_back_to_linear() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(clip::Curve::new(bad), clip::Curve::LINEAR);
        }
        assert_eq!(clip::Curve::new(9.0).shape, 1.0, "and out of range clamps");
        assert_eq!(clip::Curve::new(-9.0).shape, -1.0);
    }

    /// THE CHARACTER SWITCH: every position has a name, every name has a
    /// position, and the table's range covers exactly the list it
    /// indexes. The glue switches' rule, for the same reason — a names
    /// list one longer than its values is a position that reads one thing
    /// and does another, and nothing fails to compile.
    #[test]
    fn the_filter_characters_and_their_names_line_up() {
        use filter as f;
        assert_eq!(f::CHARACTERS.len(), f::CHARACTER_NAMES.len());
        assert_eq!(f::CHAR_MAX as usize + 1, f::CHARACTERS.len());
        for (i, name) in f::CHARACTER_NAMES.iter().enumerate() {
            assert_eq!(&f::character(i as u32).name, name, "position {i}");
        }
        // The row spans exactly the list.
        let def = def(f::TABLE, f::CHARACTER);
        assert_eq!(def.min, 0.0);
        assert_eq!(def.max, f::CHAR_MAX as f32);
        // A stale index still filters rather than reading past the end.
        assert_eq!(f::character(99).name, "clean");
    }

    /// THE CHARACTERS ARE ACTUALLY DIFFERENT. A switch whose positions
    /// cannot be told apart is a switch nobody moves twice — so the thing
    /// worth pinning is that they diverge, not that any one of them holds
    /// a particular number.
    #[test]
    fn the_filter_characters_do_different_things() {
        use filter as f;
        let q = 12.0;
        let driven: Vec<f32> = (0..f::CHARACTERS.len())
            .map(|i| f::effective_q_for(q, 1.0, i as u32))
            .collect();

        // Every pair differs audibly at full drive.
        for (i, a) in driven.iter().enumerate() {
            for (j, b) in driven.iter().enumerate().skip(i + 1) {
                assert!(
                    (a - b).abs() > 0.05,
                    "characters {i} and {j} squash the same: {a} vs {b}"
                );
            }
        }

        // The ordering the docs claim: an OTA holds its peak, a diode
        // gives it up first.
        let ota = f::effective_q_for(q, 1.0, f::CHAR_OTA);
        let ladder = f::effective_q_for(q, 1.0, f::CHAR_LADDER);
        let diode = f::effective_q_for(q, 1.0, f::CHAR_DIODE);
        assert!(ota > ladder, "the OTA should hold on: {ota} vs {ladder}");
        assert!(ladder > diode, "the diode should fold first");

        // CLEAN IS THE OLD BEHAVIOUR, exactly — a project saved before
        // there were characters must still sound the way it did.
        for drive in [0.0f32, 0.5, 1.0] {
            for res in [0.3f32, 1.0, 24.0] {
                assert_eq!(
                    f::effective_q_for(res, drive, f::CHAR_CLEAN),
                    f::effective_q(res, drive),
                    "clean drifted from the original at {res}/{drive}"
                );
                assert_eq!(
                    f::shaper_drive_for(drive, f::CHAR_CLEAN),
                    f::shaper_drive(drive)
                );
            }
        }

        // Nothing any character does makes the resonance non-finite or
        // takes it below flat.
        for i in 0..f::CHARACTERS.len() as u32 {
            for drive in [0.0f32, 0.5, 1.0] {
                for res in [0.05f32, f::FLAT_Q, 24.0] {
                    let q = f::effective_q_for(res, drive, i);
                    assert!(q.is_finite() && q >= f::FLAT_Q - 1e-6, "{i}: {q}");
                }
                let d = f::shaper_drive_for(drive, i);
                assert!(d >= crate::dsp::shaper::DRIVE_MIN && d <= crate::dsp::shaper::DRIVE_MAX);
            }
        }
    }

    /// THE SPREAD WIDENS AROUND THE CORNER RATHER THAN DETUNING IT. The
    /// cutoff you set stays the centre of what you hear, so turning
    /// spread up never moves the filter — it only opens it out.
    #[test]
    fn the_spread_opens_symmetrically_around_the_cutoff() {
        use filter as f;
        for cutoff in [100.0f32, 1_000.0, 8_000.0] {
            // At zero it is exactly the knob, both sides.
            assert!((f::spread_cutoff(cutoff, 0.0, -1.0) - cutoff).abs() < 1e-3);
            assert!((f::spread_cutoff(cutoff, 0.0, 1.0) - cutoff).abs() < 1e-3);

            for spread in [1.0f32, 6.0, f::SPREAD_MAX_ST] {
                let lo = f::spread_cutoff(cutoff, spread, -1.0);
                let hi = f::spread_cutoff(cutoff, spread, 1.0);
                assert!(lo < cutoff && cutoff < hi, "at {cutoff}/{spread}");
                // The GEOMETRIC mean is the corner: equal musical
                // intervals either side, which is what makes this a width
                // control rather than a detune.
                let centre = (lo * hi).sqrt();
                assert!(
                    (centre - cutoff).abs() < cutoff * 1e-3,
                    "the spread moved the corner: {centre} against {cutoff}"
                );
            }
        }

        // Both sides stay inside the table's own cutoff range, however
        // hard the corner is pushed against a rail.
        let row = def(f::TABLE, f::CUTOFF);
        for cutoff in [row.min, row.max] {
            for side in [-1.0f32, 1.0] {
                let hz = f::spread_cutoff(cutoff, f::SPREAD_MAX_ST, side);
                assert!(hz >= row.min - 1e-3 && hz <= row.max + 1e-3, "{hz}");
            }
        }
    }
}

/// The console's sections, one module each, in `crate::console::SectionKind`'s
/// order. Every id is its row's position in its table.
pub mod console {
    pub mod preamp {
        use super::super::ParamDef;

        pub const TRIM: u32 = 0;
        pub const IRON: u32 = 1;
        pub const CHARACTER: u32 = 2;
        pub const PHASE: u32 = 3;
        pub const COLOUR: u32 = 4;

        /// The two stages: a transformer-coupled class-A stage, even
        /// harmonics and a low emphasis; a push-pull op-amp stage, odd
        /// harmonics and a harder knee.
        pub const CHARACTER_IRON: u32 = 0;
        pub const CHARACTER_STEEL: u32 = 1;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Trim",
                min: -24.0,
                max: 24.0,
                default: 0.0,
            },
            ParamDef {
                id: 1,
                name: "Iron",
                min: 0.0,
                max: 100.0,
                default: 0.0,
            },
            ParamDef {
                id: 2,
                name: "Character",
                min: 0.0,
                max: 1.0,
                default: 0.0,
            },
            ParamDef {
                id: 3,
                name: "Phase",
                min: 0.0,
                max: 1.0,
                default: 0.0,
            },
            ParamDef {
                id: 4,
                name: "Colour",
                min: 0.0,
                max: 1.0,
                default: 0.0,
            },
        ];
    }

    pub mod tone {
        use super::super::ParamDef;

        pub const LO: u32 = 0;
        pub const MID: u32 = 1;
        pub const HI: u32 = 2;
        pub const MID_HZ: u32 = 3;
        pub const KILL_LO: u32 = 4;
        pub const KILL_MID: u32 = 5;
        pub const KILL_HI: u32 = 6;

        /// The desk chose the corners once: the shelves' corners and the
        /// mid's Q, and the kills' slope and depth.
        pub const LO_HZ: f32 = 100.0;
        pub const HI_HZ: f32 = 8_000.0;
        pub const SHELF_Q: f32 = 0.7;
        /// The mid's Q is proportional: broad below the knee, rising to
        /// focused at full boost, and narrower again on a cut.
        pub const Q_KNEE_DB: f32 = 3.0;
        pub const Q_BROAD: f32 = 0.5;
        pub const Q_FOCUSED: f32 = 1.2;
        pub const CUT_NARROWER: f32 = 1.5;
        /// A kill crosses over in this long, so a kill locked on a trig
        /// stutters clean and an un-kill is a small swell.
        pub const KILL_FADE_MS: f32 = 20.0;
        /// Past this much boost the boosted band is driven through the
        /// iron, harder with every dB — a big low boost on a transformer
        /// desk is thick, not clean.
        pub const IRON_FROM_DB: f32 = 6.0;
        pub const IRON_DRIVE: f32 = 2.5;
        /// A killed shelf is a 24 dB/octave cut at its corner.
        pub const KILL_ORDER: u32 = 4;
        /// A killed mid is a wide bell this deep.
        pub const KILL_MID_DB: f32 = -36.0;
        pub const KILL_MID_Q: f32 = 0.4;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Lo",
                min: -15.0,
                max: 15.0,
                default: 0.0,
            },
            ParamDef {
                id: 1,
                name: "Mid",
                min: -15.0,
                max: 15.0,
                default: 0.0,
            },
            ParamDef {
                id: 2,
                name: "Hi",
                min: -15.0,
                max: 15.0,
                default: 0.0,
            },
            ParamDef {
                id: 3,
                name: "Mid Hz",
                min: 200.0,
                max: 6000.0,
                default: 1000.0,
            },
            ParamDef {
                id: 4,
                name: "Kill Lo",
                min: 0.0,
                max: 1.0,
                default: 0.0,
            },
            ParamDef {
                id: 5,
                name: "Kill Mid",
                min: 0.0,
                max: 1.0,
                default: 0.0,
            },
            ParamDef {
                id: 6,
                name: "Kill Hi",
                min: 0.0,
                max: 1.0,
                default: 0.0,
            },
        ];
    }

    pub mod door {
        use super::super::ParamDef;

        pub const MODE: u32 = 0;
        pub const THRESHOLD: u32 = 1;
        pub const RATIO: u32 = 2;
        pub const ATTACK: u32 = 3;
        pub const HOLD: u32 = 4;
        pub const RELEASE: u32 = 5;
        pub const RANGE: u32 = 6;
        pub const KEY_HP: u32 = 7;
        pub const KEY_LP: u32 = 8;
        pub const HYSTERESIS: u32 = 9;
        pub const DIVISION: u32 = 10;
        pub const DUTY: u32 = 11;

        pub const MODE_KEY: u32 = 0;
        pub const MODE_RHYTHM: u32 = 1;
        /// The door looks ahead by this much, so a transient is never
        /// clipped by its own opening. Fixed, so the latency is fixed.
        pub const LOOKAHEAD_MS: f32 = 2.0;
        /// The detector's own ballistics: as fast as a VCA's.
        pub const DETECT_ATTACK_MS: f32 = 0.1;
        pub const DETECT_RELEASE_MS: f32 = 3.0;
        /// The key filters' slope.
        pub const KEY_ORDER: u32 = 2;
        /// A short burst releases faster than a sustained note: the
        /// release scales from this share up to one over this long open.
        pub const RELEASE_QUICK: f32 = 0.35;
        pub const RELEASE_SETTLE_MS: f32 = 200.0;
        /// The rhythm's divisions, in beats, in `DIVISION`'s order.
        pub const DIVISION_BEATS: [f32; 6] = [1.0, 0.5, 0.25, 0.125, 1.0 / 3.0, 1.0 / 6.0];

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Mode",
                min: 0.0,
                max: 1.0,
                default: 0.0,
            },
            ParamDef {
                id: 1,
                name: "Threshold",
                min: -60.0,
                max: 0.0,
                default: -40.0,
            },
            ParamDef {
                id: 2,
                name: "Ratio",
                min: 1.5,
                max: 20.0,
                default: 10.0,
            },
            ParamDef {
                id: 3,
                name: "Attack",
                min: 0.05,
                max: 100.0,
                default: 0.5,
            },
            ParamDef {
                id: 4,
                name: "Hold",
                min: 0.0,
                max: 500.0,
                default: 20.0,
            },
            ParamDef {
                id: 5,
                name: "Release",
                min: 5.0,
                max: 2000.0,
                default: 100.0,
            },
            ParamDef {
                id: 6,
                name: "Range",
                min: 0.0,
                max: 80.0,
                default: 40.0,
            },
            ParamDef {
                id: 7,
                name: "Key HP",
                min: 20.0,
                max: 2000.0,
                default: 20.0,
            },
            ParamDef {
                id: 8,
                name: "Key LP",
                min: 200.0,
                max: 20000.0,
                default: 20000.0,
            },
            ParamDef {
                id: 9,
                name: "Hysteresis",
                min: 0.0,
                max: 12.0,
                default: 3.0,
            },
            ParamDef {
                id: 10,
                name: "Division",
                min: 0.0,
                max: 5.0,
                default: 2.0,
            },
            ParamDef {
                id: 11,
                name: "Duty",
                min: 5.0,
                max: 95.0,
                default: 50.0,
            },
        ];
    }

    pub mod cut {
        use super::super::ParamDef;

        pub const HP_HZ: u32 = 0;
        pub const HP_RES: u32 = 1;
        pub const LP_HZ: u32 = 2;
        pub const LP_RES: u32 = 3;
        pub const CRUNCH: u32 = 4;

        /// The two knobs' resting ends: parked here a filter is off.
        pub const HP_OFF_HZ: f32 = 20.0;
        pub const LP_OFF_HZ: f32 = 20_000.0;
        /// The resonance's reach: from a flat 1/√2 to just past
        /// self-oscillation.
        pub const Q_MIN: f32 = 0.707;
        pub const Q_MAX: f32 = 40.0;
        /// How much the passband is pulled down at full resonance, in
        /// dB, so a resonant sweep gets a peak and not a level jump.
        pub const RES_COMPENSATION_DB: f32 = 6.0;
        /// The loop's saturation: a floor that always holds a singing
        /// loop, and the OTA's knee at full crunch.
        pub const CRUNCH_FLOOR: f32 = 0.25;
        pub const CRUNCH_DRIVE: f32 = 6.0;
        /// Past this much resonance the damping goes through zero to
        /// this, and the loop sings on its own.
        pub const SING_FROM: f32 = 0.97;
        pub const SING_DAMPING: f32 = -0.03;
        /// How much of a block's peak-hold survives into the next one,
        /// for every held figure the readout carries — the loop heat and
        /// the two resonance rings. A ring that has stopped is dark
        /// within a dozen blocks, with no wall clock anywhere.
        pub const READOUT_DECAY: f32 = 0.8;
        /// The driven band-pass state that reads as full heat: how hard
        /// the loop is into its own tanh.
        pub const HEAT_FULL: f32 = 3.0;
        /// The band-pass state magnitude — the resonance current in the
        /// loop — that reads as a fully lit ring.
        pub const RING_FULL: f32 = 1.5;
        /// What a silent block reports for a level, in dBFS: the floor
        /// for both the output level and the input peak.
        pub const SILENT_DB: f32 = -120.0;
        /// Below this peak a block is silence rather than a very quiet
        /// level, so the log is never taken of nothing.
        pub const SILENCE_PEAK: f32 = 1e-6;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "HP",
                min: 20.0,
                max: 4000.0,
                default: 20.0,
            },
            ParamDef {
                id: 1,
                name: "HP Res",
                min: 0.0,
                max: 100.0,
                default: 20.0,
            },
            ParamDef {
                id: 2,
                name: "LP",
                min: 100.0,
                max: 20000.0,
                default: 20000.0,
            },
            ParamDef {
                id: 3,
                name: "LP Res",
                min: 0.0,
                max: 100.0,
                default: 20.0,
            },
            ParamDef {
                id: 4,
                name: "Crunch",
                min: 0.0,
                max: 100.0,
                default: 35.0,
            },
        ];
    }

    pub mod hit {
        use super::super::ParamDef;

        pub const ATTACK: u32 = 0;
        pub const SUSTAIN: u32 = 1;
        pub const BRIGHT: u32 = 2;

        /// The most a lever moves its part of the sound, in dB, at full.
        pub const RANGE_DB: f32 = 12.0;
        /// How long a strike is allowed to last.
        pub const WINDOW_MS: f32 = 20.0;
        /// The tail is what has decayed from the recent peak: the quick
        /// follower against the long one.
        pub const TAIL_QUICK_MS: f32 = 60.0;
        pub const TAIL_LONG_MS: f32 = 800.0;
        /// Where the brightness starts, and how much at full.
        pub const BRIGHT_HZ: f32 = 3_000.0;
        pub const BRIGHT_AMOUNT: f32 = 0.8;
        /// The quietest linear amplitude the section will divide by: the
        /// floor under the tail's follower ratio, under the readout's
        /// edge share, and under the peak that reads as silence. Below
        /// it a ratio is noise over noise, so it reads as nothing at all.
        pub const LEVEL_FLOOR: f32 = 1e-6;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Attack",
                min: -100.0,
                max: 100.0,
                default: 0.0,
            },
            ParamDef {
                id: 1,
                name: "Sustain",
                min: -100.0,
                max: 100.0,
                default: 0.0,
            },
            ParamDef {
                id: 2,
                name: "Bright",
                min: 0.0,
                max: 100.0,
                default: 0.0,
            },
        ];
    }

    pub mod four {
        use super::super::ParamDef;

        pub const LOW_HZ: u32 = 0;
        pub const LOW_DB: u32 = 1;
        pub const LOW_SHAPE: u32 = 2;
        pub const LMF_HZ: u32 = 3;
        pub const LMF_DB: u32 = 4;
        pub const LMF_Q: u32 = 5;
        pub const HMF_HZ: u32 = 6;
        pub const HMF_DB: u32 = 7;
        pub const HMF_Q: u32 = 8;
        pub const HIGH_HZ: u32 = 9;
        pub const HIGH_DB: u32 = 10;
        pub const HIGH_SHAPE: u32 = 11;

        pub const SHELF: u32 = 0;
        pub const BELL: u32 = 1;
        /// The shelves' Q.
        pub const SHELF_Q: f32 = 0.7;
        /// The inductor bump: a passive low shelf resonates a little
        /// just inside its corner, which is why an old EQ's bottom is
        /// tight. A bell at this share of the corner, this much of the
        /// shelf's own gain, at this Q.
        pub const INDUCTOR_AT: f32 = 1.6;
        pub const INDUCTOR_SHARE: f32 = 0.28;
        pub const INDUCTOR_Q: f32 = 1.1;

        /// The readout's three zones, cut on exactly the two frequencies
        /// the card's spine is cut on: below LOW is the bottom zone,
        /// LOW to HIGH the middle, above HIGH the top.
        pub const ZONE_LOW_HZ: f32 = 200.0;
        pub const ZONE_HIGH_HZ: f32 = 2000.0;
        /// The quietest a zone reports while a block is not silent. The
        /// card's glow is already at its floor well above this, so there
        /// is nothing to be had from reporting further down.
        pub const ZONE_FLOOR_DB: f32 = -72.0;
        /// Silence: what every level in the readout rests at, and the
        /// sentinel a block with nothing in it reports.
        pub const SILENCE_DB: f32 = -120.0;
        /// A peak at or under this is silence rather than a number —
        /// it is the sentinel's own amplitude, -120 dBFS.
        pub const SILENCE_PEAK: f32 = 1e-6;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Low Hz",
                min: 30.0,
                max: 500.0,
                default: 100.0,
            },
            ParamDef {
                id: 1,
                name: "Low",
                min: -15.0,
                max: 15.0,
                default: 0.0,
            },
            ParamDef {
                id: 2,
                name: "Low Shape",
                min: 0.0,
                max: 1.0,
                default: 0.0,
            },
            ParamDef {
                id: 3,
                name: "LMF Hz",
                min: 80.0,
                max: 2000.0,
                default: 400.0,
            },
            ParamDef {
                id: 4,
                name: "LMF",
                min: -15.0,
                max: 15.0,
                default: 0.0,
            },
            ParamDef {
                id: 5,
                name: "LMF Q",
                min: 0.3,
                max: 4.0,
                default: 0.8,
            },
            ParamDef {
                id: 6,
                name: "HMF Hz",
                min: 600.0,
                max: 8000.0,
                default: 2500.0,
            },
            ParamDef {
                id: 7,
                name: "HMF",
                min: -15.0,
                max: 15.0,
                default: 0.0,
            },
            ParamDef {
                id: 8,
                name: "HMF Q",
                min: 0.3,
                max: 4.0,
                default: 0.8,
            },
            ParamDef {
                id: 9,
                name: "High Hz",
                min: 2000.0,
                max: 16000.0,
                default: 8000.0,
            },
            ParamDef {
                id: 10,
                name: "High",
                min: -15.0,
                max: 15.0,
                default: 0.0,
            },
            ParamDef {
                id: 11,
                name: "High Shape",
                min: 0.0,
                max: 1.0,
                default: 0.0,
            },
        ];
    }

    pub mod vca {
        use super::super::ParamDef;

        pub const THRESHOLD: u32 = 0;
        pub const RATIO: u32 = 1;
        pub const ATTACK: u32 = 2;
        pub const RELEASE: u32 = 3;
        pub const MAKEUP: u32 = 4;
        pub const SC_HP: u32 = 5;
        pub const MIX: u32 = 6;

        /// The bus compressor's steps, as printed on the box.
        pub const RATIO_VALUES: [f32; 3] = [2.0, 4.0, 10.0];
        pub const ATTACK_MS: [f32; 6] = [0.1, 0.3, 1.0, 3.0, 10.0, 30.0];
        /// The last position is AUTO: two time constants at once.
        pub const RELEASE_MS: [f32; 4] = [100.0, 300.0, 600.0, 1200.0];
        pub const RELEASE_AUTO: usize = 4;
        /// The knee the detector and the feedback give it, fixed.
        pub const KNEE_DB: f32 = 3.0;
        /// The detector's window: fast, a peak more than an average.
        /// The pre-high-pass key detector runs at the same window, so
        /// the two key bands differ only by the filter.
        pub const DETECT_MS: f32 = 5.0;
        /// Silence, in dBFS: where every level the readout carries
        /// rests when the section has heard nothing. The same figure
        /// `Readout::default()` uses, so a section at rest and a
        /// section that is not there read alike.
        pub const FLOOR_DB: f32 = -120.0;
        /// `FLOOR_DB` as a linear gain: the smallest reading trusted
        /// before a level is called silence, and the guard that keeps
        /// `log10` off zero in the red zone.
        pub const FLOOR_GAIN: f32 = 1e-6;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Threshold",
                min: -40.0,
                max: 0.0,
                default: -10.0,
            },
            ParamDef {
                id: 1,
                name: "Ratio",
                min: 0.0,
                max: 2.0,
                default: 1.0,
            },
            ParamDef {
                id: 2,
                name: "Attack",
                min: 0.0,
                max: 5.0,
                default: 3.0,
            },
            ParamDef {
                id: 3,
                name: "Release",
                min: 0.0,
                max: 4.0,
                default: 4.0,
            },
            ParamDef {
                id: 4,
                name: "Makeup",
                min: 0.0,
                max: 24.0,
                default: 0.0,
            },
            ParamDef {
                id: 5,
                name: "SC HP",
                min: 20.0,
                max: 500.0,
                default: 20.0,
            },
            ParamDef {
                id: 6,
                name: "Mix",
                min: 0.0,
                max: 100.0,
                default: 100.0,
            },
        ];
    }

    pub mod split {
        use super::super::ParamDef;

        pub const LOW_HZ: u32 = 0;
        pub const HIGH_HZ: u32 = 1;
        pub const LOW: u32 = 2;
        pub const MID: u32 = 3;
        pub const HIGH: u32 = 4;
        pub const LOW_DB: u32 = 5;
        pub const MID_DB: u32 = 6;
        pub const HIGH_DB: u32 = 7;

        /// The most a band's dynamics move it, in dB, at full amount.
        pub const REACH_DB: f32 = 12.0;
        /// Each band's own ballistics, fixed: the bottom slow, the top
        /// quick. Attack and release in ms, low, mid, high.
        pub const ATTACK_MS: [f32; 3] = [20.0, 10.0, 5.0];
        pub const RELEASE_MS: [f32; 3] = [200.0, 120.0, 80.0];
        /// The detector's window per band, and the long average a
        /// band is held against.
        pub const DETECT_MS: [f32; 3] = [30.0, 15.0, 8.0];
        pub const AVERAGE_MS: f32 = 1_500.0;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Low Hz",
                min: 60.0,
                max: 500.0,
                default: 150.0,
            },
            ParamDef {
                id: 1,
                name: "High Hz",
                min: 1000.0,
                max: 8000.0,
                default: 2500.0,
            },
            ParamDef {
                id: 2,
                name: "Low",
                min: -100.0,
                max: 100.0,
                default: 0.0,
            },
            ParamDef {
                id: 3,
                name: "Mid",
                min: -100.0,
                max: 100.0,
                default: 0.0,
            },
            ParamDef {
                id: 4,
                name: "High",
                min: -100.0,
                max: 100.0,
                default: 0.0,
            },
            ParamDef {
                id: 5,
                name: "Low Gain",
                min: -12.0,
                max: 12.0,
                default: 0.0,
            },
            ParamDef {
                id: 6,
                name: "Mid Gain",
                min: -12.0,
                max: 12.0,
                default: 0.0,
            },
            ParamDef {
                id: 7,
                name: "High Gain",
                min: -12.0,
                max: 12.0,
                default: 0.0,
            },
        ];
    }

    pub mod pump {
        use super::super::ParamDef;

        pub const DIVISION: u32 = 0;
        pub const DEPTH: u32 = 1;
        pub const SHAPE: u32 = 2;
        pub const HOLD: u32 = 3;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Division",
                min: 0.0,
                max: 4.0,
                default: 2.0,
            },
            ParamDef {
                id: 1,
                name: "Depth",
                min: 0.0,
                max: 100.0,
                default: 50.0,
            },
            ParamDef {
                id: 2,
                name: "Shape",
                min: 0.0,
                max: 100.0,
                default: 50.0,
            },
            ParamDef {
                id: 3,
                name: "Hold",
                min: 0.0,
                max: 100.0,
                default: 10.0,
            },
        ];
    }

    pub mod drive {
        use super::super::ParamDef;

        pub const CHARACTER: u32 = 0;
        pub const DRIVE: u32 = 1;
        pub const TILT_PRE: u32 = 2;
        pub const TILT_POST: u32 = 3;
        pub const MIX: u32 = 4;
        pub const OUT: u32 = 5;

        pub const TUBE: u32 = 0;
        pub const TAPE: u32 = 1;
        pub const TRANSISTOR: u32 = 2;
        pub const FUZZ: u32 = 3;
        pub const FOLD: u32 = 4;
        /// How hard each character drives its curve at full.
        pub const TUBE_DRIVE: f32 = 4.0;
        /// The tube's bias: a triode's asymmetry.
        pub const TUBE_BIAS: f32 = 0.45;
        pub const TAPE_DRIVE: f32 = 6.0;
        pub const TRANSISTOR_DRIVE: f32 = 8.0;
        pub const FUZZ_DRIVE: f32 = 24.0;
        pub const FOLD_DRIVE: f32 = 4.0;
        /// The tilts' pivot.
        pub const TILT_HZ: f32 = 1_000.0;
        /// Tape softens the top as it is driven: its corner falls from
        /// the first figure toward the second.
        pub const TAPE_TOP_HZ: f32 = 16_000.0;
        pub const TAPE_TOP_DRIVEN_HZ: f32 = 5_000.0;
        /// The fuzz's bias: the gate-like asymmetry of a starved fuzz.
        pub const FUZZ_BIAS: f32 = 0.3;

        /// How far up its curve the hottest sample of a block has to go
        /// to read as full heat: `|x*k|` measured against this.
        pub const HEAT_FULL: f32 = 3.0;
        /// How much of a block's held figure survives into the next one,
        /// for every live figure the readout carries — the heat, the
        /// input peak, the dirt and the top share. A tenth of a figure
        /// survives ten blocks, so a section that has gone quiet is at
        /// rest within a few dozen, with no wall clock anywhere.
        pub const READOUT_DECAY: f32 = 0.8;
        /// The quietest input the meter draws, in dBFS: below this the
        /// press is at rest and the specimen wave is at its floor.
        pub const INPUT_FLOOR_DB: f32 = -72.0;
        /// What a silent block reports for the OUTPUT level, in dBFS,
        /// and the output peak at or under which a block counts as
        /// silent.
        pub const SILENT_DB: f32 = -120.0;
        pub const SILENT_PEAK: f32 = 1e-6;
        /// The smallest RMS the dirt ratio will divide by: a dry block
        /// quieter than this is silence, not a denominator.
        pub const DIRT_FLOOR: f32 = 1e-6;
        /// The smallest block energy the meters will divide by or take a
        /// logarithm of, so silence reads as rest instead of as a ratio
        /// of two nothings.
        pub const ENERGY_FLOOR: f32 = 1e-12;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Character",
                min: 0.0,
                max: 4.0,
                default: 0.0,
            },
            ParamDef {
                id: 1,
                name: "Drive",
                min: 0.0,
                max: 100.0,
                default: 0.0,
            },
            ParamDef {
                id: 2,
                name: "Tilt Pre",
                min: -6.0,
                max: 6.0,
                default: 0.0,
            },
            ParamDef {
                id: 3,
                name: "Tilt Post",
                min: -6.0,
                max: 6.0,
                default: 0.0,
            },
            ParamDef {
                id: 4,
                name: "Mix",
                min: 0.0,
                max: 100.0,
                default: 100.0,
            },
            ParamDef {
                id: 5,
                name: "Out",
                min: -24.0,
                max: 12.0,
                default: 0.0,
            },
        ];
    }

    pub mod grit {
        use super::super::ParamDef;

        pub const RATE: u32 = 0;
        pub const BITS: u32 = 1;
        pub const JITTER: u32 = 2;
        pub const HISS: u32 = 3;
        pub const POST: u32 = 4;
        pub const MIX: u32 = 5;

        /// The converter is off at the top of its range and the post
        /// filter is off at the top of its.
        pub const RATE_OFF_HZ: f32 = 48_000.0;
        pub const POST_OFF_HZ: f32 = 20_000.0;
        /// Jitter at full wobbles the clock by this share of the rate.
        pub const JITTER_DEPTH: f32 = 0.35;
        /// Hiss at full, in dBFS.
        pub const HISS_DB: f32 = -30.0;
        /// The post filter's slope: enough to take the images off.
        pub const POST_ORDER: u32 = 4;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Rate",
                min: 1000.0,
                max: 48000.0,
                default: 48000.0,
            },
            ParamDef {
                id: 1,
                name: "Bits",
                min: 2.0,
                max: 16.0,
                default: 16.0,
            },
            ParamDef {
                id: 2,
                name: "Jitter",
                min: 0.0,
                max: 100.0,
                default: 0.0,
            },
            ParamDef {
                id: 3,
                name: "Hiss",
                min: 0.0,
                max: 100.0,
                default: 0.0,
            },
            ParamDef {
                id: 4,
                name: "Post LP",
                min: 200.0,
                max: 20000.0,
                default: 20000.0,
            },
            ParamDef {
                id: 5,
                name: "Mix",
                min: 0.0,
                max: 100.0,
                default: 100.0,
            },
        ];
    }

    pub mod shine {
        use super::super::ParamDef;

        pub const AMOUNT: u32 = 0;
        pub const TUNE: u32 = 1;
        pub const MIX: u32 = 2;

        /// The top the exciter works on is taken off with this slope.
        pub const SPLIT_ORDER: u32 = 2;
        /// How hard the top is driven at full amount, and the bias that
        /// makes the exciter's even harmonics.
        pub const DRIVE: f32 = 8.0;
        pub const BIAS: f32 = 0.35;
        /// What the generated top is added back at, at full amount.
        pub const RETURN: f32 = 0.7;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Amount",
                min: 0.0,
                max: 100.0,
                default: 0.0,
            },
            ParamDef {
                id: 1,
                name: "Tune",
                min: 1000.0,
                max: 12000.0,
                default: 4000.0,
            },
            ParamDef {
                id: 2,
                name: "Mix",
                min: 0.0,
                max: 100.0,
                default: 50.0,
            },
        ];
    }

    pub mod drift {
        use super::super::ParamDef;

        pub const MODE: u32 = 0;
        pub const RATE: u32 = 1;
        pub const DEPTH: u32 = 2;
        pub const FEEDBACK: u32 = 3;
        pub const WIDTH: u32 = 4;
        pub const MIX: u32 = 5;

        pub const CHORUS: u32 = 0;
        pub const FLANGER: u32 = 1;
        pub const VIBRATO: u32 = 2;
        pub const ENSEMBLE: u32 = 3;
        /// Each mode's line: the delay it sits at and how far the sweep
        /// reaches at full depth, in ms, chorus, flanger, vibrato,
        /// ensemble.
        pub const BASE_MS: [f32; 4] = [7.0, 1.0, 4.0, 6.0];
        pub const SWEEP_MS: [f32; 4] = [4.0, 3.5, 3.0, 3.0];
        /// The longest line any mode needs, in ms.
        pub const MAX_MS: f32 = 16.0;
        /// The bucket brigade's band: its clock's anti-alias filters,
        /// duller the longer the line, in Hz for the short line and the
        /// long one.
        pub const BBD_SHORT_HZ: f32 = 12_000.0;
        pub const BBD_LONG_HZ: f32 = 8_000.0;
        /// The brigade's floor and its knee: a little hiss and a soft
        /// top, both fixed.
        pub const BBD_NOISE_DB: f32 = -72.0;
        pub const BBD_KNEE: f32 = 0.6;
        /// The sweep is not a metronome: a slow random walk wobbles the
        /// rate and the depth by these shares.
        pub const DRIFT_HZ: f32 = 0.3;
        pub const DRIFT_RATE: f32 = 0.06;
        pub const DRIFT_DEPTH: f32 = 0.05;
        /// The ensemble's second, fast sweep: its rate in Hz and its
        /// reach in ms.
        pub const ENSEMBLE_FAST_HZ: f32 = 5.7;
        pub const ENSEMBLE_FAST_MS: f32 = 0.35;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Mode",
                min: 0.0,
                max: 3.0,
                default: 0.0,
            },
            ParamDef {
                id: 1,
                name: "Rate",
                min: 0.05,
                max: 10.0,
                default: 0.5,
            },
            ParamDef {
                id: 2,
                name: "Depth",
                min: 0.0,
                max: 100.0,
                default: 40.0,
            },
            ParamDef {
                id: 3,
                name: "Feedback",
                min: -90.0,
                max: 90.0,
                default: 0.0,
            },
            ParamDef {
                id: 4,
                name: "Width",
                min: 0.0,
                max: 100.0,
                default: 100.0,
            },
            ParamDef {
                id: 5,
                name: "Mix",
                min: 0.0,
                max: 100.0,
                default: 0.0,
            },
        ];
    }

    pub mod phase {
        use super::super::ParamDef;

        pub const STAGES: u32 = 0;
        pub const RATE: u32 = 1;
        pub const DEPTH: u32 = 2;
        pub const FEEDBACK: u32 = 3;
        pub const OFFSET: u32 = 4;

        /// How many allpass sections each position runs.
        pub const STAGE_COUNTS: [u32; 6] = [2, 4, 6, 8, 12, 16];
        /// The sweep's ends, in Hz: the notches walk between them.
        pub const LOW_HZ: f32 = 200.0;
        pub const HIGH_HZ: f32 = 6_000.0;
        /// The allpass sections' Q — the phaser's resonance.
        pub const STAGE_Q: f32 = 0.7;
        /// The feedback path's soft top, so a fed-back phaser sings.
        pub const FEEDBACK_KNEE: f32 = 0.8;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Stages",
                min: 0.0,
                max: 5.0,
                default: 0.0,
            },
            ParamDef {
                id: 1,
                name: "Rate",
                min: 0.05,
                max: 10.0,
                default: 0.3,
            },
            ParamDef {
                id: 2,
                name: "Depth",
                min: 0.0,
                max: 100.0,
                default: 0.0,
            },
            ParamDef {
                id: 3,
                name: "Feedback",
                min: -90.0,
                max: 90.0,
                default: 0.0,
            },
            ParamDef {
                id: 4,
                name: "Offset",
                min: 0.0,
                max: 180.0,
                default: 90.0,
            },
        ];
    }

    pub mod smear {
        use super::super::ParamDef;

        pub const AMOUNT: u32 = 0;
        pub const CENTRE: u32 = 1;

        /// The allpass sections' Q: high, so each one's phase turns
        /// sharply and the smear is a chirp rather than a wash.
        pub const STAGE_Q: f32 = 1.2;

        /// The onset clock's FAST envelope, in ms: a one-pole on the
        /// input peak, quick enough to ride a transient's leading edge.
        pub const ONSET_FAST_MS: f32 = 1.0;

        /// The onset clock's SLOW envelope, in ms: the running bed the
        /// fast one is measured against.
        pub const ONSET_SLOW_MS: f32 = 120.0;

        /// How far the fast envelope must stand over the slow one, as a
        /// ratio, for the sample to count as an onset.
        pub const ONSET_RATIO: f32 = 1.6;

        /// The quietest peak that may start an onset, linear: below it
        /// the ratio is only noise arguing with noise.
        pub const ONSET_FLOOR: f32 = 1e-3;

        /// A floor on the slow envelope where it is used as a DIVISOR,
        /// linear, so the strength figure stays finite out of silence.
        pub const ONSET_BED_FLOOR: f32 = 1e-9;

        /// The retrigger lockout in ms: no second onset inside it, so
        /// one hit sends one wavefront rather than a burst of them.
        pub const ONSET_LOCKOUT_MS: f32 = 30.0;

        /// How many dB of fast-over-slow reads as a full-strength hit,
        /// for the 0..1 onset strength the card lights bars with.
        pub const ONSET_FULL_DB: f32 = 12.0;

        /// How long an onset stays IN FLIGHT, in ms. Past it the clock
        /// reads exactly 0.0 again, which is the card's "nothing
        /// crawling across the field" — longer than the longest smear
        /// the section can make (244 ms at 32 stages, 100 Hz).
        pub const ONSET_EXPIRE_MS: f32 = 1000.0;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Amount",
                min: 0.0,
                max: 32.0,
                default: 0.0,
            },
            ParamDef {
                id: 1,
                name: "Centre",
                min: 100.0,
                max: 8000.0,
                default: 1000.0,
            },
        ];
    }

    pub mod ring {
        use super::super::ParamDef;

        pub const CARRIER: u32 = 0;
        pub const HZ: u32 = 1;
        pub const HOLD_RATE: u32 = 2;
        pub const MIX: u32 = 3;

        pub const SINE: u32 = 0;
        pub const TRIANGLE: u32 = 1;
        pub const SQUARE: u32 = 2;
        pub const NOISE: u32 = 3;
        /// How far the sample-and-hold walks the carrier, in octaves.
        pub const HOLD_OCTAVES: f32 = 1.5;
        /// The noise carrier's band: a low-pass, so it is a hiss
        /// modulator and not a bit crusher.
        pub const NOISE_HZ: f32 = 3_000.0;
        /// The level meter's floor, in dBFS: what a block quieter than
        /// [`LEVEL_SILENCE`] reads as, instead of minus infinity.
        pub const LEVEL_FLOOR_DB: f32 = -120.0;
        /// The peak below which a block is silence rather than a level.
        pub const LEVEL_SILENCE: f32 = 1e-6;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Carrier",
                min: 0.0,
                max: 3.0,
                default: 0.0,
            },
            ParamDef {
                id: 1,
                name: "Hz",
                min: 20.0,
                max: 5000.0,
                default: 440.0,
            },
            ParamDef {
                id: 2,
                name: "Hold",
                min: 0.0,
                max: 50.0,
                default: 0.0,
            },
            ParamDef {
                id: 3,
                name: "Mix",
                min: 0.0,
                max: 100.0,
                default: 0.0,
            },
        ];
    }

    pub mod spectra {
        use super::super::ParamDef;

        pub const MODE: u32 = 0;
        pub const FREEZE: u32 = 1;
        pub const BLUR: u32 = 2;
        pub const PITCH: u32 = 3;
        pub const VOICE_A: u32 = 4;
        pub const VOICE_B: u32 = 5;
        pub const MIX: u32 = 6;

        pub const MODE_FREEZE: u32 = 0;
        pub const MODE_BLUR: u32 = 1;
        pub const MODE_PITCH: u32 = 2;
        pub const MODE_CHOIR: u32 = 3;
        pub const MODE_ROBOT: u32 = 4;
        /// The transform: a window long enough to hold a low note and
        /// a quarter-window hop, which is the usual bargain between
        /// smearing and cost.
        pub const SIZE: usize = 1_024;
        pub const HOP: usize = 256;
        /// Blur at full holds this share of the last frame's shape,
        /// per frame — so a blurred sound arrives late and leaves late.
        pub const BLUR_HOLD: f32 = 0.94;
        /// The readout's one-pole, in seconds: how long the card's
        /// centroid, spread and flux take to arrive at a new value.
        /// Short enough to follow a phrase, long enough not to strobe
        /// at the frame rate, which is one hop.
        pub const READOUT_TAU_S: f32 = 0.060;
        /// The floor under the readout's dB figures, in dBFS: what the
        /// analysed frame's peak reads when nothing arrived.
        pub const READOUT_FLOOR_DB: f32 = -120.0;
        /// How much magnitude a frame must carry before its centroid
        /// and spread mean anything. Under it the card is told 0
        /// rather than the ratio of two roundings.
        pub const MOMENT_FLOOR: f32 = 1e-6;
        /// The guard under the flux's denominator, so a silent frame
        /// divides by something.
        pub const FLUX_EPS: f32 = 1e-9;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Mode",
                min: 0.0,
                max: 4.0,
                default: 0.0,
            },
            ParamDef {
                id: 1,
                name: "Freeze",
                min: 0.0,
                max: 1.0,
                default: 0.0,
            },
            ParamDef {
                id: 2,
                name: "Blur",
                min: 0.0,
                max: 100.0,
                default: 0.0,
            },
            ParamDef {
                id: 3,
                name: "Pitch",
                min: -24.0,
                max: 24.0,
                default: 0.0,
            },
            ParamDef {
                id: 4,
                name: "Voice A",
                min: -12.0,
                max: 12.0,
                default: 4.0,
            },
            ParamDef {
                id: 5,
                name: "Voice B",
                min: -12.0,
                max: 12.0,
                default: 7.0,
            },
            ParamDef {
                id: 6,
                name: "Mix",
                min: 0.0,
                max: 100.0,
                default: 0.0,
            },
        ];
    }

    pub mod echo {
        use super::super::ParamDef;

        pub const SYNC: u32 = 0;
        pub const TIME: u32 = 1;
        pub const FEEDBACK: u32 = 2;
        pub const TONE: u32 = 3;
        pub const WOW: u32 = 4;
        pub const PINGPONG: u32 = 5;
        pub const MIX: u32 = 6;

        pub const FREE: u32 = 0;
        /// The synced divisions, in beats, in SYNC's order past FREE.
        pub const SYNC_BEATS: [f32; 5] = [0.25, 0.5, 0.75, 1.0, 2.0];
        /// The longest echo the line holds, in ms.
        pub const MAX_MS: f32 = 2_000.0;
        /// The loop's soft top: an analogue delay's repeats round off
        /// rather than clip, which is why a hot feedback growls.
        pub const LOOP_DRIVE: f32 = 0.35;
        /// The wow: how far the head wanders at full, as a share of the
        /// time, and how fast it wanders.
        pub const WOW_DEPTH: f32 = 0.02;
        pub const WOW_HZ: f32 = 0.6;
        /// How long the time takes to reach a new setting: an analogue
        /// delay glides rather than jumping, so a turn is a swoop.
        pub const GLIDE_MS: f32 = 120.0;
        /// The linear peak inside the loop under which the soft top's
        /// compression is not reported. Below it `soft(x)/x` is 1.0 to
        /// the last bit and the logarithm is dividing noise by noise,
        /// so the readout says "none" rather than a number made of
        /// rounding.
        pub const READOUT_QUIET: f32 = 1e-6;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Sync",
                min: 0.0,
                max: 5.0,
                default: 4.0,
            },
            ParamDef {
                id: 1,
                name: "Time",
                min: 1.0,
                max: 2000.0,
                default: 375.0,
            },
            ParamDef {
                id: 2,
                name: "Feedback",
                min: 0.0,
                max: 100.0,
                default: 35.0,
            },
            ParamDef {
                id: 3,
                name: "Tone",
                min: 200.0,
                max: 12000.0,
                default: 3000.0,
            },
            ParamDef {
                id: 4,
                name: "Wow",
                min: 0.0,
                max: 100.0,
                default: 10.0,
            },
            ParamDef {
                id: 5,
                name: "Ping-pong",
                min: 0.0,
                max: 1.0,
                default: 0.0,
            },
            ParamDef {
                id: 6,
                name: "Mix",
                min: 0.0,
                max: 100.0,
                default: 0.0,
            },
        ];
    }

    pub mod room {
        use super::super::ParamDef;

        pub const ALGO: u32 = 0;
        pub const PREDELAY: u32 = 1;
        pub const SIZE: u32 = 2;
        pub const DAMP: u32 = 3;
        pub const MIX: u32 = 4;

        pub const ROOM: u32 = 0;
        pub const HALL: u32 = 1;
        /// The longest pre-delay, in ms.
        pub const MAX_PREDELAY_MS: f32 = 200.0;
        /// The damping corner at no damp and at full, in Hz.
        pub const DAMP_OPEN_HZ: f32 = 18_000.0;
        pub const DAMP_SHUT_HZ: f32 = 1_400.0;
        /// The hall's decay at the smallest size and the largest, in
        /// seconds, and its diffusion and modulation.
        pub const HALL_SHORT_S: f32 = 0.6;
        pub const HALL_LONG_S: f32 = 9.0;
        pub const HALL_DIFFUSION: f32 = 0.8;
        pub const HALL_MODULATION: f32 = 6.0;
        /// The room's decay across its size.
        pub const ROOM_DECAY_LOW: f32 = 0.3;
        pub const ROOM_DECAY_HIGH: f32 = 0.92;
        /// How fast the readout's three bands fall, in ms. A tail is
        /// read at frame rate, so the bands hold their peak and fall
        /// this slowly: a ring reads as a smooth ramp, not a flicker.
        pub const READOUT_FALL_MS: f32 = 300.0;
        /// The bottom of the two level bands' range, in dBFS: a peak at
        /// or under this reads 0.0 and a peak at full scale reads 1.0,
        /// so the bands are a 60 dB window on the send and the return.
        pub const READOUT_FLOOR_DB: f32 = -60.0;
        /// The linear amplitude at which the wet return counts as
        /// silence. Below it the brightness ratio is dividing noise by
        /// noise, so that band falls instead of reading it; it is also
        /// where a falling band is snapped to zero, so the meter never
        /// trails off into denormals.
        pub const READOUT_QUIET: f32 = 1e-7;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Algo",
                min: 0.0,
                max: 1.0,
                default: 0.0,
            },
            ParamDef {
                id: 1,
                name: "Predelay",
                min: 0.0,
                max: 200.0,
                default: 10.0,
            },
            ParamDef {
                id: 2,
                name: "Size",
                min: 0.0,
                max: 100.0,
                default: 50.0,
            },
            ParamDef {
                id: 3,
                name: "Damp",
                min: 0.0,
                max: 100.0,
                default: 40.0,
            },
            ParamDef {
                id: 4,
                name: "Mix",
                min: 0.0,
                max: 100.0,
                default: 0.0,
            },
        ];
    }

    pub mod out {
        use super::super::ParamDef;

        pub const WIDTH: u32 = 0;
        pub const BASS_MONO: u32 = 1;
        pub const SEND_TAPE: u32 = 2;
        pub const SEND_SHADOW: u32 = 3;

        /// Width is off at unity; bass mono is off at the bottom of its
        /// range.
        pub const WIDTH_OFF: f32 = 100.0;
        pub const BASS_MONO_OFF_HZ: f32 = 0.0;
        /// The crossover that takes the bottom to mono.
        pub const BASS_ORDER: u32 = 2;

        /// The readout's three followers, in milliseconds: how long a
        /// measured figure takes to cover ~63% of a step toward what the
        /// block just said. Correlation is the figure a reader stares
        /// at, so it is steadied hardest; the spread has to open as fast
        /// as a hand can pan; the bass share only moves when the corner
        /// does, and a slow one keeps it from flickering on transients.
        pub const CORRELATION_MS: f32 = 150.0;
        pub const SPREAD_MS: f32 = 120.0;
        pub const BASS_SHARE_MS: f32 = 200.0;

        /// A block whose summed squares fall under this is silence, and
        /// a ratio taken from it would be the quotient of two roundings.
        /// The measured bands rest at zero instead of inventing a
        /// direction for noise.
        pub const QUIET_SUM: f32 = 1e-9;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Width",
                min: 0.0,
                max: 200.0,
                default: 100.0,
            },
            ParamDef {
                id: 1,
                name: "Bass Mono",
                min: 0.0,
                max: 300.0,
                default: 0.0,
            },
            ParamDef {
                id: 2,
                name: "Send Tape",
                min: 0.0,
                max: 100.0,
                default: 0.0,
            },
            ParamDef {
                id: 3,
                name: "Send Shadow",
                min: 0.0,
                max: 100.0,
                default: 0.0,
            },
        ];
    }

    pub mod glue {
        use super::super::ParamDef;

        pub const LEAN: u32 = 0;

        /// The desk decided the rest: a bus compressor's ratio, its
        /// attack, its knee, and an auto release.
        pub const RATIO: f32 = 2.5;
        pub const ATTACK_MS: f32 = 10.0;
        pub const RELEASE_MS: f32 = 400.0;
        pub const KNEE_DB: f32 = 6.0;
        pub const DETECT_MS: f32 = 8.0;
        /// The threshold the LEAN knob walks: nothing at the bottom,
        /// well into the mix at the top.
        pub const THRESHOLD_HIGH_DB: f32 = -6.0;
        pub const THRESHOLD_LOW_DB: f32 = -34.0;
        /// Half the reduction is given back, so leaning does not simply
        /// turn the bus down.
        pub const MAKEUP_SHARE: f32 = 0.5;

        pub const TABLE: &[ParamDef] = &[ParamDef {
            id: 0,
            name: "Lean",
            min: 0.0,
            max: 100.0,
            default: 30.0,
        }];
    }

    pub mod iron {
        use super::super::ParamDef;

        pub const DRIVE: u32 = 0;

        /// The transformer: the bottom is lifted before the curve and
        /// put back after, so low frequencies drive the iron harder.
        pub const LIFT_HZ: f32 = 120.0;
        pub const LIFT_DB: f32 = 4.0;
        /// How hard the curve is driven at full, and how far its bias
        /// walks: a transformer's asymmetry grows with the flux, so the
        /// even harmonics keep coming as the drive goes up rather than
        /// settling at the floor's.
        pub const CURVE_DRIVE: f32 = 3.0;
        pub const BIAS_FLOOR: f32 = 0.08;
        pub const BIAS_FULL: f32 = 0.55;
        /// The level the stage is unity at: a −10 dBFS sine leaves as
        /// it arrived, whatever the drive.
        pub const UNITY_AT: f32 = 0.316;

        /// One ballistics law for every figure the section reports:
        /// peak-hold within a block, then this much of it survives into
        /// the next. At a 256-sample block that is a ~5 ms half-life,
        /// fast enough to follow a bus and slow enough to read.
        pub const TELEMETRY_DECAY: f32 = 0.8;
        /// The asymmetry reading's full-scale: the shaped signal's mean
        /// as a fraction of its peak, times this, is 1.0. A quarter of
        /// the peak is as lopsided as this curve ever gets, so a quarter
        /// is the top of the scale.
        pub const ASYM_SCALE: f32 = 4.0;
        /// The guard on every telemetry divisor, so a silent block
        /// reports zero rather than a NaN.
        pub const TELEMETRY_EPS: f32 = 1e-6;

        pub const TABLE: &[ParamDef] = &[ParamDef {
            id: 0,
            name: "Drive",
            min: 5.0,
            max: 100.0,
            default: 20.0,
        }];
    }

    pub mod ceiling {
        use super::super::ParamDef;

        /// The mix's ceiling, in dBFS, and the limiter's window. Not
        /// knobs: the desk's last stage has one job and one setting.
        pub const CEILING_DB: f32 = -0.3;
        pub const LOOKAHEAD_MS: f32 = 1.5;
        pub const RELEASE_MS: f32 = 120.0;

        /// Where the HELD GAIN reads at rest, in dB. An open limiter is
        /// spending nothing, and a surface draws that as a press closed
        /// flush against the beam rather than as a zeroed meter.
        pub const REST_GAIN_DB: f32 = 0.0;
        /// Where the WINDOW PEAK reads on silence, in dBFS. Borrowed
        /// from the kernel's own floor rather than restated, so the band
        /// and `LookaheadLimiter::window_peak_db` can never disagree
        /// about what an empty lookahead window reads.
        pub const SILENCE_DB: f32 = crate::dsp::dynamics::FLOOR_DB;

        pub const TABLE: &[ParamDef] = &[];
    }

    pub mod scope {
        use super::super::ParamDef;

        /// The analyser's three bands, by their corners in Hz. Three
        /// because three is what the telemetry channel carries; a bin
        /// -by-bin spectrum wants a channel of its own.
        pub const LOW_HZ: f32 = 200.0;
        pub const HIGH_HZ: f32 = 2_500.0;
        /// How fast the bands fall, in ms: slow enough to read.
        pub const FALL_MS: f32 = 300.0;

        pub const TABLE: &[ParamDef] = &[];
    }

    pub mod tape {
        use super::super::ParamDef;

        pub const TIME: u32 = 0;
        pub const FEEDBACK: u32 = 1;
        pub const WOW: u32 = 2;
        pub const FLUTTER: u32 = 3;
        pub const HISS: u32 = 4;
        pub const TONE: u32 = 5;

        /// The longest the line holds, in ms.
        pub const MAX_MS: f32 = 1_600.0;
        /// Wow is slow and deep; flutter is fast and shallow. Rates in
        /// Hz, depths as a share of the time at full.
        pub const WOW_HZ: f32 = 0.7;
        pub const WOW_DEPTH: f32 = 0.03;
        pub const FLUTTER_HZ: f32 = 7.5;
        pub const FLUTTER_DEPTH: f32 = 0.004;
        /// The loop's soft top, and the tape's own floor in dBFS.
        pub const LOOP_DRIVE: f32 = 0.4;
        pub const HISS_DB: f32 = -60.0;
        /// The hiss rides what the tape is carrying: an idle return is
        /// silent, and a running one hisses under its repeats. How fast
        /// that follower rises and falls, in ms.
        pub const HISS_RISE_MS: f32 = 5.0;
        pub const HISS_FALL_MS: f32 = 400.0;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Time",
                min: 50.0,
                max: 1500.0,
                default: 375.0,
            },
            ParamDef {
                id: 1,
                name: "Feedback",
                min: 0.0,
                max: 100.0,
                default: 45.0,
            },
            ParamDef {
                id: 2,
                name: "Wow",
                min: 0.0,
                max: 100.0,
                default: 20.0,
            },
            ParamDef {
                id: 3,
                name: "Flutter",
                min: 0.0,
                max: 100.0,
                default: 10.0,
            },
            ParamDef {
                id: 4,
                name: "Hiss",
                min: 0.0,
                max: 100.0,
                default: 15.0,
            },
            ParamDef {
                id: 5,
                name: "Tone",
                min: 500.0,
                max: 10000.0,
                default: 3500.0,
            },
        ];
    }

    pub mod shadow {
        use super::super::ParamDef;

        pub const PREDELAY: u32 = 0;
        pub const SIZE: u32 = 1;
        pub const DAMP: u32 = 2;

        pub const MAX_PREDELAY_MS: f32 = 200.0;
        /// The decay across the size knob, in seconds.
        pub const SHORT_S: f32 = 0.8;
        pub const LONG_S: f32 = 12.0;
        /// The damping corner at no damp and at full, in Hz.
        pub const DAMP_OPEN_HZ: f32 = 18_000.0;
        pub const DAMP_SHUT_HZ: f32 = 1_200.0;
        pub const DIFFUSION: f32 = 0.85;
        pub const MODULATION: f32 = 8.0;

        pub const TABLE: &[ParamDef] = &[
            ParamDef {
                id: 0,
                name: "Predelay",
                min: 0.0,
                max: 200.0,
                default: 20.0,
            },
            ParamDef {
                id: 1,
                name: "Size",
                min: 0.0,
                max: 100.0,
                default: 70.0,
            },
            ParamDef {
                id: 2,
                name: "Damp",
                min: 0.0,
                max: 100.0,
                default: 50.0,
            },
        ];
    }
}
