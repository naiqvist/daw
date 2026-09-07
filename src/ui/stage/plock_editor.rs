//! Multi-cell parameter-lock editor state.
//!
//! This is a green-zone transaction: values preview into the mutable Song,
//! while `before` retains the exact lock vectors needed by Escape. The audio
//! graph only sees the normal immutable recompilation after those edits.

use std::collections::BTreeSet;

use crate::plock_ops::{self as ops, Curve, Range};
use crate::sequencing::{PATTERN_STEP_TICKS, ParamLock, Pattern, PatternId};

use super::trig_menu::LockRow;

/// One ordinary nudge crosses the useful range in forty presses. The old
/// hundred-press span made a working arrow look dead in an 80 px graph.
const COARSE_FRACTION: f32 = 0.025;
const FINE_FRACTION: f32 = 0.0025;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Focus {
    Parameters,
    Graphs,
    Controls,
}

impl Focus {
    fn next(self, backwards: bool) -> Self {
        match (self, backwards) {
            (Self::Parameters, false) | (Self::Graphs, true) => Self::Graphs,
            (Self::Graphs, false) | (Self::Parameters, true) => Self::Controls,
            (Self::Controls, false) | (Self::Controls, true) => Self::Parameters,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Algorithm {
    RampUp,
    RampDown,
    Crescendo,
    Decrescendo,
    Randomize,
    Spread,
    Compress,
    Rotate,
    Alternate,
    EveryNth,
    TowardKnob,
    Quantize,
}

impl Algorithm {
    pub(super) const ALL: [Self; 12] = [
        Self::RampUp,
        Self::RampDown,
        Self::Crescendo,
        Self::Decrescendo,
        Self::Randomize,
        Self::Spread,
        Self::Compress,
        Self::Rotate,
        Self::Alternate,
        Self::EveryNth,
        Self::TowardKnob,
        Self::Quantize,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::RampUp => "RAMP UP",
            Self::RampDown => "RAMP DOWN",
            Self::Crescendo => "CRESCENDO",
            Self::Decrescendo => "DECRESCENDO",
            Self::Randomize => "RANDOMIZE",
            Self::Spread => "SPREAD",
            Self::Compress => "COMPRESS",
            Self::Rotate => "ROTATE",
            Self::Alternate => "ALTERNATE",
            Self::EveryNth => "EVERY N",
            Self::TowardKnob => "TOWARD KNOB",
            Self::Quantize => "QUANTIZE",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Param {
    pub(super) device: Option<u64>,
    pub(super) id: u32,
    pub(super) name: String,
    pub(super) min: f32,
    pub(super) max: f32,
    pub(super) step: f32,
    pub(super) base: f32,
    pub(super) unit: &'static str,
    pub(super) choices: &'static [&'static str],
}

impl Param {
    pub(super) fn range(&self) -> Range {
        Range {
            min: self.min,
            max: self.max,
            step: self.step,
        }
    }

    pub(super) fn fraction(&self, value: f32) -> f32 {
        let span = self.max - self.min;
        if span <= 0.0 {
            0.0
        } else {
            ((value - self.min) / span).clamp(0.0, 1.0)
        }
    }

    /// A lock value in the same words as the device card: named choices
    /// stay named, continuous parameters retain their unit.
    pub(super) fn face(&self, value: f32) -> String {
        if !self.choices.is_empty() {
            let at = (value - self.min).round().max(0.0) as usize;
            return self.choices[at.min(self.choices.len() - 1)].to_uppercase();
        }
        crate::ui::stage::chain::format_value(value, self.unit.trim())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Editor {
    pub(super) pattern: PatternId,
    pub(super) track: usize,
    pub(super) ticks: Vec<usize>,
    pub(super) active: Vec<bool>,
    pub(super) params: Vec<Param>,
    pub(super) selected_params: BTreeSet<usize>,
    pub(super) param_cursor: usize,
    pub(super) graph_lane: usize,
    pub(super) graph_cell: usize,
    pub(super) control_cursor: usize,
    pub(super) focus: Focus,
    pub(super) picker: bool,
    pub(super) algorithm: Algorithm,
    pub(super) curve: Curve,
    /// Parameter-major lock values. `None` is intentionally unlocked.
    pub(super) locks: Vec<Vec<Option<f32>>>,
    before: Vec<(usize, Vec<ParamLock>)>,
    pub(super) dirty: bool,
    seed: u64,
    low: f32,
    high: f32,
    amount: f32,
    factor: f32,
    rotate: i32,
    every: usize,
    quantize: u32,
}

impl Editor {
    pub(super) fn open(
        pattern_id: PatternId,
        track: usize,
        ticks: impl IntoIterator<Item = usize>,
        rows: impl IntoIterator<Item = LockRow>,
        pattern: &Pattern,
    ) -> Option<Self> {
        let mut ticks: Vec<_> = ticks
            .into_iter()
            .map(|tick| tick / PATTERN_STEP_TICKS * PATTERN_STEP_TICKS)
            .filter(|tick| *tick < pattern.length_ticks)
            .collect();
        ticks.sort_unstable();
        ticks.dedup();
        if ticks.is_empty() {
            return None;
        }
        let rows: Vec<_> = rows.into_iter().collect();
        let params: Vec<_> = rows
            .iter()
            .map(|row| {
                let duplicated = rows
                    .iter()
                    .filter(|other| {
                        other.device == row.device && other.label.name == row.label.name
                    })
                    .count()
                    > 1;
                let parameter = if duplicated && !row.label.group.is_empty() {
                    format!("{} {}", row.label.group, row.label.name)
                } else {
                    row.label.name.to_owned()
                };
                let owner = row
                    .instance
                    .or_else(|| (!row.prefix.is_empty()).then_some(row.prefix));
                let name = owner.map_or(parameter.clone(), |owner| format!("{owner} {parameter}"));
                Param {
                    device: row.device.map(|device| device.0),
                    id: row.def.id,
                    name: name.to_uppercase(),
                    min: row.def.min,
                    max: row.def.max,
                    step: if row.label.choices.is_empty() {
                        0.0
                    } else {
                        1.0
                    },
                    base: row.knob,
                    unit: row.label.unit,
                    choices: row.label.choices,
                }
            })
            .collect();
        if params.is_empty() {
            return None;
        }
        let locks = params
            .iter()
            .map(|param| {
                ticks
                    .iter()
                    .map(|tick| {
                        pattern
                            .trig(tick / PATTERN_STEP_TICKS)
                            .lock_on(param.device.map(crate::sequencing::DeviceId), param.id)
                    })
                    .collect()
            })
            .collect();
        let before = ticks
            .iter()
            .map(|tick| {
                let step = tick / PATTERN_STEP_TICKS;
                (step, pattern.trig(step).locks.clone())
            })
            .collect();
        Some(Self {
            pattern: pattern_id,
            track,
            active: vec![true; ticks.len()],
            ticks,
            params,
            selected_params: BTreeSet::from([0]),
            param_cursor: 0,
            graph_lane: 0,
            graph_cell: 0,
            control_cursor: 0,
            focus: Focus::Parameters,
            picker: false,
            algorithm: Algorithm::RampUp,
            curve: Curve::Linear,
            locks,
            before,
            dirty: false,
            seed: 0xC0DE_5EED,
            low: 0.0,
            high: 1.0,
            amount: 1.0,
            factor: 2.0,
            rotate: 1,
            every: 2,
            quantize: 8,
        })
    }

    pub(super) fn selected_param_indices(&self) -> Vec<usize> {
        self.selected_params.iter().copied().collect()
    }

    pub(super) fn displayed(&self, param: usize, cell: usize) -> f32 {
        self.locks
            .get(param)
            .and_then(|values| values.get(cell))
            .and_then(|value| *value)
            .unwrap_or_else(|| self.params.get(param).map_or(0.0, |param| param.base))
    }

    /// How many addressed, active notes actually hold this parameter.
    pub(super) fn lock_count(&self, param: usize) -> usize {
        self.active
            .iter()
            .enumerate()
            .filter(|(cell, active)| {
                **active
                    && self
                        .locks
                        .get(param)
                        .and_then(|values| values.get(*cell))
                        .copied()
                        .flatten()
                        .is_some()
            })
            .count()
    }

    /// The active cells' standing value as one reading, or a range when the
    /// selection intentionally carries different locks.
    pub(super) fn value_text(&self, param: usize) -> String {
        let Some(def) = self.params.get(param) else {
            return "--".to_owned();
        };
        let mut values = self
            .active
            .iter()
            .enumerate()
            .filter_map(|(cell, active)| active.then_some(self.displayed(param, cell)));
        let Some(first) = values.next() else {
            return def.face(def.base);
        };
        let (mut low, mut high) = (first, first);
        for value in values {
            low = low.min(value);
            high = high.max(value);
        }
        let tolerance = if def.step > 0.0 {
            def.step * 0.25
        } else {
            (def.max - def.min).abs() * 0.000_05
        };
        if (high - low).abs() <= tolerance {
            def.face(first)
        } else {
            format!("{}…{}", def.face(low), def.face(high))
        }
    }

    /// Put the keyboard hand on a parameter. Pointer selection replaces the
    /// set by default; Shift-click can extend it without a second mode.
    pub(super) fn point_parameter(&mut self, index: usize, extend: bool) {
        if self.params.is_empty() {
            return;
        }
        self.param_cursor = index.min(self.params.len() - 1);
        self.focus = Focus::Parameters;
        self.picker = false;
        if extend {
            if !self.selected_params.remove(&self.param_cursor) {
                self.selected_params.insert(self.param_cursor);
            }
        } else {
            self.selected_params.clear();
            self.selected_params.insert(self.param_cursor);
        }
        self.graph_lane = self
            .selected_param_indices()
            .iter()
            .position(|param| *param == self.param_cursor)
            .unwrap_or(0);
    }

    /// Put the hand on one graph cell. Used by both direct pointing and the
    /// keyboard's explicit lane navigation.
    pub(super) fn point_graph(&mut self, lane: usize, cell: usize) {
        let lanes = self.selected_param_indices();
        if lanes.is_empty() || self.ticks.is_empty() {
            return;
        }
        self.graph_lane = lane.min(lanes.len() - 1);
        self.graph_cell = cell.min(self.ticks.len() - 1);
        self.focus = Focus::Graphs;
        self.picker = false;
    }

    pub(super) fn move_graph_lane(&mut self, down: bool) -> bool {
        let lanes = self.selected_params.len();
        if lanes == 0 {
            return false;
        }
        let before = self.graph_lane;
        self.graph_lane = if down {
            self.graph_lane.saturating_add(1).min(lanes - 1)
        } else {
            self.graph_lane.saturating_sub(1)
        };
        self.graph_lane != before
    }

    pub(super) fn tab(&mut self, backwards: bool) {
        self.focus = self.focus.next(backwards);
        self.picker = false;
        self.control_cursor = self
            .control_cursor
            .min(self.control_count().saturating_sub(1));
    }

    pub(super) fn toggle(&mut self) -> Option<(usize, bool)> {
        match self.focus {
            Focus::Parameters => {
                if !self.selected_params.remove(&self.param_cursor) {
                    self.selected_params.insert(self.param_cursor);
                }
                self.graph_lane = self
                    .graph_lane
                    .min(self.selected_params.len().saturating_sub(1));
                None
            }
            Focus::Graphs => {
                if let Some(active) = self.active.get_mut(self.graph_cell) {
                    *active = !*active;
                    return self
                        .ticks
                        .get(self.graph_cell)
                        .copied()
                        .map(|tick| (tick, *active));
                }
                None
            }
            Focus::Controls => None,
        }
    }

    /// Select everything in the zone the hand is currently in. In the
    /// parameter list that means every parameter; in the graph it means
    /// every addressed trig. This keeps Ctrl+A local and predictable.
    pub(super) fn select_all(&mut self) -> Vec<(usize, bool)> {
        match self.focus {
            Focus::Parameters | Focus::Controls => {
                self.selected_params = (0..self.params.len()).collect();
                self.graph_lane = self
                    .graph_lane
                    .min(self.selected_params.len().saturating_sub(1));
                Vec::new()
            }
            Focus::Graphs => {
                self.active.fill(true);
                self.ticks
                    .iter()
                    .copied()
                    .map(|tick| (tick, true))
                    .collect()
            }
        }
    }

    /// Delete is surgical in the graph and broad in the parameter list:
    /// one visible bar under the graph cursor, or every active cell for
    /// the selected parameter rows.
    pub(super) fn clear_locks(&mut self) -> bool {
        let mut changed = false;
        match self.focus {
            Focus::Graphs => {
                if let Some(&param) = self.selected_param_indices().get(self.graph_lane)
                    && self.active.get(self.graph_cell).copied().unwrap_or(false)
                    && self.locks[param][self.graph_cell].take().is_some()
                {
                    changed = true;
                }
            }
            Focus::Parameters => {
                // A plain list edit targets the row under the hand. Once a
                // multi-parameter set explicitly includes that row, Delete
                // becomes the useful bulk clear for the whole set.
                let params = if self.selected_params.len() > 1
                    && self.selected_params.contains(&self.param_cursor)
                {
                    self.selected_param_indices()
                } else {
                    vec![self.param_cursor]
                };
                for param in params {
                    for (cell, active) in self.active.iter().copied().enumerate() {
                        if active && self.locks[param][cell].take().is_some() {
                            changed = true;
                        }
                    }
                }
            }
            Focus::Controls => {
                for param in self.selected_param_indices() {
                    for (cell, active) in self.active.iter().copied().enumerate() {
                        if active && self.locks[param][cell].take().is_some() {
                            changed = true;
                        }
                    }
                }
            }
        }
        self.dirty |= changed;
        changed
    }

    pub(super) fn step(&mut self, vertical: i32, horizontal: i32, fine: bool) {
        if self.picker {
            let at = Algorithm::ALL
                .iter()
                .position(|algorithm| *algorithm == self.algorithm)
                .unwrap_or(0) as i32;
            let delta = if vertical != 0 { -vertical } else { horizontal };
            let len = Algorithm::ALL.len() as i32;
            self.algorithm = Algorithm::ALL[(at + delta).rem_euclid(len) as usize];
            return;
        }
        match self.focus {
            Focus::Parameters => {
                if vertical != 0 {
                    self.param_cursor = (self.param_cursor as i32 - vertical)
                        .clamp(0, self.params.len().saturating_sub(1) as i32)
                        as usize;
                }
                if horizontal != 0 {
                    self.adjust_parameter(self.param_cursor, horizontal > 0, fine);
                }
            }
            Focus::Graphs => {
                if horizontal != 0 {
                    self.graph_cell = (self.graph_cell as i32 + horizontal)
                        .clamp(0, self.ticks.len().saturating_sub(1) as i32)
                        as usize;
                }
                if vertical != 0 {
                    self.adjust_bar(vertical > 0, fine);
                }
            }
            Focus::Controls => {
                if horizontal != 0 {
                    self.control_cursor = (self.control_cursor as i32 + horizontal)
                        .clamp(0, self.control_count().saturating_sub(1) as i32)
                        as usize;
                }
                if vertical != 0 {
                    self.tune_control(vertical > 0, fine);
                    self.apply_algorithm();
                }
            }
        }
    }

    pub(super) fn extreme(&mut self, high: bool) {
        match self.focus {
            Focus::Parameters => {
                let param = self.param_cursor.min(self.params.len().saturating_sub(1));
                self.address_parameter(param);
                let value = if high {
                    self.params[param].max
                } else {
                    self.params[param].min
                };
                for (cell, active) in self.active.iter().copied().enumerate() {
                    if active {
                        self.locks[param][cell] = Some(value);
                        self.dirty = true;
                    }
                }
            }
            Focus::Graphs => {
                let Some(&param) = self.selected_param_indices().get(self.graph_lane) else {
                    return;
                };
                let value = if high {
                    self.params[param].max
                } else {
                    self.params[param].min
                };
                if self.active.get(self.graph_cell).copied().unwrap_or(false) {
                    self.locks[param][self.graph_cell] = Some(value);
                    self.dirty = true;
                }
            }
            Focus::Controls => {}
        }
    }

    fn include_parameter(&mut self, param: usize) {
        self.selected_params.insert(param);
        self.graph_lane = self
            .selected_param_indices()
            .iter()
            .position(|candidate| *candidate == param)
            .unwrap_or(0);
    }

    /// A direct value edit addresses the row under the hand. An explicit X
    /// inclusion survives because the row is already selected; merely walking
    /// away from the default row does not leave a ghost lane behind.
    fn address_parameter(&mut self, param: usize) {
        if !self.selected_params.contains(&param) {
            self.selected_params.clear();
        }
        self.include_parameter(param);
    }

    fn value_step(&self, param: usize, fine: bool) -> f32 {
        let range = self.params[param].range();
        if range.step > 0.0 {
            range.step
        } else {
            range.span() * if fine { FINE_FRACTION } else { COARSE_FRACTION }
        }
    }

    fn adjust_parameter(&mut self, param: usize, up: bool, fine: bool) {
        if param >= self.params.len() {
            return;
        }
        self.address_parameter(param);
        let range = self.params[param].range();
        let delta = self.value_step(param, fine) * if up { 1.0 } else { -1.0 };
        for cell in 0..self.active.len() {
            if !self.active[cell] {
                continue;
            }
            let value = range.hold(self.displayed(param, cell) + delta);
            self.locks[param][cell] = Some(value);
            self.dirty = true;
        }
    }

    /// Direct manipulation in the graph: a vertical fraction becomes the
    /// parameter's value and a lock immediately, exactly like an arrow edit.
    pub(super) fn set_graph_fraction(&mut self, lane: usize, cell: usize, fraction: f32) -> bool {
        let Some(&param) = self.selected_param_indices().get(lane) else {
            return false;
        };
        if !self.active.get(cell).copied().unwrap_or(false) {
            return false;
        }
        self.point_graph(lane, cell);
        let def = &self.params[param];
        let value = def
            .range()
            .hold(def.min + def.range().span() * fraction.clamp(0.0, 1.0));
        let changed = self.locks[param].get(cell).copied().flatten() != Some(value);
        self.locks[param][cell] = Some(value);
        self.dirty |= changed;
        changed
    }

    fn adjust_bar(&mut self, up: bool, fine: bool) {
        let Some(&param) = self.selected_param_indices().get(self.graph_lane) else {
            return;
        };
        if !self.active.get(self.graph_cell).copied().unwrap_or(false) {
            return;
        }
        let range = self.params[param].range();
        let step = self.value_step(param, fine);
        let value = self.displayed(param, self.graph_cell) + if up { step } else { -step };
        self.locks[param][self.graph_cell] = Some(range.hold(value));
        self.dirty = true;
    }

    fn tune_control(&mut self, up: bool, fine: bool) {
        let direction = if up { 1.0 } else { -1.0 };
        let step = if fine { 0.01 } else { 0.1 };
        match self.algorithm {
            Algorithm::RampUp | Algorithm::RampDown => match self.control_cursor {
                0 => self.low = (self.low + direction * step).clamp(0.0, self.high),
                1 => self.high = (self.high + direction * step).clamp(self.low, 1.0),
                _ => {
                    self.curve = match (self.curve, up) {
                        (Curve::Linear, true) | (Curve::Logarithmic, false) => Curve::Exponential,
                        (Curve::Exponential, true) | (Curve::S, false) => Curve::Logarithmic,
                        (Curve::Logarithmic, true) | (Curve::Linear, false) => Curve::S,
                        (Curve::S, true) | (Curve::Exponential, false) => Curve::Linear,
                    }
                }
            },
            Algorithm::Randomize => match self.control_cursor {
                0 => self.low = (self.low + direction * step).clamp(0.0, self.high),
                1 => self.high = (self.high + direction * step).clamp(self.low, 1.0),
                _ => self.amount = (self.amount + direction * step).clamp(0.0, 1.0),
            },
            Algorithm::Spread => self.factor = (self.factor + direction * step).clamp(1.0, 8.0),
            Algorithm::Compress => self.factor = (self.factor + direction * step).clamp(0.0, 1.0),
            Algorithm::Rotate => {
                self.rotate = (self.rotate + if up { 1 } else { -1 }).clamp(-64, 64)
            }
            Algorithm::EveryNth => {
                self.every = self
                    .every
                    .saturating_add_signed(if up { 1 } else { -1 })
                    .max(1)
            }
            Algorithm::Quantize => {
                self.quantize = self
                    .quantize
                    .saturating_add_signed(if up { 1 } else { -1 })
                    .max(2)
            }
            Algorithm::Crescendo | Algorithm::Decrescendo | Algorithm::TowardKnob => {
                self.amount = (self.amount + direction * step).clamp(0.0, 1.0)
            }
            Algorithm::Alternate => match self.control_cursor {
                0 => self.low = (self.low + direction * step).clamp(0.0, self.high),
                _ => self.high = (self.high + direction * step).clamp(self.low, 1.0),
            },
        }
    }

    fn control_count(&self) -> usize {
        match self.algorithm {
            Algorithm::RampUp | Algorithm::RampDown | Algorithm::Randomize => 3,
            Algorithm::Alternate => 2,
            _ => 1,
        }
    }

    pub(super) fn open_picker(&mut self) {
        self.picker = true;
        self.focus = Focus::Controls;
    }

    pub(super) fn picker_enter(&mut self) {
        self.picker = false;
        self.control_cursor = self
            .control_cursor
            .min(self.control_count().saturating_sub(1));
        self.apply_algorithm();
    }

    pub(super) fn picker_escape(&mut self) {
        self.picker = false;
    }

    pub(super) fn apply_algorithm(&mut self) {
        let active_indices: Vec<_> = self
            .active
            .iter()
            .enumerate()
            .filter_map(|(index, active)| active.then_some(index))
            .collect();
        if active_indices.is_empty() {
            return;
        }
        let positions: Vec<_> = active_indices
            .iter()
            .map(|index| self.ticks[*index])
            .collect();
        let first_position = self.ticks[0];
        let last_position = *self.ticks.last().unwrap_or(&first_position);
        let selected = self.selected_param_indices();
        for param_index in selected {
            let param = &self.params[param_index];
            let range = param.range();
            let mut values: Vec<_> = active_indices
                .iter()
                .map(|index| self.displayed(param_index, *index))
                .collect();
            let mut keep = None;
            match self.algorithm {
                Algorithm::RampUp | Algorithm::RampDown => {
                    let (first, last) = if self.algorithm == Algorithm::RampUp {
                        (
                            range.hold(range.min + range.span() * self.low),
                            range.hold(range.min + range.span() * self.high),
                        )
                    } else {
                        (
                            range.hold(range.min + range.span() * self.high),
                            range.hold(range.min + range.span() * self.low),
                        )
                    };
                    ops::ramp_positions(
                        &mut values,
                        &positions,
                        first_position,
                        last_position,
                        first,
                        last,
                        self.curve,
                        range,
                    );
                }
                Algorithm::Crescendo => ops::crescendo(&mut values, range, true),
                Algorithm::Decrescendo => ops::crescendo(&mut values, range, false),
                Algorithm::Randomize => {
                    self.seed = self
                        .seed
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1);
                    ops::randomize_range(
                        &mut values,
                        range.min + range.span() * self.low,
                        range.min + range.span() * self.high,
                        self.amount,
                        range,
                        self.seed ^ param_index as u64,
                    );
                }
                Algorithm::Spread => ops::spread(&mut values, self.factor.max(1.0), range),
                Algorithm::Compress => ops::spread(&mut values, self.factor.min(1.0), range),
                Algorithm::Rotate => ops::rotate(&mut values, self.rotate),
                Algorithm::Alternate => ops::alternate(
                    &mut values,
                    range.min + range.span() * self.low,
                    range.min + range.span() * self.high,
                    range,
                ),
                Algorithm::EveryNth => keep = Some(ops::every_nth(values.len(), self.every, 0)),
                Algorithm::TowardKnob => {
                    ops::scale_toward(&mut values, param.base, self.amount, range)
                }
                Algorithm::Quantize => ops::quantize(&mut values, self.quantize, range),
            }
            for (slot, &cell) in active_indices.iter().enumerate() {
                self.locks[param_index][cell] = if keep
                    .as_ref()
                    .is_none_or(|mask| mask.get(slot).copied().unwrap_or(true))
                {
                    values.get(slot).copied()
                } else {
                    None
                };
            }
        }
        self.dirty = true;
    }

    /// Rebuild the edited parameters from the transaction state. Inactive
    /// holes restore their exact baseline values rather than receiving a
    /// compressed algorithm write.
    pub(super) fn preview(&self, pattern: &mut Pattern) {
        for (cell, tick) in self.ticks.iter().enumerate() {
            let step = tick / PATTERN_STEP_TICKS;
            for (param_index, param) in self.params.iter().enumerate() {
                let baseline = self.before[cell]
                    .1
                    .iter()
                    .find(|lock| {
                        lock.device.map(|device| device.0) == param.device && lock.param == param.id
                    })
                    .map(|lock| lock.value);
                let desired = if self.active[cell] {
                    self.locks[param_index][cell]
                } else {
                    baseline
                };
                match desired {
                    Some(value) => pattern.trig_mut(step).set_lock_on(
                        param.device.map(crate::sequencing::DeviceId),
                        param.id,
                        value,
                    ),
                    None => {
                        pattern
                            .trig_mut(step)
                            .clear_lock_on(param.device.map(crate::sequencing::DeviceId), param.id);
                    }
                }
            }
        }
    }

    pub(super) fn cancel(&self, pattern: &mut Pattern) {
        for (step, locks) in &self.before {
            pattern.trig_mut(*step).locks = locks.clone();
        }
    }

    pub(super) fn control_text(&self) -> String {
        let marker = |index: usize, text: String| {
            if self.control_cursor == index {
                format!("[{text}]")
            } else {
                text
            }
        };
        match self.algorithm {
            Algorithm::RampUp | Algorithm::RampDown => format!(
                "{} → {}  {}",
                marker(0, format!("{:>3}%", (self.low * 100.0) as u32)),
                marker(1, format!("{:>3}%", (self.high * 100.0) as u32)),
                marker(2, format!("{:?}", self.curve))
            ),
            Algorithm::Randomize => format!(
                "{}..{}  AMOUNT {}",
                marker(0, format!("{:>3}%", (self.low * 100.0) as u32)),
                marker(1, format!("{:>3}%", (self.high * 100.0) as u32)),
                marker(2, format!("{:>3}%", (self.amount * 100.0) as u32))
            ),
            Algorithm::Spread | Algorithm::Compress => format!("FACTOR {:.2}", self.factor),
            Algorithm::Rotate => format!("BY {:+}", self.rotate),
            Algorithm::EveryNth => format!("EVERY {}", self.every),
            Algorithm::TowardKnob => format!("AMOUNT {:>3}%", (self.amount * 100.0) as u32),
            Algorithm::Quantize => format!("{} DIVISIONS", self.quantize),
            Algorithm::Alternate => format!(
                "{} ↔ {}",
                marker(0, format!("{:>3}%", (self.low * 100.0) as u32)),
                marker(1, format!("{:>3}%", (self.high * 100.0) as u32))
            ),
            _ => "↑↓ ADJUST".to_owned(),
        }
    }
}

/// A centred, cursor-following slice of a long list. The caller supplies the
/// capacity its real rectangle can show, so no model-side pixel state leaks
/// into the editor transaction.
pub(super) fn visible_span(cursor: usize, len: usize, capacity: usize) -> (usize, usize) {
    let capacity = capacity.max(1).min(len.max(1));
    if len <= capacity {
        return (0, len);
    }
    let start = cursor
        .saturating_sub(capacity / 2)
        .min(len.saturating_sub(capacity));
    (start, (start + capacity).min(len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::DeviceKind;
    use crate::sequencing::Song;
    use crate::ui::stage::trig_menu;

    fn editor() -> (Pattern, Editor) {
        let mut song = Song::default();
        song.add_device(0, DeviceKind::Poly).expect("poly device");
        let track = &song.tracks[0];
        let pattern = Pattern::default();
        let rows = trig_menu::menu_rows(track, pattern.trig(0))
            .into_iter()
            .filter_map(|row| match row {
                trig_menu::MenuRow::Param(row) => Some(row),
                _ => None,
            });
        let editor = Editor::open(
            pattern.id,
            0,
            [0, PATTERN_STEP_TICKS, PATTERN_STEP_TICKS * 2],
            rows,
            &pattern,
        )
        .expect("editor");
        (pattern, editor)
    }

    #[test]
    fn graph_edits_create_trigless_locks_and_cancel_restores_exactly() {
        let (mut pattern, mut editor) = editor();
        editor.focus = Focus::Graphs;
        editor.step(1, 0, false);
        editor.preview(&mut pattern);
        assert!(pattern.trig(0).notes.is_empty());
        assert!(!pattern.trig(0).locks.is_empty());
        editor.cancel(&mut pattern);
        assert!(pattern.trig(0).locks.is_empty());
    }

    #[test]
    fn ramp_keeps_hole_timing_and_writes_several_parameters() {
        let (mut pattern, mut editor) = editor();
        editor.selected_params.insert(1);
        editor.active[1] = false;
        editor.algorithm = Algorithm::RampUp;
        editor.apply_algorithm();
        editor.preview(&mut pattern);
        assert!(pattern.trig(0).locks.len() >= 2);
        assert!(pattern.trig(1).locks.is_empty(), "the hole received locks");
        assert!(pattern.trig(2).locks.len() >= 2);
    }

    #[test]
    fn controls_expose_range_amount_and_curve_from_the_keyboard() {
        let (_, mut editor) = editor();
        editor.focus = Focus::Controls;
        editor.algorithm = Algorithm::Randomize;

        editor.step(1, 0, false);
        assert_eq!(editor.low, 0.1);
        editor.step(0, 1, false);
        editor.step(-1, 0, false);
        assert_eq!(editor.high, 0.9);
        editor.step(0, 1, false);
        editor.step(-1, 0, true);
        assert!((editor.amount - 0.99).abs() < 1e-6);

        editor.algorithm = Algorithm::RampUp;
        editor.control_cursor = 2;
        editor.step(1, 0, false);
        assert_eq!(editor.curve, Curve::Exponential);
    }

    #[test]
    fn select_all_and_delete_follow_the_focused_zone() {
        let (_, mut editor) = editor();
        editor.select_all();
        assert_eq!(editor.selected_params.len(), editor.params.len());

        editor.focus = Focus::Graphs;
        editor.active[1] = false;
        assert_eq!(editor.select_all().len(), editor.ticks.len());
        assert!(editor.active.iter().all(|active| *active));

        editor.locks[0][0] = Some(editor.params[0].base);
        editor.graph_lane = 0;
        editor.graph_cell = 0;
        assert!(editor.clear_locks());
        assert_eq!(editor.locks[0][0], None);
        assert!(editor.dirty);
    }

    #[test]
    fn parameter_arrows_navigate_then_edit_the_row_under_the_hand() {
        let (_, mut editor) = editor();
        let base = editor.params[1].base;

        editor.step(-1, 0, false);
        assert_eq!(editor.param_cursor, 1, "Down did not choose the next row");
        editor.step(0, 1, false);

        assert_eq!(editor.selected_params, BTreeSet::from([1]));
        assert!(editor.locks[1].iter().all(Option::is_some));
        assert!(
            editor.displayed(1, 0) > base,
            "Right did not raise the lock"
        );
        assert!(editor.dirty);
    }

    #[test]
    fn graph_lanes_can_be_addressed_and_edited_independently() {
        let (_, mut editor) = editor();
        editor.selected_params.insert(1);
        editor.focus = Focus::Graphs;
        let first = editor.locks[0].clone();

        assert!(editor.move_graph_lane(true));
        assert_eq!(editor.graph_lane, 1);
        editor.step(1, 0, false);

        assert_eq!(editor.locks[0], first);
        assert!(editor.locks[1][0].is_some());
    }

    #[test]
    fn a_long_parameter_window_always_contains_its_cursor() {
        assert_eq!(visible_span(0, 62, 12), (0, 12));
        assert_eq!(visible_span(31, 62, 12), (25, 37));
        assert_eq!(visible_span(61, 62, 12), (50, 62));
        assert_eq!(visible_span(1, 3, 12), (0, 3));
    }
}
