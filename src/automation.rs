//! Track automation: envelopes drawn against the timeline.
//!
//! An envelope is a list of points against one target name, and
//! `TrackAutomation` is every envelope one track carries. The value at a
//! beat is interpolated here rather than by the caller, so the arrangement
//! view and a bounce read the same curve.
//!
//! `TrackAutomationWire` is the on-disk shape, kept separate so the
//! hand-written `Deserialize` can migrate older files.
//!
//! Lifted out of `main.rs` unchanged.

use crate::targets::{TRACK_PAN_TARGET, TRACK_VOLUME_TARGET};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AutomationPoint {
    pub beat: f32,
    pub value: f32,
    /// A continuous bend edited through Alt/Option-dragging the segment.
    /// Zero is a straight line; positive and negative values bow it either
    /// side without adding a visible handle or a competing curve menu.
    pub bend: f32,
}

impl Default for AutomationPoint {
    fn default() -> Self {
        Self {
            beat: 0.0,
            value: 0.0,
            bend: 0.0,
        }
    }
}

#[derive(Clone, Default, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AutomationEnvelope {
    pub target: String,
    pub points: Vec<AutomationPoint>,
}

#[derive(Clone, Default, Debug, PartialEq, serde::Serialize)]
pub struct TrackAutomation {
    pub envelopes: Vec<AutomationEnvelope>,
}

/// Compatibility shape for projects written before target ids became
/// generic. Unknown future fields remain harmless through serde defaults.
#[derive(Default, serde::Deserialize)]
#[serde(default)]
pub struct TrackAutomationWire {
    pub envelopes: Vec<AutomationEnvelope>,
    pub volume: Vec<AutomationPoint>,
    pub pan: Vec<AutomationPoint>,
}

impl<'de> serde::Deserialize<'de> for TrackAutomation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut wire = TrackAutomationWire::deserialize(deserializer)?;
        if !wire.volume.is_empty()
            && !wire
                .envelopes
                .iter()
                .any(|envelope| envelope.target == TRACK_VOLUME_TARGET)
        {
            wire.envelopes.push(AutomationEnvelope {
                target: TRACK_VOLUME_TARGET.to_owned(),
                points: wire.volume,
            });
        }
        if !wire.pan.is_empty()
            && !wire
                .envelopes
                .iter()
                .any(|envelope| envelope.target == TRACK_PAN_TARGET)
        {
            wire.envelopes.push(AutomationEnvelope {
                target: TRACK_PAN_TARGET.to_owned(),
                points: wire.pan,
            });
        }
        Ok(Self {
            envelopes: wire.envelopes,
        })
    }
}

impl TrackAutomation {
    pub fn points(&self, target: &str) -> &[AutomationPoint] {
        self.envelopes
            .iter()
            .find(|envelope| envelope.target == target)
            .map_or(&[], |envelope| envelope.points.as_slice())
    }

    pub fn points_mut(&mut self, target: &str) -> &mut Vec<AutomationPoint> {
        if let Some(index) = self
            .envelopes
            .iter()
            .position(|envelope| envelope.target == target)
        {
            return &mut self.envelopes[index].points;
        }
        self.envelopes.push(AutomationEnvelope {
            target: target.to_owned(),
            points: Vec::new(),
        });
        &mut self.envelopes.last_mut().expect("just inserted").points
    }

    /// Hold the manual value until the first point, follow each point's
    /// outgoing curve, then hold the final value.
    pub fn value_at(&self, target: &str, beat: f32, base: f32) -> f32 {
        let points = self.points(target);
        let Some(first) = points.first() else {
            return base;
        };
        if beat < first.beat {
            return base;
        }
        for pair in points.windows(2) {
            let [a, b] = pair else { continue };
            if beat <= b.beat {
                let t = ((beat - a.beat) / (b.beat - a.beat).max(f32::EPSILON)).clamp(0.0, 1.0);
                let bent = if a.bend >= 0.0 {
                    t.powf(1.0 + a.bend * 5.0)
                } else {
                    1.0 - (1.0 - t).powf(1.0 + -a.bend * 5.0)
                };
                return a.value + (b.value - a.value) * bent;
            }
        }
        points.last().map_or(base, |point| point.value)
    }

    pub fn insert(&mut self, target: &str, beat: f32, value: f32) {
        let points = self.points_mut(target);
        let at = points.partition_point(|point| point.beat < beat - 1e-3);
        if points
            .get(at)
            .is_some_and(|point| (point.beat - beat).abs() < 1e-3)
        {
            points[at].value = value;
        } else {
            points.insert(
                at,
                AutomationPoint {
                    beat,
                    value,
                    bend: 0.0,
                },
            );
        }
    }

    /// Materialize a breakpoint where an edit cuts through an active
    /// envelope. No point is invented before the envelope starts or after
    /// it has finished changing.
    pub fn split_at(&mut self, target: &str, beat: f32, base: f32) {
        let points = self.points(target);
        let Some(first) = points.first() else { return };
        let Some(last) = points.last() else { return };
        if beat <= first.beat || beat >= last.beat {
            return;
        }
        let value = self.value_at(target, beat, base);
        self.insert(target, beat, value);
    }

    pub fn insert_time(&mut self, target: &str, at: f32, amount: f32, base: f32) {
        let points = self.points(target);
        let Some(first) = points.first() else { return };
        let Some(last) = points.last() else { return };
        if at > last.beat {
            return;
        }
        let value = (at >= first.beat).then(|| self.value_at(target, at, base));
        for point in self.points_mut(target) {
            if point.beat >= at {
                point.beat += amount;
            }
        }
        if let Some(value) = value {
            self.insert(target, at, value);
        }
    }

    pub fn delete_time(&mut self, target: &str, from: f32, to: f32, base: f32) {
        let points = self.points(target);
        let Some(first) = points.first() else { return };
        let Some(last) = points.last() else { return };
        let active_after_cut = first.beat <= to && last.beat >= to;
        let value_after_cut = active_after_cut.then(|| self.value_at(target, to, base));
        let span = to - from;
        let points = self.points_mut(target);
        points.retain(|point| point.beat < from || point.beat >= to);
        for point in points {
            if point.beat >= to {
                point.beat -= span;
            }
        }
        if let Some(value) = value_after_cut {
            self.insert(target, from, value);
        }
    }

    pub fn copy_span(&self, target: &str, from: f32, to: f32) -> Vec<AutomationPoint> {
        self.points(target)
            .iter()
            .filter(|point| point.beat >= from && point.beat < to)
            .cloned()
            .collect()
    }

    pub fn insert_points(
        &mut self,
        target: &str,
        points: impl IntoIterator<Item = AutomationPoint>,
    ) {
        let target_points = self.points_mut(target);
        for point in points {
            let at = target_points.partition_point(|existing| existing.beat < point.beat - 1e-3);
            if target_points
                .get(at)
                .is_some_and(|existing| (existing.beat - point.beat).abs() < 1e-3)
            {
                target_points[at] = point;
            } else {
                target_points.insert(at, point);
            }
        }
    }

    pub fn targets(&self) -> Vec<String> {
        self.envelopes
            .iter()
            .map(|envelope| envelope.target.clone())
            .collect()
    }

    pub fn copy_all_span(&self, from: f32, to: f32) -> Vec<AutomationEnvelope> {
        self.envelopes
            .iter()
            .filter_map(|envelope| {
                let points = self.copy_span(&envelope.target, from, to);
                (!points.is_empty()).then(|| AutomationEnvelope {
                    target: envelope.target.clone(),
                    points,
                })
            })
            .collect()
    }

    pub fn insert_envelopes(&mut self, envelopes: Vec<AutomationEnvelope>, offset: f32) {
        for envelope in envelopes {
            self.insert_points(
                &envelope.target,
                envelope.points.into_iter().map(|mut point| {
                    point.beat += offset;
                    point
                }),
            );
        }
    }
}
