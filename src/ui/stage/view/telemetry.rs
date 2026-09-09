//! What the machine has been doing: a log of the core's state changes,
//! and a trace of the mix.
//!
//! Nothing here is fabricated and nothing runs on a timer. Each frame the
//! view compares the core's state with the frame before and writes a
//! line for what changed — a launch, a stop, a refusal, a chord, a door
//! opened — stamped with the view's measured uptime. The trace is the
//! master meter's reading, one sample per frame while sound passes.
//! Both are the view's memory, not the document's: a project reopened
//! tomorrow starts with an empty log.

use crate::ui::stage::key::{Key, Mods};
use crate::ui::stage::keymap::{self, StageInput};
use crate::ui::stage::transport::Motion;
use std::collections::VecDeque;

/// How many lines the log keeps.
pub const LINES: usize = 40;
/// How many trace samples the strip keeps.
pub const TRACE: usize = 240;

/// How urgently a logged fact should be read. This is assigned where
/// the fact is observed, never inferred later from its prose.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Info,
    /// A transport or clip actually became live.
    Live,
    /// The machine refused something or left work requiring attention.
    Attention,
}

impl Severity {
    pub const fn word(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Live => "LIVE",
            Self::Attention => "ATTN",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Line {
    /// View uptime in seconds when it happened.
    pub at: f32,
    pub severity: Severity,
    /// A short, capitalised verb: LAUNCH, STOP, REFUSED, KEY …
    pub verb: &'static str,
    /// The rest, in words.
    pub what: String,
}

/// The frame before, as much of it as the log compares.
#[derive(Clone, Debug, Default, PartialEq)]
struct Was {
    motion: Option<Motion>,
    playing: Vec<Option<usize>>,
    /// The open clip's tag and track.
    inside: Option<(String, usize)>,
    chain: bool,
    browser: bool,
    help: bool,
    mixing: bool,
    song_view: bool,
    sample: bool,
    room: bool,
    dirty: bool,
    tracks: usize,
    scenes: usize,
    refusal: Option<String>,
    notice: Option<String>,
}

pub struct Telemetry {
    /// Seconds since the view first drew.
    pub uptime: f32,
    /// The machine's offset from UTC in seconds, read once from `date`
    /// when the view first draws — one process at startup, never again.
    pub tz_offset: i64,
    /// The last chord the codebook bound, until the next replaces it.
    pub last_chord: Option<String>,
    pub lines: VecDeque<Line>,
    pub trace: VecDeque<f32>,
    was: Was,
    primed: bool,
}

impl Default for Telemetry {
    fn default() -> Self {
        let tz_offset = std::process::Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| {
                let s = s.trim();
                let sign = if s.starts_with('-') { -1 } else { 1 };
                let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
                let hh: i64 = digits.get(0..2)?.parse().ok()?;
                let mm: i64 = digits.get(2..4)?.parse().ok()?;
                Some(sign * (hh * 3600 + mm * 60))
            })
            .unwrap_or(0);
        Self {
            uptime: 0.0,
            tz_offset,
            last_chord: None,
            lines: VecDeque::new(),
            trace: VecDeque::new(),
            was: Was::default(),
            primed: false,
        }
    }
}

impl Telemetry {
    /// A thing a document lost on the way in. The core hands these over
    /// after an open; they are the migration's receipt.
    pub fn dropped(&mut self, what: impl Into<String>) {
        self.push(Severity::Attention, "DROPPED", what);
    }

    fn push(&mut self, severity: Severity, verb: &'static str, what: impl Into<String>) {
        if self.lines.len() == LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(Line {
            at: self.uptime,
            severity,
            verb,
            what: what.into(),
        });
    }

    /// One frame: the clock, the keys that were bound this frame, the
    /// changes since the frame before, and the mix's reading.
    #[allow(clippy::too_many_arguments)]
    pub fn observe(&mut self, dt: f32, chords: &[StageInput], now: Was_, level: f32) {
        self.uptime += dt.clamp(0.0, 0.25);
        for input in chords {
            if let StageInput::Chord(mods, key) = input {
                let chord = carve(*mods, *key);
                self.last_chord = Some(chord.clone());
                self.push(Severity::Info, "KEY", chord);
            }
        }
        let now = now.0;
        if self.primed {
            let was = self.was.clone();
            if now.motion != was.motion {
                match now.motion {
                    Some(Motion::Rolling) => self.push(Severity::Live, "ROLL", ""),
                    Some(Motion::Recording) => self.push(Severity::Live, "RECORD", ""),
                    Some(Motion::Stopped) => self.push(Severity::Info, "STOP", ""),
                    None => {}
                }
            }
            for (track, (a, b)) in was.playing.iter().zip(now.playing.iter()).enumerate() {
                if a != b {
                    match b {
                        Some(scene) => self.push(
                            Severity::Live,
                            "LAUNCH",
                            format!("tr{:02} sc{:02}", track + 1, scene + 1),
                        ),
                        None => self.push(Severity::Info, "SILENCE", format!("tr{:02}", track + 1)),
                    }
                }
            }
            if now.inside != was.inside {
                match now.inside.clone() {
                    Some((pattern, track)) => self.push(
                        Severity::Info,
                        "ENTER",
                        format!("clip {pattern} tr{:02}", track + 1),
                    ),
                    None => self.push(Severity::Info, "LEAVE", "clip"),
                }
            }
            for (name, a, b) in [
                ("band", was.chain, now.chain),
                ("archive", was.browser, now.browser),
                ("codebook", was.help, now.help),
                ("mixer", was.mixing, now.mixing),
                ("song view", was.song_view, now.song_view),
                ("cutting room", was.sample, now.sample),
                ("machine room", was.room, now.room),
            ] {
                if a != b {
                    self.push(Severity::Info, if b { "OPEN" } else { "CLOSE" }, name);
                }
            }
            if now.tracks != was.tracks {
                self.push(
                    Severity::Info,
                    "TRACKS",
                    format!("{} -> {}", was.tracks, now.tracks),
                );
            }
            if now.scenes != was.scenes {
                self.push(
                    Severity::Info,
                    "SCENES",
                    format!("{} -> {}", was.scenes, now.scenes),
                );
            }
            if now.dirty && !was.dirty {
                self.push(Severity::Attention, "EDIT", "unsaved");
            }
            if !now.dirty && was.dirty {
                self.push(Severity::Info, "SAVED", "");
            }
            if now.refusal.is_some()
                && now.refusal != was.refusal
                && let Some(reason) = &now.refusal
            {
                self.push(Severity::Attention, "REFUSED", reason.clone());
            }
            if now.notice.is_some()
                && now.notice != was.notice
                && let Some(notice) = &now.notice
            {
                self.push(Severity::Attention, "NOTICE", notice.clone());
            }
        } else {
            self.push(Severity::Info, "BOOT", "view up");
        }
        self.was = now;
        self.primed = true;
        if self.trace.len() == TRACE {
            self.trace.pop_front();
        }
        self.trace.push_back(level.clamp(0.0, 1.5));
    }
}

/// The frame's facts the log compares, gathered by the view from the core.
pub struct Was_(Was);

impl Was_ {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        motion: Motion,
        playing: &[Option<usize>],
        inside: Option<(String, usize)>,
        chain: bool,
        browser: bool,
        help: bool,
        mixing: bool,
        song_view: bool,
        sample: bool,
        room: bool,
        dirty: bool,
        tracks: usize,
        scenes: usize,
        refusal: Option<String>,
        notice: Option<String>,
    ) -> Self {
        Self(Was {
            motion: Some(motion),
            playing: playing.to_vec(),
            inside,
            chain,
            browser,
            help,
            mixing,
            song_view,
            sample,
            room,
            dirty,
            tracks,
            scenes,
            refusal,
            notice,
        })
    }
}

/// A chord as the codebook carves it.
fn carve(mods: Mods, key: Key) -> String {
    crate::ui::stage::carved_chord(&keymap::chord_name(mods, key))
}

/// `mm:ss.t` of session time.
pub fn stamp(at: f32) -> String {
    let m = (at / 60.0).floor() as u32;
    let s = at - m as f32 * 60.0;
    format!("{m:02}:{s:04.1}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_words_are_stable_console_columns() {
        assert_eq!(Severity::Info.word(), "INFO");
        assert_eq!(Severity::Live.word(), "LIVE");
        assert_eq!(Severity::Attention.word(), "ATTN");
    }

    #[test]
    fn uptime_is_fixed_width_to_a_tenth() {
        assert_eq!(stamp(0.0), "00:00.0");
        assert_eq!(stamp(65.25), "01:05.2");
    }
}
