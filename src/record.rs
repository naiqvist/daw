//! Capture, green zone: the ring the callback fills becomes files on disk.
//!
//! The split is the red-zone rule made concrete. The callback may not
//! open a file, allocate a buffer or block on a write, so all it does is
//! interleave the input block into an `rtrb` ring. Everything that can
//! take an unbounded amount of time — opening, encoding, writing,
//! closing — happens here, on the UI thread, once per frame.
//!
//! # What is captured
//!
//! The device's input channels, ALL of them, exactly as they arrived and
//! before anything in the graph touches them. A take is therefore what
//! came in, not what came out: monitoring through a lane's effects
//! changes what you hear while you play and never what is written.
//! Choosing otherwise would mean a take you cannot re-effect, which is
//! the one thing a recording must not be.
//!
//! Demultiplexing happens HERE rather than in the callback, because the
//! callback would have to know which lanes are armed and which channels
//! each wants — a table it would have to be sent, kept in step, and read
//! under a lock it is not allowed to take. One interleaved stream and a
//! table on this side costs a copy and owes nothing.

use std::io::BufWriter;
use std::path::{Path, PathBuf};

/// Where one armed lane's take is going.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordRoute {
    /// The lane this take belongs to.
    pub track: usize,
    /// Which device input channels it takes, in order. One is mono, two
    /// is a stereo pair — the same shape a lane's input route has.
    pub channels: Vec<u32>,
}

/// A take that has been closed and is ready to be placed.
#[derive(Debug, Clone, PartialEq)]
pub struct Take {
    pub track: usize,
    pub path: PathBuf,
    pub frames: u64,
    pub channels: u16,
    pub sample_rate: u32,
}

/// One performed MIDI note, stamped on the transport sample timeline.
///
/// Pairing note-on with note-off belongs to the input side; landing owns
/// the later sample→tick conversion and the explicit quantisation preview.
/// Keeping samples here satisfies the sequencing contract without making a
/// transient take into project data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiTakeNote {
    pub start_sample: u64,
    pub end_sample: u64,
    pub pitch: u8,
    pub velocity: u8,
}

/// A closed controller performance ready to be previewed into one Song
/// pattern. `track` is the same projection-twin address an audio [`Take`]
/// carries; the landing resolves it back to canonical Song ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiTake {
    pub track: usize,
    pub notes: Vec<MidiTakeNote>,
}

/// Pairs controller note-ons and note-offs on the transport sample clock.
///
/// This is green-zone state. The MIDI callback only hands messages to its
/// input service; the host stamps those messages against the engine timeline
/// and feeds them here once per frame. One performance is copied to every
/// armed instrument track when it closes, just as one audio input can be
/// routed to more than one armed audio lane.
pub struct MidiRecorder {
    tracks: Vec<usize>,
    held: [Option<(u64, u8)>; 128],
    notes: Vec<MidiTakeNote>,
}

impl Default for MidiRecorder {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            held: [None; 128],
            notes: Vec::new(),
        }
    }
}

impl MidiRecorder {
    pub fn recording(&self) -> bool {
        !self.tracks.is_empty()
    }

    /// Begin a fresh performance for these instrument tracks.
    pub fn begin(&mut self, tracks: &[usize]) {
        self.tracks.clear();
        self.tracks.extend_from_slice(tracks);
        self.held.fill(None);
        self.notes.clear();
    }

    /// Stamp a note-on. A repeated on for an already-held pitch first closes
    /// the old note at the same sample: note-off before note-on on a tie, as
    /// required by the sequencing contract.
    pub fn note_on(&mut self, sample: u64, pitch: u8, velocity: u8) {
        if !self.recording() {
            return;
        }
        let pitch = pitch.min(127);
        self.close(pitch, sample);
        self.held[usize::from(pitch)] = Some((sample, velocity.clamp(1, 127)));
    }

    pub fn note_off(&mut self, sample: u64, pitch: u8) {
        if !self.recording() {
            return;
        }
        self.close(pitch.min(127), sample);
    }

    /// Split every held note at a transport discontinuity and keep the key
    /// held on the destination side.
    ///
    /// A loop wrap moves the musical sample clock backwards. Treating the
    /// later note-off as the end of the pre-wrap note collapses it to one
    /// sample; keeping one note across both sides is not representable on a
    /// linear arrangement either. Two notes are the truthful take: one ends
    /// at the loop boundary and its continuation begins at the loop head.
    pub fn discontinuity(&mut self, close_sample: u64, resume_sample: u64) {
        if !self.recording() {
            return;
        }
        let held = self.held;
        for (pitch, voice) in held.into_iter().enumerate() {
            let Some((_, velocity)) = voice else {
                continue;
            };
            self.close(pitch as u8, close_sample);
            self.held[pitch] = Some((resume_sample, velocity));
        }
    }

    fn close(&mut self, pitch: u8, sample: u64) {
        let Some((start_sample, velocity)) = self.held[usize::from(pitch)].take() else {
            return;
        };
        self.notes.push(MidiTakeNote {
            start_sample,
            end_sample: sample.max(start_sample.saturating_add(1)),
            pitch,
            velocity,
        });
    }

    /// Close every still-held note (the discontinuity all-sound-off), then
    /// hand one identical take to each armed destination.
    pub fn finish(&mut self, sample: u64) -> Vec<MidiTake> {
        for pitch in 0..128u8 {
            self.close(pitch, sample);
        }
        self.notes
            .sort_by_key(|note| (note.start_sample, note.end_sample, note.pitch));
        let notes = std::mem::take(&mut self.notes);
        let tracks = std::mem::take(&mut self.tracks);
        self.held.fill(None);
        tracks
            .into_iter()
            .map(|track| MidiTake {
                track,
                notes: notes.clone(),
            })
            .collect()
    }

    /// Abandon transient performance state without producing project data.
    pub fn cancel(&mut self) {
        self.tracks.clear();
        self.held.fill(None);
        self.notes.clear();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    #[error("could not make room for the recording: {0}")]
    Directory(String),
    #[error("could not open {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: hound::Error,
    },
    #[error("could not write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: hound::Error,
    },
    #[error("could not finish {path}: {source}")]
    Finish {
        path: PathBuf,
        #[source]
        source: hound::Error,
    },
    #[error("nothing is armed")]
    NothingArmed,
}

struct OpenTake {
    route: RecordRoute,
    path: PathBuf,
    writer: hound::WavWriter<BufWriter<std::fs::File>>,
    frames: u64,
    write_error: Option<hound::Error>,
}

/// Drains the capture ring into one file per armed lane.
pub struct Recorder {
    input: rtrb::Consumer<f32>,
    /// How many channels one frame in the ring carries. Fixed by the
    /// stream, and the whole reason the interleaved stream can be split.
    in_channels: usize,
    sample_rate: u32,
    open: Vec<OpenTake>,
    /// Whole frames pulled off the ring but not yet consumed. Green zone,
    /// so growing it is allowed — it is reused across polls rather than
    /// reallocated, which keeps a long take from churning the allocator.
    scratch: Vec<f32>,
}

impl Recorder {
    pub fn new(input: rtrb::Consumer<f32>, in_channels: usize, sample_rate: u32) -> Self {
        Self {
            input,
            in_channels: in_channels.max(1),
            sample_rate,
            open: Vec::new(),
            scratch: Vec::new(),
        }
    }

    pub fn recording(&self) -> bool {
        !self.open.is_empty()
    }

    /// How many frames the longest take has written so far.
    ///
    /// Read while the take is still open — `finish` empties the list —
    /// so it is what a caller uses to say how long a recording ran.
    pub fn frames(&self) -> u64 {
        self.open.iter().map(|take| take.frames).max().unwrap_or(0)
    }

    /// Open a file per route and start writing.
    ///
    /// The ring is DRAINED FIRST and the caller starts the callback
    /// filling it only after this returns: the ring is a pipe, not a
    /// session, and samples left in front of a new take would shift the
    /// whole take late by however long the last one ran over.
    pub fn begin(&mut self, routes: &[RecordRoute], dir: &Path) -> Result<(), RecordError> {
        if routes.is_empty() {
            return Err(RecordError::NothingArmed);
        }
        std::fs::create_dir_all(dir).map_err(|error| RecordError::Directory(error.to_string()))?;
        self.discard();

        let mut open: Vec<OpenTake> = Vec::with_capacity(routes.len());
        for route in routes {
            let channels = route.channels.len().clamp(1, 2) as u16;
            let path = unique_path(dir, route.track);
            let spec = hound::WavSpec {
                channels,
                sample_rate: self.sample_rate,
                // Float, and deliberately: a take is the input exactly as
                // it arrived, and rounding it on the way to disk would
                // make the file a worse copy than the thing that is
                // already in memory. Space is cheaper than the question
                // "was that clipped before or after I wrote it".
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            };
            let writer = match hound::WavWriter::create(&path, spec) {
                Ok(writer) => writer,
                Err(source) => {
                    // Opening a take is transactional across all armed lanes.
                    // Do not leave the earlier lanes looking like successful
                    // empty recordings if a later route cannot be opened.
                    for take in open {
                        let opened_path = take.path;
                        let _ = take.writer.finalize();
                        let _ = std::fs::remove_file(opened_path);
                    }
                    return Err(RecordError::Open { path, source });
                }
            };
            open.push(OpenTake {
                route: route.clone(),
                path,
                writer,
                frames: 0,
                write_error: None,
            });
        }
        self.open = open;
        Ok(())
    }

    /// Move whatever the callback has produced into the open files.
    ///
    /// Call once per frame. Does nothing at all when no take is open,
    /// including when the ring has leftovers — those belong to a take
    /// that is already closed and are dropped by the next `begin`.
    pub fn poll(&mut self) {
        if self.open.is_empty() {
            return;
        }
        let ready = self.input.slots() / self.in_channels * self.in_channels;
        if ready == 0 {
            return;
        }
        self.scratch.clear();
        self.scratch.reserve(ready);
        // `read_chunk` hands back up to two slices — the ring wraps —
        // and both are copied before anything is committed.
        if let Ok(chunk) = self.input.read_chunk(ready) {
            let (first, second) = chunk.as_slices();
            self.scratch.extend_from_slice(first);
            self.scratch.extend_from_slice(second);
            chunk.commit_all();
        }
        for take in &mut self.open {
            if take.write_error.is_some() {
                continue;
            }
            for frame in self.scratch.chunks_exact(self.in_channels) {
                // A route naming a channel the interface does not have
                // writes silence rather than failing: the same answer
                // `Node::Input` gives, so what is monitored and what is
                // recorded agree about a channel that is not there.
                let mut wrote_frame = true;
                if take.route.channels.is_empty() {
                    if let Err(error) = take.writer.write_sample(0.0) {
                        take.write_error = Some(error);
                        wrote_frame = false;
                    }
                } else {
                    for channel in take.route.channels.iter().take(2) {
                        let sample = frame.get(*channel as usize).copied().unwrap_or(0.0);
                        if let Err(error) = take.writer.write_sample(sample) {
                            take.write_error = Some(error);
                            wrote_frame = false;
                            break;
                        }
                    }
                }
                if !wrote_frame {
                    break;
                }
                take.frames += 1;
            }
        }
    }

    /// Close every open take and hand back what was written.
    ///
    /// The caller must have STOPPED the callback filling the ring before
    /// this, and should `poll` once more first — whatever is still in the
    /// ring at that moment is the tail of the take, and dropping it would
    /// clip the end off every recording.
    pub fn finish(&mut self) -> (Vec<Take>, Vec<RecordError>) {
        let mut takes = Vec::new();
        let mut errors = Vec::new();
        for take in std::mem::take(&mut self.open) {
            let path = take.path;
            let frames = take.frames;
            let channels = take.route.channels.len().clamp(1, 2) as u16;
            let write_error = take.write_error;
            let finalized = take.writer.finalize();
            if let Some(source) = write_error {
                errors.push(RecordError::Write {
                    path: path.clone(),
                    source,
                });
                if let Err(source) = finalized {
                    errors.push(RecordError::Finish {
                        path: path.clone(),
                        source,
                    });
                }
                let _ = std::fs::remove_file(path);
                continue;
            }
            match finalized {
                // An empty take is not a file worth keeping: it would sit
                // in the folder forever looking like a failed recording,
                // which is exactly what it is.
                Ok(()) if frames == 0 => {
                    let _ = std::fs::remove_file(&path);
                }
                Ok(()) => takes.push(Take {
                    track: take.route.track,
                    path,
                    frames,
                    channels,
                    sample_rate: self.sample_rate,
                }),
                Err(source) => errors.push(RecordError::Finish { path, source }),
            }
        }
        (takes, errors)
    }

    /// Throw away whatever is in the ring. What separates one take from
    /// the next.
    pub fn discard(&mut self) {
        let waiting = self.input.slots();
        if waiting > 0
            && let Ok(chunk) = self.input.read_chunk(waiting)
        {
            chunk.commit_all();
        }
    }
}

/// A name nothing else has: the lane, then a counter walked until the
/// file does not exist.
///
/// A counter and not a timestamp, because the clock is not available to
/// this crate's pure code and a take named for the wall time is not
/// nicer to read than a take named for its lane and its number.
fn unique_path(dir: &Path, track: usize) -> PathBuf {
    for take in 1..10_000 {
        let path = dir.join(format!("track-{}-take-{take}.wav", track + 1));
        if !path.exists() {
            return path;
        }
    }
    dir.join(format!("track-{}-take.wav", track + 1))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A directory nothing else in this run is using.
    fn scratch(name: &str) -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "daw-record-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A recorder wired to a ring, with the ring's writing end handed back.
    fn rig(channels: usize) -> (Recorder, rtrb::Producer<f32>) {
        let (tx, rx) = rtrb::RingBuffer::<f32>::new(1024);
        (Recorder::new(rx, channels, 48_000), tx)
    }

    /// Push whole frames of interleaved input, the way the callback does.
    fn feed(tx: &mut rtrb::Producer<f32>, frames: &[&[f32]]) {
        for frame in frames {
            for sample in *frame {
                tx.push(*sample).expect("the test ring is big enough");
            }
        }
    }

    fn read_back(path: &Path) -> (u16, Vec<f32>) {
        let reader = hound::WavReader::open(path).expect("the take is a readable wav");
        let channels = reader.spec().channels;
        let samples = reader
            .into_samples::<f32>()
            .map(|sample| sample.expect("a whole sample"))
            .collect();
        (channels, samples)
    }

    /// A TAKE IS THE CHANNELS IT WAS ROUTED TO, AND NOTHING ELSE.
    ///
    /// The demultiplex is the whole job on this side: one interleaved
    /// stream from the callback becomes one file per armed lane, each
    /// holding only the channels that lane asked for. Two lanes reading
    /// from one four-channel stream is the case that proves it, because
    /// getting the stride wrong still produces plausible-looking audio.
    #[test]
    fn a_take_holds_the_channels_its_lane_was_routed_to() {
        let dir = scratch("demux");
        let (mut recorder, mut tx) = rig(4);
        recorder
            .begin(
                &[
                    RecordRoute {
                        track: 0,
                        channels: vec![2],
                    },
                    RecordRoute {
                        track: 3,
                        channels: vec![0, 1],
                    },
                ],
                &dir,
            )
            .expect("the takes open");
        feed(
            &mut tx,
            &[
                &[0.1, 0.2, 0.3, 0.4],
                &[0.5, 0.6, 0.7, 0.8],
                &[0.9, 1.0, -1.0, -0.5],
            ],
        );
        recorder.poll();
        assert!(recorder.recording());
        assert_eq!(recorder.frames(), 3);
        let (takes, errors) = recorder.finish();
        assert!(errors.is_empty(), "{errors:?}");
        assert!(!recorder.recording());

        assert_eq!(takes.len(), 2);
        assert_eq!(takes[0].track, 0);
        assert_eq!(takes[0].channels, 1);
        assert_eq!(takes[0].frames, 3);
        let (channels, samples) = read_back(&takes[0].path);
        assert_eq!(channels, 1);
        assert_eq!(
            samples,
            vec![0.3, 0.7, -1.0],
            "the mono lane took the wrong channel"
        );

        assert_eq!(takes[1].track, 3);
        let (channels, samples) = read_back(&takes[1].path);
        assert_eq!(channels, 2);
        assert_eq!(
            samples,
            vec![0.1, 0.2, 0.5, 0.6, 0.9, 1.0],
            "the pair did not stay a pair"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A PARTIAL FRAME WAITS FOR THE REST OF ITSELF.
    ///
    /// The callback writes whole blocks, but the ring is drained on a
    /// UI frame that has no idea where a block boundary is. Consuming a
    /// half-arrived frame would shift every channel by one from there
    /// on — the take would still play, and every channel after the split
    /// would be somebody else's.
    #[test]
    fn a_half_arrived_frame_is_left_alone_until_it_is_whole() {
        let dir = scratch("partial");
        let (mut recorder, mut tx) = rig(3);
        recorder
            .begin(
                &[RecordRoute {
                    track: 0,
                    channels: vec![1],
                }],
                &dir,
            )
            .unwrap();
        // One whole frame and one sample of the next.
        feed(&mut tx, &[&[1.0, 2.0, 3.0]]);
        tx.push(4.0).unwrap();
        recorder.poll();
        assert_eq!(recorder.frames(), 1, "a partial frame was consumed");

        // The rest arrives and the frame completes where it left off.
        tx.push(5.0).unwrap();
        tx.push(6.0).unwrap();
        recorder.poll();
        assert_eq!(recorder.frames(), 2);
        let (takes, _) = recorder.finish();
        let (_, samples) = read_back(&takes[0].path);
        assert_eq!(samples, vec![2.0, 5.0], "the stride slipped");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// WHAT IS LEFT IN THE RING BELONGS TO THE LAST TAKE.
    ///
    /// The ring is a pipe, not a session. Samples still in it when a new
    /// take opens are the tail of the previous one, and letting them
    /// through would push the whole new take late by however long the
    /// last one ran over.
    #[test]
    fn a_new_take_does_not_begin_with_the_last_one() {
        let dir = scratch("stale");
        let (mut recorder, mut tx) = rig(2);
        feed(&mut tx, &[&[9.0, 9.0], &[9.0, 9.0]]);
        recorder
            .begin(
                &[RecordRoute {
                    track: 0,
                    channels: vec![0],
                }],
                &dir,
            )
            .unwrap();
        feed(&mut tx, &[&[1.0, 0.0], &[2.0, 0.0]]);
        recorder.poll();
        let (takes, _) = recorder.finish();
        let (_, samples) = read_back(&takes[0].path);
        assert_eq!(
            samples,
            vec![1.0, 2.0],
            "the last take leaked into this one"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AN EMPTY TAKE LEAVES NO FILE.
    ///
    /// A nought-length wav in the folder looks exactly like a recording
    /// that failed, which is what it is — so it is not kept.
    #[test]
    fn an_empty_take_leaves_nothing_behind() {
        let dir = scratch("empty");
        let (mut recorder, _tx) = rig(2);
        recorder
            .begin(
                &[RecordRoute {
                    track: 0,
                    channels: vec![0],
                }],
                &dir,
            )
            .unwrap();
        recorder.poll();
        let (takes, errors) = recorder.finish();
        assert!(takes.is_empty() && errors.is_empty());
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(left.is_empty(), "an empty take was kept: {left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A route naming a channel the interface does not have writes
    /// SILENCE, which is the same answer `Node::Input` gives — so what
    /// is monitored and what is recorded agree about a channel that is
    /// not there.
    #[test]
    fn a_channel_that_is_not_there_records_silence() {
        let dir = scratch("missing");
        let (mut recorder, mut tx) = rig(2);
        recorder
            .begin(
                &[RecordRoute {
                    track: 0,
                    channels: vec![7],
                }],
                &dir,
            )
            .unwrap();
        feed(&mut tx, &[&[1.0, 2.0], &[3.0, 4.0]]);
        recorder.poll();
        let (takes, _) = recorder.finish();
        let (_, samples) = read_back(&takes[0].path);
        assert_eq!(samples, vec![0.0, 0.0]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A malformed empty route is normalised to one silent channel. The WAV
    /// header and payload must agree even when older project data omitted its
    /// input-channel list.
    #[test]
    fn an_empty_route_still_writes_the_one_channel_declared_in_its_header() {
        let dir = scratch("empty-route");
        let (mut recorder, mut tx) = rig(2);
        recorder
            .begin(
                &[RecordRoute {
                    track: 0,
                    channels: Vec::new(),
                }],
                &dir,
            )
            .unwrap();
        feed(&mut tx, &[&[1.0, 2.0], &[3.0, 4.0]]);
        recorder.poll();
        let (takes, errors) = recorder.finish();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(takes.len(), 1);
        assert_eq!(read_back(&takes[0].path), (1, vec![0.0, 0.0]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two takes on one lane do not fight over a name.
    #[test]
    fn takes_do_not_overwrite_each_other() {
        let dir = scratch("names");
        let (mut recorder, mut tx) = rig(1);
        let route = [RecordRoute {
            track: 0,
            channels: vec![0],
        }];
        let mut paths = Vec::new();
        for value in [1.0_f32, 2.0] {
            recorder.begin(&route, &dir).unwrap();
            feed(&mut tx, &[&[value]]);
            recorder.poll();
            let (takes, _) = recorder.finish();
            paths.push(takes[0].path.clone());
        }
        assert_ne!(paths[0], paths[1], "the second take ate the first");
        assert_eq!(read_back(&paths[0]).1, vec![1.0]);
        assert_eq!(read_back(&paths[1]).1, vec![2.0]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Recording nothing is refused rather than opening no files and
    /// looking like it worked.
    #[test]
    fn recording_nothing_is_refused() {
        let dir = scratch("nothing");
        let (mut recorder, _tx) = rig(2);
        assert!(matches!(
            recorder.begin(&[], &dir),
            Err(RecordError::NothingArmed)
        ));
        assert!(!recorder.recording());
    }

    #[test]
    fn midi_pairing_keeps_sample_stamps_and_duplicates_destinations() {
        let mut recorder = MidiRecorder::default();
        recorder.begin(&[1, 4]);
        recorder.note_on(120, 60, 96);
        recorder.note_off(420, 60);
        let takes = recorder.finish(500);

        assert_eq!(takes.len(), 2);
        assert_eq!((takes[0].track, takes[1].track), (1, 4));
        assert_eq!(takes[0].notes, takes[1].notes);
        assert_eq!(
            takes[0].notes,
            vec![MidiTakeNote {
                start_sample: 120,
                end_sample: 420,
                pitch: 60,
                velocity: 96,
            }]
        );
        assert!(!recorder.recording());
    }

    #[test]
    fn repeated_midi_on_closes_before_reopening_and_finish_is_all_sound_off() {
        let mut recorder = MidiRecorder::default();
        recorder.begin(&[0]);
        recorder.note_on(10, 64, 80);
        recorder.note_on(20, 64, 100);
        let takes = recorder.finish(40);

        assert_eq!(
            takes[0].notes,
            vec![
                MidiTakeNote {
                    start_sample: 10,
                    end_sample: 20,
                    pitch: 64,
                    velocity: 80,
                },
                MidiTakeNote {
                    start_sample: 20,
                    end_sample: 40,
                    pitch: 64,
                    velocity: 100,
                },
            ]
        );
    }

    #[test]
    fn a_held_note_is_split_across_a_loop_wrap_instead_of_collapsing() {
        let mut recorder = MidiRecorder::default();
        recorder.begin(&[0]);
        recorder.note_on(90, 67, 88);
        recorder.discontinuity(100, 0);
        recorder.note_off(12, 67);
        let takes = recorder.finish(20);

        assert_eq!(
            takes[0].notes,
            vec![
                MidiTakeNote {
                    start_sample: 0,
                    end_sample: 12,
                    pitch: 67,
                    velocity: 88,
                },
                MidiTakeNote {
                    start_sample: 90,
                    end_sample: 100,
                    pitch: 67,
                    velocity: 88,
                },
            ]
        );
    }
}
