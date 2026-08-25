//! Audio clip waveform analysis and the bottom-region waveform editor.
//!
//! Analysis is green-zone work. A background worker reads the same WAV the
//! audio node streams and reduces it into immutable min/max levels. The UI
//! therefore draws work proportional to its pixel width, never to the file's
//! duration, and the audio callback never sees this module or its allocations.
//!
//! The editor is a view over [`Clip`], just like the piano roll: clip/source
//! data remains owned by the arrangement. The BPM grid is a reference only;
//! audio is not warped or time-stretched here.

use crate::{AudioSource, Clip, Focus, GRID_MIN_PX, claim};
use daw::ui::theme::Theme;
use daw::ui::tokens::{font, stroke};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One stored peak covers this many source frames at the finest level.
/// At 48 kHz stereo this costs about 11 MiB per hour, while still putting
/// several peaks under each screen pixel at ordinary editing zooms.
const BASE_FRAMES_PER_PEAK: u64 = 128;
/// Each coarser level merges this many peaks from the previous one.
const LEVEL_REDUCTION: usize = 4;

const HEADER_H: f32 = 30.0;
const CHANNEL_PAD: f32 = 8.0;
const LABEL_PAD: f32 = 6.0;
const PX_PER_BEAT_DEFAULT: f32 = 48.0;
const PX_PER_BEAT_MIN: f32 = 4.0;
const PX_PER_BEAT_MAX: f32 = 1_600.0;
const AMPLITUDE_MIN: f32 = 0.25;
const AMPLITUDE_MAX: f32 = 8.0;
const ZOOM_STEP: f32 = 1.2;
const MAX_GRID_LINES: usize = 8_192;

pub const DEFAULT_H: f32 = 300.0;
pub const H_RANGE: std::ops::RangeInclusive<f32> = 120.0..=900.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    pub min: f32,
    pub max: f32,
}

impl Peak {
    const SILENCE: Self = Self { min: 0.0, max: 0.0 };

    fn include(&mut self, other: Self) {
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
    }
}

#[derive(Debug, Clone)]
struct PeakLevel {
    frames_per_peak: u64,
    /// Channel-major peak arrays. Every channel has the same number of bins.
    channels: Vec<Vec<Peak>>,
}

/// Immutable multiresolution waveform analysis for one playback file.
#[derive(Debug, Clone)]
pub struct Peaks {
    sample_rate: u32,
    channels: u16,
    frames: u64,
    levels: Vec<PeakLevel>,
}

impl Peaks {
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> usize {
        usize::from(self.channels)
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Min/max for a source-frame interval. The selected level leaves at
    /// most a handful of bins to combine for one display pixel.
    pub fn extrema(&self, channel: usize, start: u64, end: u64) -> Option<Peak> {
        let start = start.min(self.frames);
        let end = end.min(self.frames);
        if channel >= self.channels() || start >= end {
            return None;
        }
        let span = end - start;
        let level = self
            .levels
            .iter()
            .rev()
            .find(|level| level.frames_per_peak <= span)
            .or_else(|| self.levels.first())?;
        let peaks = level.channels.get(channel)?;
        let first = (start / level.frames_per_peak) as usize;
        let last = ((end - 1) / level.frames_per_peak) as usize;
        let mut out: Option<Peak> = None;
        for peak in peaks.get(first..=last)? {
            match &mut out {
                Some(out) => out.include(*peak),
                None => out = Some(*peak),
            }
        }
        out
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WaveformError {
    #[error("cannot decode waveform {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: hound::Error,
    },
    #[error("unsupported waveform {path}: {reason}")]
    Unsupported { path: PathBuf, reason: String },
}

enum Command {
    Build(PathBuf),
}

pub struct LoadResult {
    pub path: PathBuf,
    pub result: Result<Arc<Peaks>, WaveformError>,
}

/// Background owner for waveform analysis. Requests and results are entirely
/// on the UI/control side; no handle from here enters an audio schedule.
pub struct Service {
    commands: crossbeam_channel::Sender<Command>,
    results: crossbeam_channel::Receiver<LoadResult>,
}

impl Service {
    pub fn start() -> Self {
        let (commands, command_rx) = crossbeam_channel::unbounded();
        let (result_tx, results) = crossbeam_channel::unbounded();
        std::thread::Builder::new()
            .name("waveform-analysis".to_owned())
            .spawn(move || {
                while let Ok(Command::Build(path)) = command_rx.recv() {
                    let result = build_peaks(&path).map(Arc::new);
                    if result_tx
                        .send(LoadResult {
                            path: path.clone(),
                            result,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            })
            .expect("waveform worker must start");
        Self { commands, results }
    }

    pub fn request(&self, path: PathBuf) {
        let _ = self.commands.send(Command::Build(path));
    }

    pub fn try_result(&self) -> Option<LoadResult> {
        self.results.try_recv().ok()
    }
}

fn build_peaks(path: &Path) -> Result<Peaks, WaveformError> {
    let mut reader = hound::WavReader::open(path).map_err(|source| WaveformError::Decode {
        path: path.to_path_buf(),
        source,
    })?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.sample_rate == 0 {
        return Err(WaveformError::Unsupported {
            path: path.to_path_buf(),
            reason: "zero channels or sample rate".to_owned(),
        });
    }
    let channels = usize::from(spec.channels);
    match spec.sample_format {
        hound::SampleFormat::Float => build_from_samples(
            path,
            spec.sample_rate,
            spec.channels,
            reader.samples::<f32>(),
        ),
        hound::SampleFormat::Int if (1..=16).contains(&spec.bits_per_sample) => {
            let scale = (1_u64 << (spec.bits_per_sample - 1)) as f32;
            build_from_samples(
                path,
                spec.sample_rate,
                spec.channels,
                reader
                    .samples::<i16>()
                    .map(move |sample| sample.map(|sample| f32::from(sample) / scale)),
            )
        }
        hound::SampleFormat::Int if (17..=32).contains(&spec.bits_per_sample) => {
            let scale = (1_u64 << (spec.bits_per_sample - 1)) as f32;
            build_from_samples(
                path,
                spec.sample_rate,
                spec.channels,
                reader
                    .samples::<i32>()
                    .map(move |sample| sample.map(|sample| sample as f32 / scale)),
            )
        }
        _ => Err(WaveformError::Unsupported {
            path: path.to_path_buf(),
            reason: format!(
                "{}-bit {:?}, {channels} channels",
                spec.bits_per_sample, spec.sample_format
            ),
        }),
    }
}

fn build_from_samples<I>(
    path: &Path,
    sample_rate: u32,
    channel_count: u16,
    samples: I,
) -> Result<Peaks, WaveformError>
where
    I: Iterator<Item = Result<f32, hound::Error>>,
{
    let channels = usize::from(channel_count);
    let mut base = (0..channels).map(|_| Vec::new()).collect::<Vec<_>>();
    let mut mins = vec![f32::INFINITY; channels];
    let mut maxs = vec![f32::NEG_INFINITY; channels];
    let mut sample_count = 0usize;
    let mut frames = 0u64;
    let mut frames_in_peak = 0u64;

    for sample in samples {
        let sample = sample.map_err(|source| WaveformError::Decode {
            path: path.to_path_buf(),
            source,
        })?;
        let channel = sample_count % channels;
        let sample = if sample.is_finite() {
            sample.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        mins[channel] = mins[channel].min(sample);
        maxs[channel] = maxs[channel].max(sample);
        sample_count += 1;

        if channel + 1 == channels {
            frames += 1;
            frames_in_peak += 1;
            if frames_in_peak == BASE_FRAMES_PER_PEAK {
                push_peak(&mut base, &mut mins, &mut maxs);
                frames_in_peak = 0;
            }
        }
    }

    if !sample_count.is_multiple_of(channels) {
        return Err(WaveformError::Unsupported {
            path: path.to_path_buf(),
            reason: "partial interleaved frame".to_owned(),
        });
    }
    if frames_in_peak > 0 {
        push_peak(&mut base, &mut mins, &mut maxs);
    }
    if frames == 0 {
        return Err(WaveformError::Unsupported {
            path: path.to_path_buf(),
            reason: "file contains no audio frames".to_owned(),
        });
    }

    let mut levels = vec![PeakLevel {
        frames_per_peak: BASE_FRAMES_PER_PEAK,
        channels: base,
    }];
    loop {
        let bin_count = levels
            .last()
            .and_then(|level| level.channels.first())
            .map_or(0, Vec::len);
        if bin_count <= 1 {
            break;
        }
        let next = reduce_level(&levels[levels.len() - 1]);
        levels.push(next);
    }

    Ok(Peaks {
        sample_rate,
        channels: channel_count,
        frames,
        levels,
    })
}

fn push_peak(base: &mut [Vec<Peak>], mins: &mut [f32], maxs: &mut [f32]) {
    for channel in 0..base.len() {
        let peak = if mins[channel].is_finite() && maxs[channel].is_finite() {
            Peak {
                min: mins[channel],
                max: maxs[channel],
            }
        } else {
            Peak::SILENCE
        };
        base[channel].push(peak);
        mins[channel] = f32::INFINITY;
        maxs[channel] = f32::NEG_INFINITY;
    }
}

fn reduce_level(previous: &PeakLevel) -> PeakLevel {
    let channels = previous
        .channels
        .iter()
        .map(|channel| {
            channel
                .chunks(LEVEL_REDUCTION)
                .map(|chunk| {
                    let mut out = chunk.first().copied().unwrap_or(Peak::SILENCE);
                    for peak in &chunk[1..] {
                        out.include(*peak);
                    }
                    out
                })
                .collect()
        })
        .collect();
    PeakLevel {
        frames_per_peak: previous
            .frames_per_peak
            .saturating_mul(LEVEL_REDUCTION as u64),
        channels,
    }
}

/// Render-time transport and grid information. Keeping this together makes
/// the editor's geometry explicit and avoids a wide argument list.
#[derive(Clone, Copy)]
pub struct TimeView<'a> {
    pub bpm: f64,
    pub beats_per_bar: u32,
    pub playhead: f32,
    pub grid_beats: f32,
    pub grid_name: &'a str,
}

struct PaintView<'a> {
    display: egui::Rect,
    editor: &'a Editor,
    clip: &'a Clip,
    source: &'a AudioSource,
    peaks: Option<&'a Peaks>,
    time: TimeView<'a>,
}

/// View state only. Audio content and placement remain in [`Clip`].
pub struct Editor {
    shown_clip: Option<u64>,
    pub scroll_beats: f32,
    pub pixels_per_beat: f32,
    pub amplitude_zoom: f32,
    fit_pending: bool,
    pan_origin: Option<f32>,
    pub owns_keys: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            shown_clip: None,
            scroll_beats: 0.0,
            pixels_per_beat: PX_PER_BEAT_DEFAULT,
            amplitude_zoom: 1.0,
            fit_pending: true,
            pan_origin: None,
            owns_keys: false,
        }
    }
}

impl Editor {
    pub fn follow_clip(&mut self, clip: Option<u64>) {
        if self.shown_clip != clip {
            self.shown_clip = clip;
            self.scroll_beats = 0.0;
            self.amplitude_zoom = 1.0;
            self.fit_pending = true;
            self.pan_origin = None;
        }
    }

    pub fn step_zoom(&mut self, notches: i32) {
        self.pixels_per_beat = (self.pixels_per_beat * ZOOM_STEP.powi(notches))
            .clamp(PX_PER_BEAT_MIN, PX_PER_BEAT_MAX);
        self.fit_pending = false;
    }

    pub fn fit(&mut self) {
        self.fit_pending = true;
    }
}

pub fn keys(ctx: &egui::Context, editor: &mut Editor, has_clip: bool) {
    if !has_clip || !editor.owns_keys || ctx.egui_wants_keyboard_input() {
        return;
    }
    ctx.input_mut(|input| {
        if input.consume_key(egui::Modifiers::NONE, egui::Key::Plus)
            || input.consume_key(egui::Modifiers::NONE, egui::Key::Equals)
        {
            editor.step_zoom(1);
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::Minus) {
            editor.step_zoom(-1);
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::F) {
            editor.fit();
        }
    });
}

/// Draw the audio clip editor. The waveform is read-only for now; zoom, pan,
/// fit, amplitude scale, source/clip boundaries, grid and playhead are view
/// operations and never trigger an engine recompile.
pub fn body(
    ui: &mut egui::Ui,
    focus: &mut Focus,
    theme: &Theme,
    editor: &mut Editor,
    clip: Option<&Clip>,
    peaks: Option<&Peaks>,
    time: TimeView<'_>,
) {
    let area = ui.max_rect();
    claim(ui);
    ui.painter().rect_filled(area, 0.0, theme.surface_sunken);
    let header = egui::Rect::from_min_max(
        area.min,
        egui::pos2(area.right(), (area.top() + HEADER_H).min(area.bottom())),
    );
    let display = egui::Rect::from_min_max(egui::pos2(area.left(), header.bottom()), area.max);
    ui.painter().rect_filled(header, 0.0, theme.surface);
    ui.painter().line_segment(
        [header.left_bottom(), header.right_bottom()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );

    let Some(clip) = clip else {
        editor.owns_keys = false;
        ui.painter().text(
            area.center(),
            egui::Align2::CENTER_CENTER,
            "select an audio clip",
            egui::FontId::new(font::BODY, egui::FontFamily::Proportional),
            theme.text_muted,
        );
        return;
    };
    let Some(source) = clip.audio.as_ref() else {
        editor.owns_keys = false;
        return;
    };
    if display.width() <= 0.0 || display.height() <= 0.0 {
        return;
    }

    if editor.fit_pending && clip.len > 0.0 {
        editor.pixels_per_beat =
            (display.width() / clip.len).clamp(PX_PER_BEAT_MIN, PX_PER_BEAT_MAX);
        editor.scroll_beats = 0.0;
        editor.fit_pending = false;
    }

    let id = ui.id().with("waveform_display");
    let response = ui.interact(display, id, egui::Sense::click_and_drag());
    editor.owns_keys = focus.register(id, display);
    if response.double_clicked() {
        editor.fit();
    }
    if response.drag_started() {
        editor.pan_origin = Some(editor.scroll_beats);
    }
    if response.dragged() {
        let origin = editor.pan_origin.unwrap_or(editor.scroll_beats);
        editor.scroll_beats = origin - response.drag_delta().x / editor.pixels_per_beat;
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if response.drag_stopped() {
        editor.pan_origin = None;
    }

    zoom_and_scroll(ui, editor, display, clip.len);
    clamp_scroll(editor, display.width(), clip.len);

    let view = PaintView {
        display,
        editor,
        clip,
        source,
        peaks,
        time,
    };
    paint_header(ui, theme, header, &view, response.hover_pos());
    paint_channels(ui, theme, display, peaks.map_or(1, Peaks::channels));
    if peaks.is_some() {
        paint_waveform(ui, theme, &view);
    } else {
        ui.painter().text(
            display.center(),
            egui::Align2::CENTER_CENTER,
            "building waveform…",
            egui::FontId::new(font::BODY, egui::FontFamily::Proportional),
            theme.text_muted,
        );
    }
    paint_grid(ui, theme, &view);
    paint_clip_end(ui, theme, &view);
    paint_playhead(ui, theme, &view);
}

fn zoom_and_scroll(ui: &egui::Ui, editor: &mut Editor, display: egui::Rect, clip_len: f32) {
    let (modifiers, scroll) = ui.input(|input| (input.modifiers, input.smooth_scroll_delta));
    if !ui.rect_contains_pointer(display) || scroll == egui::Vec2::ZERO {
        return;
    }
    if modifiers.ctrl {
        let amount = scroll.y / 50.0;
        if modifiers.shift {
            editor.amplitude_zoom = (editor.amplitude_zoom * ZOOM_STEP.powf(amount))
                .clamp(AMPLITUDE_MIN, AMPLITUDE_MAX);
        } else {
            let old = editor.pixels_per_beat;
            let new = (old * ZOOM_STEP.powf(amount)).clamp(PX_PER_BEAT_MIN, PX_PER_BEAT_MAX);
            let x = ui
                .ctx()
                .pointer_latest_pos()
                .map_or(display.center().x, |position| position.x);
            let anchor = editor.scroll_beats + (x - display.left()) / old;
            editor.pixels_per_beat = new;
            editor.scroll_beats = anchor - (x - display.left()) / new;
            editor.fit_pending = false;
        }
        ui.ctx().input_mut(|input| {
            input.smooth_scroll_delta = egui::Vec2::ZERO;
        });
    } else {
        editor.scroll_beats -= (scroll.x + scroll.y) / editor.pixels_per_beat;
    }
    clamp_scroll(editor, display.width(), clip_len);
}

fn clamp_scroll(editor: &mut Editor, width: f32, clip_len: f32) {
    let visible = width / editor.pixels_per_beat.max(PX_PER_BEAT_MIN);
    editor.scroll_beats = editor
        .scroll_beats
        .clamp(0.0, (clip_len - visible).max(0.0));
}

fn paint_header(
    ui: &egui::Ui,
    theme: &Theme,
    header: egui::Rect,
    view: &PaintView<'_>,
    hover: Option<egui::Pos2>,
) {
    let detail = view.peaks.map_or_else(
        || "analysing…".to_owned(),
        |peaks| {
            let seconds = peaks.frames() as f64 / f64::from(peaks.sample_rate());
            format!(
                "{} ch  ·  {:.1} kHz  ·  {:.3} s",
                peaks.channels(),
                peaks.sample_rate() as f32 / 1_000.0,
                seconds
            )
        },
    );
    ui.painter().text(
        egui::pos2(header.left() + LABEL_PAD, header.center().y),
        egui::Align2::LEFT_CENTER,
        format!("{}  ·  {detail}", view.clip.name),
        egui::FontId::new(font::LABEL, egui::FontFamily::Proportional),
        theme.text,
    );
    let right = hover.map_or_else(
        || format!("{}  ·  {:.2} BPM", view.time.grid_name, view.time.bpm),
        |position| {
            let local = (view.editor.scroll_beats
                + (position.x - header.left()) / view.editor.pixels_per_beat)
                .clamp(0.0, view.clip.len);
            format!(
                "beat {:.3}  ·  {:.3} s",
                local,
                f64::from(local) * 60.0 / view.time.bpm
            )
        },
    );
    ui.painter().text(
        egui::pos2(header.right() - LABEL_PAD, header.center().y),
        egui::Align2::RIGHT_CENTER,
        right,
        egui::FontId::new(font::VALUE, egui::FontFamily::Monospace),
        theme.text_value,
    );
}

fn paint_channels(ui: &egui::Ui, theme: &Theme, display: egui::Rect, channels: usize) {
    let channels = channels.max(1);
    let lane_h = display.height() / channels as f32;
    for channel in 0..channels {
        let top = display.top() + channel as f32 * lane_h;
        let center = top + lane_h * 0.5;
        if channel > 0 {
            ui.painter().line_segment(
                [
                    egui::pos2(display.left(), top),
                    egui::pos2(display.right(), top),
                ],
                egui::Stroke::new(stroke::HAIR, theme.divider),
            );
        }
        ui.painter().line_segment(
            [
                egui::pos2(display.left(), center),
                egui::pos2(display.right(), center),
            ],
            egui::Stroke::new(stroke::HAIR, theme.outline),
        );
        let name = match (channels, channel) {
            (2, 0) => "L".to_owned(),
            (2, 1) => "R".to_owned(),
            (_, channel) => format!("{}", channel + 1),
        };
        ui.painter().text(
            egui::pos2(display.left() + LABEL_PAD, top + LABEL_PAD),
            egui::Align2::LEFT_TOP,
            name,
            egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
            theme.text_muted,
        );
    }
}

fn paint_waveform(ui: &egui::Ui, theme: &Theme, view: &PaintView<'_>) {
    let Some(peaks) = view.peaks else { return };
    let display = view.display;
    let editor = view.editor;
    let channels = peaks.channels().max(1);
    let lane_h = display.height() / channels as f32;
    let width = waveform_columns(
        display.width(),
        view.clip.len,
        editor.scroll_beats,
        editor.pixels_per_beat,
    );
    let colour = theme.accent;
    for pixel in 0..width {
        let x = display.left() + pixel as f32;
        let beat0 = editor.scroll_beats + pixel as f32 / editor.pixels_per_beat;
        let beat1 =
            (editor.scroll_beats + (pixel + 1) as f32 / editor.pixels_per_beat).min(view.clip.len);
        for channel in 0..channels {
            let Some(peak) =
                source_extrema(peaks, view.source, channel, beat0, beat1, view.time.bpm)
            else {
                continue;
            };
            let top = display.top() + channel as f32 * lane_h;
            let center = top + lane_h * 0.5;
            let half = (lane_h * 0.5 - CHANNEL_PAD).max(1.0) * editor.amplitude_zoom;
            let y0 = (center - peak.max * half).clamp(top, top + lane_h);
            let y1 = (center - peak.min * half).clamp(top, top + lane_h);
            ui.painter().line_segment(
                [egui::pos2(x, y0), egui::pos2(x, y1.max(y0 + 1.0))],
                egui::Stroke::new(1.0, colour),
            );
        }
    }
}

pub struct ClipThumbnail<'a> {
    pub full_clip: egui::Rect,
    pub visible_clip: egui::Rect,
    pub clip: &'a Clip,
    pub peaks: &'a Peaks,
    pub bpm: f64,
    pub opacity: f32,
}

/// Draw a compact waveform inside an arrangement clip. Geometry is measured
/// against the FULL clip rect even when only part is on screen; otherwise a
/// panned view would incorrectly stretch the visible source slice to fill the
/// clipped rect. [`source_extrema`] then applies source offset, source length,
/// loop wrapping and the current tempo exactly like the large editor.
pub fn paint_clip_thumbnail(ui: &egui::Ui, theme: &Theme, view: ClipThumbnail<'_>) {
    let Some(source) = view.clip.audio.as_ref() else {
        return;
    };
    if view.full_clip.width() <= 0.0
        || view.full_clip.height() <= 0.0
        || view.visible_clip.width() <= 0.0
    {
        return;
    }
    let label_h = if view.full_clip.height() >= 30.0 {
        18.0
    } else {
        2.0
    };
    let full_waveform = egui::Rect::from_min_max(
        egui::pos2(
            view.full_clip.left(),
            (view.full_clip.top() + label_h).min(view.full_clip.bottom()),
        ),
        egui::pos2(
            view.full_clip.right(),
            (view.full_clip.bottom() - 3.0).max(view.full_clip.top()),
        ),
    );
    let visible = full_waveform.intersect(view.visible_clip);
    if visible.width() <= 0.0 || visible.height() <= 0.0 {
        return;
    }

    let painter = ui.painter().with_clip_rect(visible);
    let channels = view.peaks.channels().max(1);
    let lane_h = full_waveform.height() / channels as f32;
    let columns = visible.width().ceil().max(0.0) as usize;
    let colour = theme.clip_note.gamma_multiply(view.opacity.clamp(0.0, 1.0));
    for pixel in 0..columns {
        let x = visible.left() + pixel as f32;
        let beat0 = thumbnail_local_beat(view.full_clip, view.clip.len, x);
        let beat1 = thumbnail_local_beat(view.full_clip, view.clip.len, x + 1.0).max(beat0);
        for channel in 0..channels {
            let Some(peak) = source_extrema(view.peaks, source, channel, beat0, beat1, view.bpm)
            else {
                continue;
            };
            let lane_top = full_waveform.top() + channel as f32 * lane_h;
            let center = lane_top + lane_h * 0.5;
            let half = (lane_h * 0.5 - 1.5).max(1.0);
            let y0 = (center - peak.max * half).clamp(lane_top, lane_top + lane_h);
            let y1 = (center - peak.min * half).clamp(lane_top, lane_top + lane_h);
            painter.line_segment(
                [egui::pos2(x, y0), egui::pos2(x, y1.max(y0 + 1.0))],
                egui::Stroke::new(1.0, colour),
            );
        }
    }
}

fn thumbnail_local_beat(full_clip: egui::Rect, clip_len: f32, x: f32) -> f32 {
    ((x - full_clip.left()) / full_clip.width().max(1e-6) * clip_len).clamp(0.0, clip_len)
}

/// The waveform issues no more than one vertical primitive per channel per
/// screen column. File length can reduce this count, never increase it.
fn waveform_columns(width: f32, clip_len: f32, scroll: f32, pixels_per_beat: f32) -> usize {
    let viewport = width.ceil().max(0.0);
    let remaining = ((clip_len - scroll).max(0.0) * pixels_per_beat)
        .ceil()
        .max(0.0);
    viewport.min(remaining) as usize
}

fn source_extrema(
    peaks: &Peaks,
    source: &AudioSource,
    channel: usize,
    beat0: f32,
    beat1: f32,
    bpm: f64,
) -> Option<Peak> {
    if source.source_frames == 0 || source.sample_rate == 0 || !bpm.is_finite() || bpm <= 0.0 {
        return None;
    }
    let frames_per_beat = f64::from(source.sample_rate) * 60.0 / bpm;
    let relative0 = (f64::from(beat0.max(0.0)) * frames_per_beat).floor() as u64;
    let relative1 = (f64::from(beat1.max(beat0)) * frames_per_beat)
        .ceil()
        .max(relative0 as f64 + 1.0) as u64;
    let region_end = source
        .source_offset
        .saturating_add(source.source_frames)
        .min(peaks.frames());
    if source.source_offset >= region_end {
        return None;
    }

    if !source.looped {
        let start = source.source_offset.saturating_add(relative0);
        let end = source
            .source_offset
            .saturating_add(relative1)
            .min(region_end);
        return peaks.extrema(channel, start, end);
    }

    let region_frames = region_end - source.source_offset;
    let span = relative1.saturating_sub(relative0);
    if span >= region_frames {
        return peaks.extrema(channel, source.source_offset, region_end);
    }
    let wrapped_start = relative0 % region_frames;
    let wrapped_end = wrapped_start.saturating_add(span.max(1));
    if wrapped_end <= region_frames {
        peaks.extrema(
            channel,
            source.source_offset + wrapped_start,
            source.source_offset + wrapped_end,
        )
    } else {
        let mut out = peaks.extrema(channel, source.source_offset + wrapped_start, region_end);
        if let Some(second) = peaks.extrema(
            channel,
            source.source_offset,
            source.source_offset + (wrapped_end - region_frames),
        ) {
            match &mut out {
                Some(out) => out.include(second),
                None => out = Some(second),
            }
        }
        out
    }
}

fn paint_grid(ui: &egui::Ui, theme: &Theme, view: &PaintView<'_>) {
    let subdivision = view.time.grid_beats.max(0.001);
    let step = if subdivision * view.editor.pixels_per_beat < GRID_MIN_PX {
        1.0
    } else {
        subdivision
    };
    let per_bar = view.time.beats_per_bar.max(1) as f32;
    let mut beat = first_grid_beat(view.clip.start, view.editor.scroll_beats, step);
    for _ in 0..MAX_GRID_LINES {
        let local = beat - view.clip.start;
        let x =
            view.display.left() + (local - view.editor.scroll_beats) * view.editor.pixels_per_beat;
        if x > view.display.right() {
            break;
        }
        if x >= view.display.left() {
            let on_bar = (beat % per_bar).abs() < 1e-3;
            let on_beat = beat.fract().abs() < 1e-3;
            let colour = if on_bar {
                theme.grid_bar
            } else if on_beat {
                theme.grid_beat
            } else {
                theme.grid_sub
            };
            ui.painter().line_segment(
                [
                    egui::pos2(x, view.display.top()),
                    egui::pos2(x, view.display.bottom()),
                ],
                egui::Stroke::new(stroke::HAIR, colour),
            );
            if on_bar {
                let bar = (beat / per_bar).floor().max(0.0) as u64 + 1;
                ui.painter().text(
                    egui::pos2(x + 3.0, view.display.top() + 3.0),
                    egui::Align2::LEFT_TOP,
                    bar.to_string(),
                    egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
                    theme.text_muted,
                );
            }
        }
        beat += step;
    }
}

fn first_grid_beat(clip_start: f32, scroll: f32, step: f32) -> f32 {
    let step = step.max(0.001);
    ((clip_start + scroll) / step).floor() * step
}

fn paint_clip_end(ui: &egui::Ui, theme: &Theme, view: &PaintView<'_>) {
    let end_x = view.display.left()
        + (view.clip.len - view.editor.scroll_beats) * view.editor.pixels_per_beat;
    if end_x < view.display.right() {
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(end_x.max(view.display.left()), view.display.top()),
                view.display.max,
            ),
            0.0,
            theme.bg.gamma_multiply(0.65),
        );
    }
    if end_x >= view.display.left() && end_x <= view.display.right() {
        ui.painter().line_segment(
            [
                egui::pos2(end_x, view.display.top()),
                egui::pos2(end_x, view.display.bottom()),
            ],
            egui::Stroke::new(1.5, theme.clip_selected),
        );
    }
}

fn paint_playhead(ui: &egui::Ui, theme: &Theme, view: &PaintView<'_>) {
    let local = view.time.playhead - view.clip.start;
    if local < 0.0 || local > view.clip.len {
        return;
    }
    let x = view.display.left() + (local - view.editor.scroll_beats) * view.editor.pixels_per_beat;
    if x >= view.display.left() && x <= view.display.right() {
        ui.painter().line_segment(
            [
                egui::pos2(x, view.display.top()),
                egui::pos2(x, view.display.bottom()),
            ],
            egui::Stroke::new(1.5, theme.playhead),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_wav() -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("daw-waveform-{}-{nonce}.wav", std::process::id()))
    }

    fn peaks(samples: &[f32], channels: u16) -> Peaks {
        build_from_samples(
            Path::new("test.wav"),
            48_000,
            channels,
            samples.iter().copied().map(Ok),
        )
        .expect("test samples are valid")
    }

    #[test]
    fn stereo_peaks_keep_channels_separate_and_sanitize_nonsense() {
        let waveform = peaks(
            &[-1.0, 0.25, 0.5, -0.75, f32::NAN, f32::INFINITY, 0.1, -0.2],
            2,
        );
        assert_eq!(waveform.channels(), 2);
        assert_eq!(waveform.frames(), 4);
        assert_eq!(
            waveform.extrema(0, 0, 4),
            Some(Peak {
                min: -1.0,
                max: 0.5
            })
        );
        assert_eq!(
            waveform.extrema(1, 0, 4),
            Some(Peak {
                min: -0.75,
                max: 0.25
            })
        );
    }

    #[test]
    fn wav_decoder_builds_the_playback_files_peak_pyramid() {
        let path = temp_wav();
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .expect("create test WAV");
        for sample in [i16::MIN, 8_192, 16_384, -16_384] {
            writer.write_sample(sample).expect("write test sample");
        }
        writer.finalize().expect("finalize test WAV");

        let waveform = build_peaks(&path).expect("decode test WAV");
        std::fs::remove_file(&path).expect("remove test WAV");

        assert_eq!(waveform.frames(), 2);
        assert_eq!(waveform.channels(), 2);
        let left = waveform.extrema(0, 0, 2).expect("left peaks");
        let right = waveform.extrema(1, 0, 2).expect("right peaks");
        assert!(left.min <= -1.0 && left.max >= 0.5);
        assert!(right.min <= -0.5 && right.max >= 0.25);
    }

    #[test]
    fn pyramid_queries_are_bounded_and_cover_the_requested_signal() {
        let samples = (0..4_096)
            .map(|index| if index == 2_048 { -1.0 } else { 0.5 })
            .collect::<Vec<_>>();
        let waveform = peaks(&samples, 1);
        assert!(waveform.levels.len() > 1);
        assert_eq!(
            waveform.extrema(0, 0, waveform.frames()),
            Some(Peak {
                min: -1.0,
                max: 0.5
            })
        );
        assert_eq!(waveform.extrema(1, 0, 10), None);
        assert_eq!(waveform.extrema(0, 10, 10), None);
    }

    #[test]
    fn source_mapping_obeys_bpm_offset_end_and_looping() {
        let samples = (0..512)
            .map(|index| index as f32 / 511.0)
            .collect::<Vec<_>>();
        let waveform =
            build_from_samples(Path::new("test.wav"), 128, 1, samples.into_iter().map(Ok))
                .expect("test samples are valid");
        let mut source = AudioSource {
            path: PathBuf::from("test.wav"),
            sample_rate: 128,
            source_offset: 128,
            source_frames: 256,
            gain: 1.0,
            looped: false,
        };
        // At 60 BPM, one beat is one second / 128 source frames.
        let first =
            source_extrema(&waveform, &source, 0, 0.0, 1.0, 60.0).expect("first source second");
        let second =
            source_extrema(&waveform, &source, 0, 1.0, 2.0, 60.0).expect("second source second");
        assert!(second.min > first.min);
        assert!(source_extrema(&waveform, &source, 0, 2.0, 3.0, 60.0).is_none());

        source.looped = true;
        assert_eq!(
            source_extrema(&waveform, &source, 0, 0.0, 1.0, 60.0),
            source_extrema(&waveform, &source, 0, 2.0, 3.0, 60.0)
        );
    }

    #[test]
    fn changing_clips_resets_view_but_reselecting_does_not() {
        let mut editor = Editor::default();
        editor.follow_clip(Some(7));
        editor.scroll_beats = 12.0;
        editor.amplitude_zoom = 3.0;
        editor.fit_pending = false;
        editor.follow_clip(Some(7));
        assert_eq!(editor.scroll_beats, 12.0);
        assert_eq!(editor.amplitude_zoom, 3.0);

        editor.follow_clip(Some(8));
        assert_eq!(editor.scroll_beats, 0.0);
        assert_eq!(editor.amplitude_zoom, 1.0);
        assert!(editor.fit_pending);
    }

    #[test]
    fn scroll_never_exposes_space_beyond_both_clip_edges() {
        let mut editor = Editor {
            scroll_beats: 99.0,
            pixels_per_beat: 100.0,
            ..Default::default()
        };
        clamp_scroll(&mut editor, 400.0, 10.0);
        assert_eq!(editor.scroll_beats, 6.0);
        editor.scroll_beats = -5.0;
        clamp_scroll(&mut editor, 400.0, 10.0);
        assert_eq!(editor.scroll_beats, 0.0);
    }

    #[test]
    fn waveform_work_is_bounded_by_viewport_not_file_duration() {
        assert_eq!(waveform_columns(800.0, 4.0, 0.0, 100.0), 400);
        assert_eq!(waveform_columns(800.0, 1_000_000.0, 0.0, 100.0), 800);
        assert_eq!(waveform_columns(800.0, 4.0, 4.0, 100.0), 0);
    }

    #[test]
    fn clipped_thumbnail_pixels_still_map_against_the_full_clip() {
        let full = egui::Rect::from_min_size(egui::pos2(100.0, 0.0), egui::vec2(400.0, 64.0));
        assert_eq!(thumbnail_local_beat(full, 4.0, 100.0), 0.0);
        assert_eq!(thumbnail_local_beat(full, 4.0, 300.0), 2.0);
        assert_eq!(thumbnail_local_beat(full, 4.0, 500.0), 4.0);
        // If the viewport begins at x=300, that first visible pixel remains
        // beat 2. It must not be remapped to beat 0 and stretch the waveform.
        let visible = full.intersect(egui::Rect::from_min_max(
            egui::pos2(300.0, 0.0),
            egui::pos2(500.0, 64.0),
        ));
        assert_eq!(thumbnail_local_beat(full, 4.0, visible.left()), 2.0);
    }

    #[test]
    fn complete_editor_renders_headlessly_and_fits_the_clip() {
        let samples = vec![0.5; 48_000];
        let waveform = peaks(&samples, 1);
        let clip = Clip {
            id: 42,
            name: "voice".to_owned(),
            start: 8.0,
            len: 2.0,
            notes: Vec::new(),
            audio: Some(AudioSource {
                path: PathBuf::from("voice.wav"),
                sample_rate: 48_000,
                source_offset: 0,
                source_frames: 48_000,
                gain: 1.0,
                looped: false,
            }),
        };
        let context = egui::Context::default();
        let mut editor = Editor::default();
        editor.follow_clip(Some(clip.id));
        let mut focus = Focus::default();
        let theme = Theme::dark();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 300.0),
                )),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    body(
                        ui,
                        &mut focus,
                        &theme,
                        &mut editor,
                        Some(&clip),
                        Some(&waveform),
                        TimeView {
                            bpm: 120.0,
                            beats_per_bar: 4,
                            playhead: 9.0,
                            grid_beats: 0.25,
                            grid_name: "1/16",
                        },
                    );
                    let full_clip =
                        egui::Rect::from_min_size(egui::pos2(20.0, 80.0), egui::vec2(600.0, 64.0));
                    paint_clip_thumbnail(
                        ui,
                        &theme,
                        ClipThumbnail {
                            full_clip,
                            visible_clip: full_clip.intersect(egui::Rect::from_min_max(
                                egui::pos2(220.0, 80.0),
                                egui::pos2(620.0, 144.0),
                            )),
                            clip: &clip,
                            peaks: &waveform,
                            bpm: 120.0,
                            opacity: 1.0,
                        },
                    );
                });
            },
        );
        output.textures_delta.clear();

        assert!(!editor.fit_pending);
        assert_eq!(editor.scroll_beats, 0.0);
        assert!(editor.pixels_per_beat > PX_PER_BEAT_DEFAULT);
    }

    #[test]
    fn grid_lines_are_absolute_even_when_the_clip_starts_off_grid() {
        assert_eq!(first_grid_beat(1.5, 0.0, 1.0), 1.0);
        assert_eq!(first_grid_beat(1.5, 0.75, 1.0), 2.0);
        assert_eq!(first_grid_beat(5.25, 0.0, 0.5), 5.0);
    }
}
