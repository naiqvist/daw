//! The clip tray: the field's foot, where the clip under the session
//! cursor is edited.
//!
//! The sequencer is `ui::sequencer::SequencePanel`, a widget shared
//! with the older frame and driven by being shown — its grammar reads
//! the keys and its edits come back as intents, which this tray applies
//! to the song mid-frame, exactly as the last view did. It draws in the
//! design alphabet, lifted through the console's palette
//! (`palette::lift`) so the grid sits in the same room as the chassis.
//!
//! A tray with no clip under the cursor is quiet, and says so with a
//! label rather than a blank: `clip --`.

use super::heads::{GUTTER, MARGIN};
use super::palette;
use crate::PROFONT;
use crate::sequencing::{
    GRID_COLUMNS as PATTERN_COLS, PATTERN_STEP_TICKS, PATTERN_STEPS, TICKS_PER_BEAT,
};
use crate::ui::sequencer::{self, grammar, lens, midi_typing, sequence};
use crate::ui::stage::{FocusScope, StageIntent};
use eframe::egui;

/// The tray's height, cut off the foot of the field. Fixed, so the
/// session above never changes shape when a clip opens or closes.
/// @tune 120..480 px
pub(super) const TRAY_H: f32 = 272.0;
/// The widest the sequencer is drawn; past this the tray is left empty
/// rather than stretched, so the grid keeps a size the eye has learned.
/// @tune 400..1600 px
const WIDE: f32 = 1080.0;
/// The tray's own label row.
const LABEL_H: f32 = 18.0;
const TYPE_PX: f32 = 12.0;

pub(super) fn tray_h() -> f32 {
    crate::tune!(TRAY_H)
}

impl super::super::Stage {
    /// Draw the tray, and let the sequencer act on the keys it owns.
    /// Draw the tray; report where the sequencer's cursor cell is, for a
    /// callout to point at.
    pub(super) fn draw_tray(&mut self, ui: &mut egui::Ui, tray: egui::Rect) -> Option<egui::Rect> {
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let left = tray.min.x + crate::tune!(MARGIN) + crate::tune!(GUTTER);
        let label_y = tray.min.y + LABEL_H * 0.5;
        // The seam between the lattice and the tray.
        let y = tray.min.y.round() - 0.5;
        ui.painter().line_segment(
            [
                egui::pos2(left, y),
                egui::pos2(tray.max.x - crate::tune!(MARGIN), y),
            ],
            egui::Stroke::new(1.0, c.rule),
        );

        let Some(shown) = self.clip_in_view() else {
            ui.painter().text(
                egui::pos2(left, label_y),
                egui::Align2::LEFT_CENTER,
                "clip",
                font.clone(),
                c.label,
            );
            ui.painter().text(
                egui::pos2(left + TYPE_PX * 0.6 * 5.0, label_y),
                egui::Align2::LEFT_CENTER,
                "--",
                font,
                c.dim,
            );
            return None;
        };
        let Some(pattern) = self.song.pattern(shown.pattern) else {
            return None;
        };
        // The label row: which clip, how long, which lens.
        let length = sequencer::pattern_length(&self.song, shown.pattern);
        let bars = length as f32 / (TICKS_PER_BEAT * 4) as f32;
        let lens_name = match self.entry_mode(shown) {
            midi_typing::EntryMode::Degree { .. } => "degrees",
            midi_typing::EntryMode::Chromatic => "notes",
        };
        let mut x = left;
        for (label, value, tone) in [
            ("clip", format!("{:02}", shown.pattern.0), c.fg),
            ("tr", format!("{:02}", shown.track + 1), c.fg),
            ("len", format!("{bars:.0} bars"), c.fg),
            ("lens", lens_name.to_owned(), c.fg),
        ] {
            ui.painter().text(
                egui::pos2(x, label_y),
                egui::Align2::LEFT_CENTER,
                label,
                font.clone(),
                c.label,
            );
            x += (label.len() as f32 + 1.0) * TYPE_PX * 0.6;
            ui.painter().text(
                egui::pos2(x, label_y),
                egui::Align2::LEFT_CENTER,
                &value,
                font.clone(),
                tone,
            );
            x += (value.chars().count() as f32 + 2.5) * TYPE_PX * 0.6;
        }

        let lens_view = lens::LensView::resolve(lens_name, &self.song.key, &|_| None);
        let notes = sequencer::note_views(pattern, &self.song.key);
        let name = pattern.name.clone();
        let clip = sequence::ClipView {
            id: shown.pattern.0,
            name: &name,
            length_ticks: length,
            notes: &notes,
            ghosts: &[],
            slicing: self.slicing_track(shown.track),
        };
        let focused = self.inside.is_some() && self.browser.is_none();
        // While a callout is up the sequencer is seen and not heard from.
        let keys = focused && self.trig_menu.is_none() && self.plock_editor.is_none();

        let area = egui::Rect::from_min_max(
            egui::pos2(left, tray.min.y + LABEL_H),
            egui::pos2(
                (tray.max.x - crate::tune!(MARGIN)).min(left + crate::tune!(WIDE)),
                tray.max.y,
            ),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).id_salt("stage-clip"));
        // Computed BEFORE the panel borrows the sequencer: the moment is
        // a fact about the song and the transport, not about the editor.
        let playhead = self.playhead(shown);
        let outcome = self.sequencer.show(
            &mut child,
            keys,
            grammar::Voice {
                sentence: &mut self.sentence,
                registers: &mut self.registers,
            },
            self.entered_pitch.take(),
            Some(clip),
            &lens_view,
            self.polarity,
            playhead,
        );
        let anchor = outcome.cursor_rect;
        if focused {
            let edited = !outcome.intents.is_empty();
            self.apply_sequence(shown.pattern, &outcome.intents);
            if edited {
                if self.entry_held {
                    self.entry_transaction = true;
                } else {
                    // Sequencer edits are produced while drawing, outside
                    // `Stage::apply`; give each completed command the same
                    // history boundary as a stage intent.
                    self.settle();
                }
            }
            // The level under the cursor mirrors the sequencer's step, so
            // the ancestry tells the truth about where the performer is.
            if let Some(tick) = outcome.cursor_tick {
                let step = (tick / PATTERN_STEP_TICKS).min(PATTERN_STEPS - 1);
                if let FocusScope::Grid(grid) = self.focus.active_mut() {
                    grid.set_cursor(step % PATTERN_COLS, step / PATTERN_COLS);
                }
            }
        } else {
            // A veil, not a repaint: the tray keeps every mark it would
            // have, one step toward the ground. Exactly one thing on the
            // screen is focus-bright, and it is on the session.
            let g = c.ground;
            ui.painter().rect_filled(
                area,
                0.0,
                egui::Color32::from_rgba_unmultiplied(g.r(), g.g(), g.b(), 150),
            );
            if outcome.claim_focus && self.browser.is_none() {
                let _ = self.apply(StageIntent::Enter);
            }
        }
        anchor
    }
}
