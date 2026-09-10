use super::{Stage, grid::Step, lab::Instrument};
use crate::midi_lab::{self, Destination, Draft, Recipe, Rhythm, Voice};
use crate::theory::harmony::HarmonicStyle;
use std::sync::Arc;

/// One control on the MIDI Lab's page that the keyboard can reach.
///
/// The lab was drawn for a pointer: rows of buttons, combo boxes, steppers
/// and drag targets, none of which the arrows could touch. This is the same
/// page enumerated — every control, in the order it is drawn — so a cursor
/// can walk it. The keyboard and the pointer edit the same recipe through
/// the same rules; neither is a second implementation of the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Field {
    Clip,
    Target,
    Chord,
    Root,
    Quality,
    Beats,
    Remove,
    Layout,
    Inversion,
    Octave,
    Lead,
    RangeLow,
    RangeHigh,
    Style,
    /// A degree of the chord, on or off.
    Member(u8),
    /// A tension above the seventh, on or off.
    Colour(u8),
    /// A degree displaced by octaves, and doubled with Enter.
    Spread(u8),
    Voice,
    On,
    Rhythm,
    Motion,
    Regenerate,
    Gate,
    Swing,
    Velocity,
    VoiceLow,
    VoiceHigh,
    Hits,
    Steps,
    Rotate,
    View,
    Snap,
    Length,
    Hear,
    Play,
    Send,
    Stop,
}

impl Field {
    pub(super) fn name(self) -> String {
        match self {
            Self::Clip => "Clip".into(),
            Self::Target => "Target".into(),
            Self::Chord => "Chord".into(),
            Self::Root => "Root".into(),
            Self::Quality => "Quality".into(),
            Self::Beats => "Beats".into(),
            Self::Remove => "Remove chord".into(),
            Self::Layout => "Voicing".into(),
            Self::Inversion => "Inversion".into(),
            Self::Octave => "Octave".into(),
            Self::Lead => "Voice leading".into(),
            Self::RangeLow => "Range low".into(),
            Self::RangeHigh => "Range high".into(),
            Self::Style => "Chord rules".into(),
            Self::Member(degree) => format!("Member {degree}"),
            Self::Colour(degree) => format!("Colour {degree}"),
            Self::Spread(degree) => format!("Spread {degree}"),
            Self::Voice => "Voice".into(),
            Self::On => "On".into(),
            Self::Rhythm => "Rhythm".into(),
            Self::Motion => "Motion".into(),
            Self::Regenerate => "Regenerate".into(),
            Self::Gate => "Gate".into(),
            Self::Swing => "Swing".into(),
            Self::Velocity => "Velocity".into(),
            Self::VoiceLow => "Voice low".into(),
            Self::VoiceHigh => "Voice high".into(),
            Self::Hits => "Hits".into(),
            Self::Steps => "Steps".into(),
            Self::Rotate => "Rotate".into(),
            Self::View => "View".into(),
            Self::Snap => "Snap".into(),
            Self::Length => "Clip length".into(),
            Self::Hear => "Hear chord".into(),
            Self::Play => "Play clip".into(),
            Self::Send => "Send".into(),
            Self::Stop => "Stop".into(),
        }
    }

    /// What Enter does here, as the status line says it.
    fn verb(self) -> &'static str {
        match self {
            Self::Clip | Self::Target => "Enter targets the clip",
            Self::Remove => "Enter removes the chord",
            Self::Regenerate => "Enter reseeds the voice",
            Self::Hear | Self::Play | Self::Send | Self::Stop => "Enter does it",
            Self::Lead | Self::On | Self::View => "Enter toggles",
            Self::Member(_) | Self::Colour(_) => "Enter toggles it",
            Self::Spread(_) => "Enter doubles the degree",
            _ => "Enter hears the chord",
        }
    }

    /// A control with nothing to turn: Enter is the whole of it.
    fn is_button(self) -> bool {
        matches!(
            self,
            Self::Target
                | Self::Remove
                | Self::Regenerate
                | Self::Hear
                | Self::Play
                | Self::Send
                | Self::Stop
        )
    }
}

/// The page as the cursor walks it: rows of controls, in the order they are
/// drawn. Built from the recipe, so a row that is not on screen — the
/// Euclidean counts under any other rhythm, a chord's own colours — is not
/// in the cursor's way either.
pub(super) fn layout(recipe: &Recipe, state: &MidiLab) -> Vec<Vec<Field>> {
    let mut rows = vec![
        vec![Field::Clip, Field::Target],
        vec![
            Field::Chord,
            Field::Root,
            Field::Quality,
            Field::Beats,
            Field::Remove,
        ],
        vec![Field::Layout, Field::Inversion, Field::Octave, Field::Lead],
        vec![Field::RangeLow, Field::RangeHigh, Field::Style],
    ];
    if let Some(h) = recipe.harmony.get(state.chord)
        && let Ok(chord) = crate::theory::harmony::parse(&h.symbol)
    {
        let mut members = chord.members.clone();
        members.extend(h.voicing.added.iter().copied());
        members.sort_by_key(|m| m.degree);
        members.dedup_by_key(|m| m.degree);
        let row: Vec<Field> = members.iter().map(|m| Field::Member(m.degree)).collect();
        if !row.is_empty() {
            rows.push(row);
        }
        let colours: Vec<Field> = crate::theory::harmony::available(&chord)
            .into_iter()
            .filter(|m| m.degree >= 9)
            .map(|m| Field::Colour(m.degree))
            .collect();
        if !colours.is_empty() {
            rows.push(colours);
        }
        rows.push([1, 3, 5, 7, 9, 11, 13].map(Field::Spread).to_vec());
    }
    let mut voice = vec![Field::Voice, Field::On, Field::Rhythm];
    if matches!(state.voice, Voice::Arp | Voice::Bass | Voice::Melody) {
        voice.push(Field::Motion);
    }
    if state.voice != Voice::Chords {
        voice.push(Field::Regenerate);
    }
    rows.push(voice);
    let mut shape = vec![Field::Gate, Field::Swing, Field::Velocity];
    if state.voice != Voice::Chords {
        shape.push(Field::VoiceLow);
        shape.push(Field::VoiceHigh);
    }
    if recipe.voices[state.voice.index()].rhythm == Rhythm::Euclidean {
        shape.extend([Field::Hits, Field::Steps, Field::Rotate]);
    }
    rows.push(shape);
    rows.push(vec![Field::View, Field::Snap, Field::Length]);
    rows.push(vec![Field::Hear, Field::Play, Field::Send, Field::Stop]);
    rows
}

/// Where the cursor actually stands, whatever the indices have been left at
/// by a row that has since gone away.
pub(super) fn focused(recipe: &Recipe, state: &MidiLab) -> Option<Field> {
    let rows = layout(recipe, state);
    let row = rows.get(state.row.min(rows.len().checked_sub(1)?))?;
    row.get(state.column.min(row.len().checked_sub(1)?))
        .copied()
}

/// The chord qualities the Quality control cycles, as lead-sheet text. Every
/// one of them parses; it is a shortlist of what a hand actually reaches
/// for, not the whole grammar the field still accepts by typing.
const QUALITIES: [&str; 16] = [
    "", "m", "7", "maj7", "m7", "m7b5", "dim7", "sus2", "sus4", "6", "m6", "9", "maj9", "m9", "13",
    "aug",
];

const ROOTS: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

const SNAPS: [usize; 4] = [1, 8, 12, 24];

/// Split a lead-sheet symbol into its root pitch class and everything after
/// it: `C#m7` is `(1, "m7")`. `None` when the head is not a pitch.
fn split_symbol(symbol: &str) -> Option<(u8, &str)> {
    let mut chars = symbol.char_indices();
    let (_, letter) = chars.next()?;
    let base: i32 = match letter.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let mut at = letter.len_utf8();
    let mut accidental = 0i32;
    while let Some(byte) = symbol.as_bytes().get(at) {
        match byte {
            b'b' => accidental -= 1,
            b'#' => accidental += 1,
            _ => break,
        }
        at += 1;
    }
    Some((
        base.wrapping_add(accidental).rem_euclid(12) as u8,
        &symbol[at..],
    ))
}

/// The same chord a semitone up or down, written back as text. Flats become
/// their sharp spelling, which is the one thing a round trip cannot keep and
/// the only thing the parser does not care about.
pub(super) fn transposed(symbol: &str, by: i32) -> Option<String> {
    let (root, rest) = split_symbol(symbol)?;
    let moved = (i32::from(root) + by).rem_euclid(12) as usize;
    Some(format!("{}{rest}", ROOTS[moved]))
}

/// The next or previous quality on the shortlist, keeping the root.
pub(super) fn requalified(symbol: &str, forward: bool) -> Option<String> {
    let (root, rest) = split_symbol(symbol)?;
    let at = QUALITIES.iter().position(|q| *q == rest);
    let next = match (at, forward) {
        (Some(at), true) => (at + 1) % QUALITIES.len(),
        (Some(at), false) => (at + QUALITIES.len() - 1) % QUALITIES.len(),
        // A typed symbol the shortlist does not carry is left where it is
        // and the control starts the list rather than pretending to know it.
        (None, true) => 0,
        (None, false) => QUALITIES.len() - 1,
    };
    Some(format!("{}{}", ROOTS[usize::from(root)], QUALITIES[next]))
}

/// Lay the chords end to end again and say how long the whole recipe is.
/// Every length edit goes through this, so the harmony can never come to
/// hold a gap or an overlap.
fn reflow(recipe: &mut Recipe) {
    let mut start = 0;
    for chord in &mut recipe.harmony {
        chord.start = start;
        start += chord.length;
    }
    recipe.length = recipe.length.max(start);
}

/// What one control reads right now.
pub(super) fn reading(field: Field, recipe: &Recipe, state: &MidiLab) -> String {
    let chord = recipe.harmony.get(state.chord);
    let spec = &recipe.voices[state.voice.index()];
    let on = |yes: bool| if yes { "on" } else { "off" }.to_owned();
    match field {
        Field::Clip => {
            if state.address.trim().is_empty() {
                "—".to_owned()
            } else {
                state.address.clone()
            }
        }
        Field::Chord => format!("{} of {}", state.chord + 1, recipe.harmony.len().max(1)),
        Field::Root => chord.and_then(|h| split_symbol(&h.symbol)).map_or_else(
            || "—".to_owned(),
            |(pc, _)| ROOTS[usize::from(pc)].to_owned(),
        ),
        Field::Quality => chord.and_then(|h| split_symbol(&h.symbol)).map_or_else(
            || "—".to_owned(),
            |(_, rest)| {
                if rest.is_empty() {
                    "major".to_owned()
                } else {
                    rest.to_owned()
                }
            },
        ),
        Field::Beats => chord.map_or_else(
            || "—".to_owned(),
            |h| {
                let beats = h.length as f64 / 48.;
                format!("{beats}")
            },
        ),
        Field::Layout => {
            chord.map_or_else(|| "—".to_owned(), |h| h.voicing.layout.label().to_owned())
        }
        Field::Inversion => {
            chord.map_or_else(|| "—".to_owned(), |h| h.voicing.inversion.to_string())
        }
        Field::Octave => chord.map_or_else(|| "—".to_owned(), |h| h.voicing.octave.to_string()),
        Field::Lead => chord.map_or_else(|| "—".to_owned(), |h| on(h.voicing.lead)),
        Field::RangeLow => chord.map_or_else(|| "—".to_owned(), |h| h.voicing.low.to_string()),
        Field::RangeHigh => chord.map_or_else(|| "—".to_owned(), |h| h.voicing.high.to_string()),
        Field::Style => recipe.style.label().to_owned(),
        Field::Member(degree) => chord.map_or_else(
            || "—".to_owned(),
            |h| on(!h.voicing.omitted.contains(&degree)),
        ),
        Field::Colour(degree) => chord.map_or_else(
            || "—".to_owned(),
            |h| {
                let added = h.voicing.added.iter().any(|m| m.degree == degree);
                let native = crate::theory::harmony::parse(&h.symbol)
                    .is_ok_and(|c| c.members.iter().any(|m| m.degree == degree));
                on((added || native) && !h.voicing.omitted.contains(&degree))
            },
        ),
        Field::Spread(degree) => chord.map_or_else(
            || "—".to_owned(),
            |h| {
                let octave = h
                    .voicing
                    .offsets
                    .iter()
                    .find(|(d, _)| *d == degree)
                    .map_or(0, |(_, o)| *o);
                let doubled = if h.voicing.doubled.contains(&degree) {
                    " ×2"
                } else {
                    ""
                };
                format!("{octave:+}{doubled}")
            },
        ),
        Field::Voice => state.voice.label().to_owned(),
        Field::On => on(spec.enabled),
        // The model's own name is a sentence — right for a menu, too long
        // for a readout. This says the same thing in a word.
        Field::Rhythm => match spec.rhythm {
            Rhythm::Hold => "Hold",
            Rhythm::Quarter => "1/4",
            Rhythm::Eighth => "1/8",
            Rhythm::Sixteenth => "1/16",
            Rhythm::Triplet => "Triplet",
            Rhythm::Syncopated => "Synco",
            Rhythm::Euclidean => "Euclid",
            Rhythm::Custom => "Drawn",
        }
        .to_owned(),
        Field::Motion => motion_label(state.voice, spec.motion).to_owned(),
        Field::Gate => format!("{}%", spec.gate),
        Field::Swing => spec.swing.to_string(),
        Field::Velocity => spec.velocity.to_string(),
        Field::VoiceLow => spec.low.to_string(),
        Field::VoiceHigh => spec.high.to_string(),
        Field::Hits => spec.pulses.to_string(),
        Field::Steps => spec.steps.to_string(),
        Field::Rotate => spec.rotation.to_string(),
        Field::View => if state.roll { "Notes" } else { "Rhythm" }.to_owned(),
        Field::Snap => format!("{} ticks", state.snap),
        Field::Length => format!("{} beats", recipe.length as f64 / 48.),
        Field::Target
        | Field::Remove
        | Field::Regenerate
        | Field::Hear
        | Field::Play
        | Field::Send
        | Field::Stop => String::new(),
    }
}

/// One counter, moved and kept inside its range. Refuses at the ends, so a
/// control that cannot go further says so rather than sitting still.
fn step_u8(value: &mut u8, up: bool, by: u8, min: u8, max: u8) -> Result<(), super::RefusalReason> {
    let next = if up {
        value.saturating_add(by)
    } else {
        value.saturating_sub(by)
    }
    .clamp(min, max);
    if next == *value {
        return Err(super::RefusalReason::Edge(if up {
            Step::Up
        } else {
            Step::Down
        }));
    }
    *value = next;
    Ok(())
}

/// Turn one of the controls that live in the recipe. The pointer reaches
/// these through combo boxes and steppers; this is the same edit, made by
/// the same rules, from the keys.
fn turn_recipe(
    recipe: &mut Recipe,
    field: Field,
    chord: usize,
    voice: Voice,
    up: bool,
    coarse: bool,
) -> Result<(), super::RefusalReason> {
    use super::RefusalReason as R;
    let edge = R::Edge(if up { Step::Up } else { Step::Down });
    let cycle = |at: usize, len: usize| {
        if up {
            (at + 1) % len
        } else {
            (at + len - 1) % len
        }
    };
    match field {
        Field::Style => {
            recipe.style = match recipe.style {
                HarmonicStyle::Strict => HarmonicStyle::Chromatic,
                _ => HarmonicStyle::Strict,
            };
            Ok(())
        }
        Field::Length => {
            let by = if coarse { 192 } else { 48 };
            let next = if up {
                (recipe.length + by).min(crate::sequencing::DEFAULT_PATTERN_TICKS)
            } else {
                recipe.length.saturating_sub(by)
            };
            let floor = recipe
                .harmony
                .iter()
                .map(|h| h.start + h.length)
                .max()
                .unwrap_or(48);
            let next = next.max(floor);
            if next == recipe.length {
                return Err(edge);
            }
            recipe.length = next;
            Ok(())
        }
        Field::On
        | Field::Rhythm
        | Field::Motion
        | Field::Gate
        | Field::Swing
        | Field::Velocity
        | Field::VoiceLow
        | Field::VoiceHigh
        | Field::Hits
        | Field::Steps
        | Field::Rotate => {
            let spec = &mut recipe.voices[voice.index()];
            match field {
                Field::On => {
                    if spec.enabled == up {
                        return Err(edge);
                    }
                    spec.enabled = up;
                    Ok(())
                }
                Field::Rhythm => {
                    let at = Rhythm::ALL
                        .iter()
                        .position(|r| *r == spec.rhythm)
                        .unwrap_or(0);
                    spec.rhythm = Rhythm::ALL[cycle(at, Rhythm::ALL.len())];
                    Ok(())
                }
                Field::Motion => step_u8(&mut spec.motion, up, 1, 0, 3),
                Field::Gate => step_u8(&mut spec.gate, up, if coarse { 10 } else { 1 }, 5, 100),
                Field::Swing => step_u8(&mut spec.swing, up, if coarse { 5 } else { 1 }, 50, 75),
                Field::Velocity => {
                    step_u8(&mut spec.velocity, up, if coarse { 10 } else { 1 }, 1, 127)
                }
                Field::VoiceLow => step_u8(&mut spec.low, up, if coarse { 12 } else { 1 }, 0, 127),
                Field::VoiceHigh => {
                    step_u8(&mut spec.high, up, if coarse { 12 } else { 1 }, 0, 127)
                }
                Field::Hits => step_u8(&mut spec.pulses, up, 1, 0, 32),
                Field::Steps => step_u8(&mut spec.steps, up, 1, 1, 32),
                _ => step_u8(&mut spec.rotation, up, 1, 0, 31),
            }
        }
        Field::Beats => {
            let by = if coarse { 48 } else { 24 };
            let total: usize = recipe.harmony.iter().map(|h| h.length).sum();
            let h = recipe.harmony.get_mut(chord).ok_or(R::Empty)?;
            let next = if up {
                h.length + by
            } else {
                h.length.checked_sub(by).ok_or(edge)?
            };
            if next < 12 {
                return Err(edge);
            }
            if total - h.length + next > crate::sequencing::DEFAULT_PATTERN_TICKS {
                return Err(edge);
            }
            h.length = next;
            reflow(recipe);
            Ok(())
        }
        _ => {
            let h = recipe.harmony.get_mut(chord).ok_or(R::Empty)?;
            match field {
                Field::Root | Field::Quality => {
                    let next = if field == Field::Root {
                        transposed(&h.symbol, if up { 1 } else { -1 })
                    } else {
                        requalified(&h.symbol, up)
                    }
                    .ok_or(R::Unavailable)?;
                    // A control may only write what the field itself would
                    // have accepted from the hand.
                    crate::theory::harmony::parse(&next).map_err(|_| R::Unavailable)?;
                    if next == h.symbol {
                        return Err(edge);
                    }
                    h.symbol = next;
                    Ok(())
                }
                Field::Layout => {
                    let all = crate::theory::harmony::Layout::ALL;
                    let at = all.iter().position(|l| *l == h.voicing.layout).unwrap_or(0);
                    h.voicing.layout = all[cycle(at, all.len())];
                    Ok(())
                }
                Field::Inversion => step_u8(&mut h.voicing.inversion, up, 1, 0, 9),
                Field::Octave => {
                    let next = (h.voicing.octave + if up { 1 } else { -1 }).clamp(-1, 8);
                    if next == h.voicing.octave {
                        return Err(edge);
                    }
                    h.voicing.octave = next;
                    Ok(())
                }
                Field::Lead => {
                    if h.voicing.lead == up {
                        return Err(edge);
                    }
                    h.voicing.lead = up;
                    Ok(())
                }
                Field::RangeLow => {
                    step_u8(&mut h.voicing.low, up, if coarse { 12 } else { 1 }, 0, 127)
                }
                Field::RangeHigh => {
                    step_u8(&mut h.voicing.high, up, if coarse { 12 } else { 1 }, 0, 127)
                }
                Field::Member(degree) => {
                    let omitted = h.voicing.omitted.contains(&degree);
                    if omitted != up {
                        return Err(edge);
                    }
                    if up {
                        h.voicing.omitted.retain(|d| *d != degree);
                    } else {
                        h.voicing.omitted.push(degree);
                    }
                    Ok(())
                }
                Field::Colour(degree) => {
                    let chord_symbol =
                        crate::theory::harmony::parse(&h.symbol).map_err(|_| R::Unavailable)?;
                    let native = chord_symbol
                        .members
                        .iter()
                        .find(|m| m.degree == degree)
                        .copied();
                    let member = native.or_else(|| {
                        crate::theory::harmony::available(&chord_symbol)
                            .into_iter()
                            .find(|m| m.degree == degree)
                    });
                    let Some(member) = member else {
                        return Err(R::Unavailable);
                    };
                    let on = (h.voicing.added.contains(&member) || native.is_some())
                        && !h.voicing.omitted.contains(&degree);
                    if on == up {
                        return Err(edge);
                    }
                    if up {
                        if native.is_none() {
                            h.voicing.added.push(member);
                        }
                        h.voicing.omitted.retain(|d| *d != degree);
                    } else {
                        h.voicing.added.retain(|m| *m != member);
                        if native.is_some() {
                            h.voicing.omitted.push(degree);
                        }
                    }
                    Ok(())
                }
                Field::Spread(degree) => {
                    let at = h
                        .voicing
                        .offsets
                        .iter()
                        .find(|(d, _)| *d == degree)
                        .map_or(0, |(_, o)| *o);
                    let next = (at + if up { 1 } else { -1 }).clamp(-2, 2);
                    if next == at {
                        return Err(edge);
                    }
                    h.voicing.offsets.retain(|(d, _)| *d != degree);
                    if next != 0 {
                        h.voicing.offsets.push((degree, next));
                    }
                    Ok(())
                }
                _ => Err(R::Unavailable),
            }
        }
    }
}

/// The four motions each voice has, under the names that voice uses. The
/// same table the combo box shows.
pub(super) fn motion_label(voice: Voice, motion: u8) -> &'static str {
    let modes = match voice {
        Voice::Arp => ["Up", "Down", "Pendulum", "Random"],
        Voice::Bass => ["Roots", "Walk", "Pedal C", "Variation"],
        Voice::Melody => ["Lyrical", "Angular", "Motif", "Variation"],
        Voice::Counterpoint => ["Contrary", "Contrary", "Contrary", "Contrary"],
        Voice::Chords => ["Voiced", "Voiced", "Voiced", "Voiced"],
    };
    modes[usize::from(motion).min(3)]
}

/// The progression as the text field writes it.
pub(super) fn progression_text(recipe: &Recipe) -> String {
    recipe
        .harmony
        .iter()
        .map(|h| format!("{}:{}", h.symbol, h.length as f64 / 48.))
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Debug)]
pub(super) struct MidiLab {
    pub composer: super::composer::State,
    /// The guided ladder, when this window is on it. `None` is the full
    /// inspector — the ladder is a front door, not a mode the lab is
    /// trapped in.
    pub ladder: Option<super::midi_ladder::Ladder>,
    pub draft: u64,
    pub address: String,
    pub progression: String,
    /// Where the keyboard cursor stands in [`layout`]: a row of the page
    /// and a control within it.
    pub row: usize,
    pub column: usize,
    /// The cursor moved, so the view should scroll to it once. Cleared by
    /// the frame that obeys, so a hand scrolling with the wheel is not
    /// dragged back.
    pub chase: bool,
    pub chord: usize,
    pub voice: Voice,
    pub note: Option<u64>,
    pub roll: bool,
    pub snap: usize,
    pub bpm: f64,
    pub status: String,
    pub camera: [f32; 3],
    pub job: Option<Arc<midi_lab::audition::Job>>,
    pub submitted: Option<Recipe>,
    pub played: Option<std::time::Instant>,
    pub preview_seconds: f64,
    pub clip_preview: bool,
    pub stop: bool,
}
impl PartialEq for MidiLab {
    fn eq(&self, other: &Self) -> bool {
        self.draft == other.draft
            && self.address == other.address
            // The cursor is part of what a key press is allowed to change:
            // moving it and changing nothing else is still a change. The
            // ladder's own place is a cursor for the same reason.
            && self.ladder == other.ladder
            && self.row == other.row
            && self.column == other.column
            && self.chord == other.chord
            && self.voice == other.voice
            && self.roll == other.roll
            && self.status == other.status
    }
}
impl MidiLab {
    pub fn new(draft: u64, address: String) -> Self {
        let status = format!(
            "Ready · {address} · arrows walk the page · shift and an arrow changes · H hear · S send"
        );
        Self {
            composer: super::composer::State::default(),
            // A new lab opens on the ladder: the first question it asks
            // is the one that used to be a typed tag.
            ladder: Some(super::midi_ladder::Ladder::default()),
            draft,
            address,
            progression: "Cmaj7:4 Am7:2 Dm7:1 G7:1".into(),
            row: 1,
            column: 0,
            chase: false,
            chord: 0,
            voice: Voice::Chords,
            note: None,
            roll: false,
            snap: 12,
            bpm: 120.,
            status,
            camera: [-0.6, 0.35, 3.5],
            job: None,
            submitted: None,
            played: None,
            preview_seconds: 0.,
            clip_preview: false,
            stop: false,
        }
    }
    pub fn cancel(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel();
        }
        self.submitted = None;
        self.played = None;
        self.stop = true;
    }
}
impl Stage {
    /// Deterministic states for the native screenshot harness.
    #[doc(hidden)]
    pub fn pose_midi_lab(&mut self, pose: &str) {
        // A pose on the ladder: `stage-midi-ladder-chords`, and so on.
        if pose.contains("ladder") {
            self.open_midi_lab("");
            let rung = if pose.contains("chords") {
                super::midi_ladder::Rung::Chords
            } else if pose.contains("parts") {
                super::midi_ladder::Rung::Parts
            } else if pose.contains("feel") {
                super::midi_ladder::Rung::Feel
            } else if pose.contains("listen") {
                super::midi_ladder::Rung::Listen
            } else {
                super::midi_ladder::Rung::Clip
            };
            if let Some(window) = self.midi_window_mut(0) {
                window.ladder = Some(super::midi_ladder::Ladder {
                    rung,
                    at: 0,
                    tonic: 0,
                });
            }
            return;
        }
        use super::composer::{self as controller, Subject};
        use midi_lab::composer::*;
        self.open_midi_lab("");
        let mut subject = Subject::Harmony;
        let mut note = None;
        if let Some(draft) = self.song.midi_labs.first_mut() {
            let c = draft.recipe.composition.as_mut().expect("new composer");
            if pose.contains("quartal") {
                c.harmony[0].material = crate::theory::material::Material::parse("Cmaj13").unwrap();
                c.harmony[0].voicing.layout = crate::theory::harmony::Layout::Quartal;
                subject = Subject::Voicing;
            }
            if pose.contains("material") {
                c.harmony[0].material = crate::theory::material::Material::from_mask(4095);
            }
            if [
                "notes",
                "bass",
                "motif",
                "form",
                "alternatives",
                "inspect-note",
            ]
            .iter()
            .any(|name| pose.contains(name))
            {
                c.voices[2].enabled = true;
                c.voices[3].enabled = true;
            }
            if pose.contains("notes") || pose.contains("alternatives") {
                subject = Subject::Melody;
            }
            if pose.contains("bass") {
                c.voices[3].bass.role = BassRole::Walking;
                c.voices[3].bass.style = BassStyle::Jazz;
                subject = Subject::Bass;
            }
            if pose.contains("rhythm") {
                subject = Subject::Rhythm;
            }
            if pose.contains("recipe") {
                subject = Subject::Recipe;
            }
            if pose.contains("motif") {
                let notes = render(c).unwrap().notes;
                motif::capture(c, &notes, Voice::Melody, 0, 192, "Opening statement".into())
                    .unwrap();
                subject = Subject::Motif;
            }
            if pose.contains("form") {
                c.sections = vec![
                    Section {
                        id: 10,
                        name: "A".into(),
                        start: 0,
                        length: c.length,
                        source: None,
                        transpose: 0,
                        diatonic: false,
                        enabled: [true; 5],
                        simplify_bass: false,
                    },
                    Section {
                        id: 11,
                        name: "B".into(),
                        start: 0,
                        length: c.length,
                        source: Some(10),
                        transpose: 2,
                        diatonic: false,
                        enabled: [true; 5],
                        simplify_bass: false,
                    },
                ];
                c.form = vec![10, 11, 10];
                subject = Subject::Form;
            }
            if pose.contains("counterpoint") {
                c.voices[2].enabled = true;
                c.voices[4].enabled = true;
                c.voices[4].counter.species = Species::Canon;
                subject = Subject::Counterpoint;
            }
            if pose.contains("inspect-note") {
                note = render(c)
                    .unwrap()
                    .notes
                    .iter()
                    .find(|n| n.voice == Voice::Melody)
                    .map(|n| n.id);
                subject = Subject::Note;
            }
        }
        if let Some(w) = self.lab.focus.and_then(|id| self.lab.window_mut(id))
            && let Instrument::Midi(m) = &mut w.instrument
        {
            m.composer.subject = subject;
            m.composer.note = note;
            m.composer.voice = match subject {
                Subject::Bass => Voice::Bass,
                Subject::Melody | Subject::Motif | Subject::Note => Voice::Melody,
                Subject::Counterpoint => Voice::Counterpoint,
                _ => Voice::Chords,
            };
        }
        if pose.contains("alternatives") {
            let index = 0;
            let window = self.lab.focus.unwrap();
            let Instrument::Midi(m) = &mut self.lab.window_mut(window).unwrap().instrument else {
                return;
            };
            let mut state = m.composer.clone();
            let c = self.song.midi_labs[index]
                .recipe
                .composition
                .as_mut()
                .unwrap();
            let _ = controller::act(c, &mut state, controller::Control::Explore);
            if let Instrument::Midi(m) = &mut self.lab.window_mut(window).unwrap().instrument {
                m.composer = state;
            }
        }
    }
    pub(super) fn midi_intent(
        &mut self,
        intent: super::StageIntent,
    ) -> Result<(), super::RefusalReason> {
        let window = self.lab.focus.ok_or(super::RefusalReason::Empty)?;
        let (draft, address) = match &self
            .lab
            .window(window)
            .ok_or(super::RefusalReason::Empty)?
            .instrument
        {
            Instrument::Midi(m) => (m.draft, m.address.clone()),
            _ => return Err(super::RefusalReason::Unavailable),
        };
        if intent != super::StageIntent::MidiLabStop {
            match self.resolve_midi_tag(&address) {
                Ok(destination) => {
                    if let Some(d) = self.song.midi_labs.iter_mut().find(|d| d.id == draft) {
                        d.destination = Some(destination);
                    }
                }
                Err(e) => {
                    if let Some(w) = self.lab.window_mut(window)
                        && let Instrument::Midi(m) = &mut w.instrument
                    {
                        m.status = e;
                    }
                    return Err(super::RefusalReason::Unavailable);
                }
            }
        }
        let result = match intent {
            super::StageIntent::MidiLabHear => self.midi_hear(window, true).map(|_| None),
            super::StageIntent::MidiLabPlay => self.midi_hear(window, false).map(|_| None),
            super::StageIntent::MidiLabSend => self.midi_send(draft).map(Some),
            _ => {
                if let Some(w) = self.lab.window_mut(window)
                    && let Instrument::Midi(m) = &mut w.instrument
                {
                    m.cancel();
                    m.status = "Stopped".into();
                }
                Ok(None)
            }
        };
        if let Some(w) = self.lab.window_mut(window)
            && let Instrument::Midi(m) = &mut w.instrument
        {
            match result {
                Ok(Some(s)) => m.status = s,
                Err(e) => {
                    m.status = e;
                    return Err(super::RefusalReason::Unavailable);
                }
                _ => {}
            }
        }
        Ok(())
    }
    /// The focused MIDI Lab window and the draft it edits.
    pub(super) fn midi_focus(&self) -> Option<(usize, u64)> {
        let window = self.lab.focus?;
        match &self.lab.window(window)?.instrument {
            Instrument::Midi(m) => Some((window, m.draft)),
            _ => None,
        }
    }

    pub(super) fn midi_window(&self, window: usize) -> Option<&MidiLab> {
        match &self.lab.window(window)?.instrument {
            Instrument::Midi(m) => Some(m),
            _ => None,
        }
    }

    pub(super) fn midi_window_mut(&mut self, window: usize) -> Option<&mut MidiLab> {
        match &mut self.lab.window_mut(window)?.instrument {
            Instrument::Midi(m) => Some(m),
            _ => None,
        }
    }

    pub(super) fn midi_recipe(&self, draft: u64) -> Option<&Recipe> {
        self.song
            .midi_labs
            .iter()
            .find(|d| d.id == draft)
            .map(|d| &d.recipe)
    }

    /// Say where the cursor is and what it can do there. Every keyboard
    /// verb ends here, so the status line is never stale.
    fn midi_say(&mut self, window: usize) {
        let said = {
            let Some(state) = self.midi_window(window) else {
                return;
            };
            let Some(recipe) = self.midi_recipe(state.draft) else {
                return;
            };
            let Some(field) = focused(recipe, state) else {
                return;
            };
            let value = reading(field, recipe, state);
            let turn = if field.is_button() {
                ""
            } else {
                " · shift ← → change · shift ↑ ↓ by more"
            };
            format!("{} {value}{turn} · {}", field.name(), field.verb())
        };
        if let Some(state) = self.midi_window_mut(window) {
            state.status = said;
            state.chase = true;
        }
    }

    /// The arrows: walk the page. Left and Right run along a row and carry
    /// on into the next one, the way reading does; Up and Down change row
    /// and keep as much of the column as the new row has.
    ///
    /// Nothing here edits the recipe. A page this wide needs its arrows for
    /// getting about, which is what the pointer had and the keyboard did
    /// not; shift turns whatever the cursor has reached.
    pub(super) fn midi_move(&mut self, step: Step) -> Result<(), super::RefusalReason> {
        if self.has_composer() {
            return self.composer_key(Some(step), false, false);
        }
        use super::RefusalReason as R;
        let (window, draft) = self.midi_focus().ok_or(R::Empty)?;
        let rows = {
            let state = self.midi_window(window).ok_or(R::Empty)?;
            let recipe = self.midi_recipe(draft).ok_or(R::Empty)?;
            layout(recipe, state)
        };
        if rows.is_empty() {
            return Err(R::Empty);
        }
        let (row, column) = {
            let state = self.midi_window(window).ok_or(R::Empty)?;
            (
                state.row.min(rows.len() - 1),
                state
                    .column
                    .min(rows[state.row.min(rows.len() - 1)].len().saturating_sub(1)),
            )
        };
        let (next_row, next_column) = match step {
            Step::Up => (row.checked_sub(1).ok_or(R::Edge(step))?, column),
            Step::Down => {
                if row + 1 >= rows.len() {
                    return Err(R::Edge(step));
                }
                (row + 1, column)
            }
            Step::Left => {
                if column > 0 {
                    (row, column - 1)
                } else {
                    // Off the front of a row is the end of the one above:
                    // one long line of controls, folded.
                    let above = row.checked_sub(1).ok_or(R::Edge(step))?;
                    (above, rows[above].len().saturating_sub(1))
                }
            }
            Step::Right => {
                if column + 1 < rows[row].len() {
                    (row, column + 1)
                } else {
                    if row + 1 >= rows.len() {
                        return Err(R::Edge(step));
                    }
                    (row + 1, 0)
                }
            }
        };
        let column = next_column.min(rows[next_row].len().saturating_sub(1));
        if let Some(state) = self.midi_window_mut(window) {
            state.row = next_row;
            state.column = column;
        }
        self.midi_say(window);
        Ok(())
    }

    /// Tab: the next row of the page, which is how a hand crosses it in
    /// one key rather than five.
    pub(super) fn midi_group(&mut self) -> Result<(), super::RefusalReason> {
        if self.has_composer() {
            return self.composer_key(Some(Step::Down), false, false);
        }
        use super::RefusalReason as R;
        let (window, draft) = self.midi_focus().ok_or(R::Empty)?;
        let rows = {
            let state = self.midi_window(window).ok_or(R::Empty)?;
            let recipe = self.midi_recipe(draft).ok_or(R::Empty)?;
            layout(recipe, state)
        };
        if rows.is_empty() {
            return Err(R::Empty);
        }
        if let Some(state) = self.midi_window_mut(window) {
            state.row = (state.row + 1) % rows.len();
            state.column = 0;
        }
        self.midi_say(window);
        Ok(())
    }

    /// Enter: do the obvious thing where the cursor stands.
    pub(super) fn midi_enter(&mut self) -> Result<(), super::RefusalReason> {
        if self.has_composer() {
            return self.composer_key(None, false, true);
        }
        use super::RefusalReason as R;
        let (window, draft) = self.midi_focus().ok_or(R::Empty)?;
        let field = {
            let state = self.midi_window(window).ok_or(R::Empty)?;
            let recipe = self.midi_recipe(draft).ok_or(R::Empty)?;
            focused(recipe, state).ok_or(R::Empty)?
        };
        match field {
            // Enter on either half of the destination does the same thing:
            // the tag under the cursor becomes the clip this lab sends to.
            Field::Clip | Field::Target => {
                let address = self.midi_window(window).ok_or(R::Empty)?.address.clone();
                match self.resolve_midi_tag(&address) {
                    Ok(destination) => {
                        if let Some(d) = self.song.midi_labs.iter_mut().find(|d| d.id == draft) {
                            d.destination = Some(destination);
                        }
                        let said = format!(
                            "Destination locked to {}",
                            self.song.tag_of(destination.pattern)
                        );
                        if let Some(state) = self.midi_window_mut(window) {
                            state.status = said;
                        }
                        Ok(())
                    }
                    Err(e) => {
                        if let Some(state) = self.midi_window_mut(window) {
                            state.status = e;
                        }
                        Err(R::Unavailable)
                    }
                }
            }
            Field::Remove => {
                let chord = self.midi_window(window).ok_or(R::Empty)?.chord;
                let left = self.midi_recipe(draft).map_or(0, |r| r.harmony.len());
                // The last chord is the piece; removing it would leave a
                // recipe that cannot generate a note.
                if left <= 1 {
                    return Err(R::Edge(Step::Down));
                }
                if let Some(d) = self.song.midi_labs.iter_mut().find(|d| d.id == draft) {
                    d.recipe.harmony.remove(chord);
                    reflow(&mut d.recipe);
                }
                if let Some(state) = self.midi_window_mut(window) {
                    state.chord = state.chord.saturating_sub(1);
                }
                self.midi_edited(window, draft);
                Ok(())
            }
            Field::Regenerate => {
                let voice = self.midi_window(window).ok_or(R::Empty)?.voice.index();
                if let Some(d) = self.song.midi_labs.iter_mut().find(|d| d.id == draft) {
                    d.recipe.voices[voice].seed = d.recipe.voices[voice].seed.wrapping_add(1);
                }
                self.midi_edited(window, draft);
                Ok(())
            }
            Field::Spread(degree) => {
                if let Some(d) = self.song.midi_labs.iter_mut().find(|d| d.id == draft) {
                    let chord = self.lab.window(window).and_then(|w| match &w.instrument {
                        Instrument::Midi(m) => Some(m.chord),
                        _ => None,
                    });
                    if let Some(h) = chord.and_then(|at| d.recipe.harmony.get_mut(at)) {
                        if h.voicing.doubled.contains(&degree) {
                            h.voicing.doubled.retain(|x| *x != degree);
                        } else {
                            h.voicing.doubled.push(degree);
                        }
                    }
                }
                self.midi_edited(window, draft);
                Ok(())
            }
            Field::Hear => self.midi_intent(super::StageIntent::MidiLabHear),
            Field::Play => self.midi_intent(super::StageIntent::MidiLabPlay),
            Field::Send => self.midi_intent(super::StageIntent::MidiLabSend),
            Field::Stop => self.midi_intent(super::StageIntent::MidiLabStop),
            // A switch is turned by Enter as well as by shift, because a
            // switch has nowhere to travel and Enter is the nearer key.
            Field::Lead | Field::On | Field::View | Field::Member(_) | Field::Colour(_) => {
                let now = {
                    let state = self.midi_window(window).ok_or(R::Empty)?;
                    let recipe = self.midi_recipe(draft).ok_or(R::Empty)?;
                    reading(field, recipe, state) == "on"
                };
                self.midi_turn(window, draft, !now, false)
            }
            _ => self.midi_intent(super::StageIntent::MidiLabHear),
        }
    }

    /// Shift and an arrow: change the control under the cursor. Right and
    /// Up raise it, Left and Down lower it; the vertical pair moves by more.
    pub(super) fn midi_shift(&mut self, step: Step) -> Result<(), super::RefusalReason> {
        if self.has_composer() {
            return self.composer_key(Some(step), true, false);
        }
        let (window, draft) = self.midi_focus().ok_or(super::RefusalReason::Empty)?;
        let up = matches!(step, Step::Right | Step::Up);
        let coarse = matches!(step, Step::Up | Step::Down);
        self.midi_turn(window, draft, up, coarse)
    }

    /// A draft edit: the audition of the recipe that no longer exists is
    /// retired, the progression field is rewritten, and one undo point is
    /// taken.
    fn midi_edited(&mut self, window: usize, draft: u64) {
        let progression = self.midi_recipe(draft).map(progression_text);
        if let Some(state) = self.midi_window_mut(window) {
            state.cancel();
            if let Some(text) = progression {
                state.progression = text;
            }
        }
        self.settle();
        self.midi_say(window);
    }

    /// Turn the control under the cursor. One that cannot move that way
    /// refuses rather than doing nothing quietly.
    fn midi_turn(
        &mut self,
        window: usize,
        draft: u64,
        up: bool,
        coarse: bool,
    ) -> Result<(), super::RefusalReason> {
        use super::RefusalReason as R;
        let edge = if up {
            R::Edge(Step::Up)
        } else {
            R::Edge(Step::Down)
        };
        let (field, chord, voice, address) = {
            let state = self.midi_window(window).ok_or(R::Empty)?;
            let recipe = self.midi_recipe(draft).ok_or(R::Empty)?;
            (
                focused(recipe, state).ok_or(R::Empty)?,
                state.chord,
                state.voice,
                state.address.clone(),
            )
        };
        if field.is_button() {
            return Err(R::Unavailable);
        }
        // A step for a counter, and the bigger one shift's vertical pair asks
        // for. Pitches move by octaves, percentages by tens.
        let mut edited = true;
        match field {
            Field::Clip => {
                edited = false;
                let tags: Vec<String> = self
                    .song
                    .patterns
                    .iter()
                    .filter(|p| !p.tag.is_empty())
                    .map(|p| p.tag.clone())
                    .collect();
                if tags.is_empty() {
                    return Err(R::Empty);
                }
                let at = tags
                    .iter()
                    .position(|t| t.eq_ignore_ascii_case(address.trim()));
                let next = match (at, up) {
                    (Some(at), true) if at + 1 < tags.len() => at + 1,
                    (Some(at), false) if at > 0 => at - 1,
                    (Some(_), _) => return Err(edge),
                    (None, true) => 0,
                    (None, false) => tags.len() - 1,
                };
                if let Some(state) = self.midi_window_mut(window) {
                    state.address = tags[next].clone();
                }
            }
            Field::Chord => {
                edited = false;
                let chords = self.midi_recipe(draft).map_or(0, |r| r.harmony.len());
                if chords == 0 {
                    return Err(R::Empty);
                }
                let next = if up {
                    if chord + 1 >= chords {
                        return Err(edge);
                    }
                    chord + 1
                } else {
                    chord.checked_sub(1).ok_or(edge)?
                };
                if let Some(state) = self.midi_window_mut(window) {
                    state.chord = next;
                    state.note = None;
                }
            }
            Field::Voice => {
                edited = false;
                let at = Voice::ALL.iter().position(|v| *v == voice).unwrap_or(0);
                let next = if up {
                    (at + 1) % Voice::ALL.len()
                } else {
                    (at + Voice::ALL.len() - 1) % Voice::ALL.len()
                };
                if let Some(state) = self.midi_window_mut(window) {
                    state.voice = Voice::ALL[next];
                    state.note = None;
                }
            }
            Field::View => {
                edited = false;
                let roll = self.midi_window(window).ok_or(R::Empty)?.roll;
                if roll == up {
                    return Err(edge);
                }
                if let Some(state) = self.midi_window_mut(window) {
                    state.roll = up;
                    state.note = None;
                }
            }
            Field::Snap => {
                edited = false;
                let snap = self.midi_window(window).ok_or(R::Empty)?.snap;
                let at = SNAPS.iter().position(|s| *s == snap).unwrap_or(2);
                let next = if up {
                    if at + 1 >= SNAPS.len() {
                        return Err(edge);
                    }
                    at + 1
                } else {
                    at.checked_sub(1).ok_or(edge)?
                };
                if let Some(state) = self.midi_window_mut(window) {
                    state.snap = SNAPS[next];
                }
            }
            _ => {
                let Some(d) = self.song.midi_labs.iter_mut().find(|d| d.id == draft) else {
                    return Err(R::Empty);
                };
                turn_recipe(&mut d.recipe, field, chord, voice, up, coarse)?;
            }
        }
        if edited {
            self.midi_edited(window, draft);
        } else {
            self.midi_say(window);
        }
        Ok(())
    }

    pub fn open_midi_lab(&mut self, address: &str) -> bool {
        let destination = if address.is_empty() {
            self.inside
                .map(|o| Destination {
                    track: self.song.tracks[o.track].id,
                    pattern: o.pattern,
                })
                .or_else(|| {
                    self.song
                        .patterns
                        .iter()
                        .find_map(|p| self.resolve_midi_tag(&p.tag).ok())
                })
        } else {
            self.resolve_midi_tag(address).ok()
        };
        let existing = self
            .song
            .midi_labs
            .iter()
            .find(|d| d.destination == destination)
            .map(|d| d.id);
        let id = existing.unwrap_or_else(|| {
            let id = self.song.midi_labs.iter().map(|d| d.id).max().unwrap_or(0) + 1;
            let recipe = destination
                .and_then(|d| self.song.pattern(d.pattern))
                .and_then(|p| p.midi_lab.clone())
                .unwrap_or_else(Recipe::composed);
            self.song.midi_labs.push(Draft {
                id,
                destination,
                recipe,
            });
            id
        });
        let label = destination
            .map(|d| self.song.tag_of(d.pattern))
            .unwrap_or_else(|| address.to_owned());
        self.leave_rooms();
        self.chain = None;
        self.deck.open = false;
        self.matrix.open = false;
        if let Some(window) = self
            .lab
            .windows
            .iter()
            .find(|w| matches!(&w.instrument,Instrument::Midi(m) if m.draft==id))
        {
            self.lab.focus = Some(window.id);
        } else {
            let mut state = MidiLab::new(id, label);
            if destination.is_none() {
                state.status =
                    "No destination yet · ↑↓ on the Clip cell picks one · Enter targets it".into();
            }
            if let Some(d) = self.song.midi_labs.iter().find(|d| d.id == id) {
                state.progression = d
                    .recipe
                    .harmony
                    .iter()
                    .map(|h| format!("{}:{}", h.symbol, h.length as f64 / 48.))
                    .collect::<Vec<_>>()
                    .join(" ");
            }
            self.lab.open_window(Instrument::Midi(state));
        }
        self.lab.open = true;
        self.lab.inside = true;
        self.lab.fullscreen = true;
        self.settle();
        true
    }
    pub(super) fn resolve_midi_tag(&self, tag: &str) -> Result<Destination, String> {
        let p = self
            .song
            .patterns
            .iter()
            .find(|p| p.tag.eq_ignore_ascii_case(tag.trim()) && !p.tag.is_empty())
            .ok_or_else(|| format!("Clip {tag} does not exist; choose an existing clip tag"))?;
        let t = self
            .song
            .tracks
            .iter()
            .find(|t| {
                p.tag
                    .strip_prefix(&t.letter)
                    .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
            })
            .ok_or("Destination track was removed")?;
        if t.machine.is_none() {
            return Err("The destination track needs an instrument".into());
        }
        Ok(Destination {
            track: t.id,
            pattern: p.id,
        })
    }
    pub(super) fn midi_send(&mut self, id: u64) -> Result<String, String> {
        let draft = self
            .song
            .midi_labs
            .iter()
            .find(|d| d.id == id)
            .ok_or("Draft was removed")?
            .clone();
        let dest = draft.destination.ok_or("Choose a destination clip")?;
        if let Some(composition) = &draft.recipe.composition {
            let rendered = midi_lab::composer::render(composition)?;
            let mut recipe = draft.recipe.clone();
            let c = recipe
                .composition
                .as_mut()
                .ok_or("Composition disappeared")?;
            c.snapshot = Some(midi_lab::composer::Snapshot {
                harmony: rendered.harmony.clone(),
                voicings: rendered.voicings.clone(),
                length: rendered.length,
                input: c.input()?,
                events: rendered.notes.clone(),
                label: c.name.clone(),
            });
            let mut song = self.song.clone();
            let delivered = midi_lab::composer::output::apply(&mut song, &recipe, dest, &rendered)?;
            if let Some(d) = song.midi_labs.iter_mut().find(|d| d.id == id) {
                d.recipe = recipe;
            }
            self.settle();
            self.song = song;
            self.touched();
            self.settle();
            let count = delivered.iter().map(|d| d.events.len()).sum::<usize>();
            let changes = delivered
                .iter()
                .flat_map(|d| d.changes.iter())
                .cloned()
                .collect::<Vec<_>>()
                .join(" · ");
            return Ok(format!(
                "Sent {count} notes across {} clips · {} beats{}",
                delivered.len(),
                f64::from(rendered.length) / 48.,
                if changes.is_empty() {
                    String::new()
                } else {
                    format!(" · {changes}")
                }
            ));
        }
        if !self.song.tracks.iter().any(|t| t.id == dest.track) {
            return Err("Destination track was removed".into());
        }
        let generated = midi_lab::generate(&draft.recipe)?;
        if generated.notes.is_empty() {
            return Err("There are no notes to send".into());
        }
        self.settle();
        let pattern = self
            .song
            .pattern_mut(dest.pattern)
            .ok_or("Destination clip was removed")?;
        midi_lab::write_pattern(pattern, &draft.recipe, &generated.notes);
        let tag = pattern.tag.clone();
        self.touched();
        self.settle();
        Ok(format!(
            "Sent {} notes to {tag} · Undo restores the previous clip",
            generated.notes.len()
        ))
    }
    pub(super) fn midi_hear(&mut self, window: usize, chord_only: bool) -> Result<(), String> {
        let m = match &self.lab.window(window).ok_or("Window closed")?.instrument {
            Instrument::Midi(m) => m,
            _ => return Err("Not a MIDI Lab".into()),
        };
        let draft = self
            .song
            .midi_labs
            .iter()
            .find(|d| d.id == m.draft)
            .ok_or("Draft was removed")?;
        let dest = draft.destination.ok_or("Choose a destination clip")?;
        if !self
            .song
            .tracks
            .iter()
            .any(|t| t.id == dest.track && t.machine.is_some())
        {
            return Err("The destination track needs an instrument".into());
        }
        let mut recipe = draft.recipe.clone();
        if chord_only && let Some(composition) = &mut recipe.composition {
            let mut h = composition
                .harmony
                .get(m.composer.chord)
                .ok_or("Select a chord")?
                .clone();
            h.start = 0;
            h.length = 96;
            composition.harmony = vec![h];
            composition.length = 96;
            composition.form.clear();
            composition.sections.clear();
            composition.placements.clear();
            composition.overrides.clear();
            composition.frozen = false;
            composition.modulations.clear();
            for (i, v) in composition.voices.iter_mut().enumerate() {
                v.enabled = i == 0;
                v.rhythm.kind = midi_lab::composer::RhythmKind::Hold;
            }
        } else if chord_only {
            let mut h = recipe.harmony.get(m.chord).ok_or("Select a chord")?.clone();
            h.start = 0;
            h.length = 96;
            recipe.harmony = vec![h];
            recipe.length = 96;
            recipe.pinned.clear();
            recipe.removed.clear();
            for (i, v) in recipe.voices.iter_mut().enumerate() {
                v.enabled = i == 0;
                v.rhythm = midi_lab::Rhythm::Hold;
            }
        }
        let generated = midi_lab::generate(&recipe)?;
        if generated.notes.is_empty() {
            return Err("There are no notes to hear".into());
        }
        let rate = self.vitals.stream().map_or(48_000, |s| s.sample_rate);
        let job =
            midi_lab::audition::Job::start(self.song.clone(), recipe, dest, generated.notes, rate);
        let submitted = draft.recipe.clone();
        let bpm = self.song.bpm;
        if let Some(super::lab::LabWindow {
            instrument: Instrument::Midi(m),
            ..
        }) = self.lab.window_mut(window)
        {
            m.cancel();
            m.clip_preview = !chord_only;
            m.bpm = bpm;
            m.job = Some(job);
            m.submitted = Some(submitted);
            m.status = "Rendering through destination instrument…".into();
        }
        Ok(())
    }
    /// Called by the host, after the UI has edited this frame. A stale worker
    /// can never become the next audition after a change or Undo.
    pub fn take_midi_audio(&mut self) -> Option<midi_lab::audition::Audio> {
        for window in &mut self.lab.windows {
            let Instrument::Midi(m) = &mut window.instrument else {
                continue;
            };
            let current = self
                .song
                .midi_labs
                .iter()
                .find(|d| d.id == m.draft)
                .map(|d| &d.recipe);
            if m.submitted.as_ref().is_some_and(|r| Some(r) != current) {
                m.cancel();
                continue;
            }
            let Some(result) = m.job.as_ref().and_then(|j| j.take()) else {
                continue;
            };
            m.job = None;
            match result {
                Ok(audio) => {
                    m.preview_seconds = audio.frames as f64 / f64::from(audio.rate);
                    m.status = "Playing the current MIDI through destination instrument".into();
                    m.played = if m.clip_preview {
                        Some(std::time::Instant::now())
                    } else {
                        None
                    };
                    return Some(audio);
                }
                Err(e) => m.status = format!("Audition: {e}"),
            }
        }
        None
    }
    pub fn take_midi_stop(&mut self) -> bool {
        let mut stop = std::mem::take(&mut self.lab.midi_stop);
        for window in &mut self.lab.windows {
            if let Instrument::Midi(m) = &mut window.instrument {
                stop |= std::mem::take(&mut m.stop);
            }
        }
        stop
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Everything a turn is allowed to move, so a test can ask whether a
    /// key did anything at all.
    fn snapshot(stage: &Stage) -> (Recipe, String, usize, usize, Voice, bool, usize) {
        let Instrument::Midi(m) = &stage.lab.windows[0].instrument else {
            panic!("the lab window is not a MIDI Lab");
        };
        (
            stage.song.midi_labs[0].recipe.clone(),
            m.address.clone(),
            m.row,
            m.chord,
            m.voice,
            m.roll,
            m.snap,
        )
    }

    /// What a turn is NOT allowed to leave alone: the cursor's own place is
    /// excluded, so a test can tell an edit from a move.
    fn without_cursor(
        s: (Recipe, String, usize, usize, Voice, bool, usize),
    ) -> (Recipe, String, usize, Voice, bool, usize) {
        (s.0, s.1, s.3, s.4, s.5, s.6)
    }

    fn opened() -> Stage {
        let mut stage = Stage::new();
        stage.open_midi_lab("a0");
        stage.song.midi_labs[0].recipe = Recipe::default();
        stage
    }

    fn fields(stage: &Stage) -> Vec<Field> {
        let Instrument::Midi(m) = &stage.lab.windows[0].instrument else {
            panic!()
        };
        layout(&stage.song.midi_labs[0].recipe, m)
            .into_iter()
            .flatten()
            .collect()
    }

    /// The page is one long line of controls, folded into rows: Right runs
    /// off the end of a row into the next, Left comes back, and the two ends
    /// of the page refuse.
    #[test]
    fn the_arrows_reach_every_control_on_the_page() {
        let mut stage = opened();
        if let Some(state) = stage.midi_window_mut(0) {
            state.row = 0;
            state.column = 0;
        }
        let all = fields(&stage);
        assert!(
            all.len() > 30,
            "the page should offer the whole panel, not a strip: {}",
            all.len()
        );
        assert!(matches!(
            stage.midi_move(Step::Left),
            Err(super::super::RefusalReason::Edge(Step::Left))
        ));
        // Walking right visits every control, in order, exactly once.
        let mut seen = vec![current(&stage)];
        while stage.midi_move(Step::Right).is_ok() {
            seen.push(current(&stage));
        }
        assert_eq!(seen, all, "the walk did not match the page");
        assert!(matches!(
            stage.midi_move(Step::Right),
            Err(super::super::RefusalReason::Edge(Step::Right))
        ));
        // And back again.
        let mut back = vec![current(&stage)];
        while stage.midi_move(Step::Left).is_ok() {
            back.push(current(&stage));
        }
        back.reverse();
        assert_eq!(back, all, "Left did not retrace the same page");
    }

    fn current(stage: &Stage) -> Field {
        let Instrument::Midi(m) = &stage.lab.windows[0].instrument else {
            panic!()
        };
        focused(&stage.song.midi_labs[0].recipe, m).expect("the cursor stands somewhere")
    }

    /// Up and Down change row and keep the column, and stop at the top and
    /// the bottom rather than wrapping into the wrong place.
    #[test]
    fn up_and_down_move_by_rows() {
        let mut stage = opened();
        if let Some(state) = stage.midi_window_mut(0) {
            state.row = 0;
            state.column = 0;
        }
        assert!(matches!(
            stage.midi_move(Step::Up),
            Err(super::super::RefusalReason::Edge(Step::Up))
        ));
        let mut rows = 0;
        while stage.midi_move(Step::Down).is_ok() {
            rows += 1;
        }
        assert!(rows >= 8, "the page should be several rows deep: {rows}");
        assert!(matches!(
            stage.midi_move(Step::Down),
            Err(super::super::RefusalReason::Edge(Step::Down))
        ));
    }

    /// The rule the whole surface keeps: a key either changes something or
    /// says it cannot. Asked of every control the page offers, in both
    /// directions, with and without shift.
    #[test]
    fn every_control_changes_something_or_refuses() {
        let steps = [Step::Right, Step::Left, Step::Up, Step::Down];
        let count = fields(&opened()).len();
        for at in 0..count {
            for step in steps {
                let mut stage = opened();
                // Walk to the control under test.
                if let Some(state) = stage.midi_window_mut(0) {
                    state.row = 0;
                    state.column = 0;
                }
                for _ in 0..at {
                    stage.midi_move(Step::Right).expect("the page is that wide");
                }
                let field = current(&stage);
                let before = snapshot(&stage);
                let outcome = stage.midi_shift(step);
                let after = snapshot(&stage);
                match outcome {
                    Ok(()) => assert_ne!(
                        without_cursor(before),
                        without_cursor(after),
                        "{} accepted a change and moved nothing",
                        field.name()
                    ),
                    Err(_) => assert_eq!(
                        without_cursor(before),
                        without_cursor(after),
                        "{} refused a change and moved something anyway",
                        field.name()
                    ),
                }
            }
        }
    }

    /// Root and Quality write lead-sheet text back, and only text the field
    /// itself would have accepted.
    #[test]
    fn a_chord_is_transposed_and_requalified_from_the_keyboard() {
        assert_eq!(transposed("Cmaj7", 1).as_deref(), Some("C#maj7"));
        assert_eq!(transposed("C", -1).as_deref(), Some("B"));
        assert_eq!(transposed("Bb7", 1).as_deref(), Some("B7"));
        assert_eq!(transposed("nonsense", 1), None);
        assert_eq!(requalified("Cmaj7", true).as_deref(), Some("Cm7"));
        assert_eq!(requalified("Cm7", false).as_deref(), Some("Cmaj7"));

        let mut stage = opened();
        walk_to(&mut stage, Field::Root);
        let was = stage_symbol(&stage);
        stage.midi_shift(Step::Right).expect("a root moves");
        let now = stage_symbol(&stage);
        assert_ne!(was, now, "the root did not move");
        crate::theory::harmony::parse(&now).expect("a control wrote an unparseable symbol");
        let Instrument::Midi(m) = &stage.lab.windows[0].instrument else {
            panic!()
        };
        assert!(
            m.progression.starts_with(&now),
            "the progression field kept the old chord: {}",
            m.progression
        );
    }

    /// Put the cursor on a named control, by walking as a hand would.
    fn walk_to(stage: &mut Stage, wanted: Field) {
        if let Some(state) = stage.midi_window_mut(0) {
            state.row = 0;
            state.column = 0;
        }
        for _ in 0..200 {
            if current(stage) == wanted {
                return;
            }
            if stage.midi_move(Step::Right).is_err() {
                break;
            }
        }
        panic!("{} is not on the page", wanted.name());
    }

    fn stage_symbol(stage: &Stage) -> String {
        stage.song.midi_labs[0].recipe.harmony[0].symbol.clone()
    }

    /// A length is beats, and the chords after it move up behind it: the
    /// harmony can never come to hold a gap.
    #[test]
    fn a_chord_length_reflows_the_ones_after_it() {
        let mut stage = opened();
        walk_to(&mut stage, Field::Beats);
        let before: Vec<usize> = stage.song.midi_labs[0]
            .recipe
            .harmony
            .iter()
            .map(|h| h.start)
            .collect();
        let first = stage.song.midi_labs[0].recipe.harmony[0].length;
        stage
            .midi_shift(Step::Up)
            .expect("a chord can be lengthened");
        let recipe = &stage.song.midi_labs[0].recipe;
        assert_eq!(recipe.harmony[0].length, first + 48);
        let after: Vec<usize> = recipe.harmony.iter().map(|h| h.start).collect();
        assert_eq!(after[0], before[0], "the first chord moved");
        assert!(after[1] > before[1], "the next chord did not follow");
        for pair in recipe.harmony.windows(2) {
            assert_eq!(pair[0].start + pair[0].length, pair[1].start);
        }
    }

    /// Enter does the obvious thing where the cursor stands, and the page
    /// grows and shrinks with what the recipe actually holds.
    #[test]
    fn enter_acts_and_the_page_follows_the_recipe() {
        let mut stage = opened();
        walk_to(&mut stage, Field::Target);
        if let Some(state) = stage.midi_window_mut(0) {
            state.address = "a0".into();
        }
        stage.song.midi_labs[0].destination = None;
        stage.midi_enter().expect("a0 exists");
        assert!(stage.song.midi_labs[0].destination.is_some());
        if let Some(state) = stage.midi_window_mut(0) {
            state.address = "zz9".into();
        }
        assert!(stage.midi_enter().is_err());
        let Instrument::Midi(m) = &stage.lab.windows[0].instrument else {
            panic!()
        };
        assert!(m.status.contains("zz9"), "the refusal said nothing useful");

        // The Euclidean counts are only on the page while that rhythm is
        // chosen, so the cursor is never sent to a control nobody drew.
        let mut stage = opened();
        walk_to(&mut stage, Field::Voice);
        stage.midi_shift(Step::Right).expect("another voice");
        assert!(!fields(&stage).contains(&Field::Hits));
        walk_to(&mut stage, Field::Rhythm);
        while !fields(&stage).contains(&Field::Hits) {
            stage.midi_shift(Step::Right).expect("the rhythms cycle");
        }
        assert!(fields(&stage).contains(&Field::Steps));
    }

    /// Every control reads something a status line can quote.
    #[test]
    fn the_page_reads_every_control() {
        let stage = opened();
        let Instrument::Midi(m) = &stage.lab.windows[0].instrument else {
            panic!()
        };
        let recipe = &stage.song.midi_labs[0].recipe;
        for field in layout(recipe, m).into_iter().flatten() {
            let text = reading(field, recipe, m);
            assert!(
                !text.is_empty() || field.name().len() > 2,
                "{} reads nothing",
                field.name()
            );
        }
    }

    #[test]
    fn send_is_an_exact_single_undoable_edit_to_the_captured_clip() {
        let mut stage = Stage::new();
        let initial_revision = stage.revision();
        stage.open_midi_lab("a0");
        stage.song.midi_labs[0].recipe = Recipe::default();
        assert_eq!(
            stage.revision(),
            initial_revision,
            "an unsent draft does not rebuild live audio"
        );
        let id = stage.song.midi_labs[0].id;
        let destination = stage.song.midi_labs[0].destination.unwrap();
        let original = stage.song.pattern(destination.pattern).unwrap().clone();
        let instrument = stage.song.tracks[0].machine.clone();
        // Changing the UI's addressed clip cannot redirect Send.
        let other = stage.song.fill_slot(0, 1).unwrap();
        stage.settle();
        stage.inside = Some(super::super::Opened {
            track: 0,
            pattern: other,
        });
        let untouched = stage.song.pattern(other).unwrap().clone();
        let generated = midi_lab::generate(&stage.song.midi_labs[0].recipe).unwrap();
        let mut expected = original.clone();
        midi_lab::write_pattern(
            &mut expected,
            &stage.song.midi_labs[0].recipe,
            &generated.notes,
        );
        let revision = stage.revision();
        stage.midi_send(id).unwrap();
        assert!(
            stage.revision() > revision,
            "Send must notify the live host"
        );
        assert_eq!(*stage.song.pattern(destination.pattern).unwrap(), expected);
        assert_eq!(*stage.song.pattern(other).unwrap(), untouched);
        assert_eq!(stage.song.tracks[0].machine, instrument);
        let _ = stage.apply(super::super::StageIntent::Undo);
        assert_eq!(*stage.song.pattern(destination.pattern).unwrap(), original);
    }
    #[test]
    fn stale_audition_and_removed_destinations_cannot_be_used() {
        let mut stage = Stage::new();
        stage.open_midi_lab("a0");
        let id = stage.song.midi_labs[0].id;
        let recipe = stage.song.midi_labs[0].recipe.clone();
        if let Instrument::Midi(m) = &mut stage.lab.windows[0].instrument {
            m.submitted = Some(recipe);
        }
        stage.song.midi_labs[0].recipe.voices[0].velocity = 70;
        assert!(stage.take_midi_audio().is_none());
        assert!(stage.take_midi_stop());
        stage.song.patterns.clear();
        assert!(stage.midi_send(id).is_err());
        assert!(stage.resolve_midi_tag("z999").is_err());
    }
    #[test]
    fn draft_and_sent_recipe_round_trip_in_the_song() {
        let mut stage = Stage::new();
        stage.open_midi_lab("a0");
        let id = stage.song.midi_labs[0].id;
        stage.song.midi_labs[0].recipe.harmony[0].voicing.omitted = vec![5];
        stage.midi_send(id).unwrap();
        let text = ron::ser::to_string(&stage.song).unwrap();
        let decoded: crate::sequencing::Song = ron::from_str(&text).unwrap();
        assert_eq!(decoded.midi_labs, stage.song.midi_labs);
        assert_eq!(decoded.patterns, stage.song.patterns);
    }
}
