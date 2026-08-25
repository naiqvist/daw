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
