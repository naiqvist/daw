//! What the machine has been doing: a log of the core's state changes,
//! and a trace of the mix.
//!
//! Nothing here is fabricated and nothing runs on a timer. Each frame the
//! view compares the core's state with the frame before and writes a
//! line for what changed — a launch, a stop, a refusal, a chord, a door
//! opened — stamped with the session's own clock. The trace is the
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

#[derive(Clone, Debug)]
pub struct Line {
    /// Session seconds when it happened.
    pub at: f32,
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
    inside: Option<(u64, usize)>,
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
    fn push(&mut self, verb: &'static str, what: impl Into<String>) {
        if self.lines.len() == LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(Line {
            at: self.uptime,
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
                self.push("KEY", chord);
            }
        }
        let now = now.0;
        if self.primed {
            let was = self.was.clone();
            if now.motion != was.motion {
                match now.motion {
                    Some(Motion::Rolling) => self.push("ROLL", ""),
                    Some(Motion::Recording) => self.push("RECORD", ""),
                    Some(Motion::Stopped) => self.push("STOP", ""),
                    None => {}
                }
            }
            for (track, (a, b)) in was.playing.iter().zip(now.playing.iter()).enumerate() {
                if a != b {
                    match b {
                        Some(scene) => {
                            self.push("LAUNCH", format!("tr{:02} sc{:02}", track + 1, scene + 1))
                        }
                        None => self.push("SILENCE", format!("tr{:02}", track + 1)),
                    }
                }
            }
            if now.inside != was.inside {
                match now.inside {
                    Some((pattern, track)) => {
                        self.push("ENTER", format!("clip {pattern:02} tr{:02}", track + 1))
                    }
                    None => self.push("LEAVE", "clip"),
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
                    self.push(if b { "OPEN" } else { "CLOSE" }, name);
                }
            }
            if now.tracks != was.tracks {
                self.push("TRACKS", format!("{} -> {}", was.tracks, now.tracks));
            }
            if now.scenes != was.scenes {
                self.push("SCENES", format!("{} -> {}", was.scenes, now.scenes));
            }
            if now.dirty && !was.dirty {
                self.push("EDIT", "unsaved");
            }
            if !now.dirty && was.dirty {
                self.push("SAVED", "");
            }
            if now.refusal.is_some()
                && now.refusal != was.refusal
                && let Some(reason) = &now.refusal
            {
                self.push("REFUSED", reason.clone());
            }
            if now.notice.is_some()
                && now.notice != was.notice
                && let Some(notice) = &now.notice
            {
                self.push("NOTICE", notice.clone());
            }
        } else {
            self.push("BOOT", "view up");
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
        inside: Option<(u64, usize)>,
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
