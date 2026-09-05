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

/// One `label value` pair, the value in whatever colour its state earns.
struct Reading {
    label: &'static str,
    value: String,
    tone: Tone,
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
    }
}

/// Lay readings left to right from `x`, returning where the row ended.
fn row(
    painter: &egui::Painter,
    mut x: f32,
    y: f32,
    readings: &[Reading],
    align_right: Option<f32>,
) {
    let c = palette::colours();
    let font = egui::FontId::new(TYPE_PX, egui::FontFamily::Name(PROFONT.into()));
    let width = |s: &str| s.chars().count() as f32 * TYPE_PX * 0.6;
    if let Some(right) = align_right {
        let total: f32 = readings
            .iter()
            .map(|r| width(r.label) + 6.0 + width(&r.value) + GAP)
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
        x += width(&r.value) + GAP;
    }
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
        row(painter, strip.min.x + PAD, y, &left, None);
        // The document: its file, and whether it has unsaved work.
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".to_owned());
        let doc = [reading(
            "song",
            if self.dirty {
                format!("{name} *")
            } else {
                name
            },
            if self.dirty { Tone::Alert } else { Tone::Fact },
        )];
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
        let left = [
            reading("bar", place.readout(), Tone::Fact),
            reading("bpm", format!("{bpm:.1}"), Tone::Fact),
            reading("", format!("{n}/{d}"), Tone::Fact),
            reading("", self.transport.mode().word(), Tone::Fact),
            motion,
        ];
        row(painter, strip.min.x + PAD, y, &left, None);

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
        row(painter, 0.0, y, &right, Some(strip.max.x - PAD));
    }
}
