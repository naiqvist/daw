//! Universal 12-TET material. Names interpret material; they never gate playback.
use super::{ChordBass, ChordSymbol, harmony};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub id: u64,
    pub pc: u8,
    pub pitch: Option<u8>,
    pub spelling: String,
    pub degree: Option<u8>,
    pub offset: i16,
    /// Named polychord component, in written upper-to-lower order.
    #[serde(default)]
    pub component: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Bass {
    Class(u8),
    Pitch(u8),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Material {
    pub members: Vec<Member>,
    pub root: Option<u8>,
    pub bass: Option<Bass>,
    pub source: String,
    #[serde(default)]
    pub components: Vec<String>,
}

pub fn pitch(text: &str) -> Result<(u8, Option<u8>, String), String> {
    if text.len() > 4096 {
        return Err("Pitch entry exceeds 4096 characters".into());
    }
    let normalized = text.trim().replace('♯', "#").replace('♭', "b");
    let mut chars = normalized.char_indices();
    let (_, letter) = chars.next().ok_or("Enter a pitch name")?;
    let natural: i16 = match letter.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return Err(format!("Invalid pitch {text}; use A through G")),
    };
    let mut end = letter.len_utf8();
    let mut accidental = 0i16;
    for (at, ch) in chars {
        match ch {
            '#' => accidental += 1,
            'b' => accidental -= 1,
            _ => break,
        }
        end = at + ch.len_utf8();
    }
    if accidental.abs() > 4 {
        return Err("Use at most four accidentals".into());
    }
    let spelling = format!("{}{}", letter.to_ascii_uppercase(), &normalized[1..end]);
    let pc = (natural + accidental).rem_euclid(12) as u8;
    let absolute = if end == normalized.len() {
        None
    } else {
        let octave: i16 = normalized[end..]
            .parse()
            .map_err(|_| format!("Invalid octave in {text}"))?;
        let n = (i32::from(octave) + 1) * 12 + i32::from(natural + accidental);
        if !(0..=127).contains(&n) {
            return Err(format!("{text} falls outside MIDI 0–127"));
        }
        Some(n as u8)
    };
    Ok((pc, absolute, spelling))
}

fn raw_class(spelling: &str) -> i16 {
    let spelling = spelling.replace('♯', "#").replace('♭', "b");
    let natural = match spelling.chars().next().unwrap_or('C').to_ascii_uppercase() {
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => 0,
    };
    natural
        + spelling
            .chars()
            .skip(1)
            .take_while(|c| *c == '#' || *c == 'b')
            .map(|c| if c == '#' { 1 } else { -1 })
            .sum::<i16>()
}
/// Recover the written octave, including C-flat and B-sharp across C.
pub fn note_name(spelling: &str, pitch: u8) -> String {
    format!(
        "{spelling}{}",
        (i16::from(pitch) - raw_class(spelling)).div_euclid(12) - 1
    )
}

impl Material {
    /// Explicit forms: `notes:C4,E4,G4`, `pc:C,Eb,F#`, `int:C4:0,1,6`.
    /// Symbols and polychords (`D|C`) are conveniences over this same material.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        if text.len() > 4096 {
            return Err("Material entry exceeds 4096 characters".into());
        }
        if text.eq_ignore_ascii_case("rest") {
            return Ok(Self {
                source: "rest".into(),
                ..Self::default()
            });
        }
        if let Some(list) = text
            .strip_prefix("notes:")
            .or_else(|| text.strip_prefix("pc:"))
        {
            let exact = text.starts_with("notes:");
            let mut result = Self {
                source: text.into(),
                ..Self::default()
            };
            for (i, item) in list.split([',', ' ']).filter(|x| !x.is_empty()).enumerate() {
                let (pc, absolute, spelling) = pitch(item)?;
                if exact && absolute.is_none() {
                    return Err(format!("Give an octave for {item}, for example C4"));
                }
                if !exact && absolute.is_some() {
                    return Err("Pitch-class entry takes names without octaves".into());
                }
                result.members.push(Member {
                    id: i as u64 + 1,
                    pc,
                    pitch: absolute,
                    spelling,
                    degree: None,
                    offset: i16::from(pc),
                    component: None,
                });
            }
            if result.members.is_empty() {
                return Err("Enter notes, or use rest for silence".into());
            }
            result.validate()?;
            return Ok(result);
        }
        if let Some(formula) = text.strip_prefix("int:") {
            let (reference, intervals) = formula.split_once(':').ok_or("Use int:C4:0,1,6")?;
            let (_, anchor, _) = pitch(reference)?;
            let anchor = anchor.ok_or("Give the interval reference an octave")?;
            let mut result = Self {
                source: text.into(),
                ..Self::default()
            };
            for (i, item) in intervals.split(',').enumerate() {
                let interval: i16 = item
                    .trim()
                    .parse()
                    .map_err(|_| "Intervals must be integer semitones")?;
                let n = i32::from(anchor) + i32::from(interval);
                if !(0..=127).contains(&n) {
                    return Err("Interval formula falls outside MIDI 0–127".into());
                }
                result.members.push(Member {
                    id: i as u64 + 1,
                    pc: n as u8 % 12,
                    pitch: Some(n as u8),
                    spelling: super::pitch_class_name(n as u8).into(),
                    degree: None,
                    offset: interval,
                    component: None,
                });
            }
            result.validate()?;
            return Ok(result);
        }
        if text.contains('|') {
            let mut result = Self {
                source: text.into(),
                ..Self::default()
            };
            for group in text.split('|') {
                let material = Self::parse(group)?;
                if result.members.len() + material.members.len() > 128 {
                    return Err("Material exceeds 128 members".into());
                }
                let component = result.components.len() as u16;
                result.components.push(group.trim().into());
                for mut member in material.members {
                    member.id = result.members.len() as u64 + 1;
                    member.component = Some(component);
                    result.members.push(member);
                }
            }
            result.validate()?;
            return Ok(result);
        }
        let symbol = harmony::parse(text)?;
        Self::from_symbol(text, &symbol)
    }

    pub fn from_symbol(text: &str, symbol: &ChordSymbol) -> Result<Self, String> {
        let root = i16::from(symbol.root_pc);
        let mut members = Vec::new();
        for (i, m) in symbol.members.iter().enumerate() {
            let pc = (root + m.semitones).rem_euclid(12) as u8;
            let absolute = if let Some(octave) = symbol.root_octave {
                let value = (i32::from(octave) + 1) * 12 + i32::from(raw_class(text) + m.semitones);
                if !(0..=127).contains(&value) {
                    return Err("Explicit chord octave falls outside MIDI 0–127".into());
                }
                Some(value as u8)
            } else {
                None
            };
            let spelled = harmony::spell(text, m.degree, 60 + pc);
            let spelling = spelled
                .trim_end_matches(|c: char| c.is_ascii_digit() || c == '-')
                .to_owned();
            members.push(Member {
                id: i as u64 + 1,
                pc,
                pitch: absolute,
                spelling,
                degree: Some(m.degree),
                offset: m.semitones,
                component: None,
            });
        }
        let bass = match symbol.bass {
            Some(ChordBass::Pitch {
                pitch_class,
                octave: Some(o),
            }) => {
                let value = text
                    .rsplit_once('/')
                    .and_then(|(_, bass)| pitch(bass).ok())
                    .and_then(|(_, absolute, _)| absolute)
                    .map_or((i32::from(o) + 1) * 12 + i32::from(pitch_class), i32::from);
                if !(0..=127).contains(&value) {
                    return Err("Bass falls outside MIDI 0–127".into());
                }
                Some(Bass::Pitch(value as u8))
            }
            Some(ChordBass::Pitch {
                pitch_class,
                octave: None,
            }) => Some(Bass::Class(pitch_class)),
            Some(ChordBass::Member(degree)) => Some(Bass::Class(
                members
                    .iter()
                    .find(|m| m.degree == Some(degree))
                    .ok_or("Slash bass names an absent member")?
                    .pc,
            )),
            None => None,
        };
        let result = Self {
            members,
            root: Some(symbol.root_pc),
            bass,
            source: text.into(),
            components: vec![],
        };
        result.validate()?;
        Ok(result)
    }

    pub fn from_mask(mask: u16) -> Self {
        let members = (0u8..12)
            .filter(|pc| mask & (1 << pc) != 0)
            .map(|pc| Member {
                id: u64::from(pc) + 1,
                pc,
                pitch: None,
                spelling: super::pitch_class_name(pc).into(),
                degree: None,
                offset: i16::from(pc),
                component: None,
            })
            .collect();
        Self {
            members,
            root: None,
            bass: None,
            source: String::new(),
            components: vec![],
        }
    }

    pub fn mask(&self) -> u16 {
        self.members
            .iter()
            .fold(0, |mask, member| mask | (1 << member.pc.min(11)))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.members.len() > 128 {
            return Err("Material exceeds 128 members".into());
        }
        if self.components.len() > 128 || self.components.iter().any(|name| name.len() > 4096) {
            return Err("Polychord component labels exceed capacity".into());
        }
        if self.root.is_some_and(|p| p > 11) {
            return Err("Root must be a pitch class 0–11".into());
        }
        if self.bass.is_some_and(|b| match b {
            Bass::Class(p) => p > 11,
            Bass::Pitch(p) => p > 127,
        }) {
            return Err("Invalid bass pitch".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for member in &self.members {
            if member
                .component
                .is_some_and(|i| usize::from(i) >= self.components.len())
            {
                return Err("Member refers to a missing polychord component".into());
            }
            if member.pc > 11 || member.pitch.is_some_and(|p| p > 127 || p % 12 != member.pc) {
                return Err("Member pitch and pitch class disagree".into());
            }
            if !ids.insert(member.id) {
                return Err("Duplicate member identity".into());
            }
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        if !self.source.is_empty() {
            return self.source.clone();
        }
        if self.members.is_empty() {
            return "rest".into();
        }
        self.members
            .iter()
            .map(|m| m.spelling.clone())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// Exact pitches stay exact. Unspecified pitches choose a stated register.
    pub fn realise(&self, low: u8, high: u8, center: u8) -> Result<Vec<(u64, u8)>, String> {
        self.validate()?;
        if low > high || high > 127 {
            return Err("Invalid MIDI register".into());
        }
        let mut notes = Vec::new();
        let anchor = (i16::from(center) / 12) * 12 + i16::from(self.root.unwrap_or(0));
        for member in &self.members {
            let target = if self.root.is_some() {
                anchor + member.offset
            } else {
                (i16::from(center) / 12) * 12 + i16::from(member.pc)
            };
            let n = if let Some(p) = member.pitch {
                if p < low || p > high {
                    return Err(format!("Explicit pitch {p} is outside {low}–{high}"));
                }
                p
            } else {
                (low..=high)
                    .filter(|p| p % 12 == member.pc)
                    .min_by_key(|p| ((i16::from(*p) - target).abs(), *p))
                    .ok_or_else(|| format!("{} has no pitch in {low}–{high}", member.spelling))?
            };
            notes.push((member.id, n));
        }
        if let Some(bass) = self.bass {
            let floor = notes.iter().map(|(_, p)| *p).min().unwrap_or(high);
            let p = match bass {
                Bass::Pitch(p) if p >= low && p <= high && p <= floor => p,
                Bass::Pitch(_) => {
                    return Err("Explicit bass must fit the range below the upper voices".into());
                }
                Bass::Class(pc) => (low..=floor.min(high))
                    .rev()
                    .find(|p| p % 12 == pc)
                    .ok_or("No slash bass fits below the chord")?,
            };
            if !notes.iter().any(|(_, n)| *n == p) {
                notes.push((u64::MAX, p));
            }
        }
        notes.sort_by_key(|(id, p)| (*p, *id));
        Ok(notes)
    }

    pub fn transpose(&mut self, semitones: i16) -> Result<(), String> {
        let mut next = self.clone();
        for m in &mut next.members {
            if let Some(p) = m.pitch {
                let n = i32::from(p) + i32::from(semitones);
                if !(0..=127).contains(&n) {
                    return Err("Transposition exceeds MIDI range".into());
                }
                m.pitch = Some(n as u8);
            }
            m.pc = (i32::from(m.pc) + i32::from(semitones)).rem_euclid(12) as u8;
            m.spelling = super::pitch_class_name(m.pc).into();
        }
        next.root = next
            .root
            .map(|p| (i32::from(p) + i32::from(semitones)).rem_euclid(12) as u8);
        next.bass = match next.bass {
            Some(Bass::Pitch(p)) => {
                let n = i32::from(p) + i32::from(semitones);
                if !(0..=127).contains(&n) {
                    return Err("Bass transposition exceeds MIDI range".into());
                }
                Some(Bass::Pitch(n as u8))
            }
            Some(Bass::Class(p)) => Some(Bass::Class(
                (i32::from(p) + i32::from(semitones)).rem_euclid(12) as u8,
            )),
            None => None,
        };
        next.source.clear();
        *self = next;
        Ok(())
    }

    pub fn readings(&self) -> Vec<(u8, String)> {
        let mut result = Vec::new();
        for root in 0..12 {
            for (suffix, intervals) in [
                ("", &[0, 4, 7][..]),
                ("m", &[0, 3, 7]),
                ("dim", &[0, 3, 6]),
                ("aug", &[0, 4, 8]),
                ("sus2", &[0, 2, 7]),
                ("sus4", &[0, 5, 7]),
                ("6", &[0, 4, 7, 9]),
                ("m7", &[0, 3, 7, 10]),
                ("maj7", &[0, 4, 7, 11]),
                ("7", &[0, 4, 7, 10]),
                ("m7b5", &[0, 3, 6, 10]),
                ("dim7", &[0, 3, 6, 9]),
            ] {
                let mask = intervals
                    .iter()
                    .fold(0u16, |mask, i| mask | (1 << ((root + i) % 12)));
                if mask == self.mask() {
                    result.push((root, format!("{}{suffix}", super::pitch_class_name(root))));
                }
            }
        }
        result.sort_by_key(|(root, label)| (self.root != Some(*root), label.clone()));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn written_octaves_cross_enharmonic_c_boundaries() {
        let c = Material::parse("Cb4maj7/B#2").unwrap();
        assert_eq!(c.members[0].pitch, Some(59));
        assert_eq!(c.bass, Some(Bass::Pitch(48)));
        assert_eq!(note_name("Cb", 59), "Cb4");
        assert_eq!(note_name("B#", 60), "B#3");
    }
    #[test]
    fn named_polychord_components_survive_roundtrip_and_transposition() {
        let mut c = Material::parse("D|C").unwrap();
        assert_eq!(c.components, ["D", "C"]);
        assert_eq!(
            c.members.iter().map(|m| m.component).collect::<Vec<_>>(),
            [Some(0), Some(0), Some(0), Some(1), Some(1), Some(1)]
        );
        c.transpose(2).unwrap();
        assert_eq!(
            ron::from_str::<Material>(&ron::to_string(&c).unwrap()).unwrap(),
            c
        );
        assert_eq!(c.members[0].component, Some(0));
    }
    #[test]
    fn every_pitch_class_collection_can_sound_without_a_root_or_name() {
        for mask in 1..4096 {
            let m = Material::from_mask(mask);
            let notes = m.realise(48, 71, 60).unwrap();
            assert_eq!(
                notes
                    .iter()
                    .fold(0u16, |mask, (_, p)| mask | (1 << (p % 12))),
                mask
            );
            assert_eq!(notes.len(), mask.count_ones() as usize);
            assert_eq!(notes, m.realise(48, 71, 60).unwrap());
            assert_eq!(m.root, None);
        }
    }
    #[test]
    fn explicit_notes_keep_octaves_spelling_and_unisons() {
        let m = Material::parse("notes:Cb4,B3,Cb5,E#5").unwrap();
        assert_eq!(
            m.realise(0, 127, 48)
                .unwrap()
                .iter()
                .map(|x| x.1)
                .collect::<Vec<_>>(),
            [59, 59, 71, 77]
        );
        assert_eq!(m.members[0].spelling, "Cb");
        assert!(m.realise(60, 127, 48).is_err());
        assert_eq!(
            ron::from_str::<Material>(&ron::to_string(&m).unwrap()).unwrap(),
            m
        );
    }
    #[test]
    fn ambiguity_does_not_change_the_sound() {
        let m = Material::parse("pc:C,E,G,A").unwrap();
        assert_eq!(
            m.readings()
                .iter()
                .map(|x| x.1.as_str())
                .collect::<Vec<_>>(),
            ["Am7", "C6"]
        );
        assert_eq!(m.root, None);
        assert!(Material::parse("int:C4:0,1,6").is_ok());
        assert!(Material::parse("notes:C10").is_err());
        assert!(Material::parse("C9maj7").is_err());
    }
}
