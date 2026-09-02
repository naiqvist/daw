//! What the engine says about itself, kept readable.
//!
//! Two kinds of fact arrive here, both pushed in by the host — the stage
//! never asks the engine anything — and both need MEMORY before they can
//! be drawn:
//!
//! - **Health.** Whether there is an engine, whether it is still
//!   delivering blocks, and how many it has dropped. A dropped block is a
//!   glitch that was probably heard, and a counter that quietly goes from
//!   3 to 4 is not a report of it — so an xrun FLASHES, for long enough
//!   to be seen, and the count stays behind for the reader who looks
//!   later.
//! - **Levels.** The engine reports one peak per block, and a block is a
//!   few milliseconds. Drawn raw, that is a meter that flickers between
//!   the loudest and quietest block of every frame. So the levels ride
//!   the card tier's own ballistics: instant attack, a slow release, and
//!   a peak mark that holds — the same motion every meter in the app has.

use super::Level;
use super::mixer::Reading;
use crate::ui::device::meter::{self, Ballistics};

/// How long an xrun is shown loudly. Long enough to be seen by someone
/// looking at the session rather than the strip; short enough that a
/// single glitch does not read as an alarm that never clears.
pub const XRUN_FLASH_S: f32 = 2.0;

/// The share of the callback's deadline past which the load is worth a
/// word: the point where the next thing added is likely to be the first
/// xrun.
pub const LOAD_WORTH_SAYING: f32 = 0.75;

/// Whether the stream is alive, in the engine's own terms.
#[derive(Clone, Debug, PartialEq)]
pub enum EngineState {
    /// No device would open. The stage still runs, and says so.
    Absent,
    Running,
    /// Blocks stopped arriving: the server died silently or the graph was
    /// torn down.
    Stalled {
        seconds: f32,
    },
    /// The backend reported an error.
    Errored(String),
}

/// One report from the host, as of this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Health {
    pub state: EngineState,
    /// Blocks the device under- or over-ran since the stream started.
    pub xruns: u64,
    /// The share of the deadline the last callback spent. Past 1.0 the
    /// deadline was missed.
    pub load: f32,
}

/// How a line on the strip should be inked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tone {
    /// A fact worth reading if you look.
    Quiet,
    /// A fact that must be seen.
    Alarm,
}

/// The stream's standing facts: what the engine opened, as opposed to
/// how it is doing. These change only when the device does, so they
/// are handed in separately from `Health` and kept until replaced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stream {
    pub sample_rate: u32,
    pub buffer_frames: u32,
    /// The device's reported round trip, when it reports one.
    pub latency_frames: Option<u32>,
    pub inputs: u8,
    pub outputs: u8,
    /// The backend's name, for its seal.
    pub backend: &'static str,
}

impl Stream {
    /// The stream's latency in milliseconds, when known.
    pub fn latency_ms(&self) -> Option<f32> {
        let frames = self.latency_frames?;
        (self.sample_rate > 0).then(|| frames as f32 * 1000.0 / self.sample_rate as f32)
    }
}

/// The engine's health, with the memory that makes an xrun visible.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vitals {
    /// The last report, or `None` while nothing has reported. The
    /// difference is drawn: no report is a stage nobody has wired, and
    /// `Absent` is a host that tried and could not.
    health: Option<Health>,
    /// The stream's facts, while there is a stream.
    stream: Option<Stream>,
    /// The count the last report carried, so a rise can be noticed.
    seen_xruns: u64,
    /// Seconds of flash left.
    flash: f32,
}

impl Vitals {
    /// Take the host's report. A RISE in the xrun count starts the flash;
    /// the first report never does, because the count it carries is
    /// history from before anyone was watching.
    pub fn report(&mut self, health: Health) {
        if self.health.is_some() && health.xruns > self.seen_xruns {
            self.flash = XRUN_FLASH_S;
        }
        self.seen_xruns = health.xruns;
        self.health = Some(health);
    }

    /// Take the stream's facts, or their absence.
    pub fn set_stream(&mut self, stream: Option<Stream>) {
        self.stream = stream;
    }

    pub fn stream(&self) -> Option<&Stream> {
        self.stream.as_ref()
    }

    /// Let `dt` seconds of flash burn down.
    pub fn tick(&mut self, dt: f32) {
        self.flash = (self.flash - dt.max(0.0)).max(0.0);
    }

    pub fn flashing(&self) -> bool {
        self.flash > 0.0
    }

    /// Whether the report says an engine is running.
    pub fn running(&self) -> bool {
        matches!(
            self.health,
            Some(Health {
                state: EngineState::Running,
                ..
            })
        )
    }

    /// What the strip says about the engine, and how loudly. `None` is
    /// the normal state: an engine that is running cleanly earns no ink.
    pub fn words(&self) -> Option<(String, Tone)> {
        let health = self.health.as_ref()?;
        Some(match &health.state {
            EngineState::Absent => ("NO ENGINE".to_owned(), Tone::Alarm),
            EngineState::Errored(error) => (format!("ENGINE ERROR · {error}"), Tone::Alarm),
            EngineState::Stalled { seconds } => {
                (format!("ENGINE STALLED · {seconds:.1}s"), Tone::Alarm)
            }
            EngineState::Running if self.flashing() => {
                (format!("XRUN · {} dropped", health.xruns), Tone::Alarm)
            }
            EngineState::Running if health.load >= LOAD_WORTH_SAYING => {
                (format!("load {:.0}%", health.load * 100.0), Tone::Alarm)
            }
            EngineState::Running if health.xruns > 0 => {
                (format!("{} dropped", health.xruns), Tone::Quiet)
            }
            EngineState::Running => return None,
        })
    }
}

/// The meters: one pair of ballistics per track, and one for the master.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meters {
    tracks: Vec<[Ballistics; 2]>,
    master: [Ballistics; 2],
}

impl Meters {
    /// Advance every meter by `dt` toward what the engine last reported.
    /// The table follows the song's tracks: a track past the end of
    /// `raw` reads silence, and a track that has gone takes its meter
    /// with it.
    pub fn follow(&mut self, raw: &[Level], master: Level, dt: f32) {
        self.tracks.resize(raw.len(), [Ballistics::default(); 2]);
        for (pair, level) in self.tracks.iter_mut().zip(raw) {
            advance(pair, *level, dt);
        }
        advance(&mut self.master, master, dt);
    }

    /// Every track's meter as the strip draws it.
    pub fn readings(&self) -> Vec<Reading> {
        self.tracks.iter().map(reading).collect()
    }

    /// The master's own meter, as its strip draws it.
    ///
    /// Separate from [`Self::readings`] because the master is not a
    /// track: it is not in that table and must not be indexed out of it.
    pub fn master(&self) -> Reading {
        reading(&self.master)
    }

    /// Whether any meter is still falling. A stage at rest need not
    /// repaint for its meters.
    pub fn moving(&self) -> bool {
        self.tracks
            .iter()
            .chain(std::iter::once(&self.master))
            .flatten()
            .any(Ballistics::moving)
    }
}

fn advance(pair: &mut [Ballistics; 2], level: Level, dt: f32) {
    let floored = |amp: f32| meter::amp_to_db(amp).max(meter::FLOOR_DB);
    pair[0].advance(floored(level.left), dt);
    pair[1].advance(floored(level.right), dt);
}

fn reading(pair: &[Ballistics; 2]) -> Reading {
    // The floor is silence, and silence is zero — not the thousandth of
    // full scale the dB floor happens to convert to.
    let amp = |db: f32| {
        if db <= meter::FLOOR_DB {
            0.0
        } else {
            meter::db_to_amp(db)
        }
    };
    Reading {
        level: Level {
            left: amp(pair[0].shown_db),
            right: amp(pair[1].shown_db),
        },
        peak: Level {
            left: amp(pair[0].peak_db),
            right: amp(pair[1].peak_db),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(xruns: u64) -> Health {
        Health {
            state: EngineState::Running,
            xruns,
            load: 0.2,
        }
    }

    #[test]
    fn nothing_reported_says_nothing_and_a_clean_engine_says_nothing_either() {
        let mut vitals = Vitals::default();
        assert_eq!(vitals.words(), None);
        assert!(!vitals.running());
        vitals.report(running(0));
        assert_eq!(vitals.words(), None, "a clean engine spent ink");
        assert!(vitals.running());
    }

    #[test]
    fn no_engine_is_said_loudly() {
        let mut vitals = Vitals::default();
        vitals.report(Health {
            state: EngineState::Absent,
            xruns: 0,
            load: 0.0,
        });
        assert_eq!(vitals.words(), Some(("NO ENGINE".to_owned(), Tone::Alarm)));
        assert!(!vitals.running());
    }

    #[test]
    fn a_new_xrun_flashes_and_then_settles_into_a_count() {
        let mut vitals = Vitals::default();
        vitals.report(running(0));
        vitals.report(running(1));
        assert!(vitals.flashing(), "an xrun happened silently");
        let (words, tone) = vitals.words().expect("something to say");
        assert_eq!(tone, Tone::Alarm);
        assert!(words.starts_with("XRUN"), "{words}");

        // Time passes; the alarm goes, the count stays.
        vitals.tick(XRUN_FLASH_S / 2.0);
        assert!(vitals.flashing(), "the flash was too short to see");
        vitals.tick(XRUN_FLASH_S);
        assert!(!vitals.flashing());
        assert_eq!(vitals.words(), Some(("1 dropped".to_owned(), Tone::Quiet)));

        // The same count again is not a new xrun.
        vitals.report(running(1));
        assert!(!vitals.flashing(), "an old xrun flashed again");
        // A further one is.
        vitals.report(running(3));
        assert!(vitals.flashing());
        assert_eq!(
            vitals.words(),
            Some(("XRUN · 3 dropped".to_owned(), Tone::Alarm))
        );
    }

    #[test]
    fn the_first_report_is_history_and_does_not_flash() {
        // An engine that dropped blocks before the stage was watching
        // has nothing to alarm about NOW; the count is still there to read.
        let mut vitals = Vitals::default();
        vitals.report(running(7));
        assert!(!vitals.flashing());
        assert_eq!(vitals.words(), Some(("7 dropped".to_owned(), Tone::Quiet)));
    }

    #[test]
    fn a_stall_and_an_error_are_alarms_and_a_heavy_load_is_a_warning() {
        let mut vitals = Vitals::default();
        vitals.report(Health {
            state: EngineState::Stalled { seconds: 1.5 },
            xruns: 0,
            load: 0.0,
        });
        assert_eq!(vitals.words().map(|(_, tone)| tone), Some(Tone::Alarm));
        assert!(!vitals.running());
        vitals.report(Health {
            state: EngineState::Errored("jack went away".to_owned()),
            xruns: 0,
            load: 0.0,
        });
        let (words, tone) = vitals.words().expect("an error is said");
        assert!(
            words.contains("jack went away"),
            "the error was swallowed: {words}"
        );
        assert_eq!(tone, Tone::Alarm);
        vitals.report(Health {
            state: EngineState::Running,
            xruns: 0,
            load: 0.9,
        });
        assert_eq!(vitals.words(), Some(("load 90%".to_owned(), Tone::Alarm)));
    }

    #[test]
    fn a_meter_rises_at_once_and_falls_slowly() {
        let mut meters = Meters::default();
        let loud = Level {
            left: 1.0,
            right: 0.5,
        };
        meters.follow(&[loud], Level::default(), 1.0 / 60.0);
        let shown = meters.readings()[0];
        // Through decibels and back, so to within a rounding.
        let near =
            |a: Level, b: Level| (a.left - b.left).abs() < 1e-5 && (a.right - b.right).abs() < 1e-5;
        assert!(near(shown.level, loud), "attack was not instant: {shown:?}");
        assert!(near(shown.peak, loud));

        // Silence for one frame: the bar has come down a little, not to
        // nothing, and the peak mark has not moved at all.
        meters.follow(&[Level::default()], Level::default(), 1.0 / 60.0);
        let after = meters.readings()[0];
        assert!(after.level.left < loud.left, "the bar did not fall");
        assert!(
            after.level.left > 0.5,
            "the bar fell to nothing in one frame — that is the flicker"
        );
        assert!(near(after.peak, loud), "the peak mark did not hold");

        // Long enough, and everything is at rest.
        for _ in 0..600 {
            meters.follow(&[Level::default()], Level::default(), 1.0 / 60.0);
        }
        let rested = meters.readings()[0];
        assert_eq!(
            rested.level,
            Level::default(),
            "silence did not read as zero"
        );
        assert_eq!(rested.peak, Level::default());
        assert!(!meters.moving());
    }

    #[test]
    fn the_table_follows_the_tracks() {
        let mut meters = Meters::default();
        meters.follow(&[Level::default(); 3], Level::default(), 0.01);
        assert_eq!(meters.readings().len(), 3);
        meters.follow(&[Level::default(); 1], Level::default(), 0.01);
        assert_eq!(meters.readings().len(), 1, "a removed track kept a meter");
    }
}
