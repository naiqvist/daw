//! BRICK in the Stage chain: the hit, on the shared instrument face.
//!
//! The picture is the file the brick hits, with the START mark on it
//! and the hit's shape — attack, punch, curved fall — laid over it, so
//! what the knobs do to the file is seen against the file. The head's
//! big word is the dirt: the bit depth and the converter's clock.

use super::face::{self, Head, Layout, RowMark};
use super::palette;
#[cfg(test)]
use crate::params::brick as bp;
use crate::ui::device::brick::hit_shape;
use crate::ui::stage::chain::{self, BrickFace};
use eframe::egui;
use egui::Color32;

pub(super) use super::face::WIDTH;

fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// The file with the hit's shape over it.
fn draw_plot(
    painter: &egui::Painter,
    rect: egui::Rect,
    face: &BrickFace,
    data: Option<&crate::ui::stage::SampleData>,
) {
    let c = palette::colours();
    let inner = face::frame_plot(painter, rect);
    let small = face::font(8.5);
    let p = &face.params;
    let mid = inner.center().y;
    let half = inner.height() * 0.5 - 1.0;
    painter.line_segment(
        [
            egui::pos2(inner.left(), mid.round() - 0.5),
            egui::pos2(inner.right(), mid.round() - 0.5),
        ],
        egui::Stroke::new(1.0, c.rule),
    );
    let Some(data) = data else {
        painter.text(
            inner.center(),
            egui::Align2::CENTER_CENTER,
            if face.path.is_some() {
                "loading"
            } else {
                "no file · browse a hit onto the track"
            },
            small,
            c.dim,
        );
        return;
    };
    let columns = inner.width().floor().max(1.0) as usize;
    let bins = data.peaks.columns(None, 0.0, 1.0, columns);
    for (i, bin) in bins.iter().enumerate() {
        let x = inner.min.x + i as f32 + 0.5;
        painter.line_segment(
            [
                egui::pos2(x, mid - bin.max.clamp(-1.0, 1.0) * half),
                egui::pos2(x, mid - bin.min.clamp(-1.0, 1.0) * half),
            ],
            egui::Stroke::new(1.0, c.edge),
        );
    }
    // What plays: from START to the end, forward or back, washed.
    let start = f64::from(p.start.clamp(0.0, 1.0));
    let px = |at: f64| inner.min.x + inner.width() * at as f32;
    if p.reversed() {
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(px(1.0 - start), inner.min.y), inner.max),
            0.0,
            alpha(c.ground, 140),
        );
    } else if start > 0.0 {
        painter.rect_filled(
            egui::Rect::from_min_max(inner.min, egui::pos2(px(start), inner.max.y)),
            0.0,
            alpha(c.ground, 140),
        );
    }
    let sx = px(if p.reversed() { 1.0 - start } else { start }).round() - 0.5;
    painter.line_segment(
        [egui::pos2(sx, inner.min.y), egui::pos2(sx, inner.max.y)],
        egui::Stroke::new(1.0, c.alert),
    );
    // The hit's shape over the file's own length.
    let seconds = data.seconds().max(0.01) as f32;
    let levels = hit_shape(p.attack, p.decay, p.curve, p.punch, seconds, columns.max(2));
    let peak = levels.iter().cloned().fold(0.0f32, f32::max).max(1.0e-3);
    let points: Vec<egui::Pos2> = levels
        .iter()
        .enumerate()
        .map(|(i, level)| {
            let x = if p.reversed() {
                inner.max.x - inner.width() * i as f32 / columns.max(1) as f32
            } else {
                inner.min.x + inner.width() * i as f32 / columns.max(1) as f32
            };
            egui::pos2(
                x,
                inner.max.y - inner.height() * (level / peak).clamp(0.0, 1.0),
            )
        })
        .collect();
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, c.nominal)));
}

impl super::super::Stage {
    /// Draw the BRICK face.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_brick_chain_card(
        &self,
        ui: &mut egui::Ui,
        card: egui::Rect,
        column: &chain::Column,
        index: usize,
        cursor: Option<(usize, usize)>,
        row_offset: usize,
        rows_shown: usize,
        head_h: f32,
    ) {
        let Some(face) = column.brick.as_ref() else {
            return;
        };
        let Some(layout) = Layout::of(card, head_h, false) else {
            return;
        };
        let painter = ui.painter().clone();
        let alpha_ = self.glass();
        let selected = cursor.is_some_and(|(col, _)| col == index);
        face::draw_shell(&painter, card, layout, index, selected, &alpha_);
        let p = &face.params;
        let name = face
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "no file".to_owned());
        let head = Head {
            title: "BRICK // ONE-SHOT",
            subtitle: format!(
                "{name} · {} · {} · {}",
                if p.tracks_key() { "tracks" } else { "fixed" },
                if p.reversed() { "rev" } else { "fwd" },
                if p.cuts() { "cut" } else { "ring" }
            ),
            word: format!("{:.0}bit {:.1}k", p.bits, p.rate / 1000.0),
            plate: None,
        };
        face::draw_head(
            ui,
            layout,
            column,
            index,
            selected,
            &alpha_,
            &head,
            "stage-brick",
        );
        let data = self.sample_data.as_ref().filter(|data| {
            face.path
                .as_ref()
                .is_some_and(|path| data.path.as_path() == path.as_path())
        });
        draw_plot(&painter, layout.plot, face, data);
        face::draw_facts(
            &painter,
            layout.facts,
            &[
                ("TUNE / DROP", format!("{:+.0}st {:.0}st", p.tune, p.drop)),
                (
                    "DECAY / PUNCH",
                    format!("{:.0}ms {:.0}%", p.decay, p.punch * 100.0),
                ),
                (
                    "BODY / SNAP",
                    format!("{:.0}% {:.0}%", p.body * 100.0, p.snap * 100.0),
                ),
            ],
        );
        face::draw_params(
            &painter,
            layout.params,
            column,
            index,
            cursor,
            row_offset,
            rows_shown,
            "stage-brick-param-cursor",
            "PARAM BANK // UP/DN  LEFT/RIGHT",
            &|_| RowMark::default(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_face_carries_the_file_and_the_knobs() {
        use crate::sequencing::{Device, DeviceId};
        let mut device = Device::new(DeviceId(1), crate::devices::DeviceKind::Brick);
        device.sample = Some("/kits/kick.wav".into());
        device.set(bp::BITS, 8.0);
        let face = BrickFace::from_device(&device);
        assert_eq!(face.params.bits, 8.0);
        assert_eq!(
            face.path.as_deref(),
            Some(std::path::Path::new("/kits/kick.wav"))
        );
        assert!(face.params.cuts() && !face.params.tracks_key());
    }
}
