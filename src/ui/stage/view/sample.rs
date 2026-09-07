//! The cutting room: the field, whole, given to one sample.
//!
//! The waveform is the file's own peaks (`sample_peaks`), drawn as one
//! hairline per column from min to max with the rms inside it; the trim
//! is the sampler's start and end, in alert, with what lies outside
//! washed toward the ground; slices are chassis ticks with their
//! numbers; the cursor is bright; the range being auditioned is a
//! nominal wash. Beneath, the whole file with the window on it. On the
//! attributes page the sampler's parameters stand beside the wave as
//! rows, exactly as the band writes them.

use super::heads;
use super::{chassis, palette};
use crate::PROFONT;
use crate::params::sampler as sp;
use crate::ui::stage::chain;
use crate::ui::stage::sample::Page;
use eframe::egui;

/// The overview strip's height.
/// @tune 8..40 px
const OVERVIEW_H: f32 = 16.0;
/// The attributes column's width, on the attributes page.
/// @tune 120..400 px
const ATTR_W: f32 = 240.0;
const TITLE_H: f32 = 22.0;
const ROW_H: f32 = 18.0;
const TYPE_PX: f32 = 12.0;
const INSET: f32 = 10.0;

/// A human ruler interval that leaves roughly one label every 72 pixels.
/// The editor's geometry is normalized; only the labels speak seconds.
fn ruler_step_seconds(visible_seconds: f64, width: f32) -> f64 {
    if !visible_seconds.is_finite() || visible_seconds <= 0.0 || width <= 0.0 {
        return 1.0;
    }
    let labels = (f64::from(width) / 72.0).max(1.0);
    let raw = visible_seconds / labels;
    let decade = 10.0_f64.powf(raw.log10().floor());
    let unit = raw / decade;
    let nice = if unit <= 1.0 {
        1.0
    } else if unit <= 2.0 {
        2.0
    } else if unit <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * decade
}

impl super::super::Stage {
    pub(super) fn draw_sample(&self, painter: &egui::Painter, field: egui::Rect) {
        let Some(editor) = self.sample.as_ref() else {
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
        let device = self.song.device(editor.device);
        let data = self.sample_data.as_ref();

        // The title row: the file, the page, the facts.
        let ty = room.min.y + TITLE_H * 0.5;
        let mut x = room.min.x;
        painter.text(
            egui::pos2(x, ty),
            egui::Align2::LEFT_CENTER,
            "SAMPLE LAB //",
            font.clone(),
            c.label,
        );
        x += 14.0 * ch;
        let name = data
            .and_then(|d| d.path.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "--".to_owned());
        painter.text(
            egui::pos2(x, ty),
            egui::Align2::LEFT_CENTER,
            &name,
            font.clone(),
            c.dir,
        );
        x += (name.chars().count() as f32 + 3.0) * ch;
        for page in Page::ALL {
            let on = page == editor.page;
            painter.text(
                egui::pos2(x, ty),
                egui::Align2::LEFT_CENTER,
                page.word(),
                font.clone(),
                if on { c.bright } else { c.dim },
            );
            if on {
                let y = ty + 8.0;
                painter.line_segment(
                    [
                        egui::pos2(x, y),
                        egui::pos2(x + page.word().len() as f32 * ch, y),
                    ],
                    egui::Stroke::new(1.0, c.alert),
                );
            }
            x += (page.word().len() as f32 + 2.0) * ch;
        }
        let active_slice = device.and_then(|device| editor.slice_at(device));
        let facts: Vec<(&str, String)> = match data {
            Some(d) => vec![
                ("len", format!("{:.2}s", d.seconds())),
                ("rate", d.sample_rate.to_string()),
                ("ch", d.channels.to_string()),
                (
                    "snap",
                    if editor.snap {
                        "on".into()
                    } else {
                        "off".into()
                    },
                ),
                (
                    "slice",
                    active_slice.map_or_else(
                        || "--".to_owned(),
                        |index| {
                            format!(
                                "{:02}/{:02}",
                                index + 1,
                                device.map_or(0, |device| device.slices.len())
                            )
                        },
                    ),
                ),
                ("grid", editor.count.to_string()),
            ],
            None => vec![
                ("len", "--".into()),
                ("rate", "--".into()),
                ("ch", "--".into()),
            ],
        };
        let total: f32 = facts
            .iter()
            .map(|(l, v)| (l.len() as f32 + 1.0 + v.len() as f32 + 2.5) * ch)
            .sum();
        let mut fx = room.max.x - total;
        for (label, value) in &facts {
            painter.text(
                egui::pos2(fx, ty),
                egui::Align2::LEFT_CENTER,
                *label,
                font.clone(),
                c.label,
            );
            fx += (label.len() as f32 + 1.0) * ch;
            painter.text(
                egui::pos2(fx, ty),
                egui::Align2::LEFT_CENTER,
                value,
                font.clone(),
                c.fg,
            );
            fx += (value.len() as f32 + 2.5) * ch;
        }

        // The wave, and beside it the attributes when that page is up.
        let attr_w = if editor.page == Page::Attr {
            crate::tune!(ATTR_W) + 12.0
        } else {
            0.0
        };
        let overview_h = crate::tune!(OVERVIEW_H);
        let wave = egui::Rect::from_min_max(
            egui::pos2(room.min.x, room.min.y + TITLE_H + 4.0),
            egui::pos2(room.max.x - attr_w, room.max.y - overview_h - 8.0),
        );
        chassis::frame(painter, wave, true);
        let inner = wave.shrink(INSET);
        let Some(data) = data else {
            painter.text(
                inner.center(),
                egui::Align2::CENTER_CENTER,
                "no sample loaded",
                font,
                c.dim,
            );
            return;
        };
        // Cursor, view, trim, slices, audition and Peaks::columns all share
        // one coordinate system: normalized fractions of the file. Seconds
        // are derived exactly once, at the ruler/readout boundary.
        let length_seconds = data.seconds().max(0.0);
        let from = editor.view_from.clamp(0.0, 1.0);
        let to = editor.view_to().clamp(from, 1.0);
        let span = (to - from).max(f64::EPSILON);
        let px = |at: f64| inner.min.x + inner.width() * ((at - from) / span) as f32;
        let mid = inner.center().y;
        let half = inner.height() * 0.5 - 2.0;

        // A ruler in seconds along the top of the glass. Its interval adapts
        // to the zoom, while positions remain normalized file fractions.
        if length_seconds > 0.0 && length_seconds.is_finite() {
            let start_seconds = from * length_seconds;
            let end_seconds = to * length_seconds;
            let major_step = ruler_step_seconds(end_seconds - start_seconds, inner.width());
            let step = major_step / 5.0;
            let mut t = (start_seconds / step).floor() * step;
            let ry = inner.min.y.round() - 0.5;
            while t <= end_seconds + step * 0.5 {
                if t >= start_seconds {
                    let x = px(t / length_seconds).round() - 0.5;
                    let major = ((t / major_step).round() * major_step - t).abs() < step * 1e-4;
                    painter.line_segment(
                        [
                            egui::pos2(x, ry),
                            egui::pos2(x, ry + if major { 6.0 } else { 3.0 }),
                        ],
                        egui::Stroke::new(1.0, c.rule),
                    );
                    if major {
                        painter.text(
                            egui::pos2(x + 3.0, ry + 1.0),
                            egui::Align2::LEFT_TOP,
                            crate::ui::stage::sample::time_word(t / length_seconds, length_seconds),
                            font.clone(),
                            c.dim,
                        );
                    }
                }
                t += step;
            }
        }

        // Peaks: one column per pixel.
        let columns = inner.width().floor().max(1.0) as usize;
        let bins = data.peaks.columns(Some(&data.samples), from, to, columns);
        for (i, bin) in bins.iter().enumerate() {
            let x = inner.min.x + i as f32 + 0.5;
            let (lo, hi) = (bin.min.clamp(-1.0, 1.0), bin.max.clamp(-1.0, 1.0));
            painter.line_segment(
                [
                    egui::pos2(x, mid - hi * half),
                    egui::pos2(x, mid - lo * half),
                ],
                egui::Stroke::new(1.0, c.edge),
            );
            let r = bin.rms.clamp(0.0, 1.0);
            painter.line_segment(
                [egui::pos2(x, mid - r * half), egui::pos2(x, mid + r * half)],
                egui::Stroke::new(1.0, c.chassis),
            );
        }
        painter.line_segment(
            [
                egui::pos2(inner.min.x, mid.round() - 0.5),
                egui::pos2(inner.max.x, mid.round() - 0.5),
            ],
            egui::Stroke::new(1.0, c.rule),
        );

        // The trim: what the sampler keeps, and what it does not.
        if let Some(device) = device {
            let start = f64::from(device.value(sp::START));
            let end = f64::from(device.value(sp::END));
            let g = c.ground;
            let wash = egui::Color32::from_rgba_unmultiplied(g.r(), g.g(), g.b(), 140);
            if start > from {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        inner.min,
                        egui::pos2(px(start).min(inner.max.x), inner.max.y),
                    ),
                    0.0,
                    wash,
                );
            }
            if end < to {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(px(end).max(inner.min.x), inner.min.y),
                        inner.max,
                    ),
                    0.0,
                    wash,
                );
            }
            // The slice under the cursor is a region, not merely a numbered
            // fence. A quiet selection wash makes it immediately clear what
            // audition, delete and per-trig slice assignment will address.
            if let Some(index) = active_slice {
                let a = device.slices[index].max(from);
                let b = device.slices.get(index + 1).copied().unwrap_or(end).min(to);
                if b > a {
                    let s = c.select;
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(px(a), inner.min.y),
                            egui::pos2(px(b), inner.max.y),
                        ),
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(s.r(), s.g(), s.b(), 54),
                    );
                }
            }
            for (frame, word) in [(start, "in"), (end, "out")] {
                if frame >= from && frame <= to {
                    let x = px(frame).round() - 0.5;
                    painter.line_segment(
                        [egui::pos2(x, inner.min.y), egui::pos2(x, inner.max.y)],
                        egui::Stroke::new(1.0, c.alert),
                    );
                    painter.text(
                        egui::pos2(x + 3.0, inner.max.y - TYPE_PX - 4.0),
                        egui::Align2::LEFT_BOTTOM,
                        word,
                        font.clone(),
                        c.alert,
                    );
                }
            }
            // Slices: a tick and a number each.
            for (i, slice) in device.slices.iter().enumerate() {
                if *slice < from || *slice > to {
                    continue;
                }
                let x = px(*slice).round() - 0.5;
                painter.line_segment(
                    [egui::pos2(x, inner.min.y), egui::pos2(x, inner.max.y)],
                    egui::Stroke::new(
                        if active_slice == Some(i) { 2.0 } else { 1.0 },
                        if active_slice == Some(i) {
                            c.alert
                        } else {
                            c.chassis
                        },
                    ),
                );
                painter.text(
                    egui::pos2(x + 3.0, inner.max.y),
                    egui::Align2::LEFT_BOTTOM,
                    format!("{:02}", i + 1),
                    font.clone(),
                    if active_slice == Some(i) {
                        c.bright
                    } else {
                        c.label
                    },
                );
            }
        }
        // The audition, while it sounds.
        if let Some((a, b)) = editor.playing {
            let x0 = px(a.max(from)).max(inner.min.x);
            let x1 = px(b.min(to)).min(inner.max.x);
            if x1 > x0 {
                let n = c.nominal;
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x0, inner.min.y),
                        egui::pos2(x1, inner.max.y),
                    ),
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(n.r(), n.g(), n.b(), 40),
                );
            }
        }
        // The cursor.
        if editor.cursor >= from && editor.cursor <= to {
            let x = px(editor.cursor).round() - 0.5;
            painter.line_segment(
                [egui::pos2(x, inner.min.y), egui::pos2(x, inner.max.y)],
                egui::Stroke::new(1.5, c.bright),
            );
            crate::ui::nav_cursor::claim(
                painter,
                "cut-cursor",
                egui::Rect::from_min_max(
                    egui::pos2(x - 4.0, inner.min.y),
                    egui::pos2(x + 4.0, inner.max.y),
                ),
                crate::ui::nav_cursor::Kind::Playhead,
                crate::ui::nav_cursor::Layer::Surface,
                c.alert,
            );
            painter.text(
                egui::pos2(x + 3.0, inner.max.y - TYPE_PX - 2.0),
                egui::Align2::LEFT_BOTTOM,
                crate::ui::stage::sample::time_word(editor.cursor, length_seconds),
                font.clone(),
                c.bright,
            );
        }

        // The overview: the whole file, the window on it.
        let overview = egui::Rect::from_min_max(
            egui::pos2(wave.min.x, room.max.y - overview_h),
            egui::pos2(wave.max.x, room.max.y),
        );
        painter.rect_filled(overview, 0.0, c.panel);
        let all = data
            .peaks
            .columns(None, 0.0, 1.0, overview.width().floor().max(1.0) as usize);
        let omid = overview.center().y;
        let ohalf = overview.height() * 0.5 - 1.0;
        for (i, bin) in all.iter().enumerate() {
            let x = overview.min.x + i as f32 + 0.5;
            painter.line_segment(
                [
                    egui::pos2(x, omid - bin.max.clamp(-1.0, 1.0) * ohalf),
                    egui::pos2(x, omid - bin.min.clamp(-1.0, 1.0) * ohalf),
                ],
                egui::Stroke::new(1.0, c.edge),
            );
        }
        let ox = |at: f64| overview.min.x + overview.width() * at as f32;
        painter.rect_stroke(
            egui::Rect::from_min_max(
                egui::pos2(ox(from), overview.min.y),
                egui::pos2(ox(to), overview.max.y),
            ),
            0.0,
            egui::Stroke::new(1.0, c.chassis),
            egui::StrokeKind::Inside,
        );

        // Attributes: the sampler's parameters as rows.
        if editor.page == Page::Attr
            && let Some(device) = device
        {
            let col = egui::Rect::from_min_max(
                egui::pos2(wave.max.x + 12.0, wave.min.y),
                egui::pos2(room.max.x, wave.max.y),
            );
            chassis::frame(painter, col, false);
            let inner = col.shrink(INSET);
            let rows = chain::column(device).rows;
            let fit = ((inner.height()) / ROW_H).floor().max(1.0) as usize;
            for (i, row) in rows.iter().take(fit).enumerate() {
                let y = inner.min.y + i as f32 * ROW_H + ROW_H * 0.5;
                painter.text(
                    egui::pos2(inner.min.x, y),
                    egui::Align2::LEFT_CENTER,
                    &row.name,
                    font.clone(),
                    c.dim,
                );
                painter.text(
                    egui::pos2(inner.max.x, y),
                    egui::Align2::RIGHT_CENTER,
                    &row.value,
                    font.clone(),
                    c.fg,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ruler_step_seconds;

    #[test]
    fn ruler_intervals_are_nice_and_follow_the_visible_duration() {
        assert_eq!(ruler_step_seconds(1.0, 720.0), 0.1);
        assert_eq!(ruler_step_seconds(10.0, 720.0), 1.0);
        assert_eq!(ruler_step_seconds(90.0, 720.0), 10.0);
        assert_eq!(ruler_step_seconds(0.0, 720.0), 1.0);
    }
}
