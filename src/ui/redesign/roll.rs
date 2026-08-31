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

use crate::pitch::Pitch;
use crate::sequencing::PATTERN_STEPS;
use crate::ui::redesign::OUTLINE;
use crate::ui::redesign::grammar::{Motion, Utterance, Voice};
use crate::ui::redesign::registers::Payload;
use crate::ui::redesign::sequence::{ClipView, Intent, NoteView};
use crate::ui::redesign::sequence_grid::{
    TrigSelection, beat_fill, draw_cursor, next_probability, note_name, trig_at, velocity_ink,
};
use crate::ui::redesign::verbs::Verb;
use crate::ui::tokens::{font, space};
use eframe::egui;

/// The pattern's canonical step: a sixteenth.
const STEP_TICKS: usize = crate::sequencing::TICKS_PER_BEAT / 4;
const STATUS_HEIGHT: f32 = 24.0;
const LABEL_W: f32 = 48.0;
const ROW_H: f32 = 13.0;
const DEFAULT_MIDI: u8 = 60;
const DEFAULT_VELOCITY: u8 = 100;

const LANE_DARK: egui::Color32 = egui::Color32::from_gray(4);
const GHOST: egui::Color32 = egui::Color32::from_gray(88);
const MUTED: egui::Color32 = egui::Color32::from_gray(112);

pub(crate) struct RollPanel {
    cursor_step: usize,
    cursor_midi: u8,
    /// Topmost visible lane; follows the cursor.
    top_midi: u8,
    refusal: Option<String>,
}

impl Default for RollPanel {
    fn default() -> Self {
        Self {
            cursor_step: 0,
            cursor_midi: DEFAULT_MIDI,
            top_midi: DEFAULT_MIDI + 7,
            refusal: None,
        }
    }
}

impl RollPanel {
    #[allow(clippy::too_many_arguments)] // Mirrors the grid's surface.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        available: egui::Rect,
        focused: bool,
        voice: &mut Voice<'_>,
        clip: Option<ClipView<'_>>,
        intents: &mut Vec<Intent>,
    ) {
        if focused && let Some(utterance) = voice.sentence.consume(ui.ctx()) {
            self.refusal = None;
            self.speak(utterance, voice, clip, intents);
        }
        let overlay = if voice.sentence.is_empty() {
            self.refusal.clone()
        } else {
            Some(voice.sentence.display())
        };

        let rows = (((available.height() - STATUS_HEIGHT) / ROW_H).floor() as usize).max(1);
        self.follow(rows);
        let painter = ui.painter_at(available);
        self.draw_status(&painter, available, clip, overlay.as_deref());
        let lanes = egui::Rect::from_min_max(
            egui::pos2(available.left() + LABEL_W, available.top() + STATUS_HEIGHT),
            available.right_bottom(),
        );
        let step_w = lanes.width() / PATTERN_STEPS as f32;

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
                painter.rect_filled(lane, 0.0, LANE_DARK);
            }
            for beat in 0..(PATTERN_STEPS / 4) {
                let x = lanes.left() + (beat * 4) as f32 * step_w;
                painter.line_segment(
                    [egui::pos2(x, lane.top()), egui::pos2(x, lane.bottom())],
                    egui::Stroke::new(1.0, beat_fill((beat * 4) * STEP_TICKS)),
                );
            }
            if midi.is_multiple_of(12) {
                painter.text(
                    egui::pos2(available.left() + LABEL_W - space::SM, lane.center().y),
                    egui::Align2::RIGHT_CENTER,
                    note_name(midi),
                    egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                    MUTED,
                );
            }
        }

        if let Some(clip) = clip {
            for note in clip.notes.iter().filter(|note| note.enabled) {
                self.draw_note(&painter, lanes, rows, step_w, note, false);
            }
            for ghost in clip.ghosts {
                self.draw_note(&painter, lanes, rows, step_w, ghost, true);
            }
        }

        if let Some(row) = self.row_of(self.cursor_midi, rows) {
            let cell = egui::Rect::from_min_size(
                egui::pos2(
                    lanes.left() + self.cursor_step as f32 * step_w,
                    lanes.top() + row as f32 * ROW_H,
                ),
                egui::vec2(step_w, ROW_H),
            );
            draw_cursor(&painter, cell);
        }
    }

    fn draw_note(
        &self,
        painter: &egui::Painter,
        lanes: egui::Rect,
        rows: usize,
        step_w: f32,
        note: &NoteView,
        ghost: bool,
    ) {
        let Some(row) = self.row_of(note.midi, rows) else {
            return;
        };
        // Micro-timing displaces the onset inside its step: the push is
        // drawn, not annotated — an index, not a symbol.
        let micro = f32::from(note.micro_ticks) / STEP_TICKS as f32;
        let left =
            lanes.left() + (note.start_ticks as f32 / STEP_TICKS as f32 + micro).max(0.0) * step_w;
        let width = (note.length_ticks as f32 / STEP_TICKS as f32 * step_w - 1.0).max(2.0);
        let top = lanes.top() + row as f32 * ROW_H;
        let rect = egui::Rect::from_min_size(
            egui::pos2(left, top + 1.0),
            egui::vec2(width, (ROW_H - 2.0).max(1.0)),
        );
        if ghost {
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, GHOST),
                egui::StrokeKind::Inside,
            );
            return;
        }
        let ink = if note.probability < 1.0 {
            // The conditional sign class, roll-shaped: the rule draws at
            // half voice like the grid's conditional trigs.
            egui::Color32::from_gray(120)
        } else {
            velocity_ink(note.velocity)
        };
        painter.rect_filled(rect, 0.0, ink);
        if note.approx {
            painter.text(
                rect.left_center() - egui::vec2(space::XS, 0.0),
                egui::Align2::RIGHT_CENTER,
                "\u{2248}",
                egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                MUTED,
            );
        }
    }

    fn draw_status(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        clip: Option<ClipView<'_>>,
        overlay: Option<&str>,
    ) {
        let line = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), STATUS_HEIGHT));
        painter.text(
            line.left_center() + egui::vec2(space::SM, 0.0),
            egui::Align2::LEFT_CENTER,
            match clip {
                Some(clip) => format!("ROLL  /  {}  /  CTRL+4 GRID", clip.name),
                None => "ROLL  /  SELECT MIDI CLIP".to_owned(),
            },
            egui::FontId::new(font::BODY, egui::FontFamily::Monospace),
            OUTLINE,
        );
        if let Some(overlay) = overlay {
            painter.text(
                line.right_center() - egui::vec2(space::SM, 0.0),
                egui::Align2::RIGHT_CENTER,
                overlay,
                egui::FontId::new(font::BODY, egui::FontFamily::Monospace),
                OUTLINE,
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
            (None, Some(Motion::Left)) => {
                self.cursor_step =
                    (self.cursor_step as isize - count).rem_euclid(PATTERN_STEPS as isize) as usize;
            }
            (None, Some(Motion::Right)) => {
                self.cursor_step =
                    (self.cursor_step as isize + count).rem_euclid(PATTERN_STEPS as isize) as usize;
            }
            (None, Some(Motion::Up)) => {
                self.cursor_midi = (i16::from(self.cursor_midi) + count as i16).clamp(0, 127) as u8;
            }
            (None, Some(Motion::Down)) => {
                self.cursor_midi = (i16::from(self.cursor_midi) - count as i16).clamp(0, 127) as u8;
            }
            (Some(Verb::Act), _) => match note_here {
                // Act on a sounding lane removes THAT note, keeping its
                // step-siblings: Clear then re-add the survivors — the
                // same replace-speech put uses.
                Some(_) => self.remove_at_cursor(clip, tick, intents),
                None => intents.push(Intent::AddNote {
                    tick,
                    pitch: Pitch::from_midi(self.cursor_midi),
                    length_ticks: STEP_TICKS,
                    velocity: DEFAULT_VELOCITY,
                    probability: 1.0,
                }),
            },
            (Some(Verb::Delete), _) => match note_here {
                Some(_) => self.remove_at_cursor(clip, tick, intents),
                None => self.refusal = Some("DELETE: NOTHING HERE".to_owned()),
            },
            (Some(Verb::Nudge), Some(motion @ (Motion::Left | Motion::Right))) => {
                let steps = count * if motion == Motion::Right { 1 } else { -1 };
                intents.push(Intent::Nudge {
                    tick,
                    delta_ticks: steps * STEP_TICKS as isize,
                });
                self.cursor_step =
                    (self.cursor_step as isize + steps).rem_euclid(PATTERN_STEPS as isize) as usize;
            }
            (Some(Verb::Nudge), Some(_)) => {
                self.refusal = Some("NUDGE: LEFT OR RIGHT".to_owned());
            }
            (Some(Verb::Resize), Some(motion @ (Motion::Left | Motion::Right))) => {
                intents.push(Intent::Resize {
                    tick,
                    delta_ticks: count
                        * if motion == Motion::Right { 1 } else { -1 }
                        * STEP_TICKS as isize,
                });
            }
            (Some(Verb::Resize), Some(_)) => {
                self.refusal = Some("RESIZE: LEFT OR RIGHT".to_owned());
            }
            (Some(Verb::Yank), _) => match trig_at(clip, tick) {
                Some(notes) => {
                    voice.registers.yank(Payload::Trig(notes));
                    self.refusal = Some("YANKED A TRIG".to_owned());
                }
                None => self.refusal = Some("YANK: NOTHING HERE".to_owned()),
            },
            (Some(Verb::Put), _) => match voice.registers.trig() {
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
            (Some(Verb::Condition), _) => match trig_at(clip, tick) {
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

    /// Remove the note under the cursor's (step, lane), preserving its
    /// step-siblings through the Clear + re-add speech.
    fn remove_at_cursor(
        &mut self,
        clip: Option<ClipView<'_>>,
        tick: usize,
        intents: &mut Vec<Intent>,
    ) {
        let Some(clip) = clip else {
            return;
        };
        intents.push(Intent::Clear { tick });
        for note in clip
            .notes
            .iter()
            .filter(|note| note.start_ticks == tick && note.midi != self.cursor_midi)
        {
            intents.push(Intent::AddNote {
                tick,
                pitch: note.pitch,
                length_ticks: note.length_ticks,
                velocity: note.velocity,
                probability: note.probability,
            });
        }
    }
}

fn is_black_key(midi: u8) -> bool {
    matches!(midi % 12, 1 | 3 | 6 | 8 | 10)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::redesign::registers::Registers;

    fn utter(
        roll: &mut RollPanel,
        clip: Option<ClipView<'_>>,
        verb: Option<Verb>,
        motion: Option<Motion>,
        count: usize,
    ) -> Vec<Intent> {
        let mut registers = Registers::default();
        let mut sentence = crate::ui::redesign::grammar::Sentence::default();
        let mut voice = Voice {
            sentence: &mut sentence,
            registers: &mut registers,
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
        assert!(matches!(intents.first(), Some(Intent::Clear { tick: 0 })));
        let survivors: Vec<u8> = intents
            .iter()
            .filter_map(|intent| match intent {
                Intent::AddNote { pitch, .. } => Some(crate::pitch::nearest_midi(
                    pitch.resolve(&crate::pitch::default_key()),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(survivors, vec![60, 67], "the E leaves, C and G stay");
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
}
