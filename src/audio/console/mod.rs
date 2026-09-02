//! The console's cores: the RED side of the desk.
//!
//! Every section of the strip, every bus stage, every return is a core
//! behind one small trait, and the graph holds one node kind for all of
//! them — `NodeSpec::Section` compiles to `Node::Section { core }`. The
//! trait is small on purpose: a section is a stereo process with
//! settings, a reset, a latency and a readout, and nothing a section
//! needs beyond that belongs in the graph.
//!
//! Red-zone rules as everywhere in `audio`: a core never allocates,
//! locks, panics, or does unbounded work in `process`; every buffer it
//! needs is made in `new`, which runs green.
//!
//! Which kinds exist, what they are called and what their tables hold is
//! the green side's business: `crate::console`. This module only knows
//! how to make a core for a kind — and, until each section is written,
//! makes a WIRE for it: a core that passes its input through untouched,
//! so the desk is whole and silent-in-silent-out from the first day.

#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod door;
pub mod preamp;
pub mod tone;

use crate::audio::graph::Readout;
use crate::console::{SectionKind, SectionParams};

/// What the transport is doing while a block is processed, for the
/// sections that keep time (PUMP, a synced ECHO).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    pub playing: bool,
    /// The beat at the block's first sample.
    pub beat: f64,
    pub beats_per_sample: f64,
}

/// One section's engine. Stereo in, stereo out, in place; `r` may be
/// empty when the node runs mono, and a core must then treat `l` as the
/// whole signal.
pub trait SectionCore: Send {
    /// A letter: `param` is an id in the kind's table, already clamped
    /// by the surface but clamped again here, because a stale file can
    /// smuggle anything.
    fn set_param(&mut self, param: u32, value: f32);
    /// Forget the history, keep the settings. Called on discontinuity.
    fn reset(&mut self);
    /// Samples of delay this core adds, for the graph to compensate.
    fn latency(&self) -> usize {
        0
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], clock: &Clock);
    /// What the meters show for this section: level and reduction.
    fn readout(&self) -> Readout {
        Readout::default()
    }
}

/// The core that is not written yet: a wire. Holds its settings so a
/// letter is not lost, and passes sound through untouched.
pub struct Wire {
    kind: SectionKind,
    values: [f32; MAX_PARAMS],
}

/// The widest table any section has, with room to spare.
pub const MAX_PARAMS: usize = 16;

impl Wire {
    pub fn new(params: &SectionParams) -> Self {
        let mut values = [0.0; MAX_PARAMS];
        for (index, def) in params.kind.table().iter().enumerate().take(MAX_PARAMS) {
            values[index] = def.clamp(params.value(def.id));
        }
        Self {
            kind: params.kind,
            values,
        }
    }

    pub fn kind(&self) -> SectionKind {
        self.kind
    }

    pub fn value(&self, param: u32) -> f32 {
        self.values.get(param as usize).copied().unwrap_or(0.0)
    }
}

impl SectionCore for Wire {
    fn set_param(&mut self, param: u32, value: f32) {
        let Some(def) = self.kind.table().get(param as usize) else {
            return;
        };
        if let Some(slot) = self.values.get_mut(param as usize) {
            *slot = def.clamp(value);
        }
    }

    fn reset(&mut self) {}

    fn process(&mut self, _l: &mut [f32], _r: &mut [f32], _clock: &Clock) {}
}

/// Green zone: the core for a section, at a sample rate and block size.
/// Every kind is a [`Wire`] until its section is written; the match is
/// where each one will take its place.
pub fn core_of(params: &SectionParams, sample_rate: f32, block: usize) -> Box<dyn SectionCore> {
    match params.kind {
        SectionKind::Preamp => Box::new(preamp::PreampCore::new(params, sample_rate, block)),
        SectionKind::Tone => Box::new(tone::ToneCore::new(params, sample_rate, block)),
        SectionKind::Door => Box::new(door::DoorCore::new(params, sample_rate, block)),
        _ => Box::new(Wire::new(params)),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn clock() -> Clock {
        Clock {
            playing: true,
            beat: 0.0,
            beats_per_sample: 120.0 / 60.0 / 48_000.0,
        }
    }

    /// A wire is a wire: whatever goes in comes out, at every block
    /// length, for every kind.
    #[test]
    fn every_kind_compiles_to_something_that_passes_sound_through() {
        for kind in SectionKind::ALL {
            let params = SectionParams::of(kind);
            let mut core = core_of(&params, 48_000.0, 256);
            if core.latency() > 0 {
                // A section that looks ahead is a wire BEHIND its
                // lookahead, which its own tests hold it to.
                continue;
            }
            for len in [0usize, 1, 7, 256] {
                let mut l: Vec<f32> = (0..len).map(|i| (i as f32 * 0.1).sin()).collect();
                let mut r: Vec<f32> = l.iter().map(|s| -s).collect();
                let (before_l, before_r) = (l.clone(), r.clone());
                core.process(&mut l, &mut r, &clock());
                assert_eq!(l, before_l, "{kind:?} changed the left at {len}");
                assert_eq!(r, before_r, "{kind:?} changed the right at {len}");
            }
            core.reset();
            assert_eq!(core.latency(), 0);
        }
    }

    /// A section tapped for telemetry reports through the schedule: a
    /// sine through a leaned-on preamp lands a level in its slot, and an
    /// untapped slot reads silence.
    #[test]
    fn a_tapped_section_reports_its_level_in_its_slot() {
        use crate::audio::graph::{GraphSpec, NodeSpec, ProcessCtx};
        let mut spec = GraphSpec::default();
        let sine = spec.push(NodeSpec::Sine {
            freq: 220.0,
            amp: 0.5,
        });
        let mut params = SectionParams::of(SectionKind::Preamp);
        params.set(crate::params::console::preamp::IRON, 40.0);
        let stage = spec.push(NodeSpec::Section { params });
        spec.connect(sine, stage);
        spec.set_output(stage);
        spec.telemetry(5, stage);
        let mut schedule = spec.compile(48_000, 256).expect("the graph runs");
        let silence = [0.0f32; 512];
        let beats_per_sample = 120.0 / 60.0 / 48_000.0;
        let mut out = vec![0.0f32; 512];
        for block in 0..4u64 {
            let ctx = ProcessCtx {
                device_input: &silence,
                in_channels: 2,
                block_frames: 256,
                offset: 0,
                len: 256,
                playing: true,
                position: block * 256,
                beat: block as f64 * 256.0 * beats_per_sample,
                beats_per_sample,
                discontinuity: block == 0,
            };
            schedule.run(&mut out, &ctx);
        }
        let said = schedule.telemetry();
        assert!(said[5].level_db > -20.0, "slot 5 read {}", said[5].level_db);
        assert_eq!(said[4].level_db, Readout::default().level_db);
    }

    /// A letter lands clamped, and an id the table lacks is dropped.
    #[test]
    fn a_wire_keeps_its_settings_within_the_table() {
        let params = SectionParams::of(SectionKind::Tone);
        let mut wire = Wire::new(&params);
        wire.set_param(crate::params::console::tone::LO, 40.0);
        assert_eq!(wire.value(crate::params::console::tone::LO), 15.0);
        wire.set_param(99, 1.0);
        assert_eq!(wire.value(99), 0.0);
    }
}
