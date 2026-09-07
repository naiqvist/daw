//! The sampler in the Stage chain.
//!
//! Every other chain card is primarily a parameter table. A sampler is
//! primarily MATERIAL: the waveform, the playable region, the loop, and the
//! cuts. This wider face keeps the ordinary scrolling parameter table on its
//! right, so keyboard editing remains uniform, while the left side says what
//! the instrument will actually play. The full cutting room is one explicit
//! door away rather than a feature hidden behind an undocumented Enter.

use super::{chassis, palette};
use crate::PROFONT;
use crate::design::codex::Sign;
use crate::design::kit::Weight;
use crate::params::sampler as sp;
use crate::ui::affordance::{Afford, Affords};
use crate::ui::chrome;
use crate::ui::stage::chain::{self, SamplerFace};
use eframe::egui;

/// Wide enough for a real material view and an uncompromised parameter rail.
pub(super) const WIDTH: f32 = 478.0;

const PAD: f32 = 9.0;
const GAP: f32 = 10.0;
const PARAM_W: f32 = 188.0;
const PARAM_HEAD_H: f32 = 19.0;
const FACT_H: f32 = 39.0;
const LAB_W: f32 = 126.0;

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

fn mode_word(face: &SamplerFace) -> &'static str {
    sp::MODE_NAMES.get(face.mode).copied().unwrap_or("classic")
}

fn loop_word(face: &SamplerFace) -> &'static str {
    sp::LOOP_NAMES.get(face.loop_mode).copied().unwrap_or("off")
}

fn source_word(face: &SamplerFace) -> &'static str {
    sp::SLICE_SOURCE_NAMES
        .get(face.slice_source)
        .copied()
        .unwrap_or("grid")
}

/// The table the engine will use when no explicit cuts have been authored.
/// Keeping this projection here makes the picture honest before Sample Lab
/// has written the grid back into the song.
fn effective_slices(face: &SamplerFace) -> Vec<f64> {
    if !face.slices.is_empty() {
        return face.slices.clone();
    }
    let count = face.slice_count.clamp(1, sp::SLICES_MAX as usize);
    (0..count)
        .map(|index| index as f64 / count as f64)
        .collect()
}

fn percent(value: f32) -> String {
    format!("{:.0}%", value.clamp(0.0, 1.0) * 100.0)
}

fn draw_header(
    ui: &mut egui::Ui,
    layout: Layout,
    face: &SamplerFace,
    column: &chain::Column,
    index: usize,
    selected: bool,
    alpha: &crate::design::Alphabet,
) -> bool {
    let painter = ui.painter();
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
        "SAMPLER // MATERIAL ENGINE",
        font.clone(),
        if selected {
            colours.bright
        } else {
            colours.dir
        },
    );
    let name = column.sample.as_deref().unwrap_or("NO MATERIAL LOADED");
    painter.text(
        egui::pos2(layout.head.left() + 31.0, layout.head.bottom() - 5.0),
        egui::Align2::LEFT_BOTTOM,
        super::fit_cells(name, 25),
        font.clone(),
        if column.sample.is_some() {
            family_ink
        } else {
            colours.dim
        },
    );

    // This is a real pointer door, not merely text that resembles one.
    let response = ui
        .interact(
            layout.lab,
            egui::Id::new(("stage-sampler-lab", index)),
            egui::Sense::click(),
        )
        .affords(Affords::Press);
    let open_ink = if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        painter.rect_filled(layout.lab, 0.0, colours.select);
        colours.bright
    } else if column.sample.is_some() {
        colours.alert
    } else {
        colours.dim
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
        "ENTER  SAMPLE LAB >",
        font,
        open_ink,
    );

    // A compact mode lamp lives immediately beside the door. It remains a
    // reading rather than a second control; the parameter rail edits it.
    let mode = mode_word(face).to_ascii_uppercase();
    let mode_x = layout.lab.left() - 10.0;
    painter.text(
        egui::pos2(mode_x, layout.head.center().y),
        egui::Align2::RIGHT_CENTER,
        if face.reversed {
            format!("{mode} / REV")
        } else {
            mode
        },
        egui::FontId::new(9.0, egui::FontFamily::Name(PROFONT.into())),
        colours.nominal,
    );
    response.clicked()
}

fn draw_plot(
    painter: &egui::Painter,
    rect: egui::Rect,
    face: &SamplerFace,
    data: Option<&crate::ui::stage::SampleData>,
) {
    let c = palette::colours();
    chassis::instrument_rail(painter, rect);
    let inner = rect.shrink2(egui::vec2(6.0, 7.0));
    if inner.width() < 2.0 || inner.height() < 2.0 {
        return;
    }
    let mid = inner.center().y;
    painter.line_segment(
        [
            egui::pos2(inner.left(), mid),
            egui::pos2(inner.right(), mid),
        ],
        egui::Stroke::new(1.0, c.rule),
    );

    if let Some(data) = data {
        let count = inner.width().floor().max(2.0) as usize;
        let bins = data.peaks.columns(Some(&data.samples), 0.0, 1.0, count);
        let half = (inner.height() * 0.5 - 2.0).max(1.0);
        for (drawn, bin) in bins.iter().enumerate() {
            let x = inner.left() + drawn as f32 + 0.5;
            painter.line_segment(
                [
                    egui::pos2(x, mid - bin.max.clamp(-1.0, 1.0) * half),
                    egui::pos2(x, mid - bin.min.clamp(-1.0, 1.0) * half),
                ],
                egui::Stroke::new(1.0, c.chassis.gamma_multiply(0.65)),
            );
            let rms = bin.rms.clamp(0.0, 1.0) * half;
            painter.line_segment(
                [egui::pos2(x, mid - rms), egui::pos2(x, mid + rms)],
                egui::Stroke::new(1.0, c.nominal.gamma_multiply(0.9)),
            );
        }
    } else {
        painter.text(
            inner.center(),
            egui::Align2::CENTER_CENTER,
            if face.path.is_some() {
                "INDEXING MATERIAL"
            } else {
                "LOAD A SAMPLE TO BEGIN"
            },
            egui::FontId::new(10.0, egui::FontFamily::Name(PROFONT.into())),
            c.dim,
        );
    }

    let x = |fraction: f32| inner.left() + fraction.clamp(0.0, 1.0) * inner.width();
    let start = face.start;
    // The sampler treats a crossed END as the file's end rather than
    // silently swapping the two handles. The card follows that same rule.
    let end = if face.end > start { face.end } else { 1.0 };
    let ground = c.ground;
    let shade = egui::Color32::from_rgba_unmultiplied(ground.r(), ground.g(), ground.b(), 178);

    let slicing = face.mode == sp::MODE_SLICE as usize;
    let slices = effective_slices(face);
    if slicing {
        for (index, pair) in slices.windows(2).enumerate() {
            if index % 2 == 0 {
                let from = (pair[0] as f32).max(start);
                let to = (pair[1] as f32).min(end);
                if to <= from {
                    continue;
                }
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x(from), inner.top()),
                        egui::pos2(x(to), inner.bottom()),
                    ),
                    0.0,
                    c.select.gamma_multiply(0.42),
                );
            }
        }
    }
    for slice in slices.iter().skip(1) {
        let sx = x(*slice as f32).round() - 0.5;
        painter.line_segment(
            [egui::pos2(sx, inner.top()), egui::pos2(sx, inner.bottom())],
            egui::Stroke::new(1.0, if slicing { c.chassis } else { c.rule }),
        );
    }

    if face.loop_mode != sp::LOOP_OFF as usize {
        let loop_at = start + (end - start) * face.loop_start;
        let lx = x(loop_at);
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(lx, inner.top()),
                egui::pos2(x(end), inner.bottom()),
            ),
            0.0,
            c.nominal.gamma_multiply(0.10),
        );
        painter.line_segment(
            [egui::pos2(lx, inner.top()), egui::pos2(lx, inner.bottom())],
            egui::Stroke::new(1.5, c.nominal),
        );
    }
    // Outside the playable region is veiled LAST, after all source-relative
    // cuts and loop state. Those facts remain present, but cannot look active.
    if start > 0.0 {
        painter.rect_filled(
            egui::Rect::from_min_max(inner.min, egui::pos2(x(start), inner.bottom())),
            0.0,
            shade,
        );
    }
    if end < 1.0 {
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(x(end), inner.top()), inner.max),
            0.0,
            shade,
        );
    }
    for (at, word) in [(start, "IN"), (end, "OUT")] {
        let px = x(at).round() - 0.5;
        painter.line_segment(
            [egui::pos2(px, inner.top()), egui::pos2(px, inner.bottom())],
            egui::Stroke::new(1.5, c.alert),
        );
        painter.text(
            egui::pos2(
                px + if word == "IN" { 3.0 } else { -3.0 },
                inner.top() + 2.0,
            ),
            if word == "IN" {
                egui::Align2::LEFT_TOP
            } else {
                egui::Align2::RIGHT_TOP
            },
            word,
            egui::FontId::new(8.0, egui::FontFamily::Name(PROFONT.into())),
            c.alert,
        );
    }
}

fn draw_facts(painter: &egui::Painter, rect: egui::Rect, face: &SamplerFace) {
    let c = palette::colours();
    chassis::instrument_rail(painter, rect);
    let font = egui::FontId::new(9.0, egui::FontFamily::Name(PROFONT.into()));
    let values = [
        (
            "TRIM",
            format!("{} - {}", percent(face.start), percent(face.end)),
        ),
        (
            "LOOP",
            if face.loop_mode == sp::LOOP_OFF as usize {
                "OFF".to_owned()
            } else {
                format!(
                    "{} @ {}",
                    loop_word(face).to_ascii_uppercase(),
                    percent(face.loop_start)
                )
            },
        ),
        (
            "CUTS",
            format!(
                "{:02} {}",
                effective_slices(face).len(),
                source_word(face).to_ascii_uppercase()
            ),
        ),
    ];
    let each = rect.width() / values.len() as f32;
    for (index, (label, value)) in values.iter().enumerate() {
        let cell = egui::Rect::from_min_max(
            egui::pos2(rect.left() + index as f32 * each, rect.top()),
            egui::pos2(rect.left() + (index + 1) as f32 * each, rect.bottom()),
        );
        if index > 0 {
            chassis::rail_splice(painter, rect, cell.left());
        }
        painter.text(
            egui::pos2(cell.left() + 6.0, cell.top() + 5.0),
            egui::Align2::LEFT_TOP,
            *label,
            font.clone(),
            c.label,
        );
        painter.text(
            egui::pos2(cell.left() + 6.0, cell.bottom() - 5.0),
            egui::Align2::LEFT_BOTTOM,
            super::fit_cells(value, ((each - 10.0) / 5.4).floor().max(2.0) as usize),
            font.clone(),
            c.fg,
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
                ("stage-sampler-param-cursor", index, row_index),
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
    /// Draw the distinctive Sampler face. Returns true when the pointer asked
    /// to enter Sample Lab; keyboard Enter continues through the normal map.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_sampler_chain_card(
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
        let Some(face) = column.sampler.as_ref() else {
            return false;
        };
        let Some(layout) = Layout::of(card, head_h) else {
            return false;
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

        let opened = draw_header(ui, layout, face, column, index, selected, &alpha);
        let data = self.sample_data.as_ref().filter(|data| {
            face.path
                .as_ref()
                .is_some_and(|path| data.path.as_path() == path.as_path())
        });
        draw_plot(&painter, layout.plot, face, data);
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

        let wave = ui
            .interact(
                layout.plot,
                egui::Id::new(("stage-sampler-wave", index)),
                egui::Sense::click(),
            )
            .affords(Affords::Press);
        if wave.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ZoomIn);
        }
        opened || wave.double_clicked()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sampler_face_reserves_material_and_parameter_regions() {
        let card = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(WIDTH, 236.0));
        let layout = Layout::of(card, 40.0).expect("a full sampler card");
        assert!(layout.plot.width() > layout.params.width());
        assert!(layout.plot.height() > FACT_H);
        assert_eq!(layout.params.width(), PARAM_W);
        assert!(!layout.plot.intersects(layout.params));
        assert!(!layout.lab.intersects(layout.visual));
    }

    #[test]
    fn authored_slices_win_and_an_empty_table_projects_the_engine_grid() {
        let mut face = SamplerFace {
            path: None,
            mode: sp::MODE_SLICE as usize,
            start: 0.0,
            end: 1.0,
            loop_mode: sp::LOOP_OFF as usize,
            loop_start: 0.0,
            slice_source: sp::SLICE_GRID as usize,
            slice_count: 4,
            slices: Vec::new(),
            reversed: false,
        };
        assert_eq!(effective_slices(&face), [0.0, 0.25, 0.5, 0.75]);
        face.slices = vec![0.0, 0.3, 0.9];
        assert_eq!(effective_slices(&face), [0.0, 0.3, 0.9]);
    }
}
