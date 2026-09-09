//! The session clip matrix's window: the session small, over the song
//! view. Tracks across by their letters, scenes down by their numbers,
//! every clip its tag; the cursor an outline; the row head lit when the
//! whole scene is the subject.

use super::{Stage, palette};
use crate::PROFONT;
use crate::sequencing::Clip;
use eframe::egui;

/// @tune 0..40 px
const INSET: f32 = 8.0;
/// @tune 16..32 px
const TITLE_H: f32 = 22.0;
/// @tune 14..28 px
const ROW_H: f32 = 18.0;
/// The row heads' column.
/// @tune 40..120 px
const HEAD_W: f32 = 56.0;
/// A track column's width, at most.
/// @tune 40..160 px
const COL_W_MAX: f32 = 96.0;
const PAD: f32 = 5.0;

impl Stage {
    /// The window over the top half of the field: title, the track
    /// heads, then a row per scene as far as the panel holds, scrolled
    /// so the cursor's row is always in view.
    pub(super) fn draw_matrix_window(&self, painter: &egui::Painter, field: egui::Rect) {
        if !self.matrix.open {
            return;
        }
        let c = palette::colours();
        let inset = crate::tune!(INSET);
        let title_h = crate::tune!(TITLE_H);
        let row_h = crate::tune!(ROW_H);
        let head_w = crate::tune!(HEAD_W);
        // As tall as the scenes need, up to half the field: the lanes
        // under the window keep as much of their height as they can.
        let scenes = self.song.session.scenes.len();
        let wanted = title_h + row_h * (scenes as f32 + 1.0) + 6.0;
        let rect = egui::Rect::from_min_size(
            egui::pos2(field.left() + inset, field.top() + inset),
            egui::vec2(
                field.width() - 2.0 * inset,
                wanted
                    .min(field.height() / 2.0 - inset)
                    .max(title_h + row_h * 2.0),
            ),
        );
        painter.rect_filled(rect, 0.0, c.panel);
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, c.rule),
            egui::StrokeKind::Inside,
        );
        let title_font = egui::FontId::new(13.0, egui::FontFamily::Name(PROFONT.into()));
        let cell_font = egui::FontId::new(11.0, egui::FontFamily::Name(PROFONT.into()));
        let small = egui::FontId::new(9.0, egui::FontFamily::Name(PROFONT.into()));

        // The title: what the window is, and where a lay lands.
        let title = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), title_h));
        painter.text(
            egui::pos2(title.left() + PAD, title.center().y),
            egui::Align2::LEFT_CENTER,
            "CLIP MATRIX",
            title_font.clone(),
            c.bright,
        );
        let bar = self.arrangement.tick
            / super::super::arrangement::bar_ticks(&self.song, self.arrangement.tick).max(1)
            + 1;
        painter.text(
            egui::pos2(title.right() - PAD, title.center().y),
            egui::Align2::RIGHT_CENTER,
            format!("Enter lays at bar {bar} · +Enter lays over · S scene"),
            title_font,
            c.fg,
        );
        painter.line_segment(
            [title.left_bottom(), title.right_bottom()],
            egui::Stroke::new(1.0, c.rule),
        );

        let tracks = self.song.tracks.len().max(1);
        let col_w = ((rect.width() - head_w) / tracks as f32).min(crate::tune!(COL_W_MAX));
        let grid_top = title.bottom();
        let rows_fit = (((rect.bottom() - grid_top) / row_h).floor() as usize).saturating_sub(1);
        // Scroll so the cursor's row is in view.
        let first = if rows_fit == 0 {
            0
        } else {
            self.matrix
                .scene
                .saturating_sub(rows_fit - 1)
                .min(scenes.saturating_sub(rows_fit))
        };

        // The track heads: the letter bright, the name dim.
        for (at, track) in self.song.tracks.iter().enumerate() {
            let x = rect.left() + head_w + at as f32 * col_w;
            let head = egui::Rect::from_min_size(egui::pos2(x, grid_top), egui::vec2(col_w, row_h));
            let letter = if track.letter.is_empty() {
                "·"
            } else {
                track.letter.as_str()
            };
            painter.text(
                egui::pos2(head.left() + PAD, head.center().y),
                egui::Align2::LEFT_CENTER,
                letter,
                cell_font.clone(),
                if at == self.matrix.track && !self.matrix.on_head {
                    c.bright
                } else {
                    c.fg
                },
            );
            let name_x = head.left() + PAD + 14.0;
            if head.right() - name_x > 24.0 {
                let painter = painter.with_clip_rect(painter.clip_rect().intersect(head));
                painter.text(
                    egui::pos2(name_x, head.center().y),
                    egui::Align2::LEFT_CENTER,
                    &track.name,
                    small.clone(),
                    c.dim,
                );
            }
        }
        painter.line_segment(
            [
                egui::pos2(rect.left(), grid_top + row_h),
                egui::pos2(rect.right(), grid_top + row_h),
            ],
            egui::Stroke::new(1.0, c.edge),
        );

        // The scenes.
        for row in 0..rows_fit {
            let scene = first + row;
            if scene >= scenes {
                break;
            }
            let y = grid_top + row_h * (row as f32 + 1.0);
            let head =
                egui::Rect::from_min_size(egui::pos2(rect.left(), y), egui::vec2(head_w, row_h));
            let on_head = self.matrix.on_head && scene == self.matrix.scene;
            if on_head {
                painter.rect_filled(head.shrink(1.0), 0.0, c.select);
            }
            painter.text(
                egui::pos2(head.left() + PAD, head.center().y),
                egui::Align2::LEFT_CENTER,
                format!("sc {:02}", scene + 1),
                cell_font.clone(),
                if on_head { c.bright } else { c.dim },
            );
            if on_head {
                painter.rect_stroke(
                    head.shrink(1.0),
                    0.0,
                    egui::Stroke::new(1.5, c.bright),
                    egui::StrokeKind::Inside,
                );
            }
            for at in 0..self.song.tracks.len() {
                let x = rect.left() + head_w + at as f32 * col_w;
                let cell = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(col_w, row_h));
                let here =
                    !self.matrix.on_head && at == self.matrix.track && scene == self.matrix.scene;
                match self.song.slot_clip(at, scene) {
                    Some(Clip::Pattern(id)) => {
                        let sounding = self.playing.get(at).copied().flatten() == Some(scene);
                        painter.rect_filled(cell.shrink(1.0), 0.0, c.panel);
                        painter.text(
                            egui::pos2(cell.left() + PAD, cell.center().y),
                            egui::Align2::LEFT_CENTER,
                            self.song.tag_of(id),
                            cell_font.clone(),
                            if here || sounding { c.bright } else { c.fg },
                        );
                    }
                    None => {
                        painter.rect_filled(
                            egui::Rect::from_center_size(cell.center(), egui::vec2(2.0, 2.0)),
                            0.0,
                            c.rule,
                        );
                    }
                }
                if here {
                    painter.rect_stroke(
                        cell.shrink(1.0),
                        0.0,
                        egui::Stroke::new(1.5, c.bright),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
        if scenes > rows_fit {
            painter.text(
                egui::pos2(rect.right() - PAD, rect.bottom() - 2.0),
                egui::Align2::RIGHT_BOTTOM,
                format!(
                    "{}–{} of {scenes}",
                    first + 1,
                    (first + rows_fit).min(scenes)
                ),
                small,
                c.dim,
            );
        }
    }
}
