//! sCOMP in the Stage chain: a window into the forge on the shared
//! instrument face.
//!
//! A bounce chain is a thing that happens over time, and a list of
//! knob values cannot show it. The picture is the take: every pass of
//! it, stacked on one time axis, rendered once per change of the baked
//! knobs and cached on the stage at half rate. Rows that bake into the
//! take wear a star; the FORGE row is a door, as is the plate.

use super::face::{self, Head, Layout, RowMark};
use super::palette;
use crate::params::scomp as sp;
use crate::ui::affordance::{Afford, Affords};
use crate::ui::stage::chain::{self, ScompFace};
use crate::ui::stage::forge::ScompCard;
use eframe::egui;
use egui::Color32;

pub(super) use super::face::WIDTH;

fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn filter_word(face: &ScompFace) -> &'static str {
    sp::FILTER_NAMES
        .get(face.params.filter.round().max(0.0) as usize)
        .copied()
        .unwrap_or("band")
}

/// The window into the forge: every pass of the take, stacked on one
/// time axis.
fn draw_plot(painter: &egui::Painter, rect: egui::Rect, card: Option<&ScompCard>) {
    let c = palette::colours();
    let inner = face::frame_plot(painter, rect);
    let small = face::font(8.5);
    let Some(card) = card.filter(|card| !card.peaks.is_empty()) else {
        painter.text(
            inner.center(),
            egui::Align2::CENTER_CENTER,
            "rendering",
            small,
            c.dim,
        );
        return;
    };
    let lanes = card.peaks.len();
    let longest = card.lens.iter().copied().max().unwrap_or(1).max(1) as f32;
    let gap = 3.0;
    let lane_h = ((inner.height() - gap * (lanes as f32 - 1.0)) / lanes as f32).max(4.0);
    let label_w = 22.0;
    let wave_x0 = inner.min.x + label_w;
    let wave_w = (inner.max.x - wave_x0).max(1.0);
    for (k, peaks) in card.peaks.iter().enumerate() {
        let y0 = inner.min.y + k as f32 * (lane_h + gap);
        let last = k + 1 == lanes;
        painter.text(
            egui::pos2(inner.min.x, y0 + lane_h * 0.5),
            egui::Align2::LEFT_CENTER,
            if k == 0 {
                "~".to_owned()
            } else {
                format!("P{k}")
            },
            small.clone(),
            if last { c.bright } else { c.label },
        );
        let share = card.lens.get(k).copied().unwrap_or(0) as f32 / longest;
        let wave = egui::Rect::from_min_max(
            egui::pos2(wave_x0, y0),
            egui::pos2(wave_x0 + wave_w * share, y0 + lane_h),
        );
        let mid = wave.center().y;
        let half = wave.height() * 0.5 - 0.5;
        painter.line_segment(
            [
                egui::pos2(wave_x0, mid.round() - 0.5),
                egui::pos2(inner.max.x, mid.round() - 0.5),
            ],
            egui::Stroke::new(1.0, c.rule),
        );
        let columns = wave.width().floor().max(1.0) as usize;
        let bins = peaks.columns(None, 0.0, 1.0, columns);
        let ink = if last { c.edge } else { alpha(c.edge, 140) };
        for (i, bin) in bins.iter().enumerate() {
            let x = wave.min.x + i as f32 + 0.5;
            painter.line_segment(
                [
                    egui::pos2(x, mid - bin.max.clamp(-1.0, 1.0) * half),
                    egui::pos2(x, mid - bin.min.clamp(-1.0, 1.0) * half),
                ],
                egui::Stroke::new(1.0, ink),
            );
        }
        if last {
            painter.rect_stroke(
                egui::Rect::from_min_max(
                    egui::pos2(wave_x0 - 1.0, y0 - 1.0),
                    egui::pos2(inner.max.x + 1.0, y0 + lane_h + 1.0),
                ),
                0.0,
                egui::Stroke::new(1.0, c.alert),
                egui::StrokeKind::Outside,
            );
        }
    }
}

impl super::super::Stage {
    /// Keep the card's picture current: render the take when its baked
    /// knobs moved, and never otherwise.
    pub(super) fn refresh_scomp_card(&mut self, face: &ScompFace) {
        if self.scomp_cards.iter().any(|card| card.key == face.key) {
            return;
        }
        if self.scomp_cards.len() >= 8 {
            self.scomp_cards.remove(0);
        }
        self.scomp_cards.push(ScompCard::render(&face.params));
    }

    fn scomp_card(&self, face: &ScompFace) -> Option<&ScompCard> {
        self.scomp_cards.iter().find(|card| card.key == face.key)
    }

    /// Draw the sCOMP face. Returns true when the pointer asked to enter
    /// the forge; keyboard Enter continues through the normal map.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_scomp_chain_card(
        &self,
        ui: &mut egui::Ui,
        card: egui::Rect,
        column: &chain::Column,
        index: usize,
        cursor: Option<(usize, usize)>,
        row_offset: usize,
        rows_shown: usize,
        head_h: f32,
    ) -> bool {
        let Some(face) = column.scomp.as_ref() else {
            return false;
        };
        let Some(layout) = Layout::of(card, head_h, true) else {
            return false;
        };
        let painter = ui.painter().clone();
        let alpha_ = self.glass();
        let selected = cursor.is_some_and(|(col, _)| col == index);
        face::draw_shell(&painter, card, layout, index, selected, &alpha_);
        let p = &face.params;
        let passes = p.pass_count();
        let head = Head {
            title: "sCOMP // BOUNCE ENGINE",
            subtitle: format!(
                "{} x{:.1} · {:.0}:1 · x{:.0} · {:+.0}st",
                filter_word(face),
                p.harmonic,
                p.ratio,
                p.drive,
                p.shift_st
            ),
            word: format!("{passes} PASS{}", if passes == 1 { "" } else { "ES" }),
            plate: Some("ENTER  FORGE >"),
        };
        let opened = face::draw_head(
            ui,
            layout,
            column,
            index,
            selected,
            &alpha_,
            &head,
            "stage-scomp-forge",
        );
        let picture = self.scomp_card(face);
        draw_plot(&painter, layout.plot, picture);
        let seconds = picture
            .and_then(|card| card.lens.last().copied())
            .map_or(0.0, |len| {
                len as f32 / crate::ui::stage::forge::CARD_RATE as f32
            });
        face::draw_facts(
            &painter,
            layout.facts,
            &[
                ("TAKE", format!("{:.2}s -> {seconds:.2}s", p.take_s)),
                ("SQUASH", format!("{:+.0}dB {:.0}:1", p.thresh_db, p.ratio)),
                ("ROOT", format!("{:.0}Hz", p.root_hz())),
            ],
        );
        let spec = crate::devices::DeviceKind::Scomp.spec();
        face::draw_params(
            &painter,
            layout.params,
            column,
            index,
            cursor,
            row_offset,
            rows_shown,
            "stage-scomp-param-cursor",
            "PARAM BANK // * RE-RENDERS",
            &|row| {
                let id = spec.params.get(row).map(|def| def.id);
                RowMark {
                    star: id.is_some_and(sp::baked),
                    door: id == Some(sp::OPEN),
                }
            },
        );
        let window = ui
            .interact(
                layout.plot,
                egui::Id::new(("stage-scomp-window", index)),
                egui::Sense::click(),
            )
            .affords(Affords::Press);
        if window.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ZoomIn);
        }
        opened || window.double_clicked()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_picture_holds_every_pass_at_the_card_rate() {
        let params = crate::scomp::ScompParams {
            take_s: 0.25,
            ..Default::default()
        };
        let card = ScompCard::render(&params);
        assert_eq!(card.peaks.len(), 4);
        assert_eq!(card.lens.len(), 4);
        assert_eq!(
            card.lens[0],
            (0.25 * crate::ui::stage::forge::CARD_RATE as f32) as usize
        );
        assert_eq!(card.key, params.baked());
    }
}
