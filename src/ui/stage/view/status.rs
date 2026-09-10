//! The periphery: a title row above the field and a status strip below
//! it — the machine reporting on itself.
//!
//! Every value here is a real measurement or a document fact, drawn from
//! the transport, the song, and `vitals`. Nothing is invented: a reading
//! the engine has not made yet is `--`, and it keeps its place so the
//! row never jumps. Labels are blue (a name), values are body text, and
//! a value with a BUDGET is coloured by whether it is inside it —
//! `nominal` when fine, `alert` when not, and `fault` for a stalled or
//! errored engine—or for an xrun while that engine is otherwise running.

use super::{chassis, palette};
use crate::PROFONT;
use crate::ui::stage::keymap::ScopeContext;
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
/// Air between a field's blue name and its value. One ProFont cell keeps
/// `scope SESSION` and `cmd :` legible as two tokens.
const VALUE_GAP: f32 = TYPE_PX * 0.8;

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
    let occupied = readings
        .iter()
        .map(|r| {
            width(r.label)
                + VALUE_GAP
                + width(&r.value)
                + if r.meter.is_some() {
                    6.0 + LOAD_SEGMENTS as f32 * 5.0
                } else {
                    0.0
                }
                + GAP
        })
        .sum::<f32>();
    if readings.is_empty() {
        0.0
    } else {
        occupied - GAP
    }
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
    Identity,
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

fn metered(label: &'static str, value: impl Into<String>, tone: Tone, share: f32) -> Reading {
    Reading {
        label,
        value: value.into(),
        tone,
        meter: Some(share.clamp(0.0, 1.0)),
    }
}

/// The keyboard authority currently holding the workstation. This is the
/// command-centre breadcrumb: a core state, never a decorative mode name.
fn scope_word(scope: ScopeContext) -> &'static str {
    match scope {
        ScopeContext::Root => "SESSION",
        ScopeContext::Nested => "FIELD",
        ScopeContext::Browser => "BROWSER",
        ScopeContext::Mixer => "MIXER",
        ScopeContext::Chain => "CHAIN",
        ScopeContext::Steps => "STEPS",
        ScopeContext::Clip => "CLIP",
        ScopeContext::Rename => "RENAME",
        ScopeContext::TrigMenu => "TRIG",
        ScopeContext::Plock => "PLOCK",
        ScopeContext::Modulation => "MOD",
        ScopeContext::Sample => "SAMPLE",
        ScopeContext::Song => "SONG",
        ScopeContext::Forge => "FORGE",
        ScopeContext::Deck => "DECK",
        ScopeContext::Matrix => "MATRIX",
        ScopeContext::Lab => "LAB",
        ScopeContext::Kiln => "KILN",
        ScopeContext::KilnFilter => "FILTER",
        ScopeContext::MidiLab => "MIDILAB",
        ScopeContext::LabMenu => "KINDS",
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
        x = right - total_width(readings);
    }
    for r in readings {
        painter.text(
            egui::pos2(x, y),
            egui::Align2::LEFT_CENTER,
            r.label,
            font.clone(),
            c.label,
        );
        x += width(r.label) + VALUE_GAP;
        let colour = match r.tone {
            Tone::Identity => c.dir,
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
        chassis::instrument_rail(painter, strip);
        let y = strip.center().y;
        let left = [
            reading("", "STAGE", Tone::Identity),
            reading("scope", scope_word(self.scope_context()), Tone::Fact),
            reading(
                "cmd",
                if self.palette.is_open() { "OPEN" } else { ":" },
                if self.palette.is_open() {
                    Tone::Nominal
                } else {
                    Tone::Absent
                },
            ),
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
        chassis::rail_splice(painter, strip, end + GAP);
        // Where the keys stand, in coordinates: it follows the cursor.
        let at = match (self.inside, self.session_address()) {
            (Some(opened), _) => {
                format!(
                    "clip {}  tr {:02}",
                    self.song.tag_of(opened.pattern),
                    opened.track + 1
                )
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
        let doc_start = strip.max.x - PAD - total_width(&doc);
        chassis::rail_splice(painter, strip, doc_start - GAP * 0.5);
        row(painter, 0.0, y, &doc, Some(strip.max.x - PAD));
    }

    /// The status strip: the transport, then the engine.
    pub(super) fn draw_status(&self, painter: &egui::Painter, strip: egui::Rect) {
        chassis::instrument_rail(painter, strip);
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
        let mut transport_end = bx;
        if let Some(chord) = last_chord {
            kx = row(painter, kx, y, &[reading("key", chord, Tone::Fact)], None) + GAP;
            transport_end = kx;
        }
        if let Some(refusal) = &self.refusal {
            let word = format!("{:?}", refusal.reason).to_ascii_lowercase();
            transport_end = row(
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
        // At the reference 1280-wide canvas keep the operational core
        // legible rather than letting diagnostics collide with transport.
        // The full I/O and wall-clock register reappears on wide consoles.
        if strip.width() < 1500.0 {
            right.retain(|reading| !matches!(reading.label, "lat" | "io"));
        }
        match health {
            Some(h) => {
                right.push(metered(
                    "load",
                    format!("{:.0}%", h.load * 100.0),
                    if h.load < LOAD_WORTH_SAYING {
                        Tone::Nominal
                    } else {
                        Tone::Alert
                    },
                    h.load,
                ));
                let engine_fault = matches!(
                    &h.state,
                    EngineState::Stalled { .. } | EngineState::Errored(_)
                );
                right.push(reading(
                    "xruns",
                    h.xruns.to_string(),
                    if h.xruns == 0 {
                        Tone::Nominal
                    } else if engine_fault {
                        // The stopped engine owns the one fault colour; the
                        // xrun count remains visible as attention/history.
                        Tone::Alert
                    } else {
                        Tone::Fault
                    },
                ));
                right.push(match &h.state {
                    EngineState::Running => reading("", "RUNNING", Tone::Nominal),
                    // A machine without an audio device is a legitimate
                    // editing state, not a defect. Stalls and backend errors
                    // are the two states that earn the rare fault colour.
                    EngineState::Absent => reading("", "ABSENT", Tone::Absent),
                    EngineState::Stalled { .. } => reading("", "STALLED", Tone::Fault),
                    EngineState::Errored(_) => reading("", "ERROR", Tone::Fault),
                });
            }
            None => {
                right.push(metered("load", "--", Tone::Absent, 0.0));
                right.push(reading("xruns", "--", Tone::Absent));
                right.push(absent());
            }
        }
        if let Some(s) = stream.filter(|_| strip.width() >= 900.0) {
            right.push(reading("", s.backend, Tone::Fact));
        }
        // On the minimum supported canvas, keep the state that answers
        // "is audio healthy?" and release the setup facts to wider desks.
        // This gives transport and engine each an honest, non-overlapping
        // half instead of drawing two complete rows through one another.
        if strip.width() < 900.0 {
            right.retain(|reading| {
                matches!(reading.label, "load" | "xruns") || reading.label.is_empty()
            });
        }
        // The session's clock and the wall's, and the mix's trace: the
        // last seconds of the master meter, one sample a frame.
        let (uptime, trace, tz): (f32, Vec<f32>, i64) = {
            let t = super::telemetry();
            (t.uptime, t.trace.iter().copied().collect(), t.tz_offset)
        };
        if strip.width() >= 1500.0 {
            right.push(reading("up", super::telemetry::stamp(uptime), Tone::Fact));
            right.push(reading("", wall_clock(tz), Tone::Fact));
        }
        let right_start = strip.max.x - PAD - total_width(&right);
        chassis::rail_splice(painter, strip, right_start - GAP * 0.5);
        row(painter, right_start, y, &right, None);
        let c = palette::colours();
        let w = crate::tune!(TRACE_W);
        let x1 = right_start - GAP;
        let x0 = x1 - w;
        let track = egui::Rect::from_min_max(egui::pos2(x0, y - 5.0), egui::pos2(x1, y + 5.0));
        let n = trace.len().max(1) as f32;
        let has_room = x0 >= transport_end + GAP;
        if has_room {
            painter.rect_filled(track, 0.0, c.ground);
        }
        let mut pts: Vec<egui::Pos2> = if has_room && track.width() >= 24.0 {
            trace
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    egui::pos2(
                        x0 + track.width() * i as f32 / n,
                        track.max.y - track.height() * v.clamp(0.0, 1.0),
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        if pts.len() >= 2 {
            painter.add(egui::Shape::line(
                std::mem::take(&mut pts),
                egui::Stroke::new(1.0, c.nominal),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_reading_group_takes_no_negative_space() {
        assert_eq!(total_width(&[]), 0.0);
    }

    #[test]
    fn load_meters_are_bounded_without_falsifying_the_value_word() {
        let high = metered("load", "140%", Tone::Alert, 1.4);
        let low = metered("load", "-2%", Tone::Nominal, -0.02);
        assert_eq!(high.value, "140%");
        assert_eq!(high.meter, Some(1.0));
        assert_eq!(low.value, "-2%");
        assert_eq!(low.meter, Some(0.0));
    }

    #[test]
    fn every_keyboard_scope_has_a_terse_truthful_breadcrumb() {
        let words: Vec<&str> = ScopeContext::ALL.into_iter().map(scope_word).collect();
        assert_eq!(words.len(), ScopeContext::ALL.len());
        assert!(words.iter().all(|word| !word.is_empty() && word.len() <= 7));
        assert_eq!(scope_word(ScopeContext::Root), "SESSION");
        assert_eq!(scope_word(ScopeContext::Mixer), "MIXER");
        assert_eq!(scope_word(ScopeContext::Song), "SONG");
    }
}
