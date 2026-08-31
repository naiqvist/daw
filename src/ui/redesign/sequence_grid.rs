//! Wrapped 64-step projection of the canonical sequence.
//!
//! Time reads left-to-right, then continues on the next row. Vertical
//! position has no pitch meaning: `row * 16 + column` is the one address.

use crate::pitch::Pitch;
use crate::sequencing::{GRID_COLUMNS, GRID_ROWS, PATTERN_STEPS};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::redesign::OUTLINE;
use crate::ui::redesign::grammar::{Motion, Utterance, Voice};
use crate::ui::redesign::grid_resolution::{GridResolution, TICKS_PER_BAR};
use crate::ui::redesign::lens::LensView;
use crate::ui::redesign::registers::{Payload, Registers, TrigNote};
use crate::ui::redesign::sequence::{ClipView, Intent, NoteView};
use crate::ui::redesign::verbs::Verb;
use crate::ui::tokens::{font, space, stroke};
use eframe::egui;

const MAX_CELL_SIDE: f32 = 44.0;
const CELL_GAP: f32 = space::XXS;
const ROW_GAP: f32 = space::MD;
const STATUS_HEIGHT: f32 = 24.0;
const ROW_ADDRESS_WIDTH: f32 = 48.0;
const BEAT_STRONG: u8 = 52;
const BEAT_SECONDARY: u8 = 34;
const BEAT_WEAK: u8 = 20;
const ACTIVE_BAR_HEIGHT: f32 = space::XS;
const ACTIVE_BAR_INSET: f32 = space::XS;
const ONSET_WIDTH: f32 = 3.0;
const CURSOR_GAP: f32 = 3.0;
const CURSOR_CAP: f32 = 10.0;
const DEFAULT_PITCH: u8 = 60;
const DEFAULT_VELOCITY: u8 = 100;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TrigSelection {
    pub(crate) step: usize,
    pub(crate) tick: usize,
    /// The trig's primary note, whole: address, deviation, substrate.
    pub(crate) primary: Option<NoteView>,
    pub(crate) tone_count: usize,
}

pub(crate) struct SequenceGrid {
    cursor_step: usize,
    resolution: GridResolution,
    last_pitch: Pitch,
    /// The last refusal, shown in the status line until the next sentence.
    /// Silence is forbidden: an unsupported verb answers out loud.
    refusal: Option<String>,
}

impl Default for SequenceGrid {
    fn default() -> Self {
        Self {
            cursor_step: 0,
            resolution: GridResolution::default(),
            last_pitch: Pitch::from_midi(DEFAULT_PITCH),
            refusal: None,
        }
    }
}

impl SequenceGrid {
    pub(crate) fn update_resolution(&mut self, ctx: &egui::Context) {
        self.resolution.update(ctx);
    }

    #[allow(clippy::too_many_arguments)] // One read-only context per concern.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        available: egui::Rect,
        focused: bool,
        voice: &mut Voice<'_>,
        clip: Option<ClipView<'_>>,
        lens: &LensView,
        intents: &mut Vec<Intent>,
    ) {
        if focused {
            self.keyboard(ui.ctx(), voice, clip, intents);
        }
        // Sentence-in-progress outranks a stale refusal on the status line.
        let overlay = if voice.sentence.is_empty() {
            self.refusal.clone()
        } else {
            Some(voice.sentence.display())
        };

        let horizontal_gaps = CELL_GAP * (GRID_COLUMNS - 1) as f32;
        let width_limited =
            (available.width() - ROW_ADDRESS_WIDTH - horizontal_gaps) / GRID_COLUMNS as f32;
        let height_limited =
            (available.height() - STATUS_HEIGHT - ROW_GAP * (GRID_ROWS - 1) as f32)
                / GRID_ROWS as f32;
        let cell_side = width_limited
            .min(height_limited)
            .clamp(1.0, MAX_CELL_SIDE)
            .floor();
        let grid_width = cell_side * GRID_COLUMNS as f32 + horizontal_gaps;
        let grid_height = cell_side * GRID_ROWS as f32 + ROW_GAP * (GRID_ROWS - 1) as f32;
        let full_width = ROW_ADDRESS_WIDTH + grid_width;
        let full_height = STATUS_HEIGHT + grid_height;
        let origin = egui::pos2(
            available.center().x - full_width * 0.5,
            available.center().y - full_height * 0.5,
        );
        let grid_origin = origin + egui::vec2(ROW_ADDRESS_WIDTH, STATUS_HEIGHT);
        let painter = ui.painter_at(available);
        self.draw_status(&painter, origin, full_width, clip, lens, overlay.as_deref());

        for row in 0..GRID_ROWS {
            let row_tick = row * GRID_COLUMNS * self.resolution.step_ticks();
            draw_row_address(
                &painter,
                egui::pos2(
                    origin.x + ROW_ADDRESS_WIDTH - space::SM,
                    row_y(grid_origin.y, cell_side, row),
                ),
                row,
                row_tick,
                cell_side,
            );
            for column in 0..GRID_COLUMNS {
                let step = row * GRID_COLUMNS + column;
                let rect = egui::Rect::from_min_size(
                    egui::pos2(
                        grid_origin.x + column as f32 * (cell_side + CELL_GAP),
                        row_y(grid_origin.y, cell_side, row),
                    ),
                    egui::Vec2::splat(cell_side),
                );
                let response = ui
                    .interact(
                        rect,
                        ui.id().with(("sequence-step", step)),
                        egui::Sense::click(),
                    )
                    .affords(Affords::Press);
                if response.clicked() {
                    self.cursor_step = step;
                    self.toggle(intents);
                }

                let tick = step * self.resolution.step_ticks();
                painter.rect_filled(rect, 0.0, beat_fill(tick));
                self.draw_cell_events(&painter, rect, clip, lens, step);
                if self.cursor_step == step {
                    draw_cursor(&painter, rect);
                }
            }
        }
    }

    fn draw_status(
        &self,
        painter: &egui::Painter,
        origin: egui::Pos2,
        width: f32,
        clip: Option<ClipView<'_>>,
        lens: &LensView,
        overlay: Option<&str>,
    ) {
        let rect = egui::Rect::from_min_size(origin, egui::vec2(width, STATUS_HEIGHT));
        painter.text(
            rect.left_center() + egui::vec2(ROW_ADDRESS_WIDTH, 0.0),
            egui::Align2::LEFT_CENTER,
            // The harmonic context reads here: key sign and active lens,
            // beside the clip — a context with no sign is a trap.
            if let Some(clip) = clip {
                let bars = clip.length_ticks.div_ceil(TICKS_PER_BAR);
                if width >= 560.0 {
                    format!(
                        "SEQ 64  /  {}  /  {:02}B  /  GRID {}  /  {}",
                        clip.name,
                        bars,
                        self.resolution.label(),
                        lens.status
                    )
                } else {
                    format!("{} / {}", clip.name, self.resolution.label())
                }
            } else if width >= 560.0 {
                format!(
                    "SEQ 64  /  SELECT MIDI CLIP  /  GRID {}  /  {}",
                    self.resolution.label(),
                    lens.status
                )
            } else {
                "SELECT MIDI CLIP".to_owned()
            },
            egui::FontId::new(font::BODY, egui::FontFamily::Monospace),
            OUTLINE,
        );
        // The right side is the grammar's mouth: the sentence-in-progress
        // or a refusal takes the spot, the standing legend fills the quiet.
        if let Some(overlay) = overlay {
            painter.text(
                rect.right_center(),
                egui::Align2::RIGHT_CENTER,
                overlay,
                egui::FontId::new(font::BODY, egui::FontFamily::Monospace),
                OUTLINE,
            );
        } else if width >= 560.0 {
            let bars = PATTERN_STEPS.div_ceil(self.resolution.steps_per_bar());
            painter.text(
                rect.right_center(),
                egui::Align2::RIGHT_CENTER,
                format!("{bars:02} BARS  /  CTRL 1- 2+ 3T"),
                egui::FontId::new(font::MINI_LABEL, egui::FontFamily::Monospace),
                egui::Color32::from_gray(176),
            );
        }
    }

    fn draw_cell_events(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        clip: Option<ClipView<'_>>,
        lens: &LensView,
        step: usize,
    ) {
        let tick = step * self.resolution.step_ticks();
        let primary = clip.and_then(|clip| primary_at(clip, tick));
        let tone_count = clip.map_or(0, |clip| tone_count_at(clip, tick));
        let continuation = clip
            .map(|clip| continuation_fraction(clip, tick, self.resolution.step_ticks()))
            .unwrap_or(0.0);
        if let Some(note) = primary.filter(|note| note.enabled) {
            draw_active_rail(painter, rect, note.velocity);
        } else if continuation > 0.0 {
            draw_continuation(painter, rect, continuation);
        }

        let Some(note) = primary else {
            return;
        };
        let ink = trig_ink(note.enabled, note.probability);
        // A pushed note shows its push as geometry: the onset bar sits
        // displaced within the cell — an index, not a symbol.
        let push = (f32::from(note.micro_ticks) / self.resolution.step_ticks().max(1) as f32)
            .clamp(0.0, 0.9)
            * rect.width();
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.left() + push, rect.top()),
                egui::pos2(rect.left() + push + ONSET_WIDTH, rect.bottom()),
            ),
            0.0,
            ink,
        );
        painter.text(
            rect.center() + egui::vec2(ONSET_WIDTH * 0.5, -space::XXS),
            egui::Align2::CENTER_CENTER,
            cell_label(note, lens),
            egui::FontId::new(font::BODY, egui::FontFamily::Monospace),
            ink,
        );
        if tone_count > 1 {
            painter.text(
                rect.right_top() + egui::vec2(-space::XS, space::XS),
                egui::Align2::RIGHT_TOP,
                format!("+{}", tone_count - 1),
                egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                ink,
            );
        }
        // A conditional trig is a rule, not an event: the document says so
        // by printing the condition on the trig (settled ruling: signed
        // conditions, `notes/20260831-command-grammar.md`).
        if note.enabled && note.probability < 1.0 {
            painter.text(
                rect.right_bottom() + egui::vec2(-space::XS, -space::XS),
                egui::Align2::RIGHT_BOTTOM,
                format!("{:.0}%", note.probability * 100.0),
                egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                ink,
            );
        }
        // A pending transform previews as a ghost: the would-be spelling
        // in ghost ink under the standing one. Nothing has changed yet.
        if let Some(clip) = clip
            && let Some(ghost) = clip
                .ghosts
                .iter()
                .filter(|ghost| ghost.start_ticks == tick)
                .min_by(|a, b| a.pitch.stack_order(&b.pitch))
        {
            painter.text(
                rect.left_bottom() + egui::vec2(ONSET_WIDTH + space::XS, -space::XXS),
                egui::Align2::LEFT_BOTTOM,
                cell_label(ghost, lens),
                egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                egui::Color32::from_gray(112),
            );
        }
    }

    /// Interpret this frame's utterance against the grid's noun: the trig
    /// under the cursor. Verbs the trig does not support are refused out
    /// loud, never silently dropped (`notes/20260831-command-grammar.md`).
    fn keyboard(
        &mut self,
        ctx: &egui::Context,
        voice: &mut Voice<'_>,
        clip: Option<ClipView<'_>>,
        intents: &mut Vec<Intent>,
    ) {
        let Some(utterance) = voice.sentence.consume(ctx) else {
            return;
        };
        self.refusal = None;
        self.speak(utterance, voice.registers, clip, intents);
    }

    fn speak(
        &mut self,
        utterance: Utterance,
        registers: &mut Registers,
        clip: Option<ClipView<'_>>,
        intents: &mut Vec<Intent>,
    ) {
        let count = utterance.count as isize;
        let tick = self.cursor_step * self.resolution.step_ticks();
        match (utterance.verb, utterance.motion) {
            // Hold-as-preposition: the same arrows, spoken while holding
            // the trig qualifier, edit the trig instead of travelling.
            (None, Some(motion @ (Motion::Up | Motion::Down))) if utterance.held => {
                if trig_at(clip, tick).is_some() {
                    intents.push(Intent::AdjustVelocity {
                        tick,
                        delta: count * if motion == Motion::Up { 1 } else { -1 },
                    });
                } else {
                    self.refusal = Some("HOLD: NOTHING HERE".to_owned());
                }
            }
            (None, Some(_)) if utterance.held => {
                self.refusal = Some("HOLD: UP OR DOWN".to_owned());
            }
            (None, Some(motion)) => self.move_by(count * motion_steps(motion)),
            (Some(Verb::Act), _) => self.toggle(intents),
            (Some(Verb::Delete), _) => intents.push(Intent::Clear { tick }),
            (Some(Verb::Nudge), Some(motion)) => {
                let steps = count * motion_steps(motion);
                intents.push(Intent::Nudge {
                    tick,
                    delta_ticks: steps * self.resolution.step_ticks() as isize,
                });
                self.move_by(steps);
            }
            (Some(Verb::Resize), Some(motion @ (Motion::Left | Motion::Right))) => {
                intents.push(Intent::Resize {
                    tick,
                    delta_ticks: count
                        * motion_steps(motion)
                        * self.resolution.step_ticks() as isize,
                });
            }
            (Some(Verb::Resize), Some(_)) => {
                self.refusal = Some("RESIZE: LEFT OR RIGHT".to_owned());
            }
            (Some(Verb::Yank), _) => match trig_at(clip, tick) {
                Some(notes) => {
                    registers.yank(Payload::Trig(notes));
                    self.refusal = Some("YANKED A TRIG".to_owned());
                }
                None => self.refusal = Some("YANK: NOTHING HERE".to_owned()),
            },
            (Some(Verb::Put), _) => match registers.trig() {
                Ok(notes) => {
                    intents.push(Intent::Clear { tick });
                    for note in notes {
                        intents.push(Intent::AddNote {
                            tick,
                            pitch: note.pitch,
                            length_ticks: note.length_ticks,
                            velocity: note.velocity,
                            probability: note.probability,
                        });
                    }
                }
                Err(refusal) => self.refusal = Some(refusal),
            },
            (Some(Verb::Duplicate), _) => match trig_at(clip, tick) {
                Some(notes) => {
                    // Yank-and-put-adjacent in one keystroke: the copy
                    // lands `count` steps ahead and the cursor rides
                    // along, Elektron style. The register is untouched —
                    // duplicate is a shorthand, not a yank.
                    let steps = count.max(1);
                    let target = (self.cursor_step as isize + steps)
                        .rem_euclid(PATTERN_STEPS as isize)
                        as usize
                        * self.resolution.step_ticks();
                    intents.push(Intent::Clear { tick: target });
                    for note in notes {
                        intents.push(Intent::AddNote {
                            tick: target,
                            pitch: note.pitch,
                            length_ticks: note.length_ticks,
                            velocity: note.velocity,
                            probability: note.probability,
                        });
                    }
                    self.move_by(steps);
                }
                None => self.refusal = Some("DUPLICATE: NOTHING HERE".to_owned()),
            },
            (Some(Verb::Condition), _) => {
                match clip.and_then(|clip| primary_at(clip, tick)) {
                    Some(note) => {
                        // `50 C` says fifty percent; bare C cycles the
                        // canonical ladder. A count of 1 is read as bare —
                        // a one-percent trig is a typo, not an intent.
                        let probability = if utterance.count > 1 {
                            (utterance.count.min(100)) as f32 / 100.0
                        } else {
                            next_probability(note.probability)
                        };
                        intents.push(Intent::SetProbability { tick, probability });
                    }
                    None => self.refusal = Some("CONDITION: NOTHING HERE".to_owned()),
                }
            }
            (Some(verb), _) => {
                self.refusal = Some(format!("{}: NOT HERE", verb.name()));
            }
            (None, None) => {}
        }
    }

    fn move_by(&mut self, amount: isize) {
        self.cursor_step =
            (self.cursor_step as isize + amount).rem_euclid(PATTERN_STEPS as isize) as usize;
    }

    fn toggle(&mut self, intents: &mut Vec<Intent>) {
        intents.push(Intent::Toggle {
            tick: self.cursor_step * self.resolution.step_ticks(),
            default_pitch: self.last_pitch,
            default_length_ticks: self.resolution.step_ticks(),
            default_velocity: DEFAULT_VELOCITY,
        });
    }

    /// The cursor's address in ticks — the universal selection the
    /// palette long forms act on.
    pub(crate) fn cursor_tick(&self) -> usize {
        self.cursor_step * self.resolution.step_ticks()
    }

    pub(crate) fn selection(&self, clip: Option<ClipView<'_>>) -> TrigSelection {
        let tick = self.cursor_step * self.resolution.step_ticks();
        TrigSelection {
            step: self.cursor_step,
            tick,
            primary: clip.and_then(|clip| primary_at(clip, tick)).copied(),
            tone_count: clip.map_or(0, |clip| tone_count_at(clip, tick)),
        }
    }

    pub(crate) fn enter_pitch(&mut self, pitch: Pitch, intents: &mut Vec<Intent>) {
        self.last_pitch = pitch;
        intents.push(Intent::SetPrimary {
            tick: self.cursor_step * self.resolution.step_ticks(),
            pitch: self.last_pitch,
            length_ticks: self.resolution.step_ticks(),
            velocity: DEFAULT_VELOCITY,
        });
    }
}

/// The trig cell's pitch text: the address through the lens, the
/// musician's bend as a raised tick, the machine's approximation as a
/// leading `≈` — two deviation sign classes, because they mean different
/// things (`notes/20260831-pitch-lens-spec.md` §4).
fn cell_label(note: &NoteView, lens: &LensView) -> String {
    use crate::ui::redesign::signs;
    let name = crate::ui::redesign::lens::address_label(&lens.active, note, &lens.key);
    let bend = if note.pitch.offset_cents > 0.0 {
        signs::BEND_UP
    } else if note.pitch.offset_cents < 0.0 {
        signs::BEND_DOWN
    } else if note.micro_ticks > 0 {
        signs::BEND_UP
    } else if note.micro_ticks < 0 {
        signs::BEND_DOWN
    } else {
        ""
    };
    format!(
        "{}{name}{bend}",
        if note.approx { signs::APPROX } else { "" }
    )
}

/// The trig's three voices, ordered by value: a certain trig speaks at
/// full white, a conditional one steps down (it is a rule, not an event),
/// and a muted one recedes to furniture. Hierarchy by value alone.
pub(crate) fn trig_ink(enabled: bool, probability: f32) -> egui::Color32 {
    if !enabled {
        egui::Color32::from_gray(92)
    } else if probability < 1.0 {
        egui::Color32::from_gray(150)
    } else {
        OUTLINE
    }
}

/// A horizontal step is one cell; a vertical step is one visual row,
/// sixteen chronological steps.
fn motion_steps(motion: Motion) -> isize {
    match motion {
        Motion::Left => -1,
        Motion::Right => 1,
        Motion::Up => -(GRID_COLUMNS as isize),
        Motion::Down => GRID_COLUMNS as isize,
    }
}

/// The condition ladder bare C walks: certain, then thinner and thinner,
/// then certain again. A closed set of signs, not a continuous dial —
/// the exact percentages come from `count C`.
pub(crate) fn next_probability(current: f32) -> f32 {
    const LADDER: [f32; 5] = [1.0, 0.75, 0.5, 0.25, 0.1];
    let position = LADDER
        .iter()
        .position(|p| (p - current).abs() < 0.05)
        .unwrap_or(LADDER.len() - 1);
    LADDER[(position + 1) % LADDER.len()]
}

/// Every note sharing the step, as register payload — or `None` when the
/// step is empty, so yank and duplicate can refuse honestly.
pub(crate) fn trig_at(clip: Option<ClipView<'_>>, tick: usize) -> Option<Vec<TrigNote>> {
    let notes: Vec<TrigNote> = clip?
        .notes
        .iter()
        .filter(|note| note.start_ticks == tick)
        .map(|note| TrigNote {
            pitch: note.pitch,
            length_ticks: note.length_ticks,
            velocity: note.velocity,
            probability: note.probability,
            enabled: note.enabled,
        })
        .collect();
    (!notes.is_empty()).then_some(notes)
}

fn primary_at(clip: ClipView<'_>, tick: usize) -> Option<&NoteView> {
    clip.notes
        .iter()
        .filter(|note| note.start_ticks == tick)
        .min_by(|a, b| a.pitch.stack_order(&b.pitch))
}

fn tone_count_at(clip: ClipView<'_>, tick: usize) -> usize {
    clip.notes
        .iter()
        .filter(|note| note.start_ticks == tick)
        .count()
}

fn row_y(grid_top: f32, cell_side: f32, row: usize) -> f32 {
    grid_top + row as f32 * (cell_side + ROW_GAP)
}

fn draw_row_address(
    painter: &egui::Painter,
    right_top: egui::Pos2,
    row: usize,
    tick: usize,
    cell_side: f32,
) {
    let start = row * GRID_COLUMNS + 1;
    painter.text(
        egui::pos2(right_top.x, right_top.y + cell_side * 0.38),
        egui::Align2::RIGHT_CENTER,
        format!("{start:02}"),
        egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
        OUTLINE,
    );
    if cell_side >= 32.0 {
        painter.text(
            egui::pos2(right_top.x, right_top.y + cell_side * 0.70),
            egui::Align2::RIGHT_CENTER,
            musical_position(tick),
            egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
            egui::Color32::from_gray(112),
        );
    }
}

fn musical_position(tick: usize) -> String {
    let bar = tick / TICKS_PER_BAR + 1;
    let beat = tick % TICKS_PER_BAR / (TICKS_PER_BAR / 4) + 1;
    format!("{bar}.{beat}")
}

fn continuation_fraction(clip: ClipView<'_>, cell_start: usize, step_ticks: usize) -> f32 {
    let cell_end = cell_start + step_ticks;
    let mut fraction = 0.0_f32;
    for note in clip.notes {
        if !note.enabled || note.start_ticks >= cell_start {
            continue;
        }
        let note_end = note.start_ticks.saturating_add(note.length_ticks);
        if note_end > cell_start {
            fraction = fraction
                .max((note_end.min(cell_end) - cell_start) as f32 / step_ticks.max(1) as f32);
        }
    }
    fraction
}

pub(crate) fn beat_fill(tick: usize) -> egui::Color32 {
    let beat_ticks = TICKS_PER_BAR / 4;
    let level = if tick.is_multiple_of(beat_ticks) {
        BEAT_STRONG
    } else if tick.is_multiple_of(beat_ticks / 2) {
        BEAT_SECONDARY
    } else {
        BEAT_WEAK
    };
    egui::Color32::from_gray(level)
}

/// The rail's ink carries the VELOCITY: a whisper of a trig draws a
/// quiet rail, an accent a white one. Value is the axis (charter), and
/// the whole kit's dynamics read at a glance without opening a trig.
pub(crate) fn velocity_ink(velocity: u8) -> egui::Color32 {
    // 1..=127 maps into gray(96..=255): the floor keeps even the softest
    // trig clearly present — a rail is a fact before it is a level.
    let level = 96.0 + (f32::from(velocity.clamp(1, 127)) / 127.0) * 159.0;
    egui::Color32::from_gray(level as u8)
}

fn draw_active_rail(painter: &egui::Painter, rect: egui::Rect, velocity: u8) {
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(
                rect.left() + ACTIVE_BAR_INSET,
                rect.bottom() - ACTIVE_BAR_INSET - ACTIVE_BAR_HEIGHT,
            ),
            egui::pos2(
                rect.right() - ACTIVE_BAR_INSET,
                rect.bottom() - ACTIVE_BAR_INSET,
            ),
        ),
        0.0,
        velocity_ink(velocity),
    );
}

fn draw_continuation(painter: &egui::Painter, rect: egui::Rect, fraction: f32) {
    painter.line_segment(
        [
            egui::pos2(rect.left(), rect.bottom() - space::XS),
            egui::pos2(
                rect.left() + rect.width() * fraction.clamp(0.0, 1.0),
                rect.bottom() - space::XS,
            ),
        ],
        egui::Stroke::new(stroke::BOLD, OUTLINE),
    );
}

pub(crate) fn note_name(pitch: u8) -> String {
    let octave = i16::from(pitch / 12) - 1;
    format!("{}{octave}", crate::theory::pitch_class_name(pitch))
}

pub(crate) fn draw_cursor(painter: &egui::Painter, cell: egui::Rect) {
    let rect = cell.expand(CURSOR_GAP);
    let cap = CURSOR_CAP.min(rect.width() * 0.4);
    let cursor_stroke = egui::Stroke::new(stroke::MARK, OUTLINE);
    for (from, to) in [
        (rect.left_top(), rect.left_top() + egui::vec2(cap, 0.0)),
        (rect.left_top(), rect.left_top() + egui::vec2(0.0, cap)),
        (rect.right_top() - egui::vec2(cap, 0.0), rect.right_top()),
        (rect.right_top(), rect.right_top() + egui::vec2(0.0, cap)),
        (
            rect.left_bottom(),
            rect.left_bottom() + egui::vec2(cap, 0.0),
        ),
        (
            rect.left_bottom() - egui::vec2(0.0, cap),
            rect.left_bottom(),
        ),
        (
            rect.right_bottom() - egui::vec2(cap, 0.0),
            rect.right_bottom(),
        ),
        (
            rect.right_bottom() - egui::vec2(0.0, cap),
            rect.right_bottom(),
        ),
    ] {
        painter.line_segment([from, to], cursor_stroke);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_cursor_wraps_through_rows_and_pattern_end() {
        let mut grid = SequenceGrid::default();
        grid.cursor_step = 15;
        grid.move_by(1);
        assert_eq!(grid.cursor_step, 16);
        grid.cursor_step = 63;
        grid.move_by(1);
        assert_eq!(grid.cursor_step, 0);
        grid.move_by(-1);
        assert_eq!(grid.cursor_step, 63);
    }

    #[test]
    fn vertical_cursor_moves_sixteen_chronological_steps() {
        let mut grid = SequenceGrid::default();
        grid.cursor_step = 5;
        grid.move_by(GRID_COLUMNS as isize);
        assert_eq!(grid.cursor_step, 21);
        grid.move_by(-(GRID_COLUMNS as isize));
        assert_eq!(grid.cursor_step, 5);
    }

    #[test]
    fn duration_continues_across_the_visual_row_wrap() {
        let notes = [NoteView::from_midi(60, 15 * 12, 24, 100, 1.0, true)];
        let clip = ClipView {
            id: 1,
            name: "test",
            length_ticks: 64 * 12,
            notes: &notes,
            ghosts: &[],
        };
        assert_eq!(continuation_fraction(clip, 16 * 12, 12), 1.0);
    }

    fn utter(
        grid: &mut SequenceGrid,
        verb: Option<Verb>,
        motion: Option<Motion>,
        count: usize,
    ) -> Vec<Intent> {
        let mut registers = Registers::default();
        utter_on(grid, &mut registers, None, verb, motion, count)
    }

    fn utter_on(
        grid: &mut SequenceGrid,
        registers: &mut Registers,
        clip: Option<ClipView<'_>>,
        verb: Option<Verb>,
        motion: Option<Motion>,
        count: usize,
    ) -> Vec<Intent> {
        let mut intents = Vec::new();
        grid.refusal = None;
        grid.speak(
            Utterance {
                count,
                verb,
                motion,
                held: false,
            },
            registers,
            clip,
            &mut intents,
        );
        intents
    }

    fn one_note_clip(notes: &[NoteView]) -> ClipView<'_> {
        ClipView {
            id: 1,
            name: "test",
            length_ticks: 64 * 12,
            notes,
            ghosts: &[],
        }
    }

    fn utter_held(
        grid: &mut SequenceGrid,
        clip: Option<ClipView<'_>>,
        motion: Motion,
        count: usize,
    ) -> Vec<Intent> {
        let mut registers = Registers::default();
        let mut intents = Vec::new();
        grid.refusal = None;
        grid.speak(
            Utterance {
                count,
                verb: None,
                motion: Some(motion),
                held: true,
            },
            &mut registers,
            clip,
            &mut intents,
        );
        intents
    }

    /// Hold-as-preposition: held arrows edit the trig and never travel;
    /// a hold over nothing, or in a direction the trig cannot answer,
    /// refuses out loud.
    #[test]
    fn held_arrows_edit_velocity_and_never_travel() {
        let mut grid = SequenceGrid::default();
        let step = grid.resolution.step_ticks();
        let notes = [NoteView::from_midi(60, 0, step, 100, 1.0, true)];
        let clip = one_note_clip(&notes);

        let intents = utter_held(&mut grid, Some(clip), Motion::Up, 8);
        assert_eq!(intents, vec![Intent::AdjustVelocity { tick: 0, delta: 8 }]);
        assert_eq!(grid.cursor_step, 0, "a held motion never travels");

        let intents = utter_held(&mut grid, Some(clip), Motion::Down, 1);
        assert_eq!(intents, vec![Intent::AdjustVelocity { tick: 0, delta: -1 }]);

        let intents = utter_held(&mut grid, Some(clip), Motion::Left, 1);
        assert!(intents.is_empty());
        assert_eq!(grid.refusal.as_deref(), Some("HOLD: UP OR DOWN"));

        let intents = utter_held(&mut grid, None, Motion::Up, 1);
        assert!(intents.is_empty());
        assert_eq!(grid.refusal.as_deref(), Some("HOLD: NOTHING HERE"));
    }

    #[test]
    fn yank_then_put_lands_the_whole_trig_elsewhere() {
        let mut grid = SequenceGrid::default();
        let mut registers = Registers::default();
        let step = grid.resolution.step_ticks();
        let notes = [NoteView::from_midi(60, 0, step, 100, 0.75, true)];
        let clip = one_note_clip(&notes);

        let intents = utter_on(
            &mut grid,
            &mut registers,
            Some(clip),
            Some(Verb::Yank),
            None,
            1,
        );
        assert!(intents.is_empty());
        assert_eq!(grid.refusal.as_deref(), Some("YANKED A TRIG"));

        grid.cursor_step = 4;
        let intents = utter_on(
            &mut grid,
            &mut registers,
            Some(clip),
            Some(Verb::Put),
            None,
            1,
        );
        assert_eq!(
            intents,
            vec![
                Intent::Clear { tick: 4 * step },
                Intent::AddNote {
                    tick: 4 * step,
                    pitch: Pitch::from_midi(60),
                    length_ticks: step,
                    velocity: 100,
                    probability: 0.75,
                },
            ]
        );
    }

    #[test]
    fn yanking_an_empty_step_is_refused_and_put_says_so() {
        let mut grid = SequenceGrid::default();
        let mut registers = Registers::default();
        let intents = utter_on(&mut grid, &mut registers, None, Some(Verb::Yank), None, 1);
        assert!(intents.is_empty());
        assert_eq!(grid.refusal.as_deref(), Some("YANK: NOTHING HERE"));

        let intents = utter_on(&mut grid, &mut registers, None, Some(Verb::Put), None, 1);
        assert!(intents.is_empty());
        assert_eq!(grid.refusal.as_deref(), Some("PUT: NOTHING YANKED"));
    }

    #[test]
    fn bare_condition_walks_the_ladder_and_counted_condition_names_a_percentage() {
        let mut grid = SequenceGrid::default();
        let mut registers = Registers::default();
        let step = grid.resolution.step_ticks();
        let notes = [NoteView::from_midi(60, 0, step, 100, 1.0, true)];
        let clip = one_note_clip(&notes);

        let intents = utter_on(
            &mut grid,
            &mut registers,
            Some(clip),
            Some(Verb::Condition),
            None,
            1,
        );
        assert_eq!(
            intents,
            vec![Intent::SetProbability {
                tick: 0,
                probability: 0.75,
            }]
        );

        let intents = utter_on(
            &mut grid,
            &mut registers,
            Some(clip),
            Some(Verb::Condition),
            None,
            50,
        );
        assert_eq!(
            intents,
            vec![Intent::SetProbability {
                tick: 0,
                probability: 0.5,
            }]
        );

        let intents = utter_on(
            &mut grid,
            &mut registers,
            None,
            Some(Verb::Condition),
            None,
            1,
        );
        assert!(intents.is_empty());
        assert_eq!(grid.refusal.as_deref(), Some("CONDITION: NOTHING HERE"));
    }

    #[test]
    fn the_condition_ladder_is_closed_and_returns_home() {
        let mut probability = 1.0;
        for _ in 0..5 {
            probability = next_probability(probability);
        }
        assert_eq!(probability, 1.0, "five steps walk the whole ladder home");
        assert_eq!(next_probability(0.62), 1.0, "off-ladder values re-enter");
    }

    #[test]
    fn duplicate_copies_ahead_and_carries_the_cursor() {
        let mut grid = SequenceGrid::default();
        let mut registers = Registers::default();
        let step = grid.resolution.step_ticks();
        let notes = [NoteView::from_midi(60, 0, step, 100, 1.0, true)];
        let clip = one_note_clip(&notes);
        let intents = utter_on(
            &mut grid,
            &mut registers,
            Some(clip),
            Some(Verb::Duplicate),
            None,
            2,
        );
        assert_eq!(
            intents,
            vec![
                Intent::Clear { tick: 2 * step },
                Intent::AddNote {
                    tick: 2 * step,
                    pitch: Pitch::from_midi(60),
                    length_ticks: step,
                    velocity: 100,
                    probability: 1.0,
                },
            ]
        );
        assert_eq!(grid.cursor_step, 2);
        assert!(registers.is_empty(), "duplicate is a shorthand, not a yank");
    }

    /// Velocity reads as value on the rail: neutral throughout, floored
    /// so the softest trig stays a visible fact, accents reaching white.
    #[test]
    fn the_rail_carries_velocity_as_value() {
        let soft = velocity_ink(1);
        let mid = velocity_ink(100);
        let hard = velocity_ink(127);
        for ink in [soft, mid, hard] {
            assert!(ink.r() == ink.g() && ink.g() == ink.b());
        }
        assert!(soft.r() >= 96, "a rail is a fact before it is a level");
        assert!(soft.r() < mid.r());
        assert!(mid.r() < hard.r());
        assert_eq!(hard.r(), 255, "the accent reaches white");
    }

    /// The sign class ordering from the settled conditions ruling: certain
    /// outshines conditional outshines muted, and every voice is neutral.
    #[test]
    fn conditional_trigs_are_a_distinct_sign_class() {
        let certain = trig_ink(true, 1.0);
        let conditional = trig_ink(true, 0.75);
        let muted = trig_ink(false, 1.0);
        for ink in [certain, conditional, muted] {
            assert!(ink.r() == ink.g() && ink.g() == ink.b());
        }
        assert!(conditional.r() < certain.r());
        assert!(muted.r() < conditional.r());
    }

    #[test]
    fn a_counted_motion_travels_that_far() {
        let mut grid = SequenceGrid::default();
        utter(&mut grid, None, Some(Motion::Right), 4);
        assert_eq!(grid.cursor_step, 4);
    }

    #[test]
    fn act_speaks_the_trig_toggle() {
        let mut grid = SequenceGrid::default();
        let intents = utter(&mut grid, Some(Verb::Act), None, 1);
        assert!(matches!(
            intents.as_slice(),
            [Intent::Toggle { tick: 0, .. }]
        ));
    }

    #[test]
    fn nudge_carries_the_cursor_with_the_trig() {
        let mut grid = SequenceGrid::default();
        grid.cursor_step = 8;
        let step = grid.resolution.step_ticks();
        let intents = utter(&mut grid, Some(Verb::Nudge), Some(Motion::Right), 2);
        assert_eq!(
            intents,
            vec![Intent::Nudge {
                tick: 8 * step,
                delta_ticks: 2 * step as isize,
            }]
        );
        assert_eq!(grid.cursor_step, 10);
    }

    #[test]
    fn an_unsupported_verb_is_refused_out_loud() {
        let mut grid = SequenceGrid::default();
        let intents = utter(&mut grid, Some(Verb::Rename), None, 1);
        assert!(intents.is_empty());
        assert_eq!(grid.refusal.as_deref(), Some("RENAME: NOT HERE"));
    }

    #[test]
    fn resize_refuses_a_vertical_motion() {
        let mut grid = SequenceGrid::default();
        let intents = utter(&mut grid, Some(Verb::Resize), Some(Motion::Up), 1);
        assert!(intents.is_empty());
        assert_eq!(grid.refusal.as_deref(), Some("RESIZE: LEFT OR RIGHT"));
    }

    #[test]
    fn sixty_four_sixteenths_are_four_bars() {
        let grid = SequenceGrid::default();
        assert_eq!(
            PATTERN_STEPS * grid.resolution.step_ticks(),
            TICKS_PER_BAR * 4
        );
    }
}
