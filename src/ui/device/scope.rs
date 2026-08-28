//! The dynamics scope: what a compressor is DOING, over the last few
//! seconds.
//!
//! # Why a time axis and not a transfer curve
//!
//! [`dynamics::transfer_curve`] draws the RULES — what would happen to
//! any level — and its own module says why that is the wrong hero for a
//! bus compressor: it "is the only one of the two that can show attack
//! and release at all" is said of the BAR view, and neither view has a
//! time axis to show them ON.
//!
//! The two things that make the bus compressor what it is are both
//! invisible without one:
//!
//! - **Auto release is program dependent.** Its whole behaviour is the
//!   SHAPE of the recovery — fast after a transient, slow after a
//!   sustained passage. On a static curve those are the same picture.
//! - **The feedback loop bends the ratio.** How far it settles depends
//!   on how hard it was hit, which is a thing that happens over time.
//!
//! And a transfer curve for a THREE-POSITION ratio switch is a picture
//! with three states. It is interesting once.
//!
//! # The two lanes
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │ ────────────────────────────────────────     │ ← 0: reduction hangs
//! │    ███▓▓░        ████▓▓▓░░                   │   DOWN from unity
//! ├──────────────────────────────────────────────┤
//! │ ╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌   │ ← threshold, dragged
//! │   ▁▂▃▅▇█▇▅▃▂▁   ▁▂▃▅▇██▇▅▃▂▁                 │ ← level, from the floor
//! └──────────────────────────────────────────────┘
//!   old                                     now
//! ```
//!
//! Reduction hangs downward for the reason
//! [`dynamics::reduction_meter`] gives: it is something being taken
//! away, and a bar that filled upward would read as "more is louder",
//! which is backwards. Level fills from the floor because it is a level.
//!
//! They share the time axis, and that is the point of putting them
//! together: the kick crosses the threshold in the lower lane, and three
//! milliseconds later the upper lane dips. That is the attack control,
//! drawn — nothing else on the card explains it at all.
//!
//! # Where the numbers come from
//!
//! The engine's own, through `graph::Readout` — the loudest the detector
//! heard and the most it reduced, per BLOCK. Not the newest sample: this
//! draws at frame rate and the engine works per sample, so the newest
//! value would miss every transient that happened between two repaints.
//!
//! [`dynamics::transfer_curve`]: crate::ui::device::dynamics::transfer_curve
//! [`dynamics::reduction_meter`]: crate::ui::device::dynamics::reduction_meter

use crate::ui::affordance::{Afford, Affords};
use crate::ui::device::{Footprint, metrics};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// How many readings the history keeps.
///
/// At 60 frames a second this is about four seconds, which is the window
/// a bus compressor is judged over — long enough to see a chorus breathe,
/// short enough that the newest reading is still near the right edge.
pub const HISTORY: usize = 240;

/// The floor of the level lane, in dBFS. Everything below reads as
/// silence; a bus rarely lives under this and the lane's resolution is
/// better spent where the decisions are.
pub const LEVEL_FLOOR_DB: f32 = -48.0;

/// The CEILING of the level lane, in dBFS — above full scale on purpose.
///
/// A bus can exceed 0 dBFS internally, so the headroom is honest. It also
/// gives the threshold somewhere to live at its default: pinned to the
/// top of the lane it is indistinguishable from the lane's own edge, and
/// a compressor whose main control looks like a border is a compressor
/// nobody finds the control on.
pub const LEVEL_CEILING_DB: f32 = 6.0;

/// The dB rules drawn across the level lane.
const LEVEL_RULES: [f32; 3] = [0.0, -12.0, -24.0];

/// The dB rules drawn across the reduction lane.
const REDUCTION_RULES: [f32; 2] = [-6.0, -12.0];

/// The floor of the reduction lane, in dB. Past this a compressor is not
/// gluing anything, and giving the lane more range would flatten every
/// ordinary 2-6 dB move into nothing.
pub const REDUCTION_FLOOR_DB: f32 = -24.0;

/// How tall the reduction lane is as a fraction of the whole scope. The
/// smaller share on purpose: reduction is read as a DEPTH from a fixed
/// rule, which needs less room than a level whose shape carries meaning.
const REDUCTION_SHARE: f32 = 0.36;

/// One block's worth of what the engine saw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    pub level_db: f32,
    /// Reduction, in dB. Zero is none, negative is reduction — the gain
    /// computer's own sign, carried all the way to the paint.
    pub reduction_db: f32,
}

impl Default for Reading {
    fn default() -> Self {
        Self {
            level_db: LEVEL_FLOOR_DB,
            reduction_db: 0.0,
        }
    }
}

/// The rolling history a scope draws, and the peak it holds.
///
/// A plain ring: `push` once per frame, oldest to newest left to right.
/// Kept by the CARD rather than by the widget, because a widget that
/// owned it would forget everything the moment the card scrolled out of
/// view — and a compressor's history is the one thing you look away from
/// and back at.
#[derive(Debug, Clone)]
pub struct History {
    ring: [Reading; HISTORY],
    /// Where the NEXT reading goes; the oldest is here too.
    head: usize,
    /// The most reduction seen since the hold was last cleared, in dB.
    peak_db: f32,
}

impl Default for History {
    fn default() -> Self {
        Self {
            ring: [Reading::default(); HISTORY],
            head: 0,
            peak_db: 0.0,
        }
    }
}

impl History {
    /// Add this frame's reading.
    pub fn push(&mut self, reading: Reading) {
        let reading = Reading {
            level_db: finite(reading.level_db, LEVEL_FLOOR_DB),
            reduction_db: finite(reading.reduction_db, 0.0).min(0.0),
        };
        if let Some(slot) = self.ring.get_mut(self.head) {
            *slot = reading;
        }
        self.head = (self.head + 1) % HISTORY;
        self.peak_db = self.peak_db.min(reading.reduction_db);
    }

    /// The newest reading — what the corner tag prints.
    pub fn latest(&self) -> Reading {
        let last = (self.head + HISTORY - 1) % HISTORY;
        self.ring.get(last).copied().unwrap_or_default()
    }

    /// The most reduction held since [`History::clear_peak`].
    pub fn peak_db(&self) -> f32 {
        self.peak_db
    }

    /// Let the hold go — what clicking the tag does.
    pub fn clear_peak(&mut self) {
        self.peak_db = 0.0;
    }

    /// Oldest first, so a caller walks left to right.
    fn iter(&self) -> impl Iterator<Item = Reading> + '_ {
        (0..HISTORY).filter_map(move |i| self.ring.get((self.head + i) % HISTORY).copied())
    }
}

/// The scope's size contract: as wide as a display wants, and tall
/// enough for two lanes that can both be read.
pub fn footprint(theme: &Theme) -> Footprint {
    Footprint::new(theme.sp(control::XY_PAD), theme.sp(control::SPECTRUM_H))
}

/// Where `db` sits up the level lane, `0..=1`.
fn level_norm(db: f32) -> f32 {
    ((db - LEVEL_FLOOR_DB) / (LEVEL_CEILING_DB - LEVEL_FLOOR_DB)).clamp(0.0, 1.0)
}

/// The inverse, for a threshold drag.
fn level_db_at(t: f32) -> f32 {
    LEVEL_FLOOR_DB + (LEVEL_CEILING_DB - LEVEL_FLOOR_DB) * t.clamp(0.0, 1.0)
}

/// How deep `db` of reduction hangs into its lane, `0..=1`.
fn reduction_norm(db: f32) -> f32 {
    (db.min(0.0) / REDUCTION_FLOOR_DB).clamp(0.0, 1.0)
}

/// Draw the scope, with the threshold as a rule you can drag.
///
/// Returns the threshold in dBFS when the user moved it, and `None`
/// otherwise. The caller owns the value — this widget never keeps it.
pub fn scope(
    ui: &mut egui::Ui,
    theme: &Theme,
    history: &History,
    threshold_db: f32,
    tag: &str,
) -> Option<f32> {
    let size = egui::vec2(ui.available_width(), ui.available_height());
    let (rect, background) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());

    let split = rect.top() + rect.height() * REDUCTION_SHARE;
    let gr_lane = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), split));
    let level_lane = egui::Rect::from_min_max(egui::pos2(rect.left(), split), rect.max);

    // The threshold rule owns its OWN interaction, so a drag that began
    // on it stays with it however far the pointer wanders — the device
    // UI contract's first rule, and the reason this is not a proximity
    // test against the background's drag.
    let rule_y = level_lane.bottom() - level_lane.height() * level_norm(threshold_db);
    let grab = theme
        .sp(control::HANDLE)
        .max(metrics::interactive_min(ui).y * 0.5);
    let rule_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rule_y - grab),
        egui::pos2(rect.right(), rule_y + grab),
    );
    let handle = ui
        .interact(
            rule_rect,
            background.id.with("threshold"),
            egui::Sense::click_and_drag(),
        )
        .affords(Affords::Sweep);

    let mut moved = None;
    if handle.dragged() {
        let dy = handle.drag_delta().y;
        if dy != 0.0 && level_lane.height() > 0.0 {
            let t = level_norm(threshold_db) - dy / level_lane.height();
            let next = level_db_at(t);
            if next != threshold_db {
                moved = Some(next);
            }
        }
    }
    if handle.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }

    paint(
        ui,
        theme,
        Lanes {
            all: rect,
            gr: gr_lane,
            level: level_lane,
        },
        history,
        moved.unwrap_or(threshold_db),
        tag,
        &handle,
    );
    moved
}

struct Lanes {
    all: egui::Rect,
    gr: egui::Rect,
    level: egui::Rect,
}

fn paint(
    ui: &egui::Ui,
    theme: &Theme,
    lanes: Lanes,
    history: &History,
    threshold_db: f32,
    tag: &str,
    handle: &egui::Response,
) {
    let painter = ui.painter();
    let rect = lanes.all;
    let step = rect.width() / (HISTORY - 1).max(1) as f32;

    // --- the two lanes, told apart ------------------------------------
    //
    // By FILL and a seam, not by a border each: the screen guide's rule
    // is that a well's fill does the grouping and borders around borders
    // are what a dense display must not become. Undifferentiated, the
    // two lanes and their rules read as one striped box, and a reader
    // has no way to know the top half means something else.
    painter.rect_filled(lanes.gr, 0.0, theme.surface_raised);
    painter.line_segment(
        [lanes.level.left_top(), lanes.level.right_top()],
        egui::Stroke::new(stroke::HAIR, theme.outline),
    );

    // --- the grid ------------------------------------------------------
    //
    // Drawn before anything else and present whether or not the engine
    // is running. Without it an idle scope is an empty box, which is a
    // poor thing for the largest object on a card to be: a display with
    // a scale is an instrument at rest, and a display without one is a
    // hole.
    let rule = |lane: egui::Rect, t: f32, label: &str| {
        let y = lane.bottom() - lane.height() * t;
        // A rule within a line of its lane's own edge is the edge, drawn
        // twice — and its label sits half outside the lane. Skip it.
        let room = theme.sp(space::SM);
        if y < lane.top() + room || y > lane.bottom() - room {
            return;
        }
        painter.line_segment(
            [egui::pos2(lane.left(), y), egui::pos2(lane.right(), y)],
            egui::Stroke::new(stroke::HAIR, theme.grid_beat),
        );
        painter.text(
            egui::pos2(lane.right() - theme.sp(space::XS), y),
            egui::Align2::RIGHT_CENTER,
            label,
            egui::FontId::monospace(font::MICRO_LABEL),
            theme.text_muted,
        );
    };
    for db in REDUCTION_RULES {
        rule(lanes.gr, 1.0 - reduction_norm(db), &format!("{db:.0}"));
    }
    for db in LEVEL_RULES {
        rule(lanes.level, level_norm(db), &format!("{db:.0}"));
    }

    // --- the reduction lane: hangs from its own zero rule --------------
    painter.line_segment(
        [lanes.gr.left_top(), lanes.gr.right_top()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );
    for (i, reading) in history.iter().enumerate() {
        let depth = reduction_norm(reading.reduction_db);
        if depth <= 0.0 {
            continue;
        }
        let x = rect.left() + step * i as f32;
        painter.line_segment(
            [
                egui::pos2(x, lanes.gr.top()),
                egui::pos2(x, lanes.gr.top() + lanes.gr.height() * depth),
            ],
            // `role_mod`: the destructive edge. Reduction is the thing
            // being done TO the signal, not a property of it.
            egui::Stroke::new(step.max(stroke::HAIR), theme.role_mod),
        );
    }

    // --- the level lane: fills from the floor --------------------------
    painter.line_segment(
        [lanes.level.left_top(), lanes.level.right_top()],
        egui::Stroke::new(stroke::HAIR, theme.grid_beat),
    );
    for (i, reading) in history.iter().enumerate() {
        let height = level_norm(reading.level_db);
        if height <= 0.0 {
            continue;
        }
        let x = rect.left() + step * i as f32;
        painter.line_segment(
            [
                egui::pos2(x, lanes.level.bottom()),
                egui::pos2(x, lanes.level.bottom() - lanes.level.height() * height),
            ],
            // The dim partner of the level family: this is context for
            // the threshold, not the thing being read.
            egui::Stroke::new(step.max(stroke::HAIR), theme.grid_bar),
        );
    }

    // --- the threshold, over the level it is set against ---------------
    let y = lanes.level.bottom() - lanes.level.height() * level_norm(threshold_db);
    let live = handle.hovered() || handle.dragged();
    let weight = if live { stroke::MARK } else { stroke::BOLD };
    painter.line_segment(
        [
            egui::pos2(lanes.level.left(), y),
            egui::pos2(lanes.level.right(), y),
        ],
        egui::Stroke::new(weight, theme.role_level),
    );
    // A grip at the left edge, so the rule reads as something you can
    // take hold of rather than as the brightest of several grid lines.
    // Shape, not colour alone — the guide's rule, and the difference
    // between a control and a decoration in a grey screenshot.
    let grip = theme.sp(space::XS);
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(lanes.level.left(), y - grip),
            egui::pos2(lanes.level.left() + grip, y),
            egui::pos2(lanes.level.left(), y + grip),
        ],
        theme.role_level,
        egui::Stroke::NONE,
    ));

    // --- the corner tag: values at the display's edge -------------------
    painter.text(
        rect.left_top() + egui::vec2(theme.sp(space::XS), theme.sp(space::XXS)),
        egui::Align2::LEFT_TOP,
        tag,
        egui::FontId::monospace(font::MICRO_LABEL),
        theme.text_muted,
    );
}

fn finite(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v } else { fallback }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ui::device::probe;

    const RECT: egui::Rect = egui::Rect {
        min: egui::pos2(0.0, 0.0),
        max: egui::pos2(400.0, 120.0),
    };

    /// The ring runs oldest to newest and holds exactly its length.
    #[test]
    fn the_history_rolls_oldest_to_newest() {
        let mut history = History::default();
        for i in 0..HISTORY + 40 {
            history.push(Reading {
                level_db: -60.0 + i as f32,
                reduction_db: 0.0,
            });
        }
        let seen: Vec<f32> = history.iter().map(|r| r.level_db).collect();
        assert_eq!(seen.len(), HISTORY);
        assert!(
            seen.windows(2).all(|w| w[1] > w[0]),
            "the ring must read oldest first"
        );
        let newest = -60.0 + (HISTORY + 40 - 1) as f32;
        assert_eq!(history.latest().level_db, newest);
        assert_eq!(*seen.last().unwrap(), newest);
    }

    /// The peak hold keeps the DEEPEST reduction, and lets go when told.
    #[test]
    fn the_peak_holds_the_deepest_reduction() {
        let mut history = History::default();
        for db in [-1.0, -6.5, -2.0, -0.5] {
            history.push(Reading {
                level_db: -12.0,
                reduction_db: db,
            });
        }
        assert_eq!(history.peak_db(), -6.5);
        // A later shallower reading must not raise it.
        history.push(Reading {
            level_db: -12.0,
            reduction_db: 0.0,
        });
        assert_eq!(history.peak_db(), -6.5);
        history.clear_peak();
        assert_eq!(history.peak_db(), 0.0);
    }

    /// A NaN reading cannot lodge in the ring — the engine guards its
    /// side, and a display that trusted it would draw a hole forever.
    #[test]
    fn a_poisoned_reading_is_refused() {
        let mut history = History::default();
        history.push(Reading {
            level_db: f32::NAN,
            reduction_db: f32::NAN,
        });
        assert!(history.latest().level_db.is_finite());
        assert!(history.latest().reduction_db.is_finite());
        assert!(history.peak_db().is_finite());
        // And a positive "reduction" is not a thing: reduction is ≤ 0.
        history.push(Reading {
            level_db: -6.0,
            reduction_db: 12.0,
        });
        assert!(history.latest().reduction_db <= 0.0);
    }

    /// Both scales run the right way up, and their ends are their ends.
    #[test]
    fn the_lanes_scale_the_way_they_are_drawn() {
        assert_eq!(
            level_norm(LEVEL_CEILING_DB),
            1.0,
            "the ceiling is the top of the lane"
        );
        assert_eq!(level_norm(LEVEL_FLOOR_DB), 0.0);
        // Full scale sits BELOW the top, so the threshold has somewhere
        // to be at its default without hiding on the lane's own edge.
        let full = level_norm(0.0);
        assert!(
            full > 0.8 && full < 1.0,
            "0 dBFS should be near the top but not on it: {full:.3}"
        );
        assert!(level_norm(-12.0) > level_norm(-24.0), "louder is higher");
        assert!((level_db_at(level_norm(-18.0)) + 18.0).abs() < 0.01);

        assert_eq!(reduction_norm(0.0), 0.0, "no reduction hangs nothing");
        assert_eq!(reduction_norm(REDUCTION_FLOOR_DB), 1.0);
        assert!(
            reduction_norm(-12.0) > reduction_norm(-3.0),
            "more reduction hangs deeper"
        );
        // Past the floor it pins rather than running off the lane.
        assert_eq!(reduction_norm(-90.0), 1.0);
    }

    /// DRAGGING THE RULE UP RAISES THE THRESHOLD. Screen y grows
    /// downward, so this is the sign that is easy to get backwards and
    /// impossible to see in a screenshot.
    #[test]
    fn dragging_the_threshold_up_raises_it() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let history = History::default();
        let start = -24.0;

        let rule_y =
            |db: f32| RECT.bottom() - RECT.height() * (1.0 - REDUCTION_SHARE) * level_norm(db);
        let from = egui::pos2(RECT.center().x, rule_y(start));

        let run = |to: egui::Pos2| {
            let path = probe::drag_path(from, to, 8);
            let mut latest = start;
            for out in probe::run(&ctx, RECT, &path, |ui| {
                scope(ui, &theme, &history, latest, "gr")
            }) {
                if let Some(db) = out {
                    latest = db;
                }
            }
            latest
        };

        let up = run(egui::pos2(from.x, from.y - 20.0));
        assert!(up > start, "dragging up must raise it: {up:+.1} dB");
        let down = run(egui::pos2(from.x, from.y + 20.0));
        assert!(down < start, "dragging down must lower it: {down:+.1} dB");
    }

    /// A press on the lanes that is NOT on the rule moves nothing — the
    /// scope is a display first, and only the rule is a control.
    #[test]
    fn pressing_the_lanes_away_from_the_rule_moves_nothing() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let history = History::default();
        let threshold = -24.0;
        // The very top of the reduction lane, a long way from the rule.
        let at = egui::pos2(RECT.center().x, RECT.top() + 4.0);
        let moved: Vec<Option<f32>> = probe::run(
            &ctx,
            RECT,
            &probe::drag_path(at, egui::pos2(at.x, at.y + 6.0), 4),
            |ui| scope(ui, &theme, &history, threshold, "gr"),
        );
        assert!(
            moved.iter().all(|m| m.is_none()),
            "the display took a drag that was not on its control"
        );
    }
}
