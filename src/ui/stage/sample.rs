//! The cutting room: the sample editor's state, and everything about it
//! that has nothing to do with a painter.
//!
//! An Octatrack's audio editor, in the deck's hand: one file, full
//! screen, the waveform the hero, and three pages that change which
//! keys are in reach — TRIM, SLICE, ATTR — never what is on the
//! picture. Every act is a key. Plan and reasoning:
//! `notes/20260902-sample-editor-plan.md`.
//!
//! What lives here: the editor's state (which device, which page, where
//! the cursor and the view are), the file as the stage is allowed to
//! hold it, the pure motions (step, zoom, scroll, snap), and the
//! requests the host answers (load this file, play this range). The
//! edits themselves land on the device through `mod.rs`, which owns the
//! song; the picture is `mod.rs` too, because it owns the ink.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::sample_peaks::Peaks;
use crate::sequencing::{Device, DeviceId};
use crate::slice::Planar;

/// Which page of the editor is up.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Page {
    Trim,
    Slice,
    Attr,
}

impl Page {
    pub(super) const ALL: [Page; 3] = [Page::Trim, Page::Slice, Page::Attr];

    pub(super) fn next(self) -> Page {
        match self {
            Page::Trim => Page::Slice,
            Page::Slice => Page::Attr,
            Page::Attr => Page::Trim,
        }
    }

    pub(super) fn word(self) -> &'static str {
        match self {
            Page::Trim => "TRIM",
            Page::Slice => "SLICE",
            Page::Attr => "ATTR",
        }
    }
}

/// A marker the hand can take hold of: the trim's two ends, the loop's
/// start, or one slice by its index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Marker {
    Start,
    End,
    Loop,
    Slice(usize),
}

impl Marker {
    pub(super) fn word(self) -> String {
        match self {
            Self::Start => "in".to_owned(),
            Self::End => "out".to_owned(),
            Self::Loop => "loop".to_owned(),
            Self::Slice(index) => format!("slice {:02}", index + 1),
        }
    }
}

/// A range being auditioned, and when it began. The stage never hears
/// the engine, so the head is reckoned from the clock against the
/// file's length: near enough for the eye, and no seam crossed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Playing {
    pub(super) from: f64,
    pub(super) to: f64,
    pub(super) since: Instant,
}

impl Playing {
    /// Where the head stands now, as a fraction of the file, or `None`
    /// once the range has run out.
    pub(super) fn head(&self, seconds: f64) -> Option<f64> {
        let elapsed = self.since.elapsed().as_secs_f64();
        self.head_after(elapsed, seconds)
    }

    pub(super) fn head_after(&self, elapsed: f64, seconds: f64) -> Option<f64> {
        if !(seconds > 0.0) || self.to <= self.from {
            return None;
        }
        let at = self.from + elapsed / seconds;
        (at <= self.to).then_some(at)
    }
}

/// The editor, while it is up.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SampleEditor {
    /// The track whose head is the sampler.
    pub(super) track: usize,
    /// The sampler being edited. By id, so a chain edited underneath
    /// cannot make the editor point at a different device.
    pub(super) device: DeviceId,
    pub(super) page: Page,
    /// The cursor, as a fraction of the file.
    pub(super) cursor: f64,
    /// The view: where it starts and how much of the file it shows, as
    /// fractions. A span of one is the whole file.
    pub(super) view_from: f64,
    pub(super) view_span: f64,
    /// How many slices G lays.
    pub(super) count: usize,
    /// How eager T is, `0..=1`.
    pub(super) sensitivity: f32,
    /// Whether placed markers walk back to the nearest zero crossing.
    pub(super) snap: bool,
    /// The range being auditioned, while one is: the head is drawn over it.
    pub(super) playing: Option<Playing>,
    /// The marker in hand, if one is: the arrows carry it with the cursor.
    pub(super) grabbed: Option<Marker>,
    /// The kit pad this room is open on, when it is a pad and not a
    /// sampler; `stand_in` is then the sampler standing for it.
    pub(super) pad: Option<usize>,
    pub(super) stand_in: Option<crate::sequencing::Device>,
}

/// The least of the file a view may show: sixty-four frames' worth is
/// where the pyramid hands over to the samples, and a view narrower
/// than a few hundred frames is a view of nothing.
const MIN_SPAN_FRAMES: f64 = 256.0;
/// One zoom step multiplies or divides the span by this.
const ZOOM_STEP: f64 = 1.6;
/// A fine cursor step is this share of the view; a coarse one, eight
/// times it.
const STEP_SHARE: f64 = 1.0 / 64.0;
const COARSE: f64 = 8.0;
/// A scroll moves the view by this share of itself.
const SCROLL_SHARE: f64 = 0.5;
/// A fitted view leaves this share of the range as air on each side.
const FIT_AIR: f64 = 0.08;

impl SampleEditor {
    pub(super) fn open(track: usize, device: DeviceId, cursor: f64) -> Self {
        Self {
            track,
            device,
            page: Page::Trim,
            cursor: cursor.clamp(0.0, 1.0),
            view_from: 0.0,
            view_span: 1.0,
            count: 16,
            sensitivity: 0.5,
            snap: true,
            playing: None,
            grabbed: None,
            pad: None,
            stand_in: None,
        }
    }

    /// Start auditioning a range now.
    pub(super) fn play(&mut self, from: f64, to: f64) {
        self.playing = Some(Playing {
            from,
            to,
            since: Instant::now(),
        });
    }

    /// Forget an audition that has run its course.
    pub(super) fn settle(&mut self, seconds: f64) {
        if self
            .playing
            .is_some_and(|playing| playing.head(seconds).is_none())
        {
            self.playing = None;
        }
    }

    /// Show exactly `a..b`, with a little air either side, never
    /// narrower than the samples allow.
    pub(super) fn fit(&mut self, a: f64, b: f64, frames: u64) -> bool {
        let (a, b) = (a.min(b).clamp(0.0, 1.0), a.max(b).clamp(0.0, 1.0));
        let min_span = if frames == 0 {
            1.0
        } else {
            (MIN_SPAN_FRAMES / frames as f64).min(1.0)
        };
        let air = (b - a) * FIT_AIR;
        let span = ((b - a) + 2.0 * air).clamp(min_span, 1.0);
        let from = (a - air).clamp(0.0, 1.0 - span);
        if (span - self.view_span).abs() < 1e-12 && (from - self.view_from).abs() < 1e-12 {
            return false;
        }
        self.view_span = span;
        self.view_from = from;
        self.cursor = self.cursor.clamp(self.view_from, self.view_to());
        true
    }

    /// The whole file in view.
    pub(super) fn whole(&mut self) -> bool {
        if self.view_span >= 1.0 {
            return false;
        }
        self.view_span = 1.0;
        self.view_from = 0.0;
        true
    }

    pub(super) fn view_to(&self) -> f64 {
        (self.view_from + self.view_span).min(1.0)
    }

    /// Move the cursor by a share of the view, and keep it in view.
    pub(super) fn step(&mut self, right: bool, coarse: bool) -> bool {
        let share = STEP_SHARE * if coarse { COARSE } else { 1.0 };
        let delta = self.view_span * share * if right { 1.0 } else { -1.0 };
        let next = (self.cursor + delta).clamp(0.0, 1.0);
        if next == self.cursor {
            return false;
        }
        self.cursor = next;
        self.follow();
        true
    }

    /// Put the cursor somewhere, and keep it in view.
    pub(super) fn seek(&mut self, to: f64) {
        self.cursor = to.clamp(0.0, 1.0);
        self.follow();
    }

    /// Zoom about the cursor: the cursor keeps its place on screen.
    pub(super) fn zoom(&mut self, closer: bool, frames: u64) -> bool {
        let min_span = if frames == 0 {
            1.0
        } else {
            (MIN_SPAN_FRAMES / frames as f64).min(1.0)
        };
        let next = if closer {
            (self.view_span / ZOOM_STEP).max(min_span)
        } else {
            (self.view_span * ZOOM_STEP).min(1.0)
        };
        if (next - self.view_span).abs() < 1e-12 {
            return false;
        }
        let anchor = if self.view_span > 0.0 {
            ((self.cursor - self.view_from) / self.view_span).clamp(0.0, 1.0)
        } else {
            0.5
        };
        self.view_span = next;
        self.view_from = (self.cursor - anchor * next).clamp(0.0, 1.0 - next);
        true
    }

    /// Scroll the view by half of itself.
    pub(super) fn scroll(&mut self, right: bool) -> bool {
        let delta = self.view_span * SCROLL_SHARE * if right { 1.0 } else { -1.0 };
        let next = (self.view_from + delta).clamp(0.0, 1.0 - self.view_span);
        if (next - self.view_from).abs() < 1e-12 {
            return false;
        }
        self.view_from = next;
        // The cursor rides along if it would otherwise leave the view.
        self.cursor = self.cursor.clamp(self.view_from, self.view_to());
        true
    }

    /// Keep the cursor inside the view, moving the view as little as
    /// it takes.
    fn follow(&mut self) {
        if self.cursor < self.view_from {
            self.view_from = self.cursor;
        } else if self.cursor > self.view_to() {
            self.view_from = (self.cursor - self.view_span).max(0.0);
        }
        self.view_from = self.view_from.clamp(0.0, 1.0 - self.view_span);
    }

    /// The slice the cursor stands in, if the device has slices.
    pub(super) fn slice_at(&self, device: &Device) -> Option<usize> {
        if device.slices.is_empty() {
            return None;
        }
        Some(
            device
                .slices
                .iter()
                .rposition(|at| *at <= self.cursor + 1e-12)
                .unwrap_or(0),
        )
    }

    /// The slice's own range: from its start to the next start, or to
    /// the end of the file.
    pub(super) fn slice_bounds(device: &Device, index: usize) -> Option<(f64, f64)> {
        let from = *device.slices.get(index)?;
        let to = device.slices.get(index + 1).copied().unwrap_or(1.0);
        Some((from, to))
    }

    /// The marker nearest the cursor, of every kind there is.
    pub(super) fn nearest_marker(&self, device: &Device) -> Option<Marker> {
        use crate::params::sampler as sp;
        let mut candidates: Vec<(Marker, f64)> = vec![
            (Marker::Start, f64::from(device.value(sp::START))),
            (Marker::End, f64::from(device.value(sp::END))),
        ];
        if device.value(sp::LOOP_MODE).round() >= 1.0 {
            candidates.push((Marker::Loop, loop_at(device)));
        }
        candidates.extend(
            device
                .slices
                .iter()
                .enumerate()
                .map(|(index, at)| (Marker::Slice(index), *at)),
        );
        candidates
            .into_iter()
            .filter(|(_, at)| at.is_finite())
            .min_by(|(_, a), (_, b)| {
                (*a - self.cursor)
                    .abs()
                    .total_cmp(&(*b - self.cursor).abs())
            })
            .map(|(marker, _)| marker)
    }

    /// The nearest slice to the cursor, for removing.
    pub(super) fn nearest_slice(&self, device: &Device) -> Option<usize> {
        device
            .slices
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (*a - self.cursor)
                    .abs()
                    .total_cmp(&(*b - self.cursor).abs())
            })
            .map(|(index, _)| index)
    }
}

/// Where the loop starts, as a fraction of the FILE. The sampler keeps
/// `LOOP_START` as a fraction of the trim — the brief's choice, so a
/// loop survives a re-trim — and the picture is drawn in file fractions,
/// so the two meet here and in [`loop_of`], nowhere else.
pub(super) fn loop_at(device: &Device) -> f64 {
    use crate::params::sampler as sp;
    let start = f64::from(device.value(sp::START));
    let end = f64::from(device.value(sp::END));
    let share = f64::from(device.value(sp::LOOP_START)).clamp(0.0, 1.0);
    (start + (end - start).max(0.0) * share).clamp(0.0, 1.0)
}

/// The `LOOP_START` value that puts the loop at file fraction `at`.
pub(super) fn loop_of(device: &Device, at: f64) -> f32 {
    use crate::params::sampler as sp;
    let start = f64::from(device.value(sp::START));
    let end = f64::from(device.value(sp::END));
    let len = end - start;
    if len <= 0.0 {
        return 0.0;
    }
    ((at - start) / len).clamp(0.0, 1.0) as f32
}

/// Every marker a jump can land on: the trim, the loop start, and the
/// slices, sorted. From the device, so a jump is always to something
/// real.
pub(super) fn markers(device: &Device) -> Vec<f64> {
    use crate::params::sampler as sp;
    let mut out: Vec<f64> = vec![
        f64::from(device.value(sp::START)),
        f64::from(device.value(sp::END)),
        loop_at(device),
    ];
    out.extend(device.slices.iter().copied());
    out.retain(|m| m.is_finite());
    out.sort_by(f64::total_cmp);
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    out
}

/// The next marker past `from` in the direction asked, if any.
pub(super) fn jump(markers: &[f64], from: f64, forward: bool) -> Option<f64> {
    if forward {
        markers.iter().copied().find(|m| *m > from + 1e-9)
    } else {
        markers.iter().rev().copied().find(|m| *m < from - 1e-9)
    }
}

/// The file, as the stage may hold it: the same samples the engine
/// plays, through the material's own `Arc`, and the pyramid drawn from
/// them. Built by the host; the stage never opens a file itself.
#[derive(Clone, Debug)]
pub struct SampleData {
    pub path: PathBuf,
    pub samples: Arc<Vec<f32>>,
    pub channels: usize,
    pub frames: u64,
    pub sample_rate: u32,
    pub peaks: Arc<Peaks>,
}

impl SampleData {
    /// Build from planar samples. The host calls this on a loaded
    /// material; the shot harness calls it on a synthesized one.
    pub fn from_planar(
        path: PathBuf,
        samples: Arc<Vec<f32>>,
        channels: usize,
        frames: u64,
        sample_rate: u32,
    ) -> Self {
        let peaks = Arc::new(Peaks::build(&samples, channels, frames, sample_rate));
        Self {
            path,
            samples,
            channels,
            frames,
            sample_rate,
            peaks,
        }
    }

    pub fn planar(&self) -> Planar<'_> {
        Planar {
            samples: &self.samples,
            channels: self.channels,
            frames: self.frames,
            sample_rate: self.sample_rate,
        }
    }

    /// `at` walked back to the nearest zero crossing, as a fraction.
    pub fn snapped(&self, at: f64) -> f64 {
        if self.frames == 0 {
            return at;
        }
        let frame = (at.clamp(0.0, 1.0) * self.frames as f64).round() as u64;
        let zero = crate::slice::walk_back_to_zero(&self.planar(), frame, self.sample_rate as f32);
        zero as f64 / self.frames as f64
    }

    pub fn seconds(&self) -> f64 {
        self.peaks.seconds()
    }

    /// The gain, in decibels, that brings the file's peak to full scale.
    pub fn normalizing_db(&self) -> f32 {
        let peak = self.peaks.peak();
        if peak <= 0.0 {
            0.0
        } else {
            -20.0 * peak.log10()
        }
    }
}

/// A range the host is asked to play through the engine's audition
/// path, as fractions of the file.
#[derive(Clone, Debug, PartialEq)]
pub struct Audition {
    pub path: PathBuf,
    pub from: f64,
    pub to: f64,
}

impl Audition {
    pub fn of(path: &Path, from: f64, to: f64) -> Self {
        Self {
            path: path.to_path_buf(),
            from: from.clamp(0.0, 1.0),
            to: to.clamp(0.0, 1.0),
        }
    }
}

/// A word for a fraction of a file, in seconds or milliseconds, for
/// the readouts.
pub(super) fn time_word(fraction: f64, seconds: f64) -> String {
    let at = fraction.clamp(0.0, 1.0) * seconds;
    if at < 1.0 {
        format!("{:.0}ms", at * 1000.0)
    } else if at < 60.0 {
        format!("{at:.2}s")
    } else {
        format!("{}:{:05.2}", (at / 60.0).floor() as u32, at % 60.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> SampleEditor {
        SampleEditor::open(0, DeviceId(1), 0.5)
    }

    #[test]
    fn a_step_is_a_share_of_the_view_and_a_coarse_one_eight_of_them() {
        let mut e = editor();
        assert!(e.step(true, false));
        assert!((e.cursor - (0.5 + STEP_SHARE)).abs() < 1e-12);
        assert!(e.step(false, true));
        assert!((e.cursor - (0.5 + STEP_SHARE - STEP_SHARE * COARSE)).abs() < 1e-12);
        e.cursor = 1.0;
        assert!(!e.step(true, false), "stepped past the end");
    }

    #[test]
    fn zooming_keeps_the_cursor_where_it_is_on_screen() {
        let mut e = editor();
        e.cursor = 0.25;
        assert!(e.zoom(true, 48_000));
        let on_screen = (e.cursor - e.view_from) / e.view_span;
        assert!(
            (on_screen - 0.25).abs() < 1e-9,
            "the cursor moved to {on_screen}"
        );
        assert!(e.view_span < 1.0);
        for _ in 0..40 {
            e.zoom(true, 48_000);
        }
        assert!(
            e.view_span >= MIN_SPAN_FRAMES / 48_000.0 - 1e-12,
            "zoomed past the samples"
        );
        for _ in 0..40 {
            e.zoom(false, 48_000);
        }
        assert_eq!(e.view_span, 1.0);
        assert_eq!(e.view_from, 0.0);
    }

    #[test]
    fn the_view_follows_the_cursor_and_scrolls_by_halves() {
        let mut e = editor();
        e.zoom(true, 48_000);
        e.zoom(true, 48_000);
        let span = e.view_span;
        e.seek(0.0);
        assert_eq!(e.view_from, 0.0);
        e.seek(1.0);
        assert!(
            (e.view_to() - 1.0).abs() < 1e-12,
            "the view did not reach the cursor"
        );
        assert!(e.scroll(false));
        assert!((e.view_from - (1.0 - span - span * SCROLL_SHARE)).abs() < 1e-9);
        assert!(
            e.cursor <= e.view_to() && e.cursor >= e.view_from,
            "the cursor left the view"
        );
        e.seek(0.0);
        assert!(!e.scroll(false), "scrolled past the start");
    }

    #[test]
    fn a_fit_shows_the_range_with_air_and_whole_shows_it_all() {
        let mut e = editor();
        assert!(e.fit(0.25, 0.5, 48_000));
        assert!(
            e.view_from < 0.25 && e.view_to() > 0.5,
            "the range is not in view"
        );
        assert!(e.view_span < 0.5, "the fit is loose");
        assert!(!e.fit(0.25, 0.5, 48_000), "fitting twice changed something");
        assert!(e.whole());
        assert_eq!((e.view_from, e.view_span), (0.0, 1.0));
        assert!(!e.whole());
        // A range too narrow for the samples is shown as wide as it can be.
        let mut e = editor();
        assert!(e.fit(0.5, 0.5 + 1e-9, 48_000));
        assert!(e.view_span >= MIN_SPAN_FRAMES / 48_000.0 - 1e-12);
    }

    #[test]
    fn the_nearest_marker_is_found_of_every_kind_and_the_loop_lives_in_the_trim() {
        use crate::params::sampler as sp;
        let mut device = Device::new(DeviceId(1), crate::devices::DeviceKind::Sampler);
        device.set(sp::START, 0.2);
        device.set(sp::END, 0.8);
        device.set(sp::LOOP_START, 0.5);
        device.set(sp::LOOP_MODE, sp::LOOP_FORWARD);
        device.set_slices([0.3, 0.7]);
        assert!(
            (loop_at(&device) - 0.5).abs() < 1e-6,
            "{}",
            loop_at(&device)
        );
        assert!((loop_of(&device, 0.65) - 0.75).abs() < 1e-6);
        let mut e = editor();
        e.cursor = 0.21;
        assert_eq!(e.nearest_marker(&device), Some(Marker::Start));
        e.cursor = 0.52;
        assert_eq!(e.nearest_marker(&device), Some(Marker::Loop));
        // The table always begins at the head: the cuts are one and two.
        e.cursor = 0.69;
        assert_eq!(e.nearest_marker(&device), Some(Marker::Slice(2)));
        device.set(sp::LOOP_MODE, sp::LOOP_OFF);
        e.cursor = 0.52;
        assert_ne!(
            e.nearest_marker(&device),
            Some(Marker::Loop),
            "a loop that is off is not something to hold"
        );
        assert_eq!(SampleEditor::slice_bounds(&device, 1), Some((0.3, 0.7)));
        assert_eq!(SampleEditor::slice_bounds(&device, 2), Some((0.7, 1.0)));
        assert_eq!(SampleEditor::slice_bounds(&device, 3), None);
    }

    #[test]
    fn the_head_walks_the_range_by_the_clock_and_then_is_gone() {
        let playing = Playing {
            from: 0.25,
            to: 0.75,
            since: Instant::now(),
        };
        // A two-second file: a quarter of it is half a second.
        assert!((playing.head_after(0.5, 2.0).unwrap() - 0.5).abs() < 1e-12);
        assert_eq!(playing.head_after(1.5, 2.0), None, "played past the end");
        assert_eq!(playing.head_after(0.5, 0.0), None, "a file with no length");
        let mut e = editor();
        e.play(0.0, 1.0);
        e.settle(1_000.0);
        assert!(e.playing.is_some(), "settled an audition still sounding");
        e.playing = Some(Playing {
            from: 0.0,
            to: 0.0,
            since: Instant::now(),
        });
        e.settle(1.0);
        assert_eq!(e.playing, None);
    }

    #[test]
    fn jumps_land_on_markers_in_order() {
        let mut device = Device::new(DeviceId(1), crate::devices::DeviceKind::Sampler);
        device.set_slices([0.2, 0.6]);
        let m = markers(&device);
        assert!(m.contains(&0.0) && m.contains(&0.2) && m.contains(&0.6) && m.contains(&1.0));
        assert_eq!(jump(&m, 0.3, true), Some(0.6));
        assert_eq!(jump(&m, 0.3, false), Some(0.2));
        assert_eq!(jump(&m, 1.0, true), None);
        let e = SampleEditor::open(0, DeviceId(1), 0.3);
        assert_eq!(e.slice_at(&device), Some(1));
        assert_eq!(e.nearest_slice(&device), Some(1));
    }

    #[test]
    fn a_file_snaps_to_zero_and_knows_its_normalizing_gain() {
        let frames = 4_800usize;
        let samples: Vec<f32> = (0..frames)
            .map(|i| 0.5 * (i as f32 / 48.0 * std::f32::consts::TAU).sin())
            .collect();
        let data = SampleData::from_planar(
            PathBuf::from("test.wav"),
            Arc::new(samples),
            1,
            frames as u64,
            48_000,
        );
        assert!(
            (data.normalizing_db() - 6.02).abs() < 0.1,
            "{}",
            data.normalizing_db()
        );
        // Just past a crossing at frame 2400 (every 24 frames is one).
        let at = 2_405.0 / frames as f64;
        let snapped = data.snapped(at);
        let frame = (snapped * frames as f64).round() as usize;
        assert_eq!(frame % 24, 0, "snapped to frame {frame}");
        assert!(frame <= 2_405);
        assert_eq!(time_word(0.5, 0.1), "50ms");
        assert_eq!(time_word(0.5, 3.0), "1.50s");
        assert_eq!(time_word(1.0, 90.0), "1:30.00");
    }
}
