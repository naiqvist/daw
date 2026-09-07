//! STAB in the Stage chain.
//!
//! A chord synth's one fact that a parameter table cannot show is the
//! chord. This wider face keeps the scrolling parameter rail on its
//! right, so keyboard editing stays uniform, and gives its left to a
//! keyboard with the chord lit on it — voiced by the same function the
//! voices voice it with, so the picture is what plays.

use super::{chassis, palette};
use crate::PROFONT;
use crate::design::codex::Sign;
use crate::design::kit::Weight;
use crate::params::stab as sp;
use crate::ui::chrome;
use crate::ui::stage::chain::{self, StabFace};
use eframe::egui;

/// The sampler's width: the band's wide faces stand alike.
pub(super) const WIDTH: f32 = 478.0;

const PAD: f32 = 9.0;
const GAP: f32 = 10.0;
const PARAM_W: f32 = 188.0;
const PARAM_HEAD_H: f32 = 19.0;
const FACT_H: f32 = 30.0;
/// The keyboard: three octaves, the played key at the start of the
/// second, so a dropped note has room below and a ninth above.
const OCTAVES: i32 = 3;
const KEY_BASE: i32 = 12;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    head: egui::Rect,
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
            visual,
            plot,
            facts,
            params,
        })
    }
}

fn black(semitone: i32) -> bool {
    matches!(semitone.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

fn tone_word(tone: f32) -> &'static str {
    if tone < 0.2 {
        "sine"
    } else if tone < 0.6 {
        "keys"
    } else if tone < 0.85 {
        "organ"
    } else {
        "drawbars"
    }
}

fn draw_header(
    painter: &egui::Painter,
    layout: Layout,
    face: &StabFace,
    column: &chain::Column,
    selected: bool,
    alpha: &crate::design::Alphabet,
) {
    let colours = palette::colours();
    let family_ink = if column.bypassed {
        alpha.edge.color
    } else {
        alpha.ink.color
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
        "STAB // CHORD ENGINE",
        font.clone(),
        if selected {
            colours.bright
        } else {
            colours.dir
        },
    );
    let p = &face.params;
    let inversion = sp::INVERSION_NAMES
        .get(p.inversion.round().max(0.0) as usize)
        .copied()
        .unwrap_or("root");
    painter.text(
        egui::pos2(layout.head.left() + 31.0, layout.head.bottom() - 5.0),
        egui::Align2::LEFT_BOTTOM,
        super::fit_cells(
            &format!(
                "{} · {inversion} · {}{} · {} · x{:.1}",
                face.word(),
                if p.open.round() >= 1.0 {
                    "open"
                } else {
                    "close"
                },
                match p.omit.round().max(0.0) as usize {
                    0 => String::new(),
                    omit => format!(" · no {}", sp::OMIT_NAMES.get(omit).copied().unwrap_or("?")),
                },
                tone_word(p.tone),
                p.drive
            ),
            44,
        ),
        font.clone(),
        family_ink,
    );
    // The chord's name, large, on the right of the head: it is the one
    // thing the card is for.
    painter.text(
        egui::pos2(layout.head.right() - 10.0, layout.head.center().y),
        egui::Align2::RIGHT_CENTER,
        face.word(),
        egui::FontId::new(15.0, egui::FontFamily::Name(PROFONT.into())),
        colours.alert,
    );
}

/// The keyboard, three octaves, the chord lit.
fn draw_plot(painter: &egui::Painter, rect: egui::Rect, face: &StabFace) {
    let c = palette::colours();
    painter.rect_filled(rect, 0.0, c.panel);
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, c.rule),
        egui::StrokeKind::Inside,
    );
    let keys = rect.shrink(4.0);
    if keys.width() < 40.0 || keys.height() < 12.0 {
        return;
    }
    let font = egui::FontId::new(8.5, egui::FontFamily::Name(PROFONT.into()));
    let (notes, count) = face.notes();
    let lit = |semitone: i32| notes[..count].contains(&(semitone - KEY_BASE));
    let whites = (OCTAVES * 7) as f32;
    let white_w = keys.width() / whites;
    let mut white_index = 0;
    for semitone in 0..(OCTAVES * 12) {
        if black(semitone) {
            continue;
        }
        let x = keys.left() + white_index as f32 * white_w;
        let key = egui::Rect::from_min_max(
            egui::pos2(x, keys.top()),
            egui::pos2(x + white_w - 1.0, keys.bottom()),
        );
        painter.rect_filled(key, 0.0, if lit(semitone) { c.alert } else { c.chassis });
        if semitone.rem_euclid(12) == 0 {
            painter.text(
                egui::pos2(key.center().x, key.bottom() - 2.0),
                egui::Align2::CENTER_BOTTOM,
                if semitone == KEY_BASE { "C" } else { "·" },
                font.clone(),
                if lit(semitone) { c.bright } else { c.label },
            );
        }
        white_index += 1;
    }
    let black_h = keys.height() * 0.6;
    let mut white_index = 0;
    for semitone in 0..(OCTAVES * 12) {
        if !black(semitone) {
            white_index += 1;
            continue;
        }
        let x = keys.left() + white_index as f32 * white_w - white_w * 0.3;
        let key = egui::Rect::from_min_max(
            egui::pos2(x, keys.top()),
            egui::pos2(x + white_w * 0.6, keys.top() + black_h),
        );
        painter.rect_filled(key, 0.0, if lit(semitone) { c.alert } else { c.ground });
        painter.rect_stroke(
            key,
            0.0,
            egui::Stroke::new(1.0, if lit(semitone) { c.bright } else { c.rule }),
            egui::StrokeKind::Inside,
        );
    }
}

fn draw_facts(painter: &egui::Painter, rect: egui::Rect, face: &StabFace) {
    let colours = palette::colours();
    let font = egui::FontId::new(8.5, egui::FontFamily::Name(PROFONT.into()));
    let p = &face.params;
    let (_, count) = face.notes();
    let facts = [
        ("NOTES", format!("{count} x2 · {:.0}ct", p.detune_ct)),
        (
            "PLUCK",
            format!("{:.1}k +{:.1}oct", p.cutoff_hz / 1000.0, p.env_oct),
        ),
        ("GRIT", format!("x{:.1} · {:.0}bit", p.drive, p.crush_bits)),
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
        "PARAM BANK // UP/DN  LEFT/RIGHT",
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
    for line in 0..rows_shown {
        let Some(row) = column.rows.get(row_offset + line) else {
            break;
        };
        let row_index = row_offset + line;
        let row_rect = egui::Rect::from_min_size(
            egui::pos2(body.left() + 4.0, body.top() + line as f32 * pitch),
            egui::vec2(body.width() - 8.0, pitch),
        );
        let selected = cursor == Some((index, row_index));
        if selected {
            painter.rect_filled(row_rect, 0.0, c.select);
            crate::ui::nav_cursor::claim(
                painter,
                ("stage-stab-param-cursor", index, row_index),
                row_rect,
                crate::ui::nav_cursor::Kind::Row,
                crate::ui::nav_cursor::Layer::Surface,
                c.alert,
            );
        }
        let ink = if selected {
            c.bright
        } else if row.edited {
            c.alert
        } else {
            c.fg
        };
        let value_cells = 9usize;
        let name_cells = ((row_rect.width() / cell_w).floor() as usize)
            .saturating_sub(value_cells + 2)
            .max(3);
        painter.text(
            egui::pos2(row_rect.left() + 4.0, row_rect.center().y - 1.0),
            egui::Align2::LEFT_CENTER,
            super::fit_cells(&row.name, name_cells),
            font.clone(),
            ink,
        );
        painter.text(
            egui::pos2(row_rect.right() - 4.0, row_rect.center().y - 1.0),
            egui::Align2::RIGHT_CENTER,
            super::fit_cells(&row.value, value_cells),
            font.clone(),
            ink,
        );
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
    /// Draw the STAB face.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_stab_chain_card(
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
        let Some(face) = column.stab.as_ref() else {
            return;
        };
        let Some(layout) = Layout::of(card, head_h) else {
            return;
        };
        let painter = ui.painter().clone();
        let alpha = self.glass();
        let selected = cursor.is_some_and(|(col, _)| col == index);
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
        draw_header(&painter, layout, face, column, selected, &alpha);
        draw_plot(&painter, layout.plot, face);
        draw_facts(&painter, layout.facts, face);
        draw_params(
            &painter,
            layout.params,
            column,
            index,
            cursor,
            row_offset,
            rows_shown,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stab_face_reserves_a_keyboard_and_a_parameter_rail() {
        let card = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(WIDTH, 236.0));
        let layout = Layout::of(card, 40.0).expect("a full STAB card");
        assert!(layout.plot.width() > layout.params.width());
        assert!(layout.plot.height() > FACT_H);
        assert_eq!(layout.params.width(), PARAM_W);
        assert!(!layout.plot.intersects(layout.params));
        assert!(
            Layout::of(
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(120.0, 60.0)),
                40.0
            )
            .is_none()
        );
    }

    #[test]
    fn the_face_names_the_chord_from_the_voicing() {
        use crate::sequencing::{Device, DeviceId};
        let mut device = Device::new(DeviceId(1), crate::devices::DeviceKind::Stab);
        assert_eq!(StabFace::from_device(&device).word(), "Cm7");
        device.set(sp::CHORD, 0.0);
        device.set(sp::INVERSION, 1.0);
        assert_eq!(StabFace::from_device(&device).word(), "Cmaj / E");
        device.set(sp::OPEN, 1.0);
        device.set(sp::INVERSION, 0.0);
        assert_eq!(
            StabFace::from_device(&device).word(),
            "Cmaj / E",
            "drop-two on a triad puts the third under"
        );
        device.set(sp::OPEN, 0.0);
        device.set(sp::OMIT, 1.0);
        assert_eq!(
            StabFace::from_device(&device).word(),
            "Cmaj / E",
            "rootless: the third is the bass"
        );
        assert_eq!(tone_word(0.0), "sine");
        assert_eq!(tone_word(1.0), "drawbars");
    }
}
