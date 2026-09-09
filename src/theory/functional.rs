//! Optional tonal context and explicit harmonic operations. Green-zone, UI-free.
use super::material::Material;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Ionian,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Aeolian,
    Locrian,
    HarmonicMinor,
    MelodicMinor,
    WholeTone,
    Octatonic,
    MajorPentatonic,
    MinorPentatonic,
    Chromatic,
}
impl Mode {
    pub const ALL: [Self; 14] = [
        Self::Ionian,
        Self::Dorian,
        Self::Phrygian,
        Self::Lydian,
        Self::Mixolydian,
        Self::Aeolian,
        Self::Locrian,
        Self::HarmonicMinor,
        Self::MelodicMinor,
        Self::WholeTone,
        Self::Octatonic,
        Self::MajorPentatonic,
        Self::MinorPentatonic,
        Self::Chromatic,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Ionian => "Major / Ionian",
            Self::Dorian => "Dorian",
            Self::Phrygian => "Phrygian",
            Self::Lydian => "Lydian",
            Self::Mixolydian => "Mixolydian",
            Self::Aeolian => "Natural minor",
            Self::Locrian => "Locrian",
            Self::HarmonicMinor => "Harmonic minor",
            Self::MelodicMinor => "Melodic minor (ascending)",
            Self::WholeTone => "Whole tone",
            Self::Octatonic => "Octatonic (half–whole)",
            Self::MajorPentatonic => "Major pentatonic",
            Self::MinorPentatonic => "Minor pentatonic",
            Self::Chromatic => "Chromatic",
        }
    }
    pub fn degrees(self) -> &'static [u8] {
        match self {
            Self::Ionian => &[0, 2, 4, 5, 7, 9, 11],
            Self::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Self::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            Self::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            Self::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            Self::Aeolian => &[0, 2, 3, 5, 7, 8, 10],
            Self::Locrian => &[0, 1, 3, 5, 6, 8, 10],
            Self::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            Self::MelodicMinor => &[0, 2, 3, 5, 7, 9, 11],
            Self::WholeTone => &[0, 2, 4, 6, 8, 10],
            Self::Octatonic => &[0, 1, 3, 4, 6, 7, 9, 10],
            Self::MajorPentatonic => &[0, 2, 4, 7, 9],
            Self::MinorPentatonic => &[0, 3, 5, 7, 10],
            Self::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    pub tonic: u8,
    pub mode: Mode,
    /// Optional ordered scale relative to tonic; spelling is retained on material.
    pub custom: Vec<u8>,
    pub descending_minor: bool,
}
impl Key {
    pub fn degrees(&self, descending: bool) -> &[u8] {
        if !self.custom.is_empty() {
            &self.custom
        } else if descending && self.descending_minor && self.mode == Mode::MelodicMinor {
            Mode::Aeolian.degrees()
        } else {
            self.mode.degrees()
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.tonic > 11
            || self.custom.len() > 12
            || self.custom.iter().any(|n| *n > 11)
            || self.custom.windows(2).any(|w| w[0] >= w[1])
        {
            return Err("Key needs ordered, unique pitch classes 0–11".into());
        }
        Ok(())
    }
    pub fn contains(&self, p: u8) -> bool {
        self.degrees(false)
            .contains(&((p % 12 + 12 - self.tonic % 12) % 12))
    }
    pub fn step(&self, pitch: u8, by: i16) -> Result<u8, String> {
        self.validate()?;
        let degrees = self.degrees(by < 0);
        let rel = i32::from(pitch) - i32::from(self.tonic);
        let at = degrees
            .iter()
            .position(|p| i32::from(*p) == rel.rem_euclid(12))
            .ok_or("Diatonic transform requires source notes in the selected collection")?;
        let index = rel.div_euclid(12) * degrees.len() as i32 + at as i32 + i32::from(by);
        let n = i32::from(self.tonic)
            + index.div_euclid(degrees.len() as i32) * 12
            + i32::from(degrees[index.rem_euclid(degrees.len() as i32) as usize]);
        if !(0..=127).contains(&n) {
            return Err("Diatonic transform exceeds MIDI range".into());
        }
        Ok(n as u8)
    }
    pub fn label(&self) -> String {
        format!(
            "{} {}",
            super::pitch_class_name(self.tonic),
            if self.custom.is_empty() {
                self.mode.label()
            } else {
                "custom collection"
            }
        )
    }
}

/// Functional entry supports accidentals, quality suffixes, secondary function
/// (`V7/ii`) and explicit parallel borrowing (`iv@Aeolian`).
pub fn from_function(text: &str, key: &Key) -> Result<Material, String> {
    if text.len() > 4096 {
        return Err("Function entry exceeds 4096 characters".into());
    }
    key.validate()?;
    let (function, borrowed) = text
        .split_once('@')
        .map_or((text, None), |(f, m)| (f, Some(m)));
    let mut context = key.clone();
    if let Some(mode) = borrowed {
        context.mode = Mode::ALL
            .into_iter()
            .find(|m| {
                m.label().eq_ignore_ascii_case(mode) || format!("{m:?}").eq_ignore_ascii_case(mode)
            })
            .ok_or("Unknown parallel mode")?;
        context.custom.clear();
    }
    if let Some((left, right)) = function.split_once('/') {
        if right.contains('/') {
            return Err("Secondary function supports one explicit target".into());
        }
        let target = from_function(right, &context)?;
        let tonic = target.root.ok_or("Secondary target needs a root")?;
        return from_function(
            left,
            &Key {
                tonic,
                ..Key::default()
            },
        );
    }
    let count = function
        .chars()
        .take_while(|c| *c == 'b' || *c == '#')
        .count();
    let accidental = function[..count]
        .bytes()
        .fold(0i16, |n, c| n + if c == b'#' { 1 } else { -1 });
    let tail = &function[count..];
    let end = tail
        .chars()
        .take_while(|c| matches!(c, 'i' | 'I' | 'v' | 'V'))
        .count();
    let numeral = &tail[..end];
    let degree = match numeral.to_ascii_uppercase().as_str() {
        "I" => 0,
        "II" => 1,
        "III" => 2,
        "IV" => 3,
        "V" => 4,
        "VI" => 5,
        "VII" => 6,
        _ => return Err("Use a roman numeral I–VII with optional quality".into()),
    };
    if context.degrees(false).len() != 7 {
        return Err("Roman function requires a seven-degree context".into());
    }
    let root = (i16::from(context.tonic) + i16::from(context.degrees(false)[degree]) + accidental)
        .rem_euclid(12) as u8;
    let suffix = tail[end..].replace(['º', '°'], "dim").replace('ø', "m7b5");
    let minor = numeral.chars().all(|c| c.is_lowercase());
    let quality = if suffix.starts_with("dim") || suffix.starts_with("m7b5") {
        String::new()
    } else if minor {
        "m".into()
    } else {
        String::new()
    };
    Material::parse(&format!(
        "{}{quality}{suffix}",
        super::pitch_class_name(root)
    ))
}

pub fn analysis(material: &Material, key: &Key) -> Vec<String> {
    if key.validate().is_err() || key.degrees(false).len() != 7 {
        return vec![];
    }
    let roots = if let Some(root) = material.root {
        vec![root]
    } else {
        material.readings().iter().map(|x| x.0).collect()
    };
    let mut result = Vec::new();
    for root in roots {
        let relative = (root + 12 - key.tonic) % 12;
        let degree = (0..7)
            .min_by_key(|i| {
                let delta = (i16::from(relative) - i16::from(key.degrees(false)[*i]) + 6)
                    .rem_euclid(12)
                    - 6;
                (delta.abs(), *i)
            })
            .unwrap_or(0);
        let delta =
            (i16::from(relative) - i16::from(key.degrees(false)[degree]) + 6).rem_euclid(12) - 6;
        let has = |interval: u8| material.mask() & (1 << ((root + interval) % 12)) != 0;
        let mut numeral = ["I", "II", "III", "IV", "V", "VI", "VII"][degree].to_owned();
        if has(3) && !has(4) {
            numeral = numeral.to_lowercase();
        }
        let accidental = if delta < 0 {
            "b".repeat((-delta) as usize)
        } else {
            "#".repeat(delta as usize)
        };
        let quality = if has(3) && has(6) && !has(7) {
            if has(10) {
                "ø7"
            } else if has(9) {
                "º7"
            } else {
                "º"
            }
        } else if has(11) {
            "maj7"
        } else if has(10) {
            "7"
        } else {
            ""
        };
        result.push(format!("{accidental}{numeral}{quality}"));
    }
    result.sort();
    result.dedup();
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cadence {
    Authentic,
    Half,
    Plagal,
    Deceptive,
    Phrygian,
}
impl Cadence {
    pub const ALL: [Self; 5] = [
        Self::Authentic,
        Self::Half,
        Self::Plagal,
        Self::Deceptive,
        Self::Phrygian,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Authentic => "Authentic",
            Self::Half => "Half",
            Self::Plagal => "Plagal",
            Self::Deceptive => "Deceptive",
            Self::Phrygian => "Phrygian",
        }
    }
    pub fn chords(self, key: &Key) -> Result<Vec<Material>, String> {
        let minor = matches!(
            key.mode,
            Mode::Aeolian | Mode::HarmonicMinor | Mode::MelodicMinor
        );
        let functions: &[&str] = match self {
            Self::Authentic => {
                if minor {
                    &["iv", "V7", "i"]
                } else {
                    &["ii", "V7", "I"]
                }
            }
            Self::Half => {
                if minor {
                    &["i", "iv", "V"]
                } else {
                    &["I", "ii", "V"]
                }
            }
            Self::Plagal => {
                if minor {
                    &["iv", "i"]
                } else {
                    &["IV", "I"]
                }
            }
            Self::Deceptive => {
                if minor {
                    &["V7", "VI"]
                } else {
                    &["V7", "vi"]
                }
            }
            Self::Phrygian => {
                if !minor {
                    return Err("Phrygian half cadence requires minor context".into());
                }
                &["iv", "V"]
            }
        };
        let mut result = functions
            .iter()
            .map(|f| from_function(f, key))
            .collect::<Result<Vec<_>, _>>()?;
        if self == Self::Phrygian {
            let chord = &mut result[0];
            let third = chord
                .members
                .iter()
                .find(|m| m.degree == Some(3))
                .ok_or("Cadence needs a third")?
                .pc;
            chord.bass = Some(super::material::Bass::Class(third));
        }
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Substitution {
    Tritone,
    Relative,
    ChromaticMediant,
    AddedSixth,
    UpperStructure,
}
impl Substitution {
    pub const ALL: [Self; 5] = [
        Self::Tritone,
        Self::Relative,
        Self::ChromaticMediant,
        Self::AddedSixth,
        Self::UpperStructure,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Tritone => "Tritone",
            Self::Relative => "Relative",
            Self::ChromaticMediant => "Chromatic mediant",
            Self::AddedSixth => "Added sixth",
            Self::UpperStructure => "Upper structure",
        }
    }
    pub fn apply(self, source: &Material) -> Result<Material, String> {
        let root = source
            .root
            .ok_or("Select a harmonic root for this substitution")?;
        let has = |n: u8| source.mask() & (1 << ((root + n) % 12)) != 0;
        let name = super::pitch_class_name;
        match self {
            Self::Tritone => {
                if !has(4) || !has(10) {
                    return Err("Tritone substitution requires dominant-seventh intent".into());
                }
                Material::parse(&format!("{}7", name((root + 6) % 12)))
            }
            Self::Relative => Material::parse(&format!(
                "{}{}",
                name((root + if has(3) && !has(4) { 3 } else { 9 }) % 12),
                if has(3) && !has(4) { "" } else { "m" }
            )),
            Self::ChromaticMediant => Material::parse(&format!(
                "{}{}",
                name((root + 4) % 12),
                if has(3) && !has(4) { "m" } else { "" }
            )),
            Self::AddedSixth => {
                let mut next = source.clone();
                let pc = (root + 9) % 12;
                if next.members.iter().any(|m| m.pc == pc) {
                    return Err("The sixth is already present".into());
                }
                let id = next.members.iter().map(|m| m.id).max().unwrap_or(0) + 1;
                next.members.push(super::material::Member {
                    component: None,
                    id,
                    pc,
                    pitch: None,
                    spelling: name(pc).into(),
                    degree: Some(6),
                    offset: 9,
                });
                next.source.clear();
                Ok(next)
            }
            Self::UpperStructure => {
                Material::parse(&format!("{}|{}", name((root + 2) % 12), source.label()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn function_and_secondary_targets() {
        let key = Key::default();
        assert_eq!(from_function("ii7", &key).unwrap().label(), "Dm7");
        assert_eq!(from_function("V7/ii", &key).unwrap().root, Some(9));
        assert_eq!(
            from_function("iv@Aeolian", &key).unwrap().mask(),
            Material::parse("Fm").unwrap().mask()
        );
        assert!(from_function("VIII", &key).is_err());
    }
    #[test]
    fn every_mode_has_an_ordered_collection() {
        for mode in Mode::ALL {
            let key = Key {
                mode,
                ..Key::default()
            };
            assert!(key.degrees(false).windows(2).all(|w| w[0] < w[1]));
            for p in 24..100 {
                if key.contains(p) {
                    assert_eq!(key.step(p, 1).and_then(|p| key.step(p, -1)).unwrap(), p);
                }
            }
        }
    }
    #[test]
    fn simultaneous_alterations_survive_symbol_entry() {
        let m = Material::parse("C7b9#9").unwrap();
        assert!(m.members.iter().any(|m| m.offset == 13));
        assert!(m.members.iter().any(|m| m.offset == 15));
    }
    #[test]
    fn substitutions_refuse_inapplicable_harmony() {
        assert!(
            Substitution::Tritone
                .apply(&Material::parse("C").unwrap())
                .is_err()
        );
        assert_eq!(
            Substitution::Tritone
                .apply(&Material::parse("G7").unwrap())
                .unwrap()
                .root,
            Some(1)
        );
        assert!(Cadence::Phrygian.chords(&Key::default()).is_err());
    }
}
