//! The band: the addressed track's devices, in signal order, each a card
//! carrying its whole parameter table as a scrolling list.
//!
//! What a card draws comes from the catalog through `stage::chain` — the
//! device's name and code, its lane, and every parameter as a row with a
//! name, a formatted value and a place on its range. One row offset
//! serves every column, so the rows stay level across devices. The
//! cursor's column wears the solid chassis with brackets; its row sits
//! on `select` ground with the value in `bright`. The faces — each
//! section as a vector instrument — come later and hang off the same
//! cards.

use super::heads::{GUTTER, MARGIN};
use super::{chassis, palette};
use crate::PROFONT;
use crate::ui::stage::chain;
use eframe::egui;

/// One card's width.
/// @tune 120..320 px
const CARD_W: f32 = 208.0;
/// Between two cards.
/// @tune 0..32 px
const GAP: f32 = 8.0;
/// The card's head: code, title, seal.
/// @tune 16..48 px
const HEAD_H: f32 = 30.0;
/// One parameter row.
/// @tune 12..32 px
const ROW_H: f32 = 18.0;
const LABEL_H: f32 = 18.0;
const INSET: f32 = 8.0;
const TYPE_PX: f32 = 12.0;
/// The place bar under a row's value.
const BAR_H: f32 = 2.0;

/// How many parameter rows a tray this tall shows.
pub(super) fn capacity(tray: egui::Rect) -> usize {
    let room = tray.height() - LABEL_H - crate::tune!(HEAD_H) - INSET;
    (room / crate::tune!(ROW_H)).floor().max(1.0) as usize
}

impl super::super::Stage {
    pub(super) fn draw_band(&self, painter: &egui::Painter, tray: egui::Rect) {
        let Some(lattice) = self.chain.as_ref() else {
            return;
        };
        let Some(track) = self.addressed_track() else {
            return;
        };
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let ch = TYPE_PX * 0.6;
        let left = tray.min.x + crate::tune!(MARGIN) + crate::tune!(GUTTER);
        let label_y = tray.min.y + LABEL_H * 0.5;
        let y = tray.min.y.round() - 0.5;
        painter.line_segment(
            [
                egui::pos2(left, y),
                egui::pos2(tray.max.x - crate::tune!(MARGIN), y),
            ],
            egui::Stroke::new(1.0, c.rule),
        );
        let columns = chain::band(&self.song, track);
        let mut x = left;
        for (label, value) in [
            ("band", format!("tr {:02}", track + 1)),
            ("devices", format!("{:02}", columns.len())),
        ] {
            painter.text(
                egui::pos2(x, label_y),
                egui::Align2::LEFT_CENTER,
                label,
                font.clone(),
                c.label,
            );
            x += (label.len() as f32 + 1.0) * ch;
            painter.text(
                egui::pos2(x, label_y),
                egui::Align2::LEFT_CENTER,
                &value,
                font.clone(),
                c.fg,
            );
            x += (value.len() as f32 + 2.5) * ch;
        }

        let cursor = lattice.cursor();
        let rows_shown = capacity(tray);
        let top = tray.min.y + LABEL_H;
        let bottom = tray.max.y - INSET;
        let mut x = left;
        for (col, column) in columns.iter().enumerate() {
            let card = egui::Rect::from_min_max(
                egui::pos2(x, top),
                egui::pos2(x + crate::tune!(CARD_W), bottom),
            );
            if card.max.x > tray.max.x - crate::tune!(MARGIN) {
                break;
            }
            let focused = cursor.is_some_and(|(cc, _)| cc == col);
            chassis::frame(painter, card, focused);
            let inner = card.shrink(INSET);
            // The head: code in blue, the title in body text, the lane's
            // seal on the right, and a bypassed card dimmed throughout.
            let word = if column.bypassed { c.dim } else { c.fg };
            let hy = inner.min.y + crate::tune!(HEAD_H) * 0.5 - 4.0;
            painter.text(
                egui::pos2(inner.min.x, hy),
                egui::Align2::LEFT_CENTER,
                column.code,
                font.clone(),
                c.label,
            );
            painter.text(
                egui::pos2(inner.min.x + (column.code.len() as f32 + 1.0) * ch, hy),
                egui::Align2::LEFT_CENTER,
                column.title,
                font.clone(),
                word,
            );
            if let Some(seal) = column.lane.seal(&self.song) {
                painter.text(
                    egui::pos2(inner.max.x, hy),
                    egui::Align2::RIGHT_CENTER,
                    seal,
                    font.clone(),
                    c.dim,
                );
            }
            if column.bypassed {
                painter.text(
                    egui::pos2(inner.max.x, hy),
                    egui::Align2::RIGHT_CENTER,
                    "OFF",
                    font.clone(),
                    c.alert,
                );
            }
            // The rows, from the shared offset.
            let rows_top = inner.min.y + crate::tune!(HEAD_H);
            for (i, row) in column
                .rows
                .iter()
                .enumerate()
                .skip(self.chain_offset)
                .take(rows_shown)
            {
                let ry = rows_top + (i - self.chain_offset) as f32 * crate::tune!(ROW_H);
                let rect = egui::Rect::from_min_max(
                    egui::pos2(inner.min.x, ry),
                    egui::pos2(inner.max.x, ry + crate::tune!(ROW_H)),
                );
                let on = focused && cursor.is_some_and(|(_, r)| r == i);
                if on {
                    painter.rect_filled(rect.expand2(egui::vec2(3.0, 0.0)), 0.0, c.select);
                }
                let cy = rect.center().y - 2.0;
                painter.text(
                    egui::pos2(rect.min.x, cy),
                    egui::Align2::LEFT_CENTER,
                    &row.name,
                    font.clone(),
                    if on { c.fg } else { c.dim },
                );
                painter.text(
                    egui::pos2(rect.max.x, cy),
                    egui::Align2::RIGHT_CENTER,
                    &row.value,
                    font.clone(),
                    if on { c.bright } else { word },
                );
                // The place on the range, as a bar along the row's foot.
                let track_y = rect.max.y - BAR_H - 1.0;
                let bar = egui::Rect::from_min_max(
                    egui::pos2(rect.min.x, track_y),
                    egui::pos2(rect.max.x, track_y + BAR_H),
                );
                painter.rect_filled(bar, 0.0, c.rule);
                let place = row.place.clamp(0.0, 1.0);
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        bar.min,
                        egui::pos2(bar.min.x + bar.width() * place, bar.max.y),
                    ),
                    0.0,
                    if on { c.chassis } else { c.edge },
                );
            }
            x += crate::tune!(CARD_W) + crate::tune!(GAP);
        }
    }
}
