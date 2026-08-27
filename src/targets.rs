//! Automation and modulation targets: every parameter, by name.
//!
//! A target is a string — `track.volume`, `dev.7.reverb.mix` — because it
//! is written into project files and into modulation wires, and it has to
//! survive a device being reordered in a chain. `ParameterRegistry` is the
//! lookup from that name to what the parameter IS.
//!
//! The names are part of the file format: renaming one orphans every wire
//! and every envelope that points at it.
//!
//! Lifted out of `main.rs` unchanged.

use crate::devices::{DEVICES, DeviceSpec};

pub const TRACK_VOLUME_TARGET: &str = "track.volume";
pub const TRACK_PAN_TARGET: &str = "track.pan";

/// Stable, content-independent metadata for an automatable parameter.
/// Devices will register more specs; the timeline only speaks these ids.
#[derive(Clone, Debug, PartialEq)]
pub struct ParameterSpec {
    pub id: String,
    pub group: String,
    pub name: String,
    pub unit: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub stepped: bool,
}

#[derive(Clone, Debug)]
pub struct ParameterRegistry {
    pub specs: Vec<ParameterSpec>,
}

/// What every device target starts with. A target names an INSTANCE:
/// `dev.7.reverb.mix` is the mix of the device with id 7, wherever it sits
/// in whichever chain — which is what lets a chain be reordered without
/// breaking a wire.
pub const DEVICE_TARGET_PREFIX: &str = "dev.";

/// The target id of one parameter of one device instance.
pub fn device_target(id: u64, spec: &DeviceSpec, param: &'static str) -> String {
    format!("{DEVICE_TARGET_PREFIX}{id}.{}.{param}", spec.prefix)
}

/// The kind-scoped tail of a target: `dev.7.reverb.mix` -> `reverb.mix`,
/// and a track target is its own tail. What the registry is keyed by, since
/// every instance of a kind has the same range, unit and name.
pub fn target_tail(target: &str) -> &str {
    target
        .strip_prefix(DEVICE_TARGET_PREFIX)
        .map_or(target, |rest| {
            rest.split_once('.').map_or(rest, |(_, tail)| tail)
        })
}

impl Default for ParameterRegistry {
    fn default() -> Self {
        let mut registry = Self { specs: Vec::new() };
        registry.register(ParameterSpec {
            id: TRACK_VOLUME_TARGET.to_owned(),
            group: "Track".to_owned(),
            name: "Volume".to_owned(),
            unit: "dB".to_owned(),
            min: 0.0,
            max: 1.5,
            default: 1.0,
            stepped: false,
        });
        registry.register(ParameterSpec {
            id: TRACK_PAN_TARGET.to_owned(),
            group: "Track".to_owned(),
            name: "Pan".to_owned(),
            unit: "%".to_owned(),
            min: -1.0,
            max: 1.0,
            default: 0.0,
            stepped: false,
        });
        // The device rows are walked out of DEVICES: range and default come
        // from `daw::params`, words from the device's labels. Adding a
        // device adds its rows here without an edit.
        for device in DEVICES {
            for (def, label) in device.params.iter().zip(device.labels) {
                registry.register(ParameterSpec {
                    id: format!("{}.{}", device.prefix, def.name),
                    group: label.group.to_owned(),
                    name: label.name.to_owned(),
                    unit: label.unit.to_owned(),
                    min: def.min,
                    max: def.max,
                    default: def.default,
                    stepped: false,
                });
            }
        }
        registry
    }
}

impl ParameterRegistry {
    /// The spec behind a target. Device targets are looked up by their
    /// KIND-scoped tail: the registry holds one row per device parameter,
    /// not one per instance, so a project with forty reverbs still has
    /// three reverb specs.
    pub fn spec(&self, id: &str) -> Option<&ParameterSpec> {
        let tail = target_tail(id);
        self.specs.iter().find(|spec| spec.id == tail)
    }

    pub fn register(&mut self, spec: ParameterSpec) {
        if let Some(existing) = self
            .specs
            .iter_mut()
            .find(|existing| existing.id == spec.id)
        {
            *existing = spec;
        } else {
            self.specs.push(spec);
        }
    }
}
