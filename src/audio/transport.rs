//! Transport and time. Red zone — but deliberately PURE: no device, no
//! threads, no clock reads. The callback drives this; tests drive it harder.
//!
//! The one rule: the sample counter is the master clock. Wall time never
//! decides where the music is. Musical positions are DERIVED from the sample
//! count through `TimeMap`, in f64, at the moment of asking — never
//! accumulated, so they cannot drift.
//!
//! Serialization rule (decided 2026-08-23, before anything serializes):
//! projects store musical time (f64 beats or seconds). Sample positions are
//! runtime-only, derived for the current sample rate. A project saved at 48k
//! must open correctly at 44.1k.

/// Shortest loop the transport will accept, in samples. Rejecting degenerate
/// loops at the command boundary is one of three guards that make an infinite
/// segment loop impossible (the others: segments are provably >= 1 frame, and
/// the callback bounds segments per block).
pub const MIN_LOOP_LEN: u64 = 64;

/// The single authority for converting between musical and sample time.
/// Both directions live here and nowhere else — two call sites rounding
/// differently is a one-sample click that reproduces only at certain tempos.
#[derive(Debug, Clone, Copy)]
pub struct TimeMap {
    pub bpm: f64,
    pub sample_rate: f64,
}

impl TimeMap {
    pub fn beats_to_samples(&self, beats: f64) -> u64 {
        // One rounding rule for the whole engine: round-to-nearest.
        (beats * 60.0 / self.bpm * self.sample_rate)
            .round()
            .max(0.0) as u64
    }

    pub fn samples_to_beats(&self, samples: u64) -> f64 {
        samples as f64 / self.sample_rate * self.bpm / 60.0
    }

    /// Beats advanced per sample — for per-sample beat tracking inside a node.
    pub fn beats_per_sample(&self) -> f64 {
        self.bpm / 60.0 / self.sample_rate
    }
}

/// Commands from the UI. One ring, drained in order at block start — so a
/// gesture like stop+seek+play is atomic by ring order, and N seeks in one
/// block collapse to the last one (each just sets position; audio is only
/// produced after the drain). No cross-ring ordering exists to get wrong.
#[derive(Debug, Clone, Copy)]
pub enum TransportCmd {
    Play,
    Stop,
    /// Stop and return to zero.
    Return,
    Seek(u64),
    /// Sample positions, validated on apply: end > start, len >= MIN_LOOP_LEN,
    /// else the command is ignored.
    SetLoop {
        start: u64,
        end: u64,
    },
    ClearLoop,
    SetTempo(f64),
}

/// One run of contiguous samples with no transport event inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// Timeline position of this segment's first sample, as it will sound at
    /// the output. (Latency compensation, when it exists, offsets from this
    /// definition — the reference point is decided here, once.)
    pub position: u64,
    /// Frames in this segment. Always >= 1.
    pub len: usize,
    pub playing: bool,
    /// Beat of the first sample, derived (never accumulated).
    pub beat: f64,
    /// True when this segment does NOT continue seamlessly from the last
    /// playing segment: first play, seek, or loop wrap. THE sequencing
    /// contract hangs off this flag: any node holding a sounding voice cuts
    /// it (all-sound-off) when it sees a discontinuity — this is what makes
    /// hanging notes structurally impossible. Pause/resume at the same
    /// position is seamless and does NOT set it.
    pub discontinuity: bool,
}

/// The transport state machine. Owned by the audio thread; the UI only sends
/// commands and reads snapshots.
#[derive(Debug, Clone, Copy)]
pub struct Transport {
    playing: bool,
    pos: u64,
    loop_region: Option<(u64, u64)>,
    /// Where seamless playback would continue: position right after the last
    /// playing segment, before any wrap. None until the first play.
    continuity: Option<u64>,
    pub map: TimeMap,
}

impl Transport {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            playing: false,
            pos: 0,
            loop_region: None,
            continuity: None,
            map: TimeMap {
                bpm: 120.0,
                sample_rate,
            },
        }
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    pub fn position(&self) -> u64 {
        self.pos
    }

    /// Apply one command. Invalid commands are ignored, never "fixed up" —
    /// a clamped wrong loop is still wrong, just quieter about it.
    pub fn apply(&mut self, cmd: TransportCmd) {
        match cmd {
            TransportCmd::Play => self.playing = true,
            TransportCmd::Stop => self.playing = false,
            TransportCmd::Return => {
                self.playing = false;
                self.pos = 0;
            }
            TransportCmd::Seek(pos) => self.pos = pos,
            TransportCmd::SetLoop { start, end } => {
                if end > start && end - start >= MIN_LOOP_LEN {
                    self.loop_region = Some((start, end));
                }
            }
            TransportCmd::ClearLoop => self.loop_region = None,
            TransportCmd::SetTempo(bpm) => {
                if bpm.is_finite() && bpm > 0.0 {
                    self.map.bpm = bpm.clamp(1.0, 999.0);
                }
            }
        }
    }

    /// Produce the next segment covering at most `remaining` frames, and
    /// advance past it. `remaining` must be >= 1.
    ///
    /// Progress proof: the returned len is >= 1 in every branch — when
    /// stopped, len == remaining; when a loop end is ahead, `loop_end - pos`
    /// is >= 1 because the branch requires pos < loop_end. A wrap only
    /// triggers when playback lands exactly on loop_end; seeking past the
    /// loop simply plays on (wrap-on-crossing semantics).
    pub fn next_segment(&mut self, remaining: usize) -> Segment {
        debug_assert!(remaining >= 1);
        let seg_start = self.pos;

        let len = if !self.playing {
            remaining // position frozen; the graph still runs (monitoring)
        } else {
            match self.loop_region {
                Some((_, end)) if self.pos < end => {
                    let to_end = (end - self.pos) as usize;
                    remaining.min(to_end)
                }
                _ => remaining,
            }
        };

        // A playing segment is discontinuous unless it starts exactly where
        // seamless playback would have continued. Seeks move pos away from
        // continuity; a loop wrap moves pos while continuity records the
        // linear continuation; stop/resume at the same position stays equal.
        let discontinuity = self.playing && self.continuity != Some(seg_start);

        if self.playing {
            self.pos += len as u64;
            self.continuity = Some(self.pos); // linear continuation, pre-wrap
            if let Some((start, end)) = self.loop_region
                && self.pos == end
            {
                self.pos = start;
            }
        }

        Segment {
            position: seg_start,
            len,
            playing: self.playing,
            beat: self.map.samples_to_beats(seg_start),
            discontinuity,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests may panic loudly; the deny guards the red zone
mod tests {
    use super::*;

    fn t() -> Transport {
        Transport::new(48_000.0)
    }

    /// Drive a whole block through the splitter, checking the progress proof.
    fn drive(tr: &mut Transport, frames: usize) -> Vec<Segment> {
        let mut segs = Vec::new();
        let mut done = 0;
        while done < frames {
            let s = tr.next_segment(frames - done);
            assert!(s.len >= 1, "zero-length segment: infinite loop in callback");
            done += s.len;
            segs.push(s);
        }
        assert_eq!(done, frames, "segments must exactly cover the block");
        segs
    }

    #[test]
    fn stopped_transport_is_one_frozen_segment() {
        let mut tr = t();
        let segs = drive(&mut tr, 256);
        assert_eq!(segs.len(), 1);
        assert!(!segs[0].playing);
        assert_eq!(tr.position(), 0, "stop gates advance, not processing");
    }

    #[test]
    fn loop_shorter_than_block_wraps_repeatedly_and_terminates() {
        let mut tr = t();
        tr.apply(TransportCmd::SetLoop { start: 0, end: 100 });
        tr.apply(TransportCmd::Play);
        let segs = drive(&mut tr, 256); // 100 + 100 + 56
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0].position, 0);
        assert_eq!(segs[1].position, 0, "wrapped to loop start");
        assert_eq!(segs[2].position, 0);
        assert_eq!(tr.position(), 56);
    }

    #[test]
    fn loop_boundary_exactly_on_block_edge() {
        let mut tr = t();
        tr.apply(TransportCmd::SetLoop { start: 0, end: 256 });
        tr.apply(TransportCmd::Play);
        let segs = drive(&mut tr, 256);
        assert_eq!(segs.len(), 1);
        assert_eq!(tr.position(), 0, "landed on end -> wrapped");
        let segs = drive(&mut tr, 256); // second pass identical
        assert_eq!(segs[0].position, 0);
    }

    #[test]
    fn degenerate_loops_are_rejected_at_the_boundary() {
        let mut tr = t();
        tr.apply(TransportCmd::SetLoop {
            start: 100,
            end: 100,
        }); // zero-length
        tr.apply(TransportCmd::SetLoop {
            start: 200,
            end: 150,
        }); // inverted
        tr.apply(TransportCmd::SetLoop { start: 0, end: 10 }); // below MIN_LOOP_LEN
        tr.apply(TransportCmd::Play);
        let segs = drive(&mut tr, 256);
        assert_eq!(segs.len(), 1, "no loop should be active");
    }

    #[test]
    fn seek_past_loop_end_plays_on_rather_than_wrapping() {
        let mut tr = t();
        tr.apply(TransportCmd::SetLoop { start: 0, end: 100 });
        tr.apply(TransportCmd::Seek(500));
        tr.apply(TransportCmd::Play);
        drive(&mut tr, 256);
        assert_eq!(tr.position(), 756, "wrap only triggers on crossing");
    }

    #[test]
    fn many_seeks_before_a_block_collapse_to_the_last() {
        let mut tr = t();
        for p in [10, 20, 30, 999] {
            tr.apply(TransportCmd::Seek(p));
        }
        tr.apply(TransportCmd::Play);
        let segs = drive(&mut tr, 64);
        assert_eq!(segs[0].position, 999);
    }

    #[test]
    fn discontinuity_flags_first_play_seek_and_wrap_but_not_steady_play() {
        let mut tr = t();
        tr.apply(TransportCmd::Play);
        let s1 = tr.next_segment(100);
        assert!(s1.discontinuity, "first play starts fresh");
        let s2 = tr.next_segment(100);
        assert!(!s2.discontinuity, "steady playback is seamless");

        tr.apply(TransportCmd::Seek(5_000));
        let s3 = tr.next_segment(100);
        assert!(s3.discontinuity, "seek breaks continuity");

        tr.apply(TransportCmd::SetLoop {
            start: 0,
            end: 5_200,
        });
        let s4 = tr.next_segment(100); // 5100..5200, lands on loop end
        assert!(!s4.discontinuity);
        let s5 = tr.next_segment(100); // wrapped to 0
        assert_eq!(s5.position, 0);
        assert!(s5.discontinuity, "loop wrap is a discontinuity");
    }

    #[test]
    fn pause_resume_at_same_position_is_seamless() {
        let mut tr = t();
        tr.apply(TransportCmd::Play);
        tr.next_segment(256);
        tr.apply(TransportCmd::Stop);
        tr.next_segment(256); // stopped block; position frozen
        tr.apply(TransportCmd::Play);
        let s = tr.next_segment(256);
        assert!(
            !s.discontinuity,
            "resume at same position must not cut voices"
        );
    }

    #[test]
    fn beat_is_derived_not_accumulated() {
        let mut tr = t();
        tr.apply(TransportCmd::Play);
        // Advance an hour of audio in blocks; beat must stay exact.
        for _ in 0..675_000 {
            tr.next_segment(256);
        }
        let samples = tr.position();
        assert_eq!(samples, 675_000 * 256);
        let beat = tr.map.samples_to_beats(samples);
        let exact = samples as f64 / 48_000.0 * 2.0; // 120bpm = 2 beats/sec
        assert!(
            (beat - exact).abs() < 1e-9,
            "drift: {}",
            (beat - exact).abs()
        );
    }

    #[test]
    fn conversion_roundtrip_is_stable() {
        let map = TimeMap {
            bpm: 137.3,
            sample_rate: 48_000.0,
        };
        for b in 0..1000 {
            let beats = b as f64 * 0.25;
            let s = map.beats_to_samples(beats);
            let s2 = map.beats_to_samples(map.samples_to_beats(s));
            assert!(s.abs_diff(s2) <= 1, "roundtrip moved {} -> {}", s, s2);
        }
    }

    #[test]
    fn tempo_change_moves_the_map_not_the_position() {
        let mut tr = t();
        tr.apply(TransportCmd::Play);
        drive(&mut tr, 48_000); // one second
        let pos_before = tr.position();
        tr.apply(TransportCmd::SetTempo(240.0));
        assert_eq!(tr.position(), pos_before, "tempo never teleports position");
    }
}
