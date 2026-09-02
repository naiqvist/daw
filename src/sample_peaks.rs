//! The peaks of a file, for drawing it: a pyramid of min / max / RMS bins.
//!
//! GREEN ZONE, and no audio types: built from plain samples so a surface
//! that may not name the audio module can hold one. The stage draws a
//! waveform as work proportional to its pixel width — one column, one
//! bin — never to the file's length, at every zoom. The finest level
//! covers [`BASE`] frames per bin; each coarser level merges [`FOLD`] of
//! the last. A view asks for `count` columns over a span and gets the
//! level whose bins are the largest that still fit under a column, so
//! nothing on screen was averaged past what a pixel can show.
//!
//! Three numbers per bin, because a waveform read at a glance is three
//! tones: the extremes say where the transients reach, the RMS says how
//! dense the sound is, and the difference between them is the picture.
//! When the view is closer than the finest bin, the columns are cut from
//! the samples themselves, so zooming all the way in shows the cycles.

/// Frames per bin at the finest level.
pub const BASE: u64 = 64;
/// How many bins of one level make one bin of the next.
pub const FOLD: usize = 4;

/// One bin, or one drawn column: the extremes and the density.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bin {
    pub min: f32,
    pub max: f32,
    pub rms: f32,
}

impl Bin {
    const EMPTY: Bin = Bin {
        min: 0.0,
        max: 0.0,
        rms: 0.0,
    };

    fn merge(bins: &[Bin]) -> Bin {
        if bins.is_empty() {
            return Bin::EMPTY;
        }
        let mut out = Bin {
            min: f32::MAX,
            max: f32::MIN,
            rms: 0.0,
        };
        let mut energy = 0.0f64;
        for bin in bins {
            out.min = out.min.min(bin.min);
            out.max = out.max.max(bin.max);
            energy += f64::from(bin.rms) * f64::from(bin.rms);
        }
        out.rms = (energy / bins.len() as f64).sqrt() as f32;
        out
    }
}

/// The pyramid.
#[derive(Clone, Debug, PartialEq)]
pub struct Peaks {
    /// `levels[0]` is the finest; each next level folds the last.
    levels: Vec<Vec<Bin>>,
    frames: u64,
    channels: usize,
    sample_rate: u32,
    /// The loudest magnitude anywhere in the file, for normalizing.
    peak: f32,
}

impl Peaks {
    /// Build from planar samples: channel `c` at
    /// `samples[c * frames .. (c + 1) * frames]`. Channels are folded
    /// together — the extremes across all of them, the RMS over all of
    /// them — because the editor shows one picture of the file.
    pub fn build(samples: &[f32], channels: usize, frames: u64, sample_rate: u32) -> Self {
        let channels = channels.max(1);
        let frames_usize = frames as usize;
        let mut peak = 0.0f32;
        let bins = frames.div_ceil(BASE) as usize;
        let mut level0 = Vec::with_capacity(bins);
        for bin in 0..bins {
            let from = bin * BASE as usize;
            let to = ((bin + 1) * BASE as usize).min(frames_usize);
            let mut out = Bin {
                min: f32::MAX,
                max: f32::MIN,
                rms: 0.0,
            };
            let mut energy = 0.0f64;
            let mut counted = 0usize;
            for channel in 0..channels {
                let base = channel * frames_usize;
                let Some(span) = samples.get(base + from..base + to) else {
                    continue;
                };
                for s in span {
                    let s = if s.is_finite() { *s } else { 0.0 };
                    out.min = out.min.min(s);
                    out.max = out.max.max(s);
                    energy += f64::from(s) * f64::from(s);
                    peak = peak.max(s.abs());
                }
                counted += span.len();
            }
            if counted == 0 {
                out = Bin::EMPTY;
            } else {
                out.rms = (energy / counted as f64).sqrt() as f32;
            }
            level0.push(out);
        }
        let mut levels = vec![level0];
        while levels.last().is_some_and(|level| level.len() > FOLD) {
            let last = levels.last().map(Vec::as_slice).unwrap_or(&[]);
            let next: Vec<Bin> = last.chunks(FOLD).map(Bin::merge).collect();
            levels.push(next);
        }
        Self {
            levels,
            frames,
            channels,
            sample_rate,
            peak,
        }
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The loudest magnitude in the file, `0..=1` for a file in range.
    pub fn peak(&self) -> f32 {
        self.peak
    }

    pub fn seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.frames as f64 / f64::from(self.sample_rate)
        }
    }

    /// How many levels the pyramid holds.
    pub fn depth(&self) -> usize {
        self.levels.len()
    }

    /// `count` columns over the span `from..to`, as fractions of the
    /// file. From the pyramid's best-fitting level, or — when a column
    /// is finer than the finest bin and `samples` are given — from the
    /// samples themselves.
    pub fn columns(&self, samples: Option<&[f32]>, from: f64, to: f64, count: usize) -> Vec<Bin> {
        if count == 0 || self.frames == 0 {
            return Vec::new();
        }
        let from = from.clamp(0.0, 1.0);
        let to = to.clamp(from, 1.0);
        let span_frames = (to - from) * self.frames as f64;
        let per_column = span_frames / count as f64;
        if per_column < BASE as f64
            && let Some(samples) = samples
        {
            return self.columns_from_samples(samples, from, to, count);
        }
        // The coarsest level whose bins are no wider than a column.
        let mut level = 0usize;
        let mut bin_frames = BASE as f64;
        while level + 1 < self.levels.len() && bin_frames * FOLD as f64 <= per_column {
            level += 1;
            bin_frames *= FOLD as f64;
        }
        let bins = &self.levels[level];
        let start_frame = from * self.frames as f64;
        (0..count)
            .map(|column| {
                let a = start_frame + per_column * column as f64;
                let b = a + per_column;
                let first = (a / bin_frames).floor() as usize;
                let last = ((b / bin_frames).ceil() as usize).max(first + 1);
                let slice = bins.get(first..last.min(bins.len())).unwrap_or(&[]);
                Bin::merge(slice)
            })
            .collect()
    }

    fn columns_from_samples(&self, samples: &[f32], from: f64, to: f64, count: usize) -> Vec<Bin> {
        let frames = self.frames as usize;
        let start = from * frames as f64;
        let per_column = (to - from) * frames as f64 / count as f64;
        (0..count)
            .map(|column| {
                let a = (start + per_column * column as f64).floor() as usize;
                let b = ((start + per_column * (column + 1) as f64).ceil() as usize)
                    .max(a + 1)
                    .min(frames);
                if a >= frames {
                    return Bin::EMPTY;
                }
                let mut out = Bin {
                    min: f32::MAX,
                    max: f32::MIN,
                    rms: 0.0,
                };
                let mut energy = 0.0f64;
                let mut counted = 0usize;
                for channel in 0..self.channels {
                    let base = channel * frames;
                    let Some(span) = samples.get(base + a..base + b) else {
                        continue;
                    };
                    for s in span {
                        let s = if s.is_finite() { *s } else { 0.0 };
                        out.min = out.min.min(s);
                        out.max = out.max.max(s);
                        energy += f64::from(s) * f64::from(s);
                    }
                    counted += span.len();
                }
                if counted == 0 {
                    Bin::EMPTY
                } else {
                    out.rms = (energy / counted as f64).sqrt() as f32;
                    out
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(frames: usize, cycles: f32, amplitude: f32) -> Vec<f32> {
        (0..frames)
            .map(|i| amplitude * (i as f32 / frames as f32 * cycles * std::f32::consts::TAU).sin())
            .collect()
    }

    #[test]
    fn the_pyramid_folds_down_to_a_handful_of_bins() {
        let frames = 48_000usize;
        let peaks = Peaks::build(&sine(frames, 100.0, 0.5), 1, frames as u64, 48_000);
        assert_eq!(peaks.frames(), 48_000);
        assert!(peaks.depth() >= 4, "depth {}", peaks.depth());
        assert!((peaks.peak() - 0.5).abs() < 0.01, "peak {}", peaks.peak());
        assert!((peaks.seconds() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn columns_cover_the_span_and_read_the_signal() {
        let frames = 48_000usize;
        // Two thousand cycles in the file: every column of two hundred
        // holds ten whole cycles, so each reaches both extremes.
        let peaks = Peaks::build(&sine(frames, 2_000.0, 0.8), 1, frames as u64, 48_000);
        let columns = peaks.columns(None, 0.0, 1.0, 200);
        assert_eq!(columns.len(), 200);
        for bin in &columns {
            assert!(bin.max > 0.7 && bin.min < -0.7, "extremes {bin:?}");
            assert!((bin.rms - 0.8 / 2f32.sqrt()).abs() < 0.05, "rms {bin:?}");
        }
        let silence = Peaks::build(&vec![0.0; frames], 1, frames as u64, 48_000);
        assert!(
            silence
                .columns(None, 0.0, 1.0, 50)
                .iter()
                .all(|bin| *bin == Bin::default())
        );
    }

    #[test]
    fn a_close_view_is_cut_from_the_samples() {
        let frames = 4_096usize;
        let samples = sine(frames, 4.0, 1.0);
        let peaks = Peaks::build(&samples, 1, frames as u64, 48_000);
        // A quarter of one cycle from the top: the samples rise, so the
        // columns' maxima rise column by column.
        let columns = peaks.columns(Some(&samples), 0.0, 0.0625, 16);
        assert_eq!(columns.len(), 16);
        assert!(
            columns.windows(2).all(|w| w[1].max >= w[0].max),
            "the fine columns did not follow the samples"
        );
    }

    #[test]
    fn channels_fold_into_one_picture() {
        let frames = 2_048usize;
        let left = sine(frames, 2.0, 0.2);
        let right = sine(frames, 2.0, 0.9);
        let planar: Vec<f32> = left.iter().chain(right.iter()).copied().collect();
        let peaks = Peaks::build(&planar, 2, frames as u64, 48_000);
        assert!((peaks.peak() - 0.9).abs() < 0.01);
        let column = peaks.columns(None, 0.0, 1.0, 1)[0];
        assert!(column.max > 0.85 && column.min < -0.85);
    }
}
