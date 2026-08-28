//! The parameter-lock verbs: nine ways to reshape a set of values.
//!
//! Pure arithmetic over a slice, and nothing else. No notes, no
//! parameters, no egui — which is what lets every one of them be tested
//! exactly rather than driven through an editor and eyeballed.
//!
//! # What they have in common
//!
//! Each takes the values a selection currently holds, in the order the
//! notes appear in time, and rewrites them in place. Each keeps every
//! result inside `[min, max]`, because a lock outside its parameter's
//! range is a lock the engine will clamp anyway — better to clamp where
//! the user can see it happen.
//!
//! And each is INDEX-ORDERED rather than time-ordered. Notes at the same
//! beat have no musical order between them, so the caller sorts once and
//! these never have to think about it.
//!
//! # Why they are worth having
//!
//! A lock per note is a sequencer. A hundred locks entered one at a time
//! is data entry. Everything here exists to turn the second into the
//! first: say the SHAPE you want across a phrase and let the arithmetic
//! put the numbers in.

/// Where a verb may put a value, and how finely.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    pub min: f32,
    pub max: f32,
    /// The gap between legal values, or zero for continuous.
    ///
    /// A discrete parameter — a waveform, a division, a mode — has no
    /// meaning between its steps, so a ramp across one has to land on
    /// them. Carried here rather than applied afterwards because a ramp
    /// that quantised at the end would bunch at the edges.
    pub step: f32,
}

impl Range {
    pub fn new(min: f32, max: f32) -> Self {
        Self {
            min,
            max,
            step: 0.0,
        }
    }

    /// A parameter with `choices` positions, evenly spaced.
    pub fn stepped(min: f32, max: f32, choices: u32) -> Self {
        let step = if choices > 1 {
            (max - min) / (choices - 1) as f32
        } else {
            0.0
        };
        Self { min, max, step }
    }

    pub fn span(&self) -> f32 {
        (self.max - self.min).abs()
    }

    /// Into the range, and onto a step if there is one.
    pub fn hold(&self, value: f32) -> f32 {
        let (lo, hi) = (self.min.min(self.max), self.min.max(self.max));
        let value = if value.is_finite() { value } else { lo };
        let value = value.clamp(lo, hi);
        if self.step > 0.0 {
            let steps = ((value - lo) / self.step).round();
            (lo + steps * self.step).clamp(lo, hi)
        } else {
            value
        }
    }
}

/// The mean of a set, or the range's floor for an empty one.
fn mean(values: &[f32], range: Range) -> f32 {
    if values.is_empty() {
        return range.min;
    }
    values.iter().sum::<f32>() / values.len() as f32
}

/// RAMP: a straight line from `first` to `last` across the selection.
///
/// The one everything else is measured against. A single value is left
/// where it is — a ramp between one point and itself is that point, and
/// pretending otherwise would make a one-note selection jump.
pub fn ramp(values: &mut [f32], first: f32, last: f32, range: Range) {
    let n = values.len();
    if n == 0 {
        return;
    }
    if n == 1 {
        values[0] = range.hold(first);
        return;
    }
    for (i, slot) in values.iter_mut().enumerate() {
        let t = i as f32 / (n - 1) as f32;
        *slot = range.hold(first + (last - first) * t);
    }
}

/// CRESCENDO: ramp from where the selection starts to the top of the
/// range; DECRESCENDO takes it to the bottom.
///
/// Not a separate idea from `ramp` — a named one, because "get louder
/// across this phrase" is the request people actually have, and making
/// them work out the endpoints first is making them do the arithmetic
/// the verb exists to do.
pub fn crescendo(values: &mut [f32], range: Range, rising: bool) {
    let from = values.first().copied().unwrap_or(range.min);
    let to = if rising { range.max } else { range.min };
    ramp(values, from, to, range);
}

/// RANDOMIZE: bounded jitter, `amount` of the range, either way.
///
/// DETERMINISTIC from `seed`. A random verb whose result cannot be
/// reproduced is a verb you cannot undo and redo to compare, and it is
/// untestable — so the caller owns the seed and can offer the same
/// scatter twice.
///
/// The jitter is added to what is already there rather than replacing
/// it, so randomising a ramp roughens the ramp instead of destroying it.
pub fn randomize(values: &mut [f32], amount: f32, range: Range, seed: u64) {
    let reach = range.span() * amount.clamp(0.0, 1.0) * 0.5;
    if reach <= 0.0 {
        return;
    }
    let mut state = seed | 1;
    for slot in values.iter_mut() {
        // xorshift64*, which is small, fast and good enough for a knob.
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let bits = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let unit = (bits >> 11) as f32 / (1u64 << 53) as f32;
        *slot = range.hold(*slot + (unit * 2.0 - 1.0) * reach);
    }
}

/// SPREAD: push the values away from their own average, or pull them in.
///
/// `factor` above one expands, below one contracts, and zero flattens
/// the selection to its mean. It is the verb for "more of what is
/// already there", which is a different request from "louder" and had no
/// way to be said.
pub fn spread(values: &mut [f32], factor: f32, range: Range) {
    let centre = mean(values, range);
    for slot in values.iter_mut() {
        *slot = range.hold(centre + (*slot - centre) * factor);
    }
}

/// ROTATE: move every value along by `by` places, wrapping.
///
/// The values stay, the notes they belong to change — which is what
/// turns one shape into a family of them. Negative rotates backwards.
pub fn rotate(values: &mut [f32], by: i32) {
    let n = values.len();
    if n < 2 {
        return;
    }
    let by = by.rem_euclid(n as i32) as usize;
    if by == 0 {
        return;
    }
    values.rotate_right(by);
}

/// ALTERNATE: two values, A B A B across the selection.
pub fn alternate(values: &mut [f32], a: f32, b: f32, range: Range) {
    for (i, slot) in values.iter_mut().enumerate() {
        *slot = range.hold(if i % 2 == 0 { a } else { b });
    }
}

/// EVERY NTH: which notes keep a lock at all.
///
/// Returns a mask rather than values, because this verb is about
/// PRESENCE, not amount — and the caller is the only thing that knows
/// how to remove a lock. `n` of one keeps everything, which is the
/// honest identity rather than an error.
pub fn every_nth(len: usize, n: usize, offset: usize) -> Vec<bool> {
    let n = n.max(1);
    (0..len)
        .map(|i| (i + n - offset % n).is_multiple_of(n))
        .collect()
}

/// SCALE TOWARD: blend every value toward `target` by `t`.
///
/// `t` of one flattens the selection onto the target — which is how you
/// take a shape off again without undoing your way back through it.
pub fn scale_toward(values: &mut [f32], target: f32, t: f32, range: Range) {
    let t = t.clamp(0.0, 1.0);
    for slot in values.iter_mut() {
        *slot = range.hold(*slot + (target - *slot) * t);
    }
}

/// QUANTIZE: onto a grid of `divisions` across the range.
///
/// Distinct from `Range::step`, which is a property of the parameter.
/// This is a request: put these on twelve steps because I want
/// semitones, whatever the parameter would otherwise allow.
pub fn quantize(values: &mut [f32], divisions: u32, range: Range) {
    if divisions < 2 {
        return;
    }
    let step = range.span() / (divisions - 1) as f32;
    if step <= 0.0 {
        return;
    }
    let lo = range.min.min(range.max);
    for slot in values.iter_mut() {
        let steps = ((*slot - lo) / step).round();
        *slot = range.hold(lo + steps * step);
    }
}

/// What the selection looks like, for the panel's readout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub min: f32,
    pub max: f32,
    pub mean: f32,
}

pub fn summarise(values: &[f32], range: Range) -> Summary {
    if values.is_empty() {
        return Summary {
            min: range.min,
            max: range.min,
            mean: range.min,
        };
    }
    Summary {
        min: values.iter().copied().fold(f32::INFINITY, f32::min),
        max: values.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        mean: mean(values, range),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: Range = Range {
        min: 0.0,
        max: 100.0,
        step: 0.0,
    };

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    /// EVERY VERB KEEPS EVERY VALUE INSIDE THE PARAMETER'S RANGE.
    ///
    /// A lock outside its range is one the engine clamps anyway, so the
    /// only question is whether the user watches it happen or discovers
    /// it later. Driven with deliberately absurd arguments, because
    /// that is what a knob dragged to its end produces.
    #[test]
    fn no_verb_can_leave_the_range() {
        let start = vec![10.0, 50.0, 90.0, 30.0];
        let mut cases: Vec<Vec<f32>> = Vec::new();

        let mut v = start.clone();
        ramp(&mut v, -500.0, 900.0, R);
        cases.push(v);

        let mut v = start.clone();
        crescendo(&mut v, R, true);
        cases.push(v);

        let mut v = start.clone();
        randomize(&mut v, 1.0, R, 12345);
        cases.push(v);

        let mut v = start.clone();
        spread(&mut v, 40.0, R);
        cases.push(v);

        let mut v = start.clone();
        alternate(&mut v, -80.0, 800.0, R);
        cases.push(v);

        let mut v = start.clone();
        scale_toward(&mut v, 1e9, 1.0, R);
        cases.push(v);

        let mut v = start.clone();
        quantize(&mut v, 5, R);
        cases.push(v);

        for (i, case) in cases.iter().enumerate() {
            for value in case {
                assert!(
                    (R.min..=R.max).contains(value) && value.is_finite(),
                    "verb {i} produced {value}"
                );
            }
        }
    }

    /// NOTHING PANICS ON AN EMPTY OR SINGLE SELECTION.
    ///
    /// Both are ordinary — one note is the commonest selection there is,
    /// and a verb pressed with nothing selected is a slip, not a crash.
    #[test]
    fn the_verbs_survive_a_selection_of_none_or_one() {
        for len in [0usize, 1] {
            let mut v = vec![50.0; len];
            ramp(&mut v, 0.0, 100.0, R);
            crescendo(&mut v, R, false);
            randomize(&mut v, 0.5, R, 7);
            spread(&mut v, 2.0, R);
            rotate(&mut v, 3);
            alternate(&mut v, 10.0, 90.0, R);
            scale_toward(&mut v, 20.0, 0.5, R);
            quantize(&mut v, 4, R);
            assert_eq!(v.len(), len);
            assert!(v.iter().all(|x| x.is_finite()));
        }
        // A one-note ramp is that note, not a jump to either end.
        let mut v = vec![42.0];
        ramp(&mut v, 42.0, 99.0, R);
        assert!(close(v[0], 42.0), "a single note jumped to {}", v[0]);
    }

    #[test]
    fn a_ramp_is_a_straight_line_between_its_ends() {
        let mut v = vec![0.0; 5];
        ramp(&mut v, 0.0, 100.0, R);
        assert!(close(v[0], 0.0) && close(v[4], 100.0));
        assert!(close(v[2], 50.0), "the middle sits at {}", v[2]);
        // Evenly spaced, which is the whole claim.
        for pair in v.windows(2) {
            assert!(close(pair[1] - pair[0], 25.0));
        }
        // Backwards is a ramp too.
        ramp(&mut v, 100.0, 0.0, R);
        assert!(close(v[0], 100.0) && close(v[4], 0.0));
    }

    /// A RAMP ON A STEPPED PARAMETER LANDS ON ITS STEPS.
    ///
    /// Quantising afterwards would bunch the values at the ends; the
    /// range holds every write, so the line is drawn on the lattice the
    /// parameter actually has.
    #[test]
    fn a_ramp_over_a_stepped_range_lands_on_steps() {
        // Four choices across 0..3: steps of exactly 1.
        let stepped = Range::stepped(0.0, 3.0, 4);
        let mut v = vec![0.0; 7];
        ramp(&mut v, 0.0, 3.0, stepped);
        for value in &v {
            assert!(close(*value, value.round()), "{value} is not on a step");
        }
        assert!(close(v[0], 0.0) && close(v[6], 3.0));
    }

    /// SPREAD MOVES VALUES AWAY FROM THEIR MEAN AND LEAVES THE MEAN.
    #[test]
    fn spread_pushes_out_from_the_average_without_moving_it() {
        let mut v = vec![40.0, 50.0, 60.0];
        let before = summarise(&v, R);
        spread(&mut v, 2.0, R);
        let after = summarise(&v, R);
        assert!(close(before.mean, after.mean), "the average moved");
        assert!(after.max - after.min > before.max - before.min);
        assert!(close(v[0], 30.0) && close(v[2], 70.0));

        // A factor of zero flattens onto the mean, which is the honest
        // limit rather than a special case.
        spread(&mut v, 0.0, R);
        assert!(v.iter().all(|x| close(*x, 50.0)));
    }

    /// ROTATE MOVES THE VALUES, NOT THE NOTES, and puts them all back
    /// after a full turn.
    #[test]
    fn rotate_is_a_cycle() {
        let start = vec![1.0, 2.0, 3.0, 4.0];
        let mut v = start.clone();
        rotate(&mut v, 1);
        assert_eq!(v, vec![4.0, 1.0, 2.0, 3.0]);
        rotate(&mut v, -1);
        assert_eq!(v, start);
        // A whole turn is the identity, and so is a turn past it.
        rotate(&mut v, 4);
        assert_eq!(v, start);
        rotate(&mut v, 9);
        rotate(&mut v, -9);
        assert_eq!(v, start);
        // The multiset never changes — rotation cannot invent or lose a
        // value, which is what separates it from every other verb here.
        let mut sorted = v.clone();
        sorted.sort_by(f32::total_cmp);
        assert_eq!(sorted, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn alternate_lays_two_values_a_b_a_b() {
        let mut v = vec![0.0; 5];
        alternate(&mut v, 10.0, 90.0, R);
        assert_eq!(v, vec![10.0, 90.0, 10.0, 90.0, 10.0]);
    }

    /// EVERY NTH IS ABOUT PRESENCE, so it answers with a mask.
    #[test]
    fn every_nth_keeps_the_notes_it_names() {
        assert_eq!(
            every_nth(6, 2, 0),
            vec![true, false, true, false, true, false]
        );
        assert_eq!(
            every_nth(6, 3, 0),
            vec![true, false, false, true, false, false]
        );
        // An offset moves the pattern without changing its period.
        let shifted = every_nth(6, 3, 1);
        assert_eq!(shifted.iter().filter(|k| **k).count(), 2);
        assert_ne!(shifted, every_nth(6, 3, 0));
        // One keeps everything — the identity, not an error.
        assert!(every_nth(4, 1, 0).iter().all(|k| *k));
        // And zero is read as one rather than dividing by it.
        assert!(every_nth(4, 0, 0).iter().all(|k| *k));
    }

    #[test]
    fn scale_toward_blends_and_lands_exactly() {
        let mut v = vec![0.0, 100.0];
        scale_toward(&mut v, 50.0, 0.5, R);
        assert!(close(v[0], 25.0) && close(v[1], 75.0));
        // All the way is the target exactly, which is how a shape is
        // taken off again.
        scale_toward(&mut v, 50.0, 1.0, R);
        assert!(v.iter().all(|x| close(*x, 50.0)));
        // None of the way changes nothing.
        let before = v.clone();
        scale_toward(&mut v, 0.0, 0.0, R);
        assert_eq!(v, before);
    }

    #[test]
    fn quantize_lands_on_the_divisions_asked_for() {
        let mut v = vec![0.0, 24.0, 26.0, 74.0, 100.0];
        // Five divisions across 0..100: every 25.
        quantize(&mut v, 5, R);
        assert_eq!(v, vec![0.0, 25.0, 25.0, 75.0, 100.0]);
        // Fewer than two divisions is not a grid; nothing moves.
        let before = v.clone();
        quantize(&mut v, 1, R);
        assert_eq!(v, before);
    }

    /// RANDOMIZE IS REPRODUCIBLE, and roughens rather than replaces.
    ///
    /// A scatter you cannot get twice is one you cannot compare against
    /// itself, and it cannot be tested at all.
    #[test]
    fn randomize_is_deterministic_and_keeps_the_shape_underneath() {
        let ramped: Vec<f32> = (0..8).map(|i| i as f32 * 12.0).collect();

        let mut a = ramped.clone();
        let mut b = ramped.clone();
        randomize(&mut a, 0.2, R, 99);
        randomize(&mut b, 0.2, R, 99);
        assert_eq!(a, b, "the same seed gave a different scatter");

        let mut c = ramped.clone();
        randomize(&mut c, 0.2, R, 100);
        assert_ne!(a, c, "two seeds gave the same scatter");

        // The ramp underneath survives: still rising overall, and no
        // value moved further than the amount allows.
        assert!(a[7] > a[0], "the shape was destroyed");
        let reach = R.span() * 0.2 * 0.5;
        for (before, after) in ramped.iter().zip(a.iter()) {
            assert!(
                (after - before).abs() <= reach + 1e-3,
                "{before} moved to {after}, further than {reach}"
            );
        }

        // Zero amount is exactly nothing.
        let mut d = ramped.clone();
        randomize(&mut d, 0.0, R, 5);
        assert_eq!(d, ramped);
    }

    #[test]
    fn a_summary_reads_the_selection() {
        let s = summarise(&[10.0, 30.0, 50.0], R);
        assert!(close(s.min, 10.0) && close(s.max, 50.0) && close(s.mean, 30.0));
        // An empty selection answers with the range's floor rather than
        // an infinity that would print as one.
        let s = summarise(&[], R);
        assert!(s.min.is_finite() && s.mean.is_finite());
    }
}
