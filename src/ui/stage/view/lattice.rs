//! The scene lattice: one slot per (shown track, scene), stacked under
//! the heads, and the cursor on one of them.
//!
//! A slot is exactly as wide as its head — the column IS the track's
//! address — and rows touch, divided by hairline seams, so a column reads
//! as one board rather than a stack of pads. A filled slot says one
//! thing: the pattern's NUMBER, on a `panel`-toned pad. No name, no kind
//! (the head says the kind), no length. An empty slot is a centre dot: a
//! place exists here.
//!
//! Sounding is a state, not a motion: the sounding slot's pad steps up
//! to `select` and its number to `bright`, and nothing moves. The cursor
//! is four brackets and no outline — lighter than the head's chassis, at
//! cell scale. A held selection is `select` ground on its cells. Scene
//! numbers stand in the gutter in `dim`: a coordinate, not a name. The
//! master column carries one `rule` hairline down its centre, the desk's
//! edge.

use super::heads::{self, GUTTER, HEAD_W, MARGIN};
use super::{chassis, palette};
use crate::PROFONT;
use crate::sequencing::Clip;
use eframe::egui;

/// One row of the lattice.
/// @tune 16..48 px
const ROW_H: f32 = 24.0;
/// Between the heads' foot and the first row.
/// @tune 0..32 px
const HEAD_GAP: f32 = 8.0;
/// The empty slot's dot.
/// @tune 1..6 px
const DOT: f32 = 2.0;
/// The cursor's bracket arms.
/// @tune 4..16 px
const ARM: f64 = 7.0;
const INSET: f32 = 6.0;
const TYPE_PX: f32 = 12.0;

/// Where the first row begins.
pub(super) fn top(field: egui::Rect) -> f32 {
    heads::head_rect(field, 0).max.y + crate::tune!(HEAD_GAP)
}

/// How many whole rows fit beneath the heads.
pub(super) fn capacity(field: egui::Rect) -> usize {
    let room = field.max.y - crate::tune!(MARGIN) - top(field);
    (room / crate::tune!(ROW_H)).floor().max(0.0) as usize
}

/// The cell of the `slot`th shown track's `row`th shown scene.
pub(super) fn cell(field: egui::Rect, slot: usize, row: usize) -> egui::Rect {
    let head = heads::head_rect(field, slot);
    let y = top(field) + row as f32 * crate::tune!(ROW_H);
    egui::Rect::from_min_size(
        egui::pos2(head.min.x, y),
        egui::vec2(crate::tune!(HEAD_W), crate::tune!(ROW_H)),
    )
}

impl super::super::Stage {
    /// The scenes the lattice shows, as a range into the session's.
    pub(super) fn shown_scenes(&self, field: egui::Rect) -> std::ops::Range<usize> {
        let count = self.song.session.scenes.len();
        if count == 0 {
            return 0..0;
        }
        let first = self.scene_offset.min(count - 1);
        first..first.saturating_add(capacity(field)).min(count)
    }

    pub(super) fn draw_lattice(&self, painter: &egui::Painter, field: egui::Rect) {
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let address = self.session_address();
        let tracks = self.shown_tracks(field.width());
        let scenes = self.shown_scenes(field);
        let seam = egui::Stroke::new(1.0, c.rule);

        // The gutter: scene numbers, a coordinate each, and a graduated
        // rule beside them — a tick per row, a longer one every fourth,
        // so a row can be found by eye without reading.
        let gutter_x = field.min.x + crate::tune!(MARGIN) + crate::tune!(GUTTER) - 6.0;
        let rule_x =
            (field.min.x + crate::tune!(MARGIN) + crate::tune!(GUTTER) - 2.0).round() - 0.5;
        for (row, scene) in scenes.clone().enumerate() {
            let y = top(field) + (row as f32 + 0.5) * crate::tune!(ROW_H);
            painter.text(
                egui::pos2(gutter_x - 4.0, y),
                egui::Align2::RIGHT_CENTER,
                format!("{:02}", scene + 1),
                font.clone(),
                c.dim,
            );
            let tick_y = (top(field) + row as f32 * crate::tune!(ROW_H)).round() - 0.5;
            let reach = if scene % 4 == 0 { 6.0 } else { 3.0 };
            painter.line_segment(
                [
                    egui::pos2(rule_x - reach, tick_y),
                    egui::pos2(rule_x, tick_y),
                ],
                seam,
            );
        }
        if !scenes.is_empty() {
            let bottom = top(field) + scenes.len() as f32 * crate::tune!(ROW_H);
            painter.line_segment(
                [egui::pos2(rule_x, top(field)), egui::pos2(rule_x, bottom)],
                seam,
            );
        }

        for (slot, track) in tracks.clone().enumerate() {
            for (row, scene) in scenes.clone().enumerate() {
                let rect = cell(field, slot, row);
                let clip = self.song.slot_clip(track, scene);
                let sounding = self.playing.get(track).copied().flatten() == Some(scene);
                let selected = self.session_selection.cells.contains(&(track, scene));
                let pad = if selected || sounding {
                    Some(c.select)
                } else if clip.is_some() {
                    Some(c.panel)
                } else {
                    None
                };
                if let Some(fill) = pad {
                    painter.rect_filled(rect, 0.0, fill);
                }
                match clip {
                    Some(Clip::Pattern(id)) => {
                        painter.text(
                            egui::pos2(rect.min.x + INSET, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            format!("{:02}", id.0),
                            font.clone(),
                            if sounding { c.bright } else { c.fg },
                        );
                    }
                    None => {
                        let d = crate::tune!(DOT);
                        painter.rect_filled(
                            egui::Rect::from_center_size(rect.center(), egui::vec2(d, d)),
                            0.0,
                            c.rule,
                        );
                    }
                }
                // The seam under every row, the head's foot being the first.
                let y = rect.max.y.round() - 0.5;
                painter.line_segment([egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)], seam);
                if address == Some(super::super::Address::Slot { track, scene }) {
                    chassis::brackets(painter, rect, crate::tune!(ARM));
                }
            }
        }

        // The master column: the desk's edge, one hairline down its centre.
        let master = heads::master_rect(field);
        if !scenes.is_empty() {
            let x = master.center().x.round() - 0.5;
            let bottom = top(field) + scenes.len() as f32 * crate::tune!(ROW_H);
            painter.line_segment([egui::pos2(x, top(field)), egui::pos2(x, bottom)], seam);
        }
    }
}
