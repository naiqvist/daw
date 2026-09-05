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
        painter.text(
            egui::pos2(inner.max.x, hy),
            egui::Align2::RIGHT_CENTER,
            "? closes",
            font.clone(),
            c.dim,
        );

        // The rows: chord, meaning.
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
        if scope == ScopeContext::Clip {
            rows.extend(
                super::super::CLIP_HELP_EXAMPLES
                    .iter()
                    .map(|(chord, meaning)| {
                        (super::super::carved_chord(chord), (*meaning).to_owned())
                    }),
            );
        }
        let top = inner.min.y + HEAD_H;
        let row_h = crate::tune!(ROW_H);
        let per_col = ((inner.max.y - top) / row_h).floor().max(1.0) as usize;
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
            let y = top + (i % per_col) as f32 * row_h + row_h * 0.5;
            painter.text(
                egui::pos2(x, y),
                egui::Align2::LEFT_CENTER,
                chord,
                font.clone(),
                c.bright,
            );
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
