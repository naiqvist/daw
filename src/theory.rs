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
/// The interval, in semitones, from a sounding pitch to the note `steps`
/// SCALE DEGREES away from it in the given key.
///
/// This is the device's whole musical claim, and it is a pure function so
/// it can be tested without an FFT in the room. `source_midi` may be
/// fractional — a singer is not on the grid — and is snapped to the
/// nearest degree before counting, because "a third above" is counted
/// from the note the scale has, not from wherever the voice happened to
/// land.
pub fn scale_interval(source_midi: f32, key: i16, scale: Scale, steps: i32) -> f32 {
    if steps == 0 || !source_midi.is_finite() {
        return 0.0;
    }
    let degrees = scale.degrees();
    let n = degrees.len() as i32;
    if n == 0 {
        return 0.0;
    }
    let rounded = source_midi.round() as i16;
    let pc = (rounded - key).rem_euclid(12);
    // The degree the source is nearest to, by pitch-class distance.
    let mut nearest = 0i32;
    let mut best = i16::MAX;
    for (i, d) in degrees.iter().enumerate() {
        let raw = (pc - *d).rem_euclid(12);
        let distance = raw.min(12 - raw);
        if distance < best {
            best = distance;
            nearest = i as i32;
        }
    }
    let target = nearest + steps;
    let octave = target.div_euclid(n);
    let index = target.rem_euclid(n) as usize;
    let from = degrees.get(nearest as usize).copied().unwrap_or(0);
    let to = degrees.get(index).copied().unwrap_or(0) + 12 * octave as i16;
    // The snap has to be part of the answer. Counting degrees from the
    // note the source is NEAREST gives the interval between two scale
    // tones — but the source may not be one, and applying that interval
    // to where the voice actually is lands the harmony off the scale by
    // however far off it was. An A over C minor snapped to A-flat and
    // then rose four semitones to C-sharp, which is in no key involved.
    // Adding the snap back makes the harmony land ON the scale tone,
    // which is the only thing "harmony in key" can mean.
    let snap = {
        let raw = (from - pc).rem_euclid(12);
        if raw > 6 { raw - 12 } else { raw }
    };
    f32::from(snap + to - from)
}

/// idea than this app has yet.
pub fn pitch_class_name(pitch: u8) -> &'static str {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    NAMES[(pitch % 12) as usize]
}

// --------------------------------------------------------- chord symbols ---

/// One spelled chord member. Keeping the generic degree beside its semitone
/// is what makes stacked edits such as `C13b9#11no5` unambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChordMember {
    pub degree: u8,
    pub semitones: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChordBass {
    /// `/3`, `/5`, `/7`, etc. names a chord member, not an inversion index.
    Member(u8),
    /// `/E3` is absolute; `/E` chooses the nearest E below the upper chord.
    Pitch {
        pitch_class: u8,
        octave: Option<i16>,
    },
}

/// A composable chord formula. This is deliberately not an enum of chord
/// names: additions, alterations and omissions can be stacked indefinitely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordSymbol {
    pub root_pc: u8,
    pub root_octave: Option<i16>,
    pub members: Vec<ChordMember>,
    pub bass: Option<ChordBass>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChordQuality {
    Major,
    Minor,
    Diminished,
    Augmented,
    HalfDiminished,
    Sus2,
    Sus4,
    Power,
}

impl ChordSymbol {
    /// Parse standard lead-sheet symbols, including arbitrary stacked
    /// `add`, alteration and `no` modifiers.
    pub fn parse(source: &str) -> Result<Self, String> {
        let source = source.trim();
        let (upper, slash) = source
            .split_once('/')
            .map_or((source, None), |(upper, bass)| (upper, Some(bass)));
        if slash.is_some_and(|bass| bass.is_empty() || bass.contains('/')) {
            return Err(format!("invalid slash bass in `{source}`"));
        }
        let (root_pc, root_octave, mut descriptor) = parse_pitch_head(upper)?;
        let mut quality = ChordQuality::Major;
        let mut explicit_major = false;
        let mut force_major_seventh = false;

        if let Some(rest) =
            strip_word_ci(descriptor, "minorMajor").or_else(|| strip_word_ci(descriptor, "minMaj"))
        {
            quality = ChordQuality::Minor;
            force_major_seventh = true;
            descriptor = rest;
        } else if let Some(rest) = strip_word_ci(descriptor, "halfDiminished")
            .or_else(|| strip_word_ci(descriptor, "halfDim"))
            .or_else(|| descriptor.strip_prefix('ø'))
        {
            quality = ChordQuality::HalfDiminished;
            descriptor = rest;
        } else if let Some(rest) = strip_word_ci(descriptor, "diminished")
            .or_else(|| strip_word_ci(descriptor, "dim"))
            .or_else(|| descriptor.strip_prefix('°'))
        {
            quality = ChordQuality::Diminished;
            descriptor = rest;
        } else if let Some(rest) = strip_word_ci(descriptor, "augmented")
            .or_else(|| strip_word_ci(descriptor, "aug"))
            .or_else(|| descriptor.strip_prefix('+'))
        {
            quality = ChordQuality::Augmented;
            descriptor = rest;
        } else if let Some(rest) = strip_word_ci(descriptor, "sus2") {
            quality = ChordQuality::Sus2;
            descriptor = rest;
        } else if let Some(rest) = strip_word_ci(descriptor, "sus4") {
            quality = ChordQuality::Sus4;
            descriptor = rest;
        } else if let Some(rest) =
            strip_word_ci(descriptor, "minor").or_else(|| strip_word_ci(descriptor, "min"))
        {
            quality = ChordQuality::Minor;
            descriptor = rest;
        } else if let Some(rest) = descriptor
            .strip_prefix('M')
            .or_else(|| strip_word_ci(descriptor, "major"))
            .or_else(|| strip_word_ci(descriptor, "maj"))
        {
            quality = ChordQuality::Major;
            explicit_major = true;
            descriptor = rest;
        } else if let Some(rest) = descriptor.strip_prefix('m') {
            quality = ChordQuality::Minor;
            descriptor = rest;
        } else if let Some(rest) =
            strip_word_ci(descriptor, "dominant").or_else(|| strip_word_ci(descriptor, "dom"))
        {
            quality = ChordQuality::Major;
            descriptor = rest;
        }

        let mut extension = None;
        if let Some(rest) = strip_word_ci(descriptor, "maj7") {
            extension = Some(7);
            force_major_seventh = true;
            descriptor = rest;
        } else {
            for (token, degree) in [
                ("13", 13),
                ("11", 11),
                ("9", 9),
                ("7", 7),
                ("6", 6),
                ("5", 5),
            ] {
                if let Some(rest) = descriptor.strip_prefix(token) {
                    extension = Some(degree);
                    descriptor = rest;
                    break;
                }
            }
        }
        if extension == Some(5) {
            quality = ChordQuality::Power;
        }

        let mut members = base_members(quality);
        if let Some(extension) = extension {
            add_extension(
                &mut members,
                extension,
                quality,
                explicit_major || force_major_seventh,
            );
        }

        while !descriptor.is_empty() {
            if let Some(rest) = strip_word_ci(descriptor, "sus2") {
                remove_degree(&mut members, 3);
                set_degree(&mut members, 2, 2);
                descriptor = rest;
                continue;
            }
            if let Some(rest) = strip_word_ci(descriptor, "sus4") {
                remove_degree(&mut members, 3);
                set_degree(&mut members, 4, 5);
                descriptor = rest;
                continue;
            }
            let mut handled = false;
            for (token, degree) in [
                ("add13", 13),
                ("add11", 11),
                ("add9", 9),
                ("add7", 7),
                ("add6", 6),
                ("add4", 4),
                ("add2", 2),
            ] {
                if let Some(rest) = strip_word_ci(descriptor, token) {
                    set_degree(&mut members, degree, natural_degree_semitones(degree)?);
                    descriptor = rest;
                    handled = true;
                    break;
                }
            }
            if handled {
                continue;
            }
            for degree in [13, 11, 9, 7, 6, 5, 4, 3, 2] {
                let token = format!("no{degree}");
                if let Some(rest) = strip_word_ci(descriptor, &token) {
                    remove_degree(&mut members, degree);
                    descriptor = rest;
                    handled = true;
                    break;
                }
            }
            if handled {
                continue;
            }
            let accidentals = descriptor
                .bytes()
                .take_while(|byte| matches!(byte, b'b' | b'#'))
                .count();
            if accidentals > 0 {
                let tail = &descriptor[accidentals..];
                let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
                if digits > 0 {
                    let degree = tail[..digits]
                        .parse::<u8>()
                        .map_err(|_| format!("invalid altered degree in `{source}`"))?;
                    let delta = descriptor[..accidentals]
                        .bytes()
                        .fold(0i16, |sum, byte| sum + if byte == b'#' { 1 } else { -1 });
                    set_degree(
                        &mut members,
                        degree,
                        natural_degree_semitones(degree)? + delta,
                    );
                    descriptor = &tail[digits..];
                    continue;
                }
            }
            return Err(format!(
                "unknown chord modifier `{descriptor}` in `{source}`"
            ));
        }

        members.sort_by_key(|member| (member.semitones, member.degree));
        members.dedup_by_key(|member| member.semitones);
        let bass = slash.map(parse_chord_bass).transpose()?;
        Ok(Self {
            root_pc,
            root_octave,
            members,
            bass,
        })
    }
}

/// How a parsed formula is laid out as sounding pitches.
///
/// The symbol itself carries no register: `Cmaj7` is a formula, and where it
/// lands is the caller's business — the cursor's octave, usually.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Voicing {
    /// Root octave used when the symbol did not spell one. `C4 = MIDI 60`.
    pub octave: i16,
    /// Rotations of the upper structure: each one lifts its lowest voice by
    /// an octave. An explicit slash bass stays underneath.
    pub inversion: u8,
    /// Keep a slash bass that lands on an upper voice's exact pitch instead
    /// of collapsing the two.
    pub double_bass: bool,
}

impl Default for Voicing {
    fn default() -> Self {
        Self {
            octave: 4,
            inversion: 0,
            double_bass: false,
        }
    }
}

impl ChordSymbol {
    /// Realise the formula as MIDI pitches, low to high.
    ///
    /// Out-of-range voices REFUSE rather than clamp: a clamped chord is a
    /// different chord, and silently returning one is how a generator earns
    /// distrust. The caller decides whether to fold, drop or give up.
    pub fn pitches(&self, voicing: Voicing) -> Result<Vec<u8>, String> {
        let octave = self.root_octave.unwrap_or(voicing.octave);
        let root = (octave + 1) * 12 + i16::from(self.root_pc);

        let mut upper: Vec<i16> = self
            .members
            .iter()
            .map(|member| root + member.semitones)
            .collect();
        upper.sort_unstable();
        if usize::from(voicing.inversion) >= upper.len().max(1) {
            return Err(format!(
                "inversion {} is outside this {}-note chord",
                voicing.inversion,
                upper.len()
            ));
        }
        for _ in 0..voicing.inversion {
            let lowest = upper.remove(0);
            // Re-sorted rather than pushed: a chord wider than an octave can
            // rotate its bottom voice into the MIDDLE of the stack, and the
            // result is still promised to be ascending.
            let raised = lowest + 12;
            let at = upper.partition_point(|pitch| *pitch < raised);
            upper.insert(at, raised);
        }

        let mut pitches = upper;
        if let Some(bass) = self.bass {
            let floor = pitches.first().copied().unwrap_or(root);
            let pitch = match bass {
                ChordBass::Member(degree) => {
                    let member = self
                        .members
                        .iter()
                        .find(|member| member.degree == degree)
                        .ok_or_else(|| format!("this chord has no {degree} to put in the bass"))?;
                    below(root + member.semitones, floor)
                }
                ChordBass::Pitch {
                    pitch_class,
                    octave: Some(octave),
                } => (octave + 1) * 12 + i16::from(pitch_class),
                ChordBass::Pitch {
                    pitch_class,
                    octave: None,
                } => below((octave + 1) * 12 + i16::from(pitch_class), floor),
            };
            let at = pitches.partition_point(|other| *other < pitch);
            pitches.insert(at, pitch);
        }
        if !voicing.double_bass {
            pitches.dedup();
        }

        pitches
            .into_iter()
            .map(|pitch| {
                u8::try_from(pitch)
                    .ok()
                    .filter(|pitch| *pitch <= MAX_PITCH)
                    .ok_or_else(|| {
                        format!(
                            "{} in octave {octave} falls outside MIDI pitches 0..={MAX_PITCH}",
                            self.label()
                        )
                    })
            })
            .collect()
    }

    /// The root's pitch-class name. Spelling is sharps-only for now; see
    /// [`pitch_class_name`].
    pub fn label(&self) -> String {
        pitch_class_name(self.root_pc).to_owned()
    }
}

// ------------------------------------------------------------- intervals ---

/// Named intervals, ascending, in semitones. Every spelling this module
/// accepts is a row here, because a vocabulary the parser knows and the
/// help page does not is a vocabulary nobody finds.
///
/// Case is load-bearing: `m3` and `M3` are different intervals exactly as
/// `!cm9` and `!cM9` are different chords, and this module already asks a
/// reader to hold that distinction. The spellings where case cannot
/// possibly mean two things — `P`, `TT`, `octave` — accept either.
pub const INTERVALS: &[(&str, i16)] = &[
    ("P1", 0),
    ("p1", 0),
    ("U", 0),
    ("u", 0),
    ("unison", 0),
    ("m2", 1),
    ("M2", 2),
    ("m3", 3),
    ("M3", 4),
    ("P4", 5),
    ("p4", 5),
    ("A4", 6),
    ("a4", 6),
    ("d5", 6),
    ("TT", 6),
    ("tt", 6),
    ("tritone", 6),
    ("P5", 7),
    ("p5", 7),
    ("m6", 8),
    ("A5", 8),
    ("M6", 9),
    ("d7", 9),
    ("m7", 10),
    ("M7", 11),
    ("P8", 12),
    ("p8", 12),
    ("octave", 12),
    ("8ve", 12),
];

/// Parse a signed interval: either semitones as a number, or a named
/// interval. `-m2` is down a minor second, `M2` is up a major second, `-3`
/// and `-m3` are the same thing, and `2st` says semitones out loud.
///
/// An unsigned name reads as ASCENDING, which is how a musician says an
/// interval when they do not say a direction. Descending needs the `-`.
///
/// Degree numbers on their own are deliberately NOT accepted: `3` here is
/// three semitones, but `3` in a chord symbol is a major third, which is
/// four. One of those readings has to win and silence would pick the wrong
/// one about half the time, so a bare number is always semitones and an
/// interval always carries its quality.
pub fn parse_interval(source: &str) -> Result<i16, String> {
    let (sign, rest) = match source.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, source.strip_prefix('+').unwrap_or(source)),
    };
    if rest.is_empty() {
        return Err(format!("`{source}` is not an interval"));
    }

    let digits = rest.strip_suffix("st").unwrap_or(rest);
    if let Ok(semitones) = digits.parse::<i16>() {
        return Ok(sign * semitones);
    }

    INTERVALS
        .iter()
        .find(|(name, _)| *name == rest)
        .map(|(_, semitones)| sign * semitones)
        .ok_or_else(|| {
            format!("`{source}` is not an interval; try -m2, M2, -m3, P5, 2st or a number")
        })
}

// -------------------------------------------------------- voicing transforms ---

/// Fold a chord into its tightest voicing: every pitch class stacked inside
/// the smallest span that can hold them.
///
/// This is the transform that makes seconds out of thirds. `Fm9no5` —
/// F A♭ E♭ G — clusters to E♭ F G A♭, a four-note secundal stack, and that
/// voicing rather than the `m9` label is what the record is actually built
/// from. Chord symbols name intervals; this names a sound.
///
/// A cluster has no doublings by definition, so duplicate pitch classes
/// collapse and the result can be SHORTER than the input.
///
/// Register is preserved rather than chosen: of the octave placements the
/// stack could take, this returns the one whose centre sits nearest the
/// original chord's, so clustering re-voices without also transposing. Ties
/// go to the lower placement.
///
/// There is no looser variant, because a loose cluster is just a close
/// voicing and we already have one. `cluster` means minimal, always.
pub fn cluster(pitches: &[u8]) -> Vec<u8> {
    if pitches.is_empty() {
        return Vec::new();
    }

    let mut classes: Vec<i32> = pitches.iter().map(|pitch| i32::from(*pitch) % 12).collect();
    classes.sort_unstable();
    classes.dedup();
    let voices = classes.len();

    // The tightest rotation, found by stacking from each pitch class in turn
    // and measuring. Exhaustive over a set of at most twelve is cheaper than
    // being clever, and it cannot get the answer subtly wrong.
    let mut best: Vec<i32> = Vec::new();
    for start in 0..voices {
        let mut stack = Vec::with_capacity(voices);
        let mut last = classes[start];
        stack.push(last);
        for step in 1..voices {
            let mut next = classes[(start + step) % voices];
            while next <= last {
                next += 12;
            }
            stack.push(next);
            last = next;
        }
        if best.is_empty() || stack[voices - 1] - stack[0] < best[voices - 1] - best[0] {
            best = stack;
        }
    }

    // Nearest octave placement, compared as means. Scaled to integers rather
    // than divided: `sum * len` on both sides keeps the comparison exact, and
    // an exact comparison is what makes the tie-break reproducible.
    let original: i32 = pitches.iter().map(|pitch| i32::from(*pitch)).sum();
    let original_voices = pitches.len() as i32;
    let unit = 12 * voices as i32 * original_voices;
    let target = original * voices as i32;
    let base = best.iter().sum::<i32>() * original_voices;
    let mut shift = 0;
    let mut closest = i32::MAX;
    for octaves in -12..=12 {
        let error = (base + octaves * unit - target).abs();
        if error < closest {
            closest = error;
            shift = octaves;
        }
    }

    let mut stack: Vec<i32> = best.iter().map(|pitch| pitch + shift * 12).collect();
    // Walked into range rather than clamped: clamping would collapse two
    // voices onto one pitch and stop being a cluster. A stack of at most
    // twelve distinct classes spans at most 11 semitones, so it always fits.
    while stack[0] < 0 {
        stack.iter_mut().for_each(|pitch| *pitch += 12);
    }
    while stack[voices - 1] > i32::from(MAX_PITCH) {
        stack.iter_mut().for_each(|pitch| *pitch -= 12);
    }

    stack
        .into_iter()
        .map(|pitch| pitch.clamp(0, i32::from(MAX_PITCH)) as u8)
        .collect()
}

/// Drop `pitch` by octaves until it sits strictly under `floor`.
fn below(mut pitch: i16, floor: i16) -> i16 {
    while pitch >= floor {
        pitch -= 12;
    }
    pitch
}

fn parse_pitch_head(source: &str) -> Result<(u8, Option<i16>, &str), String> {
    let mut chars = source.char_indices();
    let (_, letter) = chars
        .next()
        .ok_or_else(|| "missing chord root".to_owned())?;
    let base = match letter.to_ascii_uppercase() {
        'C' => 0i16,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return Err(format!("invalid chord root `{letter}`; use A through G")),
    };
    let mut at = letter.len_utf8();
    let mut accidental = 0i16;
    while let Some(byte) = source.as_bytes().get(at) {
        match byte {
            b'b' => accidental -= 1,
            b'#' => accidental += 1,
            _ => break,
        }
        at += 1;
    }
    let octave_start = at;
    let negative = source.as_bytes().get(at) == Some(&b'-');
    if negative {
        at += 1;
    }
    let digit_start = at;
    while source.as_bytes().get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    let digits = &source[digit_start..at];
    let tail = &source[at..];

    // A digit run straight after the root is an octave unless it can only be
    // a chord extension: `C4maj7` is a major seventh in octave 4, `C7sus4` is
    // a dominant seventh with a suspended fourth. The number decides first —
    // only 5, 6, 7, 9, 11 and 13 name extensions — and what follows breaks
    // the ties that leaves, because a quality word must PRECEDE its extension
    // while `sus`, `add`, `no` and an alteration can only FOLLOW one.
    const EXTENSIONS: [&str; 6] = ["5", "6", "7", "9", "11", "13"];
    const MODIFIERS: [&str; 5] = ["sus", "add", "no", "b", "#"];
    let extension = !negative
        && EXTENSIONS.contains(&digits)
        && (tail.is_empty()
            || MODIFIERS
                .iter()
                .any(|token| strip_word_ci(tail, token).is_some()));

    let octave = if digits.is_empty() || extension {
        at = octave_start;
        None
    } else {
        Some(
            source[octave_start..at]
                .parse::<i16>()
                .map_err(|_| format!("invalid octave in `{source}`"))?,
        )
    };
    Ok((
        (base + accidental).rem_euclid(12) as u8,
        octave,
        &source[at..],
    ))
}

fn parse_chord_bass(source: &str) -> Result<ChordBass, String> {
    if source.bytes().all(|byte| byte.is_ascii_digit()) {
        let degree = source
            .parse::<u8>()
            .map_err(|_| format!("invalid slash member `{source}`"))?;
        return Ok(ChordBass::Member(degree));
    }
    let (pitch_class, octave, rest) = parse_pitch_head(source)?;
    if !rest.is_empty() {
        return Err(format!("invalid slash bass `{source}`"));
    }
    Ok(ChordBass::Pitch {
        pitch_class,
        octave,
    })
}

fn strip_word_ci<'a>(source: &'a str, token: &str) -> Option<&'a str> {
    let head = source.get(..token.len())?;
    head.eq_ignore_ascii_case(token)
        .then(|| &source[token.len()..])
}

fn base_members(quality: ChordQuality) -> Vec<ChordMember> {
    let degrees: &[(u8, i16)] = match quality {
        ChordQuality::Major => &[(1, 0), (3, 4), (5, 7)],
        ChordQuality::Minor => &[(1, 0), (3, 3), (5, 7)],
        ChordQuality::Diminished | ChordQuality::HalfDiminished => &[(1, 0), (3, 3), (5, 6)],
        ChordQuality::Augmented => &[(1, 0), (3, 4), (5, 8)],
        ChordQuality::Sus2 => &[(1, 0), (2, 2), (5, 7)],
        ChordQuality::Sus4 => &[(1, 0), (4, 5), (5, 7)],
        ChordQuality::Power => &[(1, 0), (5, 7)],
    };
    degrees
        .iter()
        .map(|&(degree, semitones)| ChordMember { degree, semitones })
        .collect()
}

fn add_extension(
    members: &mut Vec<ChordMember>,
    extension: u8,
    quality: ChordQuality,
    major_seventh: bool,
) {
    if extension == 5 {
        return;
    }
    if extension == 6 {
        set_degree(members, 6, 9);
        return;
    }
    let seventh = match quality {
        ChordQuality::Diminished => 9,
        ChordQuality::HalfDiminished => 10,
        _ if major_seventh => 11,
        _ => 10,
    };
    set_degree(members, 7, seventh);
    for degree in [9, 11, 13] {
        if extension >= degree {
            // A conventional 13th leaves the natural 11th available for an
            // explicit `no11`; the formula does not silently omit voices.
            if let Ok(semitones) = natural_degree_semitones(degree) {
                set_degree(members, degree, semitones);
            }
        }
    }
}

fn natural_degree_semitones(degree: u8) -> Result<i16, String> {
    let simple = (degree.saturating_sub(1)) % 7 + 1;
    let octaves = degree.saturating_sub(1) / 7;
    let within = match simple {
        1 => 0,
        2 => 2,
        3 => 4,
        4 => 5,
        5 => 7,
        6 => 9,
        7 => 11,
        _ => return Err(format!("unsupported chord degree `{degree}`")),
    };
    Ok(within + i16::from(octaves) * 12)
}

fn set_degree(members: &mut Vec<ChordMember>, degree: u8, semitones: i16) {
    remove_degree(members, degree);
    members.push(ChordMember { degree, semitones });
}

fn remove_degree(members: &mut Vec<ChordMember>, degree: u8) {
    members.retain(|member| member.degree != degree);
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

    // ---------------------------------------------------------- intervals ---

    #[test]
    fn intervals_are_named_and_signed() {
        assert_eq!(parse_interval("m2"), Ok(1));
        assert_eq!(parse_interval("M2"), Ok(2));
        assert_eq!(parse_interval("-m2"), Ok(-1));
        assert_eq!(parse_interval("+M3"), Ok(4));
        assert_eq!(parse_interval("-P5"), Ok(-7));
        assert_eq!(parse_interval("octave"), Ok(12));
        // Semitones, with or without saying so.
        assert_eq!(parse_interval("-1"), Ok(-1));
        assert_eq!(parse_interval("2st"), Ok(2));
        assert_eq!(parse_interval("-7st"), Ok(-7));
        // Unsigned reads as ascending, the way a musician says it.
        assert_eq!(parse_interval("m3"), parse_interval("+m3"));
    }

    /// Case separates two different intervals, exactly as it separates two
    /// different chords.
    #[test]
    fn interval_case_is_load_bearing() {
        assert_ne!(parse_interval("m3"), parse_interval("M3"));
        assert_eq!(parse_interval("m6"), Ok(8));
        assert_eq!(parse_interval("M6"), Ok(9));
    }

    /// A bare number is semitones, never a scale degree — the two disagree
    /// about every interval that has a quality, so only one reading is
    /// allowed to exist.
    #[test]
    fn a_bare_number_is_semitones_and_a_degree_needs_its_quality() {
        assert_eq!(parse_interval("3"), Ok(3));
        assert_eq!(parse_interval("M3"), Ok(4));
        assert!(parse_interval("b3").is_err());
        assert!(parse_interval("#4").is_err());
    }

    #[test]
    fn every_named_interval_parses_and_stays_inside_an_octave() {
        for (name, semitones) in INTERVALS {
            assert_eq!(parse_interval(name), Ok(*semitones), "{name}");
            assert_eq!(
                parse_interval(&format!("-{name}")),
                Ok(-semitones),
                "-{name}"
            );
            assert!((0..=12).contains(semitones), "{name} is not an interval");
        }
        for bad in ["", "-", "+", "x", "M9", "P3", "second", "m", "2nd"] {
            assert!(parse_interval(bad).is_err(), "`{bad}` should not parse");
        }
    }

    // ------------------------------------------------------------ cluster ---

    /// The voicing the whole idea exists for: `Fm9no5` becomes a stack of
    /// seconds, not a stack of thirds.
    #[test]
    fn a_cluster_makes_seconds_out_of_thirds() {
        // F4 m9 no5 as the symbol realises it: F Ab Eb G, spread over a 14th.
        let voiced = vec![65, 68, 75, 79];
        // Eb4 F4 G4 Ab4 — whole, whole, half.
        assert_eq!(cluster(&voiced), vec![63, 65, 67, 68]);
    }

    #[test]
    fn a_cluster_is_the_tightest_arrangement_available() {
        for chord in [
            vec![65, 68, 75, 79],
            vec![60, 64, 67],
            vec![60, 62, 65, 69, 74],
            vec![60, 61, 62, 63, 64, 65, 66],
        ] {
            let clustered = cluster(&chord);
            let span = clustered[clustered.len() - 1] - clustered[0];
            // No rotation of the same pitch classes can do better.
            let mut classes: Vec<i32> = chord.iter().map(|pitch| i32::from(*pitch) % 12).collect();
            classes.sort_unstable();
            classes.dedup();
            for start in 0..classes.len() {
                let low = classes[start];
                let mut last = low;
                for step in 1..classes.len() {
                    let mut next = classes[(start + step) % classes.len()];
                    while next <= last {
                        next += 12;
                    }
                    last = next;
                }
                assert!(
                    i32::from(span) <= last - low,
                    "{chord:?} clustered to {clustered:?}, span {span}, but a rotation spans {}",
                    last - low
                );
            }
        }
    }

    /// A cluster is a re-voicing, not a transposition: it stays where the
    /// chord already was.
    #[test]
    fn a_cluster_keeps_its_register() {
        for chord in [
            vec![65, 68, 75, 79],
            vec![36, 43, 52],
            vec![84, 88, 91, 95],
            vec![60, 64, 67],
        ] {
            let clustered = cluster(&chord);
            let mean = |voices: &[u8]| {
                voices.iter().map(|pitch| i32::from(*pitch)).sum::<i32>() / voices.len() as i32
            };
            assert!(
                (mean(&clustered) - mean(&chord)).abs() <= 6,
                "{chord:?} clustered to {clustered:?} — that is a transposition"
            );
        }
    }

    /// Doublings are what a cluster has none of, so they collapse.
    #[test]
    fn a_cluster_has_no_doublings() {
        // C major with the root doubled two octaves up.
        assert_eq!(cluster(&[48, 52, 55, 72]).len(), 3);
        assert_eq!(cluster(&[60, 60, 60]), vec![60]);
    }

    /// Total, like everything else here: every input shape answers.
    #[test]
    fn a_cluster_answers_for_every_chord() {
        assert_eq!(cluster(&[]), Vec::<u8>::new());
        assert_eq!(cluster(&[60]), vec![60]);
        for low in 0..=127u8 {
            let chord = vec![low, low.saturating_add(4), low.saturating_add(7)];
            let clustered = cluster(&chord);
            assert!(!clustered.is_empty());
            for pair in clustered.windows(2) {
                assert!(
                    pair[0] < pair[1],
                    "{chord:?} -> {clustered:?} is not ascending"
                );
            }
            for pitch in &clustered {
                assert!(*pitch <= MAX_PITCH);
                assert!(
                    chord.iter().any(|other| other % 12 == pitch % 12),
                    "{chord:?} -> {clustered:?} invented a pitch class"
                );
            }
        }
    }

    #[test]
    fn chords_are_built_from_the_root_up() {
        assert_eq!(chord_pitches(60, Quality::Major), vec![60, 64, 67]);
        assert_eq!(chord_pitches(60, Quality::Minor), vec![60, 63, 67]);
        assert_eq!(chord_pitches(60, Quality::Dominant7), vec![60, 64, 67, 70]);
        assert_eq!(chord_pitches(60, Quality::Sus4), vec![60, 65, 67]);
    }

    fn formula(source: &str) -> Vec<(u8, i16)> {
        ChordSymbol::parse(source)
            .unwrap()
            .members
            .into_iter()
            .map(|member| (member.degree, member.semitones))
            .collect()
    }

    #[test]
    fn chord_symbols_compose_extensions_alterations_and_omissions() {
        assert_eq!(
            formula("Cmaj7#11"),
            vec![(1, 0), (3, 4), (5, 7), (7, 11), (11, 18)]
        );
        assert_eq!(
            formula("C13b9no5"),
            vec![(1, 0), (3, 4), (7, 10), (9, 13), (11, 17), (13, 21)]
        );
        assert_eq!(formula("F#m7b5"), vec![(1, 0), (3, 3), (5, 6), (7, 10)]);
        assert_eq!(
            formula("CmMaj7add9"),
            vec![(1, 0), (3, 3), (5, 7), (7, 11), (9, 14)]
        );
        assert_eq!(formula("C7sus4"), vec![(1, 0), (4, 5), (5, 7), (7, 10)]);
        assert_eq!(formula("C5"), vec![(1, 0), (5, 7)]);
    }

    #[test]
    fn slash_bass_and_root_octave_have_distinct_meanings() {
        let absolute = ChordSymbol::parse("C4maj7/E3").unwrap();
        assert_eq!(absolute.root_octave, Some(4));
        assert_eq!(
            absolute.bass,
            Some(ChordBass::Pitch {
                pitch_class: 4,
                octave: Some(3)
            })
        );
        assert_eq!(
            ChordSymbol::parse("Cmaj7/3").unwrap().bass,
            Some(ChordBass::Member(3))
        );
    }

    fn voiced(source: &str, inversion: u8) -> Vec<u8> {
        ChordSymbol::parse(source)
            .unwrap()
            .pitches(Voicing {
                inversion,
                ..Voicing::default()
            })
            .unwrap()
    }

    #[test]
    fn a_formula_becomes_pitches_at_the_octave_it_is_given() {
        assert_eq!(voiced("Cmaj7", 0), vec![60, 64, 67, 71]);
        assert_eq!(voiced("C5", 0), vec![60, 67]);
        // The symbol's own octave outranks the caller's default.
        assert_eq!(voiced("C3", 0), vec![48, 52, 55]);
        assert_eq!(voiced("Bb-1", 0), vec![10, 14, 17]);
    }

    #[test]
    fn inversion_lifts_the_bottom_voice_and_the_slash_bass_stays_under_it() {
        assert_eq!(voiced("C", 1), vec![64, 67, 72]);
        assert_eq!(voiced("C", 2), vec![67, 72, 76]);
        // A member bass and the equivalent absolute bass agree.
        assert_eq!(voiced("Cmaj7/3", 0), vec![52, 60, 64, 67, 71]);
        assert_eq!(voiced("C4maj7/E3", 0), vec![52, 60, 64, 67, 71]);
        // Rotating the upper structure leaves the bass alone.
        assert_eq!(voiced("C4maj7/E3", 1), vec![52, 64, 67, 71, 72]);
    }

    #[test]
    fn a_chord_that_will_not_fit_refuses_instead_of_clamping() {
        let high = ChordSymbol::parse("Cmaj7").unwrap().pitches(Voicing {
            octave: 9,
            ..Voicing::default()
        });
        assert!(high.is_err(), "{high:?}");
        let inverted = ChordSymbol::parse("C").unwrap().pitches(Voicing {
            inversion: 3,
            ..Voicing::default()
        });
        assert!(inverted.is_err(), "{inverted:?}");
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

    // ------------------------------------------------ the musical claim ---

    /// THE CLAIM: a harmony measured in scale steps is a DIFFERENT
    /// number of semitones depending on where in the key it starts.
    ///
    /// This is the whole reason the device counts degrees instead of
    /// semitones, and it is a pure function, so it is tested exactly
    /// rather than by ear. In C major a third above C is four semitones
    /// and a third above D is three — a harmoniser that shifts by a
    /// fixed +4 is wrong on the second note, which is the failure people
    /// mean when they say harmonisers sound cheap.
    #[test]
    fn a_third_is_not_always_the_same_number_of_semitones() {
        let major = Scale::Major;
        // C major, thirds up: C-E is 4, D-F is 3, E-G is 3, F-A is 4.
        for (note, want) in [(60.0f32, 4.0f32), (62.0, 3.0), (64.0, 3.0), (65.0, 4.0)] {
            let got = scale_interval(note, 0, major, 2);
            assert_eq!(got, want, "a third above MIDI {note} in C major");
        }
        // ...and it wraps the octave correctly: B up a third is D.
        assert_eq!(scale_interval(71.0, 0, major, 2), 3.0, "B to D");
        // Fifths.
        assert_eq!(scale_interval(60.0, 0, major, 4), 7.0, "C to G");
        assert_eq!(scale_interval(62.0, 0, major, 4), 7.0, "D to A");
        // Downward, through the octave below.
        assert_eq!(scale_interval(60.0, 0, major, -2), -3.0, "C down to A");
        // A minor key gives a MINOR third off the tonic.
        assert_eq!(
            scale_interval(60.0, 0, Scale::NaturalMinor, 2),
            3.0,
            "C to Eb in C minor"
        );
        // Zero steps is unison, and never anything else.
        for note in [60.0f32, 61.5, 70.0] {
            assert_eq!(scale_interval(note, 0, major, 0), 0.0);
        }
    }

    /// A singer is not on the grid: a note a third of a semitone sharp
    /// still counts its degrees from the note it is nearest.
    #[test]
    fn an_out_of_tune_source_still_counts_from_the_right_degree() {
        for offset in [-0.4f32, -0.2, 0.0, 0.2, 0.4] {
            let got = scale_interval(62.0 + offset, 0, Scale::Major, 2);
            assert_eq!(got, 3.0, "D{offset:+} up a third should be F");
        }
    }

    /// Nonsense in, unison out — never a NaN ratio, which would silence
    /// the device for the rest of the session.
    #[test]
    fn scale_interval_survives_nonsense() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(scale_interval(bad, 0, Scale::Major, 2), 0.0);
        }
        for steps in [-99i32, 99] {
            let got = scale_interval(60.0, 0, Scale::Major, steps);
            assert!(got.is_finite(), "{steps} steps gave {got}");
        }
        for key in [-5i16, 0, 11, 40] {
            assert!(scale_interval(60.0, key, Scale::PentatonicMinor, 3).is_finite());
        }
    }

    /// THE PROPERTY: whatever came in, the harmony lands IN THE SCALE.
    ///
    /// Counting degrees is only half of it. The source may be a note the
    /// scale does not contain — a passing tone, a bend, a singer between
    /// two pitches — and an interval measured between two scale tones,
    /// applied to a source that is not one, lands off the scale by
    /// exactly how far off the source was. This walks every semitone
    /// through every scale and every step and insists the destination is
    /// a note the key actually has.
    #[test]
    fn a_harmony_always_lands_in_the_scale() {
        for scale in Scale::ALL {
            for key in 0..12i16 {
                for source in 48..84i16 {
                    for steps in [-7i32, -4, -3, -2, -1, 1, 2, 3, 4, 7] {
                        let interval = scale_interval(f32::from(source), key, scale, steps);
                        let landed = source + interval as i16;
                        assert!(
                            scale.contains(key as u8, landed as u8),
                            "{scale:?} in key {key}: {source} + {steps} steps \
                             landed on {landed}, which is not in the scale"
                        );
                    }
                }
            }
        }
    }

    /// ...and it goes the way it was asked to go.
    #[test]
    fn a_harmony_moves_in_the_direction_it_was_asked_for() {
        for scale in Scale::ALL {
            for source in [60.0f32, 61.0, 62.0, 63.5] {
                for steps in 1..=5i32 {
                    let up = scale_interval(source, 0, scale, steps);
                    let down = scale_interval(source, 0, scale, -steps);
                    assert!(up > 0.0, "{scale:?}: +{steps} gave {up}");
                    assert!(down < 0.0, "{scale:?}: -{steps} gave {down}");
                }
            }
        }
    }
}
