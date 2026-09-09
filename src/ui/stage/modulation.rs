//! The modulation workspace's state and edits.
//!
//! Sources and wires are Song data; this file keeps only the cursor that
//! addresses them. The engine receives an immutable `ModSpec` from
//! `song_graph`, while live value edits travel through the host's modulation
//! mailbox. Nothing drawn by the view is a second authority on what sounds.

use crate::audio::modulation::{MOD_HZ, MOD_RATES, ModKind, ModShape, ModWire};
use crate::devices::DeviceKind;
use crate::sequencing::{Device, Song};

use super::{RefusalReason, Stage, Step};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Focus {
    #[default]
    Sources,
    Targets,
    Response,
}

impl Focus {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Sources => "SOURCES",
            Self::Targets => "DESTINATIONS",
            Self::Response => "RESPONSE",
        }
    }

    fn next(self, backwards: bool) -> Self {
        match (self, backwards) {
            (Self::Sources, false) | (Self::Response, true) => Self::Targets,
            (Self::Targets, false) | (Self::Sources, true) => Self::Response,
            (Self::Response, false) | (Self::Targets, true) => Self::Sources,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum SourceField {
    #[default]
    Shape,
    Rate,
    Mode,
}

impl SourceField {
    pub(super) const ALL: [Self; 3] = [Self::Shape, Self::Rate, Self::Mode];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Shape => "SHAPE",
            Self::Rate => "RATE",
            Self::Mode => "CLOCK",
        }
    }

    fn step(self, right: bool) -> Self {
        let at = Self::ALL
            .iter()
            .position(|field| *field == self)
            .unwrap_or(0);
        let next = if right {
            (at + 1).min(Self::ALL.len() - 1)
        } else {
            at.saturating_sub(1)
        };
        Self::ALL[next]
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum WireControl {
    #[default]
    Depth,
    Curve,
    Steps,
    Smooth,
    Enabled,
    Solo,
}

impl WireControl {
    pub(super) const ALL: [Self; 6] = [
        Self::Depth,
        Self::Curve,
        Self::Steps,
        Self::Smooth,
        Self::Enabled,
        Self::Solo,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Depth => "DEPTH",
            Self::Curve => "CURVE",
            Self::Steps => "STEPS",
            Self::Smooth => "SMOOTH",
            Self::Enabled => "BYPASS",
            Self::Solo => "SOLO",
        }
    }

    fn step(self, down: bool) -> Self {
        let at = Self::ALL
            .iter()
            .position(|control| *control == self)
            .unwrap_or(0);
        let next = if down {
            (at + 1).min(Self::ALL.len() - 1)
        } else {
            at.saturating_sub(1)
        };
        Self::ALL[next]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Target {
    pub(super) track: usize,
    pub(super) id: String,
    pub(super) group: String,
    pub(super) name: String,
    pub(super) unit: &'static str,
    pub(super) choices: &'static [&'static str],
    pub(super) min: f32,
    pub(super) max: f32,
    pub(super) base: f32,
}

impl Target {
    pub(super) fn face(&self, value: f32) -> String {
        if !self.choices.is_empty() {
            let at = (value - self.min).round().max(0.0) as usize;
            return self.choices[at.min(self.choices.len() - 1)].to_uppercase();
        }
        // Track pan and sends are canonical fractions in Song but are
        // labelled as percentages for a musician. Device %-parameters
        // already live in 0..100 and must not be scaled a second time.
        let value = if self.id == crate::targets::TRACK_PAN_TARGET
            || crate::targets::track_send_index(&self.id).is_some()
        {
            value * 100.0
        } else {
            value
        };
        super::chain::format_value(value, self.unit.trim())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Panel {
    pub(super) focus: Focus,
    pub(super) source: usize,
    pub(super) source_field: SourceField,
    pub(super) track: usize,
    pub(super) target: usize,
    pub(super) control: WireControl,
}

impl Panel {
    pub(super) fn open(song: &Song, track: usize) -> Self {
        let mut panel = Self {
            track,
            ..Self::default()
        };
        panel.fit(song);
        panel
    }

    pub(super) fn fit(&mut self, song: &Song) {
        self.source = self.source.min(song.modulators.len().saturating_sub(1));
        self.track = self.track.min(song.tracks.len().saturating_sub(1));
        self.target = self
            .target
            .min(targets(song, self.track).len().saturating_sub(1));
        if song.modulators.is_empty() {
            self.focus = Focus::Sources;
        }
    }

    pub(super) fn tab(&mut self, backwards: bool) {
        self.focus = self.focus.next(backwards);
    }

    pub(super) fn source_id(&self, song: &Song) -> Option<u64> {
        song.modulators.get(self.source).map(|source| source.id)
    }

    pub(super) fn target(&self, song: &Song) -> Option<Target> {
        targets(song, self.track).into_iter().nth(self.target)
    }

    pub(super) fn wire_index(&self, song: &Song) -> Option<usize> {
        let source = self.source_id(song)?;
        let target = self.target(song)?;
        song.mod_wires.iter().position(|wire| {
            wire.source == source && wire.track == target.track && wire.target == target.id
        })
    }
}

fn push_device_targets(out: &mut Vec<Target>, track: usize, devices: &[Device]) {
    for (at, device) in devices.iter().enumerate() {
        let spec = device.kind.spec();
        let repeated = devices
            .iter()
            .filter(|other| other.kind == device.kind && other.role == device.role)
            .count();
        let ordinal = devices[..=at]
            .iter()
            .filter(|other| other.kind == device.kind && other.role == device.role)
            .count();
        let instance = match (device.role.code(), repeated > 1) {
            (Some(role), _) => format!("{} {}", role.to_uppercase(), spec.name),
            (None, true) => format!("{} {ordinal}", spec.name),
            (None, false) => spec.name.to_owned(),
        };
        for (def, label) in spec.params.iter().zip(spec.labels) {
            // OUT's two sends have canonical track.send.* addresses. Offering
            // the generated device alias too would draw two controls for one
            // tap, only one of which is the actual Send node.
            if device.kind == DeviceKind::Console(crate::console::SectionKind::Out)
                && matches!(
                    def.id,
                    crate::params::console::out::SEND_TAPE
                        | crate::params::console::out::SEND_SHADOW
                )
            {
                continue;
            }
            out.push(Target {
                track,
                id: crate::targets::device_target(device.id.0, spec, def.name),
                group: format!("{} / {}", instance, label.group),
                name: label.name.to_owned(),
                unit: label.unit,
                choices: label.choices,
                min: def.min,
                max: def.max,
                base: device.value(def.id),
            });
        }
    }
}

/// Every destination the selected channel owns, in audible signal order.
pub(super) fn targets(song: &Song, track: usize) -> Vec<Target> {
    let Some(lane) = song.tracks.get(track) else {
        return Vec::new();
    };
    let mut out = vec![
        Target {
            track,
            id: crate::targets::TRACK_VOLUME_TARGET.to_owned(),
            group: "CHANNEL / OUTPUT".to_owned(),
            name: "VOLUME".to_owned(),
            unit: "",
            choices: &[],
            min: 0.0,
            max: 1.5,
            base: lane.volume,
        },
        Target {
            track,
            id: crate::targets::TRACK_PAN_TARGET.to_owned(),
            group: "CHANNEL / OUTPUT".to_owned(),
            name: "PAN".to_owned(),
            unit: "%",
            choices: &[],
            min: -1.0,
            max: 1.0,
            base: lane.pan,
        },
    ];
    let out_section = lane
        .strip
        .iter()
        .find(|device| device.kind == DeviceKind::Console(crate::console::SectionKind::Out));
    for (index, (name, param)) in [
        ("SEND TAPE", crate::params::console::out::SEND_TAPE),
        ("SEND SHADOW", crate::params::console::out::SEND_SHADOW),
    ]
    .into_iter()
    .enumerate()
    {
        if let Some(id) = crate::targets::track_send_target(index) {
            out.push(Target {
                track,
                id: id.to_owned(),
                group: "CHANNEL / AUX".to_owned(),
                name: name.to_owned(),
                unit: "%",
                choices: &[],
                min: 0.0,
                max: 1.0,
                base: out_section.map_or(0.0, |section| section.value(param) * 0.01),
            });
        }
    }
    // The machine first, then the strip: a lane LFO's destination is any
    // slot on any page, and the machine's are the pages that matter most.
    if let Some(machine) = lane.machine.as_ref() {
        push_device_targets(&mut out, track, std::slice::from_ref(machine));
    }
    push_device_targets(&mut out, track, &lane.strip);
    out
}

pub(super) fn source_name(song: &Song, index: usize) -> String {
    let Some(source) = song.modulators.get(index) else {
        return "NO SOURCE".to_owned();
    };
    match source.kind {
        ModKind::Lfo { .. } => {
            let ordinal = song.modulators[..=index]
                .iter()
                .filter(|source| matches!(source.kind, ModKind::Lfo { .. }))
                .count();
            format!("LFO {ordinal:02}")
        }
        ModKind::Follower { track } => format!(
            "FOLLOW {:02} · {}",
            track + 1,
            song.tracks
                .get(track)
                .map_or("MISSING", |track| track.name.as_str())
        ),
    }
}

pub(super) fn rate_face(kind: ModKind) -> String {
    match kind {
        ModKind::Lfo { free: true, hz, .. } => {
            format!("{} HZ", super::chain::format_value(hz, ""))
        }
        ModKind::Lfo { rate_beats, .. } if rate_beats < 4.0 => {
            format!("{} BEAT", super::chain::format_value(rate_beats, ""))
        }
        ModKind::Lfo { rate_beats, .. } => {
            format!("{} BAR", super::chain::format_value(rate_beats / 4.0, ""))
        }
        ModKind::Follower { .. } => "AUDIO".to_owned(),
    }
}

fn step_index(values: &[f32], standing: f32, forward: bool) -> f32 {
    let at = values
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (**a - standing).abs().total_cmp(&(**b - standing).abs()))
        .map_or(0, |(at, _)| at);
    values[if forward {
        (at + 1).min(values.len() - 1)
    } else {
        at.saturating_sub(1)
    }]
}

fn previous_shape(shape: ModShape) -> ModShape {
    match shape {
        ModShape::Sine => ModShape::Square,
        ModShape::Triangle => ModShape::Sine,
        ModShape::Saw => ModShape::Triangle,
        ModShape::Square => ModShape::Saw,
    }
}

impl Stage {
    pub(super) fn open_modulation(&mut self) -> Result<(), RefusalReason> {
        if self.modulation.is_some() {
            self.modulation = None;
            self.mod_wire_gesture = false;
            return Ok(());
        }
        if self.song.tracks.is_empty() {
            return Err(RefusalReason::Empty);
        }
        let track = self.focused_track().unwrap_or(0);
        // This is a workspace, not a popover: it takes the field cleanly and
        // leaves no half-visible browser or device band underneath it.
        self.browser = None;
        self.chain = None;
        self.modulation = Some(Panel::open(&self.song, track));
        self.help = false;
        Ok(())
    }

    pub(super) fn close_modulation(&mut self) -> Result<(), RefusalReason> {
        let closed = self
            .modulation
            .take()
            .map(|_| ())
            .ok_or(RefusalReason::Unavailable);
        if closed.is_ok() {
            self.mod_wire_gesture = false;
        }
        closed
    }

    pub(super) fn tab_modulation(&mut self, backwards: bool) -> Result<(), RefusalReason> {
        let panel = self.modulation.as_mut().ok_or(RefusalReason::Unavailable)?;
        panel.tab(backwards);
        Ok(())
    }

    pub(super) fn step_modulation(&mut self, step: Step) -> Result<(), RefusalReason> {
        let panel = self.modulation.as_mut().ok_or(RefusalReason::Unavailable)?;
        panel.fit(&self.song);
        match (panel.focus, step) {
            (Focus::Sources, Step::Up | Step::Down) => {
                let down = step == Step::Down;
                let next = if down {
                    panel.source.saturating_add(1)
                } else {
                    panel.source.saturating_sub(1)
                };
                if next >= self.song.modulators.len() || next == panel.source {
                    Err(RefusalReason::Edge(step))
                } else {
                    panel.source = next;
                    Ok(())
                }
            }
            (Focus::Sources, Step::Left | Step::Right) => {
                let next = panel.source_field.step(step == Step::Right);
                if next == panel.source_field {
                    Err(RefusalReason::Edge(step))
                } else {
                    panel.source_field = next;
                    Ok(())
                }
            }
            (Focus::Targets, Step::Up | Step::Down) => {
                let count = targets(&self.song, panel.track).len();
                let down = step == Step::Down;
                let next = if down {
                    panel.target.saturating_add(1)
                } else {
                    panel.target.saturating_sub(1)
                };
                if next >= count || next == panel.target {
                    Err(RefusalReason::Edge(step))
                } else {
                    panel.target = next;
                    Ok(())
                }
            }
            (Focus::Targets, Step::Left | Step::Right) => {
                let right = step == Step::Right;
                let next = if right {
                    panel.track.saturating_add(1)
                } else {
                    panel.track.saturating_sub(1)
                };
                if next >= self.song.tracks.len() || next == panel.track {
                    Err(RefusalReason::Edge(step))
                } else {
                    panel.track = next;
                    panel.target = 0;
                    Ok(())
                }
            }
            (Focus::Response, Step::Up | Step::Down) => {
                let next = panel.control.step(step == Step::Down);
                if next == panel.control {
                    Err(RefusalReason::Edge(step))
                } else {
                    panel.control = next;
                    Ok(())
                }
            }
            (Focus::Response, Step::Left | Step::Right) => {
                self.adjust_mod_wire(step == Step::Right, false)
            }
        }
    }

    pub(super) fn select_mod_source(&mut self, source: usize) {
        if let Some(panel) = &mut self.modulation {
            panel.source = source.min(self.song.modulators.len().saturating_sub(1));
            panel.focus = Focus::Sources;
        }
    }

    pub(super) fn select_mod_source_field(&mut self, field: SourceField) {
        if let Some(panel) = &mut self.modulation {
            panel.source_field = field;
            panel.focus = Focus::Sources;
        }
    }

    pub(super) fn select_mod_target(&mut self, track: usize, target: usize) {
        if let Some(panel) = &mut self.modulation {
            panel.track = track.min(self.song.tracks.len().saturating_sub(1));
            panel.target = target.min(targets(&self.song, panel.track).len().saturating_sub(1));
            panel.focus = Focus::Targets;
        }
    }

    pub(super) fn select_mod_control(&mut self, control: WireControl) {
        if let Some(panel) = &mut self.modulation {
            panel.control = control;
            panel.focus = Focus::Response;
        }
    }

    pub(super) fn add_mod_lfo(&mut self) -> Result<(), RefusalReason> {
        if self.song.modulators_full() {
            self.notice = Some("MOD · 16 SOURCE LIMIT".to_owned());
            return Err(RefusalReason::Unavailable);
        }
        self.song.add_lfo().ok_or(RefusalReason::Unavailable)?;
        let panel = self.modulation.as_mut().ok_or(RefusalReason::Unavailable)?;
        panel.source = self.song.modulators.len() - 1;
        panel.focus = Focus::Sources;
        self.notice = Some("MOD · + LFO".to_owned());
        self.touched();
        Ok(())
    }

    pub(super) fn add_mod_follower(&mut self) -> Result<(), RefusalReason> {
        let track = self
            .modulation
            .as_ref()
            .map(|panel| panel.track)
            .ok_or(RefusalReason::Unavailable)?;
        if self.song.modulators_full() {
            self.notice = Some("MOD · 16 SOURCE LIMIT".to_owned());
            return Err(RefusalReason::Unavailable);
        }
        self.song
            .add_follower(track)
            .ok_or(RefusalReason::Unavailable)?;
        let panel = self.modulation.as_mut().expect("panel remains open");
        panel.source = self.song.modulators.len() - 1;
        panel.focus = Focus::Sources;
        self.notice = Some(format!("MOD · + FOLLOW {}", track + 1));
        self.touched();
        Ok(())
    }

    fn selected_modulator_index(&self) -> Result<usize, RefusalReason> {
        self.modulation
            .as_ref()
            .map(|panel| panel.source)
            .filter(|source| *source < self.song.modulators.len())
            .ok_or(RefusalReason::Empty)
    }

    pub(super) fn cycle_mod_shape(&mut self, forward: bool) -> Result<(), RefusalReason> {
        let at = self.selected_modulator_index()?;
        let ModKind::Lfo { shape, .. } = &mut self.song.modulators[at].kind else {
            return Err(RefusalReason::Unavailable);
        };
        *shape = if forward {
            shape.next()
        } else {
            previous_shape(*shape)
        };
        if let Some(panel) = &mut self.modulation {
            panel.source_field = SourceField::Shape;
        }
        self.notice = Some(format!("MOD · SHAPE {}", shape.label().to_uppercase()));
        self.remixed();
        Ok(())
    }

    pub(super) fn cycle_mod_rate(&mut self, faster: bool) -> Result<(), RefusalReason> {
        let at = self.selected_modulator_index()?;
        let kind = &mut self.song.modulators[at].kind;
        match kind {
            ModKind::Lfo { free: true, hz, .. } => *hz = step_index(&MOD_HZ, *hz, faster),
            ModKind::Lfo { rate_beats, .. } => {
                // More beats per cycle is slower.
                *rate_beats = step_index(&MOD_RATES, *rate_beats, !faster);
            }
            ModKind::Follower { .. } => return Err(RefusalReason::Unavailable),
        }
        let reading = rate_face(*kind);
        if let Some(panel) = &mut self.modulation {
            panel.source_field = SourceField::Rate;
        }
        self.notice = Some(format!("MOD · RATE {reading}"));
        self.remixed();
        Ok(())
    }

    pub(super) fn toggle_mod_clock(&mut self) -> Result<(), RefusalReason> {
        let at = self.selected_modulator_index()?;
        let ModKind::Lfo { free, .. } = &mut self.song.modulators[at].kind else {
            return Err(RefusalReason::Unavailable);
        };
        *free = !*free;
        let reading = if *free { "FREE" } else { "SYNC" };
        if let Some(panel) = &mut self.modulation {
            panel.source_field = SourceField::Mode;
        }
        self.notice = Some(format!("MOD · CLOCK {reading}"));
        self.remixed();
        Ok(())
    }

    pub(super) fn activate_mod_source_field(&mut self) -> Result<(), RefusalReason> {
        let field = self
            .modulation
            .as_ref()
            .map(|panel| panel.source_field)
            .ok_or(RefusalReason::Unavailable)?;
        match field {
            SourceField::Shape => self.cycle_mod_shape(true),
            SourceField::Rate => self.cycle_mod_rate(true),
            SourceField::Mode => self.toggle_mod_clock(),
        }
    }

    fn selected_mod_wire_index(&self) -> Result<usize, RefusalReason> {
        self.modulation
            .as_ref()
            .and_then(|panel| panel.wire_index(&self.song))
            .ok_or(RefusalReason::Empty)
    }

    pub(super) fn ensure_mod_wire(&mut self) -> Result<(), RefusalReason> {
        let panel = self
            .modulation
            .as_ref()
            .cloned()
            .ok_or(RefusalReason::Unavailable)?;
        if panel.wire_index(&self.song).is_none() {
            let source = panel.source_id(&self.song).ok_or(RefusalReason::Empty)?;
            let target = panel.target(&self.song).ok_or(RefusalReason::Empty)?;
            if self.song.mod_wires_full() {
                self.notice = Some("MOD · 64 PATCH LIMIT".to_owned());
                return Err(RefusalReason::Unavailable);
            }
            self.song
                .add_mod_wire(source, target.track, target.id.clone())
                .ok_or(RefusalReason::Unavailable)?;
            self.notice = Some(format!(
                "MOD · {} → {}",
                source_name(&self.song, panel.source),
                target.name
            ));
            self.touched();
        }
        if let Some(panel) = &mut self.modulation {
            panel.focus = Focus::Response;
        }
        Ok(())
    }

    pub(super) fn toggle_mod_wire(&mut self) -> Result<(), RefusalReason> {
        let panel = self
            .modulation
            .as_ref()
            .cloned()
            .ok_or(RefusalReason::Unavailable)?;
        if let Some(index) = panel.wire_index(&self.song) {
            let wire = self.song.mod_wires.remove(index);
            self.notice = Some(format!("MOD · UNPATCH {}", wire.target));
            if let Some(panel) = &mut self.modulation {
                panel.focus = Focus::Targets;
            }
            self.touched();
            Ok(())
        } else {
            self.ensure_mod_wire()
        }
    }

    pub(super) fn delete_mod_selection(&mut self) -> Result<(), RefusalReason> {
        let focus = self
            .modulation
            .as_ref()
            .map(|panel| panel.focus)
            .ok_or(RefusalReason::Unavailable)?;
        if focus == Focus::Sources {
            let at = self.selected_modulator_index()?;
            let id = self.song.modulators[at].id;
            self.song.remove_modulator(id);
            if let Some(panel) = &mut self.modulation {
                panel.fit(&self.song);
            }
            self.notice = Some(format!("MOD · - SOURCE {id}"));
            self.touched();
            Ok(())
        } else {
            let at = self.selected_mod_wire_index()?;
            let wire = self.song.mod_wires.remove(at);
            if let Some(panel) = &mut self.modulation {
                panel.focus = Focus::Targets;
            }
            self.notice = Some(format!("MOD · UNPATCH {}", wire.target));
            self.touched();
            Ok(())
        }
    }

    pub(super) fn toggle_mod_bypass(&mut self) -> Result<(), RefusalReason> {
        let at = self.selected_mod_wire_index()?;
        let wire = &mut self.song.mod_wires[at];
        wire.enabled = !wire.enabled;
        self.notice = Some(format!(
            "MOD · {}",
            if wire.enabled { "LIVE" } else { "BYPASS" }
        ));
        self.remixed();
        Ok(())
    }

    pub(super) fn toggle_mod_solo(&mut self) -> Result<(), RefusalReason> {
        let at = self.selected_mod_wire_index()?;
        let wire = &mut self.song.mod_wires[at];
        wire.solo = !wire.solo;
        self.notice = Some(format!(
            "MOD · SOLO {}",
            if wire.solo { "ON" } else { "OFF" }
        ));
        self.remixed();
        Ok(())
    }

    pub(super) fn adjust_mod_wire(
        &mut self,
        increase: bool,
        fine: bool,
    ) -> Result<(), RefusalReason> {
        let control = self
            .modulation
            .as_ref()
            .map(|panel| panel.control)
            .ok_or(RefusalReason::Unavailable)?;
        let at = self.selected_mod_wire_index()?;
        let wire = &mut self.song.mod_wires[at];
        let before = wire.clone();
        match control {
            WireControl::Depth => {
                let delta = if fine { 0.01 } else { 0.05 } * if increase { 1.0 } else { -1.0 };
                wire.depth = (wire.depth + delta).clamp(-1.0, 1.0);
            }
            WireControl::Curve => {
                let delta = if fine { 0.02 } else { 0.1 } * if increase { 1.0 } else { -1.0 };
                wire.curve = (wire.curve + delta).clamp(-1.0, 1.0);
            }
            WireControl::Steps => {
                const STEPS: [u32; 8] = [0, 2, 3, 4, 8, 16, 32, 64];
                let at = STEPS
                    .iter()
                    .position(|steps| *steps == wire.steps)
                    .unwrap_or(0);
                wire.steps = STEPS[if increase {
                    (at + 1).min(STEPS.len() - 1)
                } else {
                    at.saturating_sub(1)
                }];
            }
            WireControl::Smooth => {
                const MS: [f32; 11] = [
                    0.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 250.0, 500.0, 1_000.0, 2_000.0,
                ];
                wire.smooth_ms = step_index(&MS, wire.smooth_ms, increase);
            }
            WireControl::Enabled => wire.enabled = increase,
            WireControl::Solo => wire.solo = increase,
        }
        if *wire == before {
            return Err(RefusalReason::Edge(if increase {
                Step::Right
            } else {
                Step::Left
            }));
        }
        self.notice = Some(format!(
            "MOD · {} {}",
            control.label(),
            wire_control_face(wire, control)
        ));
        self.remixed();
        Ok(())
    }

    pub(super) fn activate_mod_wire_control(&mut self) -> Result<(), RefusalReason> {
        let control = self
            .modulation
            .as_ref()
            .map(|panel| panel.control)
            .ok_or(RefusalReason::Unavailable)?;
        let at = self.selected_mod_wire_index()?;
        let wire = &mut self.song.mod_wires[at];
        match control {
            WireControl::Depth => wire.depth = if wire.depth == 0.0 { 0.25 } else { -wire.depth },
            WireControl::Curve => wire.curve = 0.0,
            WireControl::Steps => return self.adjust_mod_wire(true, false),
            WireControl::Smooth => wire.smooth_ms = 0.0,
            WireControl::Enabled => wire.enabled = !wire.enabled,
            WireControl::Solo => wire.solo = !wire.solo,
        }
        self.notice = Some(format!(
            "MOD · {} {}",
            control.label(),
            wire_control_face(wire, control)
        ));
        self.remixed();
        Ok(())
    }

    pub(super) fn begin_mod_wire_gesture(&mut self) {
        self.mod_wire_gesture = true;
    }

    pub(super) fn end_mod_wire_gesture(&mut self) {
        if std::mem::take(&mut self.mod_wire_gesture) {
            self.settle();
        }
    }

    pub(super) fn set_mod_wire_fraction(
        &mut self,
        control: WireControl,
        fraction: f32,
    ) -> Result<(), RefusalReason> {
        let at = self.selected_mod_wire_index()?;
        let fraction = fraction.clamp(0.0, 1.0);
        let wire = &mut self.song.mod_wires[at];
        let before = wire.clone();
        match control {
            WireControl::Depth => wire.depth = fraction * 2.0 - 1.0,
            WireControl::Curve => wire.curve = fraction * 2.0 - 1.0,
            WireControl::Steps => wire.steps = (fraction * 64.0).round() as u32,
            WireControl::Smooth => wire.smooth_ms = fraction.powf(2.0) * 2_000.0,
            WireControl::Enabled => wire.enabled = fraction >= 0.5,
            WireControl::Solo => wire.solo = fraction >= 0.5,
        }
        if *wire != before {
            self.remixed();
            // A click is one edit. A drag is also one edit, however many
            // frames it spans; it settles when `drag_stopped` says the hand
            // lifted. Dirty immediately so even a close during the gesture
            // cannot mistake changed project data for a clean document.
            self.dirty = true;
            if !self.mod_wire_gesture {
                self.settle();
            }
        }
        Ok(())
    }
}

pub(super) fn wire_control_face(wire: &ModWire, control: WireControl) -> String {
    match control {
        WireControl::Depth => format!("{:+.0}%", wire.depth * 100.0),
        WireControl::Curve => format!("{:+.2}", wire.curve),
        WireControl::Steps if wire.steps < 2 => "OFF".to_owned(),
        WireControl::Steps => wire.steps.to_string(),
        WireControl::Smooth if wire.smooth_ms <= 0.0 => "OFF".to_owned(),
        WireControl::Smooth => format!("{:.0} MS", wire.smooth_ms),
        WireControl::Enabled => if wire.enabled { "LIVE" } else { "BYPASS" }.to_owned(),
        WireControl::Solo => if wire.solo { "SOLO" } else { "MIX" }.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_real_channel_control_is_offerable_and_instance_addressed() {
        let song = Song::default();
        let rows = targets(&song, 0);
        assert!(rows.iter().any(|target| target.id == "track.volume"));
        assert!(rows.iter().any(|target| target.id == "track.send.a"));
        let preamp = song
            .section(0, crate::console::SectionKind::Preamp)
            .expect("plain lane preamp")
            .id;
        let out = song
            .section(0, crate::console::SectionKind::Out)
            .expect("plain lane out")
            .id;
        assert!(
            rows.iter()
                .any(|target| target.id.starts_with(&format!("dev.{}.", preamp.0)))
        );
        assert!(
            rows.iter()
                .any(|target| target.id.starts_with(&format!("dev.{}.", out.0)))
        );
        assert!(rows.iter().any(|target| target.id.starts_with("dev.")));
        let pan = rows
            .iter()
            .find(|target| target.id == crate::targets::TRACK_PAN_TARGET)
            .unwrap();
        let send = rows
            .iter()
            .find(|target| crate::targets::track_send_index(&target.id).is_some())
            .unwrap();
        assert_eq!(pan.face(0.5), "50.0 %");
        assert_eq!(send.face(0.5), "50.0 %");
    }

    #[test]
    fn panel_cursors_follow_a_shrinking_song() {
        let mut song = Song::default();
        song.add_lfo();
        song.add_lfo();
        let mut panel = Panel::open(&song, 0);
        panel.source = 99;
        panel.track = 99;
        panel.target = 99;
        song.modulators.pop();
        panel.fit(&song);
        assert_eq!(panel.source, 0);
        assert_eq!(panel.track, 0);
        assert!(panel.target < targets(&song, 0).len());
    }

    #[test]
    fn one_route_is_unique_and_enters_the_response_zone() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        stage.open_modulation().expect("opens");
        stage.add_mod_lfo().expect("source");
        stage.ensure_mod_wire().expect("wire");
        assert_eq!(stage.song.mod_wires.len(), 1);
        assert_eq!(stage.modulation.as_ref().unwrap().focus, Focus::Response);
        stage.ensure_mod_wire().expect("existing wire is inspected");
        assert_eq!(stage.song.mod_wires.len(), 1);
    }

    #[test]
    fn response_arrows_change_the_sound_not_the_graph_shape() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        stage.open_modulation().unwrap();
        stage.add_mod_lfo().unwrap();
        stage.ensure_mod_wire().unwrap();
        let graph = stage.revision();
        let mix = stage.mix_revision();
        let before = stage.song.mod_wires[0].depth;
        stage.adjust_mod_wire(true, false).unwrap();
        assert!(stage.song.mod_wires[0].depth > before);
        assert_eq!(stage.revision(), graph);
        assert_ne!(stage.mix_revision(), mix);
    }

    #[test]
    fn workspace_flow_is_one_undoable_keyboard_model() {
        use crate::ui::stage::{ApplyOutcome, StageIntent};

        let mut stage = Stage::new();
        stage.set_palette_open(false);
        assert_eq!(stage.apply(StageIntent::Modulation), ApplyOutcome::Changed);
        assert_eq!(
            stage.scope_context(),
            super::super::keymap::ScopeContext::Modulation
        );
        assert_eq!(stage.apply(StageIntent::ModAddLfo), ApplyOutcome::Changed);
        assert_eq!(
            stage.apply(StageIntent::ModTab { backwards: false }),
            ApplyOutcome::Changed
        );
        assert_eq!(
            stage.apply(StageIntent::ModToggleWire),
            ApplyOutcome::Changed
        );
        assert_eq!(stage.song.mod_wires.len(), 1);
        let before = stage.song.mod_wires[0].depth;
        assert_eq!(
            stage.apply(StageIntent::ModAdjust {
                increase: true,
                fine: false,
            }),
            ApplyOutcome::Changed
        );
        assert!(stage.song.mod_wires[0].depth > before);
        assert_eq!(stage.apply(StageIntent::Undo), ApplyOutcome::Changed);
        assert_eq!(stage.song.mod_wires[0].depth, before);
        assert_eq!(
            stage.scope_context(),
            super::super::keymap::ScopeContext::Modulation
        );
    }

    #[test]
    fn engine_readings_are_id_stable_and_scoped_only_for_live_wires() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        stage.open_modulation().unwrap();
        let source = stage.song.add_lfo().unwrap();
        let wire = stage
            .song
            .add_mod_wire(source, 0, crate::targets::TRACK_VOLUME_TARGET)
            .unwrap();

        stage.set_modulation_readings(&[f32::NAN], &[0, wire, 9_999], &[9.0, 0.3, 7.0]);
        assert_eq!(stage.mod_source_values.get(&source), Some(&0.0));
        assert_eq!(stage.mod_wire_values.get(&wire), Some(&0.3));
        assert!(!stage.mod_wire_values.contains_key(&9_999));
        assert_eq!(
            stage.mod_wire_scopes.get(&wire).map(|scope| scope.len()),
            Some(1)
        );
        stage.clear_modulation_readings();
        assert!(stage.mod_source_values.is_empty());
        assert!(stage.mod_wire_values.is_empty());
        assert!(stage.mod_wire_scopes.is_empty());
    }

    #[test]
    fn a_section_target_scope_uses_the_engines_linear_domain() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        stage.open_modulation().unwrap();
        let cut = stage
            .song
            .section(0, crate::console::SectionKind::Cut)
            .expect("plain lane cut")
            .id;
        let spec = DeviceKind::Console(crate::console::SectionKind::Cut).spec();
        let def = spec
            .params
            .iter()
            .find(|def| def.id == crate::params::console::cut::HP_HZ)
            .expect("high-pass row");
        let target = crate::targets::device_target(cut.0, spec, def.name);
        let source = stage.song.add_lfo().unwrap();
        let wire = stage.song.add_mod_wire(source, 0, &target).unwrap();
        let row = targets(&stage.song, 0)
            .into_iter()
            .find(|candidate| candidate.id == target)
            .unwrap();
        let span = crate::audio::modulation::wire_span(row.min, row.max, false);

        stage.set_modulation_readings(&[0.0], &[wire], &[span * 0.5]);
        let plotted = stage.mod_wire_scopes[&wire].back().copied().unwrap();
        assert!((plotted - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn a_pointer_drag_is_one_history_step() {
        use crate::ui::stage::{ApplyOutcome, StageIntent};

        let mut stage = Stage::new();
        stage.set_palette_open(false);
        stage.open_modulation().unwrap();
        stage.add_mod_lfo().unwrap();
        stage.ensure_mod_wire().unwrap();
        stage.settle();
        let original = stage.song.mod_wires[0].depth;

        stage.begin_mod_wire_gesture();
        stage
            .set_mod_wire_fraction(WireControl::Depth, 0.75)
            .unwrap();
        stage
            .set_mod_wire_fraction(WireControl::Depth, 1.0)
            .unwrap();
        stage.end_mod_wire_gesture();
        assert_eq!(stage.song.mod_wires[0].depth, 1.0);
        assert_eq!(stage.apply(StageIntent::Undo), ApplyOutcome::Changed);
        assert_eq!(stage.song.mod_wires[0].depth, original);
    }
}
