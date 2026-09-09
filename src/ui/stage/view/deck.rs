//! The deck: the standing row of F1–F8 at the top of the screen, and
//! its window — one sub-page of eight cells, one size, floating over
//! the session whenever a page is lit.

use super::{Stage, palette};
use crate::PROFONT;
use crate::pages::PageKey;
use crate::ui::affordance::{Afford, Affords};
use crate::ui::chrome;
use eframe::egui;

/// One compact row between title and field: the eight keys.
/// @tune 18..40 px
const DECK_H: f32 = 24.0;
const PAD: f32 = 5.0;
/// @tune 16..32 px
const WINDOW_TITLE_H: f32 = 22.0;
/// @tune 0..40 px
const WINDOW_INSET: f32 = 8.0;
/// The picture stage above the cells; capped each frame at a third of
/// the field so the session under the window keeps some of its height.
/// @tune 0..240 px
const HERO_H: f32 = 140.0;
/// Clear margin around the hero panel on all four sides.
/// @tune 0..24 px
const HERO_INSET: f32 = 8.0;

struct WindowLayout {
    rect: egui::Rect,
    title: egui::Rect,
    hero: egui::Rect,
    cells: egui::Rect,
}

impl WindowLayout {
    fn of(field: egui::Rect, inset: f32, title_h: f32, hero_h: f32) -> Self {
        let base_h = field.height() / 3.0 - inset;
        let hero_h = hero_h.max(0.0).min((field.height() / 3.0).max(0.0));
        let rect = egui::Rect::from_min_size(
            field.min + egui::vec2(inset, inset),
            egui::vec2(field.width() - 2.0 * inset, base_h + hero_h),
        );
        let title = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), title_h));
        let hero = egui::Rect::from_min_size(title.left_bottom(), egui::vec2(rect.width(), hero_h));
        let cells = egui::Rect::from_min_max(hero.left_bottom(), rect.max);
        Self {
            rect,
            title,
            hero,
            cells,
        }
    }
}

pub(super) fn deck_h() -> f32 {
    crate::tune!(DECK_H)
}

impl Stage {
    pub(super) fn deck_wave_rect(&self, field: egui::Rect) -> egui::Rect {
        self.deck_window_layout(field)
            .hero
            .shrink(crate::tune!(HERO_INSET).max(0.0))
            .shrink(6.0)
    }
    pub(super) fn draw_deck(&self, painter: &egui::Painter, rect: egui::Rect) {
        let c = palette::colours();
        painter.rect_filled(rect, 0.0, c.panel);
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(1.0, c.rule),
        );
        let page_font = egui::FontId::new(11.0, egui::FontFamily::Name(PROFONT.into()));
        let cell_w = rect.width() / 8.0;
        let lit = self.deck_lit();
        let steps = self.step_keys();

        for (index, key) in PageKey::ALL.into_iter().enumerate() {
            let cell = egui::Rect::from_min_max(
                egui::pos2(rect.left() + index as f32 * cell_w, rect.top()),
                egui::pos2(rect.left() + (index + 1) as f32 * cell_w, rect.bottom()),
            );
            let active = lit == Some(key);
            let available = self.page_available(key);
            if active {
                painter.rect_filled(cell.shrink(1.0), 0.0, c.select);
                painter.line_segment(
                    [cell.left_bottom(), cell.right_bottom()],
                    egui::Stroke::new(2.0, c.bright),
                );
            }
            painter.text(
                egui::pos2(cell.left() + PAD, cell.center().y),
                egui::Align2::LEFT_CENTER,
                format!("F{}", index + 1),
                page_font.clone(),
                if active { c.ground } else { c.dim },
            );
            painter.text(
                egui::pos2(cell.left() + 27.0, cell.center().y),
                egui::Align2::LEFT_CENTER,
                self.deck_key_word(key),
                page_font.clone(),
                if !available {
                    c.dim
                } else if active {
                    c.bright
                } else {
                    c.fg
                },
            );
            let count = self.deck_subpage_count(key);
            let pip_y = cell.bottom() - 3.0;
            for pip in 0..count.min(8) {
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(cell.left() + 27.0 + pip as f32 * 5.0, pip_y),
                        egui::vec2(3.0, 1.0),
                    ),
                    0.0,
                    if active { c.bright } else { c.edge },
                );
            }
            if active && steps.is_some_and(|(_, held)| held != 0) {
                painter.text(
                    egui::pos2(cell.right() - PAD, cell.center().y),
                    egui::Align2::RIGHT_CENTER,
                    "L",
                    page_font.clone(),
                    c.alert,
                );
            }
        }

        if let Some((window, _)) = steps {
            for at in 0..PATTERN_WINDOWS {
                painter.circle_filled(
                    egui::pos2(
                        rect.right() - 6.0 - (PATTERN_WINDOWS - 1 - at) as f32 * 5.0,
                        rect.bottom() - 4.0,
                    ),
                    1.25,
                    if at == window { c.alert } else { c.edge },
                );
            }
        }
    }

    /// One sub-page over the session: title, empty hero stage, eight cells.
    /// Only the hero shrinks to keep the grid at the foot in view.
    pub(super) fn draw_deck_window(&self, painter: &egui::Painter, field: egui::Rect) {
        if !self.deck_open() {
            return;
        }
        let c = palette::colours();
        let WindowLayout {
            rect,
            title,
            hero,
            cells,
        } = self.deck_window_layout(field);
        painter.rect_filled(rect, 0.0, c.panel);
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, c.rule),
            egui::StrokeKind::Inside,
        );
        let title_font = egui::FontId::new(13.0, egui::FontFamily::Name(PROFONT.into()));
        let key_font = egui::FontId::new(11.0, egui::FontFamily::Name(PROFONT.into()));
        let name_font = egui::FontId::new(13.0, egui::FontFamily::Name(PROFONT.into()));
        let value_font = egui::FontId::new(20.0, egui::FontFamily::Name(PROFONT.into()));
        let steps = self.step_keys();

        // The title: the key, its word, the sub-page and where it stands.
        if let Some(key) = self.deck_lit() {
            let word = self.deck_key_word(key);
            let (at, count, sub) = self.deck_position().unwrap_or((0, 0, ""));
            painter.text(
                egui::pos2(title.left() + PAD, title.center().y),
                egui::Align2::LEFT_CENTER,
                format!("F{} {word}", key.index() + 1),
                title_font.clone(),
                c.bright,
            );
            let standing = if count == 0 {
                "nothing here".to_owned()
            } else if count == 1 {
                sub.to_owned()
            } else {
                format!("{sub}  {}/{count}", at + 1)
            };
            painter.text(
                egui::pos2(title.right() - PAD, title.center().y),
                egui::Align2::RIGHT_CENTER,
                standing,
                title_font.clone(),
                c.fg,
            );
            if steps.is_some_and(|(_, held)| held != 0) {
                painter.text(
                    egui::pos2(title.center().x, title.center().y),
                    egui::Align2::CENTER_CENTER,
                    "LOCK",
                    title_font.clone(),
                    c.alert,
                );
            }
        }
        painter.line_segment(
            [title.left_bottom(), title.right_bottom()],
            egui::Stroke::new(1.0, c.rule),
        );
        self.draw_deck_hero(painter, hero);

        let slots = self.deck_slots();
        let columns =
            if self.deck_hero_height() == crate::pages::HeroHeight::Tall && cells.width() < 960.0 {
                4
            } else {
                8
            };
        let rows = 8 / columns;
        let cell_w = cells.width() / columns as f32;
        let cell_h = cells.height() / rows as f32;
        for (index, view) in slots.iter().enumerate() {
            let cell = egui::Rect::from_min_max(
                egui::pos2(
                    cells.left() + (index % columns) as f32 * cell_w,
                    cells.top() + (index / columns) as f32 * cell_h,
                ),
                egui::pos2(
                    cells.left() + (index % columns + 1) as f32 * cell_w,
                    cells.top() + (index / columns + 1) as f32 * cell_h,
                ),
            )
            .shrink2(egui::vec2(4.0, 4.0));
            if index == self.deck_selected_slot() {
                let away = self.deck_hero_focus().is_some();
                if self.deck_hero_height() == crate::pages::HeroHeight::Tall && !away {
                    crate::ui::nav_cursor::claim(
                        painter,
                        ("material-cell", index),
                        cell,
                        crate::ui::nav_cursor::Kind::Cell,
                        crate::ui::nav_cursor::Layer::Overlay,
                        c.bright,
                    );
                }
                painter.rect_stroke(
                    cell,
                    0.0,
                    egui::Stroke::new(
                        if away { 1.0 } else { 1.5 },
                        if away { c.dim } else { c.bright },
                    ),
                    egui::StrokeKind::Inside,
                );
            }
            let Some(view) = view else { continue };
            if view.locked {
                painter.rect_filled(cell, 0.0, c.fg);
            }
            // A section that is OUT: its knob is heard by nothing until
            // a turn switches it in, so the cell is dimmed and says so.
            let ink = if view.locked {
                c.ground
            } else if view.out {
                c.dim
            } else {
                c.fg
            };
            let dim = if view.locked { c.ground } else { c.dim };
            if view.out {
                painter.text(
                    egui::pos2(cell.right() - PAD, cell.top() + 5.0),
                    egui::Align2::RIGHT_TOP,
                    "OUT",
                    key_font.clone(),
                    c.dim,
                );
            }
            let key = if steps.is_some() {
                ["A", "S", "D", "F", "G", "H", "J", "K"][index]
            } else {
                ["1", "2", "3", "4", "5", "6", "7", "8"][index]
            };
            painter.text(
                egui::pos2(cell.left() + PAD, cell.top() + 5.0),
                egui::Align2::LEFT_TOP,
                key,
                key_font.clone(),
                dim,
            );
            painter.text(
                egui::pos2(cell.left() + PAD + 16.0, cell.top() + 4.0),
                egui::Align2::LEFT_TOP,
                view.name,
                name_font.clone(),
                ink,
            );
            // The value, large, on its own line; the unit small after it.
            let value_y = cell.center().y + 2.0;
            let value_rect = painter.text(
                egui::pos2(cell.right() - PAD, value_y),
                egui::Align2::RIGHT_CENTER,
                &view.value,
                value_font.clone(),
                ink,
            );
            if view.slide {
                painter.text(
                    egui::pos2(value_rect.left() - 4.0, value_y),
                    egui::Align2::RIGHT_CENTER,
                    "~",
                    value_font.clone(),
                    ink,
                );
            }
            let gauge = egui::Rect::from_min_max(
                egui::pos2(cell.left() + PAD, cell.bottom() - 14.0),
                egui::pos2(cell.right() - PAD, cell.bottom() - 6.0),
            );
            let mut shapes = Vec::new();
            if view.choices.is_empty() {
                chrome::tick_bar(&mut shapes, gauge, 16, view.place, ink, c.edge, true);
            } else {
                let chosen =
                    (view.place * (view.choices.len().saturating_sub(1)) as f32).round() as usize;
                chrome::choice_bar(&mut shapes, gauge, view.choices.len(), chosen, ink, c.edge);
            }
            painter.extend(shapes);
        }

        if let Some((window, _)) = steps {
            for at in 0..PATTERN_WINDOWS {
                painter.circle_filled(
                    egui::pos2(
                        rect.right() - 8.0 - (PATTERN_WINDOWS - 1 - at) as f32 * 6.0,
                        rect.bottom() - 4.0,
                    ),
                    1.5,
                    if at == window { c.alert } else { c.edge },
                );
            }
        }
    }

    fn draw_deck_hero(&self, painter: &egui::Painter, rect: egui::Rect) {
        let panel = rect.shrink(crate::tune!(HERO_INSET).max(0.0));
        if rect.height() <= 0.0 || !panel.is_positive() {
            return;
        }
        let painter = painter.with_clip_rect(painter.clip_rect().intersect(panel));
        let c = palette::colours();
        painter.rect_filled(panel, 0.0, c.panel);
        painter.rect_stroke(
            panel,
            0.0,
            egui::Stroke::new(1.0, c.edge),
            egui::StrokeKind::Inside,
        );
        let plot = panel.shrink(6.0);
        // A machine that browses a library gets the left of the panel for
        // its list; the picture keeps the right and is drawn by the same
        // one routine as every other machine's.
        let list = self.deck_hero_list();
        let layout = list.as_ref().map(|_| ListLayout::of(plot));
        let plot = layout.as_ref().map_or(plot, |layout| layout.map);
        if plot.is_positive() {
            for quarter in 1..4 {
                let x = plot.left() + plot.width() * quarter as f32 / 4.0;
                painter.line_segment(
                    [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
                    egui::Stroke::new(1.0, c.edge),
                );
            }
            painter.line_segment(
                [plot.left_bottom(), plot.right_bottom()],
                egui::Stroke::new(1.0, c.edge),
            );
        }
        let mark = 5.0_f32.min(panel.height() / 2.0).min(panel.width() / 2.0);
        for (corner, dx, dy) in [
            (panel.left_top(), mark, mark),
            (panel.right_top(), -mark, mark),
            (panel.left_bottom(), mark, -mark),
            (panel.right_bottom(), -mark, -mark),
        ] {
            for tip in [corner + egui::vec2(dx, 0.0), corner + egui::vec2(0.0, dy)] {
                painter.line_segment([corner, tip], egui::Stroke::new(1.0, c.dim));
            }
        }
        if let Some(caption) = self.deck_hero_caption() {
            painter.text(
                panel.min + egui::vec2(6.0, 4.0),
                egui::Align2::LEFT_TOP,
                caption,
                egui::FontId::new(10.0, egui::FontFamily::Name(PROFONT.into())),
                c.dim,
            );
        }
        if let Some(hero) = self.deck_hero()
            && plot.is_positive()
        {
            draw_hero(
                &painter,
                plot,
                &hero,
                match self.deck_hero_focus() {
                    Some(crate::pages::HeroTarget::Handle { param, .. }) => Some(param),
                    _ => None,
                },
            );
        }
        if let (Some(list), Some(layout)) = (&list, &layout) {
            draw_list_hero(
                &painter,
                layout,
                list,
                self.deck_hero_tools(),
                self.deck_hero_focus().is_some(),
            );
        }
    }
}

/// The tall panel when a machine browses a library: the list on the
/// left, the machine's ordinary picture on the right, tools underneath.
pub(super) struct ListLayout {
    pub(super) list: egui::Rect,
    pub(super) map: egui::Rect,
    pub(super) tools: egui::Rect,
    pub(super) row_h: f32,
}

impl ListLayout {
    pub(super) fn of(plot: egui::Rect) -> Self {
        let tools = egui::Rect::from_min_max(
            egui::pos2(plot.left(), plot.bottom() - 22.0),
            plot.right_bottom(),
        );
        let body = egui::Rect::from_min_max(
            plot.min + egui::vec2(0.0, 14.0),
            egui::pos2(plot.right(), tools.top() - 6.0),
        );
        let width = (body.width() * 0.38).clamp(140.0, 320.0);
        let list = egui::Rect::from_min_max(
            body.min,
            egui::pos2((body.left() + width).min(body.right()), body.bottom()),
        );
        let map = egui::Rect::from_min_max(
            egui::pos2((list.right() + 14.0).min(body.right()), body.top()),
            body.max,
        );
        Self {
            list,
            map,
            tools,
            row_h: 15.0,
        }
    }

    /// How many lines fit.
    pub(super) fn visible(&self) -> usize {
        (self.list.height() / self.row_h).floor().max(1.0) as usize
    }

    /// The first line drawn, so the selected row is on screen and near
    /// the middle of a long list.
    pub(super) fn window(&self, lines: &[ListLine], selected: usize) -> usize {
        let visible = self.visible();
        if lines.len() <= visible {
            return 0;
        }
        let at = lines
            .iter()
            .position(|line| *line == ListLine::Row(selected))
            .unwrap_or(0);
        at.saturating_sub(visible / 2)
            .min(lines.len().saturating_sub(visible))
    }

    /// The rectangle of one drawn line.
    pub(super) fn line_rect(&self, slot: usize) -> egui::Rect {
        egui::Rect::from_min_size(
            self.list.min + egui::vec2(0.0, slot as f32 * self.row_h),
            egui::vec2(self.list.width(), self.row_h),
        )
    }
}

/// One line of the panel: a group's heading, or one of its rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ListLine {
    Head(&'static str),
    Row(usize),
}

/// The panel's lines: a group heading is a line of its own, and so is
/// every row. ONE function, so the pointer and the paint can never
/// disagree about what is where.
pub(super) fn list_lines(rows: &[crate::pages::ListRow]) -> Vec<ListLine> {
    let mut out = Vec::new();
    let mut group = "";
    for (at, row) in rows.iter().enumerate() {
        if row.group != group {
            group = row.group;
            out.push(ListLine::Head(group));
        }
        out.push(ListLine::Row(at));
    }
    out
}

fn draw_list_hero(
    painter: &egui::Painter,
    layout: &ListLayout,
    list: &crate::pages::ListHero,
    tools: &[crate::pages::HeroTool],
    focused: bool,
) {
    let c = palette::colours();
    let font = egui::FontId::new(11.0, egui::FontFamily::Name(PROFONT.into()));
    let small = egui::FontId::new(9.0, egui::FontFamily::Name(PROFONT.into()));
    painter.text(
        egui::pos2(layout.list.right(), layout.list.top() - 13.0),
        egui::Align2::RIGHT_TOP,
        &list.title,
        small.clone(),
        c.dim,
    );
    painter.line_segment(
        [
            egui::pos2(layout.list.right() + 7.0, layout.list.top()),
            egui::pos2(layout.list.right() + 7.0, layout.list.bottom()),
        ],
        egui::Stroke::new(1.0, c.edge),
    );
    let lines = list_lines(&list.rows);
    let first = layout.window(&lines, list.selected);
    let body = painter.with_clip_rect(painter.clip_rect().intersect(layout.list));
    for (slot, line) in lines.iter().skip(first).take(layout.visible()).enumerate() {
        let rect = layout.line_rect(slot);
        let at = match line {
            ListLine::Head(group) => {
                body.text(
                    rect.left_top() + egui::vec2(1.0, 2.0),
                    egui::Align2::LEFT_TOP,
                    group.to_uppercase(),
                    small.clone(),
                    c.dim,
                );
                continue;
            }
            ListLine::Row(at) => *at,
        };
        let Some(row) = list.rows.get(at) else {
            continue;
        };
        let chosen = at == list.selected;
        if chosen && focused {
            // The app's own cursor, so the keys are visibly HERE and not
            // on the cell strip below.
            crate::ui::nav_cursor::claim(
                painter,
                ("hero-row", at),
                rect,
                crate::ui::nav_cursor::Kind::Row,
                crate::ui::nav_cursor::Layer::Overlay,
                c.bright,
            );
        }
        if chosen {
            body.rect_filled(rect, 0.0, c.bright.linear_multiply(0.16));
            body.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, c.bright),
                egui::StrokeKind::Inside,
            );
        }
        let ink = if chosen { c.bright } else { c.fg };
        body.text(
            rect.left_top() + egui::vec2(22.0, 1.0),
            egui::Align2::LEFT_TOP,
            row.name,
            font.clone(),
            ink,
        );
        if !row.tags.is_empty() {
            // Which oscillators are on this row, in the margin. The
            // same size as the name: it is read as often.
            body.text(
                rect.left_top() + egui::vec2(3.0, 1.0),
                egui::Align2::LEFT_TOP,
                &row.tags,
                font.clone(),
                if chosen { c.bright } else { c.select },
            );
        }
        body.text(
            rect.right_top() + egui::vec2(-2.0, 3.0),
            egui::Align2::RIGHT_TOP,
            &row.detail,
            small.clone(),
            c.dim,
        );
    }
    if lines.len() > layout.visible() {
        // A list longer than the panel says so, rather than pretending
        // the rows below do not exist.
        body.text(
            layout.list.right_bottom() + egui::vec2(-2.0, -11.0),
            egui::Align2::RIGHT_TOP,
            format!("{}/{}", list.selected + 1, list.rows.len()),
            small.clone(),
            c.dim,
        );
    }
    let width = layout.tools.width() / tools.len().max(1) as f32;
    for (at, tool) in tools.iter().enumerate() {
        painter.text(
            layout.tools.min + egui::vec2(at as f32 * width + 2.0, 4.0),
            egui::Align2::LEFT_TOP,
            format!("{} {}", tool.key.name(), tool.word),
            small.clone(),
            c.dim,
        );
    }
}

/// The picture on the hero band: axes labelled at the corners, marks as
/// dashed verticals, every series a line and the lit ones washed to the
/// baseline strip by strip (a convex fill of a concave curve would be a
/// wedge), corner marks on the live curve.
fn draw_hero(
    painter: &egui::Painter,
    plot: egui::Rect,
    hero: &crate::pages::Hero,
    focus: Option<u32>,
) {
    if let Some(wave) = &hero.waveform {
        draw_wave_hero(painter, plot, &hero.title, wave, focus);
        return;
    }
    let c = palette::colours();
    let small = egui::FontId::new(9.0, egui::FontFamily::Name(PROFONT.into()));
    // The caption owns the top-left; the picture's own title sits at
    // the top-right and the curves keep clear of both.
    painter.text(
        egui::pos2(plot.right() - 2.0, plot.top() + 1.0),
        egui::Align2::RIGHT_TOP,
        &hero.title,
        small.clone(),
        c.dim,
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(plot.left() + 2.0, plot.top() + 14.0),
        egui::pos2(plot.right() - 2.0, plot.bottom() - 1.0),
    );
    if !inner.is_positive() {
        return;
    }
    let at = |x: f32, y: f32| {
        egui::pos2(
            inner.left() + x.clamp(0.0, 1.0) * inner.width(),
            inner.bottom() - y.clamp(0.0, 1.0) * inner.height(),
        )
    };
    // The curves first, so the labels and marks read over the wash.
    // A picture with nothing lit is lit whole: the selected cell is a
    // level, or its segment has no width, and the shape still matters.
    let all_lit = !hero.series.iter().any(|series| series.lit);
    for series in &hero.series {
        if series.points.len() < 2 {
            continue;
        }
        let points: Vec<egui::Pos2> = series.points.iter().map(|(x, y)| at(*x, *y)).collect();
        if series.lit || all_lit {
            for pair in points.windows(2) {
                let wash = egui::Shape::convex_polygon(
                    vec![
                        egui::pos2(pair[0].x, inner.bottom()),
                        pair[0],
                        pair[1],
                        egui::pos2(pair[1].x, inner.bottom()),
                    ],
                    c.select,
                    egui::Stroke::NONE,
                );
                painter.add(wash);
            }
            painter.add(egui::Shape::line(
                points.clone(),
                egui::Stroke::new(1.5, c.bright),
            ));
            for end in [points[0], points[points.len() - 1]] {
                painter.line_segment(
                    [end + egui::vec2(0.0, -3.0), end + egui::vec2(0.0, 3.0)],
                    egui::Stroke::new(1.0, c.bright),
                );
            }
        } else {
            painter.add(egui::Shape::line(points, egui::Stroke::new(1.0, c.dim)));
        }
    }
    // Axis labels at the corners, in the margin the curve leaves.
    painter.text(
        egui::pos2(inner.left() + 2.0, inner.top()),
        egui::Align2::LEFT_TOP,
        &hero.y_labels[1],
        small.clone(),
        c.dim,
    );
    painter.text(
        egui::pos2(inner.left() + 2.0, inner.bottom() - 1.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{} · {}", hero.y_labels[0], hero.x_labels[0]),
        small.clone(),
        c.dim,
    );
    painter.text(
        egui::pos2(inner.right() - 2.0, inner.bottom() - 1.0),
        egui::Align2::RIGHT_BOTTOM,
        &hero.x_labels[1],
        small.clone(),
        c.dim,
    );
    if hero.diagonal {
        painter.extend(egui::Shape::dashed_line(
            &[at(0.0, 0.0), at(1.0, 1.0)],
            egui::Stroke::new(1.0, c.edge),
            3.0,
            3.0,
        ));
    }
    for mark in &hero.marks {
        let ink = if mark.lit { c.fg } else { c.edge };
        painter.extend(egui::Shape::dashed_line(
            &[at(mark.x, 0.0), at(mark.x, 1.0)],
            egui::Stroke::new(1.0, ink),
            2.0,
            3.0,
        ));
        let x = at(mark.x, 0.0).x;
        let align = if mark.x > 0.85 {
            egui::Align2::RIGHT_BOTTOM
        } else {
            egui::Align2::LEFT_BOTTOM
        };
        painter.text(
            egui::pos2(
                x + if mark.x > 0.85 { -2.0 } else { 2.0 },
                inner.bottom() - 11.0,
            ),
            align,
            &mark.label,
            small.clone(),
            if mark.lit { c.fg } else { c.dim },
        );
    }
}

const PATTERN_WINDOWS: usize = crate::sequencing::PATTERN_STEPS / 16;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_hero_keeps_the_original_window_and_cells() {
        let field = egui::Rect::from_min_size(egui::pos2(30.0, 70.0), egui::vec2(1200.0, 600.0));
        let layout = WindowLayout::of(field, 8.0, 22.0, 0.0);
        assert_eq!(layout.rect.min, egui::pos2(38.0, 78.0));
        assert_eq!(layout.rect.size(), egui::vec2(1184.0, 192.0));
        assert_eq!(layout.hero.height(), 0.0);
        assert_eq!(layout.cells.top(), layout.title.bottom());
        assert_eq!(layout.cells.height(), 170.0);
    }

    #[test]
    fn hero_grows_the_window_without_resizing_cells_or_passing_two_thirds_of_the_field() {
        for height in [240.0, 480.0, 600.0, 900.0] {
            let field =
                egui::Rect::from_min_size(egui::pos2(30.0, 70.0), egui::vec2(1200.0, height));
            let original = WindowLayout::of(field, 8.0, 22.0, 0.0);
            for requested in [1.0, 96.0, 240.0] {
                let layout = WindowLayout::of(field, 8.0, 22.0, requested);
                assert_eq!(layout.title, original.title);
                assert!((layout.cells.height() - original.cells.height()).abs() < 0.001);
                assert_eq!(layout.hero.height(), requested.min(height / 3.0));
                assert_eq!(layout.hero.top(), layout.title.bottom());
                assert_eq!(layout.cells.top(), layout.hero.bottom());
                assert_eq!(layout.cells.bottom(), layout.rect.bottom());
                // A third for the cells, a third for the picture: the window
                // never passes two thirds of the field.
                assert!(layout.rect.bottom() <= field.top() + field.height() * 2.0 / 3.0 + 0.001);
            }
        }
    }
}

/// Taller material views keep the same eight-cell footer and expand only the
/// picture. Geometry is shared by paint and hit testing.
impl Stage {
    fn deck_window_layout(&self, field: egui::Rect) -> WindowLayout {
        if self.deck_hero_height() != crate::pages::HeroHeight::Tall {
            return WindowLayout::of(
                field,
                crate::tune!(WINDOW_INSET),
                crate::tune!(WINDOW_TITLE_H),
                crate::tune!(HERO_H),
            );
        }
        let inset = crate::tune!(WINDOW_INSET);
        let title_h = crate::tune!(WINDOW_TITLE_H);
        let footer = if field.width() < 976.0 {
            144.0_f32
        } else {
            96.0_f32
        }
        .min(field.height() * 0.4);
        let hero_h =
            (field.height() * 0.64).min((field.height() - footer - title_h - 2.0 * inset).max(0.0));
        let rect = egui::Rect::from_min_size(
            field.min + egui::vec2(inset, inset),
            egui::vec2(
                (field.width() - 2.0 * inset).max(0.0),
                title_h + hero_h + footer,
            ),
        );
        let title = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), title_h));
        let hero = egui::Rect::from_min_size(title.left_bottom(), egui::vec2(rect.width(), hero_h));
        let cells = egui::Rect::from_min_max(hero.left_bottom(), rect.max);
        WindowLayout {
            rect,
            title,
            hero,
            cells,
        }
    }
    pub(super) fn interact_deck_hero(&mut self, ui: &mut egui::Ui, field: egui::Rect) {
        if !self.deck_open() {
            return;
        }
        let Some(hero) = self.deck_hero() else {
            return;
        };
        if let Some(list) = self.deck_hero_list() {
            let layout = ListLayout::of(self.deck_wave_rect(field));
            let lines = list_lines(&list.rows);
            let first = layout.window(&lines, list.selected);
            for (slot, line) in lines.iter().skip(first).take(layout.visible()).enumerate() {
                let ListLine::Row(row) = line else { continue };
                let response = ui
                    .interact(
                        layout.line_rect(slot),
                        egui::Id::new(("hero-row", *row)),
                        egui::Sense::click(),
                    )
                    .affords(Affords::Press);
                if response.clicked() {
                    let _ = self.hero_pick_row(*row);
                }
            }
            let tools = self.deck_hero_tools();
            let width = layout.tools.width() / tools.len().max(1) as f32;
            for (at, tool) in tools.iter().enumerate() {
                let cell = egui::Rect::from_min_size(
                    layout.tools.min + egui::vec2(at as f32 * width, 0.0),
                    egui::vec2(width, layout.tools.height()),
                );
                if ui
                    .interact(
                        cell,
                        egui::Id::new(("hero-tool", tool.verb)),
                        egui::Sense::click(),
                    )
                    .affords(Affords::Press)
                    .clicked()
                {
                    let _ = self.apply(crate::ui::stage::StageIntent::HeroTool(tool.verb));
                }
            }
            return;
        }
        let Some(wave) = hero.waveform else {
            return;
        };
        let rect = self.deck_wave_rect(field);
        let layout = WaveLayout::of(rect);
        let span = (wave.view.1 - wave.view.0).max(1e-9);
        for handle in &wave.handles {
            let x = layout.wave.left() + (handle.at - wave.view.0) / span * layout.wave.width();
            if x < layout.wave.left() || x > layout.wave.right() {
                continue;
            }
            let hit = egui::Rect::from_min_max(
                egui::pos2(x - 8.0, layout.wave.top()),
                egui::pos2(x + 8.0, layout.wave.bottom()),
            );
            let response = ui
                .interact(
                    hit,
                    egui::Id::new(("hero-handle", self.deck_sample_device(), handle.param)),
                    egui::Sense::click_and_drag(),
                )
                .affords(Affords::Sweep);
            if response.dragged()
                && let Some(pointer) = response.interact_pointer_pos()
            {
                let fraction = wave.view.0
                    + (pointer.x - layout.wave.left()) / layout.wave.width().max(1.0) * span;
                let _ = self.hero_set_handle(handle.param, fraction);
            }
            if response.drag_stopped() {
                self.finish_hero_gesture();
            }
        }
        // Compact contextual tools. The codebook carries the complete list.
        let tools = self.deck_hero_tools();
        let width = layout.tools.width() / 6.0;
        for (i, tool) in tools.iter().take(12).enumerate() {
            let cell = egui::Rect::from_min_size(
                layout.tools.min + egui::vec2((i % 6) as f32 * width, (i / 6) as f32 * 20.0),
                egui::vec2(width, 20.0),
            );
            if ui
                .interact(
                    cell,
                    egui::Id::new(("hero-tool", tool.verb)),
                    egui::Sense::click(),
                )
                .affords(Affords::Press)
                .clicked()
            {
                let _ = self.apply(crate::ui::stage::StageIntent::HeroTool(tool.verb));
            }
        }
    }
}

pub(super) struct WaveLayout {
    overview: egui::Rect,
    pub(super) wave: egui::Rect,
    pub(super) tools: egui::Rect,
}
impl WaveLayout {
    pub(super) fn of(plot: egui::Rect) -> Self {
        let overview = egui::Rect::from_min_max(
            plot.min + egui::vec2(2.0, 22.0),
            egui::pos2(plot.right() - 2.0, plot.top() + 46.0),
        );
        let tools = egui::Rect::from_min_max(
            egui::pos2(plot.left(), plot.bottom() - 40.0),
            plot.right_bottom(),
        );
        let wave = egui::Rect::from_min_max(
            overview.left_bottom() + egui::vec2(0.0, 10.0),
            egui::pos2(overview.right(), tools.top() - 34.0),
        );
        Self {
            overview,
            wave,
            tools,
        }
    }
}
fn draw_wave_hero(
    painter: &egui::Painter,
    plot: egui::Rect,
    title: &str,
    data: &crate::pages::WaveHero,
    focus: Option<u32>,
) {
    let c = palette::colours();
    let layout = WaveLayout::of(plot);
    let font = egui::FontId::new(11.0, egui::FontFamily::Name(PROFONT.into()));
    painter.text(
        egui::pos2(plot.right() - 2.0, plot.top() + 1.0),
        egui::Align2::RIGHT_TOP,
        title,
        font.clone(),
        c.fg,
    );
    if !layout.wave.is_positive() {
        return;
    }
    let mini = painter.with_clip_rect(layout.overview);
    wave_columns(&mini, layout.overview, &data.overview, c.dim, c.edge);
    let view = egui::Rect::from_min_max(
        egui::pos2(
            layout.overview.left() + data.view.0 * layout.overview.width(),
            layout.overview.top(),
        ),
        egui::pos2(
            layout.overview.left() + data.view.1 * layout.overview.width(),
            layout.overview.bottom(),
        ),
    );
    mini.rect_filled(view, 0.0, c.select.linear_multiply(0.35));
    mini.rect_stroke(
        view,
        0.0,
        egui::Stroke::new(1.0, c.dim),
        egui::StrokeKind::Inside,
    );
    let span = (data.view.1 - data.view.0).max(1e-9);
    let x = |f: f32| layout.wave.left() + (f - data.view.0) / span * layout.wave.width();
    let body = painter.with_clip_rect(layout.wave);
    body.line_segment(
        [layout.wave.left_center(), layout.wave.right_center()],
        egui::Stroke::new(1.0, c.rule),
    );
    for q in 0..=4 {
        let at = layout.wave.left() + layout.wave.width() * q as f32 / 4.0;
        body.line_segment(
            [
                egui::pos2(at, layout.wave.top()),
                egui::pos2(at, layout.wave.bottom()),
            ],
            egui::Stroke::new(1.0, c.edge),
        );
    }
    wave_columns(&body, layout.wave, &data.columns, c.dim, c.edge);
    let selection = data.loop_region.unwrap_or(data.region);
    let selected = egui::Rect::from_min_max(
        egui::pos2(x(selection.0), layout.wave.top()),
        egui::pos2(x(selection.1), layout.wave.bottom()),
    );
    body.rect_filled(selected, 0.0, c.bright.linear_multiply(0.08));
    wave_columns(
        &body.with_clip_rect(selected.intersect(layout.wave)),
        layout.wave,
        &data.columns,
        c.bright,
        c.bright.linear_multiply(0.3),
    );
    if let Some(param) = focus
        && let Some(handle) = data.handles.iter().find(|handle| handle.param == param)
    {
        let at = x(handle.at);
        crate::ui::nav_cursor::claim(
            painter,
            ("hero-handle", param),
            egui::Rect::from_min_max(
                egui::pos2(at - 7.0, layout.wave.top()),
                egui::pos2(at + 7.0, layout.wave.bottom()),
            ),
            crate::ui::nav_cursor::Kind::Cell,
            crate::ui::nav_cursor::Layer::Overlay,
            c.bright,
        );
    }
    for (i, at) in data.slices.iter().enumerate() {
        let at = x(*at);
        body.line_segment(
            [
                egui::pos2(at, layout.wave.top()),
                egui::pos2(at, layout.wave.top() + 8.0),
            ],
            egui::Stroke::new(1.0, c.dim),
        );
        if i % 2 == 0 || data.slices.len() < 16 {
            body.text(
                egui::pos2(at + 3.0, layout.wave.top() + 12.0),
                egui::Align2::LEFT_TOP,
                format!("{:02}", i + 1),
                font.clone(),
                c.dim,
            );
        }
    }
    if let Some((a, b)) = data.loop_region {
        let xa = x(a);
        let xb = x(b);
        let fade = (xb - xa) * data.fade;
        for (left, right) in [(xa, xa + fade), (xb, xb - fade)] {
            let points = vec![
                egui::pos2(left, layout.wave.bottom()),
                egui::pos2(right, layout.wave.top()),
                egui::pos2(right, layout.wave.bottom()),
            ];
            body.add(egui::Shape::convex_polygon(
                points,
                c.bright.linear_multiply(0.1),
                egui::Stroke::NONE,
            ));
            body.line_segment(
                [
                    egui::pos2(left, layout.wave.bottom()),
                    egui::pos2(right, layout.wave.top()),
                ],
                egui::Stroke::new(1.0, c.bright.linear_multiply(0.6)),
            );
        }
    }
    for (i, handle) in data.handles.iter().enumerate() {
        let at = x(handle.at);
        let reach = if i == 0 { 8.0 } else { -8.0 };
        body.add(egui::Shape::line(
            vec![
                egui::pos2(at + reach, layout.wave.top() + 1.0),
                egui::pos2(at, layout.wave.top() + 1.0),
                egui::pos2(at, layout.wave.bottom() - 1.0),
                egui::pos2(at + reach, layout.wave.bottom() - 1.0),
            ],
            egui::Stroke::new(2.0, c.bright),
        ));
    }
    painter.text(
        egui::pos2(layout.wave.left(), layout.wave.bottom() + 6.0),
        egui::Align2::LEFT_TOP,
        format!("{:.2}%", data.view.0 * 100.0),
        font.clone(),
        c.dim,
    );
    painter.text(
        egui::pos2(layout.wave.right(), layout.wave.bottom() + 6.0),
        egui::Align2::RIGHT_TOP,
        format!("{:.2}%", data.view.1 * 100.0),
        font.clone(),
        c.dim,
    );
    painter.text(
        egui::pos2(layout.wave.center().x, layout.wave.bottom() + 6.0),
        egui::Align2::CENTER_TOP,
        &data.detail,
        font.clone(),
        c.fg,
    );
    for (at, ghost) in &data.playheads {
        let at = x(*at);
        body.line_segment(
            [
                egui::pos2(at, layout.wave.top()),
                egui::pos2(at, layout.wave.bottom()),
            ],
            egui::Stroke::new(
                if *ghost { 1.0 } else { 2.0 },
                if *ghost { c.dim } else { c.bright },
            ),
        );
    }
    for (i, word) in data.tools.iter().take(12).enumerate() {
        painter.text(
            layout.tools.min
                + egui::vec2(
                    (i % 6) as f32 * layout.tools.width() / 6.0,
                    4.0 + (i / 6) as f32 * 20.0,
                ),
            egui::Align2::LEFT_TOP,
            word,
            font.clone(),
            c.dim,
        );
    }
}
fn wave_columns(
    painter: &egui::Painter,
    rect: egui::Rect,
    columns: &[crate::sample_peaks::Bin],
    ink: egui::Color32,
    wash: egui::Color32,
) {
    let width = rect.width() / columns.len().max(1) as f32;
    for (i, bin) in columns.iter().enumerate() {
        let x = rect.left() + i as f32 * width;
        let cy = rect.center().y;
        let amp = rect.height() * 0.44;
        painter.line_segment(
            [
                egui::pos2(x, cy - bin.max.clamp(-1.0, 1.0) * amp),
                egui::pos2(x, cy - bin.min.clamp(-1.0, 1.0) * amp),
            ],
            egui::Stroke::new(width.max(0.7), wash),
        );
        painter.line_segment(
            [
                egui::pos2(x, cy - bin.rms.min(1.0) * amp),
                egui::pos2(x, cy + bin.rms.min(1.0) * amp),
            ],
            egui::Stroke::new(width.max(0.7), ink),
        );
    }
}
