//! MIDI Lab authors immutable, tick-addressed clips in the green zone.
//! The same result feeds preview, the geometry, export and Send.
pub mod audition;
pub mod composer;
pub mod generate;
use crate::sequencing::{
    DEFAULT_PATTERN_TICKS, Note, PATTERN_STEP_TICKS, Pattern, PatternId, TrackId,
};
use crate::theory::harmony::{HarmonicStyle, VoicingSpec};
pub use generate::{Generated, generate};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destination {
    pub track: TrackId,
    pub pattern: PatternId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub id: u64,
    pub destination: Option<Destination>,
    pub recipe: Recipe,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Harmony {
    pub id: u64,
    pub symbol: String,
    pub start: usize,
    pub length: usize,
    pub voicing: VoicingSpec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Voice {
    Chords,
    Arp,
    Melody,
    Bass,
    Counterpoint,
}
impl Voice {
    pub const ALL: [Self; 5] = [
        Self::Chords,
        Self::Arp,
        Self::Melody,
        Self::Bass,
        Self::Counterpoint,
    ];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&v| v == self).unwrap_or(0)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Chords => "Chords",
            Self::Arp => "Arpeggio",
            Self::Melody => "Melody",
            Self::Bass => "Bass",
            Self::Counterpoint => "Counterpoint",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rhythm {
    #[default]
    Hold,
    Quarter,
    Eighth,
    Sixteenth,
    Triplet,
    Syncopated,
    Euclidean,
    Custom,
}
impl Rhythm {
    pub const ALL: [Self; 8] = [
        Self::Hold,
        Self::Quarter,
        Self::Eighth,
        Self::Sixteenth,
        Self::Triplet,
        Self::Syncopated,
        Self::Euclidean,
        Self::Custom,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Hold => "At chord changes",
            Self::Quarter => "Quarter notes",
            Self::Eighth => "Eighth notes",
            Self::Sixteenth => "Sixteenth notes",
            Self::Triplet => "Eighth triplets",
            Self::Syncopated => "Syncopated",
            Self::Euclidean => "Euclidean",
            Self::Custom => "Drawn rhythm",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gate {
    pub id: u64,
    pub start: usize,
    pub length: usize,
    pub velocity: u8,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceSpec {
    pub enabled: bool,
    pub rhythm: Rhythm,
    pub gate: u8,
    pub swing: u8,
    pub low: u8,
    pub high: u8,
    pub velocity: u8,
    /// 0: up / lyrical / roots; 1: down / angular / walking;
    /// 2: pendulum / motif / pedal; 3: seeded variation.
    pub motion: u8,
    pub pulses: u8,
    pub steps: u8,
    pub rotation: u8,
    pub seed: u64,
    pub custom: Vec<Gate>,
}
impl VoiceSpec {
    fn new(voice: Voice) -> Self {
        Self {
            enabled: voice == Voice::Chords,
            rhythm: match voice {
                Voice::Chords => Rhythm::Hold,
                Voice::Bass => Rhythm::Quarter,
                _ => Rhythm::Eighth,
            },
            gate: 80,
            swing: 50,
            low: if voice == Voice::Bass { 28 } else { 48 },
            high: if voice == Voice::Bass { 52 } else { 84 },
            velocity: if voice == Voice::Chords { 86 } else { 94 },
            motion: 0,
            pulses: 5,
            steps: 8,
            rotation: 0,
            seed: 1,
            custom: vec![],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: u64,
    pub voice: Voice,
    pub pitch: u8,
    pub start: usize,
    pub length: usize,
    pub velocity: u8,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recipe {
    /// Versioned composer document. Absence retains the original generator for
    /// legacy projects until their exact events have been snapshotted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition: Option<Box<composer::Composition>>,
    pub length: usize,
    pub harmony: Vec<Harmony>,
    pub voices: [VoiceSpec; 5],
    pub style: HarmonicStyle,
    /// Pinned/manual notes replace the generated event with the same id.
    pub pinned: Vec<Event>,
    pub removed: Vec<u64>,
    pub next_id: u64,
}
impl Default for Recipe {
    fn default() -> Self {
        Self {
            composition: None,
            length: 384,
            harmony: vec![
                Harmony {
                    id: 1,
                    symbol: "Cmaj7".into(),
                    start: 0,
                    length: 192,
                    voicing: VoicingSpec::default(),
                },
                Harmony {
                    id: 2,
                    symbol: "Am7".into(),
                    start: 192,
                    length: 96,
                    voicing: VoicingSpec::default(),
                },
                Harmony {
                    id: 3,
                    symbol: "Dm7".into(),
                    start: 288,
                    length: 48,
                    voicing: VoicingSpec::default(),
                },
                Harmony {
                    id: 4,
                    symbol: "G7".into(),
                    start: 336,
                    length: 48,
                    voicing: VoicingSpec::default(),
                },
            ],
            voices: Voice::ALL.map(VoiceSpec::new),
            style: HarmonicStyle::Strict,
            pinned: vec![],
            removed: vec![],
            next_id: 100,
        }
    }
}
impl Recipe {
    pub fn composed() -> Self {
        Self { composition: Some(Box::new(composer::Composition::default())), ..Self::default() }
    }
    pub fn mint(&mut self) -> u64 {
        self.next_id = self.next_id.wrapping_add(1).max(100);
        self.next_id
    }
    /// Lead-sheet entries: Cmaj7:4 Am7:2 Dm7:1 G7:1. Durations are beats.
    pub fn progression(&mut self, text: &str) -> Result<(), String> {
        let mut next = Vec::new();
        let mut start = 0;
        for word in text.split_whitespace() {
            let (symbol, beats) = word.rsplit_once(':').unwrap_or((word, "4"));
            crate::theory::harmony::parse(symbol)?;
            let beats = beats
                .parse::<f64>()
                .map_err(|_| "Use chord:beats, for example Cmaj7:4")?;
            if !beats.is_finite() || beats <= 0.0 {
                return Err("Chord durations must be positive".into());
            }
            let length = (beats * 48.).round() as usize;
            if length == 0 || start + length > DEFAULT_PATTERN_TICKS {
                return Err("A clip holds up to 16 beats; use shorter durations".into());
            }
            next.push(Harmony {
                id: self.next_id + next.len() as u64 + 1,
                symbol: symbol.into(),
                start,
                length,
                voicing: VoicingSpec::default(),
            });
            start += length;
        }
        if next.is_empty() {
            return Err("Enter at least one chord".into());
        }
        self.next_id += next.len() as u64;
        self.harmony = next;
        self.length = start;
        Ok(())
    }
    pub fn harmony_at(&self, tick: usize) -> Option<&Harmony> {
        self.harmony
            .iter()
            .find(|h| tick >= h.start && tick < h.start + h.length)
    }
    pub fn pin(&mut self, event: Event) {
        self.removed.retain(|id| *id != event.id);
        self.pinned.retain(|n| n.id != event.id);
        self.pinned.push(event);
    }
    pub fn delete_note(&mut self, id: u64) {
        self.pinned.retain(|n| n.id != id);
        if !self.removed.contains(&id) {
            self.removed.push(id);
        }
    }
}

/// Keeps clip identity/tag and its host instrument. MIDI Lab authors straight
/// tick events, including swing, so inherited pattern swing/scale are reset.
pub fn write_pattern(pattern: &mut Pattern, recipe: &Recipe, notes: &[Event]) {
    let mut next = Pattern::empty(pattern.id, pattern.name.clone());
    next.tag = pattern.tag.clone();
    next.length_ticks = recipe.length;
    next.midi_lab = Some(recipe.clone());
    for n in notes {
        if n.start >= recipe.length || n.start >= DEFAULT_PATTERN_TICKS {
            continue;
        }
        let mut note = Note::new(n.pitch, n.length.min(recipe.length - n.start), n.velocity);
        note.micro_ticks = (n.start % PATTERN_STEP_TICKS) as i16;
        next.trig_mut(n.start / PATTERN_STEP_TICKS)
            .add_tone_at(note);
    }
    *pattern = next;
}
