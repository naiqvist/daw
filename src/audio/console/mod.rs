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
//! the green side's business: `crate::console`. This module makes one real
//! core for every kind. The registry is an exhaustive match on purpose: a
//! new section cannot compile as a silent placeholder by accident.

#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod ceiling;
pub mod cut;
pub mod door;
pub mod drift;
pub mod drive;
pub mod echo;
pub mod four;
pub mod glue;
pub mod grit;
pub mod hit;
pub mod iron;
pub mod out;
pub mod phase;
pub mod preamp;
pub mod pump;
pub mod ring;
pub mod room;
pub mod scope;
pub mod shadow;
pub mod shine;
pub mod smear;
pub mod spectra;
pub mod split;
pub mod tape;
pub mod tone;
pub mod vca;

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

/// Green zone: the core for a section, at a sample rate and block size.
/// The exhaustive match is a build-time completeness check for the desk.
pub fn core_of(params: &SectionParams, sample_rate: f32, block: usize) -> Box<dyn SectionCore> {
    match params.kind {
        SectionKind::Preamp => Box::new(preamp::PreampCore::new(params, sample_rate, block)),
        SectionKind::Tone => Box::new(tone::ToneCore::new(params, sample_rate, block)),
        SectionKind::Door => Box::new(door::DoorCore::new(params, sample_rate, block)),
        SectionKind::Cut => Box::new(cut::CutCore::new(params, sample_rate, block)),
        SectionKind::Hit => Box::new(hit::HitCore::new(params, sample_rate, block)),
        SectionKind::Vca => Box::new(vca::VcaCore::new(params, sample_rate, block)),
        SectionKind::Split => Box::new(split::SplitCore::new(params, sample_rate, block)),
        SectionKind::Pump => Box::new(pump::PumpCore::new(params, sample_rate, block)),
        SectionKind::Drive => Box::new(drive::DriveCore::new(params, sample_rate, block)),
        SectionKind::Grit => Box::new(grit::GritCore::new(params, sample_rate, block)),
        SectionKind::Drift => Box::new(drift::DriftCore::new(params, sample_rate, block)),
        SectionKind::Four => Box::new(four::FourCore::new(params, sample_rate, block)),
        SectionKind::Shine => Box::new(shine::ShineCore::new(params, sample_rate, block)),
        SectionKind::Out => Box::new(out::OutCore::new(params, sample_rate, block)),
        SectionKind::Phase => Box::new(phase::PhaseCore::new(params, sample_rate, block)),
        SectionKind::Smear => Box::new(smear::SmearCore::new(params, sample_rate, block)),
        SectionKind::Ring => Box::new(ring::RingCore::new(params, sample_rate, block)),
        SectionKind::Echo => Box::new(echo::EchoCore::new(params, sample_rate, block)),
        SectionKind::Room => Box::new(room::RoomCore::new(params, sample_rate, block)),
        SectionKind::Glue => Box::new(glue::GlueCore::new(params, sample_rate, block)),
        SectionKind::Iron => Box::new(iron::IronCore::new(params, sample_rate, block)),
        SectionKind::Ceiling => Box::new(ceiling::CeilingCore::new(params, sample_rate, block)),
        SectionKind::Scope => Box::new(scope::ScopeCore::new(params, sample_rate, block)),
        SectionKind::Tape => Box::new(tape::TapeCore::new(params, sample_rate, block)),
        SectionKind::Shadow => Box::new(shadow::ShadowCore::new(params, sample_rate, block)),
        SectionKind::Spectra => Box::new(spectra::SpectraCore::new(params, sample_rate, block)),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// A KNOB TURN MAY NOT ALLOCATE.
    ///
    /// A letter reaches a section on the AUDIO THREAD, so `set_param`
    /// runs in the red zone. This is the test that says so, and it is
    /// here because the desk shipped without it and paid for it: every
    /// section kept its settings in the document's SPARSE table, whose
    /// `set` pushes the first time it sees an id — a Vec growing inside
    /// the callback. The first turn of any knob on any section aborted
    /// the process with an allocation of thirty-two bytes, which is a
    /// `Vec<(u32, f32)>` taking its first four slots.
    ///
    /// `assert_no_alloc` aborts rather than fails, so a regression here
    /// takes the whole test binary down. That is the correct volume for
    /// this particular mistake.
    #[test]
    fn turning_a_knob_never_allocates() {
        use assert_no_alloc::assert_no_alloc;
        for kind in crate::console::SectionKind::ALL {
            let params = crate::console::SectionParams::of(kind);
            let mut core = core_of(&params, 48_000.0, 256);
            let table = kind.table();
            let mut l = [0.0f32; 64];
            let mut r = [0.0f32; 64];
            assert_no_alloc(|| {
                for def in table {
                    // Both ends of every range and a value between, so
                    // a core that re-tunes on a change is made to.
                    for value in [def.min, def.max, (def.min + def.max) * 0.5] {
                        core.set_param(def.id, value);
                    }
                }
                core.process(&mut l, &mut r, &clock());
                let _ = core.readout();
            });
        }
    }

    /// The document's table stays sparse — only what somebody moved is
    /// written down — while the audio side's is dense, so setting a
    /// value there can only ever overwrite one.
    #[test]
    fn the_audio_sides_table_is_dense_and_the_documents_is_not() {
        for kind in crate::console::SectionKind::ALL {
            let sparse = crate::console::SectionParams::of(kind);
            assert!(
                sparse.values.is_empty(),
                "{kind:?} was written down at rest"
            );
            let mut dense = sparse.dense();
            assert_eq!(dense.values.len(), kind.table().len(), "{kind:?}");
            // Dense carries the same answers the sparse one gave.
            for def in kind.table() {
                assert_eq!(dense.value(def.id), sparse.value(def.id), "{kind:?}");
            }
            // And setting anything leaves its length alone, which is the
            // whole point: no push, no allocation.
            let before = dense.values.len();
            for def in kind.table() {
                dense.set(def.id, def.max);
                dense.set(def.id, def.min);
            }
            dense.set(9_999, 1.0);
            assert_eq!(dense.values.len(), before, "{kind:?} grew its table");
        }
    }

    fn clock() -> Clock {
        Clock {
            playing: true,
            beat: 0.0,
            beats_per_sample: 120.0 / 60.0 / 48_000.0,
        }
    }

    /// Every optional section whose resting controls describe bypass passes
    /// a quiet signal at every block length. PUMP is the one intentional
    /// exception: once put IN, its useful resting depth is already audible.
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
            if kind.strip_index().is_none() {
                // The desk's own — the buses', the mix's, the returns'
                // — are always IN and always doing something. That is
                // what makes them the desk rather than effects.
                continue;
            }
            if kind == SectionKind::Pump {
                // PUMP is deliberately ready to hear when it is put IN:
                // its resting DEPTH is halfway down. Its bypass lives at
                // DEPTH zero (and, at the desk level, in the section being
                // OUT), which pump's own tests hold sample-exact.
                continue;
            }
            for len in [0usize, 1, 7, 256] {
                let mut l: Vec<f32> = (0..len).map(|i| (i as f32 * 0.1).sin() * 0.01).collect();
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

    /// Live parameter letters must never move a channel in time. Every
    /// oversampled structural section therefore keeps one fixed round-trip
    /// delay at its floor, midpoint, and ceiling settings.
    #[test]
    fn oversampled_sections_keep_one_declared_latency_at_every_setting() {
        let expected = crate::dsp::shaper::Oversampler2x::new().latency();
        for kind in [
            SectionKind::Preamp,
            SectionKind::Cut,
            SectionKind::Drive,
            SectionKind::Iron,
        ] {
            let params = SectionParams::of(kind);
            let mut core = core_of(&params, 48_000.0, 128);
            assert_eq!(core.latency(), expected, "{kind:?} at defaults");
            for def in kind.table() {
                for value in [def.min, def.max, (def.min + def.max) * 0.5] {
                    core.set_param(def.id, value);
                    assert_eq!(core.latency(), expected, "{kind:?} {:?}={value}", def.id);
                }
            }
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
}
