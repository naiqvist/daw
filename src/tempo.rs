//! The compiled tempo table: song ticks to absolute samples.
//!
//! Built green-side, once, and read by the compiler that stamps events.
//! Contract rule 1 says every event is timeline-sample-stamped (u64), and
//! rule 4 says sequences ride immutable compiled chunks. The green-side
//! compiler stamps events through this table, then the schedule owns one
//! immutable copy so the callback can do bounded read-only clock lookups.
//!
//! Tempo is constant between marks. A prefix sum over constant segments is
//! exact; an integrated ramp accumulates float error along a long
//! timeline, and a ramp remains strictly additive later (a segment would
//! integrate linearly instead of holding). See
//! `notes/20260831-midi-and-tempo-decisions.md`.

use crate::sequencing::{Song, TICKS_PER_BEAT};

/// One constant-tempo span, with its own origin already accumulated.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Segment {
    start_tick: usize,
    /// The absolute sample at `start_tick`.
    start_sample: f64,
    /// How long one tick lasts inside this segment.
    samples_per_tick: f64,
}

/// How long one tick lasts at `bpm`.
fn samples_per_tick(sample_rate: f64, bpm: f64) -> f64 {
    sample_rate * 60.0 / (bpm * TICKS_PER_BEAT as f64)
}

/// A song's tick-to-sample mapping, resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct TempoTable {
    /// Always non-empty: there is always a segment starting at tick 0, so
    /// every lookup lands somewhere and none can fail.
    segments: Vec<Segment>,
    /// The rate the table was resolved at. Kept because converting a
    /// real-time duration into ticks needs it, and a caller guessing it
    /// would be guessing the one number that must agree.
    sample_rate: f64,
}

impl TempoTable {
    /// Resolve `song`'s tempo map at `sample_rate`.
    ///
    /// `fallback_bpm` is the transport's single global tempo — what the
    /// song means before its first mark, and everywhere when the map is
    /// empty. That is what keeps every pre-map project sounding the same.
    pub fn build(song: &Song, sample_rate: f64, fallback_bpm: f64) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let fallback = song.bpm_at(0, fallback_bpm);

        // Usable marks only, in time order. An unusable bpm is dropped
        // rather than trusted: it would make a tick worth infinity
        // samples and hang the compile that tried to stamp against it.
        let mut marks: Vec<(usize, f64)> = song
            .tempo
            .iter()
            .filter(|mark| mark.bpm.is_finite() && mark.bpm > 0.0)
            .map(|mark| (mark.tick, mark.bpm))
            .collect();
        marks.sort_by_key(|(tick, _)| *tick);
        marks.dedup_by_key(|(tick, _)| *tick);

        let mut segments = Vec::with_capacity(marks.len() + 1);
        // The opening segment always exists, so no lookup can miss.
        let opening_bpm = marks
            .first()
            .filter(|(tick, _)| *tick == 0)
            .map_or(fallback, |(_, bpm)| *bpm);
        segments.push(Segment {
            start_tick: 0,
            start_sample: 0.0,
            samples_per_tick: samples_per_tick(sample_rate, opening_bpm),
        });

        for (tick, bpm) in marks {
            if tick == 0 {
                continue;
            }
            let previous = *segments.last().expect("the opening segment exists");
            let start_sample = previous.start_sample
                + (tick - previous.start_tick) as f64 * previous.samples_per_tick;
            segments.push(Segment {
                start_tick: tick,
                start_sample,
                samples_per_tick: samples_per_tick(sample_rate, bpm),
            });
        }

        Self {
            segments,
            sample_rate,
        }
    }

    /// The rate this table was resolved at.
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// How many ticks a real-time duration occupies, starting at
    /// `start_tick`.
    ///
    /// Consults the map rather than dividing by one tempo, so a sound
    /// laid across a tempo change gets the length it will actually
    /// occupy. Always at least one tick: a placement with no extent is
    /// not a placement.
    pub fn ticks_for_seconds(&self, start_tick: usize, seconds: f64) -> usize {
        if !seconds.is_finite() || seconds <= 0.0 {
            return 1;
        }
        let start_sample = self.sample_at(start_tick);
        let span = (seconds * self.sample_rate).round();
        if !span.is_finite() || span <= 0.0 {
            return 1;
        }
        let end_sample = start_sample.saturating_add(span as u64);
        self.tick_at(end_sample).saturating_sub(start_tick).max(1)
    }

    fn segment_for_tick(&self, tick: usize) -> Segment {
        let at = self
            .segments
            .partition_point(|segment| segment.start_tick <= tick)
            .saturating_sub(1);
        self.segments[at]
    }

    fn segment_for_beat(&self, beat: f64) -> Segment {
        let tick = beat.max(0.0) * TICKS_PER_BEAT as f64;
        let at = self
            .segments
            .partition_point(|segment| segment.start_tick as f64 <= tick)
            .saturating_sub(1);
        self.segments[at]
    }

    fn segment_for_sample(&self, sample: u64) -> Segment {
        let at = self
            .segments
            .partition_point(|segment| segment.start_sample.round().max(0.0) as u64 <= sample)
            .saturating_sub(1);
        self.segments[at]
    }

    /// The absolute sample `tick` falls on. This is the only conversion,
    /// and it happens here rather than anywhere near the callback.
    pub fn sample_at(&self, tick: usize) -> u64 {
        let segment = self.segment_for_tick(tick);
        let sample = segment.start_sample
            + (tick.saturating_sub(segment.start_tick)) as f64 * segment.samples_per_tick;
        if sample.is_finite() && sample >= 0.0 {
            sample.round() as u64
        } else {
            0
        }
    }

    /// The absolute sample a possibly fractional musical beat falls on.
    /// This keeps deliberately off-grid graph events off-grid while using
    /// the same piecewise map as Song ticks.
    pub fn sample_at_beat(&self, beat: f64) -> u64 {
        if !beat.is_finite() || beat <= 0.0 {
            return 0;
        }
        let segment = self.segment_for_beat(beat);
        let tick = beat * TICKS_PER_BEAT as f64;
        let sample = segment.start_sample
            + (tick - segment.start_tick as f64).max(0.0) * segment.samples_per_tick;
        if sample.is_finite() && sample >= 0.0 {
            sample.round() as u64
        } else {
            0
        }
    }

    /// The inverse, for putting a playhead back on the grid. Floors, so a
    /// sample inside a tick belongs to the tick it started in.
    pub fn tick_at(&self, sample: u64) -> usize {
        let sample = sample as f64;
        let at = self
            .segments
            .partition_point(|segment| segment.start_sample <= sample)
            .saturating_sub(1);
        let segment = self.segments[at];
        if segment.samples_per_tick <= 0.0 {
            return segment.start_tick;
        }
        // `sample_at` rounds to the NEAREST sample, so an exact tick can
        // come back up to half a sample early. Add that half back before
        // flooring, or the inverse lands a tick short — which is a
        // playhead that reads one tick behind where the note sounds.
        let into = ((sample - segment.start_sample + 0.5) / segment.samples_per_tick).floor();
        if into.is_finite() && into > 0.0 {
            segment.start_tick + into as usize
        } else {
            segment.start_tick
        }
    }

    /// The continuous musical beat at an absolute timeline sample. Unlike
    /// [`Self::tick_at`], this keeps the sub-tick fraction needed by synced
    /// DSP and a smoothly moving playhead.
    pub fn beat_at_sample(&self, sample: u64) -> f64 {
        let segment = self.segment_for_sample(sample);
        if segment.samples_per_tick <= 0.0 {
            return segment.start_tick as f64 / TICKS_PER_BEAT as f64;
        }
        let into = (sample as f64 - segment.start_sample).max(0.0) / segment.samples_per_tick;
        (segment.start_tick as f64 + into) / TICKS_PER_BEAT as f64
    }

    /// Musical beats advanced by one sample in the constant-tempo span
    /// containing `sample`.
    pub fn beats_per_sample_at(&self, sample: u64) -> f64 {
        let segment = self.segment_for_sample(sample);
        1.0 / (segment.samples_per_tick * TICKS_PER_BEAT as f64)
    }

    /// The next tempo boundary after `sample`, in absolute sample space.
    /// A live or offline block splitter uses this before handing DSP one
    /// constant `beats_per_sample` value.
    pub fn next_change_after(&self, sample: u64) -> Option<u64> {
        let at = self
            .segments
            .partition_point(|segment| segment.start_sample.round().max(0.0) as u64 <= sample);
        self.segments
            .get(at)
            .map(|segment| segment.start_sample.round().max(0.0) as u64)
    }

    /// How long one tick lasts at `tick` — what a ramp generator needs.
    pub fn samples_per_tick_at(&self, tick: usize) -> f64 {
        self.segment_for_tick(tick).samples_per_tick
    }

    /// How many constant-tempo spans the song resolved into. One means a
    /// song with a single tempo, whatever its map says.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;

    fn song() -> Song {
        Song::default()
    }

    /// One tick at 120bpm on 48k: a beat is half a second, and a beat is
    /// 48 ticks, so a tick is 500 samples.
    #[test]
    fn an_empty_map_is_the_fallback_tempo_everywhere() {
        let table = TempoTable::build(&song(), SR, 120.0);
        assert_eq!(table.segment_count(), 1);
        assert_eq!(table.sample_at(0), 0);
        assert_eq!(table.sample_at(TICKS_PER_BEAT), 24_000, "one beat = 0.5s");
        assert_eq!(table.sample_at(TICKS_PER_BEAT * 2), 48_000);
    }

    #[test]
    fn the_song_base_tempo_is_the_empty_maps_authority() {
        let mut song = song();
        assert!(song.set_base_bpm(80.0));
        assert!(!song.set_base_bpm(80.0));
        let table = TempoTable::build(&song, SR, 120.0);
        assert_eq!(table.sample_at(TICKS_PER_BEAT), 36_000);
        assert_eq!(song.bpm_at(0, 999.0), 80.0);
    }

    /// A mark at zero replaces the fallback outright.
    #[test]
    fn a_mark_at_zero_sets_the_opening_tempo() {
        let mut song = song();
        assert!(song.set_tempo_mark(0, 60.0));
        let table = TempoTable::build(&song, SR, 120.0);
        assert_eq!(table.segment_count(), 1);
        assert_eq!(table.sample_at(TICKS_PER_BEAT), 48_000, "one beat = 1s");
    }

    /// The prefix sum is exact at a boundary: everything before the mark
    /// is at the old tempo, everything after at the new one.
    #[test]
    fn a_later_mark_only_changes_time_after_it() {
        let mut song = song();
        song.set_tempo_mark(0, 120.0);
        song.set_tempo_mark(TICKS_PER_BEAT * 4, 60.0);
        let table = TempoTable::build(&song, SR, 120.0);
        assert_eq!(table.segment_count(), 2);

        // Four beats at 120 = 2 seconds.
        assert_eq!(table.sample_at(TICKS_PER_BEAT * 4), 96_000);
        // The next beat is at 60, so a whole second, not half.
        assert_eq!(table.sample_at(TICKS_PER_BEAT * 5), 96_000 + 48_000);
    }

    /// Time never runs backwards, whatever the map says.
    #[test]
    fn the_mapping_is_monotonic() {
        let mut song = song();
        song.set_tempo_mark(0, 90.0);
        song.set_tempo_mark(37, 200.0);
        song.set_tempo_mark(400, 45.0);
        let table = TempoTable::build(&song, SR, 120.0);
        let mut previous = 0;
        for tick in 0..1_000 {
            let sample = table.sample_at(tick);
            assert!(sample >= previous, "tick {tick} went backwards");
            previous = sample;
        }
    }

    /// tick -> sample -> tick returns where it started, across changes.
    #[test]
    fn tick_and_sample_round_trip_across_tempo_changes() {
        let mut song = song();
        song.set_tempo_mark(0, 120.0);
        song.set_tempo_mark(TICKS_PER_BEAT * 4, 60.0);
        song.set_tempo_mark(TICKS_PER_BEAT * 9, 174.0);
        let table = TempoTable::build(&song, SR, 120.0);
        // 174bpm gives a fractional samples-per-tick, which is exactly
        // where a naive floor loses a tick.
        for tick in [0, 1, 47, 48, 191, 192, 200, 431, 432, 433, 900, 901, 5_000] {
            assert_eq!(table.tick_at(table.sample_at(tick)), tick, "tick {tick}");
        }
    }

    #[test]
    fn continuous_clock_changes_slope_at_the_exact_mark() {
        let mut song = song();
        song.set_tempo_mark(0, 120.0);
        song.set_tempo_mark(TICKS_PER_BEAT, 60.0);
        let table = TempoTable::build(&song, SR, 120.0);

        assert_eq!(table.sample_at_beat(1.0), 24_000);
        assert_eq!(table.sample_at_beat(2.0), 72_000);
        assert_eq!(table.next_change_after(0), Some(24_000));
        assert_eq!(table.next_change_after(24_000), None);
        assert!((table.beat_at_sample(12_000) - 0.5).abs() < 1e-12);
        assert!((table.beat_at_sample(48_000) - 1.5).abs() < 1e-12);
        assert!((table.beats_per_sample_at(12_000) - 1.0 / 24_000.0).abs() < 1e-15);
        assert!((table.beats_per_sample_at(48_000) - 1.0 / 48_000.0).abs() < 1e-15);
    }

    /// A tempo nobody could mean is dropped, not stored and not trusted.
    #[test]
    fn an_unusable_tempo_is_refused() {
        let mut song = song();
        assert!(!song.set_tempo_mark(0, 0.0));
        assert!(!song.set_tempo_mark(0, -120.0));
        assert!(!song.set_tempo_mark(0, f64::NAN));
        assert!(!song.set_tempo_mark(0, f64::INFINITY));
        assert!(song.tempo.is_empty(), "nothing unusable was stored");

        // And a hostile document cannot poison the table either.
        song.tempo.push(crate::sequencing::TempoMark {
            tick: 0,
            bpm: f64::NAN,
        });
        let table = TempoTable::build(&song, SR, 120.0);
        assert_eq!(table.sample_at(TICKS_PER_BEAT), 24_000, "fell back cleanly");
    }

    /// Marks stay sorted however they arrive, and one tick holds one mark.
    #[test]
    fn marks_stay_sorted_and_unique() {
        let mut song = song();
        song.set_tempo_mark(96, 140.0);
        song.set_tempo_mark(0, 120.0);
        song.set_tempo_mark(48, 130.0);
        song.set_tempo_mark(48, 135.0);
        assert!(
            !song.set_tempo_mark(48, 135.0),
            "an identical write is a no-op"
        );
        let ticks: Vec<usize> = song.tempo.iter().map(|mark| mark.tick).collect();
        assert_eq!(ticks, vec![0, 48, 96]);
        assert_eq!(song.bpm_at(48, 120.0), 135.0, "the later write won");
        assert_eq!(song.bpm_at(47, 120.0), 120.0);
        assert_eq!(song.bpm_at(1_000, 120.0), 140.0, "holds after the last");

        assert!(song.remove_tempo_mark(48));
        assert!(!song.remove_tempo_mark(48));
    }

    /// Meter behaves the same way, and a degenerate signature is refused.
    #[test]
    fn the_meter_map_holds_and_refuses_nonsense() {
        let mut song = song();
        assert_eq!(song.meter_at(0, (4, 4)), (4, 4), "fallback when empty");
        assert!(song.set_meter_mark(TICKS_PER_BEAT * 4, 3, 4));
        assert!(
            !song.set_meter_mark(TICKS_PER_BEAT * 4, 3, 4),
            "an identical write is a no-op"
        );
        assert!(!song.set_meter_mark(0, 0, 4));
        assert!(!song.set_meter_mark(0, 4, 0));
        assert_eq!(song.meter_at(0, (4, 4)), (4, 4));
        assert_eq!(song.meter_at(TICKS_PER_BEAT * 4, (4, 4)), (3, 4));
        assert!(song.remove_meter_mark(TICKS_PER_BEAT * 4));
        assert!(!song.remove_meter_mark(TICKS_PER_BEAT * 4));
        assert_eq!(song.meter_at(TICKS_PER_BEAT * 4, (4, 4)), (4, 4));
    }

    /// A document written before the map loads, and means what it did.
    #[test]
    fn a_pre_map_document_still_loads_at_one_tempo() {
        let song = song();
        let text = ron::ser::to_string(&song).expect("serializes");
        let older = text
            .replace("bpm:120.0,", "")
            .replace("tempo:[],", "")
            .replace("meter:[],", "");
        let back: Song = ron::from_str(&older).expect("older document loads");
        assert_eq!(back.base_bpm(), 120.0);
        assert!(back.tempo.is_empty());
        assert!(back.meter.is_empty());
        let table = TempoTable::build(&back, SR, 120.0);
        assert_eq!(table.sample_at(TICKS_PER_BEAT), 24_000);
    }
}
