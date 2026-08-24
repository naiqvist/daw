//! Music theory: chords, scales, intervals, and the consonance rules
//! counterpoint is built from.
//!
//! ONE module, by contract (`notes/20260824-command-palette.md`): no chord
//! table gets written twice. Pure arithmetic over MIDI pitch numbers — no
//! egui, no engine, no note type. Callers own what a "note" is; this
//! module only ever answers "which pitches, and is that interval allowed".
//!
//! Everything here is total: an out-of-range pitch clamps rather than
//! panicking, because these functions are driven by a cursor the user can
//! park anywhere.

/// Highest MIDI pitch. Chords built near the ceiling clamp into range
/// rather than wrapping into the bass.
pub const MAX_PITCH: u8 = 127;

/// A chord quality, as semitone offsets from the root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Major,
    Minor,
    Diminished,
    Augmented,
    Major7,
    Minor7,
    Dominant7,
    Sus2,
    Sus4,
}

impl Quality {
    /// Every quality, in the order the palette lists them: triads first,
    /// then sevenths, then the suspensions.
    pub const ALL: [Self; 9] = [
        Self::Major,
        Self::Minor,
        Self::Diminished,
        Self::Augmented,
        Self::Major7,
        Self::Minor7,
        Self::Dominant7,
        Self::Sus2,
        Self::Sus4,
    ];

    /// Semitones above the root. Root itself is always the first entry, so
    /// a caller can take `&intervals()[..n]` for a partial voicing.
    pub fn intervals(self) -> &'static [i16] {
        match self {
            Self::Major => &[0, 4, 7],
            Self::Minor => &[0, 3, 7],
            Self::Diminished => &[0, 3, 6],
            Self::Augmented => &[0, 4, 8],
            Self::Major7 => &[0, 4, 7, 11],
            Self::Minor7 => &[0, 3, 7, 10],
            Self::Dominant7 => &[0, 4, 7, 10],
            Self::Sus2 => &[0, 2, 7],
            Self::Sus4 => &[0, 5, 7],
        }
    }

    /// What the palette calls it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Major => "major",
            Self::Minor => "minor",
            Self::Diminished => "diminished",
            Self::Augmented => "augmented",
            Self::Major7 => "major 7th",
            Self::Minor7 => "minor 7th",
            Self::Dominant7 => "dominant 7th",
            Self::Sus2 => "sus2",
            Self::Sus4 => "sus4",
        }
    }
}

/// The pitches of `quality` rooted at `root`, low to high. Any tone that
/// would pass [`MAX_PITCH`] is dropped rather than wrapped — a chord near
/// the ceiling loses its top, it does not sprout a bass note.
pub fn chord_pitches(root: u8, quality: Quality) -> Vec<u8> {
    quality
        .intervals()
        .iter()
        .filter_map(|iv| {
            let p = i16::from(root) + iv;
            (p <= i16::from(MAX_PITCH)).then_some(p as u8)
        })
        .collect()
}

/// A diatonic scale, as semitone degrees from its tonic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    Major,
    NaturalMinor,
    Dorian,
    Mixolydian,
    PentatonicMinor,
}

impl Scale {
    pub fn degrees(self) -> &'static [i16] {
        match self {
            Self::Major => &[0, 2, 4, 5, 7, 9, 11],
            Self::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
            Self::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Self::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            Self::PentatonicMinor => &[0, 3, 5, 7, 10],
        }
    }

    /// Is `pitch` in this scale rooted at `tonic`?
    pub fn contains(self, tonic: u8, pitch: u8) -> bool {
        let pc = (i16::from(pitch) - i16::from(tonic)).rem_euclid(12);
        self.degrees().contains(&pc)
    }

    pub const ALL: [Self; 5] = [
        Self::Major,
        Self::NaturalMinor,
        Self::Dorian,
        Self::Mixolydian,
        Self::PentatonicMinor,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Major => "major",
            Self::NaturalMinor => "minor",
            Self::Dorian => "dorian",
            Self::Mixolydian => "mixolydian",
            Self::PentatonicMinor => "pentatonic minor",
        }
    }

    /// The nearest in-scale pitch, preferring DOWNWARD on a tie.
    ///
    /// Ties are broken consistently rather than by rounding luck: a
    /// chromatic note between two scale tones must always land on the same
    /// one, or the same keystroke would give different notes on different
    /// days.
    pub fn snap(self, tonic: u8, pitch: u8) -> u8 {
        if self.contains(tonic, pitch) {
            return pitch;
        }
        let mut best = pitch;
        let mut best_dist = i16::MAX;
        for delta in 0..=6i16 {
            for cand in [i16::from(pitch) - delta, i16::from(pitch) + delta] {
                if !(0..=i16::from(MAX_PITCH)).contains(&cand) {
                    continue;
                }
                let cand = cand as u8;
                if self.contains(tonic, cand) && delta < best_dist {
                    best = cand;
                    best_dist = delta;
                }
            }
            if best_dist <= delta {
                break;
            }
        }
        best
    }

    /// The diatonic triad built on `root`, stacking scale thirds — so the
    /// quality follows the DEGREE instead of being chosen: in C major, a
    /// triad on D is minor and one on B is diminished, with no need to say
    /// so. This is the difference between writing chords and writing music
    /// in a key.
    ///
    /// `root` snaps into the scale first, so a cursor parked on a black key
    /// still yields a chord that belongs.
    pub fn diatonic_triad(self, tonic: u8, root: u8) -> Vec<u8> {
        let degrees = self.degrees();
        let root = self.snap(tonic, root);
        let pc = (i16::from(root) - i16::from(tonic)).rem_euclid(12);
        let Some(idx) = degrees.iter().position(|d| *d == pc) else {
            return vec![root];
        };
        // Thirds are two scale steps apart, wrapping octaves as they pass
        // the tonic.
        [0usize, 2, 4]
            .iter()
            .filter_map(|step| {
                let i = idx + step;
                let octaves = (i / degrees.len()) as i16;
                let deg = degrees[i % degrees.len()];
                let p = i16::from(root) - pc + deg + octaves * 12;
                (0..=i16::from(MAX_PITCH)).contains(&p).then_some(p as u8)
            })
            .collect()
    }
}

/// Note name of a pitch class, sharps only. Enough for a key readout;
/// proper spelling (F# vs Gb) needs a key signature, which is a bigger
/// idea than this app has yet.
pub fn pitch_class_name(pitch: u8) -> &'static str {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    NAMES[(pitch % 12) as usize]
}

// ------------------------------------------------------------ intervals ---

/// Consonant intervals, as semitone distances within an octave: unison,
/// minor/major third, perfect fourth*, perfect fifth, minor/major sixth,
/// octave.
///
/// The fourth is deliberately ABSENT. Against a bass it is a dissonance in
/// species counterpoint, and treating it as consonant is the single
/// fastest way to make generated counterpoint sound wrong.
const CONSONANT: [i16; 6] = [0, 3, 4, 7, 8, 9];

/// Perfect intervals — the ones that must not move in parallel.
const PERFECT: [i16; 2] = [0, 7];

/// Semitone distance between two pitches, folded into one octave.
pub fn interval_class(a: u8, b: u8) -> i16 {
    (i16::from(a) - i16::from(b)).abs() % 12
}

/// Is this interval consonant? See [`CONSONANT`] on the fourth.
pub fn is_consonant(a: u8, b: u8) -> bool {
    CONSONANT.contains(&interval_class(a, b))
}

/// Is this a perfect consonance (unison, fifth, octave)?
pub fn is_perfect(a: u8, b: u8) -> bool {
    PERFECT.contains(&interval_class(a, b))
}

/// The two classic parallel faults: consecutive perfect intervals of the
/// same class, reached by both voices moving in the same direction.
///
/// `prev` and `now` are `(cantus, counter)` pitch pairs.
pub fn is_parallel_perfect(prev: (u8, u8), now: (u8, u8)) -> bool {
    if !is_perfect(now.0, now.1) || interval_class(prev.0, prev.1) != interval_class(now.0, now.1) {
        return false;
    }
    let d_cantus = i16::from(now.0) - i16::from(prev.0);
    let d_counter = i16::from(now.1) - i16::from(prev.1);
    // Both moved, and in the same direction.
    d_cantus != 0 && d_counter != 0 && d_cantus.signum() == d_counter.signum()
}

/// Motion between two successive pairs. Contrary motion is what makes two
/// lines sound independent rather than like one thickened line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Contrary,
    Oblique,
    Similar,
}

pub fn motion(prev: (u8, u8), now: (u8, u8)) -> Motion {
    let a = i16::from(now.0) - i16::from(prev.0);
    let b = i16::from(now.1) - i16::from(prev.1);
    if a == 0 || b == 0 {
        Motion::Oblique
    } else if a.signum() == b.signum() {
        Motion::Similar
    } else {
        Motion::Contrary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_are_built_from_the_root_up() {
        assert_eq!(chord_pitches(60, Quality::Major), vec![60, 64, 67]);
        assert_eq!(chord_pitches(60, Quality::Minor), vec![60, 63, 67]);
        assert_eq!(chord_pitches(60, Quality::Dominant7), vec![60, 64, 67, 70]);
        assert_eq!(chord_pitches(60, Quality::Sus4), vec![60, 65, 67]);
    }

    #[test]
    fn a_chord_at_the_ceiling_loses_its_top_not_its_bottom() {
        // Root two semitones under the ceiling: the third and fifth would
        // pass 127. They are dropped, never wrapped into the bass.
        let high = chord_pitches(125, Quality::Major);
        assert_eq!(high, vec![125]);
        assert!(high.iter().all(|p| *p >= 125));
        let near = chord_pitches(120, Quality::Major);
        assert_eq!(near, vec![120, 124, 127]);
    }

    #[test]
    fn scales_answer_membership_in_any_octave() {
        // C major contains E in every octave, and never contains C#.
        for oct in 0..10u8 {
            let base = oct * 12;
            if base + 4 > MAX_PITCH {
                break;
            }
            assert!(Scale::Major.contains(0, base + 4), "E natural is diatonic");
            assert!(!Scale::Major.contains(0, base + 1), "C# is not");
        }
        assert!(Scale::Dorian.contains(62, 71), "D dorian has a natural 6th");
        assert!(!Scale::NaturalMinor.contains(62, 71), "D minor does not");
    }

    #[test]
    fn snapping_lands_in_the_scale_and_never_wanders() {
        // C major: every chromatic note lands on a scale tone, and a note
        // already in the scale never moves.
        for p in 60..72u8 {
            let s = Scale::Major.snap(0, p);
            assert!(Scale::Major.contains(0, s), "{p} snapped to {s}, off-scale");
            assert!(
                (i16::from(s) - i16::from(p)).abs() <= 1,
                "snap should be near"
            );
        }
        assert_eq!(Scale::Major.snap(0, 64), 64, "E is already diatonic");
        // Ties break downward, and do so every time.
        assert_eq!(Scale::Major.snap(0, 61), 60, "C# sits between C and D");
        assert_eq!(Scale::Major.snap(0, 61), Scale::Major.snap(0, 61));
    }

    #[test]
    fn diatonic_triads_take_their_quality_from_the_degree() {
        // C major: I major, ii minor, vii diminished — chosen by the scale,
        // not by the caller.
        assert_eq!(Scale::Major.diatonic_triad(0, 60), vec![60, 64, 67]);
        assert_eq!(Scale::Major.diatonic_triad(0, 62), vec![62, 65, 69]);
        assert_eq!(Scale::Major.diatonic_triad(0, 71), vec![71, 74, 77]);
        // Every tone of every degree is in the scale.
        for root in 60..72u8 {
            for p in Scale::Major.diatonic_triad(0, root) {
                assert!(Scale::Major.contains(0, p), "{p} is not in C major");
            }
        }
        // A chromatic root snaps in rather than producing something alien.
        assert_eq!(
            Scale::Major.diatonic_triad(0, 61),
            Scale::Major.diatonic_triad(0, 60)
        );
    }

    #[test]
    fn pitch_classes_are_named() {
        assert_eq!(pitch_class_name(60), "C");
        assert_eq!(pitch_class_name(61), "C#");
        assert_eq!(pitch_class_name(69), "A");
        assert_eq!(pitch_class_name(0), pitch_class_name(120));
    }

    #[test]
    fn consonance_excludes_the_fourth() {
        assert!(is_consonant(60, 64), "major third");
        assert!(is_consonant(60, 67), "perfect fifth");
        assert!(is_consonant(60, 72), "octave");
        assert!(is_consonant(60, 69), "major sixth");
        assert!(!is_consonant(60, 65), "the fourth is a dissonance here");
        assert!(!is_consonant(60, 61), "minor second");
        assert!(!is_consonant(60, 66), "tritone");
    }

    #[test]
    fn perfect_intervals_are_the_ones_that_may_not_move_in_parallel() {
        assert!(is_perfect(60, 67), "fifth");
        assert!(is_perfect(60, 72), "octave");
        assert!(!is_perfect(60, 64), "a third is imperfect");

        // Both voices up a tone, fifth to fifth: the classic fault.
        assert!(is_parallel_perfect((60, 67), (62, 69)));
        // Same interval, but the cantus holds — oblique, so it is legal.
        assert!(!is_parallel_perfect((60, 67), (60, 67)));
        // Contrary motion into a fifth is fine: cantus down, counter up.
        assert!(!is_parallel_perfect((62, 65), (60, 67)));
        // Thirds in parallel are not a fault at all.
        assert!(!is_parallel_perfect((60, 64), (62, 66)));
    }

    #[test]
    fn motion_names_what_the_two_lines_did() {
        assert_eq!(motion((60, 67), (62, 65)), Motion::Contrary);
        assert_eq!(motion((60, 67), (62, 69)), Motion::Similar);
        assert_eq!(motion((60, 67), (60, 69)), Motion::Oblique);
    }
}
