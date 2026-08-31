//! The parameter offset-model composition law.
//!
//! Automation and static parameter state share one authority: the base value
//! in the parameter's natural unit. A per-trig lock is an additive offset in
//! that same unit. Composition is always `clamp(base + offset)`; devices do
//! not get to reinterpret it.
//!
//! This is green-zone data arithmetic intended for sequence compilation. It
//! performs no allocation or I/O and rejects non-finite inputs before a value
//! can be baked into an immutable chunk for the audio thread.

use crate::params::ParamDef;

/// Which range edge pinned the effective value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClampState {
    Unclamped,
    Minimum,
    Maximum,
}

/// Why a value cannot cross the green-zone composition boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComposeError {
    InvalidRange,
    NonFiniteBase,
    NonFiniteOffset,
}

/// The complete, honest account of one composed parameter value.
///
/// `base` and `lock_offset` retain the two authorities independently for the
/// UI. `effective` is the finite value safe to bake into compiled sequence
/// data. When clamping occurs, `clamp_state` tells the UI that the requested
/// sum was pinned rather than pretending the lock itself was smaller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Composition {
    base: f32,
    lock_offset: Option<f32>,
    effective: f32,
    clamp_state: ClampState,
}

impl Composition {
    pub const fn base(self) -> f32 {
        self.base
    }

    /// The requested lock, preserving the distinction between no lock and a
    /// present lock whose offset is exactly zero.
    pub const fn lock_offset(self) -> Option<f32> {
        self.lock_offset
    }

    pub const fn effective(self) -> f32 {
        self.effective
    }

    pub const fn clamp_state(self) -> ClampState {
        self.clamp_state
    }

    pub const fn was_clamped(self) -> bool {
        !matches!(self.clamp_state, ClampState::Unclamped)
    }

    /// The part of the audible result above or below the base.
    ///
    /// This equals the requested lock when the sum is not clamped. At a range
    /// edge it differs, while [`Self::lock_offset`] still reports what was
    /// requested and [`Self::clamp_state`] reports why it could not all apply.
    pub fn applied_offset(self) -> f32 {
        self.effective - self.base
    }

    /// Decompose the audible value as `base + applied offset`.
    pub fn audible_decomposition(self) -> (f32, f32) {
        (self.base, self.applied_offset())
    }
}

/// Apply the one universal parameter composition law.
///
/// `base` is either the parameter's static value or an evaluated automation
/// curve value. `lock_offset` is additive in the same natural unit. Bounds,
/// base, and offset must all be finite; invalid green-zone data is rejected
/// rather than passed to the audio thread.
pub fn compose(
    range: &ParamDef,
    base: f32,
    lock_offset: Option<f32>,
) -> Result<Composition, ComposeError> {
    if !range.min.is_finite() || !range.max.is_finite() || range.min > range.max {
        return Err(ComposeError::InvalidRange);
    }
    if !base.is_finite() {
        return Err(ComposeError::NonFiniteBase);
    }

    let offset = lock_offset.unwrap_or(0.0);
    if !offset.is_finite() {
        return Err(ComposeError::NonFiniteOffset);
    }

    // Keep zero a bit-exact identity, including signed zero. Finite addition
    // may overflow to infinity; the comparisons below still pin it to a
    // finite, validated range edge.
    let sum = if offset == 0.0 { base } else { base + offset };
    let (effective, clamp_state) = if sum < range.min {
        (range.min, ClampState::Minimum)
    } else if sum > range.max {
        (range.max, ClampState::Maximum)
    } else {
        (sum, ClampState::Unclamped)
    };

    Ok(Composition {
        base,
        lock_offset,
        effective,
        clamp_state,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT: ParamDef = ParamDef {
        id: 0,
        name: "unit",
        min: 0.0,
        max: 1.0,
        default: 0.5,
    };

    const SIGNED: ParamDef = ParamDef {
        id: 0,
        name: "signed",
        min: -100.0,
        max: 100.0,
        default: 0.0,
    };

    #[test]
    fn offsets_add_in_natural_units_and_clamp_exactly() {
        let value = compose(&UNIT, 0.8, Some(0.5)).expect("finite composition");

        assert_eq!(value.effective(), 1.0);
        assert_eq!(value.clamp_state(), ClampState::Maximum);
        assert!(value.was_clamped());
    }

    #[test]
    fn a_static_base_and_the_same_constant_curve_cannot_revoice_a_lock() {
        let static_value = 0.4;
        let constant_curve_value = static_value;
        let lock = Some(0.25);

        let without_curve = compose(&UNIT, static_value, lock).expect("static base");
        let with_constant_curve =
            compose(&UNIT, constant_curve_value, lock).expect("constant curve base");

        assert_eq!(without_curve, with_constant_curve);
        assert_eq!(without_curve.effective(), 0.65);
    }

    #[test]
    fn display_decomposition_round_trips_and_clamping_stays_visible() {
        let value = compose(&SIGNED, 50.0, Some(12.0)).expect("unclamped sum");
        let (base, applied_offset) = value.audible_decomposition();

        assert_eq!(value.base(), 50.0);
        assert_eq!(value.lock_offset(), Some(12.0));
        assert_eq!(value.clamp_state(), ClampState::Unclamped);
        assert_eq!(base + applied_offset, value.effective());
        assert_eq!(applied_offset, 12.0);

        let pinned = compose(&UNIT, 0.8, Some(0.5)).expect("clamped sum");
        assert_eq!(pinned.base(), 0.8);
        assert_eq!(pinned.lock_offset(), Some(0.5));
        assert_ne!(pinned.applied_offset(), 0.5);
        assert_eq!(pinned.clamp_state(), ClampState::Maximum);
    }

    #[test]
    fn zero_offset_is_a_bit_exact_identity_across_representative_values() {
        for base in [-100.0, -1.0, -0.0, 0.0, f32::EPSILON, 0.5, 100.0] {
            for lock in [None, Some(0.0), Some(-0.0)] {
                let value = compose(&SIGNED, base, lock).expect("finite identity");
                assert_eq!(value.effective().to_bits(), base.to_bits());
                assert_eq!(value.clamp_state(), ClampState::Unclamped);
            }
        }
    }

    #[test]
    fn non_finite_values_and_ranges_are_rejected_at_the_green_zone_boundary() {
        for offset in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(
                compose(&UNIT, 0.5, Some(offset)),
                Err(ComposeError::NonFiniteOffset)
            );
        }
        for base in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(
                compose(&UNIT, base, Some(0.0)),
                Err(ComposeError::NonFiniteBase)
            );
        }

        for (min, max) in [
            (f32::NAN, 1.0),
            (0.0, f32::INFINITY),
            (0.0, f32::NEG_INFINITY),
            (1.0, 0.0),
        ] {
            let invalid = ParamDef { min, max, ..UNIT };
            assert_eq!(
                compose(&invalid, 0.5, None),
                Err(ComposeError::InvalidRange)
            );
        }
    }

    #[test]
    fn composition_allocates_nothing() {
        assert_no_alloc::assert_no_alloc(|| {
            let value = compose(&UNIT, 0.5, Some(0.25)).expect("finite composition");
            assert_eq!(value.effective(), 0.75);
        });
    }
}
