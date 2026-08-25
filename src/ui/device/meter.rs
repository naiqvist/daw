//! The level meter: a dB-scaled peak meter with hold, ballistics and a
//! latching clip light.
//!
//! # Why not [`kit::meter`]
//!
//! `kit`'s meter is a bar filled by a fraction of full scale, and its own
//! comment says so: "fractions of full scale, not dB". That is the classic
//! way to get a meter wrong. Amplitude is not perceptually linear — half
//! amplitude is −6 dB, which is a *small* drop, but a linear bar draws it
//! at the halfway mark. The result reads as if everything quiet is loud,
//! and the entire top half of the scale, where all the decisions actually
//! happen, gets compressed into nothing. This one maps through decibels.
//!
//! # The manners it keeps
//!
//! A meter is a thing engineers have opinions about, and the opinions are
//! consistent. All of this is those expectations, written down:
//!
//! - **Instant attack, slow release.** The bar jumps to a peak the frame
//!   it happens and falls back gradually. A meter that eases *upward*
//!   under-reads transients, which is the one thing a meter must never do.
//! - **A peak-hold marker** that sits at the loudest recent moment, waits
//!   long enough to be read, then slides down more slowly than the bar —
//!   so it stays legible on the way out instead of vanishing.
//! - **A clip light that LATCHES.** It stays lit after the overload has
//!   passed, because the whole point is to tell you about something you
//!   were not watching. Click the meter to clear it.
//! - **Colour by headroom, not by fraction.** Green up to −6 dBFS, amber
//!   above it, red at full scale — the thresholds every console uses.
//! - **Ticks at the round numbers**, and only when the scale is long
//!   enough to read them.
//!
//! [`kit::meter`]: crate::ui::kit::meter

use crate::ui::device::design;
use crate::ui::device::metrics::Footprint;
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, stroke};
use eframe::egui;

/// The bottom of the scale. −60 dBFS is the usual floor: quiet enough to
/// show a fade tail, shallow enough that the useful range is not a sliver
/// at the top.
pub const FLOOR_DB: f32 = -60.0;
/// The top of the scale. 0 dBFS is full scale — there is nothing above it
/// to show, only clipping to report.
pub const CEILING_DB: f32 = 0.0;
/// Where the bar turns amber: the last 6 dB of headroom.
pub const HOT_DB: f32 = -6.0;

/// How fast the bar falls, in dB per second.
///
/// 20 dB/s crosses the whole scale in three seconds: quick enough to
/// follow a mix, slow enough that a transient stays on screen long enough
/// to read. A true IEC PPM falls at ~8.6 dB/s, which is unreadably sluggish
/// on a screen refreshing sixty times a second.
const RELEASE_DB_PER_S: f32 = 20.0;
/// How long the peak marker sits still before it starts to fall.
const PEAK_HOLD_S: f32 = 1.5;
/// How fast the peak marker falls once it lets go — slower than the bar,
/// so it stays readable on the way down instead of racing it.
const PEAK_FALL_DB_PER_S: f32 = 12.0;
/// dB between scale ticks, and the shortest track that gets any.
const TICK_STEP_DB: f32 = 12.0;
const TICKS_MIN_LEN: f32 = 60.0;

// ------------------------------------------------------------ mapping ---

/// Linear amplitude to dBFS. Silence is −infinity, honestly: callers floor
/// it where they need a number, rather than this pretending 0.0 is −60.
pub fn amp_to_db(amp: f32) -> f32 {
    let a = amp.abs();
    if a <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * a.log10()
    }
}

/// dBFS to a `0..=1` position on the scale. Anything at or below the floor
/// is 0, anything at or above the ceiling is 1, and −infinity and NaN both
/// answer 0 rather than poisoning the geometry.
pub fn db_to_norm(db: f32) -> f32 {
    if db.is_nan() {
        return 0.0;
    }
    ((db - FLOOR_DB) / (CEILING_DB - FLOOR_DB)).clamp(0.0, 1.0)
}

/// Linear amplitude straight to a scale position — the two above, joined.
pub fn amp_to_norm(amp: f32) -> f32 {
    db_to_norm(amp_to_db(amp))
}

// --------------------------------------------------------- ballistics ---

/// One channel's motion over time: where the bar is, where the peak
/// marker is, and whether an overload has been seen and not yet cleared.
///
/// Pure, and deliberately free of egui: ballistics are the part of a meter
/// that is easy to get subtly wrong and impossible to eyeball, so they are
/// testable without a window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ballistics {
    /// Where the bar is drawn, in dB.
    pub shown_db: f32,
    /// Where the peak marker is drawn, in dB.
    pub peak_db: f32,
    /// Seconds left before the peak marker starts falling.
    pub hold_s: f32,
    /// An overload has been seen and not cleared.
    pub clipped: bool,
}

impl Default for Ballistics {
    fn default() -> Self {
        Self {
            shown_db: FLOOR_DB,
            peak_db: FLOOR_DB,
            hold_s: 0.0,
            clipped: false,
        }
    }
}

impl Ballistics {
    /// Advance by `dt` seconds toward `db`, which must already be floored
    /// (use `amp_to_db(x).max(FLOOR_DB)`) — every arithmetic path here
    /// assumes a finite number, and −infinity would spread through the
    /// state and never leave.
    pub fn advance(&mut self, db: f32, dt: f32) {
        let db = if db.is_nan() {
            FLOOR_DB
        } else {
            db.max(FLOOR_DB)
        };
        let dt = dt.max(0.0);

        // Attack is INSTANT. Easing upward makes a meter under-read the
        // transient it exists to catch.
        if db >= self.shown_db {
            self.shown_db = db;
        } else {
            self.shown_db = (self.shown_db - RELEASE_DB_PER_S * dt).max(db);
        }

        // The peak marker: rise with the signal, then hold, then fall.
        //
        // `db > FLOOR_DB` is load-bearing. Silence sits AT the floor, and
        // so does a settled marker — so a bare `db >= peak_db` is true on
        // every silent frame and re-arms the hold forever. The meter then
        // never reports itself at rest and asks for a repaint sixty times
        // a second at idle, which is invisible on screen and expensive
        // everywhere else.
        if db > FLOOR_DB && db >= self.peak_db {
            self.peak_db = db;
            self.hold_s = PEAK_HOLD_S;
        } else if self.hold_s > 0.0 {
            // A frame that OUTLASTS the hold spends its remainder
            // falling. Without this the marker stalls for a whole extra
            // frame every time, which is invisible at 60 Hz and very
            // visible in a lagging window — the case where a stuck peak
            // marker is most misleading.
            self.hold_s -= dt;
            if self.hold_s < 0.0 {
                self.peak_db = (self.peak_db - PEAK_FALL_DB_PER_S * -self.hold_s).max(db);
                self.hold_s = 0.0;
            }
        } else {
            self.peak_db = (self.peak_db - PEAK_FALL_DB_PER_S * dt).max(db);
        }

        // Latching: set here, cleared only by a person.
        if db >= CEILING_DB {
            self.clipped = true;
        }
    }

    /// Is anything still moving? A meter at rest should stop asking for
    /// repaints — an idle rack must not keep a GPU awake.
    pub fn moving(&self) -> bool {
        self.shown_db > FLOOR_DB || self.peak_db > FLOOR_DB || self.hold_s > 0.0
    }
}

// --------------------------------------------------------------- view ---

/// The meter's size contract: one lane per channel, side by side.
///
/// No label and no readout. A meter is read by its position on a scale,
/// and a number beside it would be a second, slower way to learn the same
/// thing — every console leaves it off for the same reason.
pub fn footprint(theme: &Theme, channels: usize) -> Footprint {
    let n = channels.max(1) as f32;
    let gap = design::gap(theme);
    Footprint::new(
        theme.sp(control::METER_W) * n + gap * (n - 1.0),
        theme.sp(control::METER_LEN),
    )
}

/// Draw a level meter over `levels` — one lane per entry, as LINEAR
/// amplitudes where 1.0 is full scale.
///
/// Returns true when the user cleared a latched clip light this frame, so
/// a caller that logs overloads can log the acknowledgement too.
pub fn meter(ui: &mut egui::Ui, theme: &Theme, levels: &[f32]) -> bool {
    let channels = levels.len().max(1);
    let size = footprint(theme, channels).size;
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    // Per-channel motion lives across frames, keyed to this widget. A
    // meter whose state came from the caller would make every caller
    // reimplement ballistics; a meter with no state could not have any.
    let id = response.id;
    let mut state: Vec<Ballistics> = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    state.resize(channels, Ballistics::default());

    let dt = ui.input(|i| i.stable_dt);
    for (i, ball) in state.iter_mut().enumerate() {
        let amp = levels.get(i).copied().unwrap_or(0.0);
        ball.advance(amp_to_db(amp).max(FLOOR_DB), dt);
    }

    // Clicking anywhere clears every lane's latch: the light is one
    // statement about this meter, so acknowledging it is one gesture.
    let cleared = response.clicked() && state.iter().any(|b| b.clipped);
    if cleared {
        for ball in state.iter_mut() {
            ball.clipped = false;
        }
    }

    paint(ui, theme, rect, &state);

    // Keep frames coming only while something is actually moving.
    if state.iter().any(Ballistics::moving) {
        ui.ctx().request_repaint();
    }
    ui.data_mut(|d| d.insert_temp(id, state));
    cleared
}

fn paint(ui: &egui::Ui, theme: &Theme, rect: egui::Rect, state: &[Ballistics]) {
    let painter = ui.painter();
    let n = state.len().max(1);
    let gap = design::gap(theme);
    let lane_w = ((rect.width() - gap * (n - 1) as f32) / n as f32).max(1.0);
    let clip_h = theme.sp(control::METER_CLIP_H);

    for (i, ball) in state.iter().enumerate() {
        let x = rect.left() + (lane_w + gap) * i as f32;
        let lane =
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(lane_w, rect.height()));

        // The clip light caps the lane; the scale is what is left.
        let light = egui::Rect::from_min_size(lane.min, egui::vec2(lane_w, clip_h));
        let track = egui::Rect::from_min_max(
            egui::pos2(lane.left(), lane.top() + clip_h + stroke::HAIR),
            lane.max,
        );

        painter.rect_filled(
            light,
            design::box_radius(),
            if ball.clipped {
                theme.meter_clip
            } else {
                theme.surface_sunken
            },
        );
        painter.rect_filled(track, design::box_radius(), theme.surface_sunken);

        // Ticks at the round numbers, on the track only, and only when
        // there is room to tell them apart.
        if track.height() >= TICKS_MIN_LEN {
            let mut db = CEILING_DB - TICK_STEP_DB;
            while db > FLOOR_DB {
                let y = track.bottom() - track.height() * db_to_norm(db);
                painter.line_segment(
                    [egui::pos2(track.left(), y), egui::pos2(track.right(), y)],
                    egui::Stroke::new(stroke::HAIR, theme.divider),
                );
                db -= TICK_STEP_DB;
            }
        }

        // The bar, in two pieces: everything below the hot threshold in
        // the low colour, the last 6 dB of headroom in the hot one. Drawn
        // as segments rather than tinted whole, so a loud signal does not
        // repaint the quiet part of its own history a different colour.
        let y_at = |db: f32| track.bottom() - track.height() * db_to_norm(db);
        let shown = ball.shown_db;
        if shown > FLOOR_DB {
            let low_top = y_at(shown.min(HOT_DB));
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(track.left(), low_top), track.max),
                design::box_radius(),
                theme.meter_low,
            );
            if shown > HOT_DB {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(track.left(), y_at(shown)),
                        egui::pos2(track.right(), y_at(HOT_DB)),
                    ),
                    0.0,
                    theme.meter_hot,
                );
            }
        }

        // The peak marker: a hairline at the loudest recent moment, in the
        // colour that level would have been.
        if ball.peak_db > FLOOR_DB {
            let y = y_at(ball.peak_db);
            let colour = if ball.peak_db >= CEILING_DB {
                theme.meter_clip
            } else if ball.peak_db > HOT_DB {
                theme.meter_hot
            } else {
                theme.meter_low
            };
            painter.line_segment(
                [egui::pos2(track.left(), y), egui::pos2(track.right(), y)],
                egui::Stroke::new(stroke::BOLD, colour),
            );
        }

        painter.rect_stroke(
            lane,
            design::box_radius(),
            egui::Stroke::new(stroke::HAIR, theme.outline),
            egui::StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// The mapping is dB, not amplitude — the whole reason this widget
    /// exists. Half amplitude is a SMALL drop and must read near the top,
    /// not at the halfway mark.
    #[test]
    fn the_scale_is_decibels_not_amplitude() {
        assert_eq!(amp_to_norm(1.0), 1.0, "full scale is the top");
        let half = amp_to_norm(0.5);
        assert!(
            (half - 0.9).abs() < 0.01,
            "half amplitude is -6 dB, which is 0.9 up a -60..0 scale, not 0.5 — got {half}"
        );
        // A linear meter would put these at 0.25 and 0.1; on a dB scale
        // they are still well up the track.
        assert!(amp_to_norm(0.25) > 0.75);
        assert!(amp_to_norm(0.1) > 0.6);
        // Monotonic all the way up.
        let mut last = -0.1;
        for i in 0..=100 {
            let n = amp_to_norm(i as f32 / 100.0);
            assert!(n >= last - 1e-6, "not monotonic at {i}: {n} after {last}");
            last = n;
        }
    }

    /// Silence, negatives and nonsense answer a number rather than
    /// poisoning the geometry. A NaN here becomes a NaN rectangle, which
    /// egui draws as nothing at all — a meter that silently disappears.
    #[test]
    fn silence_and_nonsense_stay_finite() {
        assert_eq!(amp_to_norm(0.0), 0.0);
        assert_eq!(amp_to_db(0.0), f32::NEG_INFINITY, "honestly, not -60");
        assert_eq!(db_to_norm(f32::NEG_INFINITY), 0.0);
        assert_eq!(db_to_norm(f32::NAN), 0.0);
        assert_eq!(amp_to_norm(-0.5), amp_to_norm(0.5), "polarity is not level");
        assert_eq!(amp_to_norm(4.0), 1.0, "over full scale still tops out");
        for probe in [0.0, 1.0, -1.0, 1e-30, f32::MAX] {
            assert!(amp_to_norm(probe).is_finite());
        }
    }

    /// Attack is instantaneous and release is gradual — the asymmetry
    /// that makes a peak meter a peak meter.
    #[test]
    fn attack_is_instant_and_release_is_gradual() {
        let mut b = Ballistics::default();
        b.advance(-3.0, 1.0 / 60.0);
        assert_eq!(b.shown_db, -3.0, "the bar reaches a peak the same frame");

        // Falling to silence takes the scale at the documented rate.
        b.advance(FLOOR_DB, 1.0);
        assert!(
            (b.shown_db - (-3.0 - RELEASE_DB_PER_S)).abs() < 0.01,
            "one second of release is {RELEASE_DB_PER_S} dB, got {}",
            b.shown_db
        );
        // It never falls PAST the signal.
        let mut b = Ballistics::default();
        b.advance(-10.0, 0.1);
        b.advance(-12.0, 10.0);
        assert_eq!(b.shown_db, -12.0, "release stops at the current level");
    }

    /// The peak marker holds still long enough to read, then falls more
    /// slowly than the bar.
    #[test]
    fn the_peak_marker_holds_then_falls_slowly() {
        let mut b = Ballistics::default();
        b.advance(-6.0, 0.0);
        assert_eq!(b.peak_db, -6.0);

        // Still there most of a second later, while the bar has moved on.
        b.advance(FLOOR_DB, 0.9);
        assert_eq!(b.peak_db, -6.0, "the marker holds");
        assert!(b.shown_db < -6.0, "but the bar has fallen");

        // After the hold, it starts down — and slower than the bar.
        b.advance(FLOOR_DB, PEAK_HOLD_S);
        let dropped = -6.0 - b.peak_db;
        assert!(dropped > 0.0, "the marker eventually falls");
        // The marker must fall SLOWER than the bar, or it is not a
        // marker — it is a second bar. Compared as a const so a future
        // retune of either rate trips here rather than in someone's eyes.
        const _: () = assert!(PEAK_FALL_DB_PER_S < RELEASE_DB_PER_S);
        // A louder moment re-arms the hold.
        b.advance(-2.0, 0.0);
        assert_eq!(b.peak_db, -2.0);
        assert_eq!(b.hold_s, PEAK_HOLD_S);
    }

    /// The clip light LATCHES. It exists to report something you were not
    /// watching, so it must outlive the overload — and only a person
    /// clears it.
    #[test]
    fn clipping_latches_until_it_is_cleared() {
        let mut b = Ballistics::default();
        b.advance(-1.0, 0.1);
        assert!(!b.clipped, "-1 dBFS is not clipping");

        b.advance(0.0, 0.1);
        assert!(b.clipped, "0 dBFS is full scale, which counts");

        // Ten seconds of silence later it is STILL lit.
        for _ in 0..600 {
            b.advance(FLOOR_DB, 1.0 / 60.0);
        }
        assert!(b.clipped, "the latch outlives the overload");
        assert_eq!(b.shown_db, FLOOR_DB, "while the bar has long since gone");

        b.clipped = false;
        b.advance(-20.0, 0.1);
        assert!(!b.clipped, "and stays cleared while the signal behaves");
    }

    /// A meter at rest stops asking for repaints.
    #[test]
    fn a_silent_meter_settles() {
        let mut b = Ballistics::default();
        assert!(!b.moving(), "it starts at rest");
        b.advance(-6.0, 0.0);
        assert!(b.moving());
        for _ in 0..1_000 {
            b.advance(FLOOR_DB, 1.0 / 60.0);
        }
        assert!(!b.moving(), "silence eventually stops the animation: {b:?}");
    }

    /// Time going backwards, or not at all, must not move anything.
    #[test]
    fn a_zero_or_negative_frame_changes_nothing() {
        let mut b = Ballistics::default();
        b.advance(-6.0, 0.1);
        let before = b;
        b.advance(FLOOR_DB, 0.0);
        assert_eq!(b.shown_db, before.shown_db);
        b.advance(FLOOR_DB, -5.0);
        assert_eq!(b.shown_db, before.shown_db, "negative dt is not rewind");
    }

    /// The footprint is one lane per channel with the design gap between,
    /// so a stereo meter is not simply twice as wide.
    #[test]
    fn the_footprint_is_one_lane_per_channel() {
        let theme = Theme::dark();
        let gap = design::gap(&theme);
        let mono = footprint(&theme, 1);
        let stereo = footprint(&theme, 2);
        assert_eq!(stereo.height(), mono.height(), "channels do not add height");
        assert!(
            (stereo.width() - (mono.width() * 2.0 + gap)).abs() < 0.01,
            "two lanes and one gap"
        );
        // Zero channels is still a meter, not a zero-width sliver.
        assert_eq!(footprint(&theme, 0).width(), mono.width());
    }
}
