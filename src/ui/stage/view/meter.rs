//! The meter, painted: rain that falls upward.
//!
//! The section's whole visual idea is one inversion. In the rain
//! everyone knows, a bright head falls and drags a fading tail behind
//! it; here the bright line STANDS STILL — it is the present, and the
//! present does not move — and the data rises through it, brightening
//! as it approaches and fading fast once it is past. What is coming
//! stays legible a long way down, because a reader watching a sequencer
//! is reading ahead; what has gone dims within a few steps, because it
//! is gone. Nothing blinks and nothing is on a timer of its own: every
//! brightness on this surface is a function of one number, the distance
//! from the now-line, and the whole field moves by a fraction of a row
//! each frame so the motion is a glide rather than eight jumps a
//! second.
//!
//! The inks are the console's, in their own roles and no new ones: the
//! now-line and its brackets in chassis, a note in fg, a velocity in
//! dim, a lock in label, a slide in nominal, a conditional trig in
//! alert, a sound lock in dir, the bar accents in edge. Colour never
//! carries a fact that the position and the text do not already carry —
//! it only says which KIND of fact this is.

use super::heads;
use super::{chassis, palette};
use crate::PROFONT;
use crate::ui::stage::meter::{Cell, Field, Plan};
use eframe::egui;
use egui::Color32;

/// The input panel's width: the stream of answered verbs.
/// @tune 120..360 px
const PANEL_W: f32 = 208.0;
/// One row of the body, and one line of the panel.
/// @tune 9..24 px
const ROW_H: f32 = 15.0;
/// The column heads above the body.
/// @tune 20..72 px
const HEAD_H: f32 = 38.0;
/// The readout band under it: the focused track's step, in full.
/// @tune 0..96 px
const READOUT_H: f32 = 46.0;
const TITLE_H: f32 = 22.0;
const LEGEND_H: f32 = 18.0;
const TYPE_PX: f32 = 12.0;
/// The type the panel and the cells are set in.
/// @tune 8..16 px
const CELL_PX: f32 = 11.0;
/// Where the now-line sits in the body, as a fraction from the top.
/// Low, because the reader of a sequencer is reading AHEAD: the room
/// below the line is what is about to be played, and it is worth more
/// than the room above it.
/// @tune 0.1..0.8
const NOW_AT: f32 = 0.26;
/// How fast a row dims once it is past, in rows. Short: it is gone.
/// @tune 1..24 rows
const PAST_FADE: f32 = 6.5;
/// How fast a row brightens as it comes, in rows. Long: it is coming.
/// @tune 4..80 rows
const FUTURE_FADE: f32 = 22.0;
/// The dimmest anything on the body is drawn at.
/// @tune 0..90
const FLOOR: f32 = 26.0;
/// How long a line in the input panel keeps its landing light.
/// @tune 0.05..2 s
const FLASH_S: f32 = 0.45;
/// How long an answered verb takes to fade to the panel's floor.
/// @tune 2..120 s
const PANEL_FADE_S: f32 = 24.0;
/// The most lock marks a narrow cell draws before it starts counting.
const LOCK_MARKS: usize = 3;
/// The mark itself.
const MARK: f32 = 3.0;

fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a.clamp(0.0, 255.0) as u8)
}

/// How brightly a row `d` steps from the now-line is drawn, `0..1`.
///
/// Two different exponentials meeting at 1.0 on the line, and the
/// asymmetry between them IS the section: the past is a short tail, the
/// future a long approach. One curve for both would be a scope; this is
/// a playhead.
fn weight(d: f32) -> f32 {
    if d <= 0.0 {
        (d / PAST_FADE).exp()
    } else {
        (-d / FUTURE_FADE).exp()
    }
}

/// The ink a row is drawn in, at `weight`.
fn ink(c: Color32, w: f32) -> Color32 {
    alpha(c, FLOOR + (255.0 - FLOOR) * w)
}

/// The keys the section answers to, for its foot.
const LEGEND: [(&str, &str); 10] = [
    ("arrows", "cursor"),
    ("+arrows", "block"),
    ("- =", "turn"),
    ("a-g", "note"),
    ("+a-g", "chord"),
    ("T", "trig"),
    ("S", "slide"),
    ("Del", "clear"),
    (", .", "+lock"),
    ("Esc", "leave"),
];

impl super::super::Stage {
    pub(super) fn draw_meter(&self, painter: &egui::Painter, field: egui::Rect) {
        if !self.meter_open() {
            return;
        }
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let small = egui::FontId::new(CELL_PX, egui::FontFamily::Name(PROFONT.into()));
        let ch = TYPE_PX * 0.6;
        let cell_ch = CELL_PX * 0.6;
        let margin = heads::margin();
        let room = egui::Rect::from_min_max(
            egui::pos2(field.min.x + margin, field.min.y + margin),
            egui::pos2(field.max.x - margin, field.max.y - margin),
        );

        let (now, glide) = self.meter_now();
        let plan = self.meter_plan();

        // ------------------------------------------------------ title
        let title =
            egui::Rect::from_min_max(room.min, egui::pos2(room.max.x, room.min.y + TITLE_H));
        self.draw_meter_title(painter, title, &font, ch, now, plan.columns.len());

        // ------------------------------------------------------ frame
        let work = egui::Rect::from_min_max(
            egui::pos2(room.min.x, title.max.y + 4.0),
            egui::pos2(room.max.x, room.max.y - LEGEND_H),
        );
        let panel = egui::Rect::from_min_max(
            work.min,
            egui::pos2((work.min.x + PANEL_W).min(work.max.x), work.max.y),
        );
        let tracker =
            egui::Rect::from_min_max(egui::pos2(panel.max.x + 12.0, work.min.y), work.max);
        // The one rule between the two readings. A splice rather than a
        // border: they are two columns of one instrument, not two panes.
        painter.line_segment(
            [
                egui::pos2(panel.max.x + 6.0, work.min.y),
                egui::pos2(panel.max.x + 6.0, work.max.y),
            ],
            egui::Stroke::new(1.0, alpha(c.edge, 150.0)),
        );

        self.draw_meter_panel(painter, panel, &small, cell_ch);
        if tracker.width() > 80.0 {
            self.draw_meter_body(painter, tracker, &small, cell_ch, &plan, now, glide);
        }

        // ----------------------------------------------------- legend
        let ly = room.max.y - LEGEND_H * 0.5;
        let mut lx = room.min.x;
        for (chord, word) in LEGEND {
            let width = (chord.chars().count() + word.chars().count()) as f32 * ch + 3.5 * ch;
            if lx + width > room.max.x {
                break;
            }
            painter.text(
                egui::pos2(lx, ly),
                egui::Align2::LEFT_CENTER,
                chord,
                font.clone(),
                c.bright,
            );
            lx += (chord.chars().count() as f32 + 1.0) * ch;
            painter.text(
                egui::pos2(lx, ly),
                egui::Align2::LEFT_CENTER,
                word,
                font.clone(),
                c.dim,
            );
            lx += (word.chars().count() as f32 + 2.5) * ch;
        }
    }

    /// The title row: what the section is, where the song is, and
    /// whether the body is riding the transport or being held.
    fn draw_meter_title(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        font: &egui::FontId,
        ch: f32,
        now: isize,
        tracks: usize,
    ) {
        let c = palette::colours();
        let y = rect.center().y;
        let mut x = rect.min.x;
        painter.text(
            egui::pos2(x, y),
            egui::Align2::LEFT_CENTER,
            "METER //",
            font.clone(),
            c.label,
        );
        x += 10.0 * ch;
        let per_bar = self.meter_steps_per_bar().max(1) as isize;
        let bar = now.div_euclid(per_bar) + 1;
        let in_bar = now.rem_euclid(per_bar);
        painter.text(
            egui::pos2(x, y),
            egui::Align2::LEFT_CENTER,
            &format!("bar {bar} · {}", in_bar / 4 + 1),
            font.clone(),
            c.dir,
        );
        x += 14.0 * ch;
        painter.text(
            egui::pos2(x, y),
            egui::Align2::LEFT_CENTER,
            &format!("step {now}"),
            font.clone(),
            c.dim,
        );
        x += 12.0 * ch;
        painter.text(
            egui::pos2(x, y),
            egui::Align2::LEFT_CENTER,
            &format!("{tracks} tracks"),
            font.clone(),
            c.dim,
        );
        // The one thing about this section that can surprise its reader:
        // whether what they are looking at is the present.
        let (word, held, colour) = match self.meter_hold() {
            None => ("FOLLOWING", String::new(), c.nominal),
            Some(by) => ("HELD", format!(" {by:+}"), c.alert),
        };
        let text = format!("{word}{held}");
        painter.text(
            egui::pos2(rect.max.x, y),
            egui::Align2::RIGHT_CENTER,
            &text,
            font.clone(),
            colour,
        );
    }

    /// The input panel: the verbs the stage answered, newest at the
    /// foot, rising and fading as they age.
    fn draw_meter_panel(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        font: &egui::FontId,
        ch: f32,
    ) {
        let c = palette::colours();
        let uptime = self.meter_uptime();
        painter.text(
            egui::pos2(rect.min.x, rect.min.y + ROW_H * 0.5),
            egui::Align2::LEFT_CENTER,
            "INPUT",
            font.clone(),
            c.label,
        );
        painter.text(
            egui::pos2(rect.max.x, rect.min.y + ROW_H * 0.5),
            egui::Align2::RIGHT_CENTER,
            "answered",
            font.clone(),
            alpha(c.dim, 140.0),
        );
        let top = rect.min.y + ROW_H * 1.4;
        let painter = painter.with_clip_rect(egui::Rect::from_min_max(
            egui::pos2(rect.min.x, top),
            rect.max,
        ));
        let room = ((rect.max.y - top) / ROW_H).floor().max(0.0) as usize;
        // Newest at the foot: the line just answered lands at the bottom
        // and pushes its elders up, which is the same direction the
        // steps travel. Two streams, one motion.
        for (from_foot, line) in self.meter_lines().rev().take(room).enumerate() {
            let y = rect.max.y - ROW_H * (from_foot as f32 + 0.5);
            let age = (uptime - line.at).max(0.0);
            // Two fades multiplied: how long ago it happened, and how
            // far up the panel it has been pushed. A verb answered a
            // moment ago near the top is still recent, and still dim —
            // which is the honest reading, because it is about to leave.
            let by_age = 1.0 - (age / PANEL_FADE_S).clamp(0.0, 1.0).powf(0.6);
            let by_place = 1.0 - (from_foot as f32 / room.max(1) as f32).powf(0.8);
            let w = (by_age * 0.55 + by_place * 0.45).clamp(0.0, 1.0);
            // The landing light: the newest line is lit for a moment,
            // and the light decays rather than switching off.
            if from_foot == 0 && age < FLASH_S {
                let flash = 1.0 - age / FLASH_S;
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(rect.min.x - 2.0, y - ROW_H * 0.5),
                        egui::pos2(rect.max.x, y + ROW_H * 0.5),
                    ),
                    1.0,
                    alpha(c.select, 90.0 * flash),
                );
            }
            let word_ink = if line.refused { c.alert } else { c.fg };
            painter.text(
                egui::pos2(rect.min.x, y),
                egui::Align2::LEFT_CENTER,
                line.family,
                font.clone(),
                ink(c.label, w * 0.7),
            );
            let indent = rect.min.x + 7.0 * ch;
            // The word is clipped by room, not by an ellipsis: a verb
            // cut short still reads, and a row of dots does not.
            painter.text(
                egui::pos2(indent, y),
                egui::Align2::LEFT_CENTER,
                line.word,
                font.clone(),
                ink(word_ink, w),
            );
            if line.count > 1 {
                painter.text(
                    egui::pos2(rect.max.x, y),
                    egui::Align2::RIGHT_CENTER,
                    &format!("×{}", line.count),
                    font.clone(),
                    ink(c.chassis, w),
                );
            }
        }
    }

    /// The body: the column heads, the rows rising through the
    /// now-line, the cursor, and the block if one is drawn.
    #[allow(clippy::too_many_arguments)]
    fn draw_meter_body(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        font: &egui::FontId,
        ch: f32,
        plan: &Plan,
        now: isize,
        glide: f32,
    ) {
        let c = palette::colours();
        let gutter = heads::gutter();
        let heads_rect =
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + HEAD_H));
        let readout =
            egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - READOUT_H), rect.max);
        let body = egui::Rect::from_min_max(
            egui::pos2(rect.min.x, heads_rect.max.y),
            egui::pos2(rect.max.x, readout.min.y - 6.0),
        );
        let lane = egui::Rect::from_min_max(egui::pos2(rect.min.x + gutter, body.min.y), body.max);

        // Where every column stands, and how wide. Measured once, and
        // the heads, the rows, the cursor and the block all read the
        // same measurement — a second one would let the lit cell drift
        // off the value it is lighting.
        let cursor = self.meter_at();
        let lay = Lay::of(plan, ch, lane.width(), cursor);

        let rows = ((body.height() / ROW_H).ceil() as usize).saturating_add(2);
        let above = (body.height() * NOW_AT / ROW_H).round() as isize;
        let now_y = body.min.y + body.height() * NOW_AT;
        let derived = self.meter_rows(now - above, rows);
        let clip = painter.with_clip_rect(body);

        // The present, as a place rather than as a highlight.
        clip.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(body.min.x, now_y - ROW_H * 0.5),
                egui::pos2(body.max.x, now_y + ROW_H * 0.5),
            ),
            1.0,
            alpha(c.select, 46.0),
        );

        // The block, under everything: a ground, so the values inside it
        // are read normally rather than through a colour.
        if let Some((from_row, from_at)) = self.meter_anchor() {
            let (lo, hi) = (from_row.min(now), from_row.max(now));
            let (first, last) = (from_at.min(cursor), from_at.max(cursor));
            let top = now_y + ((lo - now) as f32 - glide - 0.5) * ROW_H;
            let bottom = now_y + ((hi - now) as f32 - glide + 0.5) * ROW_H;
            if let (Some(left), Some(right)) = (lay.x_of(first), lay.x_of(last)) {
                let block = egui::Rect::from_min_max(
                    egui::pos2(lane.min.x + left - 2.0, top),
                    egui::pos2(lane.min.x + right + lay.width_of(last) + 2.0, bottom),
                );
                clip.rect_filled(block, 1.0, alpha(c.select, 78.0));
                chassis::frame(&clip, block, true);
            }
        }

        for (at, row) in derived.iter().enumerate() {
            let d = (at as isize - above) as f32 - glide;
            let y = now_y + d * ROW_H;
            if y < body.min.y - ROW_H || y > body.max.y + ROW_H {
                continue;
            }
            let w = weight(d);
            if row.in_bar == 0 {
                clip.line_segment(
                    [
                        egui::pos2(body.min.x, y - ROW_H * 0.5),
                        egui::pos2(body.max.x, y - ROW_H * 0.5),
                    ],
                    egui::Stroke::new(1.0, ink(c.edge, w * 0.8)),
                );
            }
            clip.text(
                egui::pos2(body.min.x + gutter - 6.0, y),
                egui::Align2::RIGHT_CENTER,
                &format!("{:02}", row.in_bar),
                font.clone(),
                ink(if row.in_bar == 0 { c.dir } else { c.dim }, w * 0.9),
            );
            for (index, &(track, field)) in plan.flat.iter().enumerate() {
                let Some(x) = lay.x_of(index) else {
                    continue;
                };
                let cell = row.cells.get(track).and_then(Option::as_ref);
                let lit = index == cursor;
                let at = egui::pos2(lane.min.x + x, y);
                if lit && row.step == now {
                    // The cursor: a lit chamber around the one field an
                    // edit would land on. Drawn per row rather than once,
                    // so it rides with the row it is on while the body
                    // glides underneath it.
                    clip.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(at.x - 3.0, y - ROW_H * 0.5),
                            egui::pos2(at.x + lay.width_of(index) + 1.0, y + ROW_H * 0.5),
                        ),
                        1.0,
                        alpha(c.chassis, 70.0),
                    );
                }
                draw_field(&clip, at, font, ch, cell, field, w, lit, self, track);
            }
        }

        // The now-line, over the rows: the one thing here that is not data.
        clip.line_segment(
            [
                egui::pos2(body.min.x, now_y - ROW_H * 0.5),
                egui::pos2(body.max.x, now_y - ROW_H * 0.5),
            ],
            egui::Stroke::new(1.0, alpha(c.chassis, 210.0)),
        );
        clip.line_segment(
            [
                egui::pos2(body.min.x, now_y + ROW_H * 0.5),
                egui::pos2(body.max.x, now_y + ROW_H * 0.5),
            ],
            egui::Stroke::new(1.0, alpha(c.chassis, 90.0)),
        );
        chassis::brackets(
            &clip,
            egui::Rect::from_min_max(
                egui::pos2(body.min.x, now_y - ROW_H * 0.5),
                egui::pos2(body.max.x, now_y + ROW_H * 0.5),
            ),
            7.0,
        );

        self.draw_meter_heads(painter, heads_rect, font, ch, plan, &lay, gutter);
        self.draw_meter_readout(painter, readout, font, ch, plan, now);
    }

    /// The heads: whose track a run of columns belongs to, and what
    /// each column of it is.
    #[allow(clippy::too_many_arguments)]
    fn draw_meter_heads(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        font: &egui::FontId,
        ch: f32,
        plan: &Plan,
        lay: &Lay,
        gutter: f32,
    ) {
        let c = palette::colours();
        let cursor = self.meter_at();
        let left = rect.min.x + gutter;
        let painter = painter.with_clip_rect(rect);
        let focused = plan.at(cursor).map(|(track, _)| track);
        for (index, &(track, field)) in plan.flat.iter().enumerate() {
            let Some(x) = lay.x_of(index) else {
                continue;
            };
            let Some(column) = plan.columns.get(track) else {
                continue;
            };
            let x = left + x;
            let lit = index == cursor;
            // The track's own head, once, over its first column.
            if plan.start_of(track) == index {
                let name_ink = if column.muted {
                    c.dim
                } else if column.drum {
                    c.drum
                } else {
                    c.fg
                };
                if focused == Some(track) {
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(x - 4.0, rect.min.y + 2.0),
                            egui::pos2(x + lay.track_width(plan, track), rect.max.y - 14.0),
                        ),
                        2.0,
                        alpha(c.select, 70.0),
                    );
                }
                let tag = if column.letter.is_empty() {
                    format!("{}", track + 1)
                } else {
                    column.letter.to_uppercase()
                };
                painter.text(
                    egui::pos2(x, rect.min.y + 9.0),
                    egui::Align2::LEFT_CENTER,
                    &tag,
                    font.clone(),
                    if focused == Some(track) {
                        c.bright
                    } else {
                        c.label
                    },
                );
                painter.text(
                    egui::pos2(x + 3.0 * ch, rect.min.y + 9.0),
                    egui::Align2::LEFT_CENTER,
                    &clipped(&column.name, 16),
                    font.clone(),
                    name_ink,
                );
                painter.text(
                    egui::pos2(x, rect.min.y + 21.0),
                    egui::Align2::LEFT_CENTER,
                    &match &column.clip {
                        Some(tag) => format!("{} · {}", column.machine, tag),
                        None => format!("{} · —", column.machine),
                    },
                    font.clone(),
                    alpha(
                        if column.clip.is_some() {
                            c.nominal
                        } else {
                            c.dim
                        },
                        190.0,
                    ),
                );
            }
            // The column's own name, on the row above the body.
            let head = column
                .fields
                .iter()
                .position(|owned| *owned == field)
                .and_then(|at| column.heads.get(at));
            let word = match field {
                Field::Add => match self.meter_add_param(track) {
                    Some((_, name)) => clipped(name, Field::Add.width()),
                    None => "—".to_owned(),
                },
                _ => head.cloned().unwrap_or_default(),
            };
            painter.text(
                egui::pos2(x, rect.max.y - 7.0),
                egui::Align2::LEFT_CENTER,
                &word,
                font.clone(),
                if lit {
                    c.bright
                } else if matches!(field, Field::Add) {
                    alpha(c.chassis, 170.0)
                } else {
                    alpha(c.label, 190.0)
                },
            );
        }
    }

    /// The readout band: what the cursor is on, in full, and what a key
    /// would do to it.
    #[allow(clippy::too_many_arguments)]
    fn draw_meter_readout(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        font: &egui::FontId,
        ch: f32,
        plan: &Plan,
        now: isize,
    ) {
        let c = palette::colours();
        painter.line_segment(
            [
                egui::pos2(rect.min.x, rect.min.y),
                egui::pos2(rect.max.x, rect.min.y),
            ],
            egui::Stroke::new(1.0, alpha(c.edge, 150.0)),
        );
        let Some((track, field)) = plan.at(self.meter_at()) else {
            return;
        };
        let Some(column) = plan.columns.get(track) else {
            return;
        };
        let row = self.meter_rows(now, 1);
        let cell = row
            .first()
            .and_then(|row| row.cells.get(track))
            .and_then(Option::as_ref);
        let y = rect.min.y + 14.0;
        painter.text(
            egui::pos2(rect.min.x, y),
            egui::Align2::LEFT_CENTER,
            &clipped(&column.name, 18),
            font.clone(),
            if column.drum { c.drum } else { c.dir },
        );
        let x = rect.min.x + 19.0 * ch;
        // What a block edit would land on, said as a count rather than
        // left for the reader to work out from the tinted rectangle.
        if let Some((rows, cols)) = self.meter_block() {
            painter.text(
                egui::pos2(rect.max.x, y),
                egui::Align2::RIGHT_CENTER,
                &format!("BLOCK {rows}×{cols}"),
                font.clone(),
                c.alert,
            );
        }
        let Some(cell) = cell.filter(|cell| cell.marked()) else {
            painter.text(
                egui::pos2(x, y),
                egui::Align2::LEFT_CENTER,
                match field {
                    Field::Note => "no trig here · a letter writes one",
                    Field::Add => "no trig here",
                    _ => "no trig here",
                },
                font.clone(),
                alpha(c.dim, 150.0),
            );
            return;
        };
        let mut words: Vec<(String, Color32)> = Vec::new();
        if let Some((name, velocity)) = &cell.note {
            words.push((name.clone(), c.fg));
            words.push((format!("{velocity:02X}"), c.dim));
        }
        if cell.chord > 0 {
            words.push((format!("+{}", cell.chord), c.bright));
        }
        if !cell.sounding {
            words.push(("off".to_owned(), c.dim));
        }
        if let Some((a, b)) = cell.cond {
            words.push((format!("{a}:{b}"), c.alert));
        }
        if let Some(chance) = cell.chance {
            words.push((format!("{chance}%"), c.alert));
        }
        if let Some((rate, count)) = cell.retrig {
            words.push((format!("×{count}/{rate}"), c.bright));
        }
        if cell.micro != 0 {
            words.push((format!("{:+}", cell.micro), c.chassis));
        }
        if let Some(sound) = &cell.sound {
            words.push((clipped(sound, 12), c.dir));
        }
        let mut wx = x;
        for (word, colour) in words {
            painter.text(
                egui::pos2(wx, y),
                egui::Align2::LEFT_CENTER,
                &word,
                font.clone(),
                colour,
            );
            wx += (word.chars().count() as f32 + 1.5) * ch;
        }
        let mut lx = rect.min.x;
        let ly = y + 15.0;
        if cell.locks.is_empty() {
            painter.text(
                egui::pos2(lx, ly),
                egui::Align2::LEFT_CENTER,
                "no locks",
                font.clone(),
                alpha(c.dim, 130.0),
            );
            return;
        }
        for lock in &cell.locks {
            let text = format!("{} {}", lock.name, lock.reading);
            let width = (text.chars().count() as f32 + 3.0) * ch;
            if lx + width > rect.max.x {
                painter.text(
                    egui::pos2(lx, ly),
                    egui::Align2::LEFT_CENTER,
                    "…",
                    font.clone(),
                    c.dim,
                );
                break;
            }
            // The lock the cursor is IN, lit: the band and the column
            // are two views of one address and must agree about which.
            let here = matches!(field, Field::Lock { device, param }
                if device == lock.device && param == lock.param);
            painter.text(
                egui::pos2(lx, ly),
                egui::Align2::LEFT_CENTER,
                lock.name,
                font.clone(),
                if here {
                    c.bright
                } else if lock.effect {
                    c.chassis
                } else {
                    c.label
                },
            );
            lx += (lock.name.chars().count() as f32 + 1.5) * ch;
            painter.text(
                egui::pos2(lx, ly),
                egui::Align2::LEFT_CENTER,
                &lock.reading,
                font.clone(),
                if lock.slide { c.nominal } else { c.fg },
            );
            lx += (lock.reading.chars().count() as f32 + 2.5) * ch;
        }
    }
}

/// Where every column stands, in pixels from the body's left edge.
///
/// Built once a frame from the plan and the room, including the sideways
/// scroll that keeps the cursor on screen. A column the room cannot hold
/// has no position at all rather than a position off the edge, so
/// everything that draws simply skips it and nothing has to clip by
/// hand.
struct Lay {
    x: Vec<Option<f32>>,
    width: Vec<f32>,
}

impl Lay {
    /// Between one track's last column and the next track's first.
    /// @tune 4..40 px
    const TRACK_GAP: f32 = 16.0;
    /// Between two columns of one track.
    /// @tune 2..20 px
    const FIELD_GAP: f32 = 7.0;

    fn of(plan: &Plan, ch: f32, room: f32, cursor: usize) -> Self {
        let mut width = Vec::with_capacity(plan.flat.len());
        let mut raw = Vec::with_capacity(plan.flat.len());
        let mut x = 0.0f32;
        let mut last_track = None;
        for &(track, field) in &plan.flat {
            if last_track.is_some_and(|owner| owner != track) {
                x += Self::TRACK_GAP;
            }
            last_track = Some(track);
            raw.push(x);
            let w = field.width() as f32 * ch;
            width.push(w);
            x += w + Self::FIELD_GAP;
        }
        // Scroll so the cursor's column is inside the room, and no
        // further: a body that recentred on every step would move the
        // whole song sideways under a reader walking one column.
        let scroll = match (raw.get(cursor), width.get(cursor)) {
            (Some(&at), Some(&w)) if at + w > room => at + w - room,
            _ => 0.0,
        };
        Self {
            x: raw
                .into_iter()
                .zip(&width)
                .map(|(at, w)| {
                    let at = at - scroll;
                    (at >= -w && at < room).then_some(at)
                })
                .collect(),
            width,
        }
    }

    fn x_of(&self, index: usize) -> Option<f32> {
        self.x.get(index).copied().flatten()
    }

    fn width_of(&self, index: usize) -> f32 {
        self.width.get(index).copied().unwrap_or(0.0)
    }

    /// How wide a whole track's run of columns is, for its head's band.
    fn track_width(&self, plan: &Plan, track: usize) -> f32 {
        plan.flat
            .iter()
            .enumerate()
            .filter(|(_, (owner, _))| *owner == track)
            .map(|(index, _)| self.width_of(index) + Self::FIELD_GAP)
            .sum::<f32>()
            .max(0.0)
    }
}

/// One field of one cell.
#[allow(clippy::too_many_arguments)]
fn draw_field(
    painter: &egui::Painter,
    at: egui::Pos2,
    font: &egui::FontId,
    ch: f32,
    cell: Option<&Cell>,
    field: Field,
    w: f32,
    lit: bool,
    stage: &super::super::Stage,
    track: usize,
) {
    let c = palette::colours();
    let w = if lit { 1.0 } else { w };
    // A track playing nothing has no cell at all, and draws nothing: an
    // unlaunched channel is not a channel full of rests.
    let Some(cell) = cell else {
        return;
    };
    let rest = ink(c.rule, w * 0.4);
    match field {
        Field::Note => {
            let (text, colour) = match (&cell.note, cell.marked()) {
                (Some((name, _)), _) => (
                    name.clone(),
                    ink(
                        if !cell.sounding {
                            c.dim
                        } else if stage
                            .meter_plan()
                            .columns
                            .get(track)
                            .is_some_and(|column| column.drum)
                        {
                            c.drum
                        } else {
                            c.fg
                        },
                        w,
                    ),
                ),
                (None, true) => ("---".to_owned(), ink(c.rule, w * 0.8)),
                (None, false) => ("···".to_owned(), rest),
            };
            painter.text(at, egui::Align2::LEFT_CENTER, &text, font.clone(), colour);
            // The chord, as the count it is: a chord is one step, not
            // parallel lanes, so the extra notes are a number beside
            // the first and not a column of their own.
            if cell.chord > 0 {
                painter.text(
                    egui::pos2(at.x + 4.0 * ch, at.y),
                    egui::Align2::LEFT_CENTER,
                    &format!("{}", cell.chord),
                    font.clone(),
                    ink(c.bright, w),
                );
            }
        }
        Field::Vel => {
            let (text, colour) = match &cell.note {
                Some((_, velocity)) => (format!("{velocity:02X}"), ink(c.dim, w)),
                None if cell.marked() => ("--".to_owned(), ink(c.rule, w * 0.8)),
                None => ("··".to_owned(), rest),
            };
            painter.text(at, egui::Align2::LEFT_CENTER, &text, font.clone(), colour);
        }
        Field::Len => {
            let text = match (&cell.note, cell.marked()) {
                (Some((_, _)), _) => format!("{}", cell.length.min(999)),
                (None, true) => "--".to_owned(),
                (None, false) => "··".to_owned(),
            };
            painter.text(
                at,
                egui::Align2::LEFT_CENTER,
                &text,
                font.clone(),
                if cell.note.is_some() {
                    ink(c.dim, w)
                } else {
                    rest
                },
            );
        }
        Field::Cond => {
            let (text, colour) = match cell.cond {
                Some((a, b)) => (format!("{a}:{b}"), ink(c.alert, w)),
                None if cell.marked() => ("---".to_owned(), ink(c.rule, w * 0.8)),
                None => ("···".to_owned(), rest),
            };
            painter.text(at, egui::Align2::LEFT_CENTER, &text, font.clone(), colour);
        }
        Field::Prob => {
            let (text, colour) = match cell.chance {
                Some(chance) => (format!("{chance}"), ink(c.alert, w)),
                // A step with no chance written is CERTAIN, and certain
                // is the interesting default to state rather than hide:
                // an empty column here would read as unset.
                None if cell.marked() => ("100".to_owned(), ink(c.rule, w * 0.8)),
                None => ("···".to_owned(), rest),
            };
            painter.text(at, egui::Align2::LEFT_CENTER, &text, font.clone(), colour);
        }
        Field::Rtg => {
            let (text, colour) = match cell.retrig {
                Some((rate, count)) => (format!("{count}:{rate}"), ink(c.bright, w)),
                None if cell.marked() => ("---".to_owned(), ink(c.rule, w * 0.8)),
                None => ("···".to_owned(), rest),
            };
            painter.text(at, egui::Align2::LEFT_CENTER, &text, font.clone(), colour);
        }
        Field::Micro => {
            let (text, colour) = if cell.micro != 0 {
                (format!("{:+}", cell.micro), ink(c.chassis, w))
            } else if cell.marked() {
                ("0".to_owned(), ink(c.rule, w * 0.8))
            } else {
                ("·".to_owned(), rest)
            };
            painter.text(at, egui::Align2::LEFT_CENTER, &text, font.clone(), colour);
        }
        Field::Snd => {
            let (text, colour) = match &cell.sound {
                Some(name) => (clipped(name, Field::Snd.width()), ink(c.dir, w)),
                None if cell.marked() => ("----".to_owned(), ink(c.rule, w * 0.8)),
                None => ("····".to_owned(), rest),
            };
            painter.text(at, egui::Align2::LEFT_CENTER, &text, font.clone(), colour);
        }
        Field::Lock { device, param } => {
            match cell
                .locks
                .iter()
                .find(|lock| lock.device == device && lock.param == param)
            {
                Some(lock) => {
                    painter.text(
                        at,
                        egui::Align2::LEFT_CENTER,
                        &clipped(&lock.reading, Field::Lock { device, param }.width()),
                        font.clone(),
                        ink(if lock.slide { c.nominal } else { c.label }, w),
                    );
                }
                // A step with no lock on this parameter is not a step
                // with a lock of zero. Drawn as the absence it is.
                None => {
                    painter.text(
                        at,
                        egui::Align2::LEFT_CENTER,
                        if cell.marked() { "----" } else { "····" },
                        font.clone(),
                        rest,
                    );
                }
            }
        }
        // The ADD column carries no data. It is a door, and it is drawn
        // as one: faint, until the cursor is standing in it.
        Field::Add => {
            painter.text(
                at,
                egui::Align2::LEFT_CENTER,
                if lit { "+" } else { "·" },
                font.clone(),
                ink(c.chassis, if lit { 1.0 } else { w * 0.5 }),
            );
        }
    }
}

/// `text`, cut to `chars` without an ellipsis eating one of them.
fn clipped(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        return text.to_owned();
    }
    text.chars().take(chars.saturating_sub(1)).collect()
}
