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
    #[error("compile: {0}")]
    Compile(#[from] CompileError),
    #[error("wav: {0}")]
    Wav(#[from] hound::Error),
    #[error("a disk stream did not become ready within {0:?} — stalled or unreadable file")]
    StreamTimeout(std::time::Duration),
}

pub struct BounceOptions {
    pub sample_rate: u32,
    pub block_frames: usize,
    pub bpm: f64,
    /// Render length in beats (converted through the transport's TimeMap —
    /// the same conversion the engine uses everywhere).
    pub length_beats: f64,
}

impl Default for BounceOptions {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            block_frames: 256,
            bpm: 120.0,
            length_beats: 8.0,
        }
    }
}

/// Render `spec` from timeline zero for `length_beats`, writing a stereo
/// 32-bit float wav.
pub fn bounce(spec: &GraphSpec, opts: &BounceOptions, path: &Path) -> Result<(), BounceError> {
    let mut sched = spec.compile_at_tempo(opts.sample_rate, opts.block_frames, opts.bpm)?;
    let mut transport = Transport::new(opts.sample_rate as f64);
    transport.apply(TransportCmd::SetTempo(opts.bpm));
    transport.apply(TransportCmd::Play);
    let total_samples = transport.map.beats_to_samples(opts.length_beats);

    let wav_spec = hound::WavSpec {
        channels: 2,
        sample_rate: opts.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, wav_spec)?;

    let frames = opts.block_frames;
    let mut block = vec![0.0f32; frames * 2]; // planar L then R
    let no_input = vec![0.0f32; frames * 2];
    let mut rendered: u64 = 0;

    while rendered < total_samples {
        let want = frames.min((total_samples - rendered) as usize);
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
            writer.write_sample(block[i])?; // L
            writer.write_sample(block[frames + i])?; // R
        }
        rendered += want as u64;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests may panic loudly; the deny guards the red zone
mod tests {
    use super::*;
    use crate::audio::graph::{NodeSpec, Note};

    #[test]
    fn bounce_is_deterministic_and_audible() {
        let mut spec = GraphSpec::default();
        let seq = spec.push(NodeSpec::Seq {
            notes: vec![Note {
                start_beats: 0.0,
                len_beats: 0.5,
                pitch: 69,
                vel: 110,
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
        };
        let w = std::env::temp_dir().join("daw-test-len.wav");
        bounce(&spec, &opts, &w).unwrap();
        let r = hound::WavReader::open(&w).unwrap();
        // 3 beats at 120bpm = 1.5s = 72_000 frames.
        assert_eq!(r.duration(), 72_000);
    }
}
