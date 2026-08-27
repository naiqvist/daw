//! Musical and wall-clock time, as pure functions.
//!
//! Everything here turns seconds into something a musician or a view can
//! read, and nothing here touches egui, the transport, or the app. That is
//! the point: the readout's arithmetic, the loop wrap and the follow-page
//! rule are the parts most worth pinning by test, and they are checkable
//! without a window or a mouse.
//!
//! Lifted out of `main.rs` unchanged.

/// Sixteenths per beat — the readout's finest division.
pub const DIVISIONS: u64 = 4;

/// Position as bar.beat.sixteenth, all 1-indexed the way a musician counts.
///
/// Pure, and the only place seconds become musical time. Takes `bpm` and the
/// bar length rather than reading the constants, so the tempo and time
/// signature controls feed it without touching the arithmetic.
pub fn bars_beats(seconds: f64, bpm: f64, beats_per_bar: u64) -> (u64, u64, u64) {
    let beats = (seconds.max(0.0) * bpm / 60.0).max(0.0);
    let per_bar = beats_per_bar.max(1) * DIVISIONS;
    let total = (beats * DIVISIONS as f64).floor() as u64;
    let (bar, rest) = (total / per_bar, total % per_bar);
    (bar + 1, rest / DIVISIONS + 1, rest % DIVISIONS + 1)
}

/// The readout's text. Fixed field widths so digits do not shuffle sideways
/// as the playhead rolls — the whole reason this is monospace.
pub fn format_position(seconds: f64, bpm: f64, beats_per_bar: u64) -> String {
    let (bar, beat, sixteenth) = bars_beats(seconds, bpm, beats_per_bar);
    format!("{bar:>4}.{beat:>2}.{sixteenth}")
}

/// Wall-clock position, `m:ss.mmm`. Fixed width for the same reason.
pub fn format_timecode(seconds: f64) -> String {
    let s = seconds.max(0.0);
    let minutes = (s / 60.0).floor();
    format!("{:>3}:{:06.3}", minutes as u64, s - minutes * 60.0)
}

/// Move the whole loop by `delta` beats, never before beat 0, length
/// untouched. The handles change the length; the body only carries it.
///
/// Pure, so the brace-body drag is checkable without a mouse.
pub fn loop_move(range: (f32, f32), delta: f32) -> (f32, f32) {
    let len = range.1 - range.0;
    let start = (range.0 + delta).max(0.0);
    (start, start + len)
}

/// Where the view sits after follow has had its say. Follow off means hands
/// off. On: when the playhead crosses the right edge, the view jumps a page
/// so the playhead lands at the left; when the playhead falls behind the
/// view (Return, Stop), the view jumps back to it. Between edges the view
/// stays put — a continuously chasing view would be unreadable.
///
/// Pure, so the page logic is checkable without a window.
pub fn follow_view(offset: f32, playhead: f32, view_beats: f32, follow: bool) -> f32 {
    if !follow {
        return offset;
    }
    if playhead >= offset + view_beats || playhead < offset {
        playhead
    } else {
        offset
    }
}

/// Wrap the stand-in clock around the loop region. The loop's unit is the
/// beat, so the wrap is computed in beats and converted back to seconds.
///
/// This is a STAND-IN, like the clock it feeds: the engine's transport owns
/// sample-accurate loop points. When the engine is wired into the app, this
/// function and the clock both die.
pub fn wrap_loop(seconds: f64, bpm: f64, from_beats: f32, to_beats: f32) -> f64 {
    let beats = seconds * bpm / 60.0;
    let len = (to_beats - from_beats) as f64;
    if len <= 0.0 || beats < to_beats as f64 {
        return seconds;
    }
    let wrapped = from_beats as f64 + (beats - from_beats as f64) % len;
    wrapped * 60.0 / bpm
}
