//! sCOMP in the Stage chain.
//!
//! A bounce chain is a thing that happens over time, and a list of
//! knob values cannot show it. This wider face keeps the ordinary
//! scrolling parameter rail on its right, so keyboard editing stays
//! uniform, and gives its left to the take: every pass of it, stacked
//! on one time axis, in the forge's own acid on black — a window into
//! the room, cut in the deck. The forge itself is one explicit door
//! away: the ENTER FORGE plate, or the FORGE row turned up.

use super::{chassis, palette};
use crate::PROFONT;
use crate::design::codex::Sign;
use crate::design::kit::Weight;
use crate::params::scomp as sp;
use crate::ui::affordance::{Afford, Affords};
use crate::ui::chrome;
use crate::ui::stage::chain::{self, ScompFace};
use crate::ui::stage::forge::ScompCard;
use eframe::egui;
use egui::Color32;

/// The sampler's width: the band's two wide faces stand alike.
pub(super) const WIDTH: f32 = 478.0;

const PAD: f32 = 9.0;
const GAP: f32 = 10.0;
const PARAM_W: f32 = 188.0;
const PARAM_HEAD_H: f32 = 19.0;
const FACT_H: f32 = 30.0;
const LAB_W: f32 = 118.0;

/// The forge's inks, borrowed for the window into it.
fn acid() -> Color32 {
    Color32::from_rgb(0xB6, 0xFF, 0x1A)
}
fn hot() -> Color32 {
    Color32::from_rgb(0xFF, 0x2B, 0xD6)
}
fn violet() -> Color32 {
    Color32::from_rgb(0x8C, 0x6C, 0xFF)
}
fn black() -> Color32 {
    Color32::from_rgb(0x0A, 0x07, 0x12)
}
fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    head: egui::Rect,
    lab: egui::Rect,
    visual: egui::Rect,
    plot: egui::Rect,
    facts: egui::Rect,
    params: egui::Rect,
}

impl Layout {
    fn of(card: egui::Rect, head_h: f32) -> Option<Self> {
        if card.width() < PARAM_W + PAD * 2.0 + 80.0 || card.height() < head_h + FACT_H + 30.0 {
            return None;
        }
        let head = egui::Rect::from_min_size(card.min, egui::vec2(card.width(), head_h));
        let lab = egui::Rect::from_min_max(
            egui::pos2(head.right() - LAB_W - 7.0, head.top() + 6.0),
            egui::pos2(head.right() - 7.0, head.bottom() - 6.0),
        );
        let body = egui::Rect::from_min_max(
            egui::pos2(card.left() + PAD, head.bottom() + PAD),
            egui::pos2(card.right() - PAD, card.bottom() - PAD),
        );
        let params =
            egui::Rect::from_min_max(egui::pos2(body.right() - PARAM_W, body.top()), body.max);
        let visual =
            egui::Rect::from_min_max(body.min, egui::pos2(params.left() - GAP, body.bottom()));
        let facts = egui::Rect::from_min_max(
            egui::pos2(visual.left(), visual.bottom() - FACT_H),
            visual.max,
        );
        let plot =
            egui::Rect::from_min_max(visual.min, egui::pos2(visual.right(), facts.top() - 6.0));
        Some(Self {
            head,
            lab,
            visual,
            plot,
            facts,
            params,
        })
    }
}

fn filter_word(face: &ScompFace) -> &'static str {
    sp::FILTER_NAMES
        .get(face.params.filter.round().max(0.0) as usize)
        .copied()
        .unwrap_or("band")
}

fn draw_header(
    ui: &mut egui::Ui,
    layout: Layout,
    face: &ScompFace,
    column: &chain::Column,
    index: usize,
    selected: bool,
    alpha_: &crate::design::Alphabet,
) -> bool {
    let painter = ui.painter();
    let colours = palette::colours();
    let family_ink = if column.bypassed {
        alpha_.edge.color
    } else {
        alpha_.ink.color
    };
    let font = egui::FontId::new(10.0, egui::FontFamily::Name(PROFONT.into()));

    let mut seal = Vec::new();
    Sign::Seal(crate::ui::stage::browser::family_mark(column.family)).paint(
        &mut seal,
        egui::Rect::from_center_size(
            egui::pos2(layout.head.left() + 16.0, layout.head.center().y),
            egui::Vec2::splat(19.0),
        ),
        Weight::Hair,
        family_ink,
    );
    for shape in seal {
        painter.add(shape);
    }
    painter.text(
        egui::pos2(layout.head.left() + 31.0, layout.head.top() + 5.0),
        egui::Align2::LEFT_TOP,
        "sCOMP // BOUNCE ENGINE",
        font.clone(),
        if selected {
            colours.bright
        } else {
            colours.dir
        },
    );
    let p = &face.params;
    let passes = p.pass_count();
    painter.text(
        egui::pos2(layout.head.left() + 31.0, layout.head.bottom() - 5.0),
        egui::Align2::LEFT_BOTTOM,
        super::fit_cells(
            &format!(
                "{passes} pass{} · {} x{:.1} · {:.0}:1 · x{:.0} · {:+.0}st",
                if passes == 1 { "" } else { "es" },
                filter_word(face),
                p.harmonic,
                p.ratio,
                p.drive,
                p.shift_st
            ),
            44,
        ),
        font.clone(),
        family_ink,
    );

    // A real pointer door, not text that resembles one.
    let response = ui
        .interact(
            layout.lab,
            egui::Id::new(("stage-scomp-forge", index)),
            egui::Sense::click(),
        )
        .affords(Affords::Press);
    let open_ink = if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        painter.rect_filled(layout.lab, 0.0, alpha(hot(), 60));
        colours.bright
    } else {
        hot()
    };
    painter.rect_stroke(
        layout.lab,
        0.0,
        egui::Stroke::new(1.0, open_ink),
        egui::StrokeKind::Inside,
    );
    painter.text(
        layout.lab.center(),
        egui::Align2::CENTER_CENTER,
        "ENTER  FORGE >",
        font,
        open_ink,
    );
    response.clicked()
}

/// The window into the forge: every pass of the take, stacked on one
/// time axis, acid on black.
fn draw_plot(painter: &egui::Painter, rect: egui::Rect, card: Option<&ScompCard>) {
    let colours = palette::colours();
    painter.rect_filled(rect, 0.0, black());
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, alpha(violet(), 160)),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(4.0);
    let font = egui::FontId::new(8.5, egui::FontFamily::Name(PROFONT.into()));
    let Some(card) = card.filter(|card| !card.peaks.is_empty()) else {
        painter.text(
            inner.center(),
            egui::Align2::CENTER_CENTER,
            "rendering",
            font,
            colours.dim,
        );
        return;
    };
    let lanes = card.peaks.len();
    let longest = card.lens.iter().copied().max().unwrap_or(1).max(1) as f32;
    let gap = 2.0;
    let lane_h = ((inner.height() - gap * (lanes as f32 - 1.0)) / lanes as f32).max(4.0);
    let label_w = 24.0;
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
            font.clone(),
            if last { hot() } else { violet() },
        );
        let share = card.lens.get(k).copied().unwrap_or(0) as f32 / longest;
        let wave = egui::Rect::from_min_max(
            egui::pos2(wave_x0, y0),
            egui::pos2(wave_x0 + wave_w * share, y0 + lane_h),
        );
        let mid = wave.center().y;
        let half = wave.height() * 0.5 - 0.5;
        let columns = wave.width().floor().max(1.0) as usize;
        let bins = peaks.columns(None, 0.0, 1.0, columns);
        let ink = if last { acid() } else { alpha(acid(), 120) };
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
                egui::Stroke::new(1.0, alpha(hot(), 180)),
                egui::StrokeKind::Outside,
            );
        }
    }
}

fn draw_facts(
    painter: &egui::Painter,
    rect: egui::Rect,
    face: &ScompFace,
    card: Option<&ScompCard>,
) {
    let colours = palette::colours();
    let font = egui::FontId::new(8.5, egui::FontFamily::Name(PROFONT.into()));
    let p = &face.params;
    let seconds = card
        .and_then(|card| card.lens.last().copied())
        .map_or(0.0, |len| {
            len as f32 / crate::ui::stage::forge::CARD_RATE as f32
        });
    let facts = [
        ("TAKE", format!("{:.2}s -> {seconds:.2}s", p.take_s)),
        ("SQUASH", format!("{:+.0}dB {:.0}:1", p.thresh_db, p.ratio)),
        ("ROOT", format!("{:.0}Hz", p.root_hz())),
    ];
    let w = rect.width() / facts.len() as f32;
    for (i, (label, value)) in facts.iter().enumerate() {
        let x = rect.left() + w * i as f32;
        painter.text(
            egui::pos2(x, rect.top() + 4.0),
            egui::Align2::LEFT_TOP,
            *label,
            font.clone(),
            colours.label,
        );
        painter.text(
            egui::pos2(x, rect.bottom() - 4.0),
            egui::Align2::LEFT_BOTTOM,
            super::fit_cells(value, ((w / 5.5) as usize).max(4)),
            font.clone(),
            colours.fg,
        );
    }
}

fn draw_params(
    painter: &egui::Painter,
    rect: egui::Rect,
    column: &chain::Column,
    index: usize,
    cursor: Option<(usize, usize)>,
    row_offset: usize,
    rows_shown: usize,
) {
    let c = palette::colours();
    chassis::instrument_rail(painter, rect);
    let head = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), PARAM_HEAD_H));
    painter.text(
        egui::pos2(head.left() + 6.0, head.center().y),
        egui::Align2::LEFT_CENTER,
        "PARAM BANK // * RE-RENDERS",
        egui::FontId::new(8.5, egui::FontFamily::Name(PROFONT.into())),
        c.label,
    );
    let body = egui::Rect::from_min_max(egui::pos2(rect.left(), head.bottom()), rect.max);
    if rows_shown == 0 || body.height() <= 1.0 {
        return;
    }
    let pitch = (body.height() / rows_shown as f32).min(24.0).max(13.0);
    let font = egui::FontId::new(9.5, egui::FontFamily::Name(PROFONT.into()));
    let cell_w = painter
        .layout_no_wrap("M".to_owned(), font.clone(), c.fg)
        .rect
        .width()
        .max(1.0);
    let spec = crate::devices::DeviceKind::Scomp.spec();
    for line in 0..rows_shown {
        let Some(row) = column.rows.get(row_offset + line) else {
            break;
        };
        let row_index = row_offset + line;
        let id = spec.params.get(row_index).map(|def| def.id);
        let door = id == Some(sp::OPEN);
        let baked = id.is_some_and(sp::baked);
        let row_rect = egui::Rect::from_min_size(
            egui::pos2(body.left() + 4.0, body.top() + line as f32 * pitch),
            egui::vec2(body.width() - 8.0, pitch),
        );
        let selected = cursor == Some((index, row_index));
        if selected {
            painter.rect_filled(
                row_rect,
                0.0,
                if door { alpha(hot(), 70) } else { c.select },
            );
            crate::ui::nav_cursor::claim(
                painter,
                ("stage-scomp-param-cursor", index, row_index),
                row_rect,
                crate::ui::nav_cursor::Kind::Row,
                crate::ui::nav_cursor::Layer::Surface,
                c.alert,
            );
        }
        let ink = if selected {
            c.bright
        } else if door {
            hot()
        } else if row.edited {
            c.alert
        } else {
            c.fg
        };
        let value_cells = 9usize;
        let name_cells = ((row_rect.width() / cell_w).floor() as usize)
            .saturating_sub(value_cells + 2)
            .max(3);
        let name = if baked {
            format!("{}*", row.name)
        } else {
            row.name.clone()
        };
        painter.text(
            egui::pos2(row_rect.left() + 4.0, row_rect.center().y - 1.0),
            egui::Align2::LEFT_CENTER,
            super::fit_cells(&name, name_cells),
            font.clone(),
            ink,
        );
        painter.text(
            egui::pos2(row_rect.right() - 4.0, row_rect.center().y - 1.0),
            egui::Align2::RIGHT_CENTER,
            if door {
                "OPEN >".to_owned()
            } else {
                super::fit_cells(&row.value, value_cells)
            },
            font.clone(),
            ink,
        );
        if door {
            continue;
        }
        let rail = egui::Rect::from_min_max(
            egui::pos2(row_rect.left() + 4.0, row_rect.bottom() - 3.0),
            egui::pos2(row_rect.right() - 4.0, row_rect.bottom() - 2.0),
        );
        painter.rect_filled(rail, 0.0, c.rule);
        painter.rect_filled(
            egui::Rect::from_min_max(
                rail.min,
                egui::pos2(
                    rail.left() + rail.width() * row.place.clamp(0.0, 1.0),
                    rail.bottom(),
                ),
            ),
            0.0,
            if selected { c.bright } else { c.chassis },
        );
    }
}

impl super::super::Stage {
    /// Keep the card's picture current: render the take when its baked
    /// knobs moved, and never otherwise.
    pub(super) fn refresh_scomp_card(&mut self, face: &ScompFace) {
        let stale = self
            .scomp_cards
            .iter()
            .position(|card| card.key == face.key)
            .is_none();
        if !stale {
            return;
        }
        // One picture per baked state: an old state's picture goes.
        self.scomp_cards.retain(|card| card.key != face.key);
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
        let Some(layout) = Layout::of(card, head_h) else {
            return false;
        };
        let painter = ui.painter().clone();
        let alpha_ = self.glass();
        let selected = cursor.is_some_and(|(col, _)| col == index);
        let mut shell = Vec::new();
        chrome::panel_variant(
            &mut shell,
            card,
            Some(alpha_.surface.color),
            alpha_.ground.color,
            Some((
                if selected {
                    Weight::Heavy
                } else {
                    Weight::Hair
                },
                if selected {
                    alpha_.focus.color
                } else {
                    alpha_.edge.color
                },
            )),
            index as u8,
        );
        chrome::trace(
            &mut shell,
            &[
                egui::pos2(card.left() + chrome::CHAMFER, layout.head.bottom()),
                egui::pos2(card.right() - chrome::CHAMFER, layout.head.bottom()),
            ],
            Weight::Hair,
            alpha_.edge.color,
        );
        for point in [
            egui::pos2(card.center().x, card.top()),
            egui::pos2(card.center().x, card.bottom()),
            egui::pos2(card.left(), layout.head.bottom() - 5.0),
            egui::pos2(card.right(), layout.head.bottom() - 5.0),
        ] {
            chrome::pad(&mut shell, point, chrome::PAD, alpha_.ink.color, true);
        }
        for shape in shell {
            painter.add(shape);
        }

        let opened = draw_header(ui, layout, face, column, index, selected, &alpha_);
        let picture = self.scomp_card(face);
        draw_plot(&painter, layout.plot, picture);
        draw_facts(&painter, layout.facts, face, picture);
        draw_params(
            &painter,
            layout.params,
            column,
            index,
            cursor,
            row_offset,
            rows_shown,
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
    fn the_scomp_face_reserves_a_window_and_a_parameter_rail() {
        let card = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(WIDTH, 236.0));
        let layout = Layout::of(card, 40.0).expect("a full sCOMP card");
        assert!(layout.plot.width() > layout.params.width());
        assert!(layout.plot.height() > FACT_H);
        assert_eq!(layout.params.width(), PARAM_W);
        assert!(!layout.plot.intersects(layout.params));
        assert!(!layout.lab.intersects(layout.visual));
        assert!(
            Layout::of(
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(120.0, 60.0)),
                40.0
            )
            .is_none()
        );
    }

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
