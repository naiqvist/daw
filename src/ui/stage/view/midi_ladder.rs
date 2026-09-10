//! The ladder's screen: one question, a list of answers, and the rungs
//! across the top so you can see where you are and what is left.

use super::super::midi_ladder::{Ladder, Row, RowAct, Rung};
use super::{Stage, palette};
use crate::PROFONT;
use crate::ui::affordance::{Afford, Affords};
use eframe::egui;

const PAD: f32 = 10.0;
const ROW_H: f32 = 34.0;

impl Stage {
    /// Draw the ladder over the window's body. Returns false when this
    /// window is not on the ladder, and the inspector draws instead.
    pub(super) fn draw_midi_ladder(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        rect: egui::Rect,
        ladder: Ladder,
    ) -> bool {
        let c = palette::colours();
        let font = egui::FontId::new(12.0, egui::FontFamily::Name(PROFONT.into()));
        let small = egui::FontId::new(9.0, egui::FontFamily::Name(PROFONT.into()));
        painter.rect_filled(rect, 0.0, c.ground);

        // The rungs, across the top: where you are, what is behind and
        // what is still to come.
        let mut x = rect.left() + PAD;
        for rung in Rung::ALL {
            let here = rung == ladder.rung;
            let word = rung.word();
            let width = word.len() as f32 * 7.2 + 18.0;
            let tab =
                egui::Rect::from_min_size(egui::pos2(x, rect.top() + 6.0), egui::vec2(width, 20.0));
            if here {
                painter.rect_filled(tab, 0.0, c.bright.linear_multiply(0.18));
            }
            painter.text(
                tab.center(),
                egui::Align2::CENTER_CENTER,
                word,
                font.clone(),
                if here { c.bright } else { c.dim },
            );
            x = tab.right() + 4.0;
            if rung != Rung::Listen {
                painter.text(
                    egui::pos2(x, tab.center().y),
                    egui::Align2::LEFT_CENTER,
                    "·",
                    small.clone(),
                    c.rule,
                );
                x += 10.0;
            }
        }
        painter.line_segment(
            [
                egui::pos2(rect.left(), rect.top() + 30.0),
                egui::pos2(rect.right(), rect.top() + 30.0),
            ],
            egui::Stroke::new(1.0, c.rule),
        );

        // The question this rung asks.
        painter.text(
            egui::pos2(rect.left() + PAD, rect.top() + 46.0),
            egui::Align2::LEFT_CENTER,
            ladder.rung.question(),
            font.clone(),
            c.label,
        );

        let rows = self.midi_ladder_rows();
        // A column, not a field: the value belongs beside its label, not
        // at the far edge of however wide the window happens to be.
        let measure = 680.0f32.min(rect.width());
        let body = egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.top() + 60.0),
            egui::pos2(rect.left() + measure, rect.bottom() - 26.0),
        );
        let visible = ((body.height() / ROW_H).floor().max(1.0)) as usize;
        let first = if rows.len() <= visible {
            0
        } else {
            ladder
                .at
                .saturating_sub(visible / 2)
                .min(rows.len() - visible)
        };
        // The rows are clipped to the body; the status and the key
        // line below it are NOT, which is why this does not shadow.
        let rows_painter = painter.with_clip_rect(painter.clip_rect().intersect(body));
        for (slot, (index, row)) in rows
            .iter()
            .enumerate()
            .skip(first)
            .take(visible)
            .enumerate()
            .map(|(slot, pair)| (slot, pair))
        {
            let area = egui::Rect::from_min_size(
                egui::pos2(body.left(), body.top() + slot as f32 * ROW_H),
                egui::vec2(body.width(), ROW_H),
            );
            let under = index == ladder.at;
            if under {
                rows_painter.rect_filled(area, 0.0, c.bright.linear_multiply(0.14));
                crate::ui::nav_cursor::claim(
                    &rows_painter,
                    ("midi-ladder", index),
                    area,
                    crate::ui::nav_cursor::Kind::Row,
                    crate::ui::nav_cursor::Layer::Overlay,
                    c.bright,
                );
            }
            // A row that IS the standing answer wears a mark, so the
            // list says what was chosen as well as what is possible.
            if row.chosen {
                rows_painter.text(
                    egui::pos2(area.left() + PAD, area.top() + 11.0),
                    egui::Align2::LEFT_CENTER,
                    ">",
                    font.clone(),
                    c.nominal,
                );
            }
            rows_painter.text(
                egui::pos2(area.left() + PAD + 16.0, area.top() + 11.0),
                egui::Align2::LEFT_CENTER,
                &row.label,
                font.clone(),
                if under { c.bright } else { c.fg },
            );
            rows_painter.text(
                egui::pos2(area.right() - PAD, area.top() + 11.0),
                egui::Align2::RIGHT_CENTER,
                &row.value,
                font.clone(),
                if row.chosen { c.nominal } else { c.dim },
            );
            if !row.note.is_empty() {
                rows_painter.text(
                    egui::pos2(area.left() + PAD + 16.0, area.top() + 24.0),
                    egui::Align2::LEFT_CENTER,
                    &row.note,
                    small.clone(),
                    c.dim,
                );
            }
            if ui
                .interact(
                    area,
                    egui::Id::new(("midi-ladder", index)),
                    egui::Sense::click(),
                )
                .affords(Affords::Press)
                .clicked()
            {
                self.midi_ladder_click(index);
            }
        }

        // WHAT THE LAB SAID. A ladder that takes an action and reports
        // nothing is a ladder you cannot trust: the send that refused
        // because two parts shared a pitch said so, into a status line
        // nothing drew.
        let said = self
            .midi_focus()
            .and_then(|(window, _)| self.midi_window(window))
            .map(|state| state.status.clone())
            .unwrap_or_default();
        if !said.is_empty() {
            // Beside the question, not at the foot of the window: what
            // just happened belongs where the eye already is.
            painter.text(
                egui::pos2(rect.right() - PAD, rect.top() + 46.0),
                egui::Align2::RIGHT_CENTER,
                &said,
                font.clone(),
                c.nominal,
            );
        }

        // The keys, along the bottom: what this rung answers to.
        let keys = match rows.get(ladder.at).map(|row| &row.act) {
            Some(RowAct::Tonic | RowAct::Turn(_)) => {
                "← → change · Tab next rung · Escape back · A inspector"
            }
            Some(RowAct::Part { .. }) => {
                "Enter on/off · ← → how it plays · Tab next rung · A inspector"
            }
            Some(RowAct::Hear | RowAct::Play | RowAct::Send) => {
                "Enter does it · Escape back · A inspector"
            }
            _ => "Enter chooses · Tab next rung · Escape back · A inspector",
        };
        painter.text(
            egui::pos2(rect.left() + PAD, rect.bottom() - 12.0),
            egui::Align2::LEFT_CENTER,
            keys,
            small.clone(),
            c.dim,
        );
        if rows.is_empty() {
            painter.text(
                body.center(),
                egui::Align2::CENTER_CENTER,
                match ladder.rung {
                    Rung::Clip => "no instrument track to write into — make one first",
                    _ => "nothing here yet",
                },
                font,
                c.dim,
            );
        }
        true
    }

    /// A row taken with the pointer: the cursor goes there and the row
    /// is taken, so a click is the two keys it stands for.
    fn midi_ladder_click(&mut self, index: usize) {
        if let Some((window, _)) = self.midi_focus()
            && let Some(state) = self.midi_window_mut(window)
            && let Some(ladder) = state.ladder.as_mut()
        {
            ladder.at = index;
        }
        let _ = self.ladder_enter();
    }
}
