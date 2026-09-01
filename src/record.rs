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

        let mut open = Vec::with_capacity(routes.len());
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
            let writer =
                hound::WavWriter::create(&path, spec).map_err(|source| RecordError::Open {
                    path: path.clone(),
                    source,
                })?;
            open.push(OpenTake {
                route: route.clone(),
                path,
                writer,
                frames: 0,
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
            for frame in self.scratch.chunks_exact(self.in_channels) {
                // A route naming a channel the interface does not have
                // writes silence rather than failing: the same answer
                // `Node::Input` gives, so what is monitored and what is
                // recorded agree about a channel that is not there.
                for channel in take.route.channels.iter().take(2) {
                    let sample = frame.get(*channel as usize).copied().unwrap_or(0.0);
                    let _ = take.writer.write_sample(sample);
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
            match take.writer.finalize() {
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
}
