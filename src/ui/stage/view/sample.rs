//! The cutting room: the field, whole, given to one sample.
//!
//! The waveform is the file's own peaks (`sample_peaks`), drawn as one
//! hairline per column from min to max with the rms inside it; the trim
//! is the sampler's start and end, in alert, with what lies outside
//! washed toward the ground; the loop is a nominal region from its
//! start to the out point, its crossfade a wedge at the seam; the fades
//! are ramps at either end; slices are chassis ticks with their
//! numbers; the cursor is bright; a marker in hand glows; the range
//! being auditioned carries a head that walks it. Beneath, a legend of
//! the page's keys, and the whole file with the window on it. Beside
//! the wave, a column that changes with the page: the trim's numbers,
//! the slice table, or the sampler's attributes.

use super::heads;
use super::{chassis, palette};
use crate::PROFONT;
use crate::params::sampler as sp;
use crate::sequencing::Device;
use crate::ui::stage::sample::{self, Marker, Page, SampleEditor};
use eframe::egui;

/// The overview strip's height.
/// @tune 8..40 px
const OVERVIEW_H: f32 = 16.0;
/// The page column's width.
/// @tune 120..400 px
const ATTR_W: f32 = 240.0;
/// The key legend's height, under the wave.
/// @tune 14..30 px
const LEGEND_H: f32 = 18.0;
const TITLE_H: f32 = 22.0;
const ROW_H: f32 = 18.0;
const TYPE_PX: f32 = 12.0;
const INSET: f32 = 10.0;
/// A dashed marker: this much ink, then this much gap.
const DASH: f32 = 4.0;

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

/// The keys a page puts in reach, as the legend writes them: the chord
/// and the word. The globals — arrows, zoom, Tab, P, Escape — are the
/// same on every page and sit first.
fn legend(page: Page, grabbed: bool) -> Vec<(&'static str, &'static str)> {
    if grabbed {
        return vec![
            ("< >", "carry"),
            ("+< >", "carry far"),
            ("Enter", "drop"),
            ("Esc", "drop"),
            ("Z", "snap"),
        ];
    }
    let mut out = vec![("< >", "cursor"), ("^< >", "marker"), ("Up Dn", "zoom")];
    out.extend(match page {
        Page::Trim => vec![
            ("S", "in"),
            ("E", "out"),
            ("L", "loop"),
            ("+Enter", "grab"),
            ("+ -", "xfade"),
            ("F", "fit"),
            ("P", "play"),
        ],
        Page::Slice => vec![
            ("Enter", "cut"),
            ("Bksp", "uncut"),
            ("X", "halve"),
            ("G", "grid"),
            ("T", "onsets"),
            ("+ -", "count"),
            ("[ ]", "eagerness"),
            ("1-0", "pick"),
            (", .", "walk"),
        ],
        Page::Attr => vec![
            ("N", "normalize"),
            ("R", "reverse"),
            ("M", "mode"),
            ("Q", "loop"),
            ("+ -", "gain"),
            ("Z", "snap"),
            ("+P", "play all"),
        ],
    });
    out
}

/// A dashed vertical hairline.
fn dashed_v(painter: &egui::Painter, x: f32, top: f32, bottom: f32, stroke: egui::Stroke) {
    let mut y = top;
    while y < bottom {
        let end = (y + DASH).min(bottom);
        painter.line_segment([egui::pos2(x, y), egui::pos2(x, end)], stroke);
        y += DASH * 2.0;
    }
}

/// A row of the page column: a name on the left, a value on the right,
/// and whether it is the one the cursor is about.
struct ColumnRow {
    name: String,
    value: String,
    live: bool,
}

fn row(name: impl Into<String>, value: impl Into<String>) -> ColumnRow {
    ColumnRow {
        name: name.into(),
        value: value.into(),
        live: false,
    }
}

fn choice(names: &[&str], value: f32) -> String {
    names
        .get(value.round().max(0.0) as usize)
        .map_or_else(|| "?".to_owned(), |name| (*name).to_owned())
}

/// What the column says on each page.
fn column_rows(
    page: Page,
    editor: &SampleEditor,
    device: &Device,
    data: Option<&sample::SampleData>,
) -> (String, Vec<ColumnRow>) {
    let seconds = data.map_or(0.0, sample::SampleData::seconds);
    let word = |at: f64| sample::time_word(at, seconds);
    let start = f64::from(device.value(sp::START));
    let end = f64::from(device.value(sp::END));
    let loop_mode = device.value(sp::LOOP_MODE);
    match page {
        Page::Trim => {
            let looped = loop_mode.round() >= 1.0;
            let mut rows = vec![
                row("in", word(start)),
                row("out", word(end)),
                row("length", word((end - start).max(0.0))),
                row("loop", choice(sp::LOOP_NAMES, loop_mode)),
            ];
            if looped {
                rows.push(row(
                    "loop from",
                    format!(
                        "{} · {:.0}%",
                        word(sample::loop_at(device)),
                        device.value(sp::LOOP_START) * 100.0
                    ),
                ));
                rows.push(row(
                    "crossfade",
                    format!("{:.0} ms", device.value(sp::LOOP_XFADE)),
                ));
            }
            rows.push(row(
                "fade in",
                format!("{:.0} ms", device.value(sp::FADE_IN)),
            ));
            rows.push(row(
                "fade out",
                format!("{:.0} ms", device.value(sp::FADE_OUT)),
            ));
            rows.push(row("snap", if editor.snap { "zero" } else { "free" }));
            for (index, r) in rows.iter_mut().enumerate() {
                r.live = match editor.grabbed {
                    Some(Marker::Start) => index == 0,
                    Some(Marker::End) => index == 1,
                    Some(Marker::Loop) => index == 4,
                    _ => false,
                };
            }
            ("TRIM".to_owned(), rows)
        }
        Page::Slice => {
            let active = editor.slice_at(device);
            let source = choice(sp::SLICE_SOURCE_NAMES, device.value(sp::SLICE_SOURCE));
            let title = format!("SLICES {:02} · {source}", device.slices.len());
            let mut rows = vec![
                row("G lays", format!("{} equal", editor.count)),
                row(
                    "T finds",
                    format!("onsets at {:.0}%", editor.sensitivity * 100.0),
                ),
                row("", ""),
            ];
            rows.extend(device.slices.iter().enumerate().map(|(index, at)| {
                let to = device.slices.get(index + 1).copied().unwrap_or(1.0);
                ColumnRow {
                    name: format!("{:02}", index + 1),
                    value: format!("{} · {}", word(*at), word((to - at).max(0.0))),
                    live: active == Some(index) || editor.grabbed == Some(Marker::Slice(index)),
                }
            }));
            (title, rows)
        }
        Page::Attr => {
            let gain = device.value(sp::GAIN);
            let rows = vec![
                row("mode", choice(sp::MODE_NAMES, device.value(sp::MODE))),
                row("loop", choice(sp::LOOP_NAMES, loop_mode)),
                row(
                    "direction",
                    if device.value(sp::REVERSE).round() >= 1.0 {
                        "reverse"
                    } else {
                        "forward"
                    },
                ),
                row("gain", format!("{gain:+.1} dB")),
                row("tune", format!("{:+.0} st", device.value(sp::TUNE))),
                row("fine", format!("{:+.0} ct", device.value(sp::FINE))),
                row("root", format!("{:.0}", device.value(sp::ROOT))),
                row(
                    "file",
                    data.map_or_else(
                        || "--".to_owned(),
                        |d| {
                            format!(
                                "{:.2}s · {} Hz · {}ch",
                                d.seconds(),
                                d.sample_rate,
                                d.channels
                            )
                        },
                    ),
                ),
                row(
                    "peak",
                    data.map_or_else(
                        || "--".to_owned(),
                        |d| format!("{:+.1} dBFS", -d.normalizing_db()),
                    ),
                ),
            ];
            ("ATTRIBUTES".to_owned(), rows)
        }
    }
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
        if let Some(marker) = editor.grabbed {
            painter.text(
                egui::pos2(x + ch, ty),
                egui::Align2::LEFT_CENTER,
                format!("HOLDING {}", marker.word().to_ascii_uppercase()),
                font.clone(),
                c.alert,
            );
        }
        let active_slice = device.and_then(|device| editor.slice_at(device));
        let facts: Vec<(&str, String)> = match (data, device) {
            (Some(d), Some(device)) => {
                let reversed = device.value(sp::REVERSE).round() >= 1.0;
                vec![
                    ("len", format!("{:.2}s", d.seconds())),
                    ("mode", choice(sp::MODE_NAMES, device.value(sp::MODE))),
                    ("dir", if reversed { "rev".into() } else { "fwd".into() }),
                    ("gain", format!("{:+.1}", device.value(sp::GAIN))),
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
                            |index| format!("{:02}/{:02}", index + 1, device.slices.len()),
                        ),
                    ),
                ]
            }
            _ => vec![("len", "--".into()), ("mode", "--".into())],
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

        // The wave, the legend under it, the overview at the foot, and
        // the page's column beside all three.
        let attr_w = crate::tune!(ATTR_W) + 12.0;
        let overview_h = crate::tune!(OVERVIEW_H);
        let legend_h = crate::tune!(LEGEND_H);
        let wave = egui::Rect::from_min_max(
            egui::pos2(room.min.x, room.min.y + TITLE_H + 4.0),
            egui::pos2(
                room.max.x - attr_w,
                room.max.y - overview_h - 8.0 - legend_h - 4.0,
            ),
        );
        chassis::frame(painter, wave, editor.grabbed.is_none());
        let inner = wave.shrink(INSET);

        // The legend: the page's keys, chord then word.
        let ly = wave.max.y + 4.0 + legend_h * 0.5;
        let mut lx = wave.min.x;
        for (chord, word) in legend(editor.page, editor.grabbed.is_some()) {
            let width =
                (chord.chars().count() as f32 + 1.0 + word.chars().count() as f32 + 2.5) * ch;
            if lx + width > wave.max.x {
                break;
            }
            painter.text(
                egui::pos2(lx, ly),
                egui::Align2::LEFT_CENTER,
                chord,
                font.clone(),
                if editor.grabbed.is_some() {
                    c.alert
                } else {
                    c.bright
                },
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

        // The column: what the page has to say, in rows.
        if let Some(device) = device {
            let col = egui::Rect::from_min_max(
                egui::pos2(wave.max.x + 12.0, wave.min.y),
                egui::pos2(room.max.x, room.max.y),
            );
            chassis::frame(painter, col, false);
            let inner = col.shrink(INSET);
            let (title, rows) = column_rows(editor.page, editor, device, data);
            painter.text(
                egui::pos2(inner.min.x, inner.min.y + ROW_H * 0.5),
                egui::Align2::LEFT_CENTER,
                &title,
                font.clone(),
                c.label,
            );
            let top = inner.min.y + ROW_H + 4.0;
            let fit = ((inner.max.y - top) / ROW_H).floor().max(1.0) as usize;
            // Keep the live row in view: a long slice table scrolls to it.
            let live = rows.iter().position(|row| row.live).unwrap_or(0);
            let first = if rows.len() <= fit {
                0
            } else {
                live.saturating_sub(fit / 2).min(rows.len() - fit)
            };
            for (i, row) in rows.iter().skip(first).take(fit).enumerate() {
                let y = top + i as f32 * ROW_H + ROW_H * 0.5;
                if row.live {
                    let s = c.select;
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(inner.min.x - 4.0, y - ROW_H * 0.5),
                            egui::pos2(inner.max.x + 4.0, y + ROW_H * 0.5),
                        ),
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(s.r(), s.g(), s.b(), 54),
                    );
                }
                painter.text(
                    egui::pos2(inner.min.x, y),
                    egui::Align2::LEFT_CENTER,
                    &row.name,
                    font.clone(),
                    if row.live { c.bright } else { c.dim },
                );
                painter.text(
                    egui::pos2(inner.max.x, y),
                    egui::Align2::RIGHT_CENTER,
                    &row.value,
                    font.clone(),
                    if row.live { c.bright } else { c.fg },
                );
            }
            if rows.len() > first + fit {
                painter.text(
                    egui::pos2(inner.max.x, inner.max.y),
                    egui::Align2::RIGHT_BOTTOM,
                    format!("+{}", rows.len() - first - fit),
                    font.clone(),
                    c.dim,
                );
            }
        }

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
        let seconds_of = |ms: f32| {
            if length_seconds > 0.0 {
                f64::from(ms.max(0.0)) / 1000.0 / length_seconds
            } else {
                0.0
            }
        };

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
                        let label = sample::time_word(t / length_seconds, length_seconds);
                        if x + 3.0 + label.len() as f32 * ch <= inner.max.x {
                            painter.text(
                                egui::pos2(x + 3.0, ry + 1.0),
                                egui::Align2::LEFT_TOP,
                                label,
                                font.clone(),
                                c.dim,
                            );
                        }
                    }
                }
                t += step;
            }
        }

        // Peaks: one column per pixel. With gain on the sampler, the
        // envelope it will actually play is ghosted over the file's own,
        // and where that would clip, the ghost turns to fault.
        let gain_db = device.map_or(0.0, |device| device.value(sp::GAIN));
        let gain = 10.0_f32.powf(gain_db / 20.0);
        let ghosted = (gain_db.abs() > 0.05) && gain.is_finite();
        let columns = inner.width().floor().max(1.0) as usize;
        let bins = data.peaks.columns(Some(&data.samples), from, to, columns);
        for (i, bin) in bins.iter().enumerate() {
            let x = inner.min.x + i as f32 + 0.5;
            let (lo, hi) = (bin.min.clamp(-1.0, 1.0), bin.max.clamp(-1.0, 1.0));
            if ghosted {
                let (glo, ghi) = (bin.min * gain, bin.max * gain);
                let clipped = glo < -1.0 || ghi > 1.0;
                painter.line_segment(
                    [
                        egui::pos2(x, mid - ghi.clamp(-1.0, 1.0) * half),
                        egui::pos2(x, mid - glo.clamp(-1.0, 1.0) * half),
                    ],
                    egui::Stroke::new(1.0, if clipped { c.fault } else { c.rule }),
                );
            }
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
            // The loop: from its start to the out point, in nominal, with
            // the crossfade drawn as the wedge it is — the tail of the
            // loop fading in the material from before its start.
            let loop_mode = device.value(sp::LOOP_MODE).round();
            if loop_mode >= 1.0 && end > start {
                let loop_at = sample::loop_at(device);
                let n = c.nominal;
                let a = loop_at.max(from);
                let b = end.min(to);
                if b > a {
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(px(a), inner.min.y),
                            egui::pos2(px(b), inner.max.y),
                        ),
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(n.r(), n.g(), n.b(), 22),
                    );
                }
                let xfade = seconds_of(device.value(sp::LOOP_XFADE))
                    .min((end - loop_at) * 0.5)
                    .min(loop_at);
                if xfade > 0.0 && loop_mode == sp::LOOP_FORWARD.round() {
                    for (seam, dir) in [(end, -1.0f64), (loop_at, -1.0)] {
                        let x0 = px(seam + dir * xfade);
                        let x1 = px(seam);
                        if x1 > inner.min.x && x0 < inner.max.x {
                            let (x0, x1) = (x0.max(inner.min.x), x1.min(inner.max.x));
                            let wedge = vec![
                                egui::pos2(x0, inner.max.y),
                                egui::pos2(x1, inner.max.y),
                                egui::pos2(x1, inner.max.y - inner.height() * 0.25),
                            ];
                            painter.add(egui::Shape::convex_polygon(
                                wedge,
                                egui::Color32::from_rgba_unmultiplied(n.r(), n.g(), n.b(), 60),
                                egui::Stroke::new(1.0, n),
                            ));
                        }
                    }
                }
                if loop_at >= from && loop_at <= to {
                    let x = px(loop_at).round() - 0.5;
                    let held = editor.grabbed == Some(Marker::Loop);
                    dashed_v(
                        painter,
                        x,
                        inner.min.y,
                        inner.max.y,
                        egui::Stroke::new(
                            if held { 2.0 } else { 1.0 },
                            if held { c.bright } else { n },
                        ),
                    );
                    painter.text(
                        egui::pos2(x + 3.0, inner.min.y + 10.0),
                        egui::Align2::LEFT_TOP,
                        if loop_mode == sp::LOOP_PINGPONG.round() {
                            "loop <>"
                        } else {
                            "loop >"
                        },
                        font.clone(),
                        if held { c.bright } else { n },
                    );
                }
            }
            // The fades: a ramp in from the in point, a ramp out to the
            // out point, each as long as its milliseconds.
            let fade_in = seconds_of(device.value(sp::FADE_IN));
            let fade_out = seconds_of(device.value(sp::FADE_OUT));
            if fade_in > 0.0 {
                painter.line_segment(
                    [
                        egui::pos2(px(start).max(inner.min.x), inner.max.y),
                        egui::pos2(
                            px(start + fade_in).clamp(inner.min.x, inner.max.x),
                            inner.min.y,
                        ),
                    ],
                    egui::Stroke::new(1.0, c.dim),
                );
            }
            if fade_out > 0.0 {
                painter.line_segment(
                    [
                        egui::pos2(
                            px(end - fade_out).clamp(inner.min.x, inner.max.x),
                            inner.min.y,
                        ),
                        egui::pos2(px(end).min(inner.max.x), inner.max.y),
                    ],
                    egui::Stroke::new(1.0, c.dim),
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
            for (frame, word, marker) in [(start, "in", Marker::Start), (end, "out", Marker::End)] {
                if frame >= from && frame <= to {
                    let x = px(frame).round() - 0.5;
                    let held = editor.grabbed == Some(marker);
                    painter.line_segment(
                        [egui::pos2(x, inner.min.y), egui::pos2(x, inner.max.y)],
                        egui::Stroke::new(
                            if held { 2.0 } else { 1.0 },
                            if held { c.bright } else { c.alert },
                        ),
                    );
                    painter.text(
                        egui::pos2(x + 3.0, inner.max.y - TYPE_PX - 4.0),
                        egui::Align2::LEFT_BOTTOM,
                        word,
                        font.clone(),
                        if held { c.bright } else { c.alert },
                    );
                }
            }
            // Slices: a tick and a number each.
            for (i, slice) in device.slices.iter().enumerate() {
                if *slice < from || *slice > to {
                    continue;
                }
                let x = px(*slice).round() - 0.5;
                let held = editor.grabbed == Some(Marker::Slice(i));
                painter.line_segment(
                    [egui::pos2(x, inner.min.y), egui::pos2(x, inner.max.y)],
                    egui::Stroke::new(
                        if held || active_slice == Some(i) {
                            2.0
                        } else {
                            1.0
                        },
                        if held {
                            c.bright
                        } else if active_slice == Some(i) {
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
                    if held || active_slice == Some(i) {
                        c.bright
                    } else {
                        c.label
                    },
                );
            }
        }
        // The audition, while it sounds: what has played is washed, and
        // the head stands where the clock says it is.
        if let Some(head) = editor.playing.and_then(|playing| {
            playing
                .head(length_seconds)
                .map(|head| (playing.from, head))
        }) {
            let (a, head) = head;
            let x0 = px(a.max(from)).max(inner.min.x);
            let x1 = px(head.min(to)).min(inner.max.x);
            let n = c.nominal;
            if x1 > x0 {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x0, inner.min.y),
                        egui::pos2(x1, inner.max.y),
                    ),
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(n.r(), n.g(), n.b(), 40),
                );
            }
            if head >= from && head <= to {
                let x = px(head).round() - 0.5;
                painter.line_segment(
                    [egui::pos2(x, inner.min.y), egui::pos2(x, inner.max.y)],
                    egui::Stroke::new(2.0, n),
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
                sample::time_word(editor.cursor, length_seconds),
                font.clone(),
                c.bright,
            );
        }

        // The overview: the whole file, the window on it, and the trim
        // and slices as ticks so the whole shape reads at a glance.
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
        if let Some(device) = device {
            for slice in &device.slices {
                let x = ox(*slice).round() - 0.5;
                painter.line_segment(
                    [egui::pos2(x, overview.min.y), egui::pos2(x, overview.max.y)],
                    egui::Stroke::new(1.0, c.chassis),
                );
            }
            for at in [device.value(sp::START), device.value(sp::END)] {
                let x = ox(f64::from(at)).round() - 0.5;
                painter.line_segment(
                    [egui::pos2(x, overview.min.y), egui::pos2(x, overview.max.y)],
                    egui::Stroke::new(1.0, c.alert),
                );
            }
        }
        painter.rect_stroke(
            egui::Rect::from_min_max(
                egui::pos2(ox(from), overview.min.y),
                egui::pos2(ox(to), overview.max.y),
            ),
            0.0,
            egui::Stroke::new(1.0, c.chassis),
            egui::StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{legend, ruler_step_seconds};
    use crate::ui::stage::sample::Page;

    #[test]
    fn ruler_intervals_are_nice_and_follow_the_visible_duration() {
        assert_eq!(ruler_step_seconds(1.0, 720.0), 0.1);
        assert_eq!(ruler_step_seconds(10.0, 720.0), 1.0);
        assert_eq!(ruler_step_seconds(90.0, 720.0), 10.0);
        assert_eq!(ruler_step_seconds(0.0, 720.0), 1.0);
    }

    #[test]
    fn every_page_has_a_legend_and_a_hand_in_use_has_its_own() {
        for page in Page::ALL {
            let keys = legend(page, false);
            assert!(keys.len() >= 6, "{page:?} legend is thin");
            assert!(
                keys.iter()
                    .all(|(chord, word)| !chord.is_empty() && !word.is_empty())
            );
        }
        let held = legend(Page::Trim, true);
        assert!(held.iter().any(|(_, word)| *word == "drop"));
        assert!(
            held.iter().all(|(chord, _)| *chord != "S"),
            "S is not a key while holding"
        );
    }
}
