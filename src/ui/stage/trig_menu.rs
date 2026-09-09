//! The trig menu: a callout over the sequencer, pointing at the trig
//! under the cursor and listing what that trig can do.
//!
//! A speech bubble in shape and in purpose — the trig is the one
//! speaking, and the list is what it can say — but drawn the way this
//! deck draws everything: a chamfered casing with a wedge for a tail,
//! not a lozenge with a curl. The tail is a leader, and it lands ON the
//! trig, so the eye never has to guess which cell the list belongs to.
//!
//! This file holds the two things a menu is made of that have nothing to
//! do with a painter: the rows, with the sequencer intents each one
//! speaks; and the geometry, which decides where the casing stands and
//! where the tail reaches, from the trig's cell and the window alone.
//! `mod.rs` owns the state (open, which row) and the ink.

use crate::devices::{DeviceKind, DeviceSpec, ParamLabel};
use crate::intent::sequence::Intent;
use crate::params::ParamDef;
use crate::sequencing::{DeviceId, PATTERN_STEP_TICKS, Track, Trig};
use crate::ui::sequencer::sequence::NoteView;
use crate::ui::sequencer::sequence_grid::next_probability;

// ------------------------------------------------------------------ state

/// Which page of the menu is showing. The locks are the menu's purpose
/// and its first page; the trig's own verbs sit one level in, under the
/// TRIG row, so they are there without crowding the sliders.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Page {
    Locks,
    Trig,
}

/// The menu's own state: the page, the row the cursor rests on, and
/// where the list's window starts when there are more rows than fit.
/// Everything else — the trig, its cell, the voice, the rows — is read
/// from the stage and the sequencer when needed, never copied here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TrigMenu {
    pub(super) page: Page,
    pub(super) row: usize,
    pub(super) offset: usize,
}

impl TrigMenu {
    /// Fresh, on the lock page, resting on the TRIG row.
    pub(super) fn open() -> Self {
        Self {
            page: Page::Locks,
            row: 0,
            offset: 0,
        }
    }
}

/// The most rows the casing shows at once. A voice may have more
/// parameters than that, and the list then scrolls under a fixed
/// casing rather than the casing growing past the window.
pub(super) const MAX_ROWS: usize = 12;

// ------------------------------------------------------------------ locks

/// One slider on the lock page: a voice parameter, what its knob says,
/// and what this trig holds it at, if anything.
#[derive(Clone, Copy)]
pub(super) struct LockRow {
    pub(super) def: &'static ParamDef,
    pub(super) label: &'static ParamLabel,
    /// The device the row locks: `None` for the voice, an effect's id
    /// for its rows.
    pub(super) device: Option<DeviceId>,
    /// The effect's short name, before the parameter's, on an effect's
    /// row; empty on the voice's.
    pub(super) prefix: &'static str,
    /// A structural instance qualifier where otherwise identical devices
    /// bookend the chain (the default input/output gain trims).
    pub(super) instance: Option<&'static str>,
    /// The knob's value: the voice's setting, which every unlocked trig
    /// sounds.
    pub(super) knob: f32,
    /// The lock, if this trig holds one.
    pub(super) lock: Option<f32>,
}

impl LockRow {
    /// Where the row stands: the lock if there is one, else the knob.
    /// This is what a step moves from.
    pub(super) fn standing(&self) -> f32 {
        self.lock.unwrap_or(self.knob)
    }

    /// `value` as a share of the parameter's range, 0..1.
    pub(super) fn fraction(&self, value: f32) -> f32 {
        let span = self.def.max - self.def.min;
        if span <= 0.0 {
            0.0
        } else {
            ((value - self.def.min) / span).clamp(0.0, 1.0)
        }
    }
}

/// The voice a track's trigs lock: its one machine slot.
pub(super) fn voice_of(track: &Track) -> (&'static DeviceSpec, Option<&crate::sequencing::Device>) {
    match track.machine.as_ref() {
        Some(device) => (device.kind.spec(), Some(device)),
        None => (DeviceKind::Poly.spec(), None),
    }
}

/// The slice row: on a sampler in slice mode, which cut this trig
/// plays. The SLICE parameter with the file's cuts beside it, so the
/// row can say "3 of 12" and the strip above can show the third.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SliceRow {
    /// How many cuts there are to choose from: the authored table, or
    /// the grid the SLICES knob will lay when nothing is authored.
    pub(super) count: usize,
    /// The device's own SLICE knob, from one.
    pub(super) knob: u8,
    /// The trig's lock, from one, if it holds one.
    pub(super) lock: Option<u8>,
}

impl SliceRow {
    pub(super) fn standing(&self) -> u8 {
        self.lock.unwrap_or(self.knob)
    }
}

/// One row of the lock page.
#[derive(Clone, Copy)]
pub(super) enum MenuRow {
    /// Leads to the trig's verbs.
    Trig,
    /// The slice selector, on a slicing sampler.
    Slice(SliceRow),
    /// A voice parameter's slider.
    Param(LockRow),
}

/// Whether `track`'s voice is a sampler in slice mode: the tracks whose
/// trigs choose a cut.
pub(super) fn slicing(track: &Track) -> bool {
    use crate::params::sampler as sp;
    match voice_of(track) {
        (spec, Some(device)) => {
            spec.kind == DeviceKind::Sampler && device.value(sp::MODE).round() == sp::MODE_SLICE
        }
        _ => false,
    }
}

/// The lock page's rows for `trig` on `track`: the slice row first
/// where the voice slices — the menu opens with the cut under the
/// hand, because on a slicing track that is the thing a trig is most
/// often told — then the TRIG row, then the voice's parameters in its
/// own order, all but SLICE, which the slice row already is.
pub(super) fn menu_rows(track: &Track, trig: &Trig) -> Vec<MenuRow> {
    use crate::params::sampler as sp;
    let (spec, device) = voice_of(track);
    let mut rows = Vec::new();
    let slices = slicing(track);
    if slices && let Some(device) = device {
        let count = if device.slices.is_empty() {
            device.value(sp::SLICES).round().max(1.0) as usize
        } else {
            device.slices.len()
        };
        rows.push(MenuRow::Slice(SliceRow {
            count,
            knob: device.value(sp::SLICE).round().clamp(1.0, 64.0) as u8,
            lock: trig
                .lock(sp::SLICE)
                .map(|slice| slice.round().clamp(1.0, 64.0) as u8),
        }));
    }
    rows.push(MenuRow::Trig);
    rows.extend(
        spec.params
            .iter()
            .zip(spec.labels)
            .filter(|(def, _)| !(slices && def.id == sp::SLICE))
            .map(|(def, label)| {
                MenuRow::Param(LockRow {
                    def,
                    label,
                    device: None,
                    prefix: "",
                    instance: None,
                    knob: device.map_or(def.default, |device| device.value(def.id)),
                    lock: trig.lock(def.id),
                })
            }),
    );
    // Then the fixed lane sections: a trig can bend the whole path, not
    // only the voice.
    for effect in &track.strip {
        let spec = effect.kind.spec();
        rows.extend(spec.params.iter().zip(spec.labels).map(|(def, label)| {
            MenuRow::Param(LockRow {
                def,
                label,
                device: Some(effect.id),
                prefix: spec.prefix,
                instance: effect.role.code(),
                knob: effect.value(def.id),
                lock: trig.lock_on(Some(effect.id), def.id),
            })
        }));
    }
    rows
}

// ------------------------------------------------------------------ verbs

/// Everything a trig can be told from the menu, in the order the rows
/// are read. Pairs sit together (a nudge and its opposite, and so on)
/// and the one destructive verb is last, furthest from the resting row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TrigAction {
    NudgeLeft,
    NudgeRight,
    Longer,
    Shorter,
    Louder,
    Softer,
    Higher,
    Lower,
    Condition,
    Mute,
    Delete,
}

/// How far one LOUDER or SOFTER moves the velocity. The grammar's held
/// arrows move it by one per press, which is for fine work with a key
/// held down; a row picked from a list is one deliberate step and
/// should be heard.
const VELOCITY_STEP: isize = 8;

impl TrigAction {
    pub(super) const ALL: [TrigAction; 11] = [
        TrigAction::NudgeLeft,
        TrigAction::NudgeRight,
        TrigAction::Longer,
        TrigAction::Shorter,
        TrigAction::Louder,
        TrigAction::Softer,
        TrigAction::Higher,
        TrigAction::Lower,
        TrigAction::Condition,
        TrigAction::Mute,
        TrigAction::Delete,
    ];

    /// The row's words, given the trig they would act on — MUTE reads
    /// UNMUTE over a muted trig, because a row that says what it will
    /// do is worth more than one that says what it is.
    pub(super) fn label(self, trig: &NoteView) -> &'static str {
        match self {
            TrigAction::NudgeLeft => "NUDGE EARLIER",
            TrigAction::NudgeRight => "NUDGE LATER",
            TrigAction::Longer => "LONGER",
            TrigAction::Shorter => "SHORTER",
            TrigAction::Louder => "LOUDER",
            TrigAction::Softer => "SOFTER",
            TrigAction::Higher => "UP A SEMITONE",
            TrigAction::Lower => "DOWN A SEMITONE",
            TrigAction::Condition => "NEXT CONDITION",
            TrigAction::Mute if trig.muted => "UNMUTE",
            TrigAction::Mute => "MUTE",
            TrigAction::Delete => "DELETE",
        }
    }

    /// What the row says to the pattern, addressed to `trig`. One step
    /// of the pattern's own grid for time, one semitone for pitch, one
    /// audible step for velocity — the same units the grammar uses for
    /// a bare verb, so a row here and a key there land the same edit.
    pub(super) fn intents(self, trig: &NoteView) -> Vec<Intent> {
        let tick = trig.start_ticks;
        let step = PATTERN_STEP_TICKS as isize;
        match self {
            TrigAction::NudgeLeft => vec![Intent::Nudge {
                tick,
                delta_ticks: -step,
            }],
            TrigAction::NudgeRight => vec![Intent::Nudge {
                tick,
                delta_ticks: step,
            }],
            TrigAction::Longer => vec![Intent::Resize {
                tick,
                delta_ticks: step,
            }],
            TrigAction::Shorter => vec![Intent::Resize {
                tick,
                delta_ticks: -step,
            }],
            TrigAction::Louder => vec![Intent::AdjustVelocity {
                tick,
                delta: VELOCITY_STEP,
            }],
            TrigAction::Softer => vec![Intent::AdjustVelocity {
                tick,
                delta: -VELOCITY_STEP,
            }],
            TrigAction::Higher => vec![Intent::Transpose {
                tick,
                delta_semitones: 1,
            }],
            TrigAction::Lower => vec![Intent::Transpose {
                tick,
                delta_semitones: -1,
            }],
            TrigAction::Condition if trig.cond.is_some() => vec![Intent::SetCondition {
                tick,
                cond: super::deck::condition_step(trig.cond, true),
            }],
            TrigAction::Condition if trig.probability > 0.11 => {
                vec![Intent::SetProbability {
                    tick,
                    probability: next_probability(trig.probability),
                }]
            }
            TrigAction::Condition => vec![
                Intent::SetProbability {
                    tick,
                    probability: 1.0,
                },
                Intent::SetCondition {
                    tick,
                    cond: Some((1, 2)),
                },
            ],
            TrigAction::Mute => vec![Intent::SetNoteMuted {
                tick,
                pitch: trig.pitch,
                muted: !trig.muted,
            }],
            TrigAction::Delete => vec![Intent::Clear { tick }],
        }
    }
}

// --------------------------------------------------------------- geometry

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::Pitch;

    fn trig(muted: bool) -> NoteView {
        NoteView {
            pitch: Pitch::from_midi(60),
            hz: 261.6,
            midi: 60,
            approx: false,
            start_ticks: 4 * PATTERN_STEP_TICKS,
            length_ticks: PATTERN_STEP_TICKS,
            micro_ticks: 0,
            velocity: 100,
            probability: 1.0,
            cond: None,
            enabled: true,
            muted,
            locks: 0,
            slice: None,
            sound: false,
            slides: 0,
        }
    }

    /// Every row speaks, addresses the trig it was given, and no two
    /// rows say the same thing.
    #[test]
    fn every_row_addresses_the_trig_and_says_something_distinct() {
        let note = trig(false);
        let mut seen = Vec::new();
        for action in TrigAction::ALL {
            let intents = action.intents(&note);
            assert!(!intents.is_empty(), "{action:?} says nothing");
            for intent in &intents {
                let tick = match intent {
                    Intent::Nudge { tick, .. }
                    | Intent::Resize { tick, .. }
                    | Intent::AdjustVelocity { tick, .. }
                    | Intent::Transpose { tick, .. }
                    | Intent::SetProbability { tick, .. }
                    | Intent::SetNoteMuted { tick, .. }
                    | Intent::Clear { tick } => *tick,
                    other => panic!("{action:?} spoke an intent it should not: {other:?}"),
                };
                assert_eq!(tick, note.start_ticks, "{action:?} addressed another trig");
            }
            let words = format!("{intents:?}");
            assert!(!seen.contains(&words), "{action:?} repeats another row");
            seen.push(words);
        }
    }

    #[test]
    fn mute_reads_as_what_it_will_do() {
        assert_eq!(TrigAction::Mute.label(&trig(false)), "MUTE");
        assert_eq!(TrigAction::Mute.label(&trig(true)), "UNMUTE");
        assert!(matches!(
            TrigAction::Mute.intents(&trig(true)).as_slice(),
            [Intent::SetNoteMuted { muted: false, .. }]
        ));
    }

    #[test]
    fn delete_is_the_last_row() {
        assert_eq!(TrigAction::ALL.last(), Some(&TrigAction::Delete));
    }
}
