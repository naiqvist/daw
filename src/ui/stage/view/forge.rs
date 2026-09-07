//! The forge, painted: sCOMP's room, in the deck's own hand.
//!
//! The same chassis, inks and rules as the cutting room, so the two
//! rooms read as two doors on one deck: the waveform in the edge ink
//! with its rms in chassis, the pass on show washed in select with an
//! alert frame, the cursor's row in select, alert for what re-renders.

use super::heads;
use super::{chassis, palette};
use crate::PROFONT;
use crate::audio::quad::QuadParams;
use crate::params::quad as qp;
use crate::params::scomp as sp;
use crate::ui::stage::chain;
use crate::ui::stage::forge::{Forge, Subject};
use eframe::egui;
use egui::Color32;

/// The column of rows, its width.
/// @tune 200..420 px
const ROWS_W: f32 = 300.0;
const TITLE_H: f32 = 22.0;
const ROW_H: f32 = 17.0;
const TYPE_PX: f32 = 12.0;
const INSET: f32 = 10.0;
const LANE_GAP: f32 = 6.0;
const LEGEND_H: f32 = 18.0;

fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn legend(subject: Subject) -> [(&'static str, &'static str); 8] {
    match subject {
        Subject::Scomp => [
            ("Up Dn", "row"),
            ("< >", "turn"),
            ("+< >", "coarse"),
            ("Tab", "group"),
            ("R", "reset"),
            (", .", "pass"),
            ("0-8", "show"),
            ("Esc", "leave"),
        ],
        Subject::Quad => [
            ("Up Dn", "row"),
            ("< >", "turn"),
            ("+< >", "coarse"),
            ("Tab", "group"),
            ("R", "reset"),
            (", .", "operator"),
            ("0-3", "show"),
            ("Esc", "leave"),
        ],
    }
}

/// A rise-then-fall envelope, as the voices shape it, as a polyline
/// across `rect`: `seconds` of it, the rise then the fall to nothing.
fn rise_fall_curve(rect: egui::Rect, rise_ms: f32, fall_ms: f32, seconds: f32) -> Vec<egui::Pos2> {
    let rise = rise_ms.max(0.0) / 1000.0;
    let fall = fall_ms.max(1.0) / 1000.0;
    let n = 48usize;
    (0..=n)
        .map(|i| {
            let t = seconds * i as f32 / n as f32;
            let y = if t < rise {
                t / rise.max(1.0e-6)
            } else {
                (-(t - rise) / (fall / 5.0)).exp()
            };
            egui::pos2(
                rect.left() + rect.width() * i as f32 / n as f32,
                rect.bottom() - rect.height() * y.clamp(0.0, 1.0),
            )
        })
        .collect()
}

/// An ADSR, as a polyline: attack up, decay to the sustain, a held
/// stretch, then the release — the held stretch fixed, so the shape
/// reads as attack/decay/release lengths against each other.
fn adsr_curve(rect: egui::Rect, op: &crate::audio::quad::Op) -> Vec<egui::Pos2> {
    let a = op.attack.max(0.0) / 1000.0;
    let d = op.decay.max(1.0) / 1000.0;
    let r = op.release.max(1.0) / 1000.0;
    let hold = 0.4f32;
    let total = (a + d + hold + r).max(0.05);
    let x = |t: f32| rect.left() + rect.width() * (t / total).clamp(0.0, 1.0);
    let y = |v: f32| rect.bottom() - rect.height() * v.clamp(0.0, 1.0);
    vec![
        egui::pos2(x(0.0), y(0.0)),
        egui::pos2(x(a), y(1.0)),
        egui::pos2(x(a + d), y(op.sustain)),
        egui::pos2(x(a + d + hold), y(op.sustain)),
        egui::pos2(x(total), y(0.0)),
    ]
}

impl super::super::Stage {
    pub(super) fn draw_forge(&self, painter: &egui::Painter, field: egui::Rect) {
        let Some(forge) = self.forge.as_ref() else {
            return;
        };
        let c = palette::colours();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let ch = TYPE_PX * 0.6;
        let margin = heads::margin();
        let room = egui::Rect::from_min_max(
            egui::pos2(field.min.x + margin + heads::gutter(), field.min.y + margin),
            egui::pos2(field.max.x - margin, field.max.y - margin),
        );
        let device = self.song.device(forge.device);
        let subject = forge.subject;
        let params = device.map(Forge::params_of).unwrap_or_default();
        let quad = device.map(Forge::quad_params_of).unwrap_or_default();

        // The title row: the room, the device, the facts.
        let ty = room.min.y + TITLE_H * 0.5;
        let mut x = room.min.x;
        painter.text(
            egui::pos2(x, ty),
            egui::Align2::LEFT_CENTER,
            "FORGE //",
            font.clone(),
            c.label,
        );
        x += 9.0 * ch;
        painter.text(
            egui::pos2(x, ty),
            egui::Align2::LEFT_CENTER,
            match subject {
                Subject::Scomp => "sCOMP",
                Subject::Quad => "QUAD",
            },
            font.clone(),
            c.dir,
        );
        x += 7.0 * ch;
        let shown = forge.shown();
        let lanes = forge.lanes();
        let seconds = forge.take.as_ref().map_or(0.0, |take| take.seconds());
        let title = match subject {
            Subject::Scomp => format!(
                "tr {:02}  ·  {} pass{}  ·  take {:.2}s -> {:.2}s  ·  root {:.1} Hz",
                forge.track + 1,
                params.pass_count(),
                if params.pass_count() == 1 { "" } else { "es" },
                params.take_s,
                seconds,
                params.root_hz(),
            ),
            Subject::Quad => format!(
                "tr {:02}  ·  {}  ·  fb {:.0}%  ·  {} {:.1}k  ·  {} x{:.1}",
                forge.track + 1,
                qp::ALGO_NAMES
                    .get(quad.algo.round().max(0.0) as usize)
                    .copied()
                    .unwrap_or("?"),
                quad.feedback * 100.0,
                qp::FMODE_NAMES
                    .get(quad.fmode.round() as usize)
                    .copied()
                    .unwrap_or("lp"),
                quad.cutoff / 1000.0,
                qp::DIST_NAMES
                    .get(quad.dist.round() as usize)
                    .copied()
                    .unwrap_or("off"),
                quad.drive,
            ),
        };
        painter.text(
            egui::pos2(x, ty),
            egui::Align2::LEFT_CENTER,
            &title,
            font.clone(),
            c.fg,
        );
        let on_show = match subject {
            Subject::Quad => format!("ON SHOW: OP {}", shown + 1),
            Subject::Scomp if shown == 0 => "ON SHOW: SOURCE".to_owned(),
            Subject::Scomp => format!("ON SHOW: PASS {shown} of {}", lanes.saturating_sub(1)),
        };
        painter.text(
            egui::pos2(room.max.x, ty),
            egui::Align2::RIGHT_CENTER,
            &on_show,
            font.clone(),
            c.nominal,
        );

        // The rows' column on the right; the lanes take the rest.
        let rows_w = crate::tune!(ROWS_W);
        let legend_h = crate::tune!(LEGEND_H);
        let top = room.min.y + TITLE_H + 4.0;
        let lanes_rect = egui::Rect::from_min_max(
            egui::pos2(room.min.x, top),
            egui::pos2(room.max.x - rows_w - 12.0, room.max.y - legend_h - 4.0),
        );
        let col = egui::Rect::from_min_max(egui::pos2(lanes_rect.max.x + 12.0, top), room.max);

        // The legend, under the lanes.
        let ly = lanes_rect.max.y + 4.0 + legend_h * 0.5;
        let mut lx = lanes_rect.min.x;
        for (chord, word) in legend(subject) {
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

        // The lanes: every pass on one time axis, the longest setting it.
        // Or, for QUAD, the routing and the envelopes.
        chassis::frame(painter, lanes_rect, true);
        let inner = lanes_rect.shrink(INSET);
        if subject == Subject::Quad {
            draw_quad_room(painter, inner, &quad, shown, &font, ch);
        } else if let Some(take) = forge.take.as_ref().filter(|take| !take.passes.is_empty()) {
            let rate = f64::from(take.sample_rate.max(1));
            let longest = take
                .passes
                .iter()
                .map(|pass| pass.len())
                .max()
                .unwrap_or(1)
                .max(1) as f64;
            let ruler_h = TYPE_PX + 4.0;
            let lane_h = ((inner.height() - ruler_h) / lanes.max(1) as f32 - LANE_GAP).max(8.0);
            let label_w = 13.0 * ch;
            let wave_x0 = inner.min.x + label_w;
            let wave_w = (inner.max.x - wave_x0).max(1.0);
            for (k, pass) in take.passes.iter().enumerate() {
                let y0 = inner.min.y + k as f32 * (lane_h + LANE_GAP);
                let lane = egui::Rect::from_min_max(
                    egui::pos2(inner.min.x, y0),
                    egui::pos2(inner.max.x, y0 + lane_h),
                );
                let live = k == shown;
                // The label: which pass, and what it went through.
                let word = if k == 0 {
                    "SINE".to_owned()
                } else {
                    format!("P{k} {:+.0}st", params.shift_st * k as f32)
                };
                painter.text(
                    egui::pos2(lane.min.x, lane.center().y),
                    egui::Align2::LEFT_CENTER,
                    &word,
                    font.clone(),
                    if live { c.bright } else { c.label },
                );
                // The wave, on the shared axis: this pass's share of the
                // longest.
                let share = (pass.len() as f64 / longest) as f32;
                let wave = egui::Rect::from_min_max(
                    egui::pos2(wave_x0, lane.min.y),
                    egui::pos2(wave_x0 + wave_w * share, lane.max.y),
                );
                let strip = egui::Rect::from_min_max(
                    egui::pos2(wave_x0, lane.min.y),
                    egui::pos2(inner.max.x, lane.max.y),
                );
                if live {
                    painter.rect_filled(strip, 0.0, alpha(c.select, 54));
                    painter.rect_stroke(
                        strip.expand(1.5),
                        0.0,
                        egui::Stroke::new(1.0, c.alert),
                        egui::StrokeKind::Outside,
                    );
                }
                let mid = wave.center().y;
                let half = wave.height() * 0.5 - 1.0;
                painter.line_segment(
                    [
                        egui::pos2(wave_x0, mid.round() - 0.5),
                        egui::pos2(inner.max.x, mid.round() - 0.5),
                    ],
                    egui::Stroke::new(1.0, c.rule),
                );
                let columns = wave.width().floor().max(1.0) as usize;
                if let Some(peaks) = forge.peaks.get(k) {
                    let bins = peaks.columns(Some(pass), 0.0, 1.0, columns);
                    let ink = if live { c.edge } else { alpha(c.edge, 140) };
                    let body = if live {
                        c.chassis
                    } else {
                        alpha(c.chassis, 120)
                    };
                    for (i, bin) in bins.iter().enumerate() {
                        let x = wave.min.x + i as f32 + 0.5;
                        painter.line_segment(
                            [
                                egui::pos2(x, mid - bin.max.clamp(-1.0, 1.0) * half),
                                egui::pos2(x, mid - bin.min.clamp(-1.0, 1.0) * half),
                            ],
                            egui::Stroke::new(1.0, ink),
                        );
                        let r = bin.rms.clamp(0.0, 1.0);
                        painter.line_segment(
                            [egui::pos2(x, mid - r * half), egui::pos2(x, mid + r * half)],
                            egui::Stroke::new(1.0, body),
                        );
                    }
                }
                // Where this pass ends, a tick — the bounce's stretch.
                if share < 0.999 {
                    let x = wave.max.x.round() - 0.5;
                    painter.line_segment(
                        [egui::pos2(x, lane.min.y), egui::pos2(x, lane.max.y)],
                        egui::Stroke::new(1.0, alpha(c.alert, 160)),
                    );
                }
            }
            // The ruler, in seconds, along the foot.
            let ry = inner.max.y - ruler_h + 2.0;
            let total = longest / rate;
            let step = if total <= 1.0 {
                0.1
            } else if total <= 3.0 {
                0.25
            } else {
                0.5
            };
            let mut t = 0.0f64;
            while t <= total + 1e-9 {
                let x = wave_x0 + wave_w * (t / total) as f32;
                painter.line_segment(
                    [
                        egui::pos2(x.round() - 0.5, ry),
                        egui::pos2(x.round() - 0.5, ry + 4.0),
                    ],
                    egui::Stroke::new(1.0, c.rule),
                );
                let label = format!("{t:.2}s");
                if x + label.len() as f32 * ch <= inner.max.x {
                    painter.text(
                        egui::pos2(x + 2.0, ry + 5.0),
                        egui::Align2::LEFT_TOP,
                        label,
                        font.clone(),
                        c.dim,
                    );
                }
                t += step;
            }
        } else {
            painter.text(
                inner.center(),
                egui::Align2::CENTER_CENTER,
                "no take",
                font.clone(),
                c.dim,
            );
        }

        // The rows: every knob, grouped, the cursor's row lit. More
        // lines than fit scroll, the cursor's line kept in view.
        chassis::frame(painter, col, false);
        let inner = col.shrink(INSET);
        let Some(device) = device else {
            return;
        };
        let spec = device.kind.spec();
        enum Line {
            Header(&'static str),
            Row(usize),
        }
        let rows = forge.rows();
        let mut lines: Vec<Line> = Vec::with_capacity(rows.len() + 8);
        let mut last_group = "";
        for (index, (group, _)) in rows.iter().enumerate() {
            if *group != last_group {
                lines.push(Line::Header(group));
                last_group = group;
            }
            lines.push(Line::Row(index));
        }
        let fit = (((inner.height() - ROW_H) / ROW_H).floor().max(1.0)) as usize;
        let cursor_line = lines
            .iter()
            .position(|line| matches!(line, Line::Row(index) if *index == forge.row))
            .unwrap_or(0);
        let first = if lines.len() <= fit {
            0
        } else {
            cursor_line.saturating_sub(fit / 2).min(lines.len() - fit)
        };
        let mut y = inner.min.y + ROW_H * 0.5;
        for line in lines.iter().skip(first).take(fit) {
            match line {
                Line::Header(group) => {
                    painter.text(
                        egui::pos2(inner.min.x, y),
                        egui::Align2::LEFT_CENTER,
                        *group,
                        font.clone(),
                        c.label,
                    );
                }
                Line::Row(index) => {
                    let (_, id) = rows[*index];
                    let live = *index == forge.row;
                    let baked = subject == Subject::Scomp && sp::baked(id);
                    if live {
                        let row_rect = egui::Rect::from_min_max(
                            egui::pos2(inner.min.x - 4.0, y - ROW_H * 0.5),
                            egui::pos2(inner.max.x + 4.0, y + ROW_H * 0.5),
                        );
                        painter.rect_filled(row_rect, 0.0, c.select);
                        crate::ui::nav_cursor::claim(
                            painter,
                            ("forge-row-cursor", *index),
                            row_rect,
                            crate::ui::nav_cursor::Kind::Row,
                            crate::ui::nav_cursor::Layer::Surface,
                            c.alert,
                        );
                    }
                    let (name, value) = spec
                        .params
                        .iter()
                        .zip(spec.labels)
                        .find(|(def, _)| def.id == id)
                        .map(|(def, label)| {
                            (
                                label.name.to_ascii_lowercase(),
                                chain::format_param(def, label, device.value(id)),
                            )
                        })
                        .unwrap_or_else(|| ("?".to_owned(), "?".to_owned()));
                    painter.text(
                        egui::pos2(inner.min.x + ch, y),
                        egui::Align2::LEFT_CENTER,
                        &name,
                        font.clone(),
                        if live { c.bright } else { c.dim },
                    );
                    if baked {
                        painter.text(
                            egui::pos2(inner.min.x + ch * (name.len() as f32 + 2.0), y),
                            egui::Align2::LEFT_CENTER,
                            "*",
                            font.clone(),
                            alpha(c.alert, if live { 255 } else { 140 }),
                        );
                    }
                    painter.text(
                        egui::pos2(inner.max.x, y),
                        egui::Align2::RIGHT_CENTER,
                        &value,
                        font.clone(),
                        if live { c.bright } else { c.fg },
                    );
                }
            }
            y += ROW_H;
        }
        if first + fit < lines.len() {
            painter.text(
                egui::pos2(inner.min.x, inner.max.y),
                egui::Align2::LEFT_BOTTOM,
                format!("+{}", lines.len() - first - fit),
                font.clone(),
                c.dim,
            );
        }
        if subject == Subject::Scomp {
            painter.text(
                egui::pos2(inner.max.x, inner.max.y),
                egui::Align2::RIGHT_BOTTOM,
                "* re-renders the take",
                font,
                alpha(c.alert, 160),
            );
        }
    }
}

/// QUAD's picture: the routing across the top, the operators' envelopes,
/// the two pitch envelopes and the filter's beneath.
fn draw_quad_room(
    painter: &egui::Painter,
    inner: egui::Rect,
    p: &QuadParams,
    shown: usize,
    font: &egui::FontId,
    ch: f32,
) {
    let c = palette::colours();
    let algo = p.algorithm();
    let split = inner.min.y + inner.height() * 0.48;
    let route = egui::Rect::from_min_max(inner.min, egui::pos2(inner.max.x, split - 8.0));
    let lower = egui::Rect::from_min_max(egui::pos2(inner.min.x, split + 8.0), inner.max);

    // The routing, on the same grid the card draws, larger.
    super::quad_card::draw_routing(painter, route, p, Some(shown), 12.0);

    // Three panels: the operators' envelopes, the pitch envelopes, the
    // filter's.
    let gap = 10.0;
    let panel_w = (lower.width() - gap * 2.0) / 3.0;
    let panel = |i: usize| {
        egui::Rect::from_min_max(
            egui::pos2(lower.left() + (panel_w + gap) * i as f32, lower.top()),
            egui::pos2(
                lower.left() + (panel_w + gap) * i as f32 + panel_w,
                lower.bottom(),
            ),
        )
    };
    let head_h = 14.0;
    let plot_of = |r: egui::Rect| {
        egui::Rect::from_min_max(
            egui::pos2(r.left() + 2.0, r.top() + head_h + 2.0),
            r.max - egui::vec2(2.0, 2.0),
        )
    };
    for (i, title) in ["OPERATOR ENVELOPES", "PITCH ENVELOPES", "FILTER"]
        .iter()
        .enumerate()
    {
        let r = panel(i);
        painter.rect_filled(r, 0.0, c.panel);
        painter.rect_stroke(
            r,
            0.0,
            egui::Stroke::new(1.0, c.rule),
            egui::StrokeKind::Inside,
        );
        painter.text(
            egui::pos2(r.left() + 4.0, r.top() + head_h * 0.5),
            egui::Align2::LEFT_CENTER,
            *title,
            font.clone(),
            c.label,
        );
    }
    // Operators' ADSRs, the one on show bright.
    let plot = plot_of(panel(0));
    for op in 0..qp::OPS {
        if op == shown {
            continue;
        }
        painter.add(egui::Shape::line(
            adsr_curve(plot, &p.ops[op]),
            egui::Stroke::new(1.0, alpha(c.edge, 110)),
        ));
    }
    painter.add(egui::Shape::line(
        adsr_curve(plot, &p.ops[shown]),
        egui::Stroke::new(2.0, c.bright),
    ));
    let op = &p.ops[shown];
    painter.text(
        egui::pos2(plot.right() - 2.0, plot.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        format!(
            "op {} · {:.0}/{:.0}/{:.0}%/{:.0}",
            shown + 1,
            op.attack,
            op.decay,
            op.sustain * 100.0,
            op.release
        ),
        font.clone(),
        c.dim,
    );
    // The two pitch envelopes over two seconds, zero at the middle.
    let plot = plot_of(panel(1));
    let mid = plot.center().y;
    painter.line_segment(
        [egui::pos2(plot.left(), mid), egui::pos2(plot.right(), mid)],
        egui::Stroke::new(1.0, c.rule),
    );
    for (amount, rise, fall, ink, word) in [
        (p.pitch1, p.p1_rise, p.p1_fall, c.alert, "1 all"),
        (p.pitch2, p.p2_rise, p.p2_fall, c.nominal, "2 mod"),
    ] {
        let half = egui::Rect::from_min_max(
            egui::pos2(plot.left(), plot.top()),
            egui::pos2(plot.right(), mid),
        );
        let mut points = rise_fall_curve(half, rise, fall, 2.0);
        let scale = (amount / 48.0).clamp(-1.0, 1.0);
        for point in points.iter_mut() {
            point.y = mid - (mid - point.y) * scale;
        }
        painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, ink)));
        let _ = word;
    }
    painter.text(
        egui::pos2(plot.right() - 2.0, plot.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        format!("1 {:+.0}st  2 {:+.0}st", p.pitch1, p.pitch2),
        font.clone(),
        c.dim,
    );
    // The filter's envelope, and its numbers.
    let plot = plot_of(panel(2));
    let points = rise_fall_curve(plot, p.fenv_att, p.fenv_dec, 2.0);
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, c.alert)));
    painter.text(
        egui::pos2(plot.right() - 2.0, plot.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        format!(
            "{} {:.0}Hz q{:.1} {:+.1}oct kt{:.0}%",
            qp::FMODE_NAMES
                .get(p.fmode.round() as usize)
                .copied()
                .unwrap_or("lp"),
            p.cutoff,
            p.reso,
            p.fenv,
            p.keytrack * 100.0
        ),
        font.clone(),
        c.dim,
    );
    let _ = ch;
}
