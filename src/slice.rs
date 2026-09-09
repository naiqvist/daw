//! Cutting a file into slices — the grid, and the onset detector.
//!
//! GREEN ZONE. Both functions allocate, both read every sample of the
//! material, and neither goes anywhere near the callback: the sampler's
//! slice table is baked at compile from what these return, exactly as a
//! clip's gain envelope is.
//!
//! # Why it reads the samples rather than the peak pyramid
//!
//! `waveform::Peaks` is a lovely envelope and it is the wrong tool: its
//! finest level is a bin of many frames, and the last thing an onset
//! detector does is walk back to a zero crossing, which needs actual
//! samples. The sampler already has the whole file in RAM, so reading it
//! costs nothing that has not been paid for.
//!
//! # The detector, in one paragraph
//!
//! Take the RMS of each short window. Convert to dB. Where the level
//! rises by more than a threshold from one window to the next, and the
//! window is above a noise floor, and it has been at least a minimum
//! spacing since the last one — that is an onset. Then walk BACKWARDS to
//! the nearest zero crossing, because a slice that starts mid-cycle
//! clicks and the whole point of detecting the onset was to avoid one.
//!
//! It is not clever. It does not need to be: the markers it produces are
//! ordinary editable data from the moment they exist, so being roughly
//! right and draggable beats being ingeniously right and fixed.

use crate::audio::material::Material;

/// A file as the detector reads it: planar samples, channel `c` at
/// `samples[c * frames .. (c + 1) * frames]`. A view rather than the
/// material itself, so a surface that holds the same samples through
/// an `Arc` — and may not name the audio module — can run the detector
/// on them too.
#[derive(Clone, Copy, Debug)]
pub struct Planar<'a> {
    pub samples: &'a [f32],
    pub channels: usize,
    pub frames: u64,
    pub sample_rate: u32,
}

impl<'a> Planar<'a> {
    pub fn channel(&self, channel: usize) -> &'a [f32] {
        let frames = self.frames as usize;
        let from = channel * frames;
        self.samples.get(from..from + frames).unwrap_or(&[])
    }

    pub fn is_empty(&self) -> bool {
        self.frames == 0 || self.channels == 0 || self.samples.is_empty()
    }
}

impl<'a> From<&'a Material> for Planar<'a> {
    fn from(material: &'a Material) -> Self {
        Self {
            samples: &material.samples,
            channels: material.channels,
            frames: material.frames,
            sample_rate: material.sample_rate,
        }
    }
}

/// Analysis window, in milliseconds. Five is short enough to place a
/// drum hit inside a sixteenth at any sane tempo and long enough that one
/// cycle of a bass note does not read as a transient.
pub const WINDOW_MS: f32 = 5.0;

/// The closest two onsets may be, in milliseconds. Fifty is a 32nd note
/// at 150 bpm: fast enough for a drum roll, slow enough that one snare
/// hit is not four.
pub const MIN_SPACING_MS: f32 = 50.0;

/// How far back a marker will walk looking for a zero crossing, in
/// milliseconds. Past this it gives up and stays where it was rather than
/// wandering into the previous hit.
pub const ZERO_SEARCH_MS: f32 = 5.0;

/// Below this, a window is silence and cannot be an onset however much it
/// rose — an onset detector without a floor finds hundreds of them in the
/// noise between hits.
pub const FLOOR_DB: f32 = -60.0;

/// The most markers either function will return.
pub const MAX: usize = crate::audio::sampler::MAX_SLICES;

/// Equal divisions of `frames`, starting at zero.
///
/// Tiles the span exactly: marker `i` is `frames * i / count`, computed
/// in integers so the divisions cannot drift and the last slice gets the
/// remainder rather than a gap.
pub fn grid(frames: u64, count: usize) -> Vec<u64> {
    let count = count.clamp(1, MAX);
    (0..count)
        .map(|i| frames.saturating_mul(i as u64) / count as u64)
        .collect()
}

/// Onsets in `material`, as source frames, sorted and starting with zero.
///
/// `sensitivity` is `0..=1`: at zero only an obvious hit counts, at one
/// almost any rise does.
pub fn transients(material: &Material, sensitivity: f32) -> Vec<u64> {
    transients_of(&Planar::from(material), sensitivity)
}

/// `transients`, over a planar view of any samples.
pub fn transients_of(material: &Planar<'_>, sensitivity: f32) -> Vec<u64> {
    transients_of_spaced(material, sensitivity, MIN_SPACING_MS)
}

/// Same detector with an authored minimum distance between hits.
pub fn transients_of_spaced(material: &Planar<'_>, sensitivity: f32, gap_ms: f32) -> Vec<u64> {
    let mut out = vec![0u64];
    if material.is_empty() || material.sample_rate == 0 {
        return out;
    }
    let sr = material.sample_rate as f32;
    let window = ((sr * WINDOW_MS / 1_000.0) as usize).max(16);
    let gap = if gap_ms.is_finite() {
        gap_ms.clamp(10.0, 500.0)
    } else {
        MIN_SPACING_MS
    };
    let spacing = ((sr * gap / 1_000.0) as u64).max(1);
    let frames = material.frames as usize;
    if frames < window * 2 {
        return out;
    }

    // A rise of this many dB from one window to the next is an onset.
    // Twelve at the shy end, three at the eager one.
    let sensitivity = sensitivity.clamp(0.0, 1.0);
    let rise_db = 12.0 - 9.0 * sensitivity;

    let windows = frames / window;
    let mut previous_db = f32::NEG_INFINITY;
    let mut last_marker = 0u64;

    for w in 0..windows {
        let from = w * window;
        let to = (from + window).min(frames);
        let db = window_db(material, from, to);
        let rose = db - previous_db;
        previous_db = db;

        if w == 0 || db < FLOOR_DB {
            continue;
        }
        if rose < rise_db {
            continue;
        }
        let at = from as u64;
        if at.saturating_sub(last_marker) < spacing {
            continue;
        }
        let refined = walk_back_to_zero(material, at, sr);
        if refined.saturating_sub(last_marker) < spacing && !out.is_empty() && out.len() > 1 {
            continue;
        }
        if out.last().copied() != Some(refined) {
            out.push(refined);
            last_marker = refined;
        }
        if out.len() >= MAX {
            break;
        }
    }
    out
}

/// RMS of one window across every channel, in dB.
fn window_db(material: &Planar<'_>, from: usize, to: usize) -> f32 {
    let mut energy = 0.0f64;
    let mut counted = 0usize;
    for c in 0..material.channels {
        let channel = material.channel(c);
        let Some(span) = channel.get(from..to) else {
            continue;
        };
        for s in span {
            energy += f64::from(*s) * f64::from(*s);
        }
        counted += span.len();
    }
    if counted == 0 {
        return f32::NEG_INFINITY;
    }
    let rms = (energy / counted as f64).sqrt() as f32;
    if rms > 0.0 {
        20.0 * rms.log10()
    } else {
        f32::NEG_INFINITY
    }
}

/// The nearest zero crossing at or before `at`, within the search window.
///
/// Backwards rather than forwards: a marker that moves EARLIER keeps the
/// attack it was pointing at, and one that moves later eats it.
pub fn walk_back_to_zero(material: &Planar<'_>, at: u64, sample_rate: f32) -> u64 {
    let channel = material.channel(0);
    if channel.is_empty() {
        return at;
    }
    let reach = ((sample_rate * ZERO_SEARCH_MS / 1_000.0) as u64).max(1);
    let floor = at.saturating_sub(reach);
    let mut i = at.min(channel.len() as u64 - 1);
    while i > floor {
        let (Some(a), Some(b)) = (
            channel.get(i as usize - 1).copied(),
            channel.get(i as usize).copied(),
        ) else {
            break;
        };
        if (a <= 0.0 && b >= 0.0) || (a >= 0.0 && b <= 0.0) {
            return i;
        }
        i -= 1;
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn mono(samples: Vec<f32>, rate: u32) -> Material {
        let frames = samples.len() as u64;
        Material {
            samples: Arc::new(samples),
            channels: 1,
            frames,
            source: std::path::PathBuf::from("test"),
            sample_rate: rate,
            original_rate: rate,
            truncated: false,
        }
    }

    /// A grid tiles its span EXACTLY: no gap, no overlap, and the last
    /// slice takes the remainder.
    #[test]
    fn a_grid_tiles_its_span_exactly() {
        for (frames, count) in [(1_600u64, 4usize), (1_000, 16), (999, 7), (1, 1), (13, 64)] {
            let g = grid(frames, count);
            assert_eq!(g.len(), count.min(MAX).max(1), "{frames}/{count}: count");
            assert_eq!(g[0], 0, "{frames}/{count}: starts at zero");
            for pair in g.windows(2) {
                assert!(pair[1] >= pair[0], "{frames}/{count}: went backwards");
            }
            assert!(
                g.last().copied().unwrap_or(0) < frames.max(1),
                "{frames}/{count}: a marker landed past the end"
            );
        }
    }

    /// Nonsense counts clamp instead of panicking or allocating a
    /// gigabyte.
    #[test]
    fn a_nonsense_grid_clamps() {
        assert_eq!(grid(1_000, 0).len(), 1);
        assert_eq!(grid(1_000, 10_000).len(), MAX);
        assert_eq!(grid(0, 4), vec![0, 0, 0, 0]);
    }

    /// The detector against a synthetic click train: four hits, evenly
    /// spaced, and it should find four markers near them.
    #[test]
    fn the_detector_finds_a_click_train() {
        let rate = 48_000u32;
        let hits = 4usize;
        let gap = 12_000usize; // 250 ms
        let mut samples = vec![0.0f32; hits * gap];
        for h in 0..hits {
            let at = h * gap;
            // A short decaying burst — a drum hit's shape, roughly.
            for i in 0..2_000 {
                let t = i as f32 / rate as f32;
                let env = (-t * 60.0).exp();
                if let Some(s) = samples.get_mut(at + i) {
                    *s = (std::f32::consts::TAU * 180.0 * t).sin() * env * 0.8;
                }
            }
        }
        let m = mono(samples, rate);
        let found = transients(&m, 0.5);
        assert_eq!(found.len(), hits, "found {found:?}");
        for (h, at) in found.iter().enumerate() {
            let want = (h * gap) as i64;
            let drift = (*at as i64 - want).abs();
            // Within 5 ms of the true onset: one analysis window either
            // way, plus the walk back to zero.
            assert!(
                drift <= 480,
                "hit {h} landed at {at}, which is {drift} frames from {want}"
            );
        }
    }

    /// Silence has no onsets except the one at the top, which is always
    /// there because a slice list starts at the beginning of the file.
    #[test]
    fn silence_has_one_marker() {
        let m = mono(vec![0.0; 48_000], 48_000);
        assert_eq!(transients(&m, 1.0), vec![0]);
    }

    /// Degenerate material is answered rather than panicked at.
    #[test]
    fn tiny_material_is_survivable() {
        for frames in [0usize, 1, 2, 100] {
            let m = if frames == 0 {
                Material::empty()
            } else {
                mono(vec![0.5; frames], 48_000)
            };
            let found = transients(&m, 0.5);
            assert!(!found.is_empty(), "{frames} frames returned nothing");
            assert_eq!(found[0], 0);
        }
    }

    /// Sensitivity does something monotone: eager finds at least as many
    /// as shy.
    #[test]
    fn sensitivity_is_monotone() {
        let rate = 48_000u32;
        let mut samples = vec![0.0f32; 48_000];
        // Six hits at wildly different levels, so the threshold has
        // something to discriminate with.
        for h in 0..6usize {
            let at = h * 8_000;
            let level = 0.9 / (h + 1) as f32;
            for i in 0..1_500 {
                let t = i as f32 / rate as f32;
                if let Some(s) = samples.get_mut(at + i) {
                    *s = (std::f32::consts::TAU * 200.0 * t).sin() * (-t * 80.0).exp() * level;
                }
            }
        }
        let m = mono(samples, rate);
        let shy = transients(&m, 0.0).len();
        let eager = transients(&m, 1.0).len();
        assert!(eager >= shy, "eager found {eager}, shy found {shy}");
    }

    /// A marker lands on a zero crossing, which is the difference between
    /// a slice and a click.
    #[test]
    fn markers_land_on_zero_crossings() {
        let rate = 48_000u32;
        let mut samples = vec![0.0f32; 24_000];
        for i in 0..4_000 {
            let t = i as f32 / rate as f32;
            if let Some(s) = samples.get_mut(12_000 + i) {
                *s = (std::f32::consts::TAU * 300.0 * t).sin() * (-t * 40.0).exp() * 0.9;
            }
        }
        let m = mono(samples, rate);
        let found = transients(&m, 0.5);
        for at in found.iter().skip(1) {
            let i = *at as usize;
            let a = m.channel(0)[i.saturating_sub(1)];
            let b = m.channel(0)[i];
            assert!(
                (a <= 0.0 && b >= 0.0) || (a >= 0.0 && b <= 0.0) || a.abs() < 1e-3,
                "marker at {at} sits at {a} -> {b}, which is not a crossing"
            );
        }
    }
}
