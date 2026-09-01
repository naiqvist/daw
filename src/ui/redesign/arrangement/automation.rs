//! The automation sublane: the offset model's BASE, addressed by hand.
//!
//! The lane is the arrangement's second projection of a track, the way the
//! roll is the pattern's second projection. Time runs left to right and the
//! vertical axis is the parameter's own span, so a curve reads as a shape.
//!
//! It owns no points. Every breakpoint lives on `sequencing::Track`; the
//! lane only says where the hand is. The cursor addresses (tick, value)
//! exactly as the roll addresses (step, pitch) — which is why the same four
//! motions do the same four things in both, and why neither needed a new
//! verb to become editable.
//!
//! Curves are TRACK-scoped in song time. A pattern placed twice carries the
//! same locks to both placements and reads different curve values at each:
//! locks travel with content, curves belong to the timeline.

use crate::sequencing::{Song, TICKS_PER_BEAT, TRACK_PAN, TRACK_VOLUME};
use crate::ui::redesign::grammar::Motion;
use crate::ui::redesign::verbs::Verb;

/// One motion of value: a sixteenth of the span, so four presses is a
/// quarter and sixteen crosses the lane exactly.
const VALUE_STEP: f32 = 1.0 / 16.0;

/// One motion of time: a beat. The lane travels in musical units, never
/// in pixels.
const TIME_STEP: usize = TICKS_PER_BEAT;

/// The span a target is drawn against, as (min, max).
///
/// The mixer's two targets are known here because they are minted in the
/// library. Device parameters carry their range in `ParamDef`, which lives
/// on the binary side of the bridge — until a registry crosses, an unknown
/// target is drawn against the unit span rather than refused, so the lane
/// is never a dead end.
pub(super) fn span(target: &str) -> (f32, f32) {
    // The real range, from the parameter registry — the same table the
    // legacy lock editor and the device cards read, so a lane draws a
    // device parameter against the span the device actually has rather
    // than against a guessed unit interval.
    if let Some(span) = crate::targets::span_of(target) {
        return span;
    }
    match target {
        TRACK_PAN => (-1.0, 1.0),
        _ => (0.0, 1.0),
    }
}

/// Where the hand sits, as a real value in the target's own unit.
pub(super) fn denormalize(target: &str, normalized: f32) -> f32 {
    let (min, max) = span(target);
    min + (max - min) * normalized.clamp(0.0, 1.0)
}

/// The inverse, for putting an existing point under the cursor.
pub(super) fn normalize(target: &str, value: f32) -> f32 {
    let (min, max) = span(target);
    if (max - min).abs() < f32::EPSILON {
        return 0.0;
    }
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}

pub(super) struct AutomationLane {
    /// Whether the sublane is showing. Closed by default: a lane not under
    /// the hands is steady state, and steady state earns no pixels.
    pub(super) open: bool,
    /// Which parameter this lane draws — a target id, and file format.
    pub(super) target: String,
    pub(super) cursor_tick: usize,
    /// The hand's height as 0..=1 of the target's span. Normalized so one
    /// lane draws any parameter without knowing its unit.
    pub(super) cursor_value: f32,
    /// Said out loud when a gesture cannot mean anything here. A silent
    /// no-op is indistinguishable from a broken key.
    pub(super) refusal: Option<String>,
}

impl Default for AutomationLane {
    fn default() -> Self {
        Self {
            open: false,
            target: TRACK_VOLUME.to_owned(),
            cursor_tick: 0,
            cursor_value: 1.0,
            refusal: None,
        }
    }
}

impl AutomationLane {
    pub(super) fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// Point the lane at another parameter, keeping the time cursor where
    /// the hand left it — the performer is travelling targets, not time.
    pub(super) fn aim(&mut self, target: &str) {
        if self.target != target {
            self.target = target.to_owned();
        }
    }

    /// The value the cursor would write, in the target's own unit.
    pub(super) fn cursor_real(&self) -> f32 {
        denormalize(&self.target, self.cursor_value)
    }

    /// One sentence against this lane.
    ///
    /// A bare motion travels; a verb acts on the spot; `Nudge` plus a
    /// motion moves the point under the cursor. Anything else refuses out
    /// loud rather than doing nothing.
    pub(super) fn speak(
        &mut self,
        song: &mut Song,
        track: usize,
        verb: Option<Verb>,
        motion: Option<Motion>,
        count: usize,
    ) {
        self.refusal = None;
        let count = count.max(1);
        if song.tracks.get(track).is_none() {
            self.refusal = Some("NO TRACK HERE".to_owned());
            return;
        }

        match (verb, motion) {
            // Travel.
            (None, Some(motion)) => self.travel(motion, count),

            // Place or replace a breakpoint under the cursor.
            (Some(Verb::Act), None) => {
                let (target, tick, value) =
                    (self.target.clone(), self.cursor_tick, self.cursor_real());
                song.tracks[track].insert_point(&target, tick, value);
            }

            // Remove the breakpoint under the cursor, or say why not.
            (Some(Verb::Delete), None) => {
                let (target, tick) = (self.target.clone(), self.cursor_tick);
                if !song.tracks[track].remove_point(&target, tick) {
                    self.refusal = Some("DELETE: NOTHING HERE".to_owned());
                }
            }

            // Move the breakpoint under the cursor, and follow it.
            (Some(Verb::Nudge), Some(motion)) => self.nudge(song, track, motion, count),

            // Bow the segment leaving the point under the cursor.
            (Some(Verb::Resize), Some(motion)) => self.bend(song, track, motion, count),

            (Some(other), _) => {
                self.refusal = Some(format!("{}: NOT HERE", verb_name(other)));
            }
            (None, None) => {}
        }
    }

    fn travel(&mut self, motion: Motion, count: usize) {
        match motion {
            Motion::Left => {
                self.cursor_tick = self.cursor_tick.saturating_sub(TIME_STEP * count);
            }
            Motion::Right => {
                self.cursor_tick = self.cursor_tick.saturating_add(TIME_STEP * count);
            }
            Motion::Up => {
                self.cursor_value = (self.cursor_value + VALUE_STEP * count as f32).clamp(0.0, 1.0);
            }
            Motion::Down => {
                self.cursor_value = (self.cursor_value - VALUE_STEP * count as f32).clamp(0.0, 1.0);
            }
        }
    }

    fn nudge(&mut self, song: &mut Song, track: usize, motion: Motion, count: usize) {
        let target = self.target.clone();
        let tick = self.cursor_tick;
        let Some(point) = song.tracks[track]
            .points(&target)
            .iter()
            .find(|point| point.tick == tick)
            .copied()
        else {
            self.refusal = Some("NUDGE: NOTHING HERE".to_owned());
            return;
        };
        song.tracks[track].remove_point(&target, tick);
        match motion {
            Motion::Left | Motion::Right => {
                let moved = if matches!(motion, Motion::Left) {
                    tick.saturating_sub(TIME_STEP * count)
                } else {
                    tick.saturating_add(TIME_STEP * count)
                };
                song.tracks[track].insert_point(&target, moved, point.value);
                self.cursor_tick = moved;
            }
            Motion::Up | Motion::Down => {
                let step = VALUE_STEP * count as f32;
                let normalized = normalize(&target, point.value);
                let moved = if matches!(motion, Motion::Up) {
                    (normalized + step).clamp(0.0, 1.0)
                } else {
                    (normalized - step).clamp(0.0, 1.0)
                };
                song.tracks[track].insert_point(&target, tick, denormalize(&target, moved));
                self.cursor_value = moved;
            }
        }
        // The bend is a property of the point, not of its position.
        song.tracks[track].bend_point(&target, self.cursor_tick, point.bend);
    }

    fn bend(&mut self, song: &mut Song, track: usize, motion: Motion, count: usize) {
        let target = self.target.clone();
        let tick = self.cursor_tick;
        let Some(point) = song.tracks[track]
            .points(&target)
            .iter()
            .find(|point| point.tick == tick)
            .copied()
        else {
            self.refusal = Some("RESIZE: NOTHING HERE".to_owned());
            return;
        };
        let step = 0.125 * count as f32;
        let bend = match motion {
            Motion::Up | Motion::Right => point.bend + step,
            Motion::Down | Motion::Left => point.bend - step,
        };
        song.tracks[track].bend_point(&target, tick, bend);
    }
}

const fn verb_name(verb: Verb) -> &'static str {
    match verb {
        Verb::Act => "ACT",
        Verb::Delete => "DELETE",
        Verb::Yank => "YANK",
        Verb::Put => "PUT",
        Verb::Duplicate => "DUPLICATE",
        Verb::Nudge => "NUDGE",
        Verb::Resize => "RESIZE",
        Verb::Mute => "MUTE",
        Verb::Solo => "SOLO",
        Verb::Rename => "RENAME",
        Verb::Condition => "CONDITION",
        Verb::Search => "SEARCH",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> Song {
        Song::default()
    }

    fn lane() -> AutomationLane {
        AutomationLane::default()
    }

    /// A bare motion travels and never edits, in both axes, and the value
    /// axis stops at the lane's edges instead of running off them.
    #[test]
    fn motion_travels_and_the_value_axis_clamps() {
        let mut song = song();
        let mut lane = lane();
        lane.cursor_value = 0.5;

        lane.speak(&mut song, 0, None, Some(Motion::Right), 2);
        assert_eq!(lane.cursor_tick, TIME_STEP * 2, "two beats right");
        lane.speak(&mut song, 0, None, Some(Motion::Left), 1);
        assert_eq!(lane.cursor_tick, TIME_STEP);
        // Time never goes negative.
        lane.speak(&mut song, 0, None, Some(Motion::Left), 99);
        assert_eq!(lane.cursor_tick, 0);

        lane.speak(&mut song, 0, None, Some(Motion::Up), 99);
        assert_eq!(lane.cursor_value, 1.0, "pinned at the ceiling");
        lane.speak(&mut song, 0, None, Some(Motion::Down), 99);
        assert_eq!(lane.cursor_value, 0.0, "pinned at the floor");

        // Travelling wrote nothing.
        assert!(song.tracks[0].automation.is_empty());
    }

    /// ACT writes the cursor's height as a value in the TARGET'S OWN UNIT,
    /// which is the whole point of addressing a normalized height.
    #[test]
    fn act_writes_the_cursor_in_the_targets_own_unit() {
        let mut song = song();
        let mut lane = lane();
        lane.aim(TRACK_PAN);
        lane.cursor_value = 0.5;
        lane.cursor_tick = 96;

        lane.speak(&mut song, 0, Some(Verb::Act), None, 1);

        let points = song.tracks[0].points(TRACK_PAN);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].tick, 96);
        assert!(
            points[0].value.abs() < 1e-6,
            "half height on a -1..1 span is centre, not 0.5"
        );

        // The same height on the volume lane means something different,
        // and the difference comes from the REGISTRY: track.volume is
        // 0..1.5, so half height is 0.75 rather than 0.5. That is the
        // whole point of addressing a normalized height.
        lane.aim(TRACK_VOLUME);
        lane.speak(&mut song, 0, Some(Verb::Act), None, 1);
        let (min, max) = span(TRACK_VOLUME);
        let half = min + (max - min) * 0.5;
        assert!((song.tracks[0].points(TRACK_VOLUME)[0].value - half).abs() < 1e-6);
        assert!(max > 1.0, "a fader can boost above unity");
    }

    /// Placing twice at one tick is an edit, never a stack.
    #[test]
    fn act_twice_at_one_tick_replaces() {
        let mut song = song();
        let mut lane = lane();
        lane.cursor_value = 1.0;
        lane.speak(&mut song, 0, Some(Verb::Act), None, 1);
        lane.cursor_value = 0.25;
        lane.speak(&mut song, 0, Some(Verb::Act), None, 1);
        assert_eq!(song.tracks[0].points(TRACK_VOLUME).len(), 1);
        let quarter = denormalize(TRACK_VOLUME, 0.25);
        assert!((song.tracks[0].points(TRACK_VOLUME)[0].value - quarter).abs() < 1e-6);
    }

    /// DELETE on silence refuses OUT LOUD. A silent no-op reads exactly
    /// like a broken key — the bug this codebase keeps re-learning.
    #[test]
    fn delete_refuses_out_loud_on_empty_air() {
        let mut song = song();
        let mut lane = lane();
        lane.speak(&mut song, 0, Some(Verb::Delete), None, 1);
        assert_eq!(lane.refusal.as_deref(), Some("DELETE: NOTHING HERE"));

        lane.speak(&mut song, 0, Some(Verb::Act), None, 1);
        lane.speak(&mut song, 0, Some(Verb::Delete), None, 1);
        assert!(lane.refusal.is_none(), "a real point deletes quietly");
        assert!(song.tracks[0].points(TRACK_VOLUME).is_empty());
    }

    /// NUDGE moves the point and the cursor FOLLOWS it — the hand stays on
    /// what it just moved rather than being left behind.
    #[test]
    fn nudge_moves_the_point_and_the_cursor_follows() {
        let mut song = song();
        let mut lane = lane();
        lane.cursor_tick = TIME_STEP;
        lane.cursor_value = 1.0;
        lane.speak(&mut song, 0, Some(Verb::Act), None, 1);

        lane.speak(&mut song, 0, Some(Verb::Nudge), Some(Motion::Right), 2);
        let points = song.tracks[0].points(TRACK_VOLUME);
        assert_eq!(points.len(), 1, "moved, not copied");
        assert_eq!(points[0].tick, TIME_STEP * 3);
        assert_eq!(lane.cursor_tick, TIME_STEP * 3, "the cursor followed");
    }

    /// A nudge in the value axis keeps the point's bend: a bend belongs to
    /// the point, not to where it happens to sit.
    #[test]
    fn a_value_nudge_preserves_the_bend() {
        let mut song = song();
        let mut lane = lane();
        lane.cursor_value = 1.0;
        lane.speak(&mut song, 0, Some(Verb::Act), None, 1);
        song.tracks[0].bend_point(TRACK_VOLUME, 0, 0.5);

        lane.speak(&mut song, 0, Some(Verb::Nudge), Some(Motion::Down), 4);
        let points = song.tracks[0].points(TRACK_VOLUME);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].bend, 0.5, "the bend survived the move");
        // Four steps of a sixteenth is a quarter of the span, whatever
        // the span happens to be.
        let expected = denormalize(TRACK_VOLUME, 0.75);
        assert!((points[0].value - expected).abs() < 1e-6, "four steps down");
    }

    /// RESIZE bows the segment, and both directions are reachable.
    #[test]
    fn resize_bends_the_segment_either_way() {
        let mut song = song();
        let mut lane = lane();
        lane.speak(&mut song, 0, Some(Verb::Act), None, 1);

        lane.speak(&mut song, 0, Some(Verb::Resize), Some(Motion::Up), 2);
        assert!(song.tracks[0].points(TRACK_VOLUME)[0].bend > 0.0);
        lane.speak(&mut song, 0, Some(Verb::Resize), Some(Motion::Down), 4);
        assert!(song.tracks[0].points(TRACK_VOLUME)[0].bend < 0.0);
    }

    /// Refusal coverage: a verb with no meaning here says so by name.
    #[test]
    fn an_inapplicable_verb_names_itself_in_the_refusal() {
        let mut song = song();
        let mut lane = lane();
        for verb in [Verb::Yank, Verb::Put, Verb::Mute, Verb::Rename] {
            lane.speak(&mut song, 0, Some(verb), None, 1);
            let refusal = lane.refusal.as_deref().expect("refuses out loud");
            assert!(
                refusal.starts_with(verb_name(verb)),
                "{refusal} should name {}",
                verb_name(verb)
            );
        }
    }

    /// A gesture aimed at a track that is not there refuses rather than
    /// panicking on the index.
    #[test]
    fn a_missing_track_refuses_instead_of_panicking() {
        let mut song = song();
        let mut lane = lane();
        lane.speak(&mut song, 99, Some(Verb::Act), None, 1);
        assert_eq!(lane.refusal.as_deref(), Some("NO TRACK HERE"));
    }

    /// Normalize and denormalize are inverses across both spans.
    #[test]
    fn the_height_mapping_round_trips() {
        for target in [TRACK_VOLUME, TRACK_PAN] {
            for step in 0..=8 {
                let normalized = step as f32 / 8.0;
                let real = denormalize(target, normalized);
                assert!((normalize(target, real) - normalized).abs() < 1e-6);
            }
        }
    }
}
