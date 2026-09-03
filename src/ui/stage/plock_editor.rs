//! Multi-cell parameter-lock editor state.
//!
//! This is a green-zone transaction: values preview into the mutable Song,
//! while `before` retains the exact lock vectors needed by Escape. The audio
//! graph only sees the normal immutable recompilation after those edits.

use std::collections::BTreeSet;

use crate::plock_ops::{self as ops, Curve, Range};
use crate::sequencing::{PATTERN_STEP_TICKS, ParamLock, Pattern, PatternId};

use super::trig_menu::LockRow;

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
        let params: Vec<_> = rows
            .into_iter()
            .map(|row| Param {
                device: row.device.map(|device| device.0),
                id: row.def.id,
                name: if row.prefix.is_empty() {
                    row.label.name.to_uppercase()
                } else {
                    format!("{} {}", row.prefix, row.label.name).to_uppercase()
                },
                min: row.def.min,
                max: row.def.max,
                step: if row.label.choices.is_empty() {
                    0.0
                } else {
                    1.0
                },
                base: row.knob,
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

    pub(super) fn step(&mut self, vertical: i32, horizontal: i32, fine: bool) {
        if self.picker {
            let at = Algorithm::ALL
                .iter()
                .position(|algorithm| *algorithm == self.algorithm)
                .unwrap_or(0) as i32;
            let delta = if vertical != 0 { vertical } else { horizontal };
            let len = Algorithm::ALL.len() as i32;
            self.algorithm = Algorithm::ALL[(at + delta).rem_euclid(len) as usize];
            return;
        }
        match self.focus {
            Focus::Parameters => {
                self.param_cursor = (self.param_cursor as i32 - vertical)
                    .clamp(0, self.params.len().saturating_sub(1) as i32)
                    as usize;
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
        if self.focus != Focus::Graphs {
            return;
        }
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

    fn adjust_bar(&mut self, up: bool, fine: bool) {
        let Some(&param) = self.selected_param_indices().get(self.graph_lane) else {
            return;
        };
        if !self.active.get(self.graph_cell).copied().unwrap_or(false) {
            return;
        }
        let range = self.params[param].range();
        let step = if range.step > 0.0 {
            range.step
        } else if fine {
            range.span() / 1000.0
        } else {
            range.span() / 100.0
        };
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
}
