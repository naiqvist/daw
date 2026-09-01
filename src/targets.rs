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

/// The legal engine-unit span of an automation or modulation target.
///
/// Device targets resolve through the same [`ParameterRegistry`] rows that
/// are built from each device's [`crate::params::ParamDef`] table. Unknown
/// or stale ids are refused rather than assigned a plausible but wrong span.
/// The value a parameter rests at — where "no change" lives, which is
/// what an automation lane draws its one informative gridline against.
/// Not always the middle and not always the ceiling: a fader's unity sits
/// two thirds up a 0..1.5 span, because a fader can boost.
pub fn default_of(target: &str) -> Option<f32> {
    static REGISTRY: std::sync::LazyLock<ParameterRegistry> =
        std::sync::LazyLock::new(ParameterRegistry::default);

    REGISTRY.spec(target).map(|spec| spec.default)
}

pub fn span_of(target: &str) -> Option<(f32, f32)> {
    static REGISTRY: std::sync::LazyLock<ParameterRegistry> =
        std::sync::LazyLock::new(ParameterRegistry::default);

    REGISTRY.spec(target).map(|spec| (spec.min, spec.max))
}

#[cfg(test)]
mod tests {
    use super::{
        DEVICE_TARGET_PREFIX, TRACK_PAN_TARGET, TRACK_VOLUME_TARGET, device_target, span_of,
    };
    use crate::devices::DeviceKind;

    #[test]
    fn target_ids_remain_the_exact_file_format_strings() {
        assert_eq!(TRACK_VOLUME_TARGET, "track.volume");
        assert_eq!(TRACK_PAN_TARGET, "track.pan");
        assert_eq!(DEVICE_TARGET_PREFIX, "dev.");
        assert_eq!(
            device_target(7, DeviceKind::Reverb.spec(), "mix"),
            "dev.7.reverb.mix"
        );
    }

    #[test]
    fn track_targets_resolve_to_the_registry_spans() {
        assert_eq!(span_of(TRACK_VOLUME_TARGET), Some((0.0, 1.5)));
        assert_eq!(span_of(TRACK_PAN_TARGET), Some((-1.0, 1.0)));
    }

    #[test]
    fn device_targets_resolve_to_their_param_def_spans() {
        assert_eq!(span_of("dev.7.reverb.predelay"), Some((0.0, 200.0)));
    }

    #[test]
    fn unknown_target_has_no_guessed_span() {
        assert_eq!(span_of("dev.7.reverb.not-a-parameter"), None);
    }
}
