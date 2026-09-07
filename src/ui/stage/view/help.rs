//! The codebook: every chord bound in the scope the keys are in, and
//! what each one means, over the field.
//!
//! A display mode, not a scope: focus never enters it, and the cursor
//! underneath is exactly where it was left. The rows come from the
//! codebook itself (`keymap::bindings_for`), the meanings from the same
//! table the palette reads, so the three can never disagree. Inside a
//! clip the counted phrases worth keeping visible are listed after.

use super::heads;
use super::{chassis, palette};
use crate::PROFONT;
use crate::ui::stage::keymap::{self, ScopeContext};
use eframe::egui;

/// One row of the codebook.
/// @tune 12..32 px
const ROW_H: f32 = 18.0;
/// A column's width.
/// @tune 200..520 px
const COL_W: f32 = 360.0;
const HEAD_H: f32 = 26.0;
const INSET: f32 = 12.0;
const TYPE_PX: f32 = 12.0;
/// Distance between the dots that bind a chord to its command.
/// @tune 3..12 px
const LEADER_STEP: f32 = 6.0;

fn leader(painter: &egui::Painter, x0: f32, x1: f32, y: f32, colour: egui::Color32) {
    let mut x = x0;
    while x <= x1 {
        painter.rect_filled(
            egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(1.0, 1.0)),
            0.0,
            colour,
        );
        x += crate::tune!(LEADER_STEP);
    }
}

impl super::super::Stage {
    pub(super) fn draw_help(&self, painter: &egui::Painter, field: egui::Rect) {
        if !self.help {
            return;
        }
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let ch = TYPE_PX * 0.6;
        let margin = heads::margin();
        let panel = egui::Rect::from_min_max(
            egui::pos2(field.min.x + margin, field.min.y + margin),
            egui::pos2(field.max.x - margin, field.max.y - margin),
        );
        painter.rect_filled(panel, 0.0, c.ground);
        chassis::frame(painter, panel, true);
        let inner = panel.shrink(INSET);

        let scope = self.scope_context();
        let entries = keymap::palette_entries();
        let mut rows: Vec<(String, String)> = keymap::bindings_for(scope)
            .map(|(mods, key, intent)| {
                let chord = super::super::carved_chord(&keymap::chord_name(mods, key));
                let meaning = entries
                    .iter()
                    .find(|e| e.scope == scope && e.intent == intent)
                    .map(|e| e.command.title.to_owned())
                    .unwrap_or_else(|| format!("{intent:?}"));
                (chord, meaning)
            })
            .collect();
        let binding_count = rows.len();
        let example_count = if scope == ScopeContext::Clip {
            super::super::CLIP_HELP_EXAMPLES.len()
        } else {
            0
        };
        if scope == ScopeContext::Clip {
            rows.extend(
                super::super::CLIP_HELP_EXAMPLES
                    .iter()
                    .map(|(chord, meaning)| {
                        (super::super::carved_chord(chord), (*meaning).to_owned())
                    }),
            );
        }

        let hy = inner.min.y + HEAD_H * 0.5 - 2.0;
        painter.text(
            egui::pos2(inner.min.x, hy),
            egui::Align2::LEFT_CENTER,
            "CODEBOOK",
            font.clone(),
            c.label,
        );
        painter.text(
            egui::pos2(inner.min.x + 10.0 * ch, hy),
            egui::Align2::LEFT_CENTER,
            format!("{scope:?}").to_ascii_uppercase(),
            font.clone(),
            c.fg,
        );
        let count = if example_count > 0 {
            format!("{binding_count:02} bindings + {example_count:02} examples")
        } else {
            format!("{binding_count:02} bindings")
        };
        painter.text(
            egui::pos2(inner.min.x + 26.0 * ch, hy),
            egui::Align2::LEFT_CENTER,
            count,
            font.clone(),
            c.dim,
        );
        painter.text(
            egui::pos2(inner.max.x, hy),
            egui::Align2::RIGHT_CENTER,
            "? closes",
            font.clone(),
            c.dim,
        );

        // The table: repeated headers keep each column independently
        // readable; a dotted leader ties the key to the action it invokes.
        let top = inner.min.y + HEAD_H;
        let row_h = crate::tune!(ROW_H);
        let rows_top = top + row_h;
        let per_col = ((inner.max.y - rows_top) / row_h).floor().max(1.0) as usize;
        let col_w = crate::tune!(COL_W);
        for (i, (chord, meaning)) in rows.iter().enumerate() {
            let col = i / per_col;
            let x = inner.min.x + col as f32 * col_w;
            if x + col_w > inner.max.x + 1.0 {
                painter.text(
                    egui::pos2(inner.max.x, inner.max.y),
                    egui::Align2::RIGHT_BOTTOM,
                    format!("{} more", rows.len() - i),
                    font,
                    c.dim,
                );
                break;
            }
            if i % per_col == 0 {
                let header_y = top + row_h * 0.5;
                painter.text(
                    egui::pos2(x, header_y),
                    egui::Align2::LEFT_CENTER,
                    "CHORD",
                    font.clone(),
                    c.label,
                );
                painter.text(
                    egui::pos2(x + 19.0 * ch, header_y),
                    egui::Align2::LEFT_CENTER,
                    "COMMAND",
                    font.clone(),
                    c.label,
                );
                let seam_y = rows_top.round() - 0.5;
                painter.line_segment(
                    [egui::pos2(x, seam_y), egui::pos2(x + col_w - INSET, seam_y)],
                    egui::Stroke::new(1.0, c.rule),
                );
            }
            let y = rows_top + (i % per_col) as f32 * row_h + row_h * 0.5;
            painter.text(
                egui::pos2(x, y),
                egui::Align2::LEFT_CENTER,
                chord,
                font.clone(),
                c.bright,
            );
            let leader_x0 = x + (chord.chars().count() as f32 + 1.0) * ch;
            let leader_x1 = x + 18.0 * ch;
            if leader_x0 <= leader_x1 {
                leader(painter, leader_x0, leader_x1, y, c.rule);
            }
            painter.text(
                egui::pos2(x + 19.0 * ch, y),
                egui::Align2::LEFT_CENTER,
                meaning,
                font.clone(),
                c.fg,
            );
        }
    }
}
