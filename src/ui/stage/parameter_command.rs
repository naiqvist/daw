//! Green-side, unit-aware edits of the ordinary Song parameter values.
//! A sentence preflights a clone and commits once; it never writes live DSP.
use super::Stage;
use crate::devices::DeviceKind;
use crate::sequencing::{DeviceId, Song};

pub(super) const COMMANDS: &[crate::ui::palette::TypedCommand] = &[
    crate::ui::palette::TypedCommand {
        name: "param",
        usage: "param [scope:] <target> =|+=|-=|*=|/= <value unit>; ... · BASE, one undo; . = selected knob",
    },
    crate::ui::palette::TypedCommand {
        name: "params",
        usage: "params [filter] · discover stable parameter addresses, units and ranges",
    },
];

#[derive(Clone, Debug, serde::Serialize)]
pub struct ParameterChange {
    pub track_id: u64,
    pub target: String,
    pub before: f32,
    pub after: f32,
    pub unit: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ParameterReceipt {
    pub changed: bool,
    pub changes: Vec<ParameterChange>,
}

fn canonical(text: &str) -> String {
    text.trim()
        .trim_matches('"')
        .to_ascii_lowercase()
        .replace(' ', "_")
}

pub(super) fn resolve(
    song: &Song,
    current: usize,
    address: &str,
) -> Result<super::modulation::Target, String> {
    let lane_qualified = address.starts_with("lane.");
    let (track, address) = if let Some(rest) = address.strip_prefix("lane.") {
        let (id, tail) = rest
            .split_once('/')
            .ok_or("use lane.<stable id>/<target>")?;
        let id = id.parse::<u64>().map_err(|_| "invalid track id")?;
        (
            song.tracks
                .iter()
                .position(|t| t.id.0 == id)
                .ok_or("track no longer exists")?,
            tail,
        )
    } else {
        (current, address)
    };
    let address = canonical(address);
    // Fully qualified device IDs resolve across tracks, independent of focus.
    let tracks: Vec<_> = if address.starts_with("dev.") && !lane_qualified {
        (0..song.tracks.len()).collect()
    } else {
        vec![track]
    };
    let mut matches = Vec::new();
    for track in tracks {
        for target in super::modulation::targets(song, track) {
            let tail = crate::targets::target_tail(&target.id);
            let short = tail.split_once('.').map_or(tail, |(_, name)| name);
            let machine_alias = song.tracks[track].machine.as_ref().is_some_and(|m| {
                target.id.starts_with(&format!("dev.{}.", m.id.0))
                    && address == format!("machine.{}", canonical(short))
            });
            if address == canonical(&target.id)
                || address == canonical(tail)
                || address == canonical(short)
                || machine_alias
            {
                matches.push(target);
            }
        }
    }
    match matches.len() {
        0 => Err(format!("unknown target {address}; use params")),
        1 => Ok(matches.remove(0)),
        _ => Err(format!(
            "ambiguous {address}: {}",
            matches
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub(super) fn device_param(song: &Song, target: &str) -> Option<(DeviceId, u32)> {
    let (id, _) = target.strip_prefix("dev.")?.split_once('.')?;
    let id = DeviceId(id.parse().ok()?);
    let device = song.device(id)?;
    let spec = device.kind.spec();
    spec.params
        .iter()
        .find(|p| crate::targets::device_target(id.0, spec, p.name) == target)
        .map(|p| (id, p.id))
}

fn operand(target: &super::modulation::Target, text: &str, op: &str) -> Result<f64, String> {
    let text = text.trim();
    if op == "="
        && let Some(index) = target
            .choices
            .iter()
            .position(|s| s.eq_ignore_ascii_case(text.trim_matches('"')))
    {
        return Ok(f64::from(target.min) + index as f64);
    }
    // Find the longest numeric prefix (also handles scientific notation).
    let (number, suffix) = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .rev()
        .find_map(|i| {
            text[..i]
                .trim()
                .parse::<f64>()
                .ok()
                .map(|n| (n, text[i..].trim().to_ascii_lowercase()))
        })
        .ok_or_else(|| format!("invalid value {text}"))?;
    if !number.is_finite() {
        return Err("value must be finite".into());
    }
    if op == "*=" || op == "/=" {
        if !suffix.is_empty() {
            return Err("multiply/divide needs a unitless factor".into());
        }
        if op == "/=" && number == 0.0 {
            return Err("division by zero".into());
        }
        return Ok(number);
    }
    let unit = target.unit.trim().to_ascii_lowercase();
    if suffix.is_empty() || suffix == unit {
        return Ok(number);
    }
    let value = match (unit.as_str(), suffix.as_str()) {
        ("ms", "s") => number * 1000.0,
        ("s", "ms") => number / 1000.0,
        ("hz", "khz") => number * 1000.0,
        ("khz", "hz") => number / 1000.0,
        ("st", "oct") => number * 12.0,
        ("ct", "st") => number * 100.0,
        ("st", "ct") => number / 100.0,
        ("", "%") if target.min >= 0.0 && target.max == 1.0 && target.choices.is_empty() => {
            number / 100.0
        }
        ("", "db") if target.id == crate::targets::TRACK_VOLUME_TARGET => {
            if op != "=" {
                return Ok(number);
            }
            10.0_f64.powf(number / 20.0)
        }
        _ => {
            return Err(format!(
                "unit {suffix} incompatible with {} ({})",
                target.id,
                target.unit.trim()
            ));
        }
    };
    Ok(value)
}

pub(super) fn evaluate(
    target: &super::modulation::Target,
    text: &str,
    op: &str,
) -> Result<f32, String> {
    let mut amount = operand(target, text, op)?;
    // Track %-targets are fractions; device %-targets are already percentages.
    if (target.id == crate::targets::TRACK_PAN_TARGET
        || crate::targets::track_send_index(&target.id).is_some())
        && text.trim().ends_with('%')
        && op != "*="
        && op != "/="
    {
        amount /= 100.0;
    }
    let base = f64::from(target.base);
    let value = if target.id == crate::targets::TRACK_VOLUME_TARGET
        && text.trim().to_ascii_lowercase().ends_with("db")
        && matches!(op, "+=" | "-=")
    {
        base * 10.0_f64.powf(amount * if op == "+=" { 1.0 } else { -1.0 } / 20.0)
    } else {
        match op {
            "=" => amount,
            "+=" => base + amount,
            "-=" => base - amount,
            "*=" => base * amount,
            "/=" => base / amount,
            _ => return Err("unknown operator".into()),
        }
    };
    if !value.is_finite() || value < f64::from(target.min) || value > f64::from(target.max) {
        return Err(format!(
            "{} must be {}..{} {} (no edits applied)",
            target.id,
            target.min,
            target.max,
            target.unit.trim()
        ));
    }
    if !target.choices.is_empty() && value.fract() != 0.0 {
        return Err("choice requires a label or integer".into());
    }
    Ok(value as f32)
}

impl Stage {
    pub(super) fn open_parameter_entry(&mut self) -> Result<(), super::RefusalReason> {
        if self.addressing_steps() {
            self.notice =
                Some("base value entry: clear step selection first; locks are unchanged".into());
            return Err(super::RefusalReason::Unavailable);
        }
        let address = self
            .selected_parameter_address()
            .map_err(|_| super::RefusalReason::Unavailable)?;
        let target = resolve(&self.song, self.deck_track().unwrap_or(0), &address)
            .map_err(|_| super::RefusalReason::Unavailable)?;
        self.notice = Some(format!(
            "BASE {} · {}..{} {} · Enter commits, Escape cancels",
            target.id,
            target.min,
            target.max,
            target.unit.trim()
        ));
        self.palette.open_with_query(format!("param {address}="));
        Ok(())
    }

    fn selected_parameter_address(&self) -> Result<String, String> {
        let (track, page) = self.selected_page().ok_or("no selected parameter")?;
        match page.slots.get(self.deck.slot).copied().flatten() {
            Some(crate::pages::Slot::Param { subject, id }) => {
                let device = match subject {
                    crate::pages::Subject::Machine => self.song.tracks[track].machine.as_ref(),
                    crate::pages::Subject::Section(kind) => self.song.section(track, kind),
                }
                .ok_or("no device")?;
                let spec = device.kind.spec();
                let def = spec
                    .params
                    .iter()
                    .find(|p| p.id == id)
                    .ok_or("no parameter")?;
                Ok(crate::targets::device_target(device.id.0, spec, def.name))
            }
            _ => Err("select an instrument or effect parameter".into()),
        }
    }

    /// One atomic base-value transaction. Receipts use stable IDs and raw Song units.
    /// Errors leave Song, history and all audio revisions unchanged.
    pub fn edit_parameters(&mut self, sentence: &str) -> Result<ParameterReceipt, String> {
        if sentence.len() > 8192 {
            return Err("parameter sentence exceeds 8192 bytes".into());
        }
        let (scope, sentence) = sentence
            .split_once(':')
            .map_or(("", sentence), |(scope, rest)| (scope.trim(), rest));
        if scope.contains(['=', ';']) {
            return Err("scope belongs before the edits".into());
        }
        let parts: Vec<_> = sentence.split(';').collect();
        if parts.len() > 64 {
            return Err("at most 64 edits per transaction".into());
        }
        let current = self.deck_track().unwrap_or(0);
        let mut candidate = self.song.clone();
        let mut changes = Vec::new();
        let mut structural = false;
        for part in parts {
            let equal = part.find('=').ok_or("use target = value, or += -= *= /=")?;
            let before_equal = &part[..equal];
            let (address, op) = match before_equal.chars().last() {
                Some(c @ ('+' | '-' | '*' | '/')) => {
                    (&before_equal[..before_equal.len() - 1], format!("{c}="))
                }
                _ => (before_equal, "=".to_owned()),
            };
            let address = if address.trim() == "." {
                self.selected_parameter_address()?
            } else if !scope.is_empty() && !address.contains('.') {
                format!("{scope}.{}", address.trim())
            } else {
                address.trim().to_owned()
            };
            let target = resolve(&candidate, current, &address)?;
            let after = evaluate(&target, &part[equal + 1..], &op)?;
            let mut enabled = false;
            if let Some((id, param)) = device_param(&candidate, &target.id) {
                let device = candidate.device_mut(id).ok_or("device disappeared")?;
                if device.kind == DeviceKind::Scomp && param == crate::params::scomp::OPEN {
                    return Err("OPEN is an action; use the Forge command".into());
                }
                if let DeviceKind::Console(kind) = device.kind {
                    if !kind.always_in() && device.bypassed {
                        device.bypassed = false;
                        enabled = true;
                        structural = true;
                    }
                }
                if after != target.base {
                    device.set(param, after);
                }
                structural |= after != target.base
                    && device.kind == DeviceKind::Scomp
                    && crate::params::scomp::baked(param);
            } else {
                let track = &mut candidate.tracks[target.track];
                match target.id.as_str() {
                    crate::targets::TRACK_VOLUME_TARGET => track.volume = after,
                    crate::targets::TRACK_PAN_TARGET => track.pan = after,
                    _ => {
                        let index = crate::targets::track_send_index(&target.id)
                            .ok_or("unsupported target")?;
                        let param = [
                            crate::params::console::out::SEND_TAPE,
                            crate::params::console::out::SEND_SHADOW,
                        ]
                        .get(index)
                        .copied()
                        .ok_or("send unavailable")?;
                        let section = track
                            .strip
                            .iter_mut()
                            .find(|d| {
                                d.kind == DeviceKind::Console(crate::console::SectionKind::Out)
                            })
                            .ok_or("OUT section missing")?;
                        if after != target.base {
                            section.set(param, after * 100.0);
                        }
                    }
                }
            }
            changes.push(ParameterChange {
                track_id: candidate.tracks[target.track].id.0,
                target: target.id,
                before: target.base,
                after,
                unit: target.unit.trim().to_owned(),
                enabled,
            });
        }
        let changed = candidate != self.song;
        if changed {
            self.song = candidate;
            if structural {
                self.touched();
            }
            self.remixed();
            self.settle();
        }
        Ok(ParameterReceipt { changed, changes })
    }

    pub(super) fn apply_parameter_command(&mut self, input: &str) -> bool {
        let (command, rest) = input.split_once(char::is_whitespace).unwrap_or((input, ""));
        if command == "params" {
            let filter = rest.trim().to_ascii_lowercase();
            let track = self.deck_track().unwrap_or(0);
            let rows: Vec<_> = super::modulation::targets(&self.song, track)
                .into_iter()
                .filter(|t| {
                    format!("{} {} {}", t.id, t.name, t.group)
                        .to_ascii_lowercase()
                        .contains(&filter)
                })
                .map(|t| {
                    format!(
                        "{} = {} [{}..{} {}]{}",
                        t.id,
                        t.base,
                        t.min,
                        t.max,
                        t.unit.trim(),
                        if t.choices.is_empty() {
                            String::new()
                        } else {
                            format!(" {:?}", t.choices)
                        }
                    )
                })
                .collect();
            self.notice = Some(format!("params · BASE · {}", rows.join("; ")));
            return true;
        }
        match self.edit_parameters(rest) {
            Ok(receipt) => {
                self.notice = Some(format!(
                    "param · BASE · {} · {}",
                    if receipt.changed {
                        "committed"
                    } else {
                        "unchanged"
                    },
                    receipt
                        .changes
                        .iter()
                        .map(|c| format!(
                            "lane.{}/{} {} → {}{}",
                            c.track_id,
                            c.target,
                            c.before,
                            c.after,
                            if c.enabled { " (IN)" } else { "" }
                        ))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
                true
            }
            Err(error) => {
                self.notice = Some(format!("REFUSED param · {error}"));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn stage() -> Stage {
        let mut stage = Stage::new();
        stage.song.add_device(0, DeviceKind::Table).unwrap();
        stage.settle();
        stage
    }
    fn value(stage: &Stage, param: u32) -> f32 {
        stage.song.tracks[0].machine.as_ref().unwrap().value(param)
    }

    #[test]
    fn units_arithmetic_and_choices_share_the_saved_model() {
        let mut s = stage();
        assert!(s.apply_timeline_command("param machine.attack = 1.6 s; machine.cutoff = 1.8 kHz; machine.sustain = 82%; machine.release = 2.5s"));
        assert_eq!(value(&s, crate::params::table::ATTACK), 1600.0);
        assert_eq!(value(&s, crate::params::table::CUTOFF), 1800.0);
        assert_eq!(value(&s, crate::params::table::SUSTAIN), 0.82);
        assert!(s.apply_timeline_command(
            "param machine.release /= 2; machine.tune += 1 oct; track.pan = -25%; room.mix = 35%"
        ));
        assert_eq!(value(&s, crate::params::table::RELEASE), 1250.0);
        assert_eq!(value(&s, crate::params::table::TUNE), 12.0);
        assert_eq!(s.song.tracks[0].pan, -0.25);
        let text = ron::to_string(&s.song).unwrap();
        assert_eq!(ron::from_str::<Song>(&text).unwrap(), s.song);
    }

    #[test]
    fn transaction_failure_and_single_undo_preserve_everything() {
        let mut s = stage();
        let before = s.song.clone();
        let revisions = (s.revision(), s.mix_revision());
        for bad in [
            "machine.release /= 0",
            "machine.cutoff = NaN",
            "machine.cutoff = 3ms",
            "machine.release = 1e99s",
            "missing = 2",
            "machine.attack = 2s; room.mix = 999%",
            "machine.attack *= 2s",
            "machine.attack = 2s;",
        ] {
            assert!(s.edit_parameters(bad).is_err(), "{bad}");
            assert_eq!(s.song, before, "{bad}");
            assert_eq!((s.revision(), s.mix_revision()), revisions);
        }
        let receipt = s
            .edit_parameters("machine.attack = 2s; machine.release *= 2; room.mix = 35%")
            .unwrap();
        assert!(receipt.changed);
        assert_eq!(receipt.changes.len(), 3);
        assert!(receipt.changes[2].enabled);
        let after = s.song.clone();
        let _ = s.apply(super::super::StageIntent::Undo);
        assert_eq!(s.song, before);
        let _ = s.apply(super::super::StageIntent::Redo);
        assert_eq!(s.song, after);
    }

    #[test]
    fn stable_device_target_ignores_track_reordering_and_focus() {
        let mut s = stage();
        let id = s.song.tracks[0].machine.as_ref().unwrap().id;
        s.song.add_track(crate::sequencing::TrackKind::Audio);
        s.song.tracks.swap(0, 1);
        s.edit_parameters(&format!("dev.{}.table.release = 3s", id.0))
            .unwrap();
        assert_eq!(
            s.song
                .device(id)
                .unwrap()
                .value(crate::params::table::RELEASE),
            3000.0
        );
        assert!(s.edit_parameters("dev.999999.table.release = 3s").is_err());
    }

    #[test]
    fn decibels_are_gain_ratios_not_raw_offsets() {
        let mut s = stage();
        s.edit_parameters("track.volume = -6 dB; track.volume += 6dB")
            .unwrap();
        assert!((s.song.tracks[0].volume - 1.0).abs() < 1e-6);
    }

    #[test]
    fn scoped_edits_current_knob_choices_and_ambiguity() {
        let mut s = stage();
        s.edit_parameters("table: attack = 2s; release = 3s")
            .unwrap();
        assert_eq!(value(&s, crate::params::table::ATTACK), 2000.0);
        s.deck.lit = Some(crate::pages::PageKey::Amp);
        s.deck.slot = 0;
        s.edit_parameters(". = 180ms").unwrap();
        assert_eq!(value(&s, crate::params::table::ATTACK), 180.0);
        s.edit_parameters("room.algo = hall").unwrap();
        let before = s.song.clone();
        for bad in [
            "mix = 25%",
            "room.algo = 0.5",
            "room.algo = missing",
            "attack = 2s: release = 1s",
        ] {
            assert!(s.edit_parameters(bad).is_err(), "{bad}");
            assert_eq!(s.song, before);
        }
        let original_track = s.song.tracks[0].id.0;
        let other_track = s.song.add_track(crate::sequencing::TrackKind::Instrument).0;
        let machine = s.song.tracks[0].machine.as_ref().unwrap().id.0;
        assert!(
            s.edit_parameters(&format!(
                "lane.{other_track}/dev.{machine}.table.attack = 1s"
            ))
            .is_err()
        );
        s.edit_parameters(&format!(
            "lane.{original_track}/dev.{machine}.table.attack = 1s"
        ))
        .unwrap();
    }

    #[test]
    fn consecutive_commands_through_real_palette_frames() {
        let mut s = stage();
        s.set_palette_open(false);
        let ctx = egui::Context::default();
        crate::install_stage_fonts(&ctx);
        let chord = |key, modifiers| {
            [true, false]
                .map(|pressed| egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed,
                    repeat: false,
                    modifiers,
                })
                .to_vec()
        };
        for command in [
            "param machine.attack = 1.6s; room.mix = 35%",
            "param machine.release = 2.5s",
            "param machine.release /= 2",
        ] {
            for events in [
                chord(egui::Key::P, egui::Modifiers::CTRL | egui::Modifiers::SHIFT),
                vec![egui::Event::Text(command.into())],
                chord(egui::Key::Enter, egui::Modifiers::NONE),
            ] {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280.0, 800.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| s.show(ui),
                );
                output.textures_delta.clear();
            }
            assert!(!s.palette.is_open());
            assert!(
                s.notice
                    .as_deref()
                    .unwrap_or_default()
                    .contains("committed"),
                "{:?}",
                s.notice
            );
        }
        assert_eq!(value(&s, crate::params::table::ATTACK), 1600.0);
        assert_eq!(value(&s, crate::params::table::RELEASE), 1250.0);
    }

    #[test]
    fn equals_on_a_knob_accepts_only_the_value_and_escape_is_nonmutating() {
        let mut s = stage();
        s.set_palette_open(false);
        s.deck.open = true;
        s.deck.lit = Some(crate::pages::PageKey::Amp);
        s.deck.slot = 0;
        let ctx = egui::Context::default();
        crate::install_stage_fonts(&ctx);
        let mut frame = |s: &mut Stage, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| s.show(ui),
            );
            output.textures_delta.clear();
        };
        let key = |key| {
            [true, false]
                .map(|pressed| egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                })
                .to_vec()
        };
        for text in ["180ms", "1.6s"] {
            frame(&mut s, key(egui::Key::Equals));
            assert!(s.palette.is_open());
            frame(&mut s, vec![egui::Event::Text(text.into())]);
            frame(&mut s, key(egui::Key::Enter));
            assert!(!s.palette.is_open());
            assert!(
                s.notice
                    .as_deref()
                    .unwrap_or_default()
                    .contains("committed"),
                "{:?}",
                s.notice
            );
        }
        assert_eq!(value(&s, crate::params::table::ATTACK), 1600.0);
        let before = s.song.clone();
        frame(&mut s, key(egui::Key::Equals));
        frame(&mut s, vec![egui::Event::Text("3s".into())]);
        frame(&mut s, key(egui::Key::Escape));
        assert_eq!(s.song, before);
    }
}
