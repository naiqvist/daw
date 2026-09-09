//! Egui-free state and operations for the eight parameter keys.

use std::collections::BTreeMap;

#[cfg(test)]
use super::ApplyOutcome;
use super::{RefusalReason, Stage, Step, Touch};
use crate::audio::modulation::{LfoTrig, MOD_RATES, ModShape};
use crate::console::SectionKind;
use crate::devices::DeviceKind;
use crate::pages::LfoField;
use crate::pages::{self, PageKey, Slot, Subject, TrackField, TrigField};
use crate::sequencing::{DeviceId, PATTERN_STEP_TICKS, PATTERN_STEPS, TrackId};
use crate::sequencing::{LFO_FADE_MAX, LFO_MULTS};
use crate::ui::sequencer::sequence;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Deck {
    pub(super) lit: Option<PageKey>,
    pub(super) sub: BTreeMap<TrackId, [usize; 8]>,
    pub(super) slot: usize,
    /// The deck's window is up over the session.
    pub(super) open: bool,
    pub(super) sample_zoom: bool,
    pub(super) sample_profile: u8,
    /// Which of the tall panel's own targets has the keys, when the
    /// panel has them at all. `None` is the ordinary state: the eight
    /// cells are the control.
    pub(super) hero_focus: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct StepKeys {
    pub(super) window: usize,
    /// The keys physically down this frame.
    pub(super) held: u16,
    /// Held keys a turn has spoken to: their release is not a tap.
    pub(super) used: u16,
    /// Steps SELECTED by a long press, standing after the key is let
    /// go, so one hand can hold a selection while the other picks a
    /// slot and turns. A second long press unselects; Escape clears.
    pub(super) latched: u16,
    /// How long each key has been down, in milliseconds, for the
    /// tap-or-hold decision at release.
    pub(super) held_ms: [u16; 16],
}

impl StepKeys {
    /// The steps a turn speaks to: the keys down and the standing
    /// selection together.
    pub(super) fn addressed(&self) -> u16 {
        self.held | self.latched
    }
}

/// How long a step key is down before its release SELECTS the step
/// rather than toggling its trig, in milliseconds. Shorter than a
/// deliberate hold, longer than any tap.
pub const STEP_HOLD_MS: u16 = 300;

/// One drawable deck slot, already formatted through the band's formatter.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotView {
    pub slot: Slot,
    pub name: &'static str,
    pub unit: &'static str,
    pub value: String,
    pub place: f32,
    pub choices: &'static [&'static str],
    pub locked: bool,
    /// The lock on an addressed step slides to the next lock.
    pub slide: bool,
    /// The slot is a section's, and the section is switched OUT: the
    /// knob and any lock on it move the document and nothing audible
    /// until a turn switches it in.
    pub out: bool,
}

impl Stage {
    pub(super) fn deck_track(&self) -> Option<usize> {
        self.inside
            .map(|opened| opened.track)
            .or_else(|| self.addressed_track())
    }

    fn deck_page(&self, track: usize, key: PageKey) -> Vec<pages::ResolvedPage> {
        self.song
            .tracks
            .get(track)
            .map_or_else(Vec::new, |track| pages::resolve(track, key))
    }

    pub(super) fn page_available(&self, key: PageKey) -> bool {
        let Some(track) = self.deck_track() else {
            return false;
        };
        self.deck_page(track, key)
            .iter()
            .any(|page| page.slots.iter().any(Option::is_some))
    }

    /// The word on `key` for the deck's track: the machine's, when it
    /// declares the key, else the standard row's.
    pub(super) fn deck_key_word(&self, key: PageKey) -> &'static str {
        self.deck_track()
            .and_then(|track| self.song.tracks.get(track))
            .map_or(key.word(), |track| pages::key_word(track, key))
    }

    pub(super) fn deck_open(&self) -> bool {
        self.deck.open
    }

    /// Where the lit key stands among its sub-pages: `(at, count)`, and
    /// the sub-page's title.
    pub(super) fn deck_position(&self) -> Option<(usize, usize, &'static str)> {
        let key = self.deck.lit?;
        let track = self.deck_track()?;
        let pages = self.deck_page(track, key);
        let id = self.song.tracks.get(track)?.id;
        let at = self
            .deck
            .sub
            .get(&id)
            .map_or(0, |memory| memory[key.index()]);
        let title = pages.get(at).map_or("", |page| page.title);
        Some((at, pages.len(), title))
    }

    /// The selected sub-page's owner and word, or the track page's own title.
    pub(super) fn deck_hero_caption(&self) -> Option<String> {
        let key = self.deck.lit?;
        let (track, page) = self.selected_page()?;
        Some(match page.subject {
            Some(Subject::Machine) => {
                let machine = self.song.tracks.get(track)?.machine.as_ref()?;
                format!(
                    "{} · {}",
                    machine.kind.spec().name.to_uppercase(),
                    self.deck_key_word(key)
                )
            }
            Some(Subject::Section(kind)) => format!("{} · {}", kind.name(), key.word()),
            None => page.title.to_owned(),
        })
    }

    /// The hero band's picture for the selected sub-page: the machine's
    /// own, computed from its knobs, with the selected cell's series
    /// lit. Plain data; the view draws it. A machine without pictures,
    /// a section page or a track page has none.
    pub(super) fn deck_hero(&self) -> Option<pages::Hero> {
        let (track, page) = self.selected_page()?;
        if page.subject != Some(Subject::Machine) {
            return None;
        }
        let machine = self.song.tracks.get(track)?.machine.as_ref()?;
        let selected = match page.slots.get(self.deck.slot).copied().flatten() {
            Some(Slot::Param {
                subject: Subject::Machine,
                id,
            }) => Some(id),
            _ => None,
        };
        match machine.kind {
            DeviceKind::Sampler => self.sampler_hero(machine, page.title, selected),
            DeviceKind::Table => {
                let mut params = crate::audio::table::TableParams::default();
                for def in crate::params::table::TABLE {
                    let value = self.deck_machine_value(track, def.id)?;
                    params.set(def.id, value);
                }
                crate::audio::table::hero(&params, page.title, selected)
            }
            DeviceKind::Ring => {
                let mut params = crate::audio::ring::RingParams::default();
                for def in crate::params::ring::TABLE {
                    let value = self.deck_machine_value(track, def.id)?;
                    params.set(def.id, value);
                }
                crate::audio::ring::hero(&params, page.title, selected)
            }
            DeviceKind::PrismVoice => {
                let mut params = crate::audio::prism_voice::PrismVoiceParams::default();
                for def in crate::params::prism_voice::TABLE {
                    let value = self.deck_machine_value(track, def.id)?;
                    params.set(def.id, value);
                }
                crate::audio::prism_voice::hero(&params, page.title, selected)
            }
            DeviceKind::Mass => {
                let mut params = crate::audio::mass::MassParams::default();
                for def in crate::params::mass::TABLE {
                    let value = self.deck_machine_value(track, def.id)?;
                    params.set(def.id, value);
                }
                crate::audio::mass::hero(&params, page.title, selected)
            }
            DeviceKind::Pluck => {
                let mut params = crate::audio::pluck::PluckParams::default();
                for def in crate::params::pluck::TABLE {
                    let value = self.deck_machine_value(track, def.id)?;
                    params.set(def.id, value);
                }
                crate::audio::pluck::hero(&params, page.title, selected)
            }
            DeviceKind::Vox => {
                let mut params = crate::audio::vox::VoxParams::default();
                for def in crate::params::vox::TABLE {
                    let value = self.deck_machine_value(track, def.id)?;
                    params.set(def.id, value);
                }
                crate::audio::vox::hero(&params, page.title, selected)
            }
            DeviceKind::Pipe => {
                let mut params = crate::audio::pipe::PipeParams::default();
                for def in crate::params::pipe::TABLE {
                    let value = self.deck_machine_value(track, def.id)?;
                    params.set(def.id, value);
                }
                crate::audio::pipe::hero(&params, page.title, selected)
            }
            DeviceKind::Glass => {
                let mut params = crate::audio::glass::GlassParams::default();
                for def in crate::params::glass::TABLE {
                    let value = self.deck_machine_value(track, def.id)?;
                    params.set(def.id, value);
                }
                crate::audio::glass::hero(&params, page.title, selected)
            }
            DeviceKind::Rom => {
                let mut params = crate::audio::rom::RomParams::default();
                for def in crate::params::rom::TABLE {
                    let value = self.deck_machine_value(track, def.id)?;
                    params.set(def.id, value);
                }
                crate::audio::rom::hero(&params, page.title, selected)
            }
            DeviceKind::Thump => {
                let mut params = crate::audio::thump::ThumpParams::default();
                for (id, value) in &machine.overrides {
                    params.set(*id, *value);
                }
                crate::audio::thump::hero(&params, page.title, selected)
            }
            DeviceKind::Clay => {
                let mut params = crate::audio::clay::ClayParams::default();
                for (id, value) in &machine.overrides {
                    params.set(*id, *value);
                }
                crate::audio::clay::hero(&params, page.title, selected)
            }
            DeviceKind::Acid => {
                let mut params = crate::audio::acid::AcidParams::default();
                for (id, value) in &machine.overrides {
                    params.set(*id, *value);
                }
                crate::audio::acid::hero(&params, page.title, selected)
            }
            _ => None,
        }
    }

    /// The hero and cell describe the same addressed note, including a held lock.
    pub(super) fn deck_machine_value(&self, track: usize, param: u32) -> Option<f32> {
        let machine = self.song.tracks.get(track)?.machine.as_ref()?;
        let lock = self
            .addressing_steps()
            .then_some(())
            .and_then(|_| self.inside)
            .and_then(|opened| {
                let step = self.addressed_step_numbers().into_iter().next()?;
                self.song
                    .pattern(opened.pattern)?
                    .trig(step)
                    .lock_on(None, param)
            });
        Some(lock.unwrap_or_else(|| machine.value(param)))
    }

    /// V: the window up on the last lit page, or TRIG when none was; V
    /// again puts it away. A room closes first, as for any page.
    pub(super) fn toggle_deck(&mut self) -> Result<(), RefusalReason> {
        if self.deck.open {
            self.deck.open = false;
            self.deck.hero_focus = None;
            return Ok(());
        }
        let key = self.deck.lit.unwrap_or(PageKey::Trig);
        self.leave_rooms();
        self.chain = None;
        self.deck.lit = Some(key);
        self.deck.open = true;
        Ok(())
    }

    /// Escape with the window up: away it goes; the page stays lit in
    /// memory for the next V.
    pub(super) fn close_deck(&mut self) {
        self.deck.open = false;
        self.deck.hero_focus = None;
    }

    /// Left and Right with the deck up: the previous or next cell, with
    /// an edge at either end.
    pub(super) fn slot_step(&mut self, step: Step) -> Result<(), RefusalReason> {
        let next = match step {
            Step::Left => self.deck.slot.checked_sub(1),
            Step::Right => (self.deck.slot + 1 < 8).then_some(self.deck.slot + 1),
            Step::Up | Step::Down => return Err(RefusalReason::Unavailable),
        };
        match next {
            Some(slot) => {
                self.deck.slot = slot;
                Ok(())
            }
            None => Err(RefusalReason::Edge(step)),
        }
    }

    pub(super) fn deck_lit(&self) -> Option<PageKey> {
        self.deck.lit
    }

    pub(super) fn deck_subpage_count(&self, key: PageKey) -> usize {
        if !self.page_available(key) {
            return 0;
        }
        self.deck_track()
            .map_or(0, |track| self.deck_page(track, key).len())
    }

    pub(super) fn deck_selected_slot(&self) -> usize {
        self.deck.slot
    }

    /// The window and the lit steps — the keys down and the standing
    /// selection — for the grid's legend and the deck's lock glyph.
    pub(super) fn step_keys(&self) -> Option<(usize, u16)> {
        self.steps.map(|steps| (steps.window, steps.addressed()))
    }

    pub(super) fn page(&mut self, key: PageKey, backwards: bool) -> Result<(), RefusalReason> {
        let Some(track) = self.deck_track() else {
            return Err(RefusalReason::Empty);
        };
        self.leave_rooms();
        self.chain = None;
        // A different page is a different panel: the keys come back to
        // the cells rather than standing on a target that has gone.
        self.deck.hero_focus = None;

        // A key with nothing under it still lights, and the window says
        // so: a press is never refused, and the row never lies about
        // which key is lit.
        let track_id = self.song.tracks[track].id;
        let count = self.deck_page(track, key).len();
        let memory = self.deck.sub.entry(track_id).or_insert([0; 8]);
        let at = &mut memory[key.index()];
        if count == 0 {
            *at = 0;
        } else {
            *at %= count;
            if self.deck.lit == Some(key) && self.deck.open {
                *at = if backwards {
                    (*at + count - 1) % count
                } else {
                    (*at + 1) % count
                };
            }
        }
        self.deck.lit = Some(key);
        self.deck.open = true;
        Ok(())
    }

    pub(super) fn select_deck_slot(&mut self, slot: u8) -> Result<(), RefusalReason> {
        if slot >= 8 {
            return Err(RefusalReason::Unavailable);
        }
        if self.deck.slot == usize::from(slot) {
            return Err(RefusalReason::Unavailable);
        }
        self.deck.slot = usize::from(slot);
        Ok(())
    }

    pub(super) fn selected_page(&self) -> Option<(usize, pages::ResolvedPage)> {
        let key = self.deck.lit?;
        let track = self.deck_track()?;
        let pages = self.deck_page(track, key);
        let track_id = self.song.tracks.get(track)?.id;
        let at = self
            .deck
            .sub
            .get(&track_id)
            .map_or(0, |memory| memory[key.index()]);
        pages
            .get(at % pages.len().max(1))
            .cloned()
            .map(|page| (track, page))
    }

    fn subject_device(&self, track: usize, subject: Subject) -> Option<DeviceId> {
        match subject {
            Subject::Machine => self
                .song
                .tracks
                .get(track)?
                .machine
                .as_ref()
                .map(|device| device.id),
            Subject::Section(kind) => self.song.section(track, kind).map(|device| device.id),
        }
    }

    fn cursor_step(&self) -> Option<usize> {
        self.inside?;
        self.focus
            .active()
            .cursor()
            .map(|step| step.min(PATTERN_STEPS - 1))
    }

    /// The steps the clip's own standing selection covers — an X sweep,
    /// Ctrl+A — when there is one. A selection made in the grammar is a
    /// selection: a turn on the deck locks all of it.
    fn standing_selection(&self) -> Option<Vec<usize>> {
        let opened = self.inside?;
        let steps = self
            .sequencer
            .standing_selection_steps(opened.pattern.0, PATTERN_STEP_TICKS)?;
        let steps: Vec<usize> = steps
            .into_iter()
            .filter(|step| *step < PATTERN_STEPS)
            .collect();
        (!steps.is_empty()).then_some(steps)
    }

    /// Whether a turn on a cell is a LOCK: steps are held or selected in
    /// the step-key mode, or the clip holds a standing selection.
    pub(super) fn addressing_steps(&self) -> bool {
        self.steps.is_some_and(|steps| steps.addressed() != 0)
            || self.standing_selection().is_some()
    }

    pub(super) fn addressed_step_numbers(&self) -> Vec<usize> {
        if let Some(steps) = self.steps
            && steps.addressed() != 0
        {
            return (0..16)
                .filter(|bit| steps.addressed() & (1 << bit) != 0)
                .map(|bit| steps.window * 16 + bit)
                .filter(|step| *step < PATTERN_STEPS)
                .collect();
        }
        if let Some(selected) = self.standing_selection() {
            return selected;
        }
        self.cursor_step().into_iter().collect()
    }

    fn trig_slot_view(&self, field: TrigField) -> SlotView {
        let trig = self.inside.and_then(|opened| {
            let step = self.addressed_step_numbers().first().copied()?;
            self.song
                .pattern(opened.pattern)
                .map(|pattern| pattern.trig(step))
        });
        let (value, place) = match field {
            TrigField::Note => {
                trig.and_then(|trig| trig.primary())
                    .map_or(("--".to_owned(), 0.0), |note| {
                        let midi = crate::pitch::nearest_midi(note.pitch.resolve(&self.song.key));
                        (format!("{midi}"), f32::from(midi) / 127.0)
                    })
            }
            TrigField::Velocity => trig
                .and_then(|trig| trig.primary())
                .map_or(("--".to_owned(), 0.0), |note| {
                    (note.velocity.to_string(), f32::from(note.velocity) / 127.0)
                }),
            TrigField::Length => {
                trig.and_then(|trig| trig.primary())
                    .map_or(("--".to_owned(), 0.0), |note| {
                        (
                            note.length_ticks.to_string(),
                            (note.length_ticks as f32 / 192.0).min(1.0),
                        )
                    })
            }
            TrigField::Micro => {
                trig.and_then(|trig| trig.primary())
                    .map_or(("--".to_owned(), 0.5), |note| {
                        (
                            format!("{:+}", note.micro_ticks),
                            (f32::from(note.micro_ticks) + 12.0) / 24.0,
                        )
                    })
            }
            TrigField::Probability => trig.map_or(("100%".to_owned(), 1.0), |trig| {
                (
                    format!("{:.0}%", trig.probability * 100.0),
                    trig.probability,
                )
            }),
            TrigField::Condition => trig
                .and_then(|trig| trig.cond)
                .map_or(("--".to_owned(), 0.0), |(a, b)| {
                    (format!("{a}:{b}"), f32::from(a) / f32::from(b.max(1)))
                }),
            TrigField::Retrig => trig
                .and_then(|trig| trig.retrig)
                .map_or(("OFF".to_owned(), 0.0), |retrig| {
                    (retrig.count.to_string(), f32::from(retrig.count) / 8.0)
                }),
            TrigField::Rate => trig
                .and_then(|trig| trig.retrig)
                .map_or(("--".to_owned(), 0.0), |retrig| {
                    (format!("1/{}", retrig.rate), f32::from(retrig.rate) / 8.0)
                }),
            TrigField::Sound => {
                let library = self.sound_library_names();
                match trig.and_then(|trig| trig.sound.as_ref()) {
                    None => ("--".to_owned(), 0.0),
                    Some(lock) => {
                        let at = library.iter().position(|record| record.name == lock.name);
                        (
                            lock.name.clone(),
                            at.map_or(1.0, |at| (at + 1) as f32 / library.len().max(1) as f32),
                        )
                    }
                }
            }
        };
        SlotView {
            slot: Slot::Trig(field),
            name: field.name(),
            unit: "",
            value,
            place: place.clamp(0.0, 1.0),
            choices: &[],
            locked: false,
            slide: false,
            out: false,
        }
    }

    fn track_slot_view(&self, track: usize, field: TrackField) -> SlotView {
        let lane = &self.song.tracks[track];
        let (value, place) = match field {
            TrackField::Volume => (mixer_reading(lane.volume), lane.volume.clamp(0.0, 1.0)),
            TrackField::Pan => (
                super::mixer::pan_label(lane.pan),
                (lane.pan.clamp(-1.0, 1.0) + 1.0) * 0.5,
            ),
            TrackField::SendA | TrackField::SendB => {
                let param = if field == TrackField::SendA {
                    crate::params::console::out::SEND_TAPE
                } else {
                    crate::params::console::out::SEND_SHADOW
                };
                let value = self
                    .song
                    .section(track, SectionKind::Out)
                    .map_or(0.0, |out| out.value(param));
                (format!("{value:.0}%"), value / 100.0)
            }
            TrackField::Bus => (
                format!("{}", char::from(b'A' + lane.bus)),
                f32::from(lane.bus) / 3.0,
            ),
        };
        SlotView {
            slot: Slot::Track(field),
            name: field.name(),
            unit: "",
            value,
            place: place.clamp(0.0, 1.0),
            choices: &[],
            locked: false,
            slide: false,
            out: false,
        }
    }

    fn param_slot_view(&self, track: usize, subject: Subject, param: u32) -> Option<SlotView> {
        let id = self.subject_device(track, subject)?;
        let device = self.song.device(id)?;
        let spec = device.kind.spec();
        let at = spec.params.iter().position(|def| def.id == param)?;
        let def = &spec.params[at];
        let label = spec.labels.get(at)?;
        let lock_device = match subject {
            Subject::Machine => None,
            Subject::Section(_) => Some(id),
        };
        // With steps held or selected, the cell shows THEIR value: the
        // lock on the first addressed step when it has one, so a turn
        // is seen to move what it moves. With none, the knob.
        let held_lock = self
            .addressing_steps()
            .then_some(())
            .and_then(|_| self.inside)
            .and_then(|opened| {
                let step = self.addressed_step_numbers().into_iter().next()?;
                self.song
                    .pattern(opened.pattern)?
                    .trig(step)
                    .lock_on(lock_device, param)
            });
        let value = held_lock.unwrap_or_else(|| device.value(param));
        let locked = self.inside.is_some_and(|opened| {
            self.addressed_step_numbers().into_iter().any(|step| {
                self.song
                    .pattern(opened.pattern)
                    .is_some_and(|pattern| pattern.trig(step).lock_on(lock_device, param).is_some())
            })
        });
        let slide = self.inside.is_some_and(|opened| {
            self.addressed_step_numbers().into_iter().any(|step| {
                self.song
                    .pattern(opened.pattern)
                    .is_some_and(|pattern| pattern.trig(step).slides_on(lock_device, param))
            })
        });
        // A section that is OUT is not in the graph: a lock on it is
        // held by the document and heard by nothing, so the cell does
        // not light it.
        let out = matches!(subject, Subject::Section(_)) && device.bypassed;
        Some(SlotView {
            slot: Slot::Param { subject, id: param },
            name: label.name,
            unit: label.unit,
            value: super::chain::format_param(def, label, value),
            place: if def.max > def.min {
                ((value - def.min) / (def.max - def.min)).clamp(0.0, 1.0)
            } else {
                0.0
            },
            choices: label.choices,
            locked: locked && !out,
            slide,
            out,
        })
    }

    /// Every destination a lane LFO on `track` may take, in the order the
    /// DEST slot walks them: the track's own, the machine's, the strip's.
    fn lfo_destinations(&self, track: usize) -> Vec<super::modulation::Target> {
        super::modulation::targets(&self.song, track)
    }

    fn lfo_slot_view(&self, track: usize, which: u8, field: LfoField) -> SlotView {
        const SHAPES: [&str; 4] = ["SIN", "TRI", "SAW", "SQR"];
        const TRIGS: [&str; 5] = ["FREE", "TRIG", "HOLD", "ONE", "HALF"];
        let lfo = &self.song.tracks[track].lfos[usize::from(which.min(1))];
        let (value, place, choices): (String, f32, &'static [&'static str]) = match field {
            LfoField::Dest => {
                let list = self.lfo_destinations(track);
                match lfo.destination.as_deref() {
                    None => ("--".to_owned(), 0.0, &[]),
                    Some(id) => match list.iter().position(|target| target.id == id) {
                        Some(at) => (
                            list[at].name.clone(),
                            (at + 1) as f32 / list.len().max(1) as f32,
                            &[],
                        ),
                        None => ("?".to_owned(), 0.0, &[]),
                    },
                }
            }
            LfoField::Shape => {
                let at = shape_index(lfo.shape);
                (SHAPES[at].to_owned(), at as f32 / 3.0, &SHAPES)
            }
            LfoField::Speed => {
                let at = usize::from(lfo.speed).min(MOD_RATES.len() - 1);
                (
                    beats_word(MOD_RATES[at]),
                    at as f32 / (MOD_RATES.len() - 1) as f32,
                    &[],
                )
            }
            LfoField::Mult => {
                let at = usize::from(lfo.mult).min(LFO_MULTS.len() - 1);
                (
                    format!("×{}", beats_word(LFO_MULTS[at])),
                    at as f32 / (LFO_MULTS.len() - 1) as f32,
                    &[],
                )
            }
            LfoField::Fade => (
                if lfo.fade == 0 {
                    "OFF".to_owned()
                } else {
                    format!("{}b", lfo.fade)
                },
                f32::from(lfo.fade) / f32::from(LFO_FADE_MAX),
                &[],
            ),
            LfoField::Depth => (
                format!("{:+.0}%", lfo.depth * 100.0),
                (lfo.depth + 1.0) * 0.5,
                &[],
            ),
            LfoField::Trig => {
                let at = LfoTrig::ALL
                    .iter()
                    .position(|trig| *trig == lfo.trig)
                    .unwrap_or(0);
                (TRIGS[at].to_owned(), at as f32 / 4.0, &TRIGS)
            }
        };
        SlotView {
            slot: Slot::Lfo { which, field },
            name: field.name(),
            unit: "",
            value,
            place: place.clamp(0.0, 1.0),
            choices,
            locked: false,
            slide: false,
            out: false,
        }
    }

    /// Turn a lane LFO slot. The LFO is a rule of the track, not a
    /// parameter with a range the engine reads, so it takes no lock: with
    /// steps held it turns the same way. A change recompiles the plan.
    fn turn_lfo(
        &mut self,
        track: usize,
        which: u8,
        field: LfoField,
        up: bool,
        coarse: bool,
    ) -> Result<(), RefusalReason> {
        let edge = || RefusalReason::Edge(if up { Step::Right } else { Step::Left });
        let destinations = if field == LfoField::Dest {
            self.lfo_destinations(track)
        } else {
            Vec::new()
        };
        let lfo = &mut self.song.tracks[track].lfos[usize::from(which.min(1))];
        let notice = match field {
            LfoField::Dest => {
                // Index 0 is nowhere; the destinations follow from 1.
                let at = lfo
                    .destination
                    .as_deref()
                    .and_then(|id| destinations.iter().position(|target| target.id == id))
                    .map_or(0, |at| at + 1);
                let next = if up {
                    if at >= destinations.len() {
                        return Err(edge());
                    }
                    at + 1
                } else {
                    if at == 0 {
                        return Err(edge());
                    }
                    at - 1
                };
                lfo.destination = (next > 0).then(|| destinations[next - 1].id.clone());
                match next {
                    0 => "LFO → nowhere".to_owned(),
                    n => format!(
                        "LFO → {} / {}",
                        destinations[n - 1].group,
                        destinations[n - 1].name
                    ),
                }
            }
            LfoField::Shape => {
                let at = shape_index(lfo.shape);
                let next = if up {
                    if at == 3 {
                        return Err(edge());
                    }
                    at + 1
                } else {
                    if at == 0 {
                        return Err(edge());
                    }
                    at - 1
                };
                lfo.shape = [
                    ModShape::Sine,
                    ModShape::Triangle,
                    ModShape::Saw,
                    ModShape::Square,
                ][next];
                format!("LFO {}", lfo.shape.label())
            }
            LfoField::Speed => {
                lfo.speed = stepped(lfo.speed, MOD_RATES.len() - 1, up).ok_or_else(edge)?;
                format!(
                    "LFO {} per cycle",
                    beats_word(MOD_RATES[usize::from(lfo.speed)])
                )
            }
            LfoField::Mult => {
                lfo.mult = stepped(lfo.mult, LFO_MULTS.len() - 1, up).ok_or_else(edge)?;
                format!("LFO ×{}", beats_word(LFO_MULTS[usize::from(lfo.mult)]))
            }
            LfoField::Fade => {
                let amount = if coarse { 4 } else { 1 };
                let next = if up {
                    lfo.fade.saturating_add(amount).min(LFO_FADE_MAX)
                } else {
                    lfo.fade.saturating_sub(amount)
                };
                if next == lfo.fade {
                    return Err(edge());
                }
                lfo.fade = next;
                format!("LFO fade {}b", lfo.fade)
            }
            LfoField::Depth => {
                let amount = if coarse { 0.10 } else { 0.01 } * if up { 1.0 } else { -1.0 };
                let next = ((lfo.depth + amount) * 100.0).round().clamp(-100.0, 100.0) / 100.0;
                if next == lfo.depth {
                    return Err(edge());
                }
                lfo.depth = next;
                format!("LFO depth {:+.0}%", lfo.depth * 100.0)
            }
            LfoField::Trig => {
                let at = LfoTrig::ALL
                    .iter()
                    .position(|trig| *trig == lfo.trig)
                    .unwrap_or(0);
                let next = stepped(at as u8, LfoTrig::ALL.len() - 1, up).ok_or_else(edge)?;
                lfo.trig = LfoTrig::ALL[usize::from(next)];
                format!("LFO {}", lfo.trig.word())
            }
        };
        self.notice = Some(notice);
        self.touched();
        if let Some(steps) = &mut self.steps {
            steps.used |= steps.held;
        }
        Ok(())
    }

    /// The neutral projection consumed by the label-strip view.
    pub fn deck_slots(&self) -> [Option<SlotView>; 8] {
        let Some((track, page)) = self.selected_page() else {
            return std::array::from_fn(|_| None);
        };
        std::array::from_fn(|at| match page.slots[at]? {
            Slot::Param { subject, id } => self.param_slot_view(track, subject, id),
            Slot::Lfo { which, field } => Some(self.lfo_slot_view(track, which, field)),
            Slot::Trig(field) => Some(self.trig_slot_view(field)),
            Slot::Track(field) => Some(self.track_slot_view(track, field)),
        })
    }

    pub(super) fn turn_param(
        &mut self,
        id: DeviceId,
        param: u32,
        up: bool,
        coarse: bool,
    ) -> Result<(), RefusalReason> {
        let Some(spec) = self.song.device(id).map(|device| device.kind.spec()) else {
            return Err(RefusalReason::Empty);
        };
        let Some((def, label)) = spec
            .params
            .iter()
            .zip(spec.labels)
            .find(|(def, _)| def.id == param)
        else {
            return Err(RefusalReason::Unavailable);
        };
        if spec.kind == DeviceKind::Scomp && def.id == crate::params::scomp::OPEN {
            return if up {
                self.apply_forge(super::ForgeIntent::Open)
            } else {
                Err(RefusalReason::Edge(Step::Down))
            };
        }
        let step = super::chain::step_of(def, label, coarse) * if up { 1.0 } else { -1.0 };
        let device = self.song.device_mut(id).ok_or(RefusalReason::Empty)?;
        let before = device.value(param);
        device.set(param, before + step);
        let after = device.value(param);
        let reading = super::chain::format_param(def, label, after);
        self.touch = Some(Touch {
            device: spec.prefix,
            name: label.name,
            value: reading.clone(),
        });
        self.notice = Some(format!("{} {reading}", label.name));
        if after == before {
            return Err(RefusalReason::Edge(if up { Step::Up } else { Step::Down }));
        }
        if spec.kind == DeviceKind::Scomp && crate::params::scomp::baked(param) {
            self.touched();
        }
        self.remixed();
        Ok(())
    }

    fn lock_param(
        &mut self,
        track: usize,
        subject: Subject,
        param: u32,
        up: bool,
        coarse: bool,
    ) -> Result<(), RefusalReason> {
        let opened = self.inside.ok_or(RefusalReason::Unavailable)?;
        let id = self
            .subject_device(track, subject)
            .ok_or(RefusalReason::Empty)?;
        let device = self.song.device(id).ok_or(RefusalReason::Empty)?;
        let spec = device.kind.spec();
        let (def, label) = spec
            .params
            .iter()
            .zip(spec.labels)
            .find(|(def, _)| def.id == param)
            .ok_or(RefusalReason::Unavailable)?;
        let knob = device.value(param);
        let target = match subject {
            Subject::Machine => None,
            Subject::Section(_) => Some(id),
        };
        let amount = super::chain::step_of(def, label, coarse) * if up { 1.0 } else { -1.0 };
        let steps = self.addressed_step_numbers();
        if steps.is_empty() {
            return Err(RefusalReason::Empty);
        }
        let pattern = self
            .song
            .pattern(opened.pattern)
            .ok_or(RefusalReason::Empty)?;
        let intents: Vec<sequence::Intent> = steps
            .into_iter()
            .map(|step| {
                let value = pattern.trig(step).lock_on(target, param).unwrap_or(knob);
                sequence::Intent::SetLock {
                    tick: step * PATTERN_STEP_TICKS,
                    device: target.map(|id| id.0),
                    param,
                    value: def.clamp(value + amount),
                }
            })
            .collect();
        self.apply_sequence(opened.pattern, &intents);
        if let Some(steps) = &mut self.steps {
            steps.used |= steps.held;
        }
        self.notice = Some(format!("{} · {} locks", label.name, intents.len()));
        Ok(())
    }

    fn turn_track(
        &mut self,
        track: usize,
        field: TrackField,
        up: bool,
        coarse: bool,
    ) -> Result<(), RefusalReason> {
        match field {
            TrackField::Volume => {
                let db = if coarse {
                    super::mixer::GAIN_STEP_DB
                } else {
                    super::mixer::GAIN_FINE_DB
                };
                let before = self.song.tracks[track].volume;
                self.song.tracks[track].volume =
                    super::mixer::step_gain(before, if up { db } else { -db });
                self.notice = Some(super::mixer::gain_label(self.song.tracks[track].volume));
                if before == self.song.tracks[track].volume {
                    return Err(RefusalReason::Edge(if up { Step::Up } else { Step::Down }));
                }
                self.remixed();
            }
            TrackField::Pan => {
                let amount = if coarse {
                    super::mixer::PAN_STEP
                } else {
                    super::mixer::PAN_STEP * 0.1
                };
                let before = self.song.tracks[track].pan;
                self.song.tracks[track].pan =
                    super::mixer::step_pan(before, if up { amount } else { -amount });
                self.notice = Some(super::mixer::pan_label(self.song.tracks[track].pan));
                if before == self.song.tracks[track].pan {
                    return Err(RefusalReason::Edge(if up {
                        Step::Right
                    } else {
                        Step::Left
                    }));
                }
                self.remixed();
            }
            TrackField::SendA | TrackField::SendB => {
                let id = self
                    .song
                    .section(track, SectionKind::Out)
                    .map(|out| out.id)
                    .ok_or(RefusalReason::Empty)?;
                let param = if field == TrackField::SendA {
                    crate::params::console::out::SEND_TAPE
                } else {
                    crate::params::console::out::SEND_SHADOW
                };
                self.turn_param(id, param, up, coarse)?;
            }
            TrackField::Bus => {
                let before = self.song.tracks[track].bus;
                let next = if up {
                    (before + 1) % crate::sequencing::BUS_COUNT as u8
                } else {
                    (before + crate::sequencing::BUS_COUNT as u8 - 1)
                        % crate::sequencing::BUS_COUNT as u8
                };
                self.song.set_bus(track, next);
                self.notice = Some(format!("BUS {}", char::from(b'A' + next)));
                self.touched();
            }
        }
        Ok(())
    }

    pub(super) fn turn_trig(
        &mut self,
        field: TrigField,
        up: bool,
        coarse: bool,
    ) -> Result<(), RefusalReason> {
        let opened = self.inside.ok_or(RefusalReason::Unavailable)?;
        let steps = self.addressed_step_numbers();
        if steps.is_empty() {
            return Err(RefusalReason::Empty);
        }
        let pattern = self
            .song
            .pattern(opened.pattern)
            .ok_or(RefusalReason::Empty)?;
        if field == TrigField::Sound {
            return self.turn_sound_lock(opened.pattern, &steps, up);
        }
        let mut intents = Vec::new();
        for step in steps {
            let tick = step * PATTERN_STEP_TICKS;
            let trig = pattern.trig(step);
            match field {
                TrigField::Sound => unreachable!("handled above"),
                TrigField::Note => intents.push(sequence::Intent::Transpose {
                    tick,
                    delta_semitones: if up {
                        if coarse { 12 } else { 1 }
                    } else if coarse {
                        -12
                    } else {
                        -1
                    },
                }),
                TrigField::Velocity => intents.push(sequence::Intent::AdjustVelocity {
                    tick,
                    delta: if up {
                        if coarse { 8 } else { 1 }
                    } else if coarse {
                        -8
                    } else {
                        -1
                    },
                }),
                TrigField::Length => intents.push(sequence::Intent::Resize {
                    tick,
                    delta_ticks: if up {
                        if coarse {
                            PATTERN_STEP_TICKS as isize
                        } else {
                            1
                        }
                    } else if coarse {
                        -(PATTERN_STEP_TICKS as isize)
                    } else {
                        -1
                    },
                }),
                TrigField::Micro => intents.push(sequence::Intent::Nudge {
                    tick,
                    delta_ticks: if up {
                        if coarse {
                            PATTERN_STEP_TICKS as isize
                        } else {
                            1
                        }
                    } else if coarse {
                        -(PATTERN_STEP_TICKS as isize)
                    } else {
                        -1
                    },
                }),
                TrigField::Probability => intents.push(sequence::Intent::SetProbability {
                    tick,
                    probability: probability_step(trig.probability, up),
                }),
                TrigField::Condition => intents.push(sequence::Intent::SetCondition {
                    tick,
                    cond: condition_step(trig.cond, up),
                }),
                TrigField::Retrig => {
                    let mut retrig = trig.retrig.unwrap_or_default();
                    retrig.count = retrig
                        .count
                        .saturating_add_signed(if up { 1 } else { -1 })
                        .clamp(1, 8);
                    intents.push(sequence::Intent::SetRetrig {
                        tick,
                        retrig: Some(retrig),
                    });
                }
                TrigField::Rate => {
                    let mut retrig = trig.retrig.unwrap_or_default();
                    retrig.rate = retrig_rate_step(retrig.rate, up);
                    intents.push(sequence::Intent::SetRetrig {
                        tick,
                        retrig: Some(retrig),
                    });
                }
            }
        }
        self.apply_sequence(opened.pattern, &intents);
        if let Some(steps) = &mut self.steps {
            steps.used |= steps.held;
        }
        Ok(())
    }

    /// The library as the SOUND slot walks it: the addressed track's lane
    /// first, then the rest, each by name. Read from the folder now; a
    /// turn is not a frame.
    fn sound_library_names(&self) -> Vec<crate::sound::SoundRecord> {
        let Some(dir) = self.sounds.as_deref() else {
            return Vec::new();
        };
        let lane = self
            .addressed_track()
            .map_or(crate::lane::Lane::Plain, |track| {
                self.song.tracks[track].lane
            });
        let mut records = crate::sound::list(dir);
        records.sort_by_key(|record| {
            (
                record.lane != lane.name(),
                record.lane.clone(),
                record.name.clone(),
            )
        });
        records
    }

    /// Turn the SOUND slot: the addressed steps take the next sound in
    /// the library, or the one before; past either end the lock is off
    /// and the steps play the track's own machine again. All held steps
    /// take the same sound, from the first step's standing lock.
    fn turn_sound_lock(
        &mut self,
        pattern: crate::sequencing::PatternId,
        steps: &[usize],
        up: bool,
    ) -> Result<(), RefusalReason> {
        let library = self.sound_library_names();
        if library.is_empty() {
            self.notice = Some("no sounds in the library".to_owned());
            return Err(RefusalReason::Unavailable);
        }
        let standing = steps
            .first()
            .and_then(|step| self.song.pattern(pattern)?.trig(*step).sound.as_ref())
            .and_then(|lock| library.iter().position(|record| record.name == lock.name));
        // Index 0 is "off"; the library follows from 1.
        let at = standing.map_or(0, |at| at + 1);
        let next = if up {
            if at >= library.len() {
                return Err(RefusalReason::Edge(Step::Right));
            }
            at + 1
        } else {
            if at == 0 {
                return Err(RefusalReason::Edge(Step::Left));
            }
            at - 1
        };
        let lock = match next {
            0 => None,
            n => {
                let record = &library[n - 1];
                match crate::sound::load(&record.path) {
                    Ok(sound) => Some(crate::sequencing::SoundLock {
                        name: record.name.clone(),
                        sound,
                    }),
                    Err(error) => {
                        self.notice = Some(format!("could not load: {error}"));
                        return Err(RefusalReason::Unavailable);
                    }
                }
            }
        };
        let mut changed = 0;
        if let Some(pattern) = self.song.pattern_mut(pattern) {
            for step in steps {
                if pattern.set_sound_lock(*step, lock.clone()) {
                    changed += 1;
                }
            }
        }
        self.notice = Some(match &lock {
            None => format!("sound lock off · {} steps", steps.len()),
            Some(lock) => format!("{} · {} steps", lock.name, steps.len()),
        });
        if changed > 0 {
            self.touched();
        }
        if let Some(steps) = &mut self.steps {
            steps.used |= steps.held;
        }
        Ok(())
    }

    /// N in the step-key mode: the selected slot's lock on every addressed
    /// step becomes a slide, or stops being one — one toggle from the
    /// first step's state, so a mixed set lands together. A step with no
    /// lock there is left alone; no lock anywhere is a refusal.
    pub(super) fn toggle_slide(&mut self) -> Result<(), RefusalReason> {
        let opened = self.inside.ok_or(RefusalReason::Unavailable)?;
        let (track, page) = self.selected_page().ok_or(RefusalReason::Empty)?;
        let Some(Slot::Param { subject, id: param }) = page.slots[self.deck.slot] else {
            self.notice = Some("slide: a parameter slot".to_owned());
            return Err(RefusalReason::Unavailable);
        };
        let device = self
            .subject_device(track, subject)
            .ok_or(RefusalReason::Empty)?;
        let target = match subject {
            Subject::Machine => None,
            Subject::Section(_) => Some(device),
        };
        let steps = self.addressed_step_numbers();
        let pattern = self
            .song
            .pattern(opened.pattern)
            .ok_or(RefusalReason::Empty)?;
        let locked: Vec<usize> = steps
            .iter()
            .copied()
            .filter(|step| pattern.trig(*step).lock_on(target, param).is_some())
            .collect();
        let Some(first) = locked.first() else {
            self.notice = Some("slide: nothing locked here".to_owned());
            return Err(RefusalReason::Empty);
        };
        let slide = !pattern.trig(*first).slides_on(target, param);
        let intents: Vec<sequence::Intent> = locked
            .iter()
            .map(|step| sequence::Intent::SetSlide {
                tick: step * PATTERN_STEP_TICKS,
                device: target.map(|id| id.0),
                param,
                slide,
            })
            .collect();
        self.apply_sequence(opened.pattern, &intents);
        self.notice = Some(format!(
            "{} · {} steps",
            if slide { "slide" } else { "slide off" },
            intents.len()
        ));
        Ok(())
    }

    /// Switch the subject's section IN when it is OUT, ahead of a turn
    /// on it. The section's name when it was switched, so the notice can
    /// say so; `None` for a machine, an always-in section, or one that
    /// was already in. Bypass decides who is in the graph at all, so the
    /// graph is rebuilt; the turn that follows settles with it as one
    /// undo step.
    fn switch_in(&mut self, track: usize, subject: Subject) -> Option<&'static str> {
        let Subject::Section(kind) = subject else {
            return None;
        };
        if kind.always_in() {
            return None;
        }
        let id = self.subject_device(track, subject)?;
        let device = self.song.device_mut(id)?;
        if !device.bypassed {
            return None;
        }
        device.bypassed = false;
        self.touched();
        Some(kind.name())
    }

    pub(super) fn turn(&mut self, up: bool, coarse: bool) -> Result<(), RefusalReason> {
        let (track, page) = self.selected_page().ok_or(RefusalReason::Empty)?;
        let slot = page.slots[self.deck.slot].ok_or(RefusalReason::Empty)?;
        match slot {
            Slot::Param { subject, id } => {
                // A turn on a section that is OUT switches it IN first,
                // so the first twist is heard: an OUT section is not in
                // the graph, and a value or a lock written to it would
                // move the document and nothing else.
                let switched = self.switch_in(track, subject);
                let turned = if self.addressing_steps() {
                    self.lock_param(track, subject, id, up, coarse)
                } else {
                    let device = self
                        .subject_device(track, subject)
                        .ok_or(RefusalReason::Empty)?;
                    self.turn_param(device, id, up, coarse)
                };
                let Some(name) = switched else {
                    return turned;
                };
                self.notice = Some(match self.notice.take() {
                    Some(rest) => format!("{name} in · {rest}"),
                    None => format!("{name} in"),
                });
                // The switch is a change even when the knob was already
                // at its edge.
                match turned {
                    Err(RefusalReason::Edge(_)) => Ok(()),
                    other => other,
                }
            }
            Slot::Trig(field) => self.turn_trig(field, up, coarse),
            Slot::Track(field) => self.turn_track(track, field, up, coarse),
            Slot::Lfo { which, field } => self.turn_lfo(track, which, field, up, coarse),
        }
    }

    pub(super) fn toggle_step_keys(&mut self) -> Result<(), RefusalReason> {
        if self.steps.is_some() {
            self.steps = None;
            self.notice = Some("STEP KEYS OFF".to_owned());
            return Ok(());
        }
        if self.inside.is_none() {
            return Err(RefusalReason::Unavailable);
        }
        let window = self.cursor_step().unwrap_or(0) / 16;
        self.steps = Some(StepKeys {
            window,
            held: 0,
            used: 0,
            latched: 0,
            held_ms: [0; 16],
        });
        self.notice = Some("STEP KEYS · tap = trig · hold = select".to_owned());
        Ok(())
    }

    /// Escape inside the mode: a standing selection is cleared first,
    /// then the deck's window goes, then the mode ends. Whether the mode
    /// is still on.
    pub(super) fn escape_step_keys(&mut self) -> bool {
        if let Some(steps) = &mut self.steps
            && steps.latched != 0
        {
            steps.latched = 0;
            self.revert_selection();
            return true;
        }
        if self.deck.open {
            self.deck.open = false;
            return true;
        }
        self.steps_checkpoint = None;
        self.steps = None;
        self.notice = Some("STEP KEYS OFF".to_owned());
        false
    }

    pub(super) fn move_step_window(&mut self, step: Step) -> Result<(), RefusalReason> {
        let steps = self.steps.as_mut().ok_or(RefusalReason::Unavailable)?;
        let before = steps.window;
        steps.window = match step {
            Step::Left => steps.window.saturating_sub(1),
            Step::Right => (steps.window + 1).min(PATTERN_STEPS / 16 - 1),
            Step::Up | Step::Down => return Err(RefusalReason::Unavailable),
        };
        (steps.window != before)
            .then_some(())
            .ok_or(RefusalReason::Edge(step))
    }

    /// A tap: a trig where there is none, and none where there is one.
    /// A sounding trig is CLEARED rather than muted, the way a trig key
    /// works: the grid should not keep a ghost the hand meant to be gone.
    fn toggle_step(&mut self, step: usize) {
        let Some(opened) = self.inside else { return };
        let sounding = self
            .song
            .pattern(opened.pattern)
            .is_some_and(|pattern| pattern.trig(step).enabled);
        if sounding {
            self.apply_sequence(
                opened.pattern,
                &[sequence::Intent::Clear {
                    tick: step * PATTERN_STEP_TICKS,
                }],
            );
            return;
        }
        self.apply_sequence(
            opened.pattern,
            &[sequence::Intent::Toggle {
                tick: step * PATTERN_STEP_TICKS,
                default_pitch: crate::pitch::Pitch::from_midi(60),
                default_length_ticks: PATTERN_STEP_TICKS,
                default_velocity: 100,
            }],
        );
        self.settle();
    }

    /// Derive presses and releases from the view's physical-key snapshot.
    /// One frame of the sixteen keys with no time passing: every
    /// release is a tap. What the tests drive; the view uses the timed
    /// form.
    pub(super) fn steps_frame(&mut self, mask: u16) {
        self.steps_frame_timed(mask, 0.0);
    }

    /// One frame of the sixteen keys. A key coming up is a TAP when it
    /// was down less than [`STEP_HOLD_MS`] and no turn spoke to it: the
    /// trig toggles. Down longer, its release SELECTS the step — or
    /// unselects a selected one — and the selection stands after the
    /// hand is gone. A key a turn used does nothing on release.
    pub(super) fn steps_frame_timed(&mut self, mask: u16, dt: f32) {
        let Some(previous) = self.steps else { return };
        let falling = previous.held & !mask;
        let rising = mask & !previous.held;
        let dt_ms = (dt.max(0.0) * 1000.0).round().min(f32::from(u16::MAX)) as u16;
        if let Some(steps) = &mut self.steps {
            steps.held = mask;
            for bit in 0..16 {
                let flag = 1u16 << bit;
                // The frame that sees the press counts from zero: its
                // elapsed time belongs to before the key went down.
                if rising & flag != 0 {
                    steps.held_ms[bit] = 0;
                } else if mask & flag != 0 {
                    steps.held_ms[bit] = steps.held_ms[bit].saturating_add(dt_ms);
                }
            }
        }
        for bit in 0..16 {
            let flag = 1u16 << bit;
            if falling & flag == 0 {
                continue;
            }
            let used = previous.used & flag != 0;
            let long = previous.held_ms[bit] >= STEP_HOLD_MS;
            let window = previous.window;
            if let Some(steps) = &mut self.steps {
                steps.used &= !flag;
            }
            if used {
                continue;
            }
            let number = window * 16 + bit + 1;
            if long {
                // A hold adds the step to the selection, or takes it out.
                self.checkpoint_selection();
                if let Some(steps) = &mut self.steps {
                    steps.latched ^= flag;
                    let count = steps.latched.count_ones();
                    self.notice = Some(if steps.latched & flag != 0 {
                        format!("step {number} selected · {count} held")
                    } else if count == 0 {
                        "selection cleared".to_owned()
                    } else {
                        format!("step {number} released · {count} held")
                    });
                }
                if self.steps.is_some_and(|steps| steps.latched == 0) {
                    self.steps_checkpoint = None;
                }
            } else if self.steps.is_some_and(|steps| steps.latched == flag) {
                // The step is the one already selected: the second tap
                // places the trig, or takes it away, and lets it go.
                self.toggle_step(window * 16 + bit);
                if let Some(steps) = &mut self.steps {
                    steps.latched = 0;
                }
                self.steps_checkpoint = None;
            } else {
                // A tap SELECTS the step, alone: its cells come up on the
                // deck to be read and shaped. Enter keeps what is done
                // to it and places the note; Escape puts it back.
                if let Some(steps) = &mut self.steps {
                    steps.latched = 0;
                }
                self.steps_checkpoint = None;
                self.checkpoint_selection();
                if let Some(steps) = &mut self.steps {
                    steps.latched = flag;
                }
                self.notice = Some(format!("step {number} · Enter keeps · tap again places"));
            }
        }
    }

    /// The pattern as it stands when a selection begins, kept once per
    /// selection so Escape can put it back.
    fn checkpoint_selection(&mut self) {
        let already = self.steps.is_some_and(|steps| steps.latched != 0);
        if already || self.steps_checkpoint.is_some() {
            return;
        }
        let Some(opened) = self.inside else { return };
        if let Some(pattern) = self.song.pattern(opened.pattern) {
            self.steps_checkpoint = Some((opened.pattern, pattern.clone()));
        }
    }

    /// Enter inside the mode: KEEP. Whatever the selected steps were
    /// given stays; a selected step with no trig yet takes one; the
    /// selection is let go. With nothing selected, the cursor's step
    /// takes or loses its trig.
    pub(super) fn confirm_steps(&mut self) -> Result<(), RefusalReason> {
        let opened = self.inside.ok_or(RefusalReason::Unavailable)?;
        let selected = self.steps.is_some_and(|steps| steps.latched != 0)
            || self.standing_selection().is_some();
        let steps = self.addressed_step_numbers();
        if steps.is_empty() {
            return Err(RefusalReason::Empty);
        }
        let mut placed = 0;
        for step in &steps {
            let sounding = self
                .song
                .pattern(opened.pattern)
                .is_some_and(|pattern| pattern.trig(*step).enabled);
            if !sounding || !selected {
                self.toggle_step(*step);
                placed += 1;
            }
        }
        if let Some(steps) = &mut self.steps {
            steps.latched = 0;
        }
        self.steps_checkpoint = None;
        self.notice = Some(if placed > 0 {
            format!("kept · {placed} placed")
        } else {
            "kept".to_owned()
        });
        Ok(())
    }

    /// Enter with the deck's window up and the step keys off: KEEP.
    /// Whatever the addressed steps were turned to stays, and that is
    /// all. Placing is the grid's, in the standard mode; the deck
    /// answers Enter only so it cannot reach the grid as ACT and paste
    /// the last chord over what was just turned. The clip's selection
    /// is left standing, since it was made in the grammar and is the
    /// grammar's to let go.
    pub(super) fn keep_steps(&mut self) -> Result<(), RefusalReason> {
        self.inside.ok_or(RefusalReason::Unavailable)?;
        if self.addressed_step_numbers().is_empty() {
            return Err(RefusalReason::Empty);
        }
        self.notice = Some("kept".to_owned());
        Ok(())
    }

    /// Escape with a selection standing: PUT BACK. The pattern returns
    /// to how it stood when the selection began, and the selection goes.
    fn revert_selection(&mut self) {
        if let Some((id, pattern)) = self.steps_checkpoint.take()
            && let Some(live) = self.song.pattern_mut(id)
            && *live != pattern
        {
            *live = pattern;
            self.touched();
            self.notice = Some("put back".to_owned());
            return;
        }
        self.notice = Some("selection cleared".to_owned());
    }

    pub(super) fn step_key_intent(&mut self, key: u8) -> Result<(), RefusalReason> {
        let steps = self.steps.as_mut().ok_or(RefusalReason::Unavailable)?;
        if key >= 16 {
            return Err(RefusalReason::Unavailable);
        }
        steps.held |= 1 << key;
        Ok(())
    }
}

fn mixer_reading(value: f32) -> String {
    super::mixer::gain_label(value)
}

fn probability_step(current: f32, forward: bool) -> f32 {
    const LADDER: [f32; 5] = [1.0, 0.75, 0.5, 0.25, 0.1];
    let at = LADDER
        .iter()
        .position(|value| (*value - current).abs() < 0.05)
        .unwrap_or(0);
    if forward {
        LADDER[(at + 1) % LADDER.len()]
    } else {
        LADDER[(at + LADDER.len() - 1) % LADDER.len()]
    }
}

pub(super) fn condition_step(current: Option<(u8, u8)>, forward: bool) -> Option<(u8, u8)> {
    const LADDER: [Option<(u8, u8)>; 11] = [
        None,
        Some((1, 2)),
        Some((2, 2)),
        Some((1, 3)),
        Some((2, 3)),
        Some((3, 3)),
        Some((1, 4)),
        Some((2, 4)),
        Some((3, 4)),
        Some((4, 4)),
        Some((1, 8)),
    ];
    let at = LADDER
        .iter()
        .position(|value| *value == current)
        .unwrap_or(0);
    if forward {
        LADDER[(at + 1) % LADDER.len()]
    } else {
        LADDER[(at + LADDER.len() - 1) % LADDER.len()]
    }
}

fn retrig_rate_step(current: u8, forward: bool) -> u8 {
    const RATES: [u8; 6] = [1, 2, 3, 4, 6, 8];
    let at = RATES.iter().position(|rate| *rate == current).unwrap_or(3);
    if forward {
        RATES[(at + 1) % RATES.len()]
    } else {
        RATES[(at + RATES.len() - 1) % RATES.len()]
    }
}

fn shape_index(shape: ModShape) -> usize {
    match shape {
        ModShape::Sine => 0,
        ModShape::Triangle => 1,
        ModShape::Saw => 2,
        ModShape::Square => 3,
    }
}

/// One step along an index in `0..=last`, or `None` at the edge.
fn stepped(at: u8, last: usize, up: bool) -> Option<u8> {
    let at = usize::from(at).min(last);
    let next = if up {
        if at >= last {
            return None;
        }
        at + 1
    } else {
        at.checked_sub(1)?
    };
    Some(next as u8)
}

/// Beats as the deck writes them: `1/4`, `1`, `16`.
fn beats_word(beats: f32) -> String {
    if beats >= 1.0 {
        format!("{}", beats as u32)
    } else {
        format!("1/{}", (1.0 / beats).round() as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::stage::{StageIntent, tests::into_clip};

    #[test]
    fn coverage_heroes_show_held_step_locks_and_turns_undo() {
        for (kind, walker) in [
            (DeviceKind::Table, crate::params::table::WALK_X),
            (DeviceKind::Ring, crate::params::ring::WALK_X),
            (DeviceKind::PrismVoice, crate::params::prism_voice::WALK_X),
            (DeviceKind::Mass, crate::params::mass::WALK_X),
            (DeviceKind::Pluck, crate::params::pluck::WALK_X),
            (DeviceKind::Vox, crate::params::vox::WALK_X),
            (DeviceKind::Pipe, crate::params::pipe::WALK_X),
            (DeviceKind::Glass, crate::params::glass::WALK_X),
        ] {
            let mut stage = Stage::new();
            stage.set_palette_open(false);
            let id = stage.song.add_device(0, kind).unwrap();
            into_clip(&mut stage);
            let (key, page, slot) = PageKey::ALL
                .into_iter()
                .find_map(|key| {
                    pages::resolve(&stage.song.tracks[0], key)
                        .iter()
                        .enumerate()
                        .find_map(|(page, p)| {
                            p.slots
                                .iter()
                                .position(|s| {
                                    *s == Some(Slot::Param {
                                        subject: Subject::Machine,
                                        id: walker,
                                    })
                                })
                                .map(|slot| (key, page, slot))
                        })
                })
                .expect("walker is on a declared page");
            stage.page(key, false).unwrap();
            let track = stage.song.tracks[0].id;
            stage.deck.sub.entry(track).or_default()[key.index()] = page;
            stage.deck.slot = slot;
            let def = &kind.spec().params[walker as usize];
            let lock = def.min + (def.max - def.min) * 0.37;
            let base_hero = stage.deck_hero().expect("walker hero");
            stage.toggle_step_keys().unwrap();
            stage.steps_frame(1);
            let pattern = stage.inside.unwrap().pattern;
            stage
                .song
                .pattern_mut(pattern)
                .unwrap()
                .trig_mut(0)
                .set_lock(walker, lock);
            assert_eq!(stage.deck_machine_value(0, walker), Some(lock));
            assert_ne!(stage.song.device(id).unwrap().value(walker), lock);
            assert_ne!(
                stage.deck_hero().unwrap(),
                base_hero,
                "{kind:?}: held lock is invisible in hero"
            );
            let before = stage.song.clone();
            stage.history = crate::history::History::new(before.clone());
            assert_eq!(
                stage.apply(StageIntent::Turn {
                    up: true,
                    coarse: false
                }),
                ApplyOutcome::Changed
            );
            assert_ne!(
                stage
                    .song
                    .pattern(pattern)
                    .unwrap()
                    .trig(0)
                    .lock_on(None, walker),
                Some(lock)
            );
            stage.apply(StageIntent::Undo);
            assert_eq!(stage.song, before, "{kind:?}: one turn must undo");
        }
    }

    #[test]
    fn hero_caption_follows_the_selected_machine_section_and_track_page() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        assert_eq!(stage.deck_hero_caption(), None);
        let track = stage.deck_track().unwrap();
        stage.song.set_lane(track, crate::lane::Lane::Drum);
        stage.song.add_device(track, DeviceKind::Drum).unwrap();
        stage.page(PageKey::Src, false).unwrap();
        assert_eq!(stage.deck_hero_caption().as_deref(), Some("DRUM · SRC"));

        stage.song.add_device(track, DeviceKind::Kit).unwrap();
        stage.page(PageKey::Fltr, false).unwrap();
        assert_eq!(stage.deck_hero_caption().as_deref(), Some("CUT · FLTR"));
        stage.page(PageKey::Lfo, false).unwrap();
        assert_eq!(stage.deck_hero_caption().as_deref(), Some("LFO A"));
        stage.page(PageKey::Lfo, false).unwrap();
        assert_eq!(stage.deck_hero_caption().as_deref(), Some("LFO B"));
        stage.page(PageKey::Trig, false).unwrap();
        assert_eq!(stage.deck_hero_caption().as_deref(), Some("TRIG"));
        stage.page(PageKey::Trig, false).unwrap();
        assert_eq!(stage.deck_hero_caption().as_deref(), Some("SOUND"));
    }

    #[test]
    fn lit_page_cycles_wraps_and_steps_back() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        stage.page(PageKey::Src, false).unwrap();
        let track = stage.deck_track().unwrap();
        let id = stage.song.tracks[track].id;
        let count = stage.deck_page(track, PageKey::Src).len();
        for _ in 0..count {
            stage.page(PageKey::Src, false).unwrap();
        }
        assert_eq!(stage.deck.sub[&id][PageKey::Src.index()], 0);
        stage.page(PageKey::Src, true).unwrap();
        assert_eq!(stage.deck.sub[&id][PageKey::Src.index()], count - 1);
    }

    #[test]
    fn a_tap_toggles_once_and_a_used_hold_does_not_toggle() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        into_clip(&mut stage);
        stage.toggle_step_keys().unwrap();
        let pattern = stage.inside.unwrap().pattern;
        // The first tap selects; nothing is placed yet.
        stage.steps_frame(1);
        stage.steps_frame(0);
        assert!(!stage.song.pattern(pattern).unwrap().trig(0).enabled);
        assert_eq!(stage.steps.unwrap().latched, 1);
        // The second tap on the selected step places the trig and lets
        // the selection go.
        stage.steps_frame(1);
        stage.steps_frame(0);
        assert!(stage.song.pattern(pattern).unwrap().trig(0).enabled);
        assert_eq!(stage.steps.unwrap().latched, 0);
        // A hold a turn used does nothing on release.
        stage.steps_frame(1);
        stage.steps.as_mut().unwrap().used = 1;
        stage.steps_frame(0);
        assert!(stage.song.pattern(pattern).unwrap().trig(0).enabled);
        // A tap on another step selects it, placing nothing; Enter keeps
        // and places, and lets the selection go.
        stage.steps_frame(0b10);
        stage.steps_frame(0);
        assert_eq!(stage.steps.unwrap().latched, 0b10);
        assert!(!stage.song.pattern(pattern).unwrap().trig(1).enabled);
        assert_eq!(stage.apply(StageIntent::Enter), ApplyOutcome::Changed);
        assert!(stage.song.pattern(pattern).unwrap().trig(1).enabled);
        assert_eq!(stage.steps.unwrap().latched, 0);
    }

    /// Select, shape, then Enter keeps and Escape puts back.
    #[test]
    fn enter_keeps_a_selections_edits_and_escape_puts_them_back() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        into_clip(&mut stage);
        stage.page(PageKey::Trig, false).unwrap();
        stage.toggle_step_keys().unwrap();
        let pattern = stage.inside.unwrap().pattern;
        // Place a trig on step 1 (select, then place), then select it.
        stage.steps_frame(1);
        stage.steps_frame(0);
        stage.steps_frame(1);
        stage.steps_frame(0);
        assert!(stage.song.pattern(pattern).unwrap().trig(0).enabled);
        stage.steps_frame(1);
        stage.steps_frame(0);
        assert_eq!(stage.steps.unwrap().latched, 1);
        let before = stage.song.pattern(pattern).unwrap().clone();
        let turn = StageIntent::Turn {
            up: true,
            coarse: false,
        };
        assert_eq!(stage.apply(turn), ApplyOutcome::Changed);
        assert_eq!(stage.apply(turn), ApplyOutcome::Changed);
        assert_ne!(*stage.song.pattern(pattern).unwrap(), before);
        // Escape: put back, selection gone, trig still there.
        assert_eq!(stage.apply(StageIntent::Escape), ApplyOutcome::Changed);
        assert_eq!(*stage.song.pattern(pattern).unwrap(), before);
        assert_eq!(stage.steps.unwrap().latched, 0);
        assert!(stage.song.pattern(pattern).unwrap().trig(0).enabled);
        // Again, and Enter this time: kept.
        stage.steps_frame(1);
        stage.steps_frame(0);
        assert_eq!(stage.apply(turn), ApplyOutcome::Changed);
        let shaped = stage.song.pattern(pattern).unwrap().clone();
        assert_ne!(shaped, before);
        assert_eq!(stage.apply(StageIntent::Enter), ApplyOutcome::Changed);
        assert_eq!(*stage.song.pattern(pattern).unwrap(), shaped);
        assert!(stage.song.pattern(pattern).unwrap().trig(0).enabled);
        assert_eq!(stage.steps.unwrap().latched, 0);
    }

    #[test]
    fn held_steps_take_a_sound_lock_from_the_sound_slot_and_undo_together() {
        let dir = std::env::temp_dir().join(format!("daw-deck-sounds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut kit = crate::sequencing::Song::default();
        kit.tracks[0].machine = None;
        kit.set_lane(0, crate::lane::Lane::Drum);
        let sound = crate::sound::Sound::capture(&kit.tracks[0]);
        crate::sound::save(&dir, "eight", &sound).expect("saves");

        let mut stage = Stage::new();
        stage.set_sound_library(dir.clone());
        stage.set_palette_open(false);
        into_clip(&mut stage);
        stage.page(PageKey::Trig, false).unwrap();
        stage.page(PageKey::Trig, false).unwrap();
        let page = stage.selected_page().unwrap().1;
        assert_eq!(page.title, "SOUND");
        assert_eq!(page.slots[0], Some(Slot::Trig(TrigField::Sound)));
        assert_eq!(stage.deck_slots()[0].as_ref().unwrap().value, "--");
        stage.toggle_step_keys().unwrap();
        stage.steps_frame(0b11);
        let before = stage.song.clone();
        stage.history = crate::history::History::new(before.clone());
        let turn = |up| StageIntent::Turn { up, coarse: false };
        assert_eq!(stage.apply(turn(true)), ApplyOutcome::Changed);
        let opened = stage.inside.unwrap();
        for step in 0..2 {
            let lock = stage
                .song
                .pattern(opened.pattern)
                .unwrap()
                .trig(step)
                .sound
                .clone();
            assert_eq!(lock.as_ref().map(|lock| lock.name.as_str()), Some("eight"));
            assert_eq!(lock.unwrap().sound, sound);
        }
        assert_eq!(stage.deck_slots()[0].as_ref().unwrap().value, "eight");
        // One sound in the library: past it is the edge.
        assert!(matches!(stage.apply(turn(true)), ApplyOutcome::Refused(_)));
        // Back below the first sound is off.
        assert_eq!(stage.apply(turn(false)), ApplyOutcome::Changed);
        assert!(
            stage
                .song
                .pattern(opened.pattern)
                .unwrap()
                .trig(0)
                .sound
                .is_none()
        );
        assert!(matches!(stage.apply(turn(false)), ApplyOutcome::Refused(_)));
        assert_eq!(stage.apply(turn(true)), ApplyOutcome::Changed);
        stage.history.observe(&stage.song);
        assert!(stage.history.undo(&mut stage.song));
        assert_eq!(stage.song, before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn n_slides_the_held_steps_lock_and_refuses_with_nothing_locked() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        into_clip(&mut stage);
        stage.song.set_lane(0, crate::lane::Lane::Drum);
        stage.page(PageKey::Fx, false).unwrap();
        stage.toggle_step_keys().unwrap();
        stage.steps_frame(0b11);
        assert!(matches!(
            stage.apply(StageIntent::Slide),
            ApplyOutcome::Refused(_)
        ));
        assert_eq!(
            stage.apply(StageIntent::Turn {
                up: true,
                coarse: false
            }),
            ApplyOutcome::Changed
        );
        assert!(!stage.deck_slots()[0].as_ref().unwrap().slide);
        assert_eq!(stage.apply(StageIntent::Slide), ApplyOutcome::Changed);
        let opened = stage.inside.unwrap();
        let page = stage.selected_page().unwrap().1;
        let Slot::Param {
            subject: Subject::Section(kind),
            id,
        } = page.slots[0].unwrap()
        else {
            panic!("section slot")
        };
        let device = stage.song.section(opened.track, kind).unwrap().id;
        for step in 0..2 {
            assert!(
                stage
                    .song
                    .pattern(opened.pattern)
                    .unwrap()
                    .trig(step)
                    .slides_on(Some(device), id)
            );
        }
        assert!(stage.deck_slots()[0].as_ref().unwrap().slide);
        assert_eq!(stage.apply(StageIntent::Slide), ApplyOutcome::Changed);
        assert!(!stage.deck_slots()[0].as_ref().unwrap().slide);
    }

    #[test]
    fn the_lfo_key_turns_a_lane_lfo_towards_a_destination() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        stage.page(PageKey::Lfo, false).unwrap();
        let page = stage.selected_page().unwrap().1;
        assert_eq!(page.title, "LFO A");
        assert_eq!(stage.deck_slots()[0].as_ref().unwrap().value, "--");
        let turn = |up, coarse| StageIntent::Turn { up, coarse };
        assert!(matches!(
            stage.apply(turn(false, false)),
            ApplyOutcome::Refused(_)
        ));
        assert_eq!(stage.apply(turn(true, false)), ApplyOutcome::Changed);
        let track = stage.deck_track().unwrap();
        assert_eq!(
            stage.song.tracks[track].lfos[0].destination.as_deref(),
            Some(crate::targets::TRACK_VOLUME_TARGET)
        );
        assert_eq!(stage.deck_slots()[0].as_ref().unwrap().value, "VOLUME");
        // Past the track's own, the machine's parameters come next.
        for _ in 0..4 {
            let _ = stage.apply(turn(true, false));
        }
        let destination = stage.song.tracks[track].lfos[0]
            .destination
            .clone()
            .unwrap();
        assert!(destination.starts_with("dev."), "{destination}");

        assert_eq!(stage.apply(StageIntent::Slot(5)), ApplyOutcome::Changed);
        assert_eq!(stage.apply(turn(true, true)), ApplyOutcome::Changed);
        assert!((stage.song.tracks[track].lfos[0].depth - 0.10).abs() < 1e-6);
        assert_eq!(stage.deck_slots()[5].as_ref().unwrap().value, "+10%");
        assert_eq!(stage.apply(StageIntent::Slot(6)), ApplyOutcome::Changed);
        assert_eq!(stage.apply(turn(true, false)), ApplyOutcome::Changed);
        assert_eq!(stage.song.tracks[track].lfos[0].trig, LfoTrig::Trig);
        assert_eq!(stage.deck_slots()[6].as_ref().unwrap().value, "TRIG");

        // LFO B is the next sub-page; the machine's own LFOs follow it.
        stage.page(PageKey::Lfo, false).unwrap();
        assert_eq!(stage.selected_page().unwrap().1.title, "LFO B");
    }

    /// A tap toggles the trig; a long press selects the step and the
    /// selection stands after release, so a turn with no key down still
    /// locks it. Escape clears the selection before it ends the mode.
    #[test]
    fn a_long_press_selects_a_step_and_the_selection_outlives_the_hand() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        into_clip(&mut stage);
        stage.song.set_lane(0, crate::lane::Lane::Drum);
        stage.page(PageKey::Fx, false).unwrap();
        stage.toggle_step_keys().unwrap();
        let pattern = stage.inside.unwrap().pattern;

        // A short press is a tap: it selects. A second tap places; a
        // third and fourth take the trig away entirely, notes and all.
        stage.steps_frame_timed(1, 0.0);
        stage.steps_frame_timed(1, f32::from(STEP_HOLD_MS) / 2000.0);
        stage.steps_frame_timed(0, 0.0);
        assert!(!stage.song.pattern(pattern).unwrap().trig(0).enabled);
        assert_eq!(stage.steps.unwrap().latched, 1);
        stage.steps_frame_timed(1, 0.0);
        stage.steps_frame_timed(0, 0.0);
        assert!(stage.song.pattern(pattern).unwrap().trig(0).enabled);
        assert_eq!(stage.steps.unwrap().latched, 0);
        for _ in 0..2 {
            stage.steps_frame_timed(1, 0.0);
            stage.steps_frame_timed(0, 0.0);
        }
        let trig = stage.song.pattern(pattern).unwrap().trig(0).clone();
        assert!(!trig.enabled && trig.notes.is_empty(), "{trig:?}");
        // The frame that sees the press does not count its own elapsed
        // time towards the hold: a tap after a long idle is a tap.
        stage.steps_frame_timed(0b1000, 5.0);
        stage.steps_frame_timed(0, 0.0);
        assert_eq!(
            stage.steps.unwrap().latched,
            0b1000,
            "a tap after a long idle"
        );
        assert!(!stage.song.pattern(pattern).unwrap().trig(3).enabled);
        stage.steps.as_mut().unwrap().latched = 0;
        stage.steps_checkpoint = None;

        // A long press selects, and does not toggle.
        stage.steps_frame_timed(0b10, 0.0);
        stage.steps_frame_timed(0b10, f32::from(STEP_HOLD_MS) / 1000.0);
        stage.steps_frame_timed(0, 0.0);
        assert!(!stage.song.pattern(pattern).unwrap().trig(1).enabled);
        assert_eq!(stage.steps.unwrap().latched, 0b10);
        assert_eq!(stage.step_keys(), Some((0, 0b10)));
        assert!(
            stage
                .notice
                .as_deref()
                .is_some_and(|n| n.contains("selected"))
        );

        // Hands free: a turn locks the selected step.
        assert_eq!(
            stage.apply(StageIntent::Turn {
                up: true,
                coarse: false
            }),
            ApplyOutcome::Changed
        );
        let page = stage.selected_page().unwrap().1;
        let Slot::Param {
            subject: Subject::Section(kind),
            id,
        } = page.slots[0].unwrap()
        else {
            panic!("section slot")
        };
        let device = stage.song.section(0, kind).unwrap().id;
        let trig = stage.song.pattern(pattern).unwrap().trig(1).clone();
        assert!(trig.lock_on(Some(device), id).is_some());
        assert!(stage.deck_slots()[0].as_ref().unwrap().locked);

        // A second long press releases the step from the selection.
        stage.steps_frame_timed(0b10, 0.0);
        stage.steps_frame_timed(0b10, f32::from(STEP_HOLD_MS) / 1000.0);
        stage.steps_frame_timed(0, 0.0);
        assert_eq!(stage.steps.unwrap().latched, 0);

        // Escape clears a selection first, then puts the deck's window
        // away, then ends the mode.
        stage.steps_frame_timed(0b100, 0.0);
        stage.steps_frame_timed(0b100, f32::from(STEP_HOLD_MS) / 1000.0);
        stage.steps_frame_timed(0, 0.0);
        assert_eq!(stage.steps.unwrap().latched, 0b100);
        assert_eq!(stage.apply(StageIntent::Escape), ApplyOutcome::Changed);
        assert!(stage.steps.is_some());
        assert_eq!(stage.steps.unwrap().latched, 0);
        assert!(stage.deck_open());
        assert_eq!(stage.apply(StageIntent::Escape), ApplyOutcome::Changed);
        assert!(!stage.deck_open());
        assert!(stage.steps.is_some());
        assert_eq!(stage.apply(StageIntent::Escape), ApplyOutcome::Changed);
        assert!(stage.steps.is_none());
    }

    /// With steps held, a cell reads the lock on those steps and moves
    /// with the turn; with none, it reads the knob.
    #[test]
    fn a_cell_shows_the_held_steps_lock_and_the_knob_otherwise() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        into_clip(&mut stage);
        stage.song.set_lane(0, crate::lane::Lane::Drum);
        stage.page(PageKey::Fx, false).unwrap();
        stage.toggle_step_keys().unwrap();
        let knob = stage.deck_slots()[0].as_ref().unwrap().value.clone();
        stage.steps_frame(0b1);
        for _ in 0..3 {
            assert_eq!(
                stage.apply(StageIntent::Turn {
                    up: true,
                    coarse: true
                }),
                ApplyOutcome::Changed
            );
        }
        let slots = stage.deck_slots();
        let held = slots[0].as_ref().unwrap();
        assert!(held.locked);
        assert_ne!(
            held.value, knob,
            "the cell kept showing the knob under the lock"
        );
        stage.steps_frame(0);
        let slots = stage.deck_slots();
        let released = slots[0].as_ref().unwrap();
        assert_eq!(
            released.value, knob,
            "with nothing held the cell reads the knob"
        );
        // The mark stays: the cursor stands on a locked step, and the
        // inverted cell says so; only the readout goes back to the knob.
        assert!(released.locked);
    }

    /// Ctrl+A in the grammar, then a turn on the deck: every selected
    /// step takes the lock, with no key held.
    /// The bug this guards: with the window up, Enter used to fall
    /// through to the grid as ACT, which pastes the last chord over the
    /// selection — clearing the note the deck had just turned.
    #[test]
    fn enter_with_the_deck_up_keeps_the_turned_note_and_places_nothing() {
        use crate::ui::stage::key::{Key, Mods};
        use crate::ui::stage::keymap::ScopeContext;
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        into_clip(&mut stage);
        let opened = stage.inside.unwrap();
        stage.toggle_step(3);
        stage.page(PageKey::Trig, false).unwrap();
        assert_eq!(stage.scope_context(), ScopeContext::Deck);
        // Select the note the grammar's way, then turn NOTE on the deck.
        let pattern = stage.song.pattern(opened.pattern).unwrap().clone();
        let notes = crate::ui::sequencer::note_views(&pattern, &stage.song.key);
        let clip = sequence::ClipView {
            id: opened.pattern.0,
            name: &pattern.name,
            length_ticks: crate::ui::sequencer::pattern_length(&stage.song, opened.pattern),
            notes: &notes,
            ghosts: &[],
            slicing: false,
            rules: &[],
        };
        for step in [3usize, 6] {
            stage
                .sequencer
                .set_time_selected(clip, step * PATTERN_STEP_TICKS, true);
        }
        let midi_of = |stage: &Stage, step: usize| {
            let pattern = stage.song.pattern(opened.pattern).unwrap();
            pattern
                .trig(step)
                .primary()
                .map(|note| crate::pitch::nearest_midi(note.pitch.resolve(&stage.song.key)))
        };
        assert_eq!(midi_of(&stage, 3), Some(60));
        assert_eq!(midi_of(&stage, 6), None);
        assert_eq!(
            stage.apply(StageIntent::Turn {
                up: true,
                coarse: true
            }),
            ApplyOutcome::Changed
        );
        assert_eq!(midi_of(&stage, 3), Some(72));
        // Enter is the deck's: the turned note stays, the empty step
        // stays empty (placing is the grid's, in the standard mode),
        // the selection stands and the scope holds.
        assert_eq!(
            stage.handle_key(Mods::NONE, Key::Enter),
            Some(ApplyOutcome::Changed)
        );
        assert_eq!(
            midi_of(&stage, 3),
            Some(72),
            "Enter pasted over the turned note"
        );
        assert_eq!(midi_of(&stage, 6), None, "the deck placed a trig");
        assert_eq!(stage.notice.as_deref(), Some("kept"));
        assert_eq!(stage.scope_context(), ScopeContext::Deck);
        assert!(stage.standing_selection().is_some());
    }

    /// The drum lane's FX start OUT, and an OUT section is not in the
    /// graph: the first turn on it switches it in, so the turn is heard,
    /// and the two settle as one undo step.
    #[test]
    fn a_turn_on_an_out_section_switches_it_in_and_is_heard() {
        use crate::params::console::drive;
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        into_clip(&mut stage);
        // A track with nothing to play is not built at all.
        stage.toggle_step(0);
        stage.song.set_lane(0, crate::lane::Lane::Drum);
        stage.settle();
        stage.page(PageKey::Fx, false).unwrap();
        assert_eq!(stage.apply(StageIntent::Slot(1)), ApplyOutcome::Changed);
        let opened = stage.inside.unwrap();
        let drive = stage
            .song
            .section(opened.track, SectionKind::Drive)
            .expect("the drum strip has DRIVE")
            .id;
        assert!(
            stage.song.device(drive).unwrap().bypassed,
            "DRIVE starts OUT"
        );
        assert!(stage.deck_slots()[1].as_ref().unwrap().out);
        let before = stage.song.device(drive).unwrap().value(drive::DRIVE);
        assert_eq!(
            stage.apply(StageIntent::Turn {
                up: true,
                coarse: false
            }),
            ApplyOutcome::Changed
        );
        let device = stage.song.device(drive).unwrap();
        assert!(!device.bypassed, "the turn did not switch DRIVE in");
        assert!(device.value(drive::DRIVE) > before, "the knob did not move");
        let notice = stage.notice.clone().unwrap_or_default();
        assert!(
            notice.starts_with(&format!("{} in", SectionKind::Drive.name())),
            "notice was {notice:?}"
        );
        assert!(!stage.deck_slots()[1].as_ref().unwrap().out);
        // Heard: the section is built, and a letter reaches it.
        let playing = vec![Some(0); stage.song.tracks.len()];
        let (_, nodes) = crate::song_graph::build(&stage.song, &playing);
        assert!(
            nodes.devices.iter().any(|(id, _)| *id == drive),
            "DRIVE is still not in the graph"
        );
        // One undo puts back both the switch and the value.
        assert_eq!(stage.apply(StageIntent::Undo), ApplyOutcome::Changed);
        let device = stage.song.device(drive).unwrap();
        assert!(device.bypassed, "undo left DRIVE in");
        assert_eq!(device.value(drive::DRIVE), before);
    }

    /// A lock written to an OUT section would be dropped at the note
    /// cut: the turn that writes it switches the section in first, and
    /// the knob itself stays where it was.
    #[test]
    fn a_lock_on_an_out_section_switches_it_in() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        into_clip(&mut stage);
        stage.song.set_lane(0, crate::lane::Lane::Drum);
        stage.page(PageKey::Fx, false).unwrap();
        let opened = stage.inside.unwrap();
        let pattern = stage.song.pattern(opened.pattern).unwrap().clone();
        let notes = crate::ui::sequencer::note_views(&pattern, &stage.song.key);
        let clip = sequence::ClipView {
            id: opened.pattern.0,
            name: &pattern.name,
            length_ticks: crate::ui::sequencer::pattern_length(&stage.song, opened.pattern),
            notes: &notes,
            ghosts: &[],
            slicing: false,
            rules: &[],
        };
        for step in [1usize, 4] {
            stage
                .sequencer
                .set_time_selected(clip, step * PATTERN_STEP_TICKS, true);
        }
        let drive = stage
            .song
            .section(opened.track, SectionKind::Drive)
            .expect("the drum strip has DRIVE")
            .id;
        assert!(stage.song.device(drive).unwrap().bypassed);
        let Slot::Param { id: param, .. } = stage.selected_page().unwrap().1.slots[0].unwrap()
        else {
            panic!("a parameter slot")
        };
        let knob = stage.song.device(drive).unwrap().value(param);
        assert_eq!(
            stage.apply(StageIntent::Turn {
                up: true,
                coarse: false
            }),
            ApplyOutcome::Changed
        );
        assert!(
            !stage.song.device(drive).unwrap().bypassed,
            "the lock did not switch DRIVE in"
        );
        assert_eq!(stage.song.device(drive).unwrap().value(param), knob);
        let pattern = stage.song.pattern(opened.pattern).unwrap();
        for step in [1usize, 4] {
            assert!(
                pattern.trig(step).lock_on(Some(drive), param).is_some(),
                "step {step} missed the lock"
            );
        }
        assert!(stage.deck_slots()[0].as_ref().unwrap().locked);
        assert!(!stage.deck_slots()[0].as_ref().unwrap().out);
    }

    /// A cell on an OUT section says so, and never lights a lock the
    /// engine cannot hear.
    #[test]
    fn an_out_cell_says_so_and_never_shows_a_lock() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        into_clip(&mut stage);
        stage.song.set_lane(0, crate::lane::Lane::Drum);
        stage.page(PageKey::Fx, false).unwrap();
        let opened = stage.inside.unwrap();
        let drive = stage
            .song
            .section(opened.track, SectionKind::Drive)
            .expect("the drum strip has DRIVE")
            .id;
        let Slot::Param { id: param, .. } = stage.selected_page().unwrap().1.slots[0].unwrap()
        else {
            panic!("a parameter slot")
        };
        let step = stage.cursor_step().expect("a cursor step");
        stage.apply_sequence(
            opened.pattern,
            &[sequence::Intent::SetLock {
                tick: step * PATTERN_STEP_TICKS,
                device: Some(drive.0),
                param,
                value: 1.0,
            }],
        );
        let cell = stage.deck_slots()[0].clone().expect("the DRIVE cell");
        assert!(cell.out, "the cell does not say OUT");
        assert!(!cell.locked, "the cell lit a lock on an OUT section");
        stage.song.device_mut(drive).unwrap().bypassed = false;
        let cell = stage.deck_slots()[0].clone().expect("the DRIVE cell");
        assert!(!cell.out);
        assert!(cell.locked);
    }

    /// A put's sound lock arrives by address: the stage looks the source
    /// step up and lays its sound on the target, or clears it.
    #[test]
    fn a_copied_sound_lock_is_resolved_by_the_stage() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        let pattern = into_clip(&mut stage);
        stage.toggle_step(0);
        let sound = crate::sound::Sound::capture(&stage.song.tracks[0]);
        stage.song.pattern_mut(pattern).unwrap().set_sound_lock(
            0,
            Some(crate::sequencing::SoundLock {
                name: "eight oh eight".to_owned(),
                sound,
            }),
        );
        stage.apply_sequence(
            pattern,
            &[sequence::Intent::CopySound {
                tick: 4 * PATTERN_STEP_TICKS,
                from_pattern: pattern.0,
                from_tick: 0,
            }],
        );
        let trig = stage.song.pattern(pattern).unwrap().trig(4).clone();
        assert_eq!(
            trig.sound.as_ref().map(|lock| lock.name.as_str()),
            Some("eight oh eight")
        );
        // From a step with no sound, the target's is cleared.
        stage.apply_sequence(
            pattern,
            &[sequence::Intent::CopySound {
                tick: 4 * PATTERN_STEP_TICKS,
                from_pattern: pattern.0,
                from_tick: 8 * PATTERN_STEP_TICKS,
            }],
        );
        assert!(stage.song.pattern(pattern).unwrap().trig(4).sound.is_none());
    }

    #[test]
    fn a_standing_selection_takes_a_lock_from_a_deck_turn() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        into_clip(&mut stage);
        stage.song.set_lane(0, crate::lane::Lane::Drum);
        stage.page(PageKey::Fx, false).unwrap();
        // Select three steps the way the grammar's X sweep and Ctrl+A
        // do: on the panel, as time.
        let opened = stage.inside.unwrap();
        let pattern = stage.song.pattern(opened.pattern).unwrap().clone();
        let notes = crate::ui::sequencer::note_views(&pattern, &stage.song.key);
        let clip = sequence::ClipView {
            id: opened.pattern.0,
            name: &pattern.name,
            length_ticks: crate::ui::sequencer::pattern_length(&stage.song, opened.pattern),
            notes: &notes,
            ghosts: &[],
            slicing: false,
            rules: &[],
        };
        for step in [0usize, 2, 5] {
            stage
                .sequencer
                .set_time_selected(clip, step * PATTERN_STEP_TICKS, true);
        }
        let selected = stage.standing_selection().expect("a standing selection");
        assert_eq!(selected, vec![0, 2, 5]);
        assert_eq!(
            stage.apply(StageIntent::Turn {
                up: true,
                coarse: false
            }),
            ApplyOutcome::Changed
        );
        let opened = stage.inside.unwrap();
        let page = stage.selected_page().unwrap().1;
        let Slot::Param {
            subject: Subject::Section(kind),
            id,
        } = page.slots[0].unwrap()
        else {
            panic!("section slot")
        };
        let device = stage.song.section(opened.track, kind).unwrap().id;
        let pattern = stage.song.pattern(opened.pattern).unwrap();
        for step in &selected {
            assert!(
                pattern.trig(*step).lock_on(Some(device), id).is_some(),
                "step {step} missed the lock"
            );
        }
        assert!(stage.deck_slots()[0].as_ref().unwrap().locked);
        // The knob itself did not move.
        let knob = stage.song.device(device).unwrap().value(id);
        let def = stage.song.device(device).unwrap().table()[0];
        assert_eq!(knob, def.default);
    }

    #[test]
    fn two_held_steps_receive_section_locks_together() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        into_clip(&mut stage);
        stage.song.set_lane(0, crate::lane::Lane::Drum);
        stage.page(PageKey::Fx, false).unwrap();
        stage.toggle_step_keys().unwrap();
        stage.steps_frame(0b11);
        let before = stage.song.clone();
        stage.history = crate::history::History::new(before.clone());
        assert_eq!(
            stage.apply(StageIntent::Turn {
                up: true,
                coarse: false
            }),
            ApplyOutcome::Changed
        );
        let opened = stage.inside.unwrap();
        let page = stage.selected_page().unwrap().1;
        let Slot::Param {
            subject: Subject::Section(kind),
            id,
        } = page.slots[0].unwrap()
        else {
            panic!("section slot")
        };
        let device = stage.song.section(opened.track, kind).unwrap().id;
        for step in 0..2 {
            assert!(
                stage
                    .song
                    .pattern(opened.pattern)
                    .unwrap()
                    .trig(step)
                    .lock_on(Some(device), id)
                    .is_some()
            );
        }
        stage.history.observe(&stage.song);
        assert!(stage.history.undo(&mut stage.song));
        assert_eq!(stage.song, before);
        assert!(
            !stage.history.can_undo(),
            "the batch made more than one step"
        );
    }
}
