//! Offline physical instruments. All work and owned render data stay green.
//! Only immutable finished samples cross the existing audition/material door.
pub mod files;
pub mod indices;
pub mod job;
pub mod membrane;
pub mod params;
use params::LabParam;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Patch {
    pub sliders: Vec<f32>,
    pub macros: [f32; 16],
    #[serde(default)]
    pub by_hand: Vec<bool>,
}
impl Default for Patch {
    fn default() -> Self {
        Self {
            sliders: params::MEMBRANE_SLIDERS.iter().map(|p| p.default).collect(),
            macros: [0.5; 16],
            by_hand: vec![false; params::MEMBRANE_SLIDERS.len()],
        }
    }
}
impl Patch {
    #[inline(always)]
    pub fn get(&self, i: usize) -> f64 {
        params::MEMBRANE_SLIDERS.get(i).map_or(0.0, |p| {
            let x = self
                .sliders
                .get(i)
                .copied()
                .filter(|v| v.is_finite())
                .unwrap_or(p.default);
            f64::from(x.clamp(p.min, p.max))
        })
    }
    pub fn set(&mut self, i: usize, value: f32) {
        if let Some(p) = params::MEMBRANE_SLIDERS.get(i) {
            self.sliders.resize(params::MEMBRANE_SLIDERS.len(), 0.0);
            self.by_hand.resize(self.sliders.len(), false);
            self.sliders[i] = if value.is_finite() {
                value.clamp(p.min, p.max)
            } else {
                p.default
            };
            self.by_hand[i] = true;
        }
    }
    /// Re-evaluating a macro preserves hand edits. An explicit TURN retakes
    /// ownership of its rows; other macros' hand edits stay untouched.
    pub fn apply_macro(&mut self, index: usize, turn: bool) {
        if let Some(table) = MACROS.get(index) {
            let m = self.macros[index].clamp(0.0, 1.0);
            self.by_hand.resize(self.sliders.len(), false);
            for &(i, from, to, curve) in table.rows {
                if i >= self.sliders.len() {
                    continue;
                }
                if turn {
                    self.by_hand[i] = false;
                }
                if !self.by_hand[i] {
                    self.sliders[i] = from + (to - from) * curve.at(m);
                }
            }
        }
    }
    pub fn turn_macro(&mut self, index: usize, value: f32) {
        if let Some(m) = self.macros.get_mut(index) {
            *m = if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                0.5
            };
        }
        self.apply_macro(index, true);
    }
    /// Stable full key, including engine version, note, velocity and rate.
    pub fn key(&self, note: u8, vel: u8, rate: u32) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        let words = std::iter::once(1u32)
            .chain([u32::from(note), u32::from(vel), rate])
            .chain((0..params::MEMBRANE_SLIDERS.len()).map(|i| (self.get(i) as f32).to_bits()))
            .chain(self.macros.iter().map(|m| m.to_bits()));
        for w in words {
            for b in w.to_le_bytes() {
                hash = (hash ^ u64::from(b)).wrapping_mul(0x100000001b3);
            }
        }
        hash
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Curve {
    Linear,
    Square,
    Smooth,
}
impl Curve {
    pub fn at(self, x: f32) -> f32 {
        match self {
            Self::Linear => x,
            Self::Square => x * x,
            Self::Smooth => x * x * (3.0 - 2.0 * x),
        }
    }
}
pub struct Macro {
    pub name: &'static str,
    pub rows: &'static [(usize, f32, f32, Curve)],
}
use Curve::{Linear as L, Smooth as S, Square as Q};
use indices::*;
pub const MACROS: &[Macro] = &[
    Macro {
        name: "SIZE",
        rows: &[
            (BATTER_RADIUS, 0.10, 0.38, S),
            (RESONANT_RADIUS, 0.10, 0.38, S),
            (BATTER_DENSITY, 0.18, 0.55, L),
            (RESONANT_DENSITY, 0.12, 0.38, L),
            (BATTER_TENSION, 4200., 1800., L),
            (RESONANT_TENSION, 3300., 1300., L),
            (SHELL_CAVITY_STIFFNESS, 0.7, 0.08, L),
            (SHELL_SHELL_HEIGHT, 0.07, 0.42, L),
            (BATTER_LOSS_LOW, 2., 0.5, L),
            (RESONANT_LOSS_LOW, 2.2, 0.6, L),
        ],
    },
    Macro {
        name: "TENSION",
        rows: &[
            (BATTER_TENSION, 500., 7500., Q),
            (RESONANT_TENSION, 500., 8000., Q),
        ],
    },
    Macro {
        name: "MATERIAL",
        rows: &[
            (BATTER_STIFFNESS, 0., 1., Q),
            (RESONANT_STIFFNESS, 0., 0.8, Q),
            (BATTER_AIR_LOADING, 0., 0.9, S),
            (RESONANT_AIR_LOADING, 0., 0.9, S),
            (BATTER_LOSS_SLOPE, 0.4, 1.7, L),
            (RESONANT_LOSS_SLOPE, 0.4, 1.7, L),
        ],
    },
    Macro {
        name: "STRIKE",
        rows: &[
            (STRIKE_MALLET_MASS, 0.065, 0.012, L),
            (STRIKE_CONTACT_STIFFNESS, 7., 10., L),
            (STRIKE_HARDNESS, 3., 1.3, L),
            (STRIKE_CONTACT_LOSS, 0.8, 0.02, L),
        ],
    },
    Macro {
        name: "WHERE",
        rows: &[(STRIKE_POSITION, 0., 0.96, L)],
    },
    Macro {
        name: "MUFFLE",
        rows: &[
            (MUFFLE_RING_WIDTH, 0., 0.4, Q),
            (MUFFLE_RING_POSITION, 0.95, 0.72, L),
            (MUFFLE_PATCH_STRENGTH, 0., 1., Q),
            (MUFFLE_RIM_DAMPING, 0., 0.8, L),
        ],
    },
    Macro {
        name: "WIRES",
        rows: &[
            (WIRES_COUNT, 0., 24., L),
            (WIRES_CONTACT_STIFFNESS, 8., 13., L),
            (WIRES_STRAINER, 0., 1., L),
        ],
    },
    Macro {
        name: "SNAP",
        rows: &[
            (WIRES_TENSION, 2., 90., Q),
            (WIRES_REST_GAP, 0.0012, 0., L),
            (WIRES_CONTACT_DAMPING, 0.05, 0.8, L),
        ],
    },
    Macro {
        name: "SHELL",
        rows: &[
            (SHELL_SHELL_STIFFNESS, 0., 1., L),
            (SHELL_SHELL_MODES, 0., 8., L),
            (SHELL_HEAD_COUPLING, 0., 0.9, L),
        ],
    },
    Macro {
        name: "AIR",
        rows: &[
            (SHELL_CAVITY_STIFFNESS, 0., 1., Q),
            (SHELL_PORT, 1., 0., L),
            (SHELL_AIR_DAMPING, 0., 0.8, L),
        ],
    },
    Macro {
        name: "MIC",
        rows: &[(MIC_MIC_DISTANCE, 2., 0.04, S), (MIC_PROXIMITY, 0., 1., L)],
    },
    Macro {
        name: "ROOM",
        rows: &[(MIC_ROOM_SIZE, 0., 1., L), (MIC_ROOM_MIX, 0., 0.7, Q)],
    },
    Macro {
        name: "DECAY",
        rows: &[
            (BATTER_LOSS_LOW, 7., 0.15, L),
            (RESONANT_LOSS_LOW, 8., 0.2, L),
            (BATTER_LOSS_HIGH, 0.003, 0.00004, L),
            (RESONANT_LOSS_HIGH, 0.0035, 0.00005, L),
            (WIRES_WIRE_LOSS, 1., 0.05, L),
        ],
    },
    Macro {
        name: "BRIGHT",
        rows: &[
            (BATTER_LOSS_SLOPE, 2., 0.25, L),
            (RESONANT_LOSS_SLOPE, 2., 0.25, L),
            (STRIKE_HARDNESS, 3., 1.3, L),
        ],
    },
    Macro {
        name: "DROP",
        rows: &[
            (BATTER_UNEVEN_TENSION, 0., 0.6, Q),
            (TIME_TENSION_DROP, 0., 0.8, Q),
            (TIME_DROP_TIME, 40., 120., L),
        ],
    },
    Macro {
        name: "TONE",
        rows: &[(RESONANT_TENSION, 1200., 4800., L)],
    },
];
pub trait KilnEngine: Send {
    fn name(&self) -> &'static str;
    fn sliders(&self) -> &'static [LabParam] {
        params::MEMBRANE_SLIDERS
    }
    fn macros(&self) -> &'static [Macro] {
        MACROS
    }
    fn preview(&self, p: &Patch, note: u8, vel: u8, rate: f32) -> Vec<f32>;
    fn bake(
        &self,
        p: &Patch,
        note: u8,
        vel: u8,
        rate: f32,
        progress: &mut dyn FnMut(f32) -> bool,
    ) -> Option<Vec<f32>>;
    fn field(&self, p: &Patch, t: f32, rings: usize, spokes: usize) -> Vec<f32>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn macro_curves_and_manual_ownership() {
        let mut p = Patch::default();
        p.turn_macro(4, 0.75);
        assert!((p.get(STRIKE_POSITION) - 0.72).abs() < 1e-6);
        p.set(STRIKE_POSITION, 0.2);
        p.apply_macro(4, false);
        assert!((p.get(STRIKE_POSITION) - 0.2).abs() < 1e-6);
        p.turn_macro(4, 0.5);
        assert!((p.get(STRIKE_POSITION) - 0.48).abs() < 1e-6);
        for (i, table) in MACROS.iter().enumerate() {
            assert_eq!(table.name, params::MEMBRANE_MACROS[i].0);
            for m in [0., 0.5, 1.] {
                p.turn_macro(i, m);
                for &(j, _, _, _) in table.rows {
                    let def = params::MEMBRANE_SLIDERS[j];
                    assert!(
                        (def.min..=def.max).contains(&p.sliders[j]),
                        "{} {}",
                        table.name,
                        def.name
                    );
                }
            }
        }
    }
}
