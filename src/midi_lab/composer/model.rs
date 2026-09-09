use crate::midi_lab::Voice;
use crate::theory::{functional::Key, harmony::Layout, material::Material};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const FORMAT_VERSION: u16 = 2;
pub const ENGINE_VERSION: u16 = 1;
pub const PPQ: u32 = 48;
pub const MAX_TICKS: u32 = 192 * 256;
pub const MAX_EVENTS: usize = 32768;

macro_rules! choices {
    ($name:ident, $default:ident, $($variant:ident => $label:literal),+ $(,)?) => {
        #[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize,Deserialize)]
        pub enum $name {$($variant),+}
        impl Default for $name {fn default()->Self{Self::$default}}
        impl $name {
            pub const ALL:&'static [Self]=&[$(Self::$variant),+];
            pub fn label(self)->&'static str{match self{$(Self::$variant=>$label),+}}
            pub fn cycle(self,by:i32)->Self{let at=Self::ALL.iter().position(|v|*v==self).unwrap_or(0);Self::ALL[(at as i32+by).rem_euclid(Self::ALL.len() as i32)as usize]}
        }
    }
}
choices!(RhythmKind,Hold,Hold=>"Hold",Quarter=>"Quarter notes",Eighth=>"Eighth notes",Sixteenth=>"Sixteenth notes",Triplet=>"Eighth triplets",Syncopated=>"Syncopated",Tresillo=>"Tresillo",Offbeat=>"Offbeats",Euclidean=>"Euclidean",Custom=>"Written cell");
choices!(Contour,Arch,Rise=>"Rising",Fall=>"Falling",Arch=>"Arch",Valley=>"Valley",Plateau=>"Plateau",Waves=>"Alternating peaks",Drawn=>"Drawn contour");
choices!(Movement,Steps,Steps=>"Stepwise",Arpeggio=>"Arpeggio",Repeated=>"Repeated notes",Sequence=>"Interval sequence",LeapRecover=>"Leap and recover");
choices!(Decoration,None,None=>"Chord members",Passing=>"Passing tones",Neighbour=>"Neighbours",Enclosure=>"Enclosures",Chromatic=>"Chromatic approach",Suspension=>"Suspensions",Anticipation=>"Anticipations",Escape=>"Escape tones");
choices!(TargetKind,Members,Members=>"Chord members",Root=>"Root arrivals",Third=>"Thirds",Seventh=>"Sevenths",Common=>"Common tones",Exact=>"Exact arrival");
choices!(Development,Answer,Repeat=>"Repeat",Answer=>"Statement and answer",Sequence=>"Sequence upward",Invert=>"Inverted answer",Fragment=>"Fragmented answer",Contrast=>"Contrasting response");
choices!(BassRole,Foundation,Foundation=>"Foundation",Pedal=>"Pedal",Riff=>"Riff",Walking=>"Walking",Melodic=>"Melodic",Sub=>"Sub");
choices!(Groove,Independent,Independent=>"Independent",Reinforce=>"Reinforce kick",Answer=>"Answer kick");
choices!(BassStyle,Support,Support=>"Harmonic support",Rock=>"Rock pulse",Funk=>"Syncopated funk",Jazz=>"Jazz walking",Electronic=>"Electronic sub");
choices!(Articulation,Normal,Normal=>"Natural",Short=>"Short",Connected=>"Connected",Legato=>"Legato overlap",Accent=>"Metrical accents");
choices!(Species,Off,Off=>"Off",First=>"First species",Second=>"Second species",Third=>"Third species",Fourth=>"Fourth species",Fifth=>"Fifth species",Canon=>"Canon");
choices!(PitchFrame,Absolute,Absolute=>"Absolute pitches",Chromatic=>"Chromatic intervals",Diatonic=>"Scale steps",ChordRoles=>"Chord member roles");
choices!(Pivot,Direct,Direct=>"Direct",CommonChord=>"Common chord",CommonTone=>"Common tone",Chromatic=>"Chromatic");
choices!(OutputPolicy,Faithful,Faithful=>"Faithful",Merge=>"Merge coincident notes");
choices!(VariationAction,NewPitches,NewPitches=>"Same rhythm, new pitches",NewRhythm=>"Same pitches, new rhythm",Answer=>"Keep opening, change answer",Approach=>"Keep targets, change approach",Simplify=>"Simplify",Range=>"Increase range",Develop=>"Develop motif",Fill=>"Change bass fill",Space=>"Leave more space");

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meter {
    pub numerator: u8,
    pub denominator: u8,
    pub groups: Vec<u8>,
}
impl Default for Meter {
    fn default() -> Self {
        Self {
            numerator: 4,
            denominator: 4,
            groups: vec![2, 2],
        }
    }
}
impl Meter {
    pub fn bar(&self) -> Result<u32, String> {
        if self.numerator == 0
            || self.numerator > 32
            || !matches!(self.denominator, 1 | 2 | 4 | 8 | 16)
            || self.groups.iter().any(|n| *n == 0)
            || (!self.groups.is_empty()
                && self.groups.iter().map(|n| u32::from(*n)).sum::<u32>()
                    != u32::from(self.numerator))
        {
            return Err("Invalid metre or beat grouping".into());
        }
        Ok(PPQ * 4 * u32::from(self.numerator) / u32::from(self.denominator))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pulse {
    pub id: u64,
    pub start: u32,
    pub length: u32,
    pub velocity: u8,
    pub tied: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RhythmSpec {
    pub kind: RhythmKind,
    pub gate: u8,
    pub swing: u8,
    pub rotation: u16,
    pub pulses: u8,
    pub steps: u8,
    pub custom_length: u32,
    pub custom: Vec<Pulse>,
    pub offsets: Vec<i16>,
    pub accents: Vec<i16>,
}
impl Default for RhythmSpec {
    fn default() -> Self {
        Self {
            kind: RhythmKind::Hold,
            gate: 80,
            swing: 50,
            rotation: 0,
            pulses: 5,
            steps: 8,
            custom_length: 192,
            custom: vec![],
            offsets: vec![],
            accents: vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MelodySpec {
    pub contour: Contour,
    pub movement: Movement,
    pub decoration: Decoration,
    pub targets: TargetKind,
    pub arrival: Option<u8>,
    pub development: Development,
    pub phrase_bars: u8,
    pub variation: u16,
    pub max_leap: u8,
    pub curve: Vec<(u16, i16)>,
    pub interval: i16,
}
impl Default for MelodySpec {
    fn default() -> Self {
        Self {
            contour: Contour::Arch,
            movement: Movement::Steps,
            decoration: Decoration::None,
            targets: TargetKind::Members,
            arrival: None,
            development: Development::Answer,
            phrase_bars: 4,
            variation: 0,
            max_leap: 12,
            curve: vec![(0, 20), (500, 90), (1000, 35)],
            interval: 2,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BassSpec {
    #[serde(default)]
    pub kick_source: Option<crate::midi_lab::Destination>,
    #[serde(default = "kick_note")]
    pub kick_note: u8,
    #[serde(default)]
    pub audition_drums: bool,
    pub role: BassRole,
    pub style: BassStyle,
    pub groove: Groove,
    pub pedal: u8,
    pub approaches: Decoration,
    pub fill_every: u8,
    pub fill_beats: u8,
    pub leave_melody_space: bool,
    pub kick_length: u32,
    pub kick: Vec<u32>,
    pub variation: u16,
    pub tuning: Vec<u8>,
}
fn kick_note() -> u8 {
    36
}
impl Default for BassSpec {
    fn default() -> Self {
        Self {
            kick_source: None,
            kick_note: 36,
            audition_drums: false,
            role: BassRole::Foundation,
            style: BassStyle::Support,
            groove: Groove::Independent,
            pedal: 36,
            approaches: Decoration::None,
            fill_every: 4,
            fill_beats: 1,
            leave_melody_space: false,
            kick_length: 192,
            kick: vec![0, 96],
            variation: 0,
            tuning: vec![],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterSpec {
    #[serde(default = "default_leader")]
    pub leader: Voice,
    pub species: Species,
    pub interval: i16,
    pub delay: u32,
    pub invertible: Option<u8>,
    pub strict: bool,
}
fn default_leader() -> Voice {
    Voice::Melody
}
impl Default for CounterSpec {
    fn default() -> Self {
        Self {
            species: Species::Off,
            leader: Voice::Melody,
            interval: -12,
            delay: 48,
            invertible: None,
            strict: true,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartSpec {
    pub enabled: bool,
    #[serde(default)]
    pub profile: Option<crate::theory::harmony::HarmonicStyle>,
    pub low: u8,
    pub high: u8,
    pub velocity: u8,
    pub rhythm: RhythmSpec,
    pub melody: MelodySpec,
    pub bass: BassSpec,
    pub counter: CounterSpec,
    pub articulation: Articulation,
    #[serde(default)]
    pub velocity_curve: Vec<(u16, i16)>,
    #[serde(default)]
    pub gate_curve: Vec<(u16, i16)>,
}
impl PartSpec {
    pub fn new(voice: Voice) -> Self {
        Self {
            enabled: voice == Voice::Chords,
            profile: None,
            low: if voice == Voice::Bass {
                28
            } else if voice == Voice::Counterpoint {
                36
            } else {
                48
            },
            high: if voice == Voice::Bass { 60 } else { 84 },
            velocity: 86,
            rhythm: RhythmSpec {
                kind: if voice == Voice::Chords {
                    RhythmKind::Hold
                } else if voice == Voice::Bass {
                    RhythmKind::Quarter
                } else {
                    RhythmKind::Eighth
                },
                ..RhythmSpec::default()
            },
            melody: MelodySpec::default(),
            bass: BassSpec::default(),
            counter: CounterSpec::default(),
            articulation: Articulation::Normal,
            velocity_curve: vec![],
            gate_curve: vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Weights {
    pub motion: i32,
    pub leap: i32,
    pub common: i32,
    pub spacing: i32,
    pub parallel: i32,
    pub crossing: i32,
    pub doubling: i32,
}
impl Default for Weights {
    fn default() -> Self {
        Self {
            motion: 4,
            leap: 2,
            common: 3,
            spacing: 1,
            parallel: 0,
            crossing: 8,
            doubling: 2,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Voicing {
    pub layout: Layout,
    pub center: u8,
    pub lead: bool,
    pub inversion: u8,
    pub omitted: Vec<u64>,
    pub doubled: Vec<u64>,
    pub offsets: BTreeMap<u64, i16>,
}
impl Default for Voicing {
    fn default() -> Self {
        Self {
            layout: Layout::Closed,
            center: 60,
            lead: true,
            inversion: 0,
            omitted: vec![],
            doubled: vec![],
            offsets: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarmonySpan {
    pub id: u64,
    pub start: u32,
    pub length: u32,
    pub material: Material,
    pub voicing: Voicing,
    pub operation: Option<String>,
    pub replaced: Option<Material>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MotifNote {
    pub id: u64,
    pub pitch: i16,
    pub start: u32,
    pub length: u32,
    pub velocity: u8,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Motif {
    pub id: u64,
    pub name: String,
    pub frame: PitchFrame,
    pub length: u32,
    pub notes: Vec<MotifNote>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transform {
    Transpose(i16),
    Diatonic(i16),
    Degree(i16),
    Invert(i16),
    Retrograde,
    ScaleTime(u16, u16),
    Rotate(u32),
    Fragment(u32, u32),
    Sequence { interval: i16, repeats: u8 },
}
impl Transform {
    pub fn label(&self) -> String {
        match self {
            Self::Transpose(n) => format!("Transpose {n:+} semitones"),
            Self::Diatonic(n) => format!("Transpose {n:+} scale steps"),
            Self::Degree(n) => format!("Move {n:+} chord members"),
            Self::Invert(n) => format!("Invert about {n}"),
            Self::Retrograde => "Retrograde".into(),
            Self::ScaleTime(a, b) => format!("Scale time {a}/{b}"),
            Self::Rotate(n) => format!("Rotate {n} ticks"),
            Self::Fragment(a, b) => format!("Fragment {a}–{b}"),
            Self::Sequence { interval, repeats } => {
                format!("Sequence {interval:+}, {repeats} statements")
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub id: u64,
    pub motif: u64,
    pub voice: Voice,
    pub start: u32,
    pub anchor: u8,
    pub transforms: Vec<Transform>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modulation {
    pub id: u64,
    pub start: u32,
    pub key: Key,
    pub pivot: Pivot,
    pub pitch: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub id: u64,
    pub name: String,
    pub start: u32,
    pub length: u32,
    pub source: Option<u64>,
    pub transpose: i16,
    pub diatonic: bool,
    pub enabled: [bool; 5],
    pub simplify_bass: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    #[serde(default)]
    pub origin: Option<u64>,
    pub rule: String,
    pub rule_version: u16,
    pub harmony: Option<u64>,
    pub target: Option<u8>,
    pub motif: Option<u64>,
    pub placement: Option<u64>,
    pub transforms: Vec<String>,
    pub detail: String,
}
impl Provenance {
    pub fn new(rule: &str, harmony: Option<u64>, detail: impl Into<String>) -> Self {
        Self {
            origin: None,
            rule: rule.into(),
            rule_version: ENGINE_VERSION,
            harmony,
            target: None,
            motif: None,
            placement: None,
            transforms: vec![],
            detail: detail.into(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteEvent {
    pub id: u64,
    pub voice: Voice,
    pub member: Option<u64>,
    pub pitch: u8,
    pub start: u32,
    pub length: u32,
    pub velocity: u8,
    pub provenance: Provenance,
}
impl NoteEvent {
    pub fn end(&self) -> u32 {
        self.start.saturating_add(self.length)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Override {
    #[serde(default)]
    pub instance: bool,
    pub id: u64,
    pub pitch: Option<u8>,
    pub start: Option<u32>,
    pub length: Option<u32>,
    pub velocity: Option<u8>,
    pub deleted: bool,
    pub inserted: Option<NoteEvent>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub harmony: Vec<HarmonySpan>,
    #[serde(default)]
    pub voicings: Vec<VoicingDecision>,
    #[serde(default)]
    pub length: u32,
    pub input: String,
    pub events: Vec<NoteEvent>,
    pub label: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TensionMap {
    pub velocity: u8,
    pub register: u8,
    pub density: u8,
    pub gate: u8,
}
impl Default for TensionMap {
    fn default() -> Self {
        Self {
            velocity: 10,
            register: 0,
            density: 0,
            gate: 0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Composition {
    pub format: u16,
    pub engine: u16,
    pub name: String,
    pub length: u32,
    pub meter: Meter,
    pub key: Option<Key>,
    pub harmony: Vec<HarmonySpan>,
    pub voices: [PartSpec; 5],
    pub weights: Weights,
    pub looping: bool,
    pub motifs: Vec<Motif>,
    pub placements: Vec<Placement>,
    pub modulations: Vec<Modulation>,
    pub sections: Vec<Section>,
    pub form: Vec<u64>,
    pub tension: Vec<(u16, i16)>,
    #[serde(default)]
    pub tension_map: TensionMap,
    pub overrides: Vec<Override>,
    pub snapshot: Option<Snapshot>,
    pub comparisons: Vec<Snapshot>,
    pub frozen: bool,
    pub output: OutputPolicy,
    pub next_id: u64,
    #[serde(default)]
    pub destinations: [Option<crate::midi_lab::Destination>; 5],
    #[serde(default)]
    pub event_destinations: BTreeMap<u64, crate::midi_lab::Destination>,
}
impl Default for Composition {
    fn default() -> Self {
        let mut start = 0;
        let harmony = [("Cmaj7", 192), ("Am7", 192), ("Dm7", 192), ("G7", 192)]
            .into_iter()
            .enumerate()
            .map(|(i, (symbol, length))| {
                let h = HarmonySpan {
                    id: i as u64 + 1,
                    start,
                    length,
                    material: Material::parse(symbol).expect("static chord"),
                    voicing: Voicing::default(),
                    operation: None,
                    replaced: None,
                };
                start += length;
                h
            })
            .collect();
        Self {
            format: FORMAT_VERSION,
            engine: ENGINE_VERSION,
            name: "Untitled composition".into(),
            length: 768,
            meter: Meter::default(),
            key: Some(Key::default()),
            harmony,
            voices: Voice::ALL.map(PartSpec::new),
            weights: Weights::default(),
            looping: true,
            motifs: vec![],
            placements: vec![],
            modulations: vec![],
            sections: vec![],
            form: vec![],
            tension: vec![(0, 50), (1000, 50)],
            tension_map: TensionMap::default(),
            overrides: vec![],
            snapshot: None,
            comparisons: vec![],
            frozen: false,
            output: OutputPolicy::Faithful,
            next_id: 100,
            destinations: [None; 5],
            event_destinations: BTreeMap::new(),
        }
    }
}
impl Composition {
    pub fn mint(&mut self) -> u64 {
        self.next_id = self.next_id.saturating_add(1);
        self.next_id
    }
    pub fn key_at(&self, tick: u32) -> Option<&Key> {
        self.modulations
            .iter()
            .filter(|m| m.start <= tick)
            .max_by_key(|m| m.start)
            .map(|m| &m.key)
            .or(self.key.as_ref())
    }
    pub fn harmony_at(&self, tick: u32) -> Option<&HarmonySpan> {
        self.harmony
            .iter()
            .find(|h| tick >= h.start && tick < h.start.saturating_add(h.length))
    }
    pub fn input(&self) -> Result<String, String> {
        let mut input = self.clone();
        if input.frozen || input.engine != ENGINE_VERSION {
            // Frozen notes are an active input. Strip the nested input record
            // so repeated saves stay bounded while distinct takes get distinct
            // cache keys, even when their former generator settings agree.
            if let Some(snapshot) = &mut input.snapshot {
                snapshot.input.clear();
                snapshot.label.clear();
            }
        } else {
            input.snapshot = None;
        }
        input.comparisons.clear();
        input.harmony.sort_by_key(|h| (h.start, h.id));
        input.motifs.sort_by_key(|m| m.id);
        input.placements.sort_by_key(|m| m.id);
        input.modulations.sort_by_key(|m| (m.start, m.id));
        input.sections.sort_by_key(|m| m.id);
        input.overrides.sort_by_key(|m| m.id);
        ron::to_string(&input).map_err(|e| e.to_string())
    }
    pub fn fingerprint(&self) -> Result<String, String> {
        let bytes = self.input()?;
        let mut hash = 0xcbf29ce484222325u64;
        for byte in bytes.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        Ok(format!("{:04x}-{hash:016x}", self.engine))
    }
    pub fn progression(&mut self, text: &str) -> Result<(), String> {
        // Whitespace separates spans; use commas within explicit note collections.
        let mut next = Vec::new();
        let mut start = 0u32;
        let mut id = self.next_id;
        for item in text.split_whitespace() {
            let (entry, duration) = item
                .rsplit_once(':')
                .ok_or("Use chord:beats, for example Cmaj7:4 or notes:C4,E4,G4:4")?;
            let length = if let Some(ticks) = duration.strip_suffix('t') {
                ticks
                    .parse::<u32>()
                    .map_err(|_| "Use a positive tick count, for example Cmaj7:193t")?
            } else {
                duration
                    .parse::<u32>()
                    .map_err(
                        |_| "Use whole beats or explicit ticks, for example Cmaj7:4 or Cmaj7:193t",
                    )?
                    .checked_mul(PPQ)
                    .ok_or("Duration exceeds timeline capacity")?
            };
            let length = Some(length)
                .filter(|n| *n > 0)
                .ok_or("Duration must be positive")?;
            if start.checked_add(length).is_none_or(|end| end > MAX_TICKS) {
                return Err("Composition exceeds 256 bars of 4/4".into());
            }
            let material = if let Some(function) = entry.strip_prefix("fn:") {
                crate::theory::functional::from_function(
                    function,
                    self.key.as_ref().ok_or("Function entry needs a key")?,
                )?
            } else {
                Material::parse(entry)?
            };
            id = id.checked_add(1).ok_or("Identity space exhausted")?;
            next.push(HarmonySpan {
                id,
                start,
                length,
                material,
                voicing: Voicing::default(),
                operation: Some(format!("Entered {entry}")),
                replaced: None,
            });
            start += length;
        }
        if next.is_empty() {
            return Err("Enter a progression".into());
        }
        self.harmony = next;
        self.length = start;
        self.next_id = id;
        self.frozen = false;
        self.snapshot = None;
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.format != FORMAT_VERSION {
            return Err(format!("Unsupported composition format {}", self.format));
        }
        if self.engine != ENGINE_VERSION && !self.frozen {
            return Err(format!(
                "Generator version {} is unavailable; play the saved snapshot or explicitly upgrade",
                self.engine
            ));
        }
        if self.length == 0 || self.length > MAX_TICKS {
            return Err("Composition length must be 1–49152 ticks".into());
        }
        self.meter.bar()?;
        if let Some(key) = &self.key {
            key.validate()?;
        }
        if self.harmony.len() > 1024
            || self.motifs.len() > 256
            || self.placements.len() > 2048
            || self.overrides.len() > MAX_EVENTS
            || self.sections.len() > 256
            || self.form.len() > 256
        {
            return Err("Composition exceeds the bounded resource budget".into());
        }
        let mut spans = self.harmony.iter().collect::<Vec<_>>();
        spans.sort_by_key(|h| (h.start, h.id));
        let mut ids = std::collections::BTreeSet::new();
        let mut end = 0;
        for h in spans {
            h.material.validate()?;
            if !ids.insert(h.id)
                || h.length == 0
                || h.start < end
                || h.start
                    .checked_add(h.length)
                    .is_none_or(|n| n > self.length)
            {
                return Err(
                    "Harmony overlaps, has duplicate IDs, or exceeds the composition".into(),
                );
            }
            end = h.start + h.length;
        }
        for v in &self.voices {
            if v.low > v.high
                || v.high > 127
                || v.velocity == 0
                || v.velocity > 127
                || v.rhythm.gate == 0
                || v.rhythm.gate > 100
                || v.rhythm.swing < 50
                || v.rhythm.swing > 75
                || v.rhythm.steps == 0
                || v.rhythm.steps > 64
                || v.rhythm.pulses > v.rhythm.steps
                || v.rhythm.custom.len() > 2048
                || v.melody.phrase_bars == 0
                || v.melody.phrase_bars > 32
                || v.melody.arrival.is_some_and(|p| p > 127)
                || v.bass.pedal > 127
                || v.bass.kick.len() > 2048
                || v.bass.kick_length == 0
            {
                return Err("Voice has invalid range, rhythm or phrase settings".into());
            }
        }
        for m in &self.modulations {
            m.key.validate()?;
            if m.start >= self.length {
                return Err("Modulation is outside the composition".into());
            }
        }
        if [
            &self.weights.motion,
            &self.weights.leap,
            &self.weights.common,
            &self.weights.spacing,
            &self.weights.parallel,
            &self.weights.crossing,
            &self.weights.doubling,
        ]
        .iter()
        .any(|w| **w < 0 || **w > 1000)
        {
            return Err("Voicing costs must be 0–1000".into());
        }
        super::validation::document(self)?;
        Ok(())
    }
    pub fn pin(&mut self, note: &NoteEvent) {
        self.overrides.retain(|o| o.id != note.id);
        self.overrides.push(Override {
            id: note.id,
            instance: !self.form.is_empty(),
            pitch: Some(note.pitch),
            start: Some(note.start),
            length: Some(note.length),
            velocity: Some(note.velocity),
            ..Override::default()
        });
    }
    pub fn remove_note(&mut self, id: u64) {
        self.overrides.retain(|o| o.id != id);
        self.overrides.push(Override {
            id,
            instance: !self.form.is_empty(),
            deleted: true,
            ..Override::default()
        });
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub rule: String,
    pub events: Vec<u64>,
    pub detail: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cost {
    pub motion: i64,
    pub leap: i64,
    pub common: i64,
    pub spacing: i64,
    pub parallel: i64,
    pub crossing: i64,
    pub doubling: i64,
}
impl Cost {
    pub fn total(&self) -> i64 {
        self.motion
            + self.leap
            + self.common
            + self.spacing
            + self.parallel
            + self.crossing
            + self.doubling
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoicingDecision {
    pub harmony: u64,
    pub notes: Vec<(u64, u8)>,
    pub cost: Cost,
    pub runner_up: Option<(Vec<u8>, i64)>,
    pub candidates: usize,
    pub searched: usize,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rendered {
    pub notes: Vec<NoteEvent>,
    pub harmony: Vec<HarmonySpan>,
    pub voicings: Vec<VoicingDecision>,
    pub findings: Vec<Finding>,
    pub length: u32,
    pub fingerprint: String,
}

/// Identity hashing only. Never use this to choose a pitch, rhythm or variant.
pub fn identity(parts: &[u64]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for value in parts {
        for b in value.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

pub fn curve(points: &[(u16, i16)], at: u16) -> i16 {
    if points.is_empty() {
        return 50;
    }
    let mut ordered = points.to_vec();
    ordered.sort_by_key(|x| x.0);
    if at <= ordered[0].0 {
        return ordered[0].1;
    }
    for pair in ordered.windows(2) {
        let [(a, x), (b, y)] = [pair[0], pair[1]];
        if at <= b && b > a {
            return (i32::from(x)
                + (i32::from(y) - i32::from(x)) * i32::from(at - a) / i32::from(b - a))
                as i16;
        }
    }
    ordered.last().map_or(50, |p| p.1)
}
