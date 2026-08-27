//! Offline render: drive the schedule through the same transport/segment
//! machinery as the live callback, but with no device and no deadline —
//! writing a stereo wav. Green zone throughout; disk streams are WAITED for
//! (block_streams_ready) so a bounce can never contain buffering gaps.
//!
//! This is also the proof of the timeline-locked contract: a bounce of the
//! same project is deterministic, byte for byte.

use crate::audio::graph::{CompileError, GraphSpec, ProcessCtx};
use crate::audio::transport::{Transport, TransportCmd};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum BounceError {
    #[error("cancelled")]
    Cancelled,
    #[error("compile: {0}")]
    Compile(#[from] CompileError),
    #[error("wav: {0}")]
    Wav(#[from] hound::Error),
    #[error("a disk stream did not become ready within {0:?} — stalled or unreadable file")]
    StreamTimeout(std::time::Duration),
}

/// What the samples are written as.
///
/// Float32 is the render's own arithmetic written down unchanged — nothing
/// is scaled, rounded or clipped, so a bounce that overshoots can still be
/// pulled back. The integer formats are the ones a mix is DELIVERED in, and
/// they clip: there is no headroom above full scale to keep.
///
/// Neither integer format dithers. At 24 bits the truncation floor sits
/// below any room's, and at 16 it is a known, deliberate omission rather
/// than an oversight — dither is a decision with a sound, and it belongs
/// beside a mastering stage rather than inside a file writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BounceFormat {
    #[default]
    Float32,
    Int24,
    Int16,
}

impl BounceFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Float32 => "32-bit float",
            Self::Int24 => "24-bit",
            Self::Int16 => "16-bit",
        }
    }

    fn wav_spec(self, sample_rate: u32) -> hound::WavSpec {
        let (bits, sample_format) = match self {
            Self::Float32 => (32, hound::SampleFormat::Float),
            Self::Int24 => (24, hound::SampleFormat::Int),
            Self::Int16 => (16, hound::SampleFormat::Int),
        };
        hound::WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: bits,
            sample_format,
        }
    }
}

pub struct BounceOptions {
    pub sample_rate: u32,
    pub block_frames: usize,
    pub bpm: f64,
    /// Render length in beats (converted through the transport's TimeMap —
    /// the same conversion the engine uses everywhere).
    pub length_beats: f64,
    /// Where the WRITTEN file starts, in beats.
    ///
    /// The render always runs from timeline zero and the head is thrown
    /// away, which is not laziness: a delay, a reverb or a compressor
    /// arriving at bar 9 sounds the way it does because of bars 1 to 8,
    /// and a render that began there would be a different piece of audio
    /// from the one the transport plays. Exporting the tail of a long song
    /// therefore costs the whole song's render time.
    pub start_beats: f64,
    pub format: BounceFormat,
}

impl Default for BounceOptions {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            block_frames: 256,
            bpm: 120.0,
            length_beats: 8.0,
            start_beats: 0.0,
            format: BounceFormat::Float32,
        }
    }
}

/// Render `spec` from timeline zero, writing the stereo wav `opts` asks
/// for. See [`BounceOptions::start_beats`] on why a later start still
/// renders from the top.
pub fn bounce(spec: &GraphSpec, opts: &BounceOptions, path: &Path) -> Result<(), BounceError> {
    bounce_with(spec, opts, path, |_| true)
}

/// The same, reporting how far along it is as a fraction in `0..=1`.
///
/// The hook runs once per block on the rendering thread, and returns
/// whether to carry on: `false` abandons the render with
/// [`BounceError::Cancelled`] and REMOVES the part-written file, because a
/// truncated wav is not a shorter song — it is a broken one, and leaving it
/// on disk under the name the user chose is worse than leaving nothing.
pub fn bounce_with(
    spec: &GraphSpec,
    opts: &BounceOptions,
    path: &Path,
    progress: impl FnMut(f32) -> bool,
) -> Result<(), BounceError> {
    bounce_automated(spec, opts, path, |_, _| {}, progress)
}

/// The same again, with a hand on the parameters.
///
/// `letters` is called once per block with the beat that block STARTS at,
/// and everything it pushes is applied before the block runs. This is how
/// automation reaches an offline render: live, an envelope is a stream of
/// parameter letters from the UI thread, and a render with no UI thread
/// would otherwise hear every fader parked at its written-down value —
/// silently, which is the worst way for a mix to be wrong.
///
/// Per BLOCK rather than per sample: a letter is smoothed by the node that
/// receives it, exactly as a live one is, so the two paths hear the same
/// ramp shape.
pub fn bounce_automated(
    spec: &GraphSpec,
    opts: &BounceOptions,
    path: &Path,
    mut letters: impl FnMut(f64, &mut Vec<crate::audio::graph::ParamChange>),
    mut progress: impl FnMut(f32) -> bool,
) -> Result<(), BounceError> {
    let mut sched = spec.compile_at_tempo(opts.sample_rate, opts.block_frames, opts.bpm)?;
    let mut transport = Transport::new(opts.sample_rate as f64);
    transport.apply(TransportCmd::SetTempo(opts.bpm));
    transport.apply(TransportCmd::Play);
    let total_samples = transport.map.beats_to_samples(opts.length_beats.max(0.0));
    // The head that is rendered and thrown away, never written.
    let skip_samples = transport
        .map
        .beats_to_samples(opts.start_beats.max(0.0))
        .min(total_samples);

    let mut writer = hound::WavWriter::create(path, opts.format.wav_spec(opts.sample_rate))?;

    let frames = opts.block_frames;
    let mut block = vec![0.0f32; frames * 2]; // planar L then R
    let no_input = vec![0.0f32; frames * 2];
    let mut rendered: u64 = 0;

    let mut pending: Vec<crate::audio::graph::ParamChange> = Vec::new();
    while rendered < total_samples {
        let want = frames.min((total_samples - rendered) as usize);
        // Automation for the block about to run, before it runs.
        pending.clear();
        letters(transport.map.samples_to_beats(rendered), &mut pending);
        for change in pending.drain(..) {
            sched.apply(change);
        }
        // Offline luxury: wait for disk streams before every block — but
        // with a deadline. A stalled stream is an error, not a hang.
        let timeout = std::time::Duration::from_secs(5);
        if !sched.wait_streams_ready(timeout) {
            return Err(BounceError::StreamTimeout(timeout));
        }

        // The same per-block meter window the live callback opens. Offline
        // has no meters to draw, but this is also what hands the previous
        // block's peaks to the modulation plan — without it every FOLLOWER
        // detects silence for the whole render, and a ducking wire bounces
        // as a flat offset instead of a pump. "Offline equals live" is the
        // contract; this call is part of it.
        sched.clear_peaks();

        let mut done = 0usize;
        while done < want {
            let seg = transport.next_segment(want - done);
            let ctx = ProcessCtx {
                device_input: &no_input,
                in_channels: 2,
                block_frames: frames,
                offset: done,
                len: seg.len,
                playing: seg.playing,
                position: seg.position,
                beat: seg.beat,
                beats_per_sample: transport.map.beats_per_sample(),
                discontinuity: seg.discontinuity,
            };
            sched.run(&mut block, &ctx);
            done += seg.len;
        }

        for i in 0..want {
            if rendered + i as u64 >= skip_samples {
                write_frame(&mut writer, opts.format, block[i], block[frames + i])?;
            }
        }
        rendered += want as u64;
        let at = if total_samples > 0 {
            rendered as f32 / total_samples as f32
        } else {
            1.0
        };
        if !progress(at) {
            // The writer is dropped without finalizing, then the file goes
            // with it: nothing on disk is better than a wav with a header
            // that lies about its length.
            drop(writer);
            let _ = std::fs::remove_file(path);
            return Err(BounceError::Cancelled);
        }
    }
    writer.finalize()?;
    Ok(())
}

/// One stereo frame, in the format asked for.
fn write_frame<W: std::io::Write + std::io::Seek>(
    writer: &mut hound::WavWriter<W>,
    format: BounceFormat,
    l: f32,
    r: f32,
) -> Result<(), hound::Error> {
    match format {
        BounceFormat::Float32 => {
            writer.write_sample(l)?;
            writer.write_sample(r)?;
        }
        // Full scale is one bit short of the positive ceiling so that -1.0
        // and +1.0 are symmetric: the alternative rounds +1.0 up past what
        // the format can hold and wraps it to silence.
        BounceFormat::Int24 => {
            writer.write_sample(to_int(l, 23))?;
            writer.write_sample(to_int(r, 23))?;
        }
        BounceFormat::Int16 => {
            writer.write_sample(to_int(l, 15) as i16)?;
            writer.write_sample(to_int(r, 15) as i16)?;
        }
    }
    Ok(())
}

/// A sample as a `bits`-plus-sign integer, CLIPPED at full scale. A mix
/// that overshoots must arrive as a flat top, which is audible, rather than
/// wrapping to the opposite rail, which is a bang.
fn to_int(sample: f32, bits: u32) -> i32 {
    let scale = ((1u32 << bits) - 1) as f32;
    let clipped = if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    (clipped * scale).round() as i32
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests may panic loudly; the deny guards the red zone
mod tests {
    use super::*;
    use crate::audio::graph::{NodeSpec, Note};

    fn ping(gain: f32) -> GraphSpec {
        let mut spec = GraphSpec::default();
        let seq = spec.push(NodeSpec::Seq {
            notes: vec![Note {
                start_beats: 0.0,
                len_beats: 0.5,
                pitch: 69,
                vel: 110,
                plocks: Vec::new(),
                prob: 1.0,
                cond: None,
            }],
            subloops: Vec::new(),
            loop_len_beats: Some(1.0),
            params: Default::default(),
        });
        let mix = spec.push(NodeSpec::Mixer { gain });
        spec.connect(seq, mix);
        spec.set_output(mix);
        spec
    }

    fn frames(path: &Path) -> u64 {
        let reader = hound::WavReader::open(path).unwrap();
        u64::from(reader.duration())
    }

    /// A later start writes a shorter file, and what it writes is the TAIL
    /// of the same render — not a fresh one begun at that beat.
    #[test]
    fn a_start_beat_trims_the_head_off_the_same_render() {
        let spec = ping(0.6);
        let whole = std::env::temp_dir().join("daw-test-range-whole.wav");
        let tail = std::env::temp_dir().join("daw-test-range-tail.wav");
        bounce(
            &spec,
            &BounceOptions {
                length_beats: 4.0,
                ..Default::default()
            },
            &whole,
        )
        .unwrap();
        bounce(
            &spec,
            &BounceOptions {
                length_beats: 4.0,
                start_beats: 3.0,
                ..Default::default()
            },
            &tail,
        )
        .unwrap();

        let (whole_n, tail_n) = (frames(&whole), frames(&tail));
        assert_eq!(tail_n * 4, whole_n, "one beat of four");

        // Sample for sample, the tail IS the end of the whole render.
        let all: Vec<f32> = hound::WavReader::open(&whole)
            .unwrap()
            .samples::<f32>()
            .map(Result::unwrap)
            .collect();
        let end: Vec<f32> = hound::WavReader::open(&tail)
            .unwrap()
            .samples::<f32>()
            .map(Result::unwrap)
            .collect();
        assert_eq!(&all[all.len() - end.len()..], &end[..]);
    }

    /// The integer formats land where they say they do, and CLIP rather
    /// than wrap when the mix is over.
    #[test]
    fn integer_formats_are_scaled_and_clipped() {
        assert_eq!(to_int(1.0, 15), 32_767);
        assert_eq!(to_int(-1.0, 15), -32_767);
        assert_eq!(to_int(0.0, 15), 0);
        assert_eq!(to_int(4.0, 15), 32_767, "an overshoot flattens, not wraps");
        assert_eq!(to_int(-4.0, 15), -32_767);
        assert_eq!(to_int(f32::NAN, 15), 0, "and a NaN is silence, not noise");
        assert_eq!(to_int(1.0, 23), 8_388_607);

        // And the file says what it is.
        let path = std::env::temp_dir().join("daw-test-int16.wav");
        bounce(
            &ping(0.6),
            &BounceOptions {
                length_beats: 1.0,
                format: BounceFormat::Int16,
                ..Default::default()
            },
            &path,
        )
        .unwrap();
        let spec = hound::WavReader::open(&path).unwrap().spec();
        assert_eq!(spec.bits_per_sample, 16);
        assert_eq!(spec.sample_format, hound::SampleFormat::Int);
        assert_eq!(spec.channels, 2);
    }

    /// Automation reaches an offline render: the same song, rendered once
    /// with a fader letter arriving mid-way and once without, must differ.
    #[test]
    fn automation_letters_reach_the_render() {
        use crate::audio::graph::ParamChange;

        let mut spec = GraphSpec::default();
        let seq = spec.push(NodeSpec::Seq {
            notes: vec![Note {
                start_beats: 0.0,
                len_beats: 4.0,
                pitch: 69,
                vel: 110,
                plocks: Vec::new(),
                prob: 1.0,
                cond: None,
            }],
            subloops: Vec::new(),
            loop_len_beats: None,
            params: Default::default(),
        });
        let pan = spec.push(NodeSpec::Pan {
            pan: 0.0,
            gain: 1.0,
        });
        spec.connect(seq, pan);
        spec.set_output(pan);

        let opts = BounceOptions {
            length_beats: 4.0,
            ..Default::default()
        };
        let plain = std::env::temp_dir().join("daw-test-auto-off.wav");
        let faded = std::env::temp_dir().join("daw-test-auto-on.wav");
        bounce(&spec, &opts, &plain).unwrap();
        bounce_automated(
            &spec,
            &opts,
            &faded,
            |beat, out| {
                // Silence from the halfway mark on.
                if beat >= 2.0 {
                    out.push(ParamChange {
                        node: pan.to_bits(),
                        param: crate::params::pan::GAIN,
                        value: 0.0,
                    });
                }
            },
            |_| true,
        )
        .unwrap();

        let tail_peak = |path: &Path| {
            let mut reader = hound::WavReader::open(path).unwrap();
            let all: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).collect();
            // The last quarter: well past the fader move and past its ramp.
            all[all.len() * 3 / 4..]
                .iter()
                .fold(0.0f32, |peak, s| peak.max(s.abs()))
        };
        assert!(
            tail_peak(&plain) > 0.01,
            "the unautomated render must still be sounding at the end"
        );
        assert!(
            tail_peak(&faded) < 0.001,
            "a fader pulled to silence must be HEARD to be pulled: {}",
            tail_peak(&faded)
        );
    }

    /// A cancelled render leaves NOTHING behind: a truncated wav under the
    /// name the user chose is worse than no file at all.
    #[test]
    fn cancelling_removes_the_part_written_file() {
        let path = std::env::temp_dir().join("daw-test-cancelled.wav");
        let _ = std::fs::remove_file(&path);
        let mut blocks = 0;
        let outcome = bounce_with(
            &ping(0.6),
            &BounceOptions {
                length_beats: 64.0,
                ..Default::default()
            },
            &path,
            |_| {
                blocks += 1;
                blocks < 3
            },
        );
        assert!(
            matches!(outcome, Err(BounceError::Cancelled)),
            "{outcome:?}"
        );
        assert!(!path.exists(), "the half-written file must not survive");
    }

    /// Progress runs from something to exactly one, so a bar cannot stall
    /// short of the end of a render that has finished.
    #[test]
    fn progress_reaches_the_end() {
        let mut seen: Vec<f32> = Vec::new();
        bounce_with(
            &ping(0.6),
            &BounceOptions {
                length_beats: 4.0,
                ..Default::default()
            },
            &std::env::temp_dir().join("daw-test-progress.wav"),
            |at| {
                seen.push(at);
                true
            },
        )
        .unwrap();
        assert!(
            seen.len() > 1,
            "a multi-block render reports more than once"
        );
        assert!(seen.windows(2).all(|w| w[1] >= w[0]), "and never goes back");
        assert_eq!(seen.last().copied(), Some(1.0));
    }

    #[test]
    fn bounce_is_deterministic_and_audible() {
        let mut spec = GraphSpec::default();
        let seq = spec.push(NodeSpec::Seq {
            notes: vec![Note {
                start_beats: 0.0,
                len_beats: 0.5,
                pitch: 69,
                vel: 110,
                plocks: Vec::new(),
                prob: 1.0,
                cond: None,
            }],
            subloops: Vec::new(),
            loop_len_beats: Some(1.0),
            params: Default::default(),
        });
        let mix = spec.push(NodeSpec::Mixer { gain: 0.6 });
        spec.connect(seq, mix);
        spec.set_output(mix);

        let opts = BounceOptions {
            length_beats: 4.0,
            ..Default::default()
        };
        let w1 = std::env::temp_dir().join("daw-test-det-1.wav");
        let w2 = std::env::temp_dir().join("daw-test-det-2.wav");
        bounce(&spec, &opts, &w1).unwrap();
        bounce(&spec, &opts, &w2).unwrap();
        assert_eq!(
            std::fs::read(&w1).unwrap(),
            std::fs::read(&w2).unwrap(),
            "two bounces of one project must be byte-identical"
        );

        // And it must actually contain the note, on both channels (mono
        // synth centered through the stereo mixer).
        let mut r = hound::WavReader::open(&w1).unwrap();
        let samples: Vec<f32> = r.samples::<f32>().map(|s| s.unwrap()).collect();
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.1, "bounce should contain audio (peak {peak})");
        let (l, r_ch): (Vec<f32>, Vec<f32>) = samples.chunks(2).map(|c| (c[0], c[1])).unzip();
        assert_eq!(l, r_ch, "centered mono content must match on both channels");
    }

    /// The reason engine-side modulation exists. Modulation used to be
    /// evaluated in the UI frame loop, which an offline render never runs —
    /// so an LFO that was plainly audible live was simply ABSENT from the
    /// wav. Compiled into the schedule, it renders here, and it renders
    /// identically twice.
    fn lfo_on_a_mixer_gain(free: bool) -> GraphSpec {
        use crate::audio::modulation::{Chain, ModKind, ModShape, ModSpec, Modulator, WireSpec};

        let mut spec = GraphSpec::default();
        let sine = spec.push(NodeSpec::Sine {
            freq: 220.0,
            amp: 0.5,
        });
        let mix = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(sine, mix);
        spec.set_output(mix);

        spec.set_modulation(ModSpec {
            sources: vec![Modulator {
                id: 1,
                kind: ModKind::Lfo {
                    shape: ModShape::Sine,
                    rate_beats: 1.0,
                    free,
                    hz: 2.0,
                },
            }],
            wires: vec![WireSpec {
                id: 10,
                source: 1,
                node: mix,
                param: crate::params::mixer::GAIN,
                min: 0.0,
                max: 2.0,
                log: false,
                base: 1.0,
                chain: Chain {
                    depth: 0.5,
                    curve: 0.0,
                    steps: 0,
                    smooth_ms: 0.0,
                },
                enabled: true,
                solo: false,
            }],
        });
        spec
    }

    /// A FOLLOWER wire in an offline render — ducking one thing by
    /// another, which the modulation module names as the reason followers
    /// exist. A follower detects the previous block's metered peak, and
    /// that only reaches it if the renderer opens a meter window per block.
    /// It did not for a while, and every follower bounced frozen at
    /// zero-level: no ducking at all, silently.
    ///
    /// A pulsing source drives the detector, so a working follower makes
    /// the output move; a starved one renders flat.
    fn follower_ducking_a_gain() -> GraphSpec {
        use crate::audio::modulation::{Chain, ModKind, ModSpec, Modulator, WireSpec};

        let mut spec = GraphSpec::default();
        // A note every beat: the detector sees a loud attack then a decay,
        // which is the level a follower is meant to track.
        let seq = spec.push(NodeSpec::Seq {
            notes: vec![Note {
                start_beats: 0.0,
                len_beats: 0.25,
                pitch: 69,
                vel: 127,
                plocks: Vec::new(),
                prob: 1.0,
                cond: None,
            }],
            subloops: Vec::new(),
            loop_len_beats: Some(1.0),
            params: Default::default(),
        });
        let mix = spec.push(NodeSpec::Mixer { gain: 1.0 });
        spec.connect(seq, mix);
        spec.set_output(mix);
        // Meter slot 0 is what `ModKind::Follower { track: 0 }` listens to.
        spec.meter(0, mix);

        spec.set_modulation(ModSpec {
            sources: vec![Modulator {
                id: 1,
                kind: ModKind::Follower { track: 0 },
            }],
            wires: vec![WireSpec {
                id: 10,
                source: 1,
                node: mix,
                param: crate::params::mixer::GAIN,
                min: 0.0,
                max: 2.0,
                log: false,
                base: 1.0,
                chain: Chain {
                    depth: 0.9,
                    curve: 0.0,
                    steps: 0,
                    smooth_ms: 0.0,
                },
                enabled: true,
                solo: false,
            }],
        });
        spec
    }

    #[test]
    fn bounce_renders_followers() {
        let spec = follower_ducking_a_gain();
        let opts = BounceOptions {
            length_beats: 4.0,
            ..Default::default()
        };
        let w = std::env::temp_dir().join("daw-test-follower.wav");
        bounce(&spec, &opts, &w).unwrap();

        // The detector's own output is what we are proving reaches the
        // node, so compare against the SAME graph with the wire removed:
        // if the follower did nothing, the two renders would match.
        let mut plain = follower_ducking_a_gain();
        plain.set_modulation(Default::default());
        let w_plain = std::env::temp_dir().join("daw-test-follower-plain.wav");
        bounce(&plain, &opts, &w_plain).unwrap();

        assert_ne!(
            std::fs::read(&w).unwrap(),
            std::fs::read(&w_plain).unwrap(),
            "a follower wire must change an offline render — a starved \
             detector renders identically to no wire at all"
        );

        // And it is still deterministic.
        let w2 = std::env::temp_dir().join("daw-test-follower-2.wav");
        bounce(&spec, &opts, &w2).unwrap();
        assert_eq!(
            std::fs::read(&w).unwrap(),
            std::fs::read(&w2).unwrap(),
            "followers ride block peaks, which are themselves deterministic"
        );
    }

    /// Peak amplitude per short window — an LFO on a gain shows up as a
    /// spread between the loudest and quietest window.
    fn window_peaks(samples: &[f32], window: usize) -> Vec<f32> {
        samples
            .chunks(window)
            .map(|c| c.iter().fold(0.0f32, |m, s| m.max(s.abs())))
            .collect()
    }

    #[test]
    fn bounce_renders_modulation_and_repeats() {
        let spec = lfo_on_a_mixer_gain(false);
        let opts = BounceOptions {
            length_beats: 4.0,
            ..Default::default()
        };
        let w1 = std::env::temp_dir().join("daw-test-mod-1.wav");
        let w2 = std::env::temp_dir().join("daw-test-mod-2.wav");
        bounce(&spec, &opts, &w1).unwrap();
        bounce(&spec, &opts, &w2).unwrap();
        assert_eq!(
            std::fs::read(&w1).unwrap(),
            std::fs::read(&w2).unwrap(),
            "a synced LFO is timeline-locked: two bounces must be identical"
        );

        // Reproducible silence would also pass the check above, so prove
        // the modulation is actually THERE: the gain must visibly breathe.
        let mut r = hound::WavReader::open(&w1).unwrap();
        let samples: Vec<f32> = r.samples::<f32>().map(|s| s.unwrap()).collect();
        let peaks = window_peaks(&samples, 4096);
        let loudest = peaks.iter().fold(0.0f32, |m, p| m.max(*p));
        let quietest = peaks.iter().fold(f32::MAX, |m, p| m.min(*p));
        assert!(
            loudest > 0.1,
            "bounce should contain audio (peak {loudest})"
        );
        assert!(
            loudest - quietest > 0.1,
            "an LFO on the gain should breathe: {quietest}..{loudest}"
        );
    }

    /// Free LFOs run on the ENGINE's sample clock, not a wall clock, so
    /// "free of the timeline" does not cost determinism.
    #[test]
    fn a_free_lfo_still_bounces_identically() {
        let spec = lfo_on_a_mixer_gain(true);
        let opts = BounceOptions {
            length_beats: 4.0,
            ..Default::default()
        };
        let w1 = std::env::temp_dir().join("daw-test-freemod-1.wav");
        let w2 = std::env::temp_dir().join("daw-test-freemod-2.wav");
        bounce(&spec, &opts, &w1).unwrap();
        bounce(&spec, &opts, &w2).unwrap();
        assert_eq!(
            std::fs::read(&w1).unwrap(),
            std::fs::read(&w2).unwrap(),
            "a free LFO rides the sample clock, so it must still repeat"
        );
    }

    #[test]
    fn bounce_length_is_exact() {
        let spec = GraphSpec::default(); // silence
        let opts = BounceOptions {
            length_beats: 3.0,
            bpm: 120.0,
            sample_rate: 48_000,
            block_frames: 256,
            ..Default::default()
        };
        let w = std::env::temp_dir().join("daw-test-len.wav");
        bounce(&spec, &opts, &w).unwrap();
        let r = hound::WavReader::open(&w).unwrap();
        // 3 beats at 120bpm = 1.5s = 72_000 frames.
        assert_eq!(r.duration(), 72_000);
    }
}
