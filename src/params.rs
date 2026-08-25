//! Parameter tables — the wire contract between widget, engine and app.
//!
//! Every knob a node exposes used to be numbered in three places that could
//! not see each other: the device widget's `P_*` consts, the bare literals in
//! `Node::apply` in `src/audio/graph.rs`, and the bare literals in the app's
//! `apply_*_edits`. Nothing connected them, and the failure mode was silent —
//! a renumbering sends a reverb's mix to a synth's gain and nothing fails to
//! compile. (The old `assert_eq!(P_MIX, 0)` test in the reverb widget was
//! this file trying to exist.)
//!
//! Now a device declares its knobs ONCE: id, name, engine-facing range,
//! default. All three sites read this table — the widget for names, ranges
//! and defaults; the engine for its red-zone clamp; the app for its match
//! arms. Adding a device means adding a module here and using its consts
//! everywhere; using a literal id anywhere else is the bug.
//!
//! Invariants (enforced by tests below): within a table, ids are dense and
//! equal to their index — so `TABLE[FOO as usize]` is the definition of
//! `FOO` — names are unique, and every default lies inside its range.

/// One knob of one device, in ENGINE units (Hz, ms, linear gain — never
/// normalized widget positions).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamDef {
    /// The wire id carried by `ParamChange` letters and `ParamEdit`s.
    pub id: u32,
    /// Display name, also the automation-facing name later.
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

impl ParamDef {
    /// Clamp an engine-facing value into this knob's range.
    pub fn clamp(self, value: f32) -> f32 {
        value.clamp(self.min, self.max)
    }
}

/// Red-zone lookup: clamp `value` into the range of `id`, or `None` for an
/// id the table does not know (a stale or misrouted letter — drop it).
/// Linear scan over a static table of at most a handful of entries; no
/// allocation, no panic.
pub fn clamp(table: &'static [ParamDef], id: u32, value: f32) -> Option<f32> {
    table
        .iter()
        .find(|def| def.id == id)
        .map(|def| def.clamp(value))
}

/// Green-zone lookup by id. Panics on an unknown id, which is a compile-time
/// mistake when callers use the named consts — never call this with a
/// runtime-provided id (that is what [`clamp`] is for).
pub fn def(table: &'static [ParamDef], id: u32) -> &'static ParamDef {
    &table[id as usize]
}

/// The built-in sequencer synth (`Node::Seq`, the SineSynth device).
pub mod seq {
    use super::ParamDef;

    pub const GAIN: u32 = 0;
    pub const ATTACK: u32 = 1;
    pub const RELEASE: u32 = 2;

    /// Defaults match the synth's original hardcoded voice: unity gain,
    /// ~1 ms attack, ~640 ms release.
    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: GAIN,
            name: "gain",
            min: 0.0,
            max: 2.0,
            default: 1.0,
        },
        ParamDef {
            id: ATTACK,
            name: "attack",
            min: 0.05,
            max: 5_000.0,
            default: 1.0,
        },
        ParamDef {
            id: RELEASE,
            name: "release",
            min: 1.0,
            max: 30_000.0,
            default: 640.0,
        },
    ];
}

/// `Node::Reverb`.
pub mod reverb {
    use super::ParamDef;

    pub const MIX: u32 = 0;
    pub const SIZE: u32 = 1;
    pub const DAMP: u32 = 2;

    /// Defaults are the widget's original medium room: audible, never a
    /// wash — loading a reverb must not drown the track it lands on.
    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: MIX,
            name: "mix",
            min: 0.0,
            max: 1.0,
            default: 0.25,
        },
        ParamDef {
            id: SIZE,
            name: "size",
            min: 0.0,
            max: 1.0,
            default: 0.60,
        },
        ParamDef {
            id: DAMP,
            name: "damp",
            min: 0.0,
            max: 1.0,
            default: 0.40,
        },
    ];
}

/// `Node::Sine`, the lab's tone generator.
pub mod sine {
    use super::ParamDef;

    pub const FREQ: u32 = 0;
    pub const AMP: u32 = 1;

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: FREQ,
            name: "freq",
            min: 1.0,
            max: 20_000.0,
            default: 440.0,
        },
        ParamDef {
            id: AMP,
            name: "amp",
            min: 0.0,
            max: 1.0,
            default: 0.5,
        },
    ];
}

/// `Node::Mixer`.
pub mod mixer {
    use super::ParamDef;

    pub const GAIN: u32 = 0;

    pub const TABLE: &[ParamDef] = &[ParamDef {
        id: GAIN,
        name: "gain",
        min: 0.0,
        max: 2.0,
        default: 1.0,
    }];
}

/// `Node::Pan`.
pub mod pan {
    use super::ParamDef;

    pub const PAN: u32 = 0;

    pub const TABLE: &[ParamDef] = &[ParamDef {
        id: PAN,
        name: "pan",
        min: -1.0,
        max: 1.0,
        default: 0.0,
    }];
}

/// `Node::AudioClip`.
pub mod clip {
    use super::ParamDef;

    pub const GAIN: u32 = 0;

    pub const TABLE: &[ParamDef] = &[ParamDef {
        id: GAIN,
        name: "gain",
        min: 0.0,
        max: 2.0,
        default: 1.0,
    }];
}

/// `Node::Filter` — the first kernel-backed effect. Beyond the knob table
/// this module owns the numbers BOTH the drawn curve and the audio path
/// derive from, so the display cannot drift from the sound: the resonance
/// mapping, the drive squash, and the slope list.
pub mod filter {
    use super::ParamDef;

    pub const MODE: u32 = 0;
    pub const SLOPE: u32 = 1;
    pub const CUTOFF: u32 = 2;
    pub const RES: u32 = 3;
    pub const DRIVE: u32 = 4;

    /// Mode indices on the wire. Same order as the widget's mode strip.
    pub const MODE_LP: u32 = 0;
    pub const MODE_HP: u32 = 1;
    pub const MODE_BP: u32 = 2;
    pub const MODE_NOTCH: u32 = 3;

    /// Butterworth-flat: no peak at the corner. The resonance knob's
    /// audible floor — below this the resonant section stays flat, in the
    /// drawing and in the audio alike.
    pub const FLAT_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

    /// How hard drive squashes resonance. The saturation lives in the
    /// feedback path, so at full drive a peak that would be `1 + x` above
    /// flat is squashed to `1 + x / (1 + DRIVE_SQUASH)`.
    pub const DRIVE_SQUASH: f32 = 3.0;

    /// Slope index -> filter order (poles). 6 dB per octave per pole.
    pub const SLOPE_ORDERS: &[u32] = &[1, 2, 3, 4, 6, 8];

    pub const TABLE: &[ParamDef] = &[
        ParamDef {
            id: MODE,
            name: "mode",
            min: 0.0,
            max: 3.0,
            default: 0.0, // lowpass
        },
        ParamDef {
            id: SLOPE,
            name: "slope",
            min: 0.0,
            max: 5.0,
            default: 3.0, // 24 dB/octave
        },
        ParamDef {
            id: CUTOFF,
            name: "cutoff",
            min: 20.0,
            max: 20_000.0,
            // Parked out of the way: loading a filter is transparent, and
            // the first knob touch is the first audible change — the
            // reverb's never-destroy-the-mix rule, applied to a filter.
            default: 20_000.0,
        },
        ParamDef {
            id: RES,
            name: "res",
            min: 0.3,
            // Just below scream: +24 dB or so of peak, matching the top of
            // the display window. Self-oscillation is a feature for a
            // later version that has a limiter behind it.
            max: 24.0,
            default: FLAT_Q,
        },
        ParamDef {
            id: DRIVE,
            name: "drive",
            min: 0.0,
            max: 1.0,
            default: 0.0,
        },
    ];

    /// The resonant section's effective Q after drive has had its say —
    /// the ONE resonance mapping. Drive squashes only the part of the
    /// resonance above flat, because that is the part living in the
    /// feedback path where the saturation is; a gentle filter driven hard
    /// does not lose its corner. Sub-flat requests floor at flat, which
    /// keeps the drawn curve and the audio identical there too.
    ///
    /// The engine feeds this straight to its resonant section; the display
    /// scales it by the section's Butterworth base via [`resonant_q`].
    /// Same function, so agreement is by construction, not by test alone.
    pub fn effective_q(q: f32, drive: f32) -> f32 {
        let excess = (q.max(0.05) - FLAT_Q).max(0.0);
        FLAT_Q + excess / (1.0 + drive.clamp(0.0, 1.0) * DRIVE_SQUASH)
    }

    /// [`effective_q`] expressed against a cascade section's Butterworth
    /// base — the form the display's per-section math consumes.
    pub fn resonant_q(q: f32, base: f32, drive: f32) -> f32 {
        base * (effective_q(q, drive) / FLAT_Q)
    }

    /// Drive knob (0..=1) -> waveshaper drive. Full drive pushes ~24 dB
    /// into the soft clipper — growl territory, not bitcrush.
    pub fn shaper_drive(drive: f32) -> f32 {
        1.0 + drive.clamp(0.0, 1.0) * 15.0
    }

    /// Slope index -> order, clamped to the steepest available.
    pub fn slope_order(index: u32) -> u32 {
        SLOPE_ORDERS[(index as usize).min(SLOPE_ORDERS.len() - 1)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[(&str, &[ParamDef])] = &[
        ("seq", seq::TABLE),
        ("reverb", reverb::TABLE),
        ("sine", sine::TABLE),
        ("mixer", mixer::TABLE),
        ("pan", pan::TABLE),
        ("clip", clip::TABLE),
        ("filter", filter::TABLE),
    ];

    /// The invariant `def()` and every `TABLE[FOO as usize]` rely on.
    #[test]
    fn ids_are_dense_and_equal_to_their_index() {
        for (device, table) in ALL {
            for (i, p) in table.iter().enumerate() {
                assert_eq!(p.id, i as u32, "{device}:{}", p.name);
            }
        }
    }

    #[test]
    fn names_are_unique_within_a_device() {
        for (device, table) in ALL {
            for (i, a) in table.iter().enumerate() {
                for b in &table[i + 1..] {
                    assert_ne!(a.name, b.name, "{device}");
                }
            }
        }
    }

    #[test]
    fn ranges_are_ordered_and_defaults_lie_inside_them() {
        for (device, table) in ALL {
            for p in *table {
                assert!(p.min < p.max, "{device}:{}", p.name);
                assert!(
                    (p.min..=p.max).contains(&p.default),
                    "{device}:{} default {} outside [{}, {}]",
                    p.name,
                    p.default,
                    p.min,
                    p.max
                );
            }
        }
    }

    /// The one resonance mapping: flat stays flat at any drive, drive
    /// squashes the excess by exactly 1 + DRIVE_SQUASH at full tilt, and
    /// sub-flat requests floor at flat.
    #[test]
    fn effective_q_squashes_excess_and_floors_at_flat() {
        use filter::{DRIVE_SQUASH, FLAT_Q, effective_q};
        assert_eq!(effective_q(FLAT_Q, 0.0), FLAT_Q);
        assert_eq!(effective_q(FLAT_Q, 1.0), FLAT_Q);
        assert_eq!(effective_q(0.3, 0.7), FLAT_Q);
        let clean = effective_q(8.0, 0.0) - FLAT_Q;
        let driven = effective_q(8.0, 1.0) - FLAT_Q;
        assert!((clean / driven - (1.0 + DRIVE_SQUASH)).abs() < 1e-5);
    }

    /// The red-zone lookup: known ids clamp, unknown ids drop.
    #[test]
    fn clamp_bounds_known_ids_and_refuses_unknown_ones() {
        assert_eq!(clamp(reverb::TABLE, reverb::MIX, 2.0), Some(1.0));
        assert_eq!(clamp(reverb::TABLE, reverb::MIX, -1.0), Some(0.0));
        assert_eq!(clamp(pan::TABLE, pan::PAN, 0.3), Some(0.3));
        assert_eq!(clamp(reverb::TABLE, 99, 0.5), None);
    }
}
