//! The forge: sCOMP's own room, where a sine is watched becoming a
//! sound.
//!
//! One instrument, full screen. The passes of its take stand stacked on
//! a shared time axis — the sine at the top, then each bounce over the
//! one before — so the resampling's stretch and the compressor's
//! flattening are seen, not inferred. Beside them, every knob as a row;
//! the arrows turn the one under the cursor and the picture is rendered
//! again, from the same function the engine renders from, so what is
//! drawn is what will play.
//!
//! What lives here: the room's state (which device, which row, which
//! pass is on show), the render cache keyed on the baked knobs, and the
//! peaks of every pass. The painter is `view::forge`.

use std::sync::Arc;

use crate::params::scomp as sp;
use crate::sample_peaks::Peaks;
use crate::scomp::{Baked, ScompParams, Take};
use crate::sequencing::{Device, DeviceId};

/// The rate the picture is rendered at. The engine renders at its own;
/// the take is the same shape at any.
pub(super) const RENDER_RATE: u32 = 48_000;

/// The rows, in signal order, each with the group it sits under.
pub(super) const ROWS: [(&str, u32); 22] = [
    ("SOURCE", sp::PASSES),
    ("SOURCE", sp::TAKE),
    ("SOURCE", sp::DROP),
    ("SOURCE", sp::DROP_MS),
    ("SOURCE", sp::DECAY),
    ("FILTER", sp::FILTER),
    ("FILTER", sp::HARMONIC),
    ("FILTER", sp::RESO),
    ("FILTER", sp::SWEEP),
    ("FILTER", sp::DRIFT),
    ("SQUASH", sp::THRESH),
    ("SQUASH", sp::RATIO),
    ("SQUASH", sp::ATTACK),
    ("SQUASH", sp::RELEASE),
    ("SQUASH", sp::MAKEUP),
    ("CLIP", sp::DRIVE),
    ("BOUNCE", sp::SHIFT),
    ("PLAY", sp::AMP_A),
    ("PLAY", sp::AMP_R),
    ("PLAY", sp::TUNE),
    ("PLAY", sp::ROOT),
    ("PLAY", sp::LEVEL),
];

/// The room, while it is up.
#[derive(Clone, Debug)]
pub(super) struct Forge {
    pub(super) track: usize,
    pub(super) device: DeviceId,
    /// The row under the cursor, an index into [`ROWS`].
    pub(super) row: usize,
    /// The pass on show, counted from the source at zero; `None` is the
    /// last, whatever the count becomes.
    pub(super) pass: Option<usize>,
    key: Option<Baked>,
    pub(super) take: Option<Take>,
    pub(super) peaks: Vec<Arc<Peaks>>,
}

impl Forge {
    pub(super) fn open(track: usize, device: DeviceId) -> Self {
        Self {
            track,
            device,
            row: 0,
            pass: None,
            key: None,
            take: None,
            peaks: Vec::new(),
        }
    }

    /// The knobs as the device holds them: the table's defaults under
    /// its overrides.
    pub(super) fn params_of(device: &Device) -> ScompParams {
        let mut params = ScompParams::default();
        for (id, value) in &device.overrides {
            params.set(*id, *value);
        }
        params
    }

    /// Render again if the baked knobs moved. Returns whether it did.
    pub(super) fn refresh(&mut self, device: &Device) -> bool {
        let params = Self::params_of(device);
        let key = params.baked();
        if self.key == Some(key) && self.take.is_some() {
            return false;
        }
        let take = crate::scomp::render(&params, RENDER_RATE);
        self.peaks = take
            .passes
            .iter()
            .map(|pass| Arc::new(Peaks::build(pass, 1, pass.len() as u64, RENDER_RATE)))
            .collect();
        self.take = Some(take);
        self.key = Some(key);
        true
    }

    pub(super) fn param(&self) -> u32 {
        ROWS.get(self.row).map_or(sp::PASSES, |(_, id)| *id)
    }

    /// How many passes the take on show has, source included.
    pub(super) fn lanes(&self) -> usize {
        self.take.as_ref().map_or(0, |take| take.passes.len())
    }

    /// The pass on show, as an index into the take's passes.
    pub(super) fn shown(&self) -> usize {
        let lanes = self.lanes();
        match self.pass {
            Some(pass) if pass < lanes => pass,
            _ => lanes.saturating_sub(1),
        }
    }

    pub(super) fn step_row(&mut self, down: bool) -> bool {
        let next = if down {
            (self.row + 1).min(ROWS.len() - 1)
        } else {
            self.row.saturating_sub(1)
        };
        if next == self.row {
            return false;
        }
        self.row = next;
        true
    }

    /// The first row of the next group, round.
    pub(super) fn next_group(&mut self) -> bool {
        let here = ROWS.get(self.row).map(|(group, _)| *group);
        let mut index = self.row;
        for _ in 0..ROWS.len() {
            index = (index + 1) % ROWS.len();
            if ROWS.get(index).map(|(group, _)| *group) != here {
                let group = ROWS.get(index).map(|(group, _)| *group);
                // Walk back to the group's first row, in case the walk
                // landed mid-group after wrapping.
                while index > 0 && ROWS.get(index - 1).map(|(g, _)| *g) == group {
                    index -= 1;
                }
                self.row = index;
                return true;
            }
        }
        false
    }

    pub(super) fn step_pass(&mut self, next: bool) -> bool {
        let lanes = self.lanes();
        if lanes == 0 {
            return false;
        }
        let shown = self.shown();
        let to = if next {
            if shown + 1 >= lanes {
                return false;
            }
            shown + 1
        } else {
            if shown == 0 {
                return false;
            }
            shown - 1
        };
        self.pass = Some(to);
        true
    }

    pub(super) fn pick_pass(&mut self, pass: usize) -> bool {
        if pass >= self.lanes() {
            return false;
        }
        self.pass = Some(pass);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::DeviceKind;

    fn device() -> Device {
        let mut device = Device::new(DeviceId(7), DeviceKind::Scomp);
        device.set(sp::TAKE, 0.25);
        device
    }

    #[test]
    fn every_row_is_a_table_id_once_and_groups_are_contiguous() {
        let mut ids: Vec<u32> = ROWS.iter().map(|(_, id)| *id).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = sp::TABLE.iter().map(|def| def.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect);
        let mut seen: Vec<&str> = Vec::new();
        for (group, _) in ROWS {
            if seen.last() != Some(&group) {
                assert!(!seen.contains(&group), "{group} appears twice");
                seen.push(group);
            }
        }
    }

    #[test]
    fn the_take_is_rendered_once_per_change_of_the_baked_knobs() {
        let mut device = device();
        let mut forge = Forge::open(0, device.id);
        assert!(forge.refresh(&device), "nothing rendered on open");
        assert_eq!(forge.lanes(), 4, "the source and three passes");
        assert_eq!(forge.peaks.len(), 4);
        assert!(!forge.refresh(&device), "rendered again for nothing");
        device.set(sp::LEVEL, 1.0);
        assert!(!forge.refresh(&device), "a letter re-rendered the take");
        device.set(sp::PASSES, 1.0);
        assert!(forge.refresh(&device));
        assert_eq!(forge.lanes(), 2);
        assert_eq!(forge.shown(), 1, "the last pass is on show by default");
    }

    #[test]
    fn rows_groups_and_passes_walk_and_refuse_at_their_ends() {
        let device = device();
        let mut forge = Forge::open(0, device.id);
        forge.refresh(&device);
        assert!(!forge.step_row(false), "stepped above the first row");
        assert!(forge.step_row(true));
        assert_eq!(forge.param(), sp::TAKE);
        assert!(forge.next_group());
        assert_eq!(forge.param(), sp::FILTER);
        forge.row = ROWS.len() - 1;
        assert!(!forge.step_row(true));
        assert!(forge.next_group(), "the last group does not wrap");
        assert_eq!(forge.row, 0);
        assert_eq!(forge.shown(), 3);
        assert!(!forge.step_pass(true), "stepped past the last pass");
        assert!(forge.step_pass(false));
        assert_eq!(forge.shown(), 2);
        assert!(forge.pick_pass(0));
        assert_eq!(forge.shown(), 0);
        assert!(!forge.pick_pass(9));
        assert!(!forge.step_pass(false));
    }
}
