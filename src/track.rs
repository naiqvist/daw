//! What a track IS: its chain, its mixer values, its automation.
//!
//! `Track` is the arrangement's unit of everything-but-the-clips — one
//! lane's devices, level, pan, mute and solo — and `MasterTrack` is the
//! one every lane sums into. `TrackWire` is the on-disk shape, kept apart
//! so the hand-written `Deserialize` can migrate a v1 file's single
//! instrument and effect into a chain.
//!
//! Lifted out of `main.rs` unchanged.

use crate::TRACK_H;
use crate::automation::TrackAutomation;
use crate::device_state::{DeviceInstance, DeviceState, ReverbParams, unit_zoom};
use crate::devices::DeviceKind;
use crate::targets::{TRACK_PAN_TARGET, TRACK_VOLUME_TARGET};
use daw::audio::graph::SynthParams;
use daw::ui::device;
use daw::ui::vm::TrackKind;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Track {
    /// What the lane carries. Fixed at creation: changing a track's kind
    /// would change what every clip on it means, which is a conversion,
    /// not a toggle.
    pub kind: TrackKind,
    /// What the header shows and what a mixer strip will show later. Not
    /// unique — two tracks may share a name, exactly as two files in
    /// different folders may.
    pub name: String,
    pub height: f32,
    /// Silenced. A muted track leaves the SCHEDULE rather than being
    /// multiplied by zero: the graph should be as small as what is
    /// actually sounding.
    pub mute: bool,
    /// Soloed. While any track is soloed, only soloed tracks are wired —
    /// solo-in-place, the meaning every DAW agrees on.
    pub solo: bool,
    /// Constant-power pan, `-1..=1`. Center is 0.0, and it is exact: the
    /// knob snaps there so "back to the middle" is reachable by hand.
    pub pan: f32,
    /// Fader level as LINEAR amplitude, 1.0 = unity. Stored linear because
    /// that is what the engine multiplies by; the fader does the dB
    /// mapping, which is the only place the curve belongs.
    pub volume: f32,
    /// Persistent track envelopes. Their values are applied live to the
    /// output node, so drawing a curve never requires a graph swap.
    pub automation: TrackAutomation,
    /// The devices on this lane, in SIGNAL order: at most one instrument,
    /// at the head, then its effects. Empty is a real state, not a
    /// placeholder — an empty track compiles to no nodes at all and is
    /// silent. Loading a device from the browser is what gives a track a
    /// voice.
    pub chain: Vec<DeviceInstance>,
    /// The FILE and slice table of every sampler on this lane, keyed by
    /// device instance id.
    ///
    /// Beside the devices rather than inside `DeviceState`, and that is a
    /// deliberate trade: `DeviceState` is `Copy` — one stored copy of one
    /// truth, cheap to pass by value, and half this file passes it by
    /// value — while a path and a slice list are neither. Putting them in
    /// the enum would cost `Copy` everywhere for the sake of one device.
    ///
    /// Keyed by instance id rather than by position, for the reason
    /// automation is: reordering a chain must not repoint a sample.
    #[serde(default)]
    pub sampler_sources: std::collections::BTreeMap<u64, SamplerSource>,
}

/// A sampler's file and the slices cut from it.
///
/// Green-zone document data. The path persists; the decoded audio never
/// does — it is rebuilt at compile from the path, through the loader's
/// cache.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SamplerSource {
    pub path: std::path::PathBuf,
    /// Boundaries in SOURCE frames, sorted, first is always 0. Empty
    /// means "not authored yet", which compiles to the grid the `slices`
    /// knob asks for.
    pub slices: Vec<u64>,
}

impl Track {
    /// A fresh track of `kind`, named. The one door new tracks walk
    /// through, so the default session and Ctrl+T build the same thing.
    pub fn new(kind: TrackKind, name: String) -> Self {
        Self {
            kind,
            name,
            ..Self::default()
        }
    }

    /// The device with this instance id, if this track carries it.
    pub fn device(&self, id: u64) -> Option<&DeviceInstance> {
        self.chain.iter().find(|instance| instance.id == id)
    }

    pub fn device_mut(&mut self, id: u64) -> Option<&mut DeviceInstance> {
        self.chain.iter_mut().find(|instance| instance.id == id)
    }

    /// The instrument at the head of the chain, if there is one.
    pub fn instrument(&self) -> Option<&DeviceInstance> {
        self.chain
            .first()
            .filter(|head| head.kind().is_instrument())
    }

    /// Put a device on this lane, honouring the ordering rule: AT MOST ONE
    /// instrument and it heads the chain, effects following in the order
    /// they were added. Returns the id of the instrument it displaced, if
    /// any — its wires are dangling pointers and the caller must drop them.
    pub fn insert_device(&mut self, instance: DeviceInstance) -> Option<u64> {
        if !instance.kind().is_instrument() {
            self.chain.push(instance);
            return None;
        }
        let displaced = self.instrument().map(|old| old.id);
        // Anything else calling itself an instrument goes too: the rule is
        // one, not one at the head and others hiding behind it.
        self.chain.retain(|other| !other.kind().is_instrument());
        self.chain.insert(0, instance);
        displaced
    }
}

/// The bus every track lands on: one fader, one pan, one chain of effects,
/// and the last thing the speakers hear.
///
/// A separate type rather than another [`Track`] in the stack, and that is
/// the whole design. A master carries no clips, no kind, no mute and no
/// solo — a muted master is just a fader at the bottom — and above all it
/// has no INDEX. Half this file addresses a track by its position, and a
/// master that could be at position 3 would put a bus in the middle of the
/// song. Giving it its own field keeps every one of those loops honest.
///
/// An instrument dropped here is refused rather than silently placed: the
/// master has no notes to give it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct MasterTrack {
    /// Fader level as LINEAR amplitude, exactly as a track's is.
    pub volume: f32,
    /// Constant-power pan over the whole mix. Rarely moved, and present
    /// for the same reason a console's master pan is: it exists.
    pub pan: f32,
    /// The master chain, in signal order. Effects only.
    pub chain: Vec<DeviceInstance>,
}

impl Default for MasterTrack {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pan: 0.0,
            chain: Vec::new(),
        }
    }
}

impl MasterTrack {
    /// The name every surface shows. Not a field: renaming the master
    /// would only ever make a mix harder to read out loud.
    pub const NAME: &'static str = "MASTER";

    pub fn device_mut(&mut self, id: u64) -> Option<&mut DeviceInstance> {
        self.chain.iter_mut().find(|instance| instance.id == id)
    }

    /// Put an effect on the master. Instruments are refused — there is
    /// nothing here for one to play — and the caller is told so it can say
    /// why rather than dropping the device into silence.
    pub fn insert_device(&mut self, instance: DeviceInstance) -> bool {
        if instance.kind().is_instrument() {
            return false;
        }
        self.chain.push(instance);
        true
    }
}

/// Force the ordering rule onto a chain that came from a FILE: keep the
/// first instrument, move it to the head, drop any others. A hand-edited
/// project is input like any other, and an instrument in the middle of a
/// chain would compile to a source the effects before it never see.
pub fn sanitize_chain(chain: &mut Vec<DeviceInstance>) {
    let Some(at) = chain.iter().position(|d| d.kind().is_instrument()) else {
        return;
    };
    let instrument = chain.remove(at);
    chain.retain(|other| !other.kind().is_instrument());
    chain.insert(0, instrument);
}

impl Default for Track {
    fn default() -> Self {
        Self {
            kind: TrackKind::default(),
            name: String::new(),
            height: TRACK_H,
            mute: false,
            solo: false,
            pan: 0.0,
            volume: 1.0,
            automation: TrackAutomation::default(),
            // A fresh track has no devices at all.
            chain: Vec::new(),
            sampler_sources: std::collections::BTreeMap::new(),
        }
    }
}

/// Compatibility shape for projects written before a track's devices became
/// a chain of identified instances.
///
/// A v1 track carried ONE instrument and ONE effect as named fields, plus
/// two copies of the synth's values: `params` in engine units and `synth` as
/// normalized knob positions. `params` is the one the engine ever read, so
/// it is the one that survives; `synth` is simply not named here and serde
/// skips it. The reverb's stored values were already engine units (its
/// ranges are `0..=1` and the percent was a rendering), so they carry over
/// as they are.
///
/// The instances land with id 0 — the document's id mint lives in
/// `apply_project_doc`, which is also where the old targets are rewritten.
#[derive(serde::Deserialize)]
#[serde(default)]
pub struct TrackWire {
    pub kind: TrackKind,
    pub name: String,
    pub height: f32,
    pub mute: bool,
    pub solo: bool,
    pub pan: f32,
    pub volume: f32,
    pub automation: TrackAutomation,
    pub chain: Vec<DeviceInstance>,
    pub sampler_sources: std::collections::BTreeMap<u64, SamplerSource>,
    pub device: Option<DeviceKind>,
    pub fx: Option<DeviceKind>,
    pub params: SynthParams,
    pub reverb: device::ReverbUi,
}

impl Default for TrackWire {
    fn default() -> Self {
        let track = Track::default();
        Self {
            kind: track.kind,
            name: track.name,
            height: track.height,
            mute: track.mute,
            solo: track.solo,
            pan: track.pan,
            volume: track.volume,
            automation: track.automation,
            chain: track.chain,
            sampler_sources: track.sampler_sources,
            device: None,
            fx: None,
            params: SynthParams::default(),
            reverb: device::ReverbUi::default(),
        }
    }
}

impl<'de> serde::Deserialize<'de> for Track {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = TrackWire::deserialize(deserializer)?;
        let mut chain = wire.chain;
        if chain.is_empty() {
            if let Some(kind) = wire.device.filter(|kind| kind.is_instrument()) {
                chain.push(DeviceInstance {
                    id: 0,
                    state: match kind {
                        DeviceKind::SineSynth => DeviceState::SineSynth(wire.params),
                        other => DeviceState::new(other),
                    },
                    bypass: false,
                    page: 0,
                    view_zoom: unit_zoom(),
                    view_scroll: 0.0,
                });
            }
            if let Some(kind) = wire.fx.filter(|kind| !kind.is_instrument()) {
                chain.push(DeviceInstance {
                    id: 0,
                    state: match kind {
                        // A project written before the network replaced
                        // the Freeverb carries three numbers. The rest
                        // come from the table's defaults, which is what
                        // `..Default::default()` is for — the old file
                        // never knew about pre-delay or width and must
                        // not be made to guess.
                        DeviceKind::Reverb => DeviceState::Reverb(ReverbParams {
                            mix: wire.reverb.mix,
                            size: wire.reverb.size,
                            damp: wire.reverb.damp,
                            ..ReverbParams::default()
                        }),
                        other => DeviceState::new(other),
                    },
                    bypass: false,
                    page: 0,
                    view_zoom: unit_zoom(),
                    view_scroll: 0.0,
                });
            }
        }
        Ok(Self {
            kind: wire.kind,
            name: wire.name,
            height: wire.height,
            mute: wire.mute,
            solo: wire.solo,
            pan: wire.pan,
            volume: wire.volume,
            automation: wire.automation,
            chain,
            sampler_sources: wire.sampler_sources,
        })
    }
}

pub fn split_track_automation_at(track: &mut Track, beat: f32) {
    for target in track.automation.targets() {
        let base = match target.as_str() {
            TRACK_VOLUME_TARGET => track.volume,
            TRACK_PAN_TARGET => track.pan,
            _ => 0.0,
        };
        track.automation.split_at(&target, beat, base);
    }
}

pub fn insert_track_automation_time(track: &mut Track, at: f32, amount: f32) {
    for target in track.automation.targets() {
        let base = match target.as_str() {
            TRACK_VOLUME_TARGET => track.volume,
            TRACK_PAN_TARGET => track.pan,
            _ => 0.0,
        };
        track.automation.insert_time(&target, at, amount, base);
    }
}

pub fn delete_track_automation_time(track: &mut Track, from: f32, to: f32) {
    for target in track.automation.targets() {
        let base = match target.as_str() {
            TRACK_VOLUME_TARGET => track.volume,
            TRACK_PAN_TARGET => track.pan,
            _ => 0.0,
        };
        track.automation.delete_time(&target, from, to, base);
    }
}
