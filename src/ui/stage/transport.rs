//! Where the song is in time, and whether it is moving.
//!
//! The MODEL half of the vitals strip's transport. It answers three
//! questions — where, moving or not, and at what tempo and meter — and it
//! answers them without an engine, because it has to work before one is
//! wired and must keep working identically after.
//!
//! **Position advances from outside.** [`Transport::advance`] takes the
//! elapsed seconds, so the caller decides what a second is: the frame
//! clock while there is no audio device, and the engine's own sample
//! count once there is. The legacy app already does exactly this — a
//! transport with no engine advances on `stable_dt` — so this is the
//! shipped fallback rather than a stand-in that gets thrown away. Nothing
//! downstream changes when the source of the number changes.
//!
//! **Tempo and meter are read from the Song**, never stored here.
//! `Song::bpm_at` and `Song::meter_at` are the authority; a transport
//! that cached them would be a second answer to a question the model
//! already answers, and the two would drift.

use crate::sequencing::{Song, TICKS_PER_BEAT};

/// The tempo a song with no marks runs at.
pub const DEFAULT_BPM: f64 = 120.0;
/// The signature a song with no marks is in.
pub const DEFAULT_METER: (u32, u32) = (4, 4);

/// Whether the transport is merely moving or actually committing.
///
/// Ordered by how much is at stake, because that is the order the display
/// escalates in: stopped is the quiet default, rolling is motion, and
/// recording is the one that can destroy something.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub enum Motion {
    #[default]
    Stopped,
    Rolling,
    Recording,
}

impl Motion {
    /// Whether time is passing. Recording rolls too.
    pub fn is_rolling(self) -> bool {
        matches!(self, Self::Rolling | Self::Recording)
    }
}

/// The transport: a position in song ticks and what it is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Transport {
    /// Song position in ticks, kept as a float so a fractional beat is not
    /// lost between frames and the drift does not accumulate.
    tick: f64,
    motion: Motion,
}

impl Transport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn motion(&self) -> Motion {
        self.motion
    }

    /// Position in whole ticks, which is the unit the model counts in.
    pub fn tick(&self) -> usize {
        self.tick.max(0.0) as usize
    }

    /// Position as a fraction of the way through the current beat, `0..1`.
    /// The subdivision display reads this; the digits do not, which is why
    /// the digits do not churn.
    pub fn beat_phase(&self) -> f32 {
        let beats = self.tick.max(0.0) / TICKS_PER_BEAT as f64;
        (beats.fract()) as f32
    }

    /// Start rolling, or start committing. Idempotent.
    pub fn set_motion(&mut self, motion: Motion) {
        self.motion = motion;
    }

    /// Stop where it stands. Returning to the top is a separate verb,
    /// because "stop" and "go back" are different intentions and a
    /// transport that conflates them cannot express the first.
    pub fn stop(&mut self) {
        self.motion = Motion::Stopped;
    }

    /// Back to the top, whether or not it is rolling.
    pub fn rewind(&mut self) {
        self.tick = 0.0;
    }

    /// Move the playhead somewhere exact.
    pub fn seek(&mut self, tick: usize) {
        self.tick = tick as f64;
    }

    /// Take the position from outside — an engine reporting where it
    /// actually is, in beats. The fraction is kept, as it is everywhere
    /// here, so the subdivision row moves smoothly between the beats the
    /// engine reports.
    pub fn follow(&mut self, beats: f64) {
        if beats.is_finite() {
            self.tick = (beats * TICKS_PER_BEAT as f64).max(0.0);
        }
    }

    /// Let `seconds` of time pass at the song's tempo where the playhead
    /// currently stands. Does nothing while stopped.
    ///
    /// Tempo is sampled at the position the block STARTS from, which is
    /// the same approximation the engine makes over a block: exact at
    /// every mark, and wrong by less than one frame between them.
    pub fn advance(&mut self, song: &Song, seconds: f64) {
        if !self.motion.is_rolling() || seconds <= 0.0 {
            return;
        }
        let bpm = song.bpm_at(self.tick(), DEFAULT_BPM).max(f64::MIN_POSITIVE);
        self.tick += seconds * (bpm / 60.0) * TICKS_PER_BEAT as f64;
    }

    /// Where the playhead is, counted the way a musician counts.
    pub fn place(&self, song: &Song) -> Place {
        Place::of(song, self.tick())
    }
}

/// A position in bars and beats, one-based the way a score is.
///
/// Computed from the meter map rather than assumed to be 4/4: bars are
/// counted from each meter mark onward, so a signature change does not
/// silently renumber everything after it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Place {
    /// Bar number, counting from one.
    pub bar: usize,
    /// Beat within the bar, counting from one.
    pub beat: usize,
    /// Beats in this bar — the meter's numerator, and the number of cells
    /// the subdivision row draws. The meter is DISPLAYED as the shape of
    /// that row rather than spelled out as text.
    pub beats_per_bar: usize,
    /// What kind of note gets the beat.
    pub denominator: u32,
}

impl Place {
    pub fn of(song: &Song, tick: usize) -> Self {
        let (numerator, denominator) = song.meter_at(tick, DEFAULT_METER);
        let beats_per_bar = numerator.max(1) as usize;

        // Whole beats since the top, then split into bars and the beat
        // within one. Only correct while the meter is constant from the
        // start; a mid-song meter change needs the marks walked in order,
        // which is worth doing when meter marks are actually editable.
        let beat_index = tick / TICKS_PER_BEAT;
        Self {
            bar: beat_index / beats_per_bar + 1,
            beat: beat_index % beats_per_bar + 1,
            beats_per_bar,
            denominator,
        }
    }

    /// The fixed-width field the vitals strip draws: `007.03`.
    ///
    /// Constant width is the point. A number whose digits grow shifts
    /// everything beside it, and a field that moves has to be found again
    /// on every glance; leading zeros buy a free lookup forever.
    pub fn readout(&self) -> String {
        format!("{:03}.{:02}", self.bar, self.beat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stopped_transport_does_not_move() {
        let song = Song::default();
        let mut transport = Transport::new();
        transport.advance(&song, 10.0);
        assert_eq!(transport.tick(), 0);
    }

    #[test]
    fn rolling_advances_at_the_songs_tempo() {
        let song = Song::default();
        let mut transport = Transport::new();
        transport.set_motion(Motion::Rolling);

        // One beat at 120bpm is half a second.
        transport.advance(&song, 0.5);
        assert_eq!(transport.tick(), TICKS_PER_BEAT);

        // Four beats in, the second bar begins.
        transport.advance(&song, 1.5);
        let place = transport.place(&song);
        assert_eq!((place.bar, place.beat), (2, 1));
    }

    #[test]
    fn recording_rolls_too() {
        let song = Song::default();
        let mut transport = Transport::new();
        transport.set_motion(Motion::Recording);
        transport.advance(&song, 0.5);
        assert_eq!(transport.tick(), TICKS_PER_BEAT);
    }

    /// Fractional time SURVIVES between frames.
    ///
    /// The position is a float for exactly this reason: if each frame
    /// rounded to whole ticks, a sixtieth of a second at 120bpm — 1.6
    /// ticks — would truncate to 1 every frame and the transport would
    /// run about forty percent slow. Keeping the fraction bounds the
    /// error at less than one tick no matter how many frames pass, which
    /// is the honest claim; summing sixty floats is not exact, and a test
    /// demanding that it be would be testing arithmetic rather than this.
    #[test]
    fn fractional_time_survives_between_frames() {
        let song = Song::default();
        let mut transport = Transport::new();
        transport.set_motion(Motion::Rolling);
        for _ in 0..60 {
            transport.advance(&song, 1.0 / 60.0);
        }

        let expected = 2 * TICKS_PER_BEAT;
        let drift = transport.tick().abs_diff(expected);
        assert!(
            drift <= 1,
            "one second of frames landed {} ticks from {expected}",
            drift
        );
    }

    #[test]
    fn stopping_leaves_the_playhead_and_rewinding_moves_it() {
        let song = Song::default();
        let mut transport = Transport::new();
        transport.set_motion(Motion::Rolling);
        transport.advance(&song, 1.0);
        let held = transport.tick();
        assert!(held > 0);

        transport.stop();
        assert_eq!(transport.tick(), held, "stop is not rewind");
        transport.rewind();
        assert_eq!(transport.tick(), 0);
    }

    #[test]
    fn the_readout_is_one_based_and_fixed_width() {
        let song = Song::default();
        let transport = Transport::new();
        assert_eq!(transport.place(&song).readout(), "001.01");

        let mut later = Transport::new();
        later.seek(TICKS_PER_BEAT * 4 * 99);
        assert_eq!(later.place(&song).readout(), "100.01");
    }

    /// The meter decides the shape of the subdivision row, so it has to
    /// come from the song rather than be assumed.
    #[test]
    fn the_meter_comes_from_the_song_not_from_four_four() {
        let mut song = Song::default();
        assert_eq!(Place::of(&song, 0).beats_per_bar, 4);

        assert!(song.set_meter_mark(0, 7, 8));
        let place = Place::of(&song, TICKS_PER_BEAT * 7);
        assert_eq!(place.beats_per_bar, 7);
        assert_eq!(place.denominator, 8);
        assert_eq!((place.bar, place.beat), (2, 1), "seven beats is one bar");
    }

    #[test]
    fn tempo_marks_change_how_fast_time_passes() {
        let mut song = Song::default();
        assert!(song.set_tempo_mark(0, 240.0));

        let mut transport = Transport::new();
        transport.set_motion(Motion::Rolling);
        transport.advance(&song, 0.5);
        assert_eq!(
            transport.tick(),
            TICKS_PER_BEAT * 2,
            "twice the tempo is twice the distance"
        );
    }

    /// Beat phase is what the subdivision row reads, and it is what lets
    /// the digits stay still: the fraction moves every frame, the bar and
    /// beat numbers only on the beat.
    #[test]
    fn beat_phase_sweeps_between_beats_and_resets_on_them() {
        let song = Song::default();
        let mut transport = Transport::new();
        transport.set_motion(Motion::Rolling);
        assert_eq!(transport.beat_phase(), 0.0);

        transport.advance(&song, 0.25);
        assert!((transport.beat_phase() - 0.5).abs() < 1e-5);

        transport.advance(&song, 0.25);
        assert!(transport.beat_phase() < 1e-5, "a whole beat resets it");
    }

    #[test]
    fn a_followed_position_replaces_the_counted_one() {
        let song = Song::default();
        let mut transport = Transport::new();
        transport.set_motion(Motion::Rolling);
        transport.advance(&song, 10.0);
        transport.follow(2.5);
        assert_eq!(transport.tick(), TICKS_PER_BEAT * 5 / 2);
        assert!(
            (transport.beat_phase() - 0.5).abs() < 1e-5,
            "the fraction was lost"
        );
        // Nonsense from outside is ignored rather than becoming the clock.
        transport.follow(f64::NAN);
        assert_eq!(transport.tick(), TICKS_PER_BEAT * 5 / 2);
        transport.follow(-4.0);
        assert_eq!(transport.tick(), 0);
    }

    #[test]
    fn motion_escalates_by_what_is_at_stake() {
        assert!(Motion::Stopped < Motion::Rolling);
        assert!(Motion::Rolling < Motion::Recording);
        assert!(!Motion::Stopped.is_rolling());
        assert!(Motion::Rolling.is_rolling() && Motion::Recording.is_rolling());
    }
}
