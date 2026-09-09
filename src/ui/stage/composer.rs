//! Controller shared by keyboard and pointer for the composition inspector.
use super::{Stage, grid::Step, lab::Instrument};
use crate::midi_lab::{Voice, composer::*};
use crate::theory::{
    functional::Key,
    material::{Bass, Material, Member},
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Subject {
    #[default]
    Harmony,
    Voicing,
    Melody,
    Bass,
    Rhythm,
    Motif,
    Counterpoint,
    Form,
    Note,
    Recipe,
}
impl Subject {
    pub const ALL: [Self; 10] = [
        Self::Harmony,
        Self::Voicing,
        Self::Melody,
        Self::Bass,
        Self::Rhythm,
        Self::Motif,
        Self::Counterpoint,
        Self::Form,
        Self::Note,
        Self::Recipe,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Harmony => "Chord",
            Self::Voicing => "Voicing",
            Self::Melody => "Melody",
            Self::Bass => "Bass",
            Self::Rhythm => "Rhythm",
            Self::Motif => "Motif",
            Self::Counterpoint => "Counterpoint",
            Self::Form => "Form",
            Self::Note => "Note",
            Self::Recipe => "Recipe",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Control {
    TensionVelocity,
    TensionRegister,
    TensionDensity,
    TensionGate,
    Interpretation,
    KickSource,
    AuditionDrums,
    Leader,
    Profile,
    PinVoice,
    PinPhrase,
    UnpinVoice,
    SavedComparison,
    RestoreComparison,
    Version,
    Subject,
    CustomKey,
    Groups,
    ModulationKey,
    Omitted,
    Doubled,
    MemberOffsets,
    Frame,
    SectionVoice,
    SectionVoiceOn,
    Input,
    CaptureInput,
    LiftClip,
    Chord,
    Entry,
    Progression,
    Root,
    BassPitch,
    Beats,
    AddChord,
    RemoveChord,
    PitchClass(u8),
    KeyOn,
    Tonic,
    Mode,
    Cadence,
    ApplyCadence,
    Substitution,
    ApplySubstitution,
    Modulation,
    Pivot,
    AddModulation,
    Layout,
    Center,
    Inversion,
    Lead,
    MotionCost,
    LeapCost,
    CommonCost,
    ParallelCost,
    SpacingCost,
    Voice,
    Enabled,
    Low,
    High,
    Velocity,
    Articulation,
    Destination,
    Rhythm,
    Gate,
    Swing,
    Rotation,
    Pulses,
    Steps,
    WrittenRhythm,
    Offsets,
    Accents,
    Meter,
    Contour,
    Movement,
    Decoration,
    Targets,
    Arrival,
    Development,
    PhraseBars,
    Variation,
    MaxLeap,
    Interval,
    BassRole,
    BassStyle,
    Groove,
    Pedal,
    Approaches,
    FillEvery,
    FillBeats,
    Space,
    Kick,
    Tuning,
    Action,
    Explore,
    Candidate,
    Accept,
    Compare,
    SaveComparison,
    Motif,
    Name,
    CaptureStart,
    CaptureEnd,
    Capture,
    PlaceStart,
    Anchor,
    Transform,
    TransformValue,
    Place,
    Placements,
    Species,
    CanonInterval,
    Delay,
    Invertible,
    Strict,
    Section,
    SectionName,
    SectionStart,
    SectionLength,
    SectionSource,
    SectionTranspose,
    SectionDiatonic,
    SectionSimple,
    AddSection,
    Form,
    NotePitch,
    NoteStart,
    NoteLength,
    NoteVelocity,
    Pin,
    Unpin,
    DeleteNote,
    Title,
    Length,
    Looping,
    Output,
    Separate,
    Path,
    Save,
    Load,
    Export,
    Thaw,
}
impl Control {
    pub fn label(self) -> String {
        match self {
            Self::CustomKey => "Scale intervals",
            Self::Groups => "Beat groups",
            Self::ModulationKey => "Destination key",
            Self::Omitted => "Omitted member IDs",
            Self::Doubled => "Doubled member IDs",
            Self::MemberOffsets => "Member ID / octaves",
            Self::Frame => "Pitch frame",
            Self::SectionVoice => "Section voice",
            Self::SectionVoiceOn => "Voice in section",
            Self::Input => "Capture MIDI notes",
            Self::CaptureInput => "Use captured chord",
            Self::LiftClip => "Lift written clip notes",
            Self::TensionVelocity => "Velocity range",
            Self::TensionRegister => "Register octaves",
            Self::TensionDensity => "Density reduction",
            Self::TensionGate => "Gate range (%)",
            Self::Interpretation => "Interpretation",
            Self::KickSource => "Copy kick attacks",
            Self::AuditionDrums => "Audition drum clip",
            Self::Leader => "Leader voice",
            Self::Profile => "Harmonic profile",
            Self::PinVoice => "Lock voice",
            Self::PinPhrase => "Lock selected phrase",
            Self::UnpinVoice => "Unlock voice",
            Self::SavedComparison => "Saved comparison",
            Self::RestoreComparison => "Recall comparison",
            Self::Version => "Format / generator",
            Self::Subject => "Subject",
            Self::Chord => "Selected chord",
            Self::Entry => "Material",
            Self::Progression => "Progression",
            Self::Root => "Harmonic root",
            Self::BassPitch => "Explicit bass",
            Self::Beats => "Duration (beats)",
            Self::AddChord => "Add chord",
            Self::RemoveChord => "Remove chord",
            Self::PitchClass(pc) => return crate::theory::pitch_class_name(pc).into(),
            Self::KeyOn => "Tonal context",
            Self::Tonic => "Tonic",
            Self::Mode => "Mode",
            Self::Cadence => "Cadence",
            Self::ApplyCadence => "Append cadence",
            Self::Substitution => "Substitution",
            Self::ApplySubstitution => "Apply substitution",
            Self::Modulation => "Modulation tick",
            Self::Pivot => "Pivot",
            Self::AddModulation => "Add modulation",
            Self::Layout => "Layout",
            Self::Center => "Register centre",
            Self::Inversion => "Inversion",
            Self::Lead => "Voice leading",
            Self::MotionCost => "Total motion",
            Self::LeapCost => "Largest leap",
            Self::CommonCost => "Common tones",
            Self::ParallelCost => "Parallel intervals",
            Self::SpacingCost => "Spacing",
            Self::Voice => "Voice",
            Self::Enabled => "Enabled",
            Self::Low => "Lowest pitch",
            Self::High => "Highest pitch",
            Self::Velocity => "Velocity",
            Self::Articulation => "Articulation",
            Self::Destination => "Voice destination",
            Self::Rhythm => "Rhythm",
            Self::Gate => "Gate (%)",
            Self::Swing => "Swing (%)",
            Self::Rotation => "Rotation",
            Self::Pulses => "Pulses",
            Self::Steps => "Steps",
            Self::WrittenRhythm => "Written rhythm",
            Self::Offsets => "Timing offsets",
            Self::Accents => "Velocity offsets",
            Self::Meter => "Metre",
            Self::Contour => "Contour",
            Self::Movement => "Movement",
            Self::Decoration => "Decoration",
            Self::Targets => "Target roles",
            Self::Arrival => "Final MIDI pitch",
            Self::Development => "Development",
            Self::PhraseBars => "Phrase bars",
            Self::Variation => "Construction",
            Self::MaxLeap => "Maximum leap",
            Self::Interval => "Sequence interval",
            Self::BassRole => "Bass role",
            Self::BassStyle => "Bass style",
            Self::Groove => "Kick relationship",
            Self::Pedal => "Pedal MIDI pitch",
            Self::Approaches => "Approach",
            Self::FillEvery => "Fill every N bars",
            Self::FillBeats => "Fill beats",
            Self::Space => "Space under melody",
            Self::Kick => "Kick ticks / length",
            Self::Tuning => "Open-string pitches",
            Self::Action => "Variation action",
            Self::Explore => "Explore alternatives",
            Self::Candidate => "Candidate",
            Self::Accept => "Use candidate",
            Self::Compare => "Compare A / B",
            Self::SaveComparison => "Save comparison",
            Self::Motif => "Selected motif",
            Self::Name => "Motif name",
            Self::CaptureStart => "Capture start",
            Self::CaptureEnd => "Capture end",
            Self::Capture => "Capture notes",
            Self::PlaceStart => "Place at tick",
            Self::Anchor => "Reference pitch",
            Self::Transform => "Transformation",
            Self::TransformValue => "Transform amount",
            Self::Place => "Place motif",
            Self::Placements => "Placement document",
            Self::Species => "Rule set",
            Self::CanonInterval => "Canon semitones",
            Self::Delay => "Canon delay",
            Self::Invertible => "Invertible at",
            Self::Strict => "Enforce rules",
            Self::Section => "Selected section",
            Self::SectionName => "Section name",
            Self::SectionStart => "Source start",
            Self::SectionLength => "Source length",
            Self::SectionSource => "Reference section",
            Self::SectionTranspose => "Transpose",
            Self::SectionDiatonic => "Diatonic transpose",
            Self::SectionSimple => "Simplify bass",
            Self::AddSection => "Add section",
            Self::Form => "Form order",
            Self::NotePitch => "MIDI pitch",
            Self::NoteStart => "Start tick",
            Self::NoteLength => "Duration ticks",
            Self::NoteVelocity => "Velocity",
            Self::Pin => "Lock note",
            Self::Unpin => "Unlock note",
            Self::DeleteNote => "Delete note",
            Self::Title => "Composition name",
            Self::Length => "Timeline beats",
            Self::Looping => "Loop connection",
            Self::Output => "Output conversion",
            Self::Separate => "Separate voice tracks",
            Self::Path => "Recipe / MIDI path",
            Self::Save => "Save recipe",
            Self::Load => "Load recipe",
            Self::Export => "Export MIDI",
            Self::Thaw => "Adopt saved notes",
        }
        .into()
    }
    pub fn text(self) -> bool {
        matches!(
            self,
            Self::KickSource
                | Self::Destination
                | Self::CustomKey
                | Self::Groups
                | Self::ModulationKey
                | Self::Omitted
                | Self::Doubled
                | Self::MemberOffsets
                | Self::Entry
                | Self::Progression
                | Self::BassPitch
                | Self::WrittenRhythm
                | Self::Offsets
                | Self::Accents
                | Self::Meter
                | Self::Kick
                | Self::Tuning
                | Self::Name
                | Self::Placements
                | Self::SectionName
                | Self::Form
                | Self::Title
                | Self::Path
        )
    }
    pub fn action(self) -> bool {
        matches!(
            self,
            Self::PinVoice
                | Self::PinPhrase
                | Self::UnpinVoice
                | Self::RestoreComparison
                | Self::CaptureInput
                | Self::LiftClip
                | Self::AddChord
                | Self::RemoveChord
                | Self::ApplyCadence
                | Self::ApplySubstitution
                | Self::AddModulation
                | Self::Explore
                | Self::Accept
                | Self::Compare
                | Self::SaveComparison
                | Self::Capture
                | Self::Place
                | Self::AddSection
                | Self::Pin
                | Self::Unpin
                | Self::DeleteNote
                | Self::Separate
                | Self::Save
                | Self::Load
                | Self::Export
                | Self::Thaw
        )
    }
}

#[derive(Clone, Debug)]
pub(super) struct State {
    pub subject: Subject,
    pub score_focus: bool,
    pub palette: bool,
    pub palette_query: String,
    pub focus: usize,
    pub chord: usize,
    pub voice: Voice,
    pub note: Option<u64>,
    pub texts: BTreeMap<Control, String>,
    pub query: String,
    pub picker: Option<Control>,
    pub picker_index: usize,
    pub action: VariationAction,
    pub candidates: Vec<alternatives::Candidate>,
    pub candidate: usize,
    pub saved: usize,
    pub explored_input: String,
    pub reference: Option<Box<Composition>>,
    pub motif: usize,
    pub name: String,
    pub capture_start: u32,
    pub capture_end: u32,
    pub place_start: u32,
    pub anchor: u8,
    pub transform: usize,
    pub amount: i16,
    pub section: usize,
    pub cadence: usize,
    pub substitution: usize,
    pub modulation: u32,
    pub pivot: Pivot,
    pub chase: bool,
    pub edit: Option<Control>,
    pub advanced: bool,
    pub zoom: u32,
    pub scroll: u32,
    pub result: Option<Rendered>,
    pub result_input: String,
    pub error: Option<String>,
    pub path: String,
    pub tempo: f64,
    pub tempo_marks: Vec<crate::sequencing::TempoMark>,
    pub destinations: Vec<(String, crate::midi_lab::Destination)>,
    pub input: bool,
    pub captured: Vec<u8>,
    pub lifted: Vec<NoteEvent>,
    pub lift_error: Option<String>,
    pub clip_sources: Vec<(String, crate::midi_lab::Destination, u32, Vec<NoteEvent>)>,
    pub modulation_key: Key,
}
impl Default for State {
    fn default() -> Self {
        Self {
            subject: crate::ui::stage::composer::Subject::Harmony,
            score_focus: false,
            palette: false,
            palette_query: String::new(),
            focus: 0,
            chord: 0,
            voice: crate::midi_lab::Voice::Chords,
            note: None,
            texts: BTreeMap::new(),
            query: String::new(),
            picker: None,
            picker_index: 0,
            action: VariationAction::NewPitches,
            candidates: vec![],
            candidate: 0,
            saved: 0,
            explored_input: String::new(),
            reference: None,
            motif: 0,
            name: "Motif 1".into(),
            capture_start: 0,
            capture_end: 192,
            place_start: 0,
            anchor: 60,
            transform: 0,
            amount: 2,
            section: 0,
            cadence: 0,
            substitution: 0,
            modulation: 192,
            pivot: Pivot::Direct,
            chase: false,
            edit: None,
            advanced: false,
            zoom: 0,
            scroll: 0,
            result: None,
            result_input: String::new(),
            error: None,
            path: "/tmp/midi-composition.ron".into(),
            tempo: 120.,
            tempo_marks: vec![],
            destinations: vec![],
            input: false,
            captured: vec![],
            lifted: vec![],
            lift_error: None,
            clip_sources: vec![],
            modulation_key: Key {
                tonic: 7,
                ..Key::default()
            },
        }
    }
}

pub(super) fn controls(state: &State, c: &Composition) -> Vec<Control> {
    use Control::*;
    let mut fields = vec![Subject];
    fields.extend(match state.subject {
        crate::ui::stage::composer::Subject::Harmony => vec![
            Chord,
            Entry,
            Interpretation,
            Beats,
            Root,
            BassPitch,
            AddChord,
            RemoveChord,
            Input,
            CaptureInput,
            Progression,
            Cadence,
            ApplyCadence,
            Substitution,
            ApplySubstitution,
        ],
        crate::ui::stage::composer::Subject::Voicing => vec![
            Chord,
            Layout,
            Center,
            Inversion,
            Lead,
            Low,
            High,
            MotionCost,
            LeapCost,
            CommonCost,
            ParallelCost,
            SpacingCost,
            Omitted,
            Doubled,
            MemberOffsets,
        ],
        crate::ui::stage::composer::Subject::Melody => vec![
            Voice,
            Enabled,
            Destination,
            Contour,
            Movement,
            Targets,
            Arrival,
            Decoration,
            Development,
            PhraseBars,
            Variation,
            MaxLeap,
            Interval,
            Low,
            High,
            Action,
            Explore,
            Candidate,
            Accept,
            Compare,
            SaveComparison,
            PinVoice,
            PinPhrase,
            UnpinVoice,
        ],
        crate::ui::stage::composer::Subject::Bass => vec![
            Enabled,
            Destination,
            BassRole,
            BassStyle,
            Groove,
            Approaches,
            Low,
            High,
            Pedal,
            FillEvery,
            FillBeats,
            Space,
            Kick,
            KickSource,
            AuditionDrums,
            Tuning,
            Variation,
            Action,
            Explore,
            Candidate,
            Accept,
            Compare,
            PinVoice,
            PinPhrase,
            UnpinVoice,
        ],
        crate::ui::stage::composer::Subject::Rhythm => vec![
            Voice,
            Enabled,
            Rhythm,
            Gate,
            Swing,
            Rotation,
            Pulses,
            Steps,
            Velocity,
            Articulation,
            Profile,
            WrittenRhythm,
            Offsets,
            Accents,
        ],
        crate::ui::stage::composer::Subject::Motif => vec![
            Voice,
            Motif,
            Frame,
            LiftClip,
            Name,
            CaptureStart,
            CaptureEnd,
            Capture,
            PlaceStart,
            Anchor,
            Transform,
            TransformValue,
            Place,
            Placements,
        ],
        crate::ui::stage::composer::Subject::Counterpoint => vec![
            Enabled,
            Destination,
            Leader,
            Species,
            CanonInterval,
            Delay,
            Invertible,
            Strict,
            Low,
            High,
        ],
        crate::ui::stage::composer::Subject::Form => vec![
            Section,
            SectionName,
            SectionStart,
            SectionLength,
            SectionSource,
            SectionTranspose,
            SectionDiatonic,
            SectionSimple,
            SectionVoice,
            SectionVoiceOn,
            AddSection,
            Form,
            TensionVelocity,
            TensionRegister,
            TensionDensity,
            TensionGate,
        ],
        crate::ui::stage::composer::Subject::Note => vec![
            NotePitch,
            NoteStart,
            NoteLength,
            NoteVelocity,
            Pin,
            Unpin,
            DeleteNote,
        ],
        crate::ui::stage::composer::Subject::Recipe => vec![
            Title,
            Version,
            KeyOn,
            Tonic,
            Mode,
            CustomKey,
            Meter,
            Groups,
            Length,
            Looping,
            Modulation,
            ModulationKey,
            Pivot,
            AddModulation,
            Output,
            Separate,
            SavedComparison,
            RestoreComparison,
            Path,
            Save,
            Load,
            Export,
            Thaw,
        ],
    });
    if state.subject == crate::ui::stage::composer::Subject::Harmony {
        let at = fields
            .iter()
            .position(|f| *f == AddChord)
            .unwrap_or(fields.len());
        fields.splice(at..at, (0..12).map(PitchClass));
    }
    if c.frozen {
        fields.retain(|f| matches!(f, Subject | Path | Save | Load | Export | Thaw));
        if !fields.contains(&Thaw) {
            fields.push(Thaw);
        }
    }
    fields
}

pub(super) fn voice(state: &State) -> Voice {
    match state.subject {
        crate::ui::stage::composer::Subject::Bass => crate::midi_lab::Voice::Bass,
        crate::ui::stage::composer::Subject::Counterpoint => crate::midi_lab::Voice::Counterpoint,
        crate::ui::stage::composer::Subject::Voicing
        | crate::ui::stage::composer::Subject::Harmony => crate::midi_lab::Voice::Chords,
        _ => state.voice,
    }
}

pub(super) fn reading(field: Control, c: &Composition, s: &State) -> String {
    use Control::*;
    let v = &c.voices[voice(s).index()];
    let h = c.harmony.get(s.chord);
    let section = c.sections.get(s.section);
    let note = s
        .result
        .as_ref()
        .and_then(|r| r.notes.iter().find(|n| Some(n.id) == s.note));
    let on = |v: bool| if v { "On" } else { "Off" }.to_owned();
    match field {
        CustomKey => c.key.as_ref().map_or(String::new(), |k| {
            k.degrees(false)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        }),
        Groups => c
            .meter
            .groups
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(","),
        ModulationKey => format!(
            "{}:{}",
            crate::theory::pitch_class_name(s.modulation_key.tonic),
            s.modulation_key.mode.label()
        ),
        Omitted => h.map_or(String::new(), |h| {
            h.voicing
                .omitted
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        }),
        Doubled => h.map_or(String::new(), |h| {
            h.voicing
                .doubled
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        }),
        MemberOffsets => h.map_or(String::new(), |h| {
            h.voicing
                .offsets
                .iter()
                .map(|(id, n)| format!("{id}/{n}"))
                .collect::<Vec<_>>()
                .join(",")
        }),
        Frame => c
            .motifs
            .get(s.motif)
            .map_or("Capture a motif".into(), |m| m.frame.label().into()),
        SectionVoice => s.voice.label().into(),
        SectionVoiceOn => on(section.is_some_and(|sct| sct.enabled[s.voice.index()])),
        Input => format!(
            "{} · {} captured",
            if s.input { "Listening" } else { "Off" },
            s.captured.len()
        ),
        TensionVelocity => c.tension_map.velocity.to_string(),
        TensionRegister => c.tension_map.register.to_string(),
        TensionDensity => c.tension_map.density.to_string(),
        TensionGate => c.tension_map.gate.to_string(),
        Interpretation => h.map_or("None".into(), |h| {
            h.material
                .root
                .and_then(|root| {
                    h.material
                        .readings()
                        .into_iter()
                        .find(|(r, _)| *r == root)
                        .map(|(_, name)| name)
                })
                .unwrap_or("Unspecified".into())
        }),
        Leader => v.counter.leader.label().into(),
        Profile => v
            .profile
            .map_or("Unrestricted".into(), |p| p.label().into()),
        SavedComparison => c
            .comparisons
            .get(s.saved)
            .map_or("None".into(), |v| v.label.clone()),
        Version => format!(
            "{} / {}{}",
            c.format,
            c.engine,
            if c.frozen { " · saved events" } else { "" }
        ),
        Subject => s.subject.label().into(),
        Chord => h.map_or("None".into(), |h| {
            format!("{} · {}", s.chord + 1, h.material.label())
        }),
        Entry => h.map_or(String::new(), |h| {
            if h.material.source.is_empty() {
                format!(
                    "pc:{}",
                    h.material
                        .members
                        .iter()
                        .map(|m| m.spelling.clone())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            } else {
                h.material.source.clone()
            }
        }),
        Progression => c
            .harmony
            .iter()
            .map(|h| {
                format!(
                    "{}:{}",
                    if h.material.source.is_empty() {
                        format!(
                            "pc:{}",
                            h.material
                                .members
                                .iter()
                                .map(|m| m.spelling.clone())
                                .collect::<Vec<_>>()
                                .join(",")
                        )
                    } else {
                        h.material.source.clone()
                    },
                    if h.length % PPQ == 0 {
                        (h.length / PPQ).to_string()
                    } else {
                        format!("{}t", h.length)
                    }
                )
            })
            .collect::<Vec<_>>()
            .join(" "),
        Root => h
            .and_then(|h| h.material.root)
            .map_or("Unspecified".into(), |p| {
                crate::theory::pitch_class_name(p).into()
            }),
        BassPitch => h
            .and_then(|h| h.material.bass)
            .map_or("none".into(), |b| match b {
                Bass::Class(pc) => crate::theory::pitch_class_name(pc).into(),
                Bass::Pitch(p) => format!(
                    "{}{}",
                    crate::theory::pitch_class_name(p),
                    i16::from(p) / 12 - 1
                ),
            }),
        Beats => h.map_or("0".into(), |h| format!("{}", h.length as f32 / PPQ as f32)),
        PitchClass(pc) => on(h.is_some_and(|h| h.material.mask() & (1 << pc) != 0)),
        KeyOn => on(c.key.is_some()),
        Tonic => c.key.as_ref().map_or("—".into(), |k| {
            crate::theory::pitch_class_name(k.tonic).into()
        }),
        Mode => c.key.as_ref().map_or("—".into(), |k| k.mode.label().into()),
        Cadence => crate::theory::functional::Cadence::ALL[s.cadence]
            .label()
            .into(),
        Substitution => crate::theory::functional::Substitution::ALL[s.substitution]
            .label()
            .into(),
        Modulation => s.modulation.to_string(),
        Pivot => s.pivot.label().into(),
        Layout => h.map_or("—".into(), |h| h.voicing.layout.label().into()),
        Center => h.map_or("—".into(), |h| h.voicing.center.to_string()),
        Inversion => h.map_or("—".into(), |h| h.voicing.inversion.to_string()),
        Lead => on(h.is_some_and(|h| h.voicing.lead)),
        MotionCost => c.weights.motion.to_string(),
        LeapCost => c.weights.leap.to_string(),
        CommonCost => c.weights.common.to_string(),
        ParallelCost => c.weights.parallel.to_string(),
        SpacingCost => c.weights.spacing.to_string(),
        Voice => voice(s).label().into(),
        Enabled => on(v.enabled),
        Low => v.low.to_string(),
        High => v.high.to_string(),
        Velocity => v.velocity.to_string(),
        Articulation => v.articulation.label().into(),
        Destination => c.destinations[voice(s).index()].map_or("Primary clip".into(), |d| {
            s.destinations.iter().find(|(_, dest)| *dest == d).map_or(
                format!("Missing {}:{}", d.track.0, d.pattern.0),
                |(tag, _)| tag.clone(),
            )
        }),
        Rhythm => v.rhythm.kind.label().into(),
        Gate => v.rhythm.gate.to_string(),
        Swing => v.rhythm.swing.to_string(),
        Rotation => v.rhythm.rotation.to_string(),
        Pulses => v.rhythm.pulses.to_string(),
        Steps => v.rhythm.steps.to_string(),
        WrittenRhythm => v
            .rhythm
            .custom
            .iter()
            .map(|p| format!("{}/{}{}", p.start, p.length, if p.tied { "~" } else { "" }))
            .collect::<Vec<_>>()
            .join(","),
        Offsets => v
            .rhythm
            .offsets
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(","),
        Accents => v
            .rhythm
            .accents
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(","),
        Meter => format!("{}/{}", c.meter.numerator, c.meter.denominator),
        Contour => v.melody.contour.label().into(),
        Movement => v.melody.movement.label().into(),
        Decoration => v.melody.decoration.label().into(),
        Targets => v.melody.targets.label().into(),
        Arrival => v.melody.arrival.map_or("None".into(), |n| n.to_string()),
        Development => v.melody.development.label().into(),
        PhraseBars => v.melody.phrase_bars.to_string(),
        Variation => {
            if voice(s) == crate::midi_lab::Voice::Bass {
                v.bass.variation.to_string()
            } else {
                v.melody.variation.to_string()
            }
        }
        MaxLeap => v.melody.max_leap.to_string(),
        Interval => v.melody.interval.to_string(),
        BassRole => v.bass.role.label().into(),
        BassStyle => v.bass.style.label().into(),
        Groove => v.bass.groove.label().into(),
        Pedal => v.bass.pedal.to_string(),
        Approaches => v.bass.approaches.label().into(),
        FillEvery => v.bass.fill_every.to_string(),
        FillBeats => v.bass.fill_beats.to_string(),
        Space => on(v.bass.leave_melody_space),
        KickSource => v.bass.kick_source.map_or("No source".into(), |dest| {
            format!(
                "{}:{}",
                s.destinations
                    .iter()
                    .find(|(_, d)| *d == dest)
                    .map_or("Missing", |(tag, _)| tag.as_str()),
                v.bass.kick_note
            )
        }),
        AuditionDrums => on(v.bass.audition_drums),
        Kick => format!(
            "{} / {}",
            v.bass
                .kick
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
            v.bass.kick_length
        ),
        Tuning => v
            .bass
            .tuning
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(","),
        Action => s.action.label().into(),
        Candidate => s
            .candidates
            .get(s.candidate)
            .map_or("None".into(), |c| c.name.clone()),
        Motif => c
            .motifs
            .get(s.motif)
            .map_or("None".into(), |m| m.name.clone()),
        Name => s.name.clone(),
        CaptureStart => s.capture_start.to_string(),
        CaptureEnd => s.capture_end.to_string(),
        PlaceStart => s.place_start.to_string(),
        Anchor => s.anchor.to_string(),
        Transform => [
            "Transpose",
            "Diatonic",
            "Invert",
            "Retrograde",
            "Augment ×2",
            "Diminish ÷2",
            "Rotate",
            "Fragment first half",
            "Sequence ×2",
            "Chord degree",
        ][s.transform]
            .into(),
        TransformValue => s.amount.to_string(),
        Placements => c
            .placements
            .iter()
            .map(|p| format!("{}:{}:{}", p.motif, p.start, p.anchor))
            .collect::<Vec<_>>()
            .join(" "),
        Species => v.counter.species.label().into(),
        CanonInterval => v.counter.interval.to_string(),
        Delay => v.counter.delay.to_string(),
        Invertible => v.counter.invertible.map_or("Off".into(), |n| n.to_string()),
        Strict => on(v.counter.strict),
        Section => section.map_or("None".into(), |s| s.name.clone()),
        SectionName => section.map_or(String::new(), |s| s.name.clone()),
        SectionStart => section.map_or("0".into(), |s| s.start.to_string()),
        SectionLength => section.map_or("0".into(), |s| s.length.to_string()),
        SectionSource => section
            .and_then(|s| s.source)
            .map_or("Original".into(), |id| {
                c.sections
                    .iter()
                    .find(|s| s.id == id)
                    .map_or("Missing".into(), |s| s.name.clone())
            }),
        SectionTranspose => section.map_or("0".into(), |s| s.transpose.to_string()),
        SectionDiatonic => on(section.is_some_and(|s| s.diatonic)),
        SectionSimple => on(section.is_some_and(|s| s.simplify_bass)),
        Form => c
            .form
            .iter()
            .filter_map(|id| {
                c.sections
                    .iter()
                    .find(|s| s.id == *id)
                    .map(|s| s.name.clone())
            })
            .collect::<Vec<_>>()
            .join(" "),
        NotePitch => note.map_or("—".into(), |n| n.pitch.to_string()),
        NoteStart => note.map_or("—".into(), |n| n.start.to_string()),
        NoteLength => note.map_or("—".into(), |n| n.length.to_string()),
        NoteVelocity => note.map_or("—".into(), |n| n.velocity.to_string()),
        Title => c.name.clone(),
        Length => (f64::from(c.length) / f64::from(PPQ)).to_string(),
        Looping => on(c.looping),
        Output => c.output.label().into(),
        Path => s.path.clone(),
        _ => "Enter".into(),
    }
}

fn number(value: i64, by: i32, min: i64, max: i64) -> Result<i64, String> {
    let next = value.saturating_add(i64::from(by)).clamp(min, max);
    if next == value {
        Err("The control is at its limit".into())
    } else {
        Ok(next)
    }
}
pub(super) fn turn(
    c: &mut Composition,
    s: &mut State,
    field: Control,
    by: i32,
) -> Result<(), String> {
    use Control::*;
    if field.text() {
        s.edit = Some(field);
        return Ok(());
    }
    let vi = voice(s).index();
    let cycling = |at: usize, len: usize| {
        if len == 0 {
            0
        } else {
            (at as i32 + by.signum()).rem_euclid(len as i32) as usize
        }
    };
    match field {
        AuditionDrums => {
            if c.voices[vi].bass.kick_source.is_none() {
                return Err("Copy kick attacks from a clip first".into());
            }
            c.voices[vi].bass.audition_drums = !c.voices[vi].bass.audition_drums;
        }

        SavedComparison => {
            if c.comparisons.is_empty() {
                return Err("Save a comparison first".into());
            }
            s.saved = cycling(s.saved, c.comparisons.len());
        }
        TensionVelocity => {
            c.tension_map.velocity = number(i64::from(c.tension_map.velocity), by, 0, 40)? as u8
        }
        TensionRegister => {
            c.tension_map.register = number(i64::from(c.tension_map.register), by, 0, 2)? as u8
        }
        TensionDensity => {
            c.tension_map.density = number(i64::from(c.tension_map.density), by, 0, 8)? as u8
        }
        TensionGate => {
            c.tension_map.gate = number(i64::from(c.tension_map.gate), by, 0, 100)? as u8
        }
        Interpretation => {
            let h = c.harmony.get_mut(s.chord).ok_or("Select a chord")?;
            let mut readings = vec![None];
            readings.extend(h.material.readings().into_iter().map(Some));
            let current = readings
                .iter()
                .position(|r| r.as_ref().map(|r| r.0) == h.material.root)
                .unwrap_or(0);
            let chosen = &readings[cycling(current, readings.len())];
            h.material.root = chosen.as_ref().map(|r| r.0);
            if let Some((_, name)) = chosen {
                let reading = Material::parse(name)?;
                for m in &mut h.material.members {
                    if let Some(role) = reading.members.iter().find(|r| r.pc == m.pc) {
                        m.degree = role.degree;
                        m.offset = role.offset;
                    }
                }
                h.operation = Some(format!("Interpreted as {name}"));
            } else {
                for m in &mut h.material.members {
                    m.degree = None;
                }
                h.operation = Some("Interpretation cleared".into());
            }
        }
        Leader => {
            let choices = [
                crate::midi_lab::Voice::Melody,
                crate::midi_lab::Voice::Bass,
                crate::midi_lab::Voice::Arp,
            ];
            c.voices[vi].counter.leader = choices[cycling(
                choices
                    .iter()
                    .position(|p| *p == c.voices[vi].counter.leader)
                    .unwrap_or(0),
                3,
            )];
        }
        Profile => {
            let choices = [
                None,
                Some(crate::theory::harmony::HarmonicStyle::Strict),
                Some(crate::theory::harmony::HarmonicStyle::Chromatic),
            ];
            c.voices[vi].profile = choices[cycling(
                choices
                    .iter()
                    .position(|p| *p == c.voices[vi].profile)
                    .unwrap_or(0),
                3,
            )];
        }
        Input => {
            s.input = !s.input;
            if s.input {
                s.captured.clear();
            }
        }
        Frame => {
            if let Some(original) = c.motifs.get(s.motif).cloned()
                && c.placements.iter().any(|p| p.motif == original.id)
            {
                let mut copy = original;
                copy.id = c.mint();
                copy.name.push_str(" · reframed");
                c.motifs.push(copy);
                s.motif = c.motifs.len() - 1;
            }
            let key = c.key.clone();
            let material = c.harmony.get(s.chord).map(|h| h.material.clone());
            let motif = c.motifs.get_mut(s.motif).ok_or("Capture a motif first")?;
            motif::reframe(
                motif,
                motif.frame.cycle(by.signum()),
                s.anchor,
                key.as_ref(),
                material.as_ref(),
            )?;
        }
        SectionVoice => s.voice = crate::midi_lab::Voice::ALL[cycling(s.voice.index(), 5)],
        SectionVoiceOn => {
            let section = c.sections.get_mut(s.section).ok_or("Select a section")?;
            section.enabled[s.voice.index()] = !section.enabled[s.voice.index()];
        }
        Subject => {
            s.subject = crate::ui::stage::composer::Subject::ALL[cycling(
                crate::ui::stage::composer::Subject::ALL
                    .iter()
                    .position(|v| *v == s.subject)
                    .unwrap_or(0),
                10,
            )];
            s.focus = 0;
            s.chase = true;
        }
        Chord => {
            if c.harmony.is_empty() {
                return Err("No chord selected".into());
            }
            s.chord = cycling(s.chord, c.harmony.len());
            s.texts.clear();
        }
        Root => {
            let h = c.harmony.get_mut(s.chord).ok_or("Select a chord")?;
            h.material.root = match h.material.root {
                None => Some(if by > 0 { 0 } else { 11 }),
                Some(p) if p == 11 && by > 0 || p == 0 && by < 0 => None,
                Some(p) => Some((i32::from(p) + by.signum()).rem_euclid(12) as u8),
            };
        }
        PitchClass(pc) => {
            let h = c.harmony.get_mut(s.chord).ok_or("Select a chord")?;
            if h.material.mask() & (1 << pc) != 0 {
                h.material.members.retain(|m| m.pc != pc);
            } else {
                let id = h.material.members.iter().map(|m| m.id).max().unwrap_or(0) + 1;
                h.material.members.push(Member {
                    id,
                    pc,
                    pitch: None,
                    spelling: crate::theory::pitch_class_name(pc).into(),
                    degree: None,
                    offset: i16::from(pc),
                    component: None,
                });
            }
            h.material.source.clear();
        }
        Beats => {
            let h = c.harmony.get(s.chord).ok_or("Select a chord")?;
            let new = number(i64::from(h.length), by * 12, 1, i64::from(MAX_TICKS))? as u32;
            let delta = i64::from(new) - i64::from(h.length);
            let end = i64::from(c.length) + delta;
            if end <= 0 || end > i64::from(MAX_TICKS) {
                return Err("Harmony duration exceeds timeline capacity".into());
            }
            for h in c.harmony.iter_mut().skip(s.chord + 1) {
                h.start = (i64::from(h.start) + delta) as u32;
            }
            c.harmony[s.chord].length = new;
            c.length = end as u32;
        }
        KeyOn => {
            c.key = if c.key.is_some() {
                None
            } else {
                Some(Key::default())
            };
        }
        Tonic => {
            let k = c.key.as_mut().ok_or("Enable tonal context")?;
            k.tonic = (i32::from(k.tonic) + by.signum()).rem_euclid(12) as u8;
        }
        Mode => {
            let k = c.key.as_mut().ok_or("Enable tonal context")?;
            k.mode = crate::theory::functional::Mode::ALL[cycling(
                crate::theory::functional::Mode::ALL
                    .iter()
                    .position(|m| *m == k.mode)
                    .unwrap_or(0),
                crate::theory::functional::Mode::ALL.len(),
            )];
            k.custom.clear();
        }
        Cadence => s.cadence = cycling(s.cadence, 5),
        Substitution => s.substitution = cycling(s.substitution, 5),
        Modulation => {
            s.modulation =
                number(i64::from(s.modulation), by * 48, 0, i64::from(c.length - 1))? as u32
        }
        Pivot => s.pivot = s.pivot.cycle(by.signum()),
        Layout => {
            let h = c.harmony.get_mut(s.chord).ok_or("Select a chord")?;
            h.voicing.layout = crate::theory::harmony::Layout::ALL[cycling(
                crate::theory::harmony::Layout::ALL
                    .iter()
                    .position(|l| *l == h.voicing.layout)
                    .unwrap_or(0),
                crate::theory::harmony::Layout::ALL.len(),
            )];
        }
        Center => {
            let h = c.harmony.get_mut(s.chord).ok_or("Select a chord")?;
            h.voicing.center = number(i64::from(h.voicing.center), by, 0, 127)? as u8;
        }
        Inversion => {
            let h = c.harmony.get_mut(s.chord).ok_or("Select a chord")?;
            h.voicing.inversion = number(
                i64::from(h.voicing.inversion),
                by,
                0,
                h.material.members.len().saturating_sub(1) as i64,
            )? as u8;
        }
        Lead => {
            let h = c.harmony.get_mut(s.chord).ok_or("Select a chord")?;
            h.voicing.lead = !h.voicing.lead;
        }
        MotionCost => c.weights.motion = number(i64::from(c.weights.motion), by, 0, 1000)? as i32,
        LeapCost => c.weights.leap = number(i64::from(c.weights.leap), by, 0, 1000)? as i32,
        CommonCost => c.weights.common = number(i64::from(c.weights.common), by, 0, 1000)? as i32,
        ParallelCost => {
            c.weights.parallel = number(i64::from(c.weights.parallel), by, 0, 1000)? as i32
        }
        SpacingCost => {
            c.weights.spacing = number(i64::from(c.weights.spacing), by, 0, 1000)? as i32
        }
        Voice => s.voice = crate::midi_lab::Voice::ALL[cycling(s.voice.index(), 5)],
        Enabled => c.voices[vi].enabled = !c.voices[vi].enabled,
        Low => {
            c.voices[vi].low = number(
                i64::from(c.voices[vi].low),
                by,
                0,
                i64::from(c.voices[vi].high),
            )? as u8
        }
        High => {
            c.voices[vi].high = number(
                i64::from(c.voices[vi].high),
                by,
                i64::from(c.voices[vi].low),
                127,
            )? as u8
        }
        Velocity => {
            c.voices[vi].velocity = number(i64::from(c.voices[vi].velocity), by, 1, 127)? as u8
        }
        Articulation => c.voices[vi].articulation = c.voices[vi].articulation.cycle(by.signum()),
        Rhythm => c.voices[vi].rhythm.kind = c.voices[vi].rhythm.kind.cycle(by.signum()),
        Gate => {
            c.voices[vi].rhythm.gate =
                number(i64::from(c.voices[vi].rhythm.gate), by, 1, 100)? as u8
        }
        Swing => {
            c.voices[vi].rhythm.swing =
                number(i64::from(c.voices[vi].rhythm.swing), by, 50, 75)? as u8
        }
        Rotation => {
            c.voices[vi].rhythm.rotation =
                number(i64::from(c.voices[vi].rhythm.rotation), by, 0, 1024)? as u16
        }
        Pulses => {
            c.voices[vi].rhythm.pulses = number(
                i64::from(c.voices[vi].rhythm.pulses),
                by,
                0,
                i64::from(c.voices[vi].rhythm.steps),
            )? as u8
        }
        Steps => {
            c.voices[vi].rhythm.steps =
                number(i64::from(c.voices[vi].rhythm.steps), by, 1, 64)? as u8;
            c.voices[vi].rhythm.pulses = c.voices[vi].rhythm.pulses.min(c.voices[vi].rhythm.steps);
        }
        Contour => c.voices[vi].melody.contour = c.voices[vi].melody.contour.cycle(by.signum()),
        Movement => c.voices[vi].melody.movement = c.voices[vi].melody.movement.cycle(by.signum()),
        Decoration => {
            c.voices[vi].melody.decoration = c.voices[vi].melody.decoration.cycle(by.signum())
        }
        Targets => c.voices[vi].melody.targets = c.voices[vi].melody.targets.cycle(by.signum()),
        Arrival => {
            let n = c.voices[vi].melody.arrival.map_or(-1, i64::from);
            let n = number(n, by, -1, 127)?;
            c.voices[vi].melody.arrival = if n < 0 { None } else { Some(n as u8) };
        }
        Development => {
            c.voices[vi].melody.development = c.voices[vi].melody.development.cycle(by.signum())
        }
        PhraseBars => {
            c.voices[vi].melody.phrase_bars =
                number(i64::from(c.voices[vi].melody.phrase_bars), by, 1, 32)? as u8
        }
        Variation => {
            if vi == 3 {
                c.voices[vi].bass.variation =
                    number(i64::from(c.voices[vi].bass.variation), by, 0, 4095)? as u16;
            } else {
                c.voices[vi].melody.variation =
                    number(i64::from(c.voices[vi].melody.variation), by, 0, 4095)? as u16;
            }
        }
        MaxLeap => {
            c.voices[vi].melody.max_leap =
                number(i64::from(c.voices[vi].melody.max_leap), by, 0, 127)? as u8
        }
        Interval => {
            c.voices[vi].melody.interval =
                number(i64::from(c.voices[vi].melody.interval), by, -24, 24)? as i16
        }
        BassRole => c.voices[vi].bass.role = c.voices[vi].bass.role.cycle(by.signum()),
        BassStyle => {
            c.voices[vi].bass.style = c.voices[vi].bass.style.cycle(by.signum());
            let v = &mut c.voices[vi];
            match v.bass.style {
                crate::midi_lab::composer::BassStyle::Jazz => {
                    v.bass.role = crate::midi_lab::composer::BassRole::Walking;
                    v.bass.approaches = crate::midi_lab::composer::Decoration::Chromatic;
                }
                crate::midi_lab::composer::BassStyle::Funk => {
                    v.bass.role = crate::midi_lab::composer::BassRole::Riff;
                    v.rhythm.kind = RhythmKind::Syncopated;
                    v.rhythm.gate = 55;
                }
                crate::midi_lab::composer::BassStyle::Rock => {
                    v.bass.role = crate::midi_lab::composer::BassRole::Foundation;
                    v.rhythm.kind = RhythmKind::Eighth;
                }
                crate::midi_lab::composer::BassStyle::Electronic => {
                    v.bass.role = crate::midi_lab::composer::BassRole::Sub;
                    v.rhythm.kind = RhythmKind::Hold;
                }
                _ => {
                    v.bass.role = crate::midi_lab::composer::BassRole::Foundation;
                    v.rhythm.kind = RhythmKind::Quarter;
                }
            }
        }
        Groove => c.voices[vi].bass.groove = c.voices[vi].bass.groove.cycle(by.signum()),
        Pedal => {
            c.voices[vi].bass.pedal = number(i64::from(c.voices[vi].bass.pedal), by, 0, 127)? as u8
        }
        Approaches => {
            c.voices[vi].bass.approaches = c.voices[vi].bass.approaches.cycle(by.signum())
        }
        FillEvery => {
            c.voices[vi].bass.fill_every =
                number(i64::from(c.voices[vi].bass.fill_every), by, 0, 32)? as u8
        }
        FillBeats => {
            c.voices[vi].bass.fill_beats =
                number(i64::from(c.voices[vi].bass.fill_beats), by, 0, 8)? as u8
        }
        Space => c.voices[vi].bass.leave_melody_space = !c.voices[vi].bass.leave_melody_space,
        Action => s.action = s.action.cycle(by.signum()),
        Candidate => {
            if s.candidates.is_empty() {
                return Err("Explore alternatives first".into());
            }
            s.candidate = cycling(s.candidate, s.candidates.len());
        }
        Motif => {
            if c.motifs.is_empty() {
                return Err("Capture a motif first".into());
            }
            s.motif = cycling(s.motif, c.motifs.len());
        }
        CaptureStart => {
            s.capture_start = number(
                i64::from(s.capture_start),
                by * 12,
                0,
                i64::from(c.length - 1),
            )? as u32
        }
        CaptureEnd => {
            s.capture_end = number(
                i64::from(s.capture_end),
                by * 12,
                i64::from(s.capture_start + 1),
                i64::from(c.length),
            )? as u32
        }
        PlaceStart => {
            s.place_start = number(
                i64::from(s.place_start),
                by * 12,
                0,
                i64::from(c.length - 1),
            )? as u32
        }
        Anchor => s.anchor = number(i64::from(s.anchor), by, 0, 127)? as u8,
        Transform => s.transform = cycling(s.transform, 10),
        TransformValue => s.amount = number(i64::from(s.amount), by, -127, 127)? as i16,
        Species => c.voices[vi].counter.species = c.voices[vi].counter.species.cycle(by.signum()),
        CanonInterval => {
            c.voices[vi].counter.interval =
                number(i64::from(c.voices[vi].counter.interval), by, -48, 48)? as i16
        }
        Delay => {
            c.voices[vi].counter.delay = number(
                i64::from(c.voices[vi].counter.delay),
                by * 12,
                0,
                i64::from(c.length),
            )? as u32
        }
        Invertible => {
            let values = [None, Some(12), Some(16), Some(19)];
            c.voices[vi].counter.invertible = values[cycling(
                values
                    .iter()
                    .position(|n| *n == c.voices[vi].counter.invertible)
                    .unwrap_or(0),
                4,
            )];
        }
        Strict => c.voices[vi].counter.strict = !c.voices[vi].counter.strict,
        Section => {
            if c.sections.is_empty() {
                return Err("Add a section first".into());
            }
            s.section = cycling(s.section, c.sections.len());
        }
        SectionStart => {
            let section = c.sections.get_mut(s.section).ok_or("Select a section")?;
            section.start = number(
                i64::from(section.start),
                by * 48,
                0,
                i64::from(c.length.saturating_sub(section.length)),
            )? as u32;
        }
        SectionLength => {
            let section = c.sections.get_mut(s.section).ok_or("Select a section")?;
            section.length = number(
                i64::from(section.length),
                by * 48,
                1,
                i64::from(c.length - section.start),
            )? as u32;
        }
        SectionSource => {
            let values = std::iter::once(None)
                .chain(
                    c.sections
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i != s.section)
                        .map(|(_, s)| Some(s.id)),
                )
                .collect::<Vec<_>>();
            let section = c.sections.get_mut(s.section).ok_or("Select a section")?;
            section.source = values[cycling(
                values
                    .iter()
                    .position(|id| *id == section.source)
                    .unwrap_or(0),
                values.len(),
            )];
        }
        SectionTranspose => {
            let section = c.sections.get_mut(s.section).ok_or("Select a section")?;
            section.transpose = number(i64::from(section.transpose), by, -48, 48)? as i16;
        }
        SectionDiatonic => {
            let section = c.sections.get_mut(s.section).ok_or("Select a section")?;
            section.diatonic = !section.diatonic;
        }
        SectionSimple => {
            let section = c.sections.get_mut(s.section).ok_or("Select a section")?;
            section.simplify_bass = !section.simplify_bass;
        }
        NotePitch | NoteStart | NoteLength | NoteVelocity => {
            let note = s
                .result
                .as_ref()
                .and_then(|r| r.notes.iter().find(|n| Some(n.id) == s.note))
                .ok_or("Select a note in the score")?;
            let mut edit = c
                .overrides
                .iter()
                .find(|o| o.id == note.id)
                .cloned()
                .unwrap_or(Override {
                    id: note.id,
                    instance: !c.form.is_empty(),
                    ..Override::default()
                });
            match field {
                NotePitch => edit.pitch = Some(number(i64::from(note.pitch), by, 0, 127)? as u8),
                NoteStart => {
                    edit.start = Some(number(
                        i64::from(note.start),
                        by,
                        0,
                        i64::from(s.result.as_ref().map_or(c.length, |r| r.length) - note.length),
                    )? as u32)
                }
                NoteLength => {
                    edit.length = Some(number(
                        i64::from(note.length),
                        by,
                        1,
                        i64::from(s.result.as_ref().map_or(c.length, |r| r.length) - note.start),
                    )? as u32)
                }
                _ => edit.velocity = Some(number(i64::from(note.velocity), by, 1, 127)? as u8),
            }
            c.overrides.retain(|o| o.id != note.id);
            c.overrides.push(edit);
        }
        Length => {
            c.length = number(
                i64::from(c.length),
                by * 48,
                i64::from(
                    c.harmony
                        .iter()
                        .map(|h| h.start + h.length)
                        .max()
                        .unwrap_or(1),
                ),
                i64::from(MAX_TICKS),
            )? as u32
        }
        Looping => c.looping = !c.looping,
        Output => c.output = c.output.cycle(by.signum()),
        _ => return Err("Press Enter to use this action".into()),
    }
    s.chase = true;
    Ok(())
}

pub(super) fn text_edit(
    c: &mut Composition,
    s: &mut State,
    field: Control,
    text: &str,
) -> Result<(), String> {
    use Control::*;
    let vi = voice(s).index();
    let numbers = |text: &str| -> Result<Vec<i16>, String> {
        text.split(',')
            .filter(|v| !v.trim().is_empty())
            .map(|v| {
                v.trim()
                    .parse::<i16>()
                    .map_err(|_| "Use comma-separated integers".into())
            })
            .collect()
    };
    match field {
        KickSource => {
            let (tag, pitch) = text
                .trim()
                .split_once(':')
                .ok_or("Use clip-tag:MIDI-pitch, for example b0:36")?;
            let pitch = pitch.parse::<u8>().map_err(|_| "Invalid kick MIDI pitch")?;
            let (_, dest, length, notes) = s
                .clip_sources
                .iter()
                .find(|(name, _, _, _)| name.eq_ignore_ascii_case(tag))
                .ok_or("Use an existing clip with straight timing and unconditional notes")?;
            let mut attacks = notes
                .iter()
                .filter(|n| n.pitch == pitch && n.start < *length)
                .map(|n| n.start)
                .collect::<Vec<_>>();
            attacks.sort();
            attacks.dedup();
            if attacks.is_empty() {
                return Err("The selected clip has no attacks at that pitch".into());
            }
            let bass = &mut c.voices[vi].bass;
            bass.kick_source = Some(*dest);
            bass.kick_note = pitch;
            bass.kick_length = *length;
            bass.kick = attacks;
        }

        Destination => {
            c.destinations[vi] = if text.eq_ignore_ascii_case("primary")
                || text.eq_ignore_ascii_case("primary clip")
            {
                None
            } else {
                Some(
                    s.destinations
                        .iter()
                        .find(|(tag, _)| tag.eq_ignore_ascii_case(text.trim()))
                        .ok_or("Enter an existing clip tag, or primary")?
                        .1,
                )
            };
        }
        CustomKey => {
            let k = c.key.as_mut().ok_or("Enable tonal context")?;
            k.custom = numbers(text)?
                .into_iter()
                .map(|n| u8::try_from(n).map_err(|_| "Scale intervals must be 0–11"))
                .collect::<Result<Vec<_>, _>>()?;
            k.validate()?;
        }
        Groups => {
            c.meter.groups = numbers(text)?
                .into_iter()
                .map(|n| u8::try_from(n).map_err(|_| "Beat groups must be positive"))
                .collect::<Result<Vec<_>, _>>()?;
            c.meter.bar()?;
        }
        ModulationKey => {
            let (root, mode) = text
                .split_once(':')
                .ok_or("Use tonic:mode, for example G:Ionian")?;
            let (pc, _, _) = crate::theory::material::pitch(root)?;
            let mode = crate::theory::functional::Mode::ALL
                .iter()
                .find(|m| {
                    m.label().eq_ignore_ascii_case(mode.trim())
                        || format!("{m:?}").eq_ignore_ascii_case(mode.trim())
                })
                .ok_or("Unknown destination mode")?;
            s.modulation_key = Key {
                tonic: pc,
                mode: *mode,
                ..Key::default()
            };
        }
        Omitted | Doubled => {
            let ids = text
                .split(',')
                .filter(|v| !v.trim().is_empty())
                .map(|n| {
                    n.trim()
                        .parse::<u64>()
                        .map_err(|_| "Use comma-separated member IDs")
                })
                .collect::<Result<Vec<_>, _>>()?;
            let h = c.harmony.get_mut(s.chord).ok_or("Select a chord")?;
            if field == Omitted {
                h.voicing.omitted = ids;
            } else {
                h.voicing.doubled = ids;
            }
        }
        MemberOffsets => {
            let mut offsets = BTreeMap::new();
            for item in text.split(',').filter(|v| !v.trim().is_empty()) {
                let (id, n) = item
                    .trim()
                    .split_once('/')
                    .ok_or("Use member-id/octaves entries")?;
                offsets.insert(
                    id.parse::<u64>().map_err(|_| "Invalid member ID")?,
                    n.parse::<i16>().map_err(|_| "Invalid octave offset")?,
                );
            }
            c.harmony
                .get_mut(s.chord)
                .ok_or("Select a chord")?
                .voicing
                .offsets = offsets;
        }

        Entry => {
            let material = if let Some(function) = text.strip_prefix("fn:") {
                crate::theory::functional::from_function(
                    function,
                    c.key_at(c.harmony.get(s.chord).map_or(0, |h| h.start))
                        .ok_or("Function entry needs a key")?,
                )?
            } else {
                Material::parse(text)?
            };
            let h = c.harmony.get_mut(s.chord).ok_or("Select a chord")?;
            h.replaced = Some(h.material.clone());
            h.material = material;
            h.voicing = Voicing::default();
            h.operation = Some(format!("Entered {text}"));
        }
        Progression => {
            c.progression(text)?;
            s.chord = 0;
        }
        BassPitch => {
            let bass = if text.trim().is_empty() || text.trim().eq_ignore_ascii_case("none") {
                None
            } else {
                let (pc, pitch, _) = crate::theory::material::pitch(text)?;
                Some(pitch.map_or(Bass::Class(pc), Bass::Pitch))
            };
            c.harmony
                .get_mut(s.chord)
                .ok_or("Select a chord")?
                .material
                .bass = bass;
        }
        Meter => {
            let (a, b) = text.split_once('/').ok_or("Use metre such as 4/4 or 7/8")?;
            let numerator = a.parse().map_err(|_| "Invalid metre numerator")?;
            let denominator = b.parse().map_err(|_| "Invalid metre denominator")?;
            let meter = crate::midi_lab::composer::Meter {
                numerator,
                denominator,
                groups: vec![numerator],
            };
            meter.bar()?;
            c.meter = meter;
        }
        WrittenRhythm => {
            let mut notes = Vec::new();
            for (i, part) in text.split(',').filter(|p| !p.trim().is_empty()).enumerate() {
                let (at, length) = part
                    .trim()
                    .split_once('/')
                    .ok_or("Use onset/duration pairs, for example 0/24,36/12,96/36")?;
                let start = at.parse::<u32>().map_err(|_| "Invalid rhythm onset")?;
                let tied = length.ends_with('~');
                let length = length
                    .trim_end_matches('~')
                    .parse::<u32>()
                    .map_err(|_| "Invalid rhythm duration")?;
                if length == 0
                    || start
                        .checked_add(length)
                        .is_none_or(|end| end > c.meter.bar().unwrap_or(192))
                {
                    return Err("Written rhythm must fit one bar".into());
                }
                notes.push(Pulse {
                    id: i as u64 + 1,
                    start,
                    length,
                    velocity: c.voices[vi].velocity,
                    tied,
                });
            }
            let bar = c.meter.bar()?;
            let v = &mut c.voices[vi];
            v.rhythm.kind = RhythmKind::Custom;
            v.rhythm.custom = notes;
            v.rhythm.custom_length = bar;
        }
        Offsets => c.voices[vi].rhythm.offsets = numbers(text)?,
        Accents => c.voices[vi].rhythm.accents = numbers(text)?,
        Kick => {
            let (ticks, length) = text
                .split_once('/')
                .ok_or("Use kick ticks / cell length, for example 0,72,96 / 192")?;
            let kick = numbers(ticks)?
                .into_iter()
                .map(|n| u32::try_from(n).map_err(|_| "Kick ticks must be nonnegative"))
                .collect::<Result<Vec<_>, _>>()?;
            let length = length
                .trim()
                .parse::<u32>()
                .map_err(|_| "Invalid kick cell length")?;
            if length == 0 || kick.iter().any(|n| *n >= length) {
                return Err("Kick attacks must fit their cell".into());
            }
            c.voices[vi].bass.kick = kick;
            c.voices[vi].bass.kick_length = length;
        }
        Tuning => {
            let pitches = numbers(text)?
                .into_iter()
                .map(|n| {
                    u8::try_from(n)
                        .ok()
                        .filter(|p| *p <= 127)
                        .ok_or("Tuning pitches must be 0–127")
                })
                .collect::<Result<Vec<_>, _>>()?;
            c.voices[vi].bass.tuning = pitches;
        }
        Name => s.name = text.to_owned(),
        Title => c.name = text.to_owned(),
        Path => s.path = text.to_owned(),
        SectionName => {
            c.sections
                .get_mut(s.section)
                .ok_or("Select a section")?
                .name = text.to_owned()
        }
        Form => {
            let names = if text.contains(' ') {
                text.split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            } else if c.sections.iter().any(|s| s.name == text) {
                vec![text.into()]
            } else {
                text.chars().map(|c| c.to_string()).collect()
            };
            let mut form = Vec::new();
            for name in names {
                form.push(
                    c.sections
                        .iter()
                        .find(|s| s.name == name)
                        .ok_or_else(|| format!("No section named {name}"))?
                        .id,
                );
            }
            c.form = form;
        }
        Placements => {
            let mut placements = Vec::new();
            for item in text.split_whitespace() {
                let fields = item.split(':').collect::<Vec<_>>();
                if fields.len() != 3 {
                    return Err("Use motif-id:start-tick:anchor-pitch entries".into());
                }
                let motif = fields[0].parse::<u64>().map_err(|_| "Invalid motif ID")?;
                if !c.motifs.iter().any(|m| m.id == motif) {
                    return Err("Placement motif is missing".into());
                }
                let start = fields[1].parse().map_err(|_| "Invalid placement start")?;
                let anchor = fields[2].parse().map_err(|_| "Invalid anchor pitch")?;
                let id = c.mint();
                placements.push(Placement {
                    id,
                    motif,
                    voice: voice(s),
                    start,
                    anchor,
                    transforms: vec![],
                });
            }
            c.placements.retain(|p| p.voice != voice(s));
            c.placements.extend(placements);
        }
        _ => return Err("This field does not accept text".into()),
    }
    s.edit = None;
    s.texts.remove(&field);
    Ok(())
}

pub(super) fn act(c: &mut Composition, s: &mut State, field: Control) -> Result<String, String> {
    use Control::*;
    let vi = voice(s).index();
    match field {
        PinVoice | PinPhrase => {
            let rendered = render(c)?;
            let voice = voice(s);
            let notes = rendered.notes.iter().filter(|n| {
                n.voice == voice
                    && (field == PinVoice
                        || (n.start >= s.capture_start && n.start < s.capture_end))
            });
            for n in notes {
                c.pin(n);
            }
        }
        UnpinVoice => {
            let rendered = render(c)?;
            let ids = rendered
                .notes
                .iter()
                .filter(|n| n.voice == voice(s))
                .map(|n| n.id)
                .collect::<std::collections::BTreeSet<_>>();
            c.overrides.retain(|o| !ids.contains(&o.id));
        }
        RestoreComparison => {
            let snapshot = c
                .comparisons
                .get(s.saved)
                .ok_or("Select a saved comparison")?
                .clone();
            let mut restored: Composition =
                ron::from_str(&snapshot.input).map_err(|e| e.to_string())?;
            restored.comparisons = c.comparisons.clone();
            restored.snapshot = Some(snapshot);
            if restored.engine != ENGINE_VERSION {
                restored.frozen = true;
            }
            restored.validate()?;
            s.reference = Some(Box::new(c.clone()));
            *c = restored;
        }

        CaptureInput => {
            if s.captured.is_empty() {
                return Err("Enable MIDI capture and play some notes first".into());
            }
            let material = crate::midi_lab::composer::capture::material(&s.captured)?;
            let h = c.harmony.get_mut(s.chord).ok_or("Select a chord")?;
            h.replaced = Some(h.material.clone());
            h.material = material;
            h.voicing = Voicing::default();
            h.operation = Some("Captured from MIDI input".into());
            s.input = false;
        }
        LiftClip => {
            if let Some(error) = &s.lift_error {
                return Err(error.clone());
            }
            if s.lifted.is_empty() {
                return Err("The primary clip has no notes to lift".into());
            }
            let notes = s
                .lifted
                .iter()
                .map(|n| {
                    let mut n = n.clone();
                    n.voice = voice(s);
                    n
                })
                .collect::<Vec<_>>();
            let end = notes.iter().map(NoteEvent::end).max().unwrap_or(1);
            motif::capture(c, &notes, voice(s), 0, end, s.name.clone())?;
            s.motif = c.motifs.len() - 1;
        }

        AddChord => {
            let start = c
                .harmony
                .iter()
                .map(|h| h.start + h.length)
                .max()
                .unwrap_or(0);
            let length = c.meter.bar()?;
            if start + length > MAX_TICKS {
                return Err("Timeline is full".into());
            }
            let material = c
                .harmony
                .get(s.chord)
                .map_or_else(|| Material::parse("C"), |h| Ok(h.material.clone()))?;
            let id = c.mint();
            c.harmony.push(HarmonySpan {
                id,
                start,
                length,
                material,
                voicing: Voicing::default(),
                operation: Some("Added chord".into()),
                replaced: None,
            });
            c.length = c.length.max(start + length);
            s.chord = c.harmony.len() - 1;
        }
        RemoveChord => {
            if s.chord >= c.harmony.len() {
                return Err("Select a chord".into());
            }
            c.harmony.remove(s.chord);
            s.chord = s.chord.min(c.harmony.len().saturating_sub(1));
        }
        ApplyCadence => {
            let chords = crate::theory::functional::Cadence::ALL[s.cadence]
                .chords(c.key.as_ref().ok_or("Cadence needs tonal context")?)?;
            let length = c.meter.bar()?;
            let mut start = c
                .harmony
                .iter()
                .map(|h| h.start + h.length)
                .max()
                .unwrap_or(0);
            for material in chords {
                let id = c.mint();
                c.harmony.push(HarmonySpan {
                    id,
                    start,
                    length,
                    material,
                    voicing: Voicing::default(),
                    operation: Some(format!(
                        "{} cadence",
                        crate::theory::functional::Cadence::ALL[s.cadence].label()
                    )),
                    replaced: None,
                });
                start += length;
            }
            if start > MAX_TICKS {
                return Err("Cadence exceeds timeline capacity".into());
            }
            c.length = c.length.max(start);
        }
        ApplySubstitution => {
            let h = c.harmony.get_mut(s.chord).ok_or("Select a chord")?;
            let op = crate::theory::functional::Substitution::ALL[s.substitution];
            let material = op.apply(&h.material)?;
            h.replaced = Some(h.material.clone());
            h.material = material;
            h.operation = Some(format!("{} substitution", op.label()));
        }
        AddModulation => {
            let key = s.modulation_key.clone();
            let at = s.modulation;
            let prior = c
                .modulations
                .iter()
                .filter(|m| m.start < at)
                .max_by_key(|m| m.start)
                .map(|m| &m.key)
                .or(c.key.as_ref());
            let harmony = c.harmony_at(at).ok_or("Place modulation inside harmony")?;
            if s.pivot == crate::midi_lab::composer::Pivot::CommonChord
                && prior.is_some_and(|before| {
                    harmony
                        .material
                        .members
                        .iter()
                        .any(|m| !before.contains(m.pc) || !key.contains(m.pc))
                })
            {
                return Err("Selected chord is not common to both keys".into());
            }
            let common =
                prior.and_then(|before| (0..12).find(|p| before.contains(*p) && key.contains(*p)));
            if s.pivot == crate::midi_lab::composer::Pivot::CommonTone && common.is_none() {
                return Err("The keys have no common tone".into());
            }
            let id = c.mint();
            c.modulations.push(crate::midi_lab::composer::Modulation {
                id,
                start: at,
                key,
                pivot: s.pivot,
                pitch: common,
            });
        }
        Explore => {
            s.candidates = alternatives::explore(c, voice(s), s.action, 6)?;
            s.explored_input = c.input()?;
            s.candidate = 0;
            s.reference = Some(Box::new(c.clone()));
            return Ok(format!("{} distinct alternatives", s.candidates.len()));
        }
        Accept => {
            if c.input()? != s.explored_input {
                return Err("The composition changed since these alternatives; explore again to keep current edits".into());
            }
            let candidate = s
                .candidates
                .get(s.candidate)
                .ok_or("Explore alternatives first")?
                .clone();
            let original = c.clone();
            *c = candidate.recipe;
            s.reference = Some(Box::new(original));
            return Ok(candidate.differences.join(" · "));
        }
        Compare => {
            let other = s
                .reference
                .take()
                .ok_or("Choose an alternative or save an A/B reference first")?;
            let current = c.clone();
            *c = *other;
            s.reference = Some(Box::new(current));
            return Ok("Switched A / B".into());
        }
        SaveComparison => {
            let rendered = render(c)?;
            let snapshot = Snapshot {
                harmony: rendered.harmony.clone(),
                voicings: rendered.voicings.clone(),
                length: rendered.length,
                input: c.input()?,
                events: rendered.notes,
                label: alternatives::describe(c, voice(s)),
            };
            if c.comparisons.len() >= 32 {
                return Err("Comparison bank holds 32 versions".into());
            }
            c.comparisons.push(snapshot);
            s.reference = Some(Box::new(c.clone()));
        }
        Capture => {
            let rendered = render(c)?;
            motif::capture(
                c,
                &rendered.notes,
                voice(s),
                s.capture_start,
                s.capture_end,
                s.name.clone(),
            )?;
            s.motif = c.motifs.len() - 1;
        }
        Place => {
            let motif = c
                .motifs
                .get(s.motif)
                .ok_or("Capture or select a motif")?
                .clone();
            let transform = match s.transform {
                0 => crate::midi_lab::composer::Transform::Transpose(s.amount),
                1 => crate::midi_lab::composer::Transform::Diatonic(s.amount),
                2 => crate::midi_lab::composer::Transform::Invert(i16::from(s.anchor)),
                3 => crate::midi_lab::composer::Transform::Retrograde,
                4 => crate::midi_lab::composer::Transform::ScaleTime(2, 1),
                5 => crate::midi_lab::composer::Transform::ScaleTime(1, 2),
                6 => {
                    crate::midi_lab::composer::Transform::Rotate(u32::from(s.amount.unsigned_abs()))
                }
                7 => crate::midi_lab::composer::Transform::Fragment(0, motif.length / 2),
                8 => crate::midi_lab::composer::Transform::Sequence {
                    interval: s.amount,
                    repeats: 2,
                },
                _ => crate::midi_lab::composer::Transform::Degree(s.amount),
            };
            let id = c.mint();
            let placement = Placement {
                id,
                motif: motif.id,
                voice: voice(s),
                start: s.place_start,
                anchor: s.anchor,
                transforms: vec![transform],
            };
            motif::place(c, &placement)?;
            c.placements.push(placement);
            c.voices[vi].enabled = true;
        }
        AddSection => {
            let id = c.mint();
            let n = c.sections.len();
            let start = s.capture_start.min(c.length - 1);
            let length = s.capture_end.min(c.length).saturating_sub(start).max(1);
            c.sections.push(crate::midi_lab::composer::Section {
                id,
                name: char::from_u32('A' as u32 + n as u32)
                    .unwrap_or('A')
                    .to_string(),
                start,
                length,
                source: None,
                transpose: 0,
                diatonic: false,
                enabled: [true; 5],
                simplify_bass: false,
            });
            s.section = n;
        }
        Pin => {
            let note = s
                .result
                .as_ref()
                .and_then(|r| r.notes.iter().find(|n| Some(n.id) == s.note))
                .ok_or("Select a note")?;
            c.pin(note);
        }
        Unpin => {
            let id = s.note.ok_or("Select a note")?;
            let before = c.overrides.len();
            c.overrides.retain(|o| o.id != id);
            if before == c.overrides.len() {
                return Err("This note is not locked".into());
            }
        }
        DeleteNote => {
            c.remove_note(s.note.ok_or("Select a note")?);
            s.note = None;
        }
        Save => {
            alternatives::save_snapshot(c, c.name.clone())?;
            let text = ron::ser::to_string_pretty(c, ron::ser::PrettyConfig::default())
                .map_err(|e| e.to_string())?;
            if text.len() > 32 * 1024 * 1024 {
                return Err(
                    "Recipe exceeds the 32 MiB save capacity; reduce the comparison bank".into(),
                );
            }
            std::fs::write(&s.path, text).map_err(|e| format!("Cannot save recipe: {e}"))?;
            return Ok(format!("Saved {}", s.path));
        }
        Load => {
            if std::fs::metadata(&s.path).map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
                return Err("Recipe exceeds 32 MiB".into());
            }
            let text =
                std::fs::read_to_string(&s.path).map_err(|e| format!("Cannot open recipe: {e}"))?;
            if text.len() > 32 * 1024 * 1024 {
                return Err("Recipe exceeds 32 MiB".into());
            }
            let mut loaded: Composition = ron::from_str(&text).map_err(|e| e.to_string())?;
            if loaded.engine != ENGINE_VERSION && loaded.snapshot.is_some() {
                loaded.frozen = true;
            }
            loaded.validate()?;
            *c = loaded;
            s.texts.clear();
            s.chord = 0;
            return Ok(format!("Loaded {}", s.path));
        }
        Export => {
            let rendered = render(c)?;
            let bytes = output::midi_with_context(&rendered, s.tempo, &s.tempo_marks, &c.meter)?;
            let path = std::path::Path::new(&s.path).with_extension("mid");
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            return Ok(format!("Exported {}", path.display()));
        }
        Thaw => {
            bridge::thaw(c)?;
            return Ok("Saved notes adopted as absolute, editable motif placements".into());
        }
        _ => {
            turn(c, s, field, 1)?;
            return Ok(format!("{} · {}", field.label(), reading(field, c, s)));
        }
    }
    Ok(field.label())
}

impl Stage {
    pub(super) fn has_composer(&self) -> bool {
        self.lab
            .focus
            .and_then(|i| self.lab.window(i))
            .and_then(|w| {
                if let Instrument::Midi(m) = &w.instrument {
                    self.song.midi_labs.iter().find(|d| d.id == m.draft)
                } else {
                    None
                }
            })
            .is_some_and(|d| d.recipe.composition.is_some())
    }
    pub(super) fn composer_key(
        &mut self,
        step: Option<Step>,
        editing: bool,
        enter: bool,
    ) -> Result<(), super::RefusalReason> {
        let window = self.lab.focus.ok_or(super::RefusalReason::Empty)?;
        let Instrument::Midi(mut state) = self
            .lab
            .window(window)
            .ok_or(super::RefusalReason::Empty)?
            .instrument
            .clone()
        else {
            return Err(super::RefusalReason::Unavailable);
        };
        let index = self
            .song
            .midi_labs
            .iter()
            .position(|d| d.id == state.draft)
            .ok_or(super::RefusalReason::Empty)?;
        let original_state = state.clone();
        self.composer_context(&mut state.composer, self.song.midi_labs[index].destination);
        let mut c = self.song.midi_labs[index]
            .recipe
            .composition
            .as_ref()
            .ok_or(super::RefusalReason::Unavailable)?
            .as_ref()
            .clone();
        let before = c.clone();
        let fields = controls(&state.composer, &c);
        let focus = state.composer.focus.min(fields.len().saturating_sub(1));
        let field = *fields.get(focus).ok_or(super::RefusalReason::Empty)?;
        let result = if state.composer.score_focus {
            score_key(&mut c, &mut state.composer, step, editing, enter)
        } else if state.composer.picker.is_some() {
            let options = options(&c, &state.composer, field);
            let choices = options
                .iter()
                .enumerate()
                .filter(|(_, label)| {
                    label
                        .to_lowercase()
                        .contains(&state.composer.query.to_lowercase())
                })
                .collect::<Vec<_>>();
            if enter {
                let offset = choices
                    .get(state.composer.picker_index)
                    .map_or(0, |(offset, _)| *offset);
                let mut result = Ok(());
                for _ in 0..offset {
                    result = turn(&mut c, &mut state.composer, field, 1);
                    if result.is_err() {
                        break;
                    }
                }
                state.composer.picker = None;
                result.map(|_| "Choice selected".into())
            } else {
                if let Some(step) = step {
                    if matches!(step, Step::Down | Step::Right) {
                        state.composer.picker_index =
                            (state.composer.picker_index + 1).min(choices.len().saturating_sub(1));
                    } else {
                        state.composer.picker_index = state.composer.picker_index.saturating_sub(1);
                    }
                }
                Ok("Choose an option · Enter applies · Escape closes".into())
            }
        } else if enter {
            if field == Control::Separate {
                return self.composer_separate(window);
            }
            if choice(field) {
                state.composer.picker = Some(field);
                state.composer.query.clear();
                state.composer.picker_index = 0;
                Ok("Search choices · Enter selects".into())
            } else if field.text() {
                state.composer.edit = Some(field);
                Ok(format!("{} · type a value and press Enter", field.label()))
            } else {
                act(&mut c, &mut state.composer, field)
            }
        } else if editing {
            let by = match step {
                Some(Step::Up) => 12,
                Some(Step::Down) => -12,
                Some(Step::Right) => 1,
                _ => -1,
            };
            turn(&mut c, &mut state.composer, field, by).map(|_| {
                format!(
                    "{} · {}",
                    field.label(),
                    reading(field, &c, &state.composer)
                )
            })
        } else {
            let by = if matches!(step, Some(Step::Right | Step::Down)) {
                1
            } else {
                -1
            };
            match number(focus as i64, by, 0, fields.len().saturating_sub(1) as i64) {
                Ok(next) => {
                    state.composer.focus = next as usize;
                    state.composer.chase = true;
                    let next = fields[next as usize];
                    Ok(format!(
                        "{} · {} · Shift+arrows edit · Enter acts",
                        next.label(),
                        reading(next, &c, &state.composer)
                    ))
                }
                Err(e) => Err(e),
            }
        };
        let result = result.and_then(|status| c.validate().map(|_| status));
        match &result {
            Ok(status) => {
                state.status = status.clone();
                if c != before {
                    state.cancel();
                    self.song.midi_labs[index].recipe.composition = Some(Box::new(c));
                }
            }
            Err(e) => {
                // An error may update feedback, but never commits a partial edit.
                // Changed reports that visible feedback; repeated feedback refuses
                // with the original state intact.
                if original_state.status == *e {
                    return Err(super::RefusalReason::Unavailable);
                }
                state = original_state;
                state.status = e.clone();
            }
        };
        if let Some(w) = self.lab.window_mut(window) {
            w.instrument = Instrument::Midi(state);
        }
        self.settle();
        Ok(())
    }
    fn composer_track(
        song: &mut crate::sequencing::Song,
        source: usize,
        name: &str,
    ) -> Result<crate::midi_lab::Destination, super::RefusalReason> {
        let at = song
            .duplicate_track(source)
            .ok_or(super::RefusalReason::Unavailable)?;
        song.tracks[at].blocks.clear();
        song.tracks[at].audio_blocks.clear();
        song.rename_track(at, name);
        let pattern = song
            .fill_slot(at, 0)
            .ok_or(super::RefusalReason::Unavailable)?;
        Ok(crate::midi_lab::Destination {
            track: song.tracks[at].id,
            pattern,
        })
    }
    pub(super) fn composer_separate(&mut self, window: usize) -> Result<(), super::RefusalReason> {
        let Some(Instrument::Midi(state)) = self.lab.window(window).map(|w| w.instrument.clone())
        else {
            return Err(super::RefusalReason::Unavailable);
        };
        let Some(index) = self.song.midi_labs.iter().position(|d| d.id == state.draft) else {
            return Err(super::RefusalReason::Empty);
        };
        let Some(primary) = self.song.midi_labs[index].destination else {
            return Err(super::RefusalReason::Empty);
        };
        let Some(source) = self.song.tracks.iter().position(|t| t.id == primary.track) else {
            return Err(super::RefusalReason::Empty);
        };
        let mut song = self.song.clone();
        let mut c = song.midi_labs[index]
            .recipe
            .composition
            .as_ref()
            .ok_or(super::RefusalReason::Unavailable)?
            .as_ref()
            .clone();
        let rendered = render(&c).map_err(|_| super::RefusalReason::Unavailable)?;
        let mut used_tracks = std::collections::BTreeSet::new();
        for voice in crate::midi_lab::Voice::ALL {
            if !c.voices[voice.index()].enabled {
                continue;
            }
            let destination = c.destinations[voice.index()].unwrap_or(primary);
            if used_tracks.contains(&destination.track.0)
                || voice != crate::midi_lab::Voice::Chords && destination.track == primary.track
            {
                c.destinations[voice.index()] =
                    Some(Self::composer_track(&mut song, source, voice.label())?);
            }
            used_tracks.insert(c.destinations[voice.index()].unwrap_or(primary).track.0);
            let notes = rendered
                .notes
                .iter()
                .filter(|n| n.voice == voice)
                .cloned()
                .collect::<Vec<_>>();
            for (lane, events) in output::lanes(&notes).into_iter().enumerate().skip(1) {
                let existing = events
                    .iter()
                    .filter_map(|n| c.event_destinations.get(&n.id).copied())
                    .find(|d| {
                        !used_tracks.contains(&d.track.0)
                            && song.tracks.iter().any(|t| t.id == d.track)
                            && song.pattern(d.pattern).is_some()
                    });
                let destination = if let Some(d) = existing {
                    d
                } else {
                    Self::composer_track(
                        &mut song,
                        source,
                        &format!("{} {}", voice.label(), lane + 1),
                    )?
                };
                used_tracks.insert(destination.track.0);
                for note in events {
                    c.event_destinations.insert(note.id, destination);
                }
            }
        }
        song.midi_labs[index].recipe.composition = Some(Box::new(c));
        self.settle();
        self.song = song;
        self.touched();
        self.settle();
        if let Some(w) = self.lab.window_mut(window)
            && let Instrument::Midi(m) = &mut w.instrument
        {
            m.cancel();
            m.status="Enabled voices now have separate instrument tracks · Undo restores the previous routing".into();
        }
        Ok(())
    }
}

impl Stage {
    /// Control-side capture shares the existing MIDI drain with recording.
    pub fn midi_lab_input(&mut self, events: &[(u64, crate::midi_input::MidiEvent)]) {
        let Some(window) = self.lab.focus else {
            return;
        };
        let Some(w) = self.lab.window_mut(window) else {
            return;
        };
        let Instrument::Midi(m) = &mut w.instrument else {
            return;
        };
        if !m.composer.input {
            return;
        }
        for (_, event) in events {
            if let crate::midi_input::MidiEvent::NoteOn { note, velocity } = event {
                if *velocity > 0 && *note < 128 && m.composer.captured.len() < 128 {
                    m.composer.captured.push(*note);
                }
            }
        }
        if !events.is_empty() {
            m.status = format!(
                "Captured {} notes · Use captured chord to accept",
                m.composer.captured.len()
            );
        }
    }
    pub(super) fn composer_context(
        &self,
        state: &mut State,
        primary: Option<crate::midi_lab::Destination>,
    ) {
        let (bpm, marks) = primary.map_or((self.song.bpm, self.song.tempo.clone()), |d| {
            output::tempo_context(&self.song, d)
        });
        state.tempo = bpm;
        state.tempo_marks = marks;
        state.clip_sources.clear();
        state.destinations.clear();
        for track in &self.song.tracks {
            if track.machine.is_none() {
                continue;
            }
            for pattern in &self.song.patterns {
                if pattern
                    .tag
                    .strip_prefix(&track.letter)
                    .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
                {
                    if state.subject == Subject::Bass
                        && pattern.swing == 50
                        && pattern.scale == crate::sequencing::Scale::One
                        && (0..pattern.step_count()).all(|i| {
                            let t = pattern.trig(i);
                            t.retrig.is_none() && t.cond.is_none() && t.probability == 1.0
                        })
                    {
                        state.clip_sources.push((
                            pattern.tag.clone(),
                            crate::midi_lab::Destination {
                                track: track.id,
                                pattern: pattern.id,
                            },
                            pattern.length_ticks as u32,
                            capture::clip(pattern, &self.song.key),
                        ));
                    }
                    state.destinations.push((
                        pattern.tag.clone(),
                        crate::midi_lab::Destination {
                            track: track.id,
                            pattern: pattern.id,
                        },
                    ));
                }
            }
        }
        let lifted = primary
            .and_then(|d| self.song.pattern(d.pattern))
            .map(|p| capture::checked_clip(p, &self.song.key))
            .unwrap_or_else(|| Ok(vec![]));
        state.lift_error = lifted.as_ref().err().cloned();
        state.lifted = lifted.unwrap_or_default();
    }
}

pub(super) fn choice(field: Control) -> bool {
    matches!(
        field,
        Control::SavedComparison
            | Control::Interpretation
            | Control::Leader
            | Control::Profile
            | Control::Frame
            | Control::SectionVoice
            | Control::Subject
            | Control::Root
            | Control::Mode
            | Control::Cadence
            | Control::Substitution
            | Control::Pivot
            | Control::Layout
            | Control::Voice
            | Control::Articulation
            | Control::Rhythm
            | Control::Contour
            | Control::Movement
            | Control::Decoration
            | Control::Targets
            | Control::Development
            | Control::BassRole
            | Control::BassStyle
            | Control::Groove
            | Control::Approaches
            | Control::Action
            | Control::Transform
            | Control::Species
            | Control::Invertible
            | Control::Output
            | Control::Candidate
            | Control::Chord
            | Control::Motif
            | Control::Section
            | Control::SectionSource
    )
}
pub(super) fn options(c: &Composition, state: &State, field: Control) -> Vec<String> {
    let mut c = c.clone();
    let mut s = state.clone();
    let first = reading(field, &c, &s);
    let mut values = vec![first.clone()];
    for _ in 0..64 {
        if turn(&mut c, &mut s, field, 1).is_err() {
            break;
        }
        let value = reading(field, &c, &s);
        if value == first {
            break;
        }
        if !values.contains(&value) {
            values.push(value);
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progression_text_roundtrip_keeps_edited_subdivision_boundaries() {
        let mut c = Composition::default();
        c.progression("Cmaj7:193t Am7:47t").unwrap();
        let text = reading(Control::Progression, &c, &State::default());
        assert!(text.contains("193t"));
        c.progression(&text).unwrap();
        assert_eq!(
            c.harmony
                .iter()
                .map(|h| (h.start, h.length))
                .collect::<Vec<_>>(),
            [(0, 193), (193, 47)]
        );
        assert_eq!(c.length, 240);
    }
    #[test]
    fn separating_independent_unisons_reuses_the_same_tracks() {
        let mut stage = Stage::new();
        stage.open_midi_lab("a0");
        let c = stage.song.midi_labs[0].recipe.composition.as_mut().unwrap();
        c.harmony.truncate(1);
        c.harmony[0].material = Material::parse("notes:C4,C4,C4").unwrap();
        stage.composer_separate(0).unwrap();
        let before = stage.song.clone();
        assert_eq!(before.tracks.len(), 3);
        stage.composer_separate(0).unwrap();
        assert_eq!(stage.song, before);
        stage.midi_send(stage.song.midi_labs[0].id).unwrap();
    }
    #[test]
    fn every_inspector_field_is_reachable_and_keyboard_edits_have_effects() {
        let base = Composition::default();
        for subject in Subject::ALL {
            let mut state = State::default();
            state.subject = subject;
            for field in controls(&state, &base) {
                if field.action() || field.text() {
                    continue;
                }
                let mut c = base.clone();
                let mut s = state.clone();
                let before = reading(field, &c, &s);
                if turn(&mut c, &mut s, field, 1).is_ok() {
                    assert!(
                        c != base || reading(field, &c, &s) != before || s.edit.is_some(),
                        "{} has no effect",
                        field.label()
                    );
                }
            }
        }
    }
    #[test]
    fn separate_tracks_and_send_are_atomic_and_undoable() {
        let mut stage = Stage::new();
        stage.open_midi_lab("a0");
        let id = stage.song.midi_labs[0].id;
        let c = stage.song.midi_labs[0].recipe.composition.as_mut().unwrap();
        c.voices[2].enabled = true;
        c.voices[3].enabled = true;
        c.progression("Cmaj7:8 Am7:8 Dm7:8 G7:8").unwrap();
        stage.settle();
        stage.composer_separate(0).unwrap();
        let before = stage.song.clone();
        let result = stage.midi_send(id).unwrap();
        assert!(result.contains("32 beats"));
        let recipe = &stage.song.midi_labs[0].recipe;
        let c = recipe.composition.as_ref().unwrap();
        let rendered = render(c).unwrap();
        for voice in [
            crate::midi_lab::Voice::Chords,
            crate::midi_lab::Voice::Melody,
            crate::midi_lab::Voice::Bass,
        ] {
            let dest = c.destinations[voice.index()]
                .or(stage.song.midi_labs[0].destination)
                .unwrap();
            let pattern = stage.song.pattern(dest.pattern).unwrap();
            assert_eq!(pattern.length_ticks, 1536);
            let notes = capture::clip(pattern, &stage.song.key);
            assert_eq!(
                notes.len(),
                rendered.notes.iter().filter(|n| n.voice == voice).count()
            );
            let track = stage
                .song
                .tracks
                .iter()
                .find(|t| t.id == dest.track)
                .unwrap();
            assert!(
                track
                    .blocks
                    .iter()
                    .any(|b| b.pattern_id == dest.pattern && b.length_ticks >= 1536)
            );
        }
        let _ = stage.apply(super::super::StageIntent::Undo);
        assert_eq!(stage.song, before);
    }
    #[test]
    fn midi_capture_keeps_octaves_and_is_explicitly_accepted() {
        let mut stage = Stage::new();
        stage.open_midi_lab("a0");
        let original = stage.song.midi_labs[0].recipe.clone();
        if let Instrument::Midi(m) = &mut stage.lab.windows[0].instrument {
            m.composer.input = true;
        }
        stage.midi_lab_input(&[
            (
                0,
                crate::midi_input::MidiEvent::NoteOn {
                    note: 36,
                    velocity: 90,
                },
            ),
            (
                1,
                crate::midi_input::MidiEvent::NoteOn {
                    note: 72,
                    velocity: 90,
                },
            ),
        ]);
        assert_eq!(stage.song.midi_labs[0].recipe, original);
        let Instrument::Midi(m) = &mut stage.lab.windows[0].instrument else {
            panic!()
        };
        let mut c = Composition::default();
        act(&mut c, &mut m.composer, Control::CaptureInput).unwrap();
        assert_eq!(
            c.harmony[0]
                .material
                .members
                .iter()
                .map(|m| m.pitch.unwrap())
                .collect::<Vec<_>>(),
            [36, 72]
        );
    }
    #[test]
    fn destination_key_does_not_rewrite_the_initial_key() {
        let mut c = Composition::default();
        let mut s = State::default();
        s.modulation = 384;
        s.modulation_key = Key {
            tonic: 7,
            ..Key::default()
        };
        act(&mut c, &mut s, Control::AddModulation).unwrap();
        assert_eq!(c.key_at(0).unwrap().tonic, 0);
        assert_eq!(c.key_at(384).unwrap().tonic, 7);
    }
    #[test]
    fn stale_alternatives_cannot_overwrite_new_user_edits() {
        let mut c = Composition::default();
        c.voices[2].enabled = true;
        let mut s = State::default();
        s.subject = Subject::Melody;
        s.voice = crate::midi_lab::Voice::Melody;
        act(&mut c, &mut s, Control::Explore).unwrap();
        c.voices[2].velocity = 37;
        let before = c.clone();
        assert!(act(&mut c, &mut s, Control::Accept).is_err());
        assert_eq!(c, before);
    }
}

fn score_key(
    c: &mut Composition,
    s: &mut State,
    step: Option<Step>,
    editing: bool,
    enter: bool,
) -> Result<String, String> {
    if s.result.is_none() {
        s.result = Some(render(c)?);
    }
    if enter {
        s.score_focus = false;
        s.subject = Subject::Note;
        s.focus = 1;
        s.chase = true;
        return Ok("Inspecting the selected note".into());
    }
    let step = step.ok_or("Choose a score direction")?;
    if editing {
        let (field, by) = match step {
            Step::Up => (Control::NotePitch, 1),
            Step::Down => (Control::NotePitch, -1),
            Step::Right => (Control::NoteStart, 12),
            Step::Left => (Control::NoteStart, -12),
        };
        turn(c, s, field, by)?;
        return Ok(format!("{} · {}", field.label(), reading(field, c, s)));
    }
    if matches!(step, Step::Up | Step::Down) {
        s.voice = Voice::ALL[(s.voice.index() as i32 + if step == Step::Down { 1 } else { -1 })
            .rem_euclid(5) as usize];
        s.note = None;
    }
    let mut notes = s
        .result
        .as_ref()
        .ok_or("No score")?
        .notes
        .iter()
        .filter(|n| n.voice == s.voice)
        .collect::<Vec<_>>();
    notes.sort_by_key(|n| (n.start, n.pitch, n.id));
    if notes.is_empty() {
        return Ok(format!("{} has no notes", s.voice.label()));
    }
    let current = s.note.and_then(|id| notes.iter().position(|n| n.id == id));
    let index = if let Some(at) = current {
        match step {
            Step::Right => (at + 1).min(notes.len() - 1),
            Step::Left => at.saturating_sub(1),
            _ => at,
        }
    } else {
        0
    };
    s.note = Some(notes[index].id);
    if s.zoom > 0 {
        let at = notes[index].start;
        if at < s.scroll || at >= s.scroll + s.zoom {
            s.scroll = at / s.zoom * s.zoom;
        }
    }
    Ok(format!(
        "{} · MIDI {} at tick {}",
        s.voice.label(),
        notes[index].pitch,
        notes[index].start
    ))
}
