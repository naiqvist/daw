//! The wide instrument face: one grid for every instrument card in the
//! band that has more to show than a table.
//!
//! The sampler, sCOMP, STAB and QUAD each put a picture on the left and
//! the scrolling parameter rail on the right. This is the grid they
//! share, so the four stand as one family: the same head with the seal,
//! the title and its subtitle, the instrument's one big word on the
//! right and, where the instrument has a room, the plate that opens it;
//! the same picture frame; the same three-column facts row under it;
//! the same rail. A card supplies its picture and its words and nothing
//! about where they go.

use super::{chassis, palette};
use crate::PROFONT;
use crate::design::codex::Sign;
use crate::design::kit::Weight;
use crate::ui::affordance::{Afford, Affords};
use crate::ui::chrome;
use crate::ui::stage::chain;
use eframe::egui;

/// Wide enough for a real picture and an uncompromised parameter rail.
pub(super) const WIDTH: f32 = 478.0;

const PAD: f32 = 9.0;
const GAP: f32 = 10.0;
const PARAM_W: f32 = 188.0;
const PARAM_HEAD_H: f32 = 19.0;
const FACT_H: f32 = 30.0;
const LAB_W: f32 = 118.0;
const SEAL_X: f32 = 16.0;
const TEXT_X: f32 = 31.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Layout {
    pub head: egui::Rect,
    /// The plate that opens the instrument's room, when it has one.
    pub lab: Option<egui::Rect>,
    pub plot: egui::Rect,
    pub facts: egui::Rect,
    pub params: egui::Rect,
}

impl Layout {
    pub(super) fn of(card: egui::Rect, head_h: f32, plate: bool) -> Option<Self> {
        if card.width() < PARAM_W + PAD * 2.0 + 80.0 || card.height() < head_h + FACT_H + 30.0 {
            return None;
        }
        let head = egui::Rect::from_min_size(card.min, egui::vec2(card.width(), head_h));
        let lab = plate.then(|| {
            egui::Rect::from_min_max(
                egui::pos2(head.right() - LAB_W - 7.0, head.top() + 6.0),
                egui::pos2(head.right() - 7.0, head.bottom() - 6.0),
            )
        });
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
            plot,
            facts,
            params,
        })
    }
}

/// What the head says.
pub(super) struct Head {
    pub title: &'static str,
    pub subtitle: String,
    /// The instrument's one big word, right of centre: the chord, the
    /// routing, the pass count.
    pub word: String,
    /// The plate's text, when the instrument has a room.
    pub plate: Option<&'static str>,
}

/// How a rail row is marked beyond its name and value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct RowMark {
    /// The row bakes into something: a star after its name.
    pub star: bool,
    /// The row is a door: it reads OPEN rather than a value.
    pub door: bool,
}

pub(super) fn font(px: f32) -> egui::FontId {
    egui::FontId::new(px, egui::FontFamily::Name(PROFONT.into()))
}

/// The card's shell: the panel, its seam under the head, its pads.
pub(super) fn draw_shell(
    painter: &egui::Painter,
    card: egui::Rect,
    layout: Layout,
    index: usize,
    selected: bool,
    alpha: &crate::design::Alphabet,
) {
    let mut shell = Vec::new();
    chrome::panel_variant(
        &mut shell,
        card,
        Some(alpha.surface.color),
        alpha.ground.color,
        Some((
            if selected {
                Weight::Heavy
            } else {
                Weight::Hair
            },
            if selected {
                alpha.focus.color
            } else {
                alpha.edge.color
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
        alpha.edge.color,
    );
    for point in [
        egui::pos2(card.center().x, card.top()),
        egui::pos2(card.center().x, card.bottom()),
        egui::pos2(card.left(), layout.head.bottom() - 5.0),
        egui::pos2(card.right(), layout.head.bottom() - 5.0),
    ] {
        chrome::pad(&mut shell, point, chrome::PAD, alpha.ink.color, true);
    }
    for shape in shell {
        painter.add(shape);
    }
}

/// The head. Returns true when the plate was pressed.
pub(super) fn draw_head(
    ui: &mut egui::Ui,
    layout: Layout,
    column: &chain::Column,
    index: usize,
    selected: bool,
    alpha: &crate::design::Alphabet,
    head: &Head,
    tag: &'static str,
) -> bool {
    let painter = ui.painter();
    let c = palette::colours();
    let family_ink = if column.bypassed {
        alpha.edge.color
    } else {
        alpha.ink.color
    };
    let small = font(10.0);
    let ch = 6.0;
    let mut seal = Vec::new();
    Sign::Seal(crate::ui::stage::browser::family_mark(column.family)).paint(
        &mut seal,
        egui::Rect::from_center_size(
            egui::pos2(layout.head.left() + SEAL_X, layout.head.center().y),
            egui::Vec2::splat(19.0),
        ),
        Weight::Hair,
        family_ink,
    );
    for shape in seal {
        painter.add(shape);
    }
    // The word and the plate take the right; the words take what is left.
    let right_edge = layout
        .lab
        .map_or(layout.head.right() - 10.0, |lab| lab.left() - 10.0);
    let word_w = head.word.chars().count() as f32 * 7.8 + 8.0;
    let text_right = (right_edge - word_w).max(layout.head.left() + TEXT_X + 40.0);
    let cells = ((text_right - layout.head.left() - TEXT_X) / ch)
        .floor()
        .max(8.0) as usize;
    painter.text(
        egui::pos2(layout.head.left() + TEXT_X, layout.head.top() + 5.0),
        egui::Align2::LEFT_TOP,
        super::fit_cells(head.title, cells),
        small.clone(),
        if selected { c.bright } else { c.dir },
    );
    painter.text(
        egui::pos2(layout.head.left() + TEXT_X, layout.head.bottom() - 5.0),
        egui::Align2::LEFT_BOTTOM,
        super::fit_cells(&head.subtitle, cells),
        small.clone(),
        family_ink,
    );
    painter.text(
        egui::pos2(right_edge, layout.head.center().y),
        egui::Align2::RIGHT_CENTER,
        &head.word,
        font(13.0),
        c.nominal,
    );
    let (Some(lab), Some(plate)) = (layout.lab, head.plate) else {
        return false;
    };
    let response = ui
        .interact(lab, egui::Id::new((tag, index)), egui::Sense::click())
        .affords(Affords::Press);
    let open_ink = if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        painter.rect_filled(lab, 0.0, c.select);
        c.bright
    } else {
        c.alert
    };
    painter.rect_stroke(
        lab,
        0.0,
        egui::Stroke::new(1.0, open_ink),
        egui::StrokeKind::Inside,
    );
    painter.text(
        lab.center(),
        egui::Align2::CENTER_CENTER,
        plate,
        small,
        open_ink,
    );
    response.clicked()
}

/// The picture's frame: a panel with a rule around it, and the inset
/// the picture is drawn in.
pub(super) fn frame_plot(painter: &egui::Painter, rect: egui::Rect) -> egui::Rect {
    let c = palette::colours();
    painter.rect_filled(rect, 0.0, c.panel);
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, c.rule),
        egui::StrokeKind::Inside,
    );
    rect.shrink(6.0)
}

/// Three facts in three equal columns, label over value, each clipped
/// to its column.
pub(super) fn draw_facts(painter: &egui::Painter, rect: egui::Rect, facts: &[(&str, String)]) {
    let c = palette::colours();
    let small = font(8.5);
    let n = facts.len().max(1);
    let w = rect.width() / n as f32;
    let cells = ((w - 6.0) / 5.2).floor().max(4.0) as usize;
    for (i, (label, value)) in facts.iter().enumerate() {
        let x = rect.left() + w * i as f32;
        painter.text(
            egui::pos2(x, rect.top() + 4.0),
            egui::Align2::LEFT_TOP,
            *label,
            small.clone(),
            c.label,
        );
        painter.text(
            egui::pos2(x, rect.bottom() - 4.0),
            egui::Align2::LEFT_BOTTOM,
            super::fit_cells(value, cells),
            small.clone(),
            c.fg,
        );
    }
}

/// The parameter rail, the same on every face.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_params(
    painter: &egui::Painter,
    rect: egui::Rect,
    column: &chain::Column,
    index: usize,
    cursor: Option<(usize, usize)>,
    row_offset: usize,
    rows_shown: usize,
    tag: &'static str,
    legend: &str,
    mark: &dyn Fn(usize) -> RowMark,
) {
    let c = palette::colours();
    chassis::instrument_rail(painter, rect);
    let head = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), PARAM_HEAD_H));
    painter.text(
        egui::pos2(head.left() + 6.0, head.center().y),
        egui::Align2::LEFT_CENTER,
        legend,
        font(8.5),
        c.label,
    );
    let body = egui::Rect::from_min_max(egui::pos2(rect.left(), head.bottom()), rect.max);
    if rows_shown == 0 || body.height() <= 1.0 {
        return;
    }
    let pitch = (body.height() / rows_shown as f32).min(24.0).max(13.0);
    let row_font = font(9.5);
    let cell_w = painter
        .layout_no_wrap("M".to_owned(), row_font.clone(), c.fg)
        .rect
        .width()
        .max(1.0);
    for line in 0..rows_shown {
        let Some(row) = column.rows.get(row_offset + line) else {
            break;
        };
        let row_index = row_offset + line;
        let mark = mark(row_index);
        let row_rect = egui::Rect::from_min_size(
            egui::pos2(body.left() + 4.0, body.top() + line as f32 * pitch),
            egui::vec2(body.width() - 8.0, pitch),
        );
        let selected = cursor == Some((index, row_index));
        if selected {
            painter.rect_filled(row_rect, 0.0, c.select);
            crate::ui::nav_cursor::claim(
                painter,
                (tag, index, row_index),
                row_rect,
                crate::ui::nav_cursor::Kind::Row,
                crate::ui::nav_cursor::Layer::Surface,
                c.alert,
            );
        }
        let ink = if selected {
            c.bright
        } else if mark.door {
            c.alert
        } else if row.edited {
            c.alert
        } else {
            c.fg
        };
        let value_cells = 9usize;
        let name_cells = ((row_rect.width() / cell_w).floor() as usize)
            .saturating_sub(value_cells + 2)
            .max(3);
        let name = if mark.star {
            format!("{}*", row.name)
        } else {
            row.name.clone()
        };
        painter.text(
            egui::pos2(row_rect.left() + 4.0, row_rect.center().y - 1.0),
            egui::Align2::LEFT_CENTER,
            super::fit_cells(&name, name_cells),
            row_font.clone(),
            ink,
        );
        painter.text(
            egui::pos2(row_rect.right() - 4.0, row_rect.center().y - 1.0),
            egui::Align2::RIGHT_CENTER,
            if mark.door {
                "OPEN >".to_owned()
            } else {
                super::fit_cells(&row.value, value_cells)
            },
            row_font.clone(),
            ink,
        );
        if mark.door {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_face_reserves_a_picture_and_a_rail_and_a_plate_only_when_asked() {
        let card = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(WIDTH, 236.0));
        let with = Layout::of(card, 40.0, true).expect("a full face");
        assert!(with.plot.width() > with.params.width());
        assert!(with.plot.height() > FACT_H);
        assert_eq!(with.params.width(), PARAM_W);
        assert!(!with.plot.intersects(with.params));
        assert!(with.lab.is_some_and(|lab| !lab.intersects(with.plot)));
        let without = Layout::of(card, 40.0, false).expect("a full face");
        assert!(without.lab.is_none());
        assert_eq!(without.plot, with.plot);
        assert!(
            Layout::of(
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(120.0, 60.0)),
                40.0,
                true
            )
            .is_none()
        );
    }
}
