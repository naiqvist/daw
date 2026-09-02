//! The piano roll: the pattern's second time-detail editor.
//!
//! Same pattern, other projection — where the step grid folds 64 steps
//! into four rows and shows one primary note per cell, the roll unfolds
//! time left-to-right ONCE and gives pitch the vertical axis, so chords
//! and lines read as shapes. Both speak the same intents against the
//! same model; neither owns notes. The roll addresses lanes by the
//! bridge-era MIDI number (a chromatic-lane view by nature — degree
//! lenses stay the grid's and trig-info's job).
//!
//! Per-pitch removal needs no private intent: it speaks `Clear` then
//! re-adds the survivors, exactly the way put replaces a trig.

use crate::design::{Polarity, circuit, kit::Weight, motion::pulse_ink};
use crate::pitch::Pitch;
use crate::sequencing::PATTERN_STEPS;
use crate::ui::sequencer::grammar::{Motion, Utterance, Voice};
use crate::ui::sequencer::registers::{Payload, TrigNote};
use crate::ui::sequencer::sequence::{
    ClipView, EDITOR_SWITCH_WIDTH, Editor, Intent, NoteView, editor_switch,
};
use crate::ui::sequencer::sequence_grid::{
    TrigSelection, beat_fill, draw_cursor, next_probability, note_name, trig_at, velocity_ink,
};
use crate::ui::sequencer::verbs::Verb;
use crate::ui::sequencer::{INK_LEVEL, phase_of, shade};
use crate::ui::tokens::{font, space, stroke};
use eframe::egui;

/// The pattern's canonical step: a sixteenth.
const STEP_TICKS: usize = crate::sequencing::TICKS_PER_BEAT / 4;
const STATUS_HEIGHT: f32 = 24.0;
const LABEL_W: f32 = 48.0;
const ROW_H: f32 = 13.0;
const DEFAULT_MIDI: u8 = 60;
const DEFAULT_VELOCITY: u8 = 100;
/// Powers of two keep every zoom window on whole sixteenth-step bounds.
/// At the ceiling four steps remain visible: enough rhythmic context to
/// read a note against its neighbours while editing its exact span.
const MAX_ZOOM: usize = 16;

/// Levels above the ground, not colours: see `sequencer::shade`.
const LANE_LIGHT: u8 = 10;
const LANE_DARK: u8 = 5;
const EDGE: u8 = 48;
const LABEL_INK: u8 = 145;
const GHOST: u8 = 88;
const MUTED: u8 = 104;
/// The hairline between lanes, and the face a note is drawn on.
const LANE_RULE: u8 = 18;
/// A selected note's outline, one step above the ghost it replaces.
const SELECTED: u8 = 120;

#[derive(Clone, Copy, Debug, PartialEq)]
struct SelectedNote {
    tick: usize,
    pitch: Pitch,
}

pub(crate) struct RollPanel {
    cursor_step: usize,
    cursor_midi: u8,
    /// Horizontal magnification over the pattern. One shows all 64 steps.
    zoom: usize,
    /// First visible sixteenth. The camera follows minimally: it moves
    /// only when the cursor would otherwise leave the window.
    view_step: usize,
    /// Active clip boundary in sixteenth lanes.
    clip_steps: usize,
    /// Topmost visible lane; follows the cursor.
    top_midi: u8,
    /// A selection belongs to one clip, never to whichever clip happens
    /// to be shown next through this reusable panel.
    selection_clip: Option<u64>,
    selected_notes: Vec<SelectedNote>,
    selection_anchor: Option<(usize, u8)>,
    refusal: Option<String>,
}

impl Default for RollPanel {
    fn default() -> Self {
        Self {
            cursor_step: 0,
            cursor_midi: DEFAULT_MIDI,
            zoom: 1,
            view_step: 0,
            clip_steps: PATTERN_STEPS,
            top_midi: DEFAULT_MIDI + 7,
            selection_clip: None,
            selected_notes: Vec::new(),
            selection_anchor: None,
            refusal: None,
        }
    }
}

impl RollPanel {
    /// Read the roll's time-camera chords. Grid and roll own separate
    /// cameras, so this is called only while the roll is the active view.
    pub(crate) fn update_view(&mut self, ctx: &egui::Context) {
        let zoom_in = ctx.input_mut(|input| {
            input.consume_key(egui::Modifiers::COMMAND, egui::Key::Plus)
                || input.consume_key(egui::Modifiers::COMMAND, egui::Key::Equals)
        });
        let zoom_out =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Minus));
        let zoom_fit =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Num0));
        if zoom_fit {
            self.zoom = 1;
            self.view_step = 0;
        } else if zoom_in {
            self.rezoom((self.zoom * 2).min(MAX_ZOOM));
        } else if zoom_out {
            self.rezoom((self.zoom / 2).max(1));
        }
        self.follow_time();
    }

    fn visible_steps(&self) -> usize {
        self.clip_steps.div_ceil(self.zoom).max(1)
    }

    fn step_width(&self, width: f32) -> f32 {
        width / self.visible_steps() as f32
    }

    /// Magnify around the cursor without making the note under the hand
    /// jump to another part of the viewport.
    fn rezoom(&mut self, zoom: usize) {
        let before = self.visible_steps();
        let offset = self.cursor_step.saturating_sub(self.view_step).min(before);
        self.zoom = zoom;
        let after = self.visible_steps();
        self.view_step = self.cursor_step.saturating_sub(offset * after / before);
        self.clamp_time_view();
    }

    fn clamp_time_view(&mut self) {
        self.view_step = self.view_step.min(self.clip_steps - self.visible_steps());
    }

    fn follow_time(&mut self) {
        let visible = self.visible_steps();
        if self.cursor_step < self.view_step {
            self.view_step = self.cursor_step;
        } else if self.cursor_step + 1 > self.view_step + visible {
            self.view_step = self.cursor_step + 1 - visible;
        }
        self.clamp_time_view();
    }

    #[allow(clippy::too_many_arguments)] // Mirrors the grid's surface.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        available: egui::Rect,
        focused: bool,
        voice: &mut Voice<'_>,
        clip: Option<ClipView<'_>>,
        intents: &mut Vec<Intent>,
        ground: Polarity,
        playhead: Option<usize>,
    ) -> Option<Editor> {
        self.clip_steps = clip
            .map_or(PATTERN_STEPS, |clip| clip.length_ticks.div_ceil(STEP_TICKS))
            .clamp(1, PATTERN_STEPS);
        self.cursor_step = self.cursor_step.min(self.clip_steps - 1);
        if focused && let Some(utterance) = voice.sentence.consume(ui.ctx()) {
            self.refusal = None;
            self.speak(utterance, voice, clip, intents);
        }
        self.follow_time();
        let overlay = if voice.sentence.is_empty() {
            self.refusal.clone()
        } else {
            Some(voice.sentence.display())
        };
        let phase = phase_of(playhead);

        let rows = (((available.height() - STATUS_HEIGHT) / ROW_H).floor() as usize).max(1);
        self.follow(rows);
        let painter = ui.painter_at(available);
        self.draw_status(&painter, available, clip, overlay.as_deref(), ground);
        let requested_editor = editor_switch(
            ui,
            egui::Rect::from_min_size(
                available.min,
                egui::vec2(EDITOR_SWITCH_WIDTH, STATUS_HEIGHT),
            ),
            Editor::Roll,
            ground,
        );
        let lanes = egui::Rect::from_min_max(
            egui::pos2(available.left() + LABEL_W, available.top() + STATUS_HEIGHT),
            available.right_bottom(),
        );
        let step_w = self.step_width(lanes.width());
        painter.rect_filled(lanes, 0.0, shade(LANE_LIGHT, ground));
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(available.left(), lanes.top()),
                egui::pos2(lanes.left(), lanes.bottom()),
            ),
            0.0,
            shade(LANE_DARK, ground),
        );

        for row in 0..rows {
            let Some(midi) = self.top_midi.checked_sub(row as u8) else {
                break;
            };
            let top = lanes.top() + row as f32 * ROW_H;
            let lane = egui::Rect::from_min_max(egui::pos2(lanes.left(), top), {
                egui::pos2(lanes.right(), (top + ROW_H).min(lanes.bottom()))
            });
            // Black-key lanes recede a step: the keyboard's geography in
            // value, not colour.
            if is_black_key(midi) {
                painter.rect_filled(lane, 0.0, shade(LANE_DARK, ground));
            }
            let mut lane_shapes = Vec::new();
            circuit::trace(
                &mut lane_shapes,
                &[lane.left_bottom(), lane.right_bottom()],
                Weight::Hair,
                shade(LANE_RULE, ground),
            );
            for step in self.view_step..=self.view_step + self.visible_steps() {
                if step.is_multiple_of(16) {
                    let x = lanes.left() + (step - self.view_step) as f32 * step_w;
                    circuit::via(
                        &mut lane_shapes,
                        egui::pos2(x, lane.center().y),
                        shade(EDGE, ground),
                        if is_black_key(midi) {
                            shade(LANE_DARK, ground)
                        } else {
                            shade(LANE_LIGHT, ground)
                        },
                    );
                }
            }
            for shape in lane_shapes {
                painter.add(shape);
            }
            if midi.is_multiple_of(12) {
                painter.text(
                    egui::pos2(available.left() + LABEL_W - space::SM, lane.center().y),
                    egui::Align2::RIGHT_CENTER,
                    note_name(midi),
                    egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                    shade(LABEL_INK, ground),
                );
            }
        }

        // The time lattice and all note geometry are clipped by the
        // camera window. A long note may begin before the window or end
        // after it; clipping cuts the drawing without changing its time.
        let roll_painter = painter.with_clip_rect(lanes);
        let visible_end = self.view_step + self.visible_steps();
        for step in self.view_step..=visible_end {
            let tick = step * STEP_TICKS;
            let x = lanes.left() + (step - self.view_step) as f32 * step_w;
            let weight = if step.is_multiple_of(16) {
                Weight::Heavy
            } else {
                Weight::Hair
            };
            let mut shapes = Vec::new();
            circuit::trace(
                &mut shapes,
                &[egui::pos2(x, lanes.top()), egui::pos2(x, lanes.bottom())],
                weight,
                beat_fill(tick, ground),
            );
            for shape in shapes {
                roll_painter.add(shape);
            }
        }

        if let Some(clip) = clip {
            for note in clip
                .notes
                .iter()
                .filter(|note| note.start_ticks < clip.length_ticks)
            {
                let selected = self.is_selected(clip, note);
                self.draw_note(
                    &roll_painter,
                    lanes,
                    rows,
                    step_w,
                    note,
                    false,
                    selected,
                    ground,
                );
            }
            for ghost in clip.ghosts {
                self.draw_note(
                    &roll_painter,
                    lanes,
                    rows,
                    step_w,
                    ghost,
                    true,
                    false,
                    ground,
                );
            }
        }

        // The present moment. The roll is one unbroken timeline, so it is
        // one line — drawn OVER the notes rather than under them, because
        // a playhead hidden behind a long note disappears exactly when
        // the music is densest.
        if let Some(tick) = playhead {
            let steps = tick as f32 / STEP_TICKS as f32;
            let x = lanes.left() + (steps - self.view_step as f32) * step_w;
            if x >= lanes.left() && x <= lanes.right() {
                let alpha = crate::design::Alphabet::for_polarity(ground);
                let ink = pulse_ink(alpha.live.color, alpha.live_dim.color, phase);
                let mut shapes = Vec::new();
                circuit::trace(
                    &mut shapes,
                    &[egui::pos2(x, lanes.top()), egui::pos2(x, lanes.bottom())],
                    Weight::Heavy,
                    ink,
                );
                circuit::pad(
                    &mut shapes,
                    egui::pos2(x, lanes.top()),
                    circuit::PAD + 1.0,
                    ink,
                    true,
                );
                for shape in shapes {
                    roll_painter.add(shape);
                }
            }
        }

        if let Some(row) = self.row_of(self.cursor_midi, rows) {
            let cell = egui::Rect::from_min_size(
                egui::pos2(
                    lanes.left() + (self.cursor_step - self.view_step) as f32 * step_w,
                    lanes.top() + row as f32 * ROW_H,
                ),
                egui::vec2(step_w, ROW_H),
            );
            draw_cursor(&roll_painter, cell, ground);
        }
        requested_editor
    }

    // A painter's arguments are its geometry, and geometry does not
    // compress: every one of these is a distinct fact about where and
    // how, and bundling them into a struct would move the same list one
    // line further from where it is read.
    #[allow(clippy::too_many_arguments)]
    fn draw_note(
        &self,
        painter: &egui::Painter,
        lanes: egui::Rect,
        rows: usize,
        step_w: f32,
        note: &NoteView,
        ghost: bool,
        selected: bool,
        ground: Polarity,
    ) {
        let Some(row) = self.row_of(note.midi, rows) else {
            return;
        };
        // `start_ticks` is already the sounding onset, including the
        // note's micro-timing. Project it once through the camera: adding
        // `micro_ticks` again here would draw every push twice.
        let start_step = note.start_ticks as f32 / STEP_TICKS as f32;
        let left = lanes.left() + (start_step - self.view_step as f32) * step_w;
        let width = (note.length_ticks as f32 / STEP_TICKS as f32 * step_w - 1.0).max(2.0);
        let top = lanes.top() + row as f32 * ROW_H;
        let rect = egui::Rect::from_min_size(
            egui::pos2(left, top + 1.0),
            egui::vec2(width, (ROW_H - 2.0).max(1.0)),
        );
        if ghost {
            let mut shapes = Vec::new();
            circuit::octagon(
                &mut shapes,
                rect,
                3.0,
                None,
                Some((Weight::Hair, shade(GHOST, ground))),
            );
            for shape in shapes {
                painter.add(shape);
            }
            return;
        }
        let ink = if note.muted || !note.enabled {
            shade(MUTED, ground)
        } else if note.probability < 1.0 {
            // The conditional sign class, roll-shaped: the rule draws at
            // half voice like the grid's conditional trigs.
            shade(SELECTED, ground)
        } else {
            velocity_ink(note.velocity, ground)
        };
        let mut shapes = Vec::new();
        circuit::octagon(
            &mut shapes,
            rect,
            3.0,
            Some(ink),
            Some((Weight::Hair, shade(LANE_RULE, ground))),
        );
        circuit::pad(
            &mut shapes,
            rect.left_center(),
            circuit::PAD - 1.0,
            shade(INK_LEVEL, ground),
            true,
        );
        if selected {
            circuit::octagon(
                &mut shapes,
                rect,
                3.0,
                None,
                Some((Weight::Bold, shade(INK_LEVEL, ground))),
            );
        }
        for shape in shapes {
            painter.add(shape);
        }
        if note.approx {
            painter.text(
                rect.left_center() - egui::vec2(space::XS, 0.0),
                egui::Align2::RIGHT_CENTER,
                "\u{2248}",
                egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                shade(MUTED, ground),
            );
        }
    }

    fn draw_status(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        clip: Option<ClipView<'_>>,
        overlay: Option<&str>,
        ground: Polarity,
    ) {
        let line = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), STATUS_HEIGHT));
        painter.rect_filled(line, 0.0, shade(LANE_LIGHT, ground));
        painter.line_segment(
            [line.left_bottom(), line.right_bottom()],
            egui::Stroke::new(stroke::HAIR, shade(EDGE, ground)),
        );
        painter.text(
            line.left_center() + egui::vec2(EDITOR_SWITCH_WIDTH + space::SM, 0.0),
            egui::Align2::LEFT_CENTER,
            match clip {
                Some(clip) => format!(
                    "{}   {:02}B{}",
                    clip.name,
                    clip.length_ticks
                        .div_ceil(crate::ui::sequencer::grid_resolution::TICKS_PER_BAR),
                    zoom_sign(self.zoom)
                ),
                None => "SELECT MIDI CLIP".to_owned(),
            },
            egui::FontId::new(font::BODY, egui::FontFamily::Monospace),
            shade(LABEL_INK, ground),
        );
        if let Some(overlay) = overlay {
            painter.text(
                line.right_center() - egui::vec2(space::SM, 0.0),
                egui::Align2::RIGHT_CENTER,
                overlay,
                egui::FontId::new(font::BODY, egui::FontFamily::Monospace),
                shade(INK_LEVEL, ground),
            );
        }
    }

    fn row_of(&self, midi: u8, rows: usize) -> Option<usize> {
        let row = self.top_midi.checked_sub(midi)? as usize;
        (row < rows).then_some(row)
    }

    fn follow(&mut self, rows: usize) {
        if self.cursor_midi > self.top_midi {
            self.top_midi = self.cursor_midi;
        }
        let bottom = self.top_midi.saturating_sub(rows.saturating_sub(1) as u8);
        if self.cursor_midi < bottom {
            self.top_midi = (self.cursor_midi + rows.saturating_sub(1) as u8).min(127);
        }
    }

    pub(crate) fn cursor_tick(&self) -> usize {
        self.cursor_step * STEP_TICKS
    }

    pub(crate) fn selection(&self, clip: Option<ClipView<'_>>) -> TrigSelection {
        let tick = self.cursor_tick();
        let at_cursor = clip.and_then(|clip| {
            clip.notes
                .iter()
                .find(|note| note.start_ticks == tick && note.midi == self.cursor_midi)
                .or_else(|| clip.notes.iter().find(|note| note.start_ticks == tick))
                .copied()
        });
        TrigSelection {
            step: self.cursor_step,
            tick,
            primary: at_cursor,
            tone_count: clip.map_or(0, |clip| {
                clip.notes
                    .iter()
                    .filter(|note| note.start_ticks == tick)
                    .count()
            }),
        }
    }

    pub(crate) fn enter_pitch(
        &mut self,
        pitch: Pitch,
        key: &crate::pitch::Key,
        intents: &mut Vec<Intent>,
    ) {
        self.clear_selection();
        self.cursor_midi = crate::pitch::nearest_midi(pitch.resolve(key));
        intents.push(Intent::AddNote {
            tick: self.cursor_tick(),
            pitch,
            length_ticks: STEP_TICKS,
            velocity: DEFAULT_VELOCITY,
            probability: 1.0,
        });
    }

    fn speak(
        &mut self,
        utterance: Utterance,
        voice: &mut Voice<'_>,
        clip: Option<ClipView<'_>>,
        intents: &mut Vec<Intent>,
    ) {
        let count = utterance.count as isize;
        let tick = self.cursor_tick();
        let note_here = clip.and_then(|clip| {
            clip.notes
                .iter()
                .find(|note| note.start_ticks == tick && note.midi == self.cursor_midi)
                .copied()
        });
        match (utterance.verb, utterance.motion) {
            (None, Some(motion)) if utterance.held => {
                self.extend_selection(clip, motion, utterance.count);
            }
            (None, Some(Motion::Left)) => {
                self.selection_anchor = None;
                self.cursor_step = (self.cursor_step as isize - count)
                    .rem_euclid(self.clip_steps as isize)
                    as usize;
            }
            (None, Some(Motion::Right)) => {
                self.selection_anchor = None;
                self.cursor_step = (self.cursor_step as isize + count)
                    .rem_euclid(self.clip_steps as isize)
                    as usize;
            }
            (None, Some(Motion::Up)) => {
                self.selection_anchor = None;
                self.cursor_midi = (i16::from(self.cursor_midi) + count as i16).clamp(0, 127) as u8;
            }
            (None, Some(Motion::Down)) => {
                self.selection_anchor = None;
                self.cursor_midi = (i16::from(self.cursor_midi) - count as i16).clamp(0, 127) as u8;
            }
            (Some(Verb::Act), _) => match note_here {
                // Act on a sounding lane removes THAT note, keeping its
                // step-siblings: Clear then re-add the survivors — the
                // same replace-speech put uses.
                Some(note) => intents.push(Intent::RemoveNote {
                    tick,
                    pitch: note.pitch,
                }),
                None => intents.push(Intent::AddNote {
                    tick,
                    pitch: Pitch::from_midi(self.cursor_midi),
                    length_ticks: STEP_TICKS,
                    velocity: DEFAULT_VELOCITY,
                    probability: 1.0,
                }),
            },
            (Some(Verb::Select), _) => match (clip, note_here) {
                (Some(clip), Some(note)) => {
                    self.toggle_selected(clip, note);
                    self.refusal = Some(format!("SELECTED {} NOTES", self.selected_notes.len()));
                }
                _ => self.refusal = Some("SELECT: NOTHING HERE".to_owned()),
            },
            (Some(Verb::SelectAll), _) => match clip {
                Some(clip) if !clip.notes.is_empty() => {
                    self.set_selection(clip, clip.notes.iter().copied());
                    self.refusal = Some(format!("SELECTED {} NOTES", clip.notes.len()));
                }
                _ => self.refusal = Some("SELECT ALL: NOTHING HERE".to_owned()),
            },
            (Some(Verb::Delete), _) => {
                let addressed = self.addressed_notes(clip, note_here);
                if addressed.is_empty() {
                    self.refusal = Some("DELETE: NOTHING HERE".to_owned());
                } else {
                    intents.extend(addressed.into_iter().map(|note| Intent::RemoveNote {
                        tick: note.start_ticks,
                        pitch: note.pitch,
                    }));
                    self.clear_selection();
                }
            }
            (Some(Verb::Nudge), Some(motion @ (Motion::Left | Motion::Right))) => {
                let steps = count * if motion == Motion::Right { 1 } else { -1 };
                match note_here {
                    Some(note) => {
                        intents.push(Intent::NudgeNote {
                            tick,
                            pitch: note.pitch,
                            delta_ticks: steps * STEP_TICKS as isize,
                        });
                        self.cursor_step = (self.cursor_step as isize + steps)
                            .rem_euclid(self.clip_steps as isize)
                            as usize;
                    }
                    None => self.refusal = Some("NUDGE: NOTHING HERE".to_owned()),
                }
            }
            (Some(Verb::Nudge), Some(motion @ (Motion::Up | Motion::Down))) => {
                let delta = count * if motion == Motion::Up { 1 } else { -1 };
                match note_here {
                    Some(note) => {
                        let target = isize::from(note.midi).saturating_add(delta);
                        if !(0..=127).contains(&target) {
                            self.refusal = Some("NUDGE: PITCH EDGE".to_owned());
                        } else {
                            intents.push(Intent::TransposeNote {
                                tick,
                                pitch: note.pitch,
                                delta_semitones: delta,
                            });
                            self.cursor_midi = target as u8;
                        }
                    }
                    None => self.refusal = Some("NUDGE: NOTHING HERE".to_owned()),
                }
            }
            (Some(Verb::StackNudge), Some(motion @ (Motion::Left | Motion::Right))) => {
                let steps = count * if motion == Motion::Right { 1 } else { -1 };
                if trig_at(clip, tick, STEP_TICKS).is_some() {
                    intents.push(Intent::Nudge {
                        tick,
                        delta_ticks: steps * STEP_TICKS as isize,
                    });
                    self.cursor_step = (self.cursor_step as isize + steps)
                        .rem_euclid(self.clip_steps as isize)
                        as usize;
                } else {
                    self.refusal = Some("STACK NUDGE: NOTHING HERE".to_owned());
                }
            }
            (Some(Verb::StackNudge), Some(motion @ (Motion::Up | Motion::Down))) => {
                let delta = count * if motion == Motion::Up { 1 } else { -1 };
                let pitches: Vec<_> = clip
                    .into_iter()
                    .flat_map(|clip| clip.notes.iter())
                    .filter(|note| note.start_ticks == tick)
                    .map(|note| note.midi)
                    .collect();
                if pitches.is_empty() {
                    self.refusal = Some("STACK NUDGE: NOTHING HERE".to_owned());
                } else if pitches
                    .iter()
                    .any(|pitch| !(0..=127).contains(&isize::from(*pitch).saturating_add(delta)))
                {
                    self.refusal = Some("STACK NUDGE: PITCH EDGE".to_owned());
                } else {
                    intents.push(Intent::Transpose {
                        tick,
                        delta_semitones: delta,
                    });
                    self.cursor_midi = isize::from(self.cursor_midi)
                        .saturating_add(delta)
                        .clamp(0, 127) as u8;
                }
            }
            (Some(Verb::Resize), Some(motion @ (Motion::Left | Motion::Right))) => {
                let addressed = self.addressed_notes(clip, note_here);
                if addressed.is_empty() {
                    self.refusal = Some("RESIZE: NOTHING HERE".to_owned());
                } else {
                    let delta_ticks =
                        count * if motion == Motion::Right { 1 } else { -1 } * STEP_TICKS as isize;
                    intents.extend(addressed.into_iter().map(|note| Intent::ResizeNote {
                        tick: note.start_ticks,
                        pitch: note.pitch,
                        delta_ticks,
                    }));
                }
            }
            (Some(Verb::Resize), Some(_)) => {
                self.refusal = Some("RESIZE: LEFT OR RIGHT".to_owned());
            }
            (Some(Verb::StackResize), Some(motion @ (Motion::Left | Motion::Right))) => {
                if trig_at(clip, tick, STEP_TICKS).is_some() {
                    intents.push(Intent::Resize {
                        tick,
                        delta_ticks: count
                            * if motion == Motion::Right { 1 } else { -1 }
                            * STEP_TICKS as isize,
                    });
                } else {
                    self.refusal = Some("STACK RESIZE: NOTHING HERE".to_owned());
                }
            }
            (Some(Verb::StackResize), Some(_)) => {
                self.refusal = Some("STACK RESIZE: LEFT OR RIGHT".to_owned());
            }
            (Some(Verb::ClipResize), Some(motion @ (Motion::Left | Motion::Right))) => {
                intents.push(Intent::ResizeClip {
                    delta_ticks: count
                        * if motion == Motion::Right { 1 } else { -1 }
                        * STEP_TICKS as isize,
                });
            }
            (Some(Verb::ClipResize), Some(_)) => {
                self.refusal = Some("CLIP RESIZE: LEFT OR RIGHT".to_owned());
            }
            (Some(Verb::Velocity), Some(motion @ (Motion::Up | Motion::Down))) => {
                let addressed = self.addressed_notes(clip, note_here);
                if addressed.is_empty() {
                    self.refusal = Some("VELOCITY: NOTHING HERE".to_owned());
                } else {
                    let delta = count * if motion == Motion::Up { 1 } else { -1 };
                    intents.extend(
                        addressed
                            .into_iter()
                            .map(|note| Intent::AdjustNoteVelocity {
                                tick: note.start_ticks,
                                pitch: note.pitch,
                                delta,
                            }),
                    );
                }
            }
            (Some(Verb::Velocity), Some(_)) => {
                self.refusal = Some("VELOCITY: UP OR DOWN".to_owned());
            }
            (Some(Verb::StackVelocity), Some(motion @ (Motion::Up | Motion::Down))) => {
                if trig_at(clip, tick, STEP_TICKS).is_some() {
                    intents.push(Intent::AdjustVelocity {
                        tick,
                        delta: count * if motion == Motion::Up { 1 } else { -1 },
                    });
                } else {
                    self.refusal = Some("STACK VELOCITY: NOTHING HERE".to_owned());
                }
            }
            (Some(Verb::StackVelocity), Some(_)) => {
                self.refusal = Some("STACK VELOCITY: UP OR DOWN".to_owned());
            }
            (Some(Verb::Duplicate), _) => {
                let addressed = self.addressed_notes(clip, note_here);
                if addressed.is_empty() {
                    self.refusal = Some("DUPLICATE: NOTHING HERE".to_owned());
                } else {
                    let delta = utterance.count.max(1) * STEP_TICKS;
                    let pattern_ticks = self.clip_steps * STEP_TICKS;
                    for note in addressed {
                        let target = (note.start_ticks + delta) % pattern_ticks;
                        intents.push(Intent::AddNote {
                            tick: target,
                            pitch: note.pitch,
                            length_ticks: note.length_ticks,
                            velocity: note.velocity,
                            probability: note.probability,
                        });
                        if note.muted {
                            intents.push(Intent::SetNoteMuted {
                                tick: target,
                                pitch: note.pitch,
                                muted: true,
                            });
                        }
                    }
                    self.cursor_step =
                        (self.cursor_step + utterance.count.max(1)) % self.clip_steps;
                }
            }
            (Some(Verb::StackDuplicate), _) => match trig_at(clip, tick, STEP_TICKS) {
                Some(notes) => {
                    let steps = utterance.count.max(1);
                    let target = (self.cursor_step + steps) % self.clip_steps * STEP_TICKS;
                    for note in notes {
                        intents.push(Intent::AddNote {
                            tick: target,
                            pitch: note.pitch,
                            length_ticks: note.length_ticks,
                            velocity: note.velocity,
                            probability: note.probability,
                        });
                        if note.muted {
                            intents.push(Intent::SetNoteMuted {
                                tick: target,
                                pitch: note.pitch,
                                muted: true,
                            });
                        }
                    }
                    self.cursor_step = (self.cursor_step + steps) % self.clip_steps;
                }
                None => self.refusal = Some("STACK DUPLICATE: NOTHING HERE".to_owned()),
            },
            (Some(Verb::Mute), _) => {
                let addressed = self.addressed_notes(clip, note_here);
                if addressed.is_empty() {
                    self.refusal = Some("MUTE: NOTHING HERE".to_owned());
                } else {
                    let muted = addressed.iter().any(|note| !note.muted);
                    intents.extend(addressed.into_iter().map(|note| Intent::SetNoteMuted {
                        tick: note.start_ticks,
                        pitch: note.pitch,
                        muted,
                    }));
                }
            }
            (Some(Verb::Yank), _) => match note_here {
                Some(note) => {
                    voice.registers.yank(Payload::Note(register_note(note)));
                    self.refusal = Some("YANKED A NOTE".to_owned());
                }
                None => self.refusal = Some("YANK: NOTHING HERE".to_owned()),
            },
            (Some(Verb::StackYank), _) => match trig_at(clip, tick, STEP_TICKS) {
                Some(notes) => {
                    voice.registers.yank(Payload::Trig(notes));
                    self.refusal = Some("YANKED A TRIG".to_owned());
                }
                None => self.refusal = Some("STACK YANK: NOTHING HERE".to_owned()),
            },
            (Some(Verb::Put), _) => match voice.registers.note() {
                Ok(note) => {
                    intents.push(Intent::AddNote {
                        tick,
                        pitch: note.pitch,
                        length_ticks: note.length_ticks,
                        velocity: note.velocity,
                        probability: note.probability,
                    });
                    if note.muted {
                        intents.push(Intent::SetNoteMuted {
                            tick,
                            pitch: note.pitch,
                            muted: true,
                        });
                    }
                }
                Err(refusal) => self.refusal = Some(refusal),
            },
            (Some(Verb::StackPut), _) => match voice.registers.trig() {
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
                        if note.muted {
                            intents.push(Intent::SetNoteMuted {
                                tick,
                                pitch: note.pitch,
                                muted: true,
                            });
                        }
                    }
                }
                Err(refusal) => self.refusal = Some(refusal),
            },
            (Some(Verb::Condition), _) => match trig_at(clip, tick, STEP_TICKS) {
                Some(_) => {
                    let current = clip
                        .and_then(|clip| {
                            clip.notes
                                .iter()
                                .find(|note| note.start_ticks == tick)
                                .map(|note| note.probability)
                        })
                        .unwrap_or(1.0);
                    let probability = if utterance.count > 1 {
                        (utterance.count.min(100)) as f32 / 100.0
                    } else {
                        next_probability(current)
                    };
                    intents.push(Intent::SetProbability { tick, probability });
                }
                None => self.refusal = Some("CONDITION: NOTHING HERE".to_owned()),
            },
            (Some(verb), _) => {
                self.refusal = Some(format!("{}: NOT HERE", verb.name()));
            }
            (None, None) => {}
        }
    }

    fn clear_selection(&mut self) {
        self.selection_clip = None;
        self.selected_notes.clear();
        self.selection_anchor = None;
    }

    #[cfg(test)]
    fn all_selected(&self, clip: Option<ClipView<'_>>) -> bool {
        clip.is_some_and(|clip| {
            self.selection_clip == Some(clip.id)
                && !clip.notes.is_empty()
                && self.selected_notes.len() == clip.notes.len()
                && clip.notes.iter().all(|note| self.is_selected(clip, note))
        })
    }

    fn is_selected(&self, clip: ClipView<'_>, note: &NoteView) -> bool {
        self.selection_clip == Some(clip.id)
            && self
                .selected_notes
                .iter()
                .any(|selected| selected.tick == note.start_ticks && selected.pitch == note.pitch)
    }

    fn set_selection(&mut self, clip: ClipView<'_>, notes: impl IntoIterator<Item = NoteView>) {
        self.selection_clip = Some(clip.id);
        self.selected_notes = notes
            .into_iter()
            .map(|note| SelectedNote {
                tick: note.start_ticks,
                pitch: note.pitch,
            })
            .collect();
    }

    fn toggle_selected(&mut self, clip: ClipView<'_>, note: NoteView) {
        if self.selection_clip != Some(clip.id) {
            self.clear_selection();
            self.selection_clip = Some(clip.id);
        }
        let selected = SelectedNote {
            tick: note.start_ticks,
            pitch: note.pitch,
        };
        if let Some(index) = self
            .selected_notes
            .iter()
            .position(|item| *item == selected)
        {
            self.selected_notes.remove(index);
        } else {
            self.selected_notes.push(selected);
        }
    }

    fn addressed_notes(
        &self,
        clip: Option<ClipView<'_>>,
        fallback: Option<NoteView>,
    ) -> Vec<NoteView> {
        let selected: Vec<_> = clip
            .filter(|clip| self.selection_clip == Some(clip.id))
            .into_iter()
            .flat_map(|clip| {
                clip.notes
                    .iter()
                    .copied()
                    .filter(move |note| self.is_selected(clip, note))
            })
            .collect();
        if selected.is_empty() {
            fallback.into_iter().collect()
        } else {
            selected
        }
    }

    fn extend_selection(&mut self, clip: Option<ClipView<'_>>, motion: Motion, count: usize) {
        let anchor = *self
            .selection_anchor
            .get_or_insert((self.cursor_step, self.cursor_midi));
        match motion {
            Motion::Left => self.cursor_step = self.cursor_step.saturating_sub(count),
            Motion::Right => {
                self.cursor_step = self
                    .cursor_step
                    .saturating_add(count)
                    .min(self.clip_steps - 1)
            }
            Motion::Up => {
                self.cursor_midi = self
                    .cursor_midi
                    .saturating_add(count.min(127) as u8)
                    .min(127)
            }
            Motion::Down => {
                self.cursor_midi = self.cursor_midi.saturating_sub(count.min(127) as u8)
            }
        }
        let Some(clip) = clip else {
            self.clear_selection();
            self.refusal = Some("SELECT: NOTHING HERE".to_owned());
            return;
        };
        let (first_step, last_step) = if anchor.0 <= self.cursor_step {
            (anchor.0, self.cursor_step)
        } else {
            (self.cursor_step, anchor.0)
        };
        let (low_midi, high_midi) = if anchor.1 <= self.cursor_midi {
            (anchor.1, self.cursor_midi)
        } else {
            (self.cursor_midi, anchor.1)
        };
        let notes: Vec<_> = clip
            .notes
            .iter()
            .copied()
            .filter(|note| {
                let step = note.start_ticks / STEP_TICKS;
                (first_step..=last_step).contains(&step)
                    && (low_midi..=high_midi).contains(&note.midi)
            })
            .collect();
        self.set_selection(clip, notes);
        self.selection_anchor = Some(anchor);
        self.refusal = Some(format!("SELECTED {} NOTES", self.selected_notes.len()));
    }
}

fn register_note(note: NoteView) -> TrigNote {
    TrigNote {
        pitch: note.pitch,
        length_ticks: note.length_ticks,
        velocity: note.velocity,
        probability: note.probability,
        enabled: note.enabled,
        muted: note.muted,
    }
}

fn zoom_sign(zoom: usize) -> String {
    if zoom > 1 {
        format!("   ×{zoom}")
    } else {
        String::new()
    }
}

fn is_black_key(midi: u8) -> bool {
    matches!(midi % 12, 1 | 3 | 6 | 8 | 10)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::sequencer::registers::Registers;

    fn utter(
        roll: &mut RollPanel,
        clip: Option<ClipView<'_>>,
        verb: Option<Verb>,
        motion: Option<Motion>,
        count: usize,
    ) -> Vec<Intent> {
        let mut registers = Registers::default();
        utter_with(roll, &mut registers, clip, verb, motion, count)
    }

    fn utter_with(
        roll: &mut RollPanel,
        registers: &mut Registers,
        clip: Option<ClipView<'_>>,
        verb: Option<Verb>,
        motion: Option<Motion>,
        count: usize,
    ) -> Vec<Intent> {
        let mut sentence = crate::ui::sequencer::grammar::Sentence::default();
        let mut voice = Voice {
            sentence: &mut sentence,
            registers,
        };
        let mut intents = Vec::new();
        roll.refusal = None;
        roll.speak(
            Utterance {
                count,
                verb,
                motion,
                held: false,
            },
            &mut voice,
            clip,
            &mut intents,
        );
        intents
    }

    fn held_motion(
        roll: &mut RollPanel,
        clip: Option<ClipView<'_>>,
        motion: Motion,
        count: usize,
    ) -> Vec<Intent> {
        let mut sentence = crate::ui::sequencer::grammar::Sentence::default();
        let mut registers = Registers::default();
        let mut voice = Voice {
            sentence: &mut sentence,
            registers: &mut registers,
        };
        let mut intents = Vec::new();
        roll.refusal = None;
        roll.speak(
            Utterance {
                count,
                verb: None,
                motion: Some(motion),
                held: true,
            },
            &mut voice,
            clip,
            &mut intents,
        );
        intents
    }

    fn chord_clip(notes: &[NoteView]) -> ClipView<'_> {
        ClipView {
            id: 1,
            name: "test",
            length_ticks: 64 * 12,
            notes,
            ghosts: &[],
        }
    }

    /// Act on an empty lane adds at the cursor's pitch; act on a sounding
    /// lane removes exactly that note, and the chord's other voices are
    /// re-spoken — per-pitch editing with no private intent.
    #[test]
    fn act_adds_and_removes_per_pitch_preserving_siblings() {
        let mut roll = RollPanel::default();
        roll.cursor_midi = 64;
        let intents = utter(&mut roll, None, Some(Verb::Act), None, 1);
        assert!(matches!(
            intents.as_slice(),
            [Intent::AddNote { tick: 0, .. }]
        ));

        let notes = [
            NoteView::from_midi(60, 0, 12, 100, 1.0, true),
            NoteView::from_midi(64, 0, 12, 90, 1.0, true),
            NoteView::from_midi(67, 0, 12, 80, 1.0, true),
        ];
        let clip = chord_clip(&notes);
        let intents = utter(&mut roll, Some(clip), Some(Verb::Act), None, 1);
        assert_eq!(
            intents,
            vec![Intent::RemoveNote {
                tick: 0,
                pitch: Pitch::from_midi(64),
            }],
            "the precise remove intent leaves C and G untouched"
        );
    }

    /// Vertical motion travels pitch; the lane view follows the cursor.
    #[test]
    fn vertical_motion_travels_pitch_and_the_view_follows() {
        let mut roll = RollPanel::default();
        utter(&mut roll, None, None, Some(Motion::Up), 9);
        assert_eq!(roll.cursor_midi, 69);
        roll.follow(12);
        assert!(roll.top_midi >= 69);
        utter(&mut roll, None, None, Some(Motion::Down), 30);
        assert_eq!(roll.cursor_midi, 39);
        roll.follow(12);
        assert!(roll.top_midi.saturating_sub(11) <= 39);
    }

    /// Delete on silence refuses; the roll never clears a whole step by
    /// accident — per-pitch precision is its reason to exist.
    #[test]
    fn delete_is_per_pitch_and_refuses_on_silence() {
        let mut roll = RollPanel::default();
        roll.cursor_midi = 72;
        let notes = [NoteView::from_midi(60, 0, 12, 100, 1.0, true)];
        let clip = chord_clip(&notes);
        let intents = utter(&mut roll, Some(clip), Some(Verb::Delete), None, 1);
        assert!(intents.is_empty());
        assert_eq!(roll.refusal.as_deref(), Some("DELETE: NOTHING HERE"));
    }

    #[test]
    fn select_all_then_delete_clears_every_note_in_the_clip() {
        let notes = [
            NoteView::from_midi(60, 0, 12, 100, 1.0, true),
            NoteView::from_midi(64, 0, 12, 90, 1.0, true),
            NoteView::from_midi(67, 12, 12, 80, 1.0, true),
        ];
        let clip = chord_clip(&notes);
        let mut roll = RollPanel::default();

        assert!(utter(&mut roll, Some(clip), Some(Verb::SelectAll), None, 1).is_empty());
        assert!(roll.all_selected(Some(clip)));
        assert_eq!(roll.refusal.as_deref(), Some("SELECTED 3 NOTES"));

        assert_eq!(
            utter(&mut roll, Some(clip), Some(Verb::Delete), None, 1),
            vec![
                Intent::RemoveNote {
                    tick: 0,
                    pitch: Pitch::from_midi(60),
                },
                Intent::RemoveNote {
                    tick: 0,
                    pitch: Pitch::from_midi(64),
                },
                Intent::RemoveNote {
                    tick: 12,
                    pitch: Pitch::from_midi(67),
                },
            ]
        );
        assert!(!roll.all_selected(Some(clip)));
    }

    #[test]
    fn toggle_and_held_motion_build_a_note_selection() {
        let notes = [
            NoteView::from_midi(60, 0, 12, 100, 1.0, true),
            NoteView::from_midi(64, 0, 12, 90, 1.0, true),
            NoteView::from_midi(60, 12, 12, 80, 1.0, true),
        ];
        let clip = chord_clip(&notes);
        let mut roll = RollPanel::default();

        assert!(held_motion(&mut roll, Some(clip), Motion::Up, 4).is_empty());
        assert_eq!(roll.cursor_midi, 64);
        assert_eq!(roll.selected_notes.len(), 2, "the vertical chord range");

        utter(&mut roll, Some(clip), Some(Verb::Select), None, 1);
        assert_eq!(roll.selected_notes.len(), 1, "X toggles the E back out");
        assert!(roll.is_selected(clip, &notes[0]));
    }

    #[test]
    fn individual_or_selected_edits_and_shifted_stack_edits_are_distinct() {
        let notes = [
            NoteView::from_midi(60, 0, 12, 100, 1.0, true),
            NoteView::from_midi(64, 0, 6, 90, 1.0, true),
            NoteView::from_midi(67, 12, 12, 80, 1.0, true),
        ];
        let clip = chord_clip(&notes);
        let mut roll = RollPanel::default();
        roll.cursor_midi = 64;

        assert_eq!(
            utter(
                &mut roll,
                Some(clip),
                Some(Verb::Resize),
                Some(Motion::Right),
                2,
            ),
            vec![Intent::ResizeNote {
                tick: 0,
                pitch: Pitch::from_midi(64),
                delta_ticks: 24,
            }]
        );
        assert_eq!(
            utter(
                &mut roll,
                Some(clip),
                Some(Verb::StackResize),
                Some(Motion::Left),
                1,
            ),
            vec![Intent::Resize {
                tick: 0,
                delta_ticks: -12,
            }]
        );
        assert_eq!(
            utter(
                &mut roll,
                Some(clip),
                Some(Verb::Velocity),
                Some(Motion::Down),
                3,
            ),
            vec![Intent::AdjustNoteVelocity {
                tick: 0,
                pitch: Pitch::from_midi(64),
                delta: -3,
            }]
        );
        assert_eq!(
            utter(
                &mut roll,
                Some(clip),
                Some(Verb::StackVelocity),
                Some(Motion::Up),
                4,
            ),
            vec![Intent::AdjustVelocity { tick: 0, delta: 4 }]
        );
    }

    #[test]
    fn roll_clip_resize_uses_sixteenth_steps() {
        let mut roll = RollPanel::default();
        assert_eq!(
            utter(
                &mut roll,
                None,
                Some(Verb::ClipResize),
                Some(Motion::Right),
                2,
            ),
            vec![Intent::ResizeClip { delta_ticks: 24 }]
        );
    }

    #[test]
    fn selection_is_the_subject_of_duplicate_and_mute() {
        let notes = [
            NoteView::from_midi(60, 0, 12, 100, 1.0, true),
            NoteView::from_midi(64, 0, 6, 90, 1.0, true),
            NoteView::from_midi(67, 12, 12, 80, 1.0, true),
        ];
        let clip = chord_clip(&notes);
        let mut roll = RollPanel::default();
        utter(&mut roll, Some(clip), Some(Verb::SelectAll), None, 1);

        let muted = utter(&mut roll, Some(clip), Some(Verb::Mute), None, 1);
        assert_eq!(muted.len(), 3);
        assert!(
            muted
                .iter()
                .all(|intent| matches!(intent, Intent::SetNoteMuted { muted: true, .. }))
        );

        let duplicated = utter(&mut roll, Some(clip), Some(Verb::Duplicate), None, 2);
        assert_eq!(duplicated.len(), 3);
        assert!(
            duplicated
                .iter()
                .all(|intent| matches!(intent, Intent::AddNote { .. }))
        );
        assert_eq!(roll.cursor_step, 2);
    }

    #[test]
    fn plain_nudge_moves_one_note_and_shifted_nudge_moves_its_stack() {
        let notes = [
            NoteView::from_midi(60, 0, 12, 100, 1.0, true),
            NoteView::from_midi(64, 0, 12, 90, 1.0, true),
        ];
        let clip = chord_clip(&notes);
        let mut roll = RollPanel::default();
        roll.cursor_midi = 64;

        assert_eq!(
            utter(
                &mut roll,
                Some(clip),
                Some(Verb::Nudge),
                Some(Motion::Right),
                1,
            ),
            vec![Intent::NudgeNote {
                tick: 0,
                pitch: Pitch::from_midi(64),
                delta_ticks: STEP_TICKS as isize,
            }]
        );

        roll.cursor_step = 0;
        assert_eq!(
            utter(
                &mut roll,
                Some(clip),
                Some(Verb::StackNudge),
                Some(Motion::Right),
                1,
            ),
            vec![Intent::Nudge {
                tick: 0,
                delta_ticks: STEP_TICKS as isize,
            }]
        );
    }

    #[test]
    fn counted_vertical_nudge_transposes_one_note_or_its_stack() {
        let notes = [
            NoteView::from_midi(60, 0, 12, 100, 1.0, true),
            NoteView::from_midi(64, 0, 12, 90, 1.0, true),
        ];
        let clip = chord_clip(&notes);
        let mut roll = RollPanel::default();
        roll.cursor_midi = 64;

        assert_eq!(
            utter(
                &mut roll,
                Some(clip),
                Some(Verb::Nudge),
                Some(Motion::Up),
                12,
            ),
            vec![Intent::TransposeNote {
                tick: 0,
                pitch: Pitch::from_midi(64),
                delta_semitones: 12,
            }]
        );
        assert_eq!(roll.cursor_midi, 76);

        roll.cursor_midi = 64;
        assert_eq!(
            utter(
                &mut roll,
                Some(clip),
                Some(Verb::StackNudge),
                Some(Motion::Down),
                12,
            ),
            vec![Intent::Transpose {
                tick: 0,
                delta_semitones: -12,
            }]
        );
        assert_eq!(roll.cursor_midi, 52);
    }

    #[test]
    fn vertical_stack_nudge_refuses_atomically_at_the_pitch_edge() {
        let notes = [
            NoteView::from_midi(60, 0, 12, 100, 1.0, true),
            NoteView::from_midi(124, 0, 12, 90, 1.0, true),
        ];
        let clip = chord_clip(&notes);
        let mut roll = RollPanel::default();

        assert!(
            utter(
                &mut roll,
                Some(clip),
                Some(Verb::StackNudge),
                Some(Motion::Up),
                12,
            )
            .is_empty()
        );
        assert_eq!(roll.refusal.as_deref(), Some("STACK NUDGE: PITCH EDGE"));
    }

    #[test]
    fn plain_yank_put_carries_one_note_and_shifted_forms_carry_the_stack() {
        let notes = [
            NoteView::from_midi(60, 0, 12, 100, 1.0, true),
            NoteView::from_midi(64, 0, 6, 90, 0.75, true),
        ];
        let clip = chord_clip(&notes);
        let mut roll = RollPanel::default();
        roll.cursor_midi = 64;
        let mut registers = Registers::default();

        assert!(
            utter_with(
                &mut roll,
                &mut registers,
                Some(clip),
                Some(Verb::Yank),
                None,
                1,
            )
            .is_empty()
        );
        assert_eq!(registers.carried_sign().as_deref(), Some("N"));
        roll.cursor_step = 1;
        assert!(matches!(
            utter_with(
                &mut roll,
                &mut registers,
                Some(clip),
                Some(Verb::Put),
                None,
                1,
            )
            .as_slice(),
            [Intent::AddNote {
                tick: STEP_TICKS,
                pitch,
                ..
            }] if *pitch == Pitch::from_midi(64)
        ));

        roll.cursor_step = 0;
        let _ = utter_with(
            &mut roll,
            &mut registers,
            Some(clip),
            Some(Verb::StackYank),
            None,
            1,
        );
        assert_eq!(registers.carried_sign().as_deref(), Some("T2"));
        roll.cursor_step = 1;
        let put = utter_with(
            &mut roll,
            &mut registers,
            Some(clip),
            Some(Verb::StackPut),
            None,
            1,
        );
        assert!(matches!(
            put.first(),
            Some(Intent::Clear { tick: STEP_TICKS })
        ));
        assert_eq!(
            put.iter()
                .filter(|intent| matches!(intent, Intent::AddNote { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn time_zoom_scales_steps_and_keeps_the_cursor_anchored() {
        let mut roll = RollPanel::default();
        roll.cursor_step = 32;
        assert_eq!(roll.visible_steps(), 64);
        assert_eq!(roll.step_width(640.0), 10.0);

        roll.rezoom(4);
        assert_eq!(roll.visible_steps(), 16);
        assert_eq!(roll.step_width(640.0), 40.0);
        assert_eq!(roll.view_step, 24);
        assert_eq!(roll.cursor_step - roll.view_step, 8);
    }

    #[test]
    fn time_camera_follows_minimally_and_stays_inside_the_pattern() {
        let mut roll = RollPanel::default();
        roll.rezoom(4);
        roll.cursor_step = 63;
        roll.follow_time();
        assert_eq!(roll.view_step, 48);

        roll.cursor_step = 47;
        roll.follow_time();
        assert_eq!(roll.view_step, 47, "camera moved farther than required");

        roll.cursor_step = 0;
        roll.follow_time();
        assert_eq!(roll.view_step, 0);
    }

    #[test]
    fn zoom_has_a_quiet_fit_state_and_names_magnification() {
        assert_eq!(zoom_sign(1), "");
        assert_eq!(zoom_sign(2), "   ×2");
        assert_eq!(zoom_sign(16), "   ×16");
    }
}
