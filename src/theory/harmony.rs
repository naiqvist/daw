//! Chord compatibility and concrete voicings for MIDI Lab. Extends the one
//! theory vocabulary; symbols are parsed by the existing lead-sheet parser.
use super::{ChordBass, ChordMember, ChordSymbol};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HarmonicStyle {
    #[default]
    Strict,
    Chromatic,
}
impl HarmonicStyle {
    pub fn label(self) -> &'static str {
        match self {
            Self::Strict => "Chord rules",
            Self::Chromatic => "Chromatic",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layout {
    #[default]
    Closed,
    Open,
    Drop2,
    Drop3,
    Drop24,
    Rootless,
    Shell,
    Quartal,
    Upper,
}
impl Layout {
    pub const ALL: [Self; 9] = [
        Self::Closed,
        Self::Open,
        Self::Drop2,
        Self::Drop3,
        Self::Drop24,
        Self::Rootless,
        Self::Shell,
        Self::Quartal,
        Self::Upper,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Closed => "Closed",
            Self::Open => "Open / spread",
            Self::Drop2 => "Drop 2",
            Self::Drop3 => "Drop 3",
            Self::Drop24 => "Drop 2 + 4",
            Self::Rootless => "Rootless",
            Self::Shell => "Shell",
            Self::Quartal => "Quartal",
            Self::Upper => "Upper structure",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoicingSpec {
    pub layout: Layout,
    pub octave: i16,
    pub inversion: u8,
    pub omitted: Vec<u8>,
    pub added: Vec<ChordMember>,
    /// Degree and octave displacement, applied after the named method.
    pub offsets: Vec<(u8, i16)>,
    pub doubled: Vec<u8>,
    pub low: u8,
    pub high: u8,
    pub lead: bool,
}
impl Default for VoicingSpec {
    fn default() -> Self {
        Self {
            layout: Layout::Closed,
            octave: 3,
            inversion: 0,
            omitted: vec![],
            added: vec![],
            offsets: vec![],
            doubled: vec![],
            low: 24,
            high: 108,
            lead: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tone {
    pub pitch: u8,
    pub degree: u8,
    pub label: String,
    pub extension: bool,
}

pub fn parse(source: &str) -> Result<ChordSymbol, String> {
    if source.len() > 160 {
        return Err("Chord symbol is too long".into());
    }
    let normal = source
        .replace('♯', "#")
        .replace('♭', "b")
        .replace('Δ', "maj")
        .replace(['(', ')', ' ', ','], "");
    ChordSymbol::parse(&normal)
}

/// Written omissions affect the realised voicing, not the harmonic family.
/// Cmaj7no3 still has the major-seventh tension policy.
pub fn context(source: &str) -> Result<ChordSymbol, String> {
    let mut source = source.to_owned();
    for degree in [13, 11, 9, 7, 5, 3, 1] {
        source = source.replace(&format!("no{degree}"), "");
    }
    parse(&source)
}

/// A vocabulary of supported tensions, with explicit alterations taking
/// precedence over the natural form of the same degree.
pub fn available(chord: &ChordSymbol) -> Vec<ChordMember> {
    let has = |pc| {
        chord
            .members
            .iter()
            .any(|m| m.semitones.rem_euclid(12) == pc)
    };
    let minor = has(3) && !has(4);
    let diminished = has(6) && !has(7) && minor;
    let third = if minor { 3 } else { 4 };
    let fourth = if has(4) { 18 } else { 17 };
    let thirteenth = if diminished { 20 } else { 21 };
    let mut result = chord.members.clone();
    // A harmonic root remains available to melodic/bass voices after a
    // voicing-only root omission.
    if !result.iter().any(|m| m.degree == 1) {
        result.push(ChordMember {
            degree: 1,
            semitones: 0,
        });
    }
    for (degree, semitones) in [(9, 14), (11, fourth), (13, thirteenth)] {
        if !result
            .iter()
            .any(|m| m.degree == degree || (m.degree + 7 == degree))
        {
            result.push(ChordMember { degree, semitones });
        }
    }
    // Never add the natural fourth above a major third by a scale fallback.
    if third == 4 && has(4) {
        result.retain(|m| m.semitones.rem_euclid(12) != 5);
    }
    result.sort_by_key(|m| (m.degree, m.semitones));
    result.dedup_by_key(|m| (m.degree, m.semitones));
    result
}

pub fn allows(chord: &ChordSymbol, pitch: u8, style: HarmonicStyle) -> bool {
    if style == HarmonicStyle::Chromatic {
        return true;
    }
    let pc = (i16::from(pitch) - i16::from(chord.root_pc)).rem_euclid(12);
    available(chord)
        .iter()
        .any(|m| m.semitones.rem_euclid(12) == pc)
}

pub fn degree_label(member: ChordMember) -> String {
    let natural = match member.degree {
        1 => 0,
        2 => 2,
        3 => 4,
        4 => 5,
        5 => 7,
        6 => 9,
        7 => 11,
        9 => 14,
        11 => 17,
        13 => 21,
        _ => member.semitones,
    };
    let delta = member.semitones - natural;
    format!(
        "{}{}",
        if delta > 0 {
            "#".repeat(delta.min(2) as usize)
        } else {
            "b".repeat((-delta).min(2) as usize)
        },
        member.degree
    )
}

pub fn spell(symbol: &str, degree: u8, pitch: u8) -> String {
    let root = symbol.chars().next().unwrap_or('C').to_ascii_uppercase();
    let letters = ['C', 'D', 'E', 'F', 'G', 'A', 'B'];
    let natural = [0i16, 2, 4, 5, 7, 9, 11];
    let at = (letters.iter().position(|&c| c == root).unwrap_or(0)
        + usize::from(degree.saturating_sub(1)))
        % 7;
    let delta = (i16::from(pitch % 12) - natural[at] + 6).rem_euclid(12) - 6;
    let accidental = if delta > 0 {
        "#".repeat(delta as usize)
    } else {
        "b".repeat((-delta) as usize)
    };
    // Octave is the spelled letter's octave, including B#/Cb at a seam.
    let octave = (i16::from(pitch) - natural[at] - delta) / 12 - 1;
    format!("{}{accidental}{octave}", letters[at])
}

pub fn realise(
    source: &str,
    spec: &VoicingSpec,
    style: HarmonicStyle,
    previous: &[Tone],
) -> Result<Vec<Tone>, String> {
    let original = parse(source)?;
    let intent = context(source)?;
    let mut chord = original.clone();
    for &added in &spec.added {
        if !available(&intent).iter().any(|m| *m == added) && style == HarmonicStyle::Strict {
            return Err(format!(
                "{} is outside {source}'s chord rules",
                degree_label(added)
            ));
        }
        chord.members.retain(|m| m.degree != added.degree);
        chord.members.push(added);
    }
    chord.members.retain(|m| !spec.omitted.contains(&m.degree));
    if spec.layout == Layout::Rootless {
        chord.members.retain(|m| m.degree != 1);
    }
    if spec.layout == Layout::Shell {
        chord.members.retain(|m| matches!(m.degree, 1 | 3 | 7));
    }
    chord.members.sort_by_key(|m| m.semitones);
    if chord.members.is_empty() {
        return Ok(vec![]);
    }
    if chord.members.len() > 10 || spec.doubled.len() > 6 {
        return Err("Use at most ten chord members and six doublings".into());
    }
    let octave = original.root_octave.unwrap_or(spec.octave).clamp(-1, 9);
    let root = (octave + 1) * 12 + i16::from(original.root_pc);
    let mut voices: Vec<(i16, ChordMember)> = chord
        .members
        .iter()
        .map(|m| (root + m.semitones, *m))
        .collect();
    if spec.layout == Layout::Quartal {
        voices = quartal(
            &chord.members,
            root,
            i16::from(spec.low),
            i16::from(spec.high),
        )
        .ok_or_else(|| {
            "No quartal voicing fits these members, omissions and range; try adding 9 and 13"
                .to_owned()
        })?;
    }
    let rotations = usize::from(spec.inversion) % voices.len();
    for _ in 0..rotations {
        let mut first = voices.remove(0);
        let ceiling = voices.last().map_or(first.0, |v| v.0);
        while first.0 <= ceiling {
            first.0 += 12;
        }
        voices.push(first);
    }
    match spec.layout {
        Layout::Open => {
            for (i, v) in voices.iter_mut().enumerate() {
                if i % 2 == 1 {
                    v.0 += 12;
                }
            }
        }
        Layout::Drop2 | Layout::Drop3 | Layout::Drop24 => {
            let count = voices.len();
            let drops: &[usize] = match spec.layout {
                Layout::Drop2 => &[2],
                Layout::Drop3 => &[3],
                _ => &[2, 4],
            };
            for &drop in drops {
                if drop > count {
                    return Err(format!(
                        "{} needs at least {drop} voices",
                        spec.layout.label()
                    ));
                }
                voices[count - drop].0 -= 12;
            }
        }
        Layout::Upper => {
            for (pitch, member) in &mut voices {
                if member.degree >= 9 {
                    *pitch += 12;
                }
            }
        }
        _ => {}
    }
    for (pitch, member) in &mut voices {
        if let Some((_, octaves)) = spec
            .offsets
            .iter()
            .find(|(degree, _)| *degree == member.degree)
        {
            *pitch += (*octaves).clamp(-4, 4) * 12;
        }
    }
    for &degree in &spec.doubled {
        if let Some(&(pitch, member)) = voices.iter().find(|(_, m)| m.degree == degree) {
            voices.push((pitch + 12, member));
        }
    }
    if let Some(bass) = original.bass {
        let floor = voices.iter().map(|v| v.0).min().unwrap_or(root);
        let (mut pitch, explicit) = match bass {
            ChordBass::Member(degree) => (
                root + original
                    .members
                    .iter()
                    .find(|m| m.degree == degree)
                    .ok_or("The slash degree is omitted")?
                    .semitones,
                false,
            ),
            ChordBass::Pitch {
                pitch_class,
                octave: Some(o),
            } => ((o + 1) * 12 + i16::from(pitch_class), true),
            ChordBass::Pitch {
                pitch_class,
                octave: None,
            } => ((octave + 1) * 12 + i16::from(pitch_class), false),
        };
        if !explicit {
            while pitch > floor {
                pitch -= 12;
            }
        }
        let pc = (pitch - root).rem_euclid(12);
        let degree = available(&original)
            .iter()
            .find(|m| m.semitones.rem_euclid(12) == pc)
            .map_or(1, |m| m.degree);
        if !voices.iter().any(|v| v.0 == pitch) {
            voices.push((
                pitch,
                ChordMember {
                    degree,
                    semitones: pc,
                },
            ));
        }
    }
    voices.sort_by_key(|v| v.0);
    if spec.lead && !previous.is_empty() && spec.inversion == 0 && original.bass.is_none() {
        // Compact layouts can search inversions. Named spread/quartal
        // spacing and explicit degree displacements keep their shape.
        let rotations = if matches!(
            spec.layout,
            Layout::Closed | Layout::Rootless | Layout::Shell
        ) && spec.offsets.is_empty()
            && spec.doubled.is_empty()
        {
            voices.len()
        } else {
            1
        };
        let mut candidate = voices.clone();
        let mut best = (f64::INFINITY, voices.clone());
        for _ in 0..rotations {
            for shift in [-24i16, -12, 0, 12, 24] {
                if candidate.iter().any(|v| {
                    v.0 + shift < i16::from(spec.low) || v.0 + shift > i16::from(spec.high)
                }) {
                    continue;
                }
                let cost = candidate
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        f64::from(
                            (v.0 + shift - i16::from(previous[i.min(previous.len() - 1)].pitch))
                                .abs(),
                        )
                    })
                    .sum::<f64>();
                if cost < best.0 {
                    best = (
                        cost,
                        candidate.iter().map(|(p, m)| (p + shift, *m)).collect(),
                    );
                }
            }
            let mut first = candidate.remove(0);
            let ceiling = candidate.last().map_or(first.0, |v| v.0);
            while first.0 <= ceiling {
                first.0 += 12;
            }
            candidate.push(first);
        }
        voices = best.1;
    }
    voices
        .into_iter()
        .map(|(pitch, member)| {
            if pitch < i16::from(spec.low)
                || pitch > i16::from(spec.high)
                || !(0..=127).contains(&pitch)
            {
                return Err(format!(
                    "Voicing exceeds {}–{}; change register or range",
                    spec.low, spec.high
                ));
            }
            if !allows(&intent, pitch as u8, style) {
                return Err(format!(
                    "{} is excluded over {source}; change the tension or chord rules",
                    spell(source, member.degree, pitch as u8)
                ));
            }
            Ok(Tone {
                pitch: pitch as u8,
                degree: member.degree,
                label: spell(source, member.degree, pitch as u8),
                extension: member.degree >= 9,
            })
        })
        .collect()
}

fn quartal(
    members: &[ChordMember],
    root: i16,
    low: i16,
    high: i16,
) -> Option<Vec<(i16, ChordMember)>> {
    // Bounded beam over pitch-class permutations. Every adjacent interval
    // is genuinely a perfect or augmented fourth, including octave spacing.
    let mut paths: Vec<Vec<(i16, usize)>> = Vec::new();
    for (i, member) in members.iter().enumerate() {
        for shift in [-12i16, 0, 12] {
            let pitch = root + member.semitones.rem_euclid(12) + shift;
            if pitch >= low && pitch <= high {
                paths.push(vec![(pitch, i)]);
            }
        }
    }
    for _ in 1..members.len() {
        let mut next = Vec::new();
        for path in &paths {
            let pitch = path.last()?.0;
            for (i, member) in members.iter().enumerate() {
                if path.iter().any(|p| p.1 == i) {
                    continue;
                }
                for interval in [5i16, 6] {
                    let p = pitch + interval;
                    if p <= high && (p - root).rem_euclid(12) == member.semitones.rem_euclid(12) {
                        let mut candidate = path.clone();
                        candidate.push((p, i));
                        next.push(candidate);
                    }
                }
            }
        }
        next.sort_by_key(|p| (p[0].0 - root).abs());
        next.truncate(192);
        paths = next;
    }
    paths.sort_by_key(|p| {
        let augmented = p.windows(2).filter(|w| w[1].0 - w[0].0 == 6).count();
        (augmented, (p[0].0 - root).abs())
    });
    paths
        .first()
        .map(|p| p.iter().map(|&(pitch, i)| (pitch, members[i])).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn voice_leading_keeps_common_tones_and_moves_only_the_seventh() {
        let mut v = VoicingSpec::default();
        v.lead = true;
        let before = realise("Cmaj7", &v, HarmonicStyle::Strict, &[]).unwrap();
        let after = realise("Am7", &v, HarmonicStyle::Strict, &before).unwrap();
        assert_eq!(
            after.iter().map(|n| n.pitch).collect::<Vec<_>>(),
            [48, 52, 55, 57]
        );
        v.inversion = 1;
        assert_eq!(
            realise("Cmaj7", &v, HarmonicStyle::Strict, &after).unwrap()[0].pitch,
            52
        );
    }
    #[test]
    fn major_seventh_colours_and_omissions_keep_the_intent() {
        let c = parse("Cmaj7").unwrap();
        for p in [60, 62, 64, 66, 67, 69, 71] {
            assert!(allows(&c, p, HarmonicStyle::Strict));
        }
        assert!(!allows(&c, 65, HarmonicStyle::Strict));
        let mut v = VoicingSpec::default();
        v.omitted.push(5);
        for layout in [
            Layout::Closed,
            Layout::Open,
            Layout::Drop2,
            Layout::Rootless,
        ] {
            v.layout = layout;
            let notes = realise("Cmaj7", &v, HarmonicStyle::Strict, &[]).unwrap();
            assert!(!notes.iter().any(|n| n.pitch % 12 == 7));
        }
        assert!(realise("Cmaj7add11", &v, HarmonicStyle::Strict, &[]).is_err());
    }
    #[test]
    fn quartal_is_a_stack_of_fourths_and_refuses_an_impossible_stack() {
        let mut v = VoicingSpec::default();
        v.layout = Layout::Quartal;
        assert!(realise("Cmaj7", &v, HarmonicStyle::Strict, &[]).is_err());
        v.added = vec![
            ChordMember {
                degree: 9,
                semitones: 14,
            },
            ChordMember {
                degree: 13,
                semitones: 21,
            },
        ];
        let notes = realise("Cmaj7", &v, HarmonicStyle::Strict, &[]).unwrap();
        assert_eq!(
            notes.iter().map(|n| n.pitch).collect::<Vec<_>>(),
            [47, 52, 57, 62, 67, 72]
        );
        assert!(notes.windows(2).all(|w| w[1].pitch - w[0].pitch == 5));
    }
    #[test]
    fn spelling_alterations_bounds_and_inversions() {
        assert!(!allows(
            &context("Cmaj7no3").unwrap(),
            65,
            HarmonicStyle::Strict
        ));
        let slash = realise(
            "Cmaj7/G4",
            &VoicingSpec::default(),
            HarmonicStyle::Strict,
            &[],
        )
        .unwrap();
        assert!(slash.iter().any(|n| n.pitch == 67));
        assert_eq!(spell("Dbmaj7", 3, 65), "F4");
        assert_eq!(spell("Cmaj7", 11, 66), "F#4");
        assert_eq!(spell("Cbmaj7", 1, 59), "Cb4");
        let mut v = VoicingSpec::default();
        v.inversion = 1;
        assert_eq!(
            realise("Cmaj7", &v, HarmonicStyle::Strict, &[]).unwrap()[0].pitch,
            52
        );
        v.octave = 9;
        assert!(realise("Cmaj7", &v, HarmonicStyle::Strict, &[]).is_err());
        assert!(parse("Cmaj7(no1,no5)").is_ok());
    }
}
