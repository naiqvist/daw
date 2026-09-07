//! The forge, painted: sCOMP's room in its own colours.
//!
//! The stage is a cool stone deck; the forge is the one door on it that
//! opens onto somewhere else — black, ultraviolet, hazard-striped, the
//! waveform in acid. Deliberate: this instrument is a bounce chain
//! wearing a synth's name, and its room should look like the machine
//! it is rather than a page of the stage. The palette lives here and
//! nowhere else, so the deck's own hues are untouched.

use super::heads;
use crate::PROFONT;
use crate::params::scomp as sp;
use crate::ui::stage::chain;
use crate::ui::stage::forge::{Forge, ROWS};
use eframe::egui;
use egui::Color32;

/// The column of rows, its width.
/// @tune 200..420 px
const ROWS_W: f32 = 300.0;
/// The hazard stripe across the head of the room.
/// @tune 4..14 px
const HAZARD_H: f32 = 7.0;
const TITLE_H: f32 = 22.0;
const ROW_H: f32 = 17.0;
const TYPE_PX: f32 = 12.0;
const INSET: f32 = 10.0;
const LANE_GAP: f32 = 6.0;
const LEGEND_H: f32 = 18.0;
/// A cut corner's reach.
const CHAMFER: f32 = 9.0;

/// Cybergoth: black ground, ultraviolet, acid, one hot pink.
struct Goth {
    ground: Color32,
    panel: Color32,
    ink: Color32,
    hot: Color32,
    violet: Color32,
    dim: Color32,
    fg: Color32,
    hazard: Color32,
    black: Color32,
}

fn goth() -> Goth {
    Goth {
        ground: Color32::from_rgb(0x0A, 0x07, 0x12),
        panel: Color32::from_rgb(0x11, 0x0C, 0x1E),
        ink: Color32::from_rgb(0xB6, 0xFF, 0x1A),
        hot: Color32::from_rgb(0xFF, 0x2B, 0xD6),
        violet: Color32::from_rgb(0x8C, 0x6C, 0xFF),
        dim: Color32::from_rgb(0x4A, 0x44, 0x66),
        fg: Color32::from_rgb(0xE8, 0xE4, 0xFF),
        hazard: Color32::from_rgb(0xF5, 0xD9, 0x0A),
        black: Color32::from_rgb(0x05, 0x04, 0x08),
    }
}

fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// A frame with its corners cut, the forge's own chassis.
fn cut_frame(
    painter: &egui::Painter,
    rect: egui::Rect,
    stroke: egui::Stroke,
    fill: Option<Color32>,
) {
    let c = CHAMFER.min(rect.width() * 0.25).min(rect.height() * 0.25);
    let points = vec![
        egui::pos2(rect.min.x + c, rect.min.y),
        egui::pos2(rect.max.x - c, rect.min.y),
        egui::pos2(rect.max.x, rect.min.y + c),
        egui::pos2(rect.max.x, rect.max.y - c),
        egui::pos2(rect.max.x - c, rect.max.y),
        egui::pos2(rect.min.x + c, rect.max.y),
        egui::pos2(rect.min.x, rect.max.y - c),
        egui::pos2(rect.min.x, rect.min.y + c),
    ];
    if let Some(fill) = fill {
        painter.add(egui::Shape::convex_polygon(
            points.clone(),
            fill,
            egui::Stroke::NONE,
        ));
    }
    painter.add(egui::Shape::closed_line(points, stroke));
}

/// Hazard stripes: yellow and black, leaning right.
fn hazard(painter: &egui::Painter, rect: egui::Rect, g: &Goth) {
    painter.rect_filled(rect, 0.0, g.black);
    let step = rect.height() * 2.0;
    let mut x = rect.min.x - rect.height();
    while x < rect.max.x + rect.height() {
        let points = vec![
            egui::pos2(x.max(rect.min.x), rect.max.y),
            egui::pos2(
                (x + rect.height()).clamp(rect.min.x, rect.max.x),
                rect.min.y,
            ),
            egui::pos2(
                (x + rect.height() * 2.0).clamp(rect.min.x, rect.max.x),
                rect.min.y,
            ),
            egui::pos2(
                (x + rect.height()).clamp(rect.min.x, rect.max.x),
                rect.max.y,
            ),
        ];
        painter.add(egui::Shape::convex_polygon(
            points,
            g.hazard,
            egui::Stroke::NONE,
        ));
        x += step;
    }
}

fn legend() -> [(&'static str, &'static str); 8] {
    [
        ("Up Dn", "row"),
        ("< >", "turn"),
        ("+< >", "coarse"),
        ("Tab", "group"),
        ("R", "reset"),
        (", .", "pass"),
        ("0-8", "show"),
        ("Esc", "leave"),
    ]
}

impl super::super::Stage {
    pub(super) fn draw_forge(&self, painter: &egui::Painter, field: egui::Rect) {
        let Some(forge) = self.forge.as_ref() else {
            return;
        };
        let g = goth();
        let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
        let ch = TYPE_PX * 0.6;
        let margin = heads::margin();
        let room = egui::Rect::from_min_max(
            egui::pos2(field.min.x + margin + heads::gutter(), field.min.y + margin),
            egui::pos2(field.max.x - margin, field.max.y - margin),
        );
        // The ground: the deck's field goes black under this room.
        painter.rect_filled(field, 0.0, g.ground);
        let device = self.song.device(forge.device);
        let params = device.map(Forge::params_of).unwrap_or_default();

        // The hazard stripe, and the title on it.
        let hazard_h = crate::tune!(HAZARD_H);
        hazard(
            painter,
            egui::Rect::from_min_size(room.min, egui::vec2(room.width(), hazard_h)),
            &g,
        );
        let ty = room.min.y + hazard_h + TITLE_H * 0.5;
        let mut x = room.min.x;
        painter.text(
            egui::pos2(x, ty),
            egui::Align2::LEFT_CENTER,
            "sCOMP // FORGE",
            font.clone(),
            g.hot,
        );
        x += 16.0 * ch;
        let shown = forge.shown();
        let lanes = forge.lanes();
        let seconds = forge.take.as_ref().map_or(0.0, |take| take.seconds());
        let title = format!(
            "tr {:02}  ·  {} pass{}  ·  take {:.2}s -> {:.2}s  ·  root {:.1} Hz",
            forge.track + 1,
            params.pass_count(),
            if params.pass_count() == 1 { "" } else { "es" },
            params.take_s,
            seconds,
            params.root_hz(),
        );
        painter.text(
            egui::pos2(x, ty),
            egui::Align2::LEFT_CENTER,
            &title,
            font.clone(),
            g.violet,
        );
        let on_show = if shown == 0 {
            "ON SHOW: SOURCE".to_owned()
        } else {
            format!("ON SHOW: PASS {shown} of {}", lanes.saturating_sub(1))
        };
        painter.text(
            egui::pos2(room.max.x, ty),
            egui::Align2::RIGHT_CENTER,
            &on_show,
            font.clone(),
            g.ink,
        );

        // The rows' column on the right; the lanes take the rest.
        let rows_w = crate::tune!(ROWS_W);
        let legend_h = crate::tune!(LEGEND_H);
        let top = room.min.y + hazard_h + TITLE_H + 4.0;
        let lanes_rect = egui::Rect::from_min_max(
            egui::pos2(room.min.x, top),
            egui::pos2(room.max.x - rows_w - 12.0, room.max.y - legend_h - 4.0),
        );
        let col = egui::Rect::from_min_max(egui::pos2(lanes_rect.max.x + 12.0, top), room.max);

        // The legend, under the lanes.
        let ly = lanes_rect.max.y + 4.0 + legend_h * 0.5;
        let mut lx = lanes_rect.min.x;
        for (chord, word) in legend() {
            painter.text(
                egui::pos2(lx, ly),
                egui::Align2::LEFT_CENTER,
                chord,
                font.clone(),
                g.hot,
            );
            lx += (chord.chars().count() as f32 + 1.0) * ch;
            painter.text(
                egui::pos2(lx, ly),
                egui::Align2::LEFT_CENTER,
                word,
                font.clone(),
                g.dim,
            );
            lx += (word.chars().count() as f32 + 2.5) * ch;
        }

        // The lanes: every pass on one time axis, the longest setting it.
        cut_frame(
            painter,
            lanes_rect,
            egui::Stroke::new(1.0, g.violet),
            Some(g.panel),
        );
        let inner = lanes_rect.shrink(INSET);
        if let Some(take) = forge.take.as_ref().filter(|take| !take.passes.is_empty()) {
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
                    if live { g.hot } else { g.violet },
                );
                // The wave, on the shared axis: this pass's share of the
                // longest.
                let share = (pass.len() as f64 / longest) as f32;
                let wave = egui::Rect::from_min_max(
                    egui::pos2(wave_x0, lane.min.y),
                    egui::pos2(wave_x0 + wave_w * share, lane.max.y),
                );
                if live {
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(wave_x0, lane.min.y),
                            egui::pos2(inner.max.x, lane.max.y),
                        ),
                        0.0,
                        alpha(g.hot, 18),
                    );
                    painter.rect_stroke(
                        egui::Rect::from_min_max(
                            egui::pos2(wave_x0 - 2.0, lane.min.y - 1.0),
                            egui::pos2(inner.max.x + 2.0, lane.max.y + 1.0),
                        ),
                        0.0,
                        egui::Stroke::new(1.0, g.hot),
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
                    egui::Stroke::new(1.0, alpha(g.dim, 120)),
                );
                let columns = wave.width().floor().max(1.0) as usize;
                if let Some(peaks) = forge.peaks.get(k) {
                    let bins = peaks.columns(Some(pass), 0.0, 1.0, columns);
                    let ink = if live { g.ink } else { alpha(g.ink, 120) };
                    let body = if live {
                        alpha(g.violet, 200)
                    } else {
                        alpha(g.violet, 90)
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
                        egui::Stroke::new(1.0, alpha(g.hazard, 160)),
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
                    egui::Stroke::new(1.0, g.dim),
                );
                let label = format!("{t:.2}s");
                if x + label.len() as f32 * ch <= inner.max.x {
                    painter.text(
                        egui::pos2(x + 2.0, ry + 5.0),
                        egui::Align2::LEFT_TOP,
                        label,
                        font.clone(),
                        g.dim,
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
                g.dim,
            );
        }

        // The rows: every knob, grouped, the cursor's row hot. More
        // lines than fit scroll, the cursor's line kept in view.
        cut_frame(
            painter,
            col,
            egui::Stroke::new(1.0, g.violet),
            Some(g.panel),
        );
        let inner = col.shrink(INSET);
        let Some(device) = device else {
            return;
        };
        let spec = device.kind.spec();
        enum Line {
            Header(&'static str),
            Row(usize),
        }
        let mut lines: Vec<Line> = Vec::with_capacity(ROWS.len() + 8);
        let mut last_group = "";
        for (index, (group, _)) in ROWS.iter().enumerate() {
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
                        g.violet,
                    );
                }
                Line::Row(index) => {
                    let (_, id) = ROWS[*index];
                    let live = *index == forge.row;
                    let baked = sp::baked(id);
                    if live {
                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(inner.min.x - 4.0, y - ROW_H * 0.5),
                                egui::pos2(inner.max.x + 4.0, y + ROW_H * 0.5),
                            ),
                            0.0,
                            alpha(g.hot, 40),
                        );
                        painter.line_segment(
                            [
                                egui::pos2(inner.min.x - 6.0, y - ROW_H * 0.5),
                                egui::pos2(inner.min.x - 6.0, y + ROW_H * 0.5),
                            ],
                            egui::Stroke::new(2.0, g.hot),
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
                        if live { g.fg } else { g.dim },
                    );
                    if baked {
                        painter.text(
                            egui::pos2(inner.min.x + ch * (name.len() as f32 + 2.0), y),
                            egui::Align2::LEFT_CENTER,
                            "*",
                            font.clone(),
                            alpha(g.hazard, if live { 255 } else { 120 }),
                        );
                    }
                    painter.text(
                        egui::pos2(inner.max.x, y),
                        egui::Align2::RIGHT_CENTER,
                        &value,
                        font.clone(),
                        if live { g.ink } else { g.fg },
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
                g.dim,
            );
        }
        painter.text(
            egui::pos2(inner.max.x, inner.max.y),
            egui::Align2::RIGHT_BOTTOM,
            "* re-renders the take",
            font,
            alpha(g.hazard, 140),
        );
    }
}
