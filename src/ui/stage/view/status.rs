//! The periphery: a title row above the field and a status strip below
//! it — the machine reporting on itself.
//!
//! Every value here is a real measurement or a document fact, drawn from
//! the transport, the song, and `vitals`. Nothing is invented: a reading
//! the engine has not made yet is `--`, and it keeps its place so the
//! row never jumps. Labels are blue (a name), values are body text, and
//! a value with a BUDGET is coloured by whether it is inside it —
//! `nominal` when fine, `alert` when not, `fault` for the one thing the
//! machine reports as broken about itself: an xrun.

use super::palette;
use crate::PROFONT;
use crate::ui::stage::transport::{DEFAULT_BPM, DEFAULT_METER, Motion};
use crate::ui::stage::vitals::{EngineState, LOAD_WORTH_SAYING};
use eframe::egui;

/// One beat cell on the strip.
/// @tune 4..16 px
const BEAT: f32 = 7.0;
/// The load meter's segments.
const LOAD_SEGMENTS: usize = 8;

/// The title row's height.
/// @tune 12..40 px
pub(super) const TITLE_H: f32 = 22.0;
/// The status strip's height.
/// @tune 12..40 px
pub(super) const STATUS_H: f32 = 22.0;
const TYPE_PX: f32 = 12.0;
const PAD: f32 = 16.0;

/// The rows' heights, live.
pub(super) fn title_h() -> f32 {
    crate::tune!(TITLE_H)
}
pub(super) fn status_h() -> f32 {
    crate::tune!(STATUS_H)
}
/// Between one reading and the next.
const GAP: f32 = 18.0;
/// The mix trace's width on the strip.
/// @tune 40..300 px
const TRACE_W: f32 = 120.0;

/// The wall's clock, hh:mm:ss, local: the system's seconds and the
/// machine's offset, read once at startup.
fn wall_clock(tz_offset: i64) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + tz_offset;
    let (h, m, s) = (
        (secs / 3600).rem_euclid(24),
        (secs / 60).rem_euclid(60),
        secs.rem_euclid(60),
    );
    format!("{h:02}:{m:02}:{s:02}")
}

/// The width a row of readings takes, as `row` lays it out.
fn total_width(readings: &[Reading]) -> f32 {
    let width = |s: &str| s.chars().count() as f32 * TYPE_PX * 0.6;
    readings
        .iter()
        .map(|r| {
            width(r.label)
                + 6.0
                + width(&r.value)
                + if r.meter.is_some() {
                    6.0 + LOAD_SEGMENTS as f32 * 5.0
                } else {
                    0.0
                }
                + GAP
        })
        .sum::<f32>()
        - GAP
}

/// One `label value` pair, the value in whatever colour its state earns.
struct Reading {
    label: &'static str,
    value: String,
    tone: Tone,
    /// A budgeted reading's share of its budget, drawn as a meter after
    /// the value.
    meter: Option<f32>,
}

#[derive(Clone, Copy)]
enum Tone {
    Fact,
    Absent,
    Nominal,
    Alert,
    Fault,
}

fn reading(label: &'static str, value: impl Into<String>, tone: Tone) -> Reading {
    Reading {
        label,
        value: value.into(),
        tone,
        meter: None,
    }
}

/// Lay readings left to right from `x`, returning where the row ended.
fn row(
    painter: &egui::Painter,
    mut x: f32,
    y: f32,
    readings: &[Reading],
    align_right: Option<f32>,
) -> f32 {
    let c = palette::colours();
    let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
    let width = |s: &str| s.chars().count() as f32 * TYPE_PX * 0.6;
    if let Some(right) = align_right {
        let total: f32 = readings
            .iter()
            .map(|r| {
                width(r.label)
                    + 6.0
                    + width(&r.value)
                    + if r.meter.is_some() {
                        6.0 + LOAD_SEGMENTS as f32 * 5.0
                    } else {
                        0.0
                    }
                    + GAP
            })
            .sum::<f32>()
            - GAP;
        x = right - total;
    }
    for r in readings {
        painter.text(
            egui::pos2(x, y),
            egui::Align2::LEFT_CENTER,
            r.label,
            font.clone(),
            c.label,
        );
        x += width(r.label) + 6.0;
        let colour = match r.tone {
            Tone::Fact => c.fg,
            Tone::Absent => c.dim,
            Tone::Nominal => c.nominal,
            Tone::Alert => c.alert,
            Tone::Fault => c.fault,
        };
        painter.text(
            egui::pos2(x, y),
            egui::Align2::LEFT_CENTER,
            &r.value,
            font.clone(),
            colour,
        );
        x += width(&r.value);
        if let Some(share) = r.meter {
            // A graduated meter: segments, the first `share` of them in the
            // reading's own colour, the rest in rule.
            x += 6.0;
            let lit = (share.clamp(0.0, 1.0) * LOAD_SEGMENTS as f32).round() as usize;
            for i in 0..LOAD_SEGMENTS {
                let seg = egui::Rect::from_min_max(
                    egui::pos2(x + i as f32 * 5.0, y - 4.0),
                    egui::pos2(x + i as f32 * 5.0 + 3.0, y + 4.0),
                );
                painter.rect_filled(seg, 0.0, if i < lit { colour } else { c.rule });
            }
            x += LOAD_SEGMENTS as f32 * 5.0;
        }
        x += GAP;
    }
    x - GAP
}

impl super::super::Stage {
    /// The title row: what this surface is, and the document's shape.
    pub(super) fn draw_title(&self, painter: &egui::Painter, strip: egui::Rect) {
        let c = palette::colours();
        let y = strip.center().y;
        let left = [
            reading("", "SESSION", Tone::Fact),
            reading("tr", format!("{:02}", self.song.tracks.len()), Tone::Fact),
            reading(
                "sc",
                format!("{:02}", self.song.session.scenes.len()),
                Tone::Fact,
            ),
            reading(
                "pat",
                format!("{:02}", self.song.patterns.len()),
                Tone::Fact,
            ),
        ];
        let end = row(painter, strip.min.x + PAD, y, &left, None);
        // Where the keys stand, in coordinates: it follows the cursor.
        let at = match (self.inside, self.session_address()) {
            (Some(opened), _) => {
                format!("clip {:02}  tr {:02}", opened.pattern.0, opened.track + 1)
            }
            (None, Some(super::super::Address::Head { track })) => format!("tr {:02}", track + 1),
            (None, Some(super::super::Address::Slot { track, scene })) => {
                format!("tr {:02}  sc {:02}", track + 1, scene + 1)
            }
            (None, Some(super::super::Address::Master)) => "master".to_owned(),
            (None, None) => "--".to_owned(),
        };
        row(
            painter,
            end + GAP * 2.0,
            y,
            &[reading("at", at, Tone::Fact)],
            None,
        );
        // The document: its file, and whether it has unsaved work.
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".to_owned());
        let doc = [
            reading("daw", env!("CARGO_PKG_VERSION"), Tone::Absent),
            reading(
                "song",
                if self.dirty {
                    format!("{name} *")
                } else {
                    name
                },
                if self.dirty { Tone::Alert } else { Tone::Fact },
            ),
        ];
        row(painter, 0.0, y, &doc, Some(strip.max.x - PAD));
        let _ = c;
    }

    /// The status strip: the transport, then the engine.
    pub(super) fn draw_status(&self, painter: &egui::Painter, strip: egui::Rect) {
        let y = strip.center().y;
        let tick = self.transport.tick();
        let place = self.transport.place(&self.song);
        let bpm = self.song.bpm_at(tick, DEFAULT_BPM);
        let (n, d) = self.song.meter_at(tick, DEFAULT_METER);
        let motion = match self.transport.motion() {
            Motion::Stopped => reading("", "STOPPED", Tone::Absent),
            Motion::Rolling => reading("", "ROLLING", Tone::Nominal),
            Motion::Recording => reading("", "RECORDING", Tone::Alert),
        };
        let rolling = self.transport.motion().is_rolling();
        let last_chord = super::telemetry().last_chord.clone();
        let left = [
            reading("bar", place.readout(), Tone::Fact),
            reading("bpm", format!("{bpm:.1}"), Tone::Fact),
            reading("", format!("{n}/{d}"), Tone::Fact),
            reading("", self.transport.mode().word(), Tone::Fact),
            motion,
        ];
        let end = row(painter, strip.min.x + PAD, y, &left, None);
        // The beat, as cells: one per beat of the bar, the current one
        // lit while rolling. It changes because the beat changed —
        // reporting, not motion.
        let c = palette::colours();
        let beat = crate::tune!(BEAT);
        let mut bx = end + GAP;
        for i in 1..=place.beats_per_bar.max(1) {
            let cell = egui::Rect::from_center_size(
                egui::pos2(bx + beat * 0.5, y),
                egui::vec2(beat, beat),
            );
            let on = rolling && i == place.beat;
            painter.rect_filled(cell, 0.0, if on { c.nominal } else { c.rule });
            bx += beat + 3.0;
        }
        // The last chord the codebook bound, until the next one; and a
        // refused keystroke named for the frame it was refused in, so
        // under key repeat it reads as a held mark.
        let mut kx = bx + GAP;
        if let Some(chord) = last_chord {
            kx = row(painter, kx, y, &[reading("key", chord, Tone::Fact)], None) + GAP;
        }
        if let Some(refusal) = &self.refusal {
            let word = format!("{:?}", refusal.reason).to_ascii_lowercase();
            row(
                painter,
                kx,
                y,
                &[reading("refused", word, Tone::Alert)],
                None,
            );
        }

        // The engine. An honest absence occupies its space.
        let stream = self.vitals.stream();
        let health = self.vitals.health();
        let absent = || reading("", "--", Tone::Absent);
        let mut right = vec![
            match stream {
                Some(s) => reading("rate", s.sample_rate.to_string(), Tone::Fact),
                None => reading("rate", "--", Tone::Absent),
            },
            match stream {
                Some(s) => reading("buf", s.buffer_frames.to_string(), Tone::Fact),
                None => reading("buf", "--", Tone::Absent),
            },
            match stream.and_then(|s| s.latency_ms()) {
                Some(ms) => reading("lat", format!("{ms:.1}ms"), Tone::Fact),
                None => reading("lat", "--", Tone::Absent),
            },
            match stream {
                Some(s) => reading("io", format!("{}/{}", s.inputs, s.outputs), Tone::Fact),
                None => reading("io", "--", Tone::Absent),
            },
        ];
        match health {
            Some(h) => {
                right.push(reading(
                    "load",
                    format!("{:.0}%", h.load * 100.0),
                    if h.load < LOAD_WORTH_SAYING {
                        Tone::Nominal
                    } else {
                        Tone::Alert
                    },
                ));
                right.push(reading(
                    "xruns",
                    h.xruns.to_string(),
                    if h.xruns == 0 {
                        Tone::Nominal
                    } else {
                        Tone::Fault
                    },
                ));
                right.push(match h.state {
                    EngineState::Running => reading("", "RUNNING", Tone::Nominal),
                    _ => reading("", "ABSENT", Tone::Absent),
                });
            }
            None => {
                right.push(reading("load", "--", Tone::Absent));
                right.push(reading("xruns", "--", Tone::Absent));
                right.push(absent());
            }
        }
        if let Some(s) = stream {
            right.push(reading("", s.backend, Tone::Fact));
        }
        // The session's clock and the wall's, and the mix's trace: the
        // last seconds of the master meter, one sample a frame.
        let (uptime, trace, tz): (f32, Vec<f32>, i64) = {
            let t = super::telemetry();
            (t.uptime, t.trace.iter().copied().collect(), t.tz_offset)
        };
        right.push(reading("up", super::telemetry::stamp(uptime), Tone::Fact));
        right.push(reading("", wall_clock(tz), Tone::Fact));
        let end = row(painter, 0.0, y, &right, Some(strip.max.x - PAD));
        let c = palette::colours();
        let w = crate::tune!(TRACE_W);
        let x1 = strip.max.x - PAD - (end - 0.0).max(0.0) * 0.0 - total_width(&right) - GAP;
        let x0 = x1 - w;
        let track = egui::Rect::from_min_max(egui::pos2(x0, y - 5.0), egui::pos2(x1, y + 5.0));
        painter.rect_filled(track, 0.0, c.panel);
        let n = trace.len().max(1) as f32;
        let mut pts: Vec<egui::Pos2> = trace
            .iter()
            .enumerate()
            .map(|(i, v)| {
                egui::pos2(
                    x0 + w * i as f32 / n,
                    track.max.y - track.height() * v.clamp(0.0, 1.0),
                )
            })
            .collect();
        if pts.len() >= 2 {
            painter.add(egui::Shape::line(
                std::mem::take(&mut pts),
                egui::Stroke::new(1.0, c.nominal),
            ));
        }
    }
}
