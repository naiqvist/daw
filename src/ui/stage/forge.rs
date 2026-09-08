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

use crate::audio::quad::QuadParams;
use crate::params::quad as qp;
use crate::params::scomp as sp;
use crate::sample_peaks::Peaks;
use crate::scomp::{Baked, ScompParams, Take};
use crate::sequencing::{Device, DeviceId};

/// The rate the picture is rendered at. The engine renders at its own;
/// the take is the same shape at any.
pub(super) const RENDER_RATE: u32 = 48_000;
/// The band's cards render at half that: a card is a thumbnail, and the
/// shape survives.
pub(super) const CARD_RATE: u32 = 24_000;

/// What an sCOMP card in the band draws: the peaks of every pass of the
/// take its knobs bake, and how long each pass is. Kept on the stage,
/// keyed on the baked knobs, so the band renders a take once per change
/// rather than once per frame.
#[derive(Clone, Debug)]
pub struct ScompCard {
    pub(super) key: Baked,
    pub(super) lens: Vec<usize>,
    pub(super) peaks: Vec<Arc<Peaks>>,
}

impl ScompCard {
    pub(super) fn render(params: &ScompParams) -> Self {
        let take = crate::scomp::render(params, CARD_RATE);
        Self {
            key: params.baked(),
            lens: take.passes.iter().map(|pass| pass.len()).collect(),
            peaks: take
                .passes
                .iter()
                .map(|pass| Arc::new(Peaks::build(pass, 1, pass.len() as u64, CARD_RATE)))
                .collect(),
        }
    }
}

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

/// QUAD's rows, in signal order: each operator, the routing, the two
/// pitch envelopes, the filter, the output.
pub(super) const QUAD_ROWS: [(&str, u32); 128] = [
    ("OP 1", qp::op_param(0, qp::RATIO)),
    ("OP 1", qp::op_param(0, qp::FINE)),
    ("OP 1", qp::op_param(0, qp::LEVEL_OP)),
    ("OP 1", qp::op_param(0, qp::WAVE)),
    ("OP 1", qp::op_param(0, qp::FIXED)),
    ("OP 1", qp::op_param(0, qp::HZ)),
    ("OP 1", qp::op_param(0, qp::VEL)),
    ("OP 1", qp::op_param(0, qp::KEYSCALE)),
    ("OP 1", qp::op_param(0, qp::DELAY)),
    ("OP 1", qp::op_param(0, qp::ATTACK)),
    ("OP 1", qp::op_param(0, qp::DECAY)),
    ("OP 1", qp::op_param(0, qp::BREAK)),
    ("OP 1", qp::op_param(0, qp::DECAY2)),
    ("OP 1", qp::op_param(0, qp::SUSTAIN)),
    ("OP 1", qp::op_param(0, qp::RELEASE)),
    ("OP 1", qp::op_param(0, qp::CURVE)),
    ("OP 2", qp::op_param(1, qp::RATIO)),
    ("OP 2", qp::op_param(1, qp::FINE)),
    ("OP 2", qp::op_param(1, qp::LEVEL_OP)),
    ("OP 2", qp::op_param(1, qp::WAVE)),
    ("OP 2", qp::op_param(1, qp::FIXED)),
    ("OP 2", qp::op_param(1, qp::HZ)),
    ("OP 2", qp::op_param(1, qp::VEL)),
    ("OP 2", qp::op_param(1, qp::KEYSCALE)),
    ("OP 2", qp::op_param(1, qp::DELAY)),
    ("OP 2", qp::op_param(1, qp::ATTACK)),
    ("OP 2", qp::op_param(1, qp::DECAY)),
    ("OP 2", qp::op_param(1, qp::BREAK)),
    ("OP 2", qp::op_param(1, qp::DECAY2)),
    ("OP 2", qp::op_param(1, qp::SUSTAIN)),
    ("OP 2", qp::op_param(1, qp::RELEASE)),
    ("OP 2", qp::op_param(1, qp::CURVE)),
    ("OP 3", qp::op_param(2, qp::RATIO)),
    ("OP 3", qp::op_param(2, qp::FINE)),
    ("OP 3", qp::op_param(2, qp::LEVEL_OP)),
    ("OP 3", qp::op_param(2, qp::WAVE)),
    ("OP 3", qp::op_param(2, qp::FIXED)),
    ("OP 3", qp::op_param(2, qp::HZ)),
    ("OP 3", qp::op_param(2, qp::VEL)),
    ("OP 3", qp::op_param(2, qp::KEYSCALE)),
    ("OP 3", qp::op_param(2, qp::DELAY)),
    ("OP 3", qp::op_param(2, qp::ATTACK)),
    ("OP 3", qp::op_param(2, qp::DECAY)),
    ("OP 3", qp::op_param(2, qp::BREAK)),
    ("OP 3", qp::op_param(2, qp::DECAY2)),
    ("OP 3", qp::op_param(2, qp::SUSTAIN)),
    ("OP 3", qp::op_param(2, qp::RELEASE)),
    ("OP 3", qp::op_param(2, qp::CURVE)),
    ("OP 4", qp::op_param(3, qp::RATIO)),
    ("OP 4", qp::op_param(3, qp::FINE)),
    ("OP 4", qp::op_param(3, qp::LEVEL_OP)),
    ("OP 4", qp::op_param(3, qp::WAVE)),
    ("OP 4", qp::op_param(3, qp::FIXED)),
    ("OP 4", qp::op_param(3, qp::HZ)),
    ("OP 4", qp::op_param(3, qp::VEL)),
    ("OP 4", qp::op_param(3, qp::KEYSCALE)),
    ("OP 4", qp::op_param(3, qp::DELAY)),
    ("OP 4", qp::op_param(3, qp::ATTACK)),
    ("OP 4", qp::op_param(3, qp::DECAY)),
    ("OP 4", qp::op_param(3, qp::BREAK)),
    ("OP 4", qp::op_param(3, qp::DECAY2)),
    ("OP 4", qp::op_param(3, qp::SUSTAIN)),
    ("OP 4", qp::op_param(3, qp::RELEASE)),
    ("OP 4", qp::op_param(3, qp::CURVE)),
    ("ROUTE", qp::ALGO),
    ("ROUTE", qp::FEEDBACK),
    ("ROUTE", qp::FB_OP),
    ("MATRIX", qp::matrix_param(0, 0)),
    ("MATRIX", qp::matrix_param(0, 1)),
    ("MATRIX", qp::matrix_param(0, 2)),
    ("MATRIX", qp::matrix_param(0, 3)),
    ("MATRIX", qp::matrix_param(1, 0)),
    ("MATRIX", qp::matrix_param(1, 1)),
    ("MATRIX", qp::matrix_param(1, 2)),
    ("MATRIX", qp::matrix_param(1, 3)),
    ("MATRIX", qp::matrix_param(2, 0)),
    ("MATRIX", qp::matrix_param(2, 1)),
    ("MATRIX", qp::matrix_param(2, 2)),
    ("MATRIX", qp::matrix_param(2, 3)),
    ("MATRIX", qp::matrix_param(3, 0)),
    ("MATRIX", qp::matrix_param(3, 1)),
    ("MATRIX", qp::matrix_param(3, 2)),
    ("MATRIX", qp::matrix_param(3, 3)),
    ("MATRIX", qp::out_param(0)),
    ("MATRIX", qp::out_param(1)),
    ("MATRIX", qp::out_param(2)),
    ("MATRIX", qp::out_param(3)),
    ("VOICE", qp::UNISON),
    ("VOICE", qp::UDETUNE),
    ("VOICE", qp::WIDTH),
    ("VOICE", qp::MONO),
    ("VOICE", qp::GLIDE),
    ("VOICE", qp::ENV_LOOP),
    ("PITCH 1", qp::PITCH1),
    ("PITCH 1", qp::PITCH1_RISE),
    ("PITCH 1", qp::PITCH1_FALL),
    ("PITCH 2", qp::PITCH2),
    ("PITCH 2", qp::PITCH2_RISE),
    ("PITCH 2", qp::PITCH2_FALL),
    ("FILTER", qp::FMODE),
    ("FILTER", qp::CUTOFF),
    ("FILTER", qp::RESO),
    ("FILTER", qp::FENV),
    ("FILTER", qp::FENV_ATT),
    ("FILTER", qp::FENV_DEC),
    ("FILTER", qp::KEYTRACK),
    ("LFO 1", qp::LFO1_RATE),
    ("LFO 1", qp::LFO1_SHAPE),
    ("LFO 1", qp::LFO1_DELAY),
    ("LFO 1", qp::LFO1_FADE),
    ("LFO 1", qp::LFO1_PITCH),
    ("LFO 1", qp::LFO1_MOD),
    ("LFO 1", qp::LFO1_AMP),
    ("LFO 1", qp::LFO1_FILTER),
    ("LFO 2", qp::LFO2_RATE),
    ("LFO 2", qp::LFO2_SHAPE),
    ("LFO 2", qp::LFO2_DELAY),
    ("LFO 2", qp::LFO2_FADE),
    ("LFO 2", qp::LFO2_PITCH),
    ("LFO 2", qp::LFO2_MOD),
    ("LFO 2", qp::LFO2_AMP),
    ("LFO 2", qp::LFO2_FILTER),
    ("OUT", qp::DIST),
    ("OUT", qp::DRIVE),
    ("OUT", qp::VELOCITY),
    ("OUT", qp::LEVEL),
    ("OUT", qp::KEY_RATE),
    ("OUT", qp::OVERSAMPLE),
];

/// Which instrument the room is open on. The room's grammar is one
/// thing; what it draws, and what the digits pick, is the subject's.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Subject {
    Scomp,
    Quad,
}

/// The room, while it is up.
#[derive(Clone, Debug)]
pub(super) struct Forge {
    pub(super) subject: Subject,
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
    /// The other side of an A/B: every row's value, held.
    pub(super) snapshot: Option<Vec<(u32, f32)>>,
    /// The dice for mutate and randomise: an xorshift, so a test can
    /// seed it and the room owes nothing to the clock.
    pub(super) seed: u32,
}

impl Forge {
    pub(super) fn open(track: usize, device: DeviceId, subject: Subject) -> Self {
        Self {
            subject,
            track,
            device,
            row: 0,
            pass: None,
            key: None,
            take: None,
            peaks: Vec::new(),
            snapshot: None,
            seed: 0x2545_F491,
        }
    }

    /// A number in `0..1`, and the next seed.
    pub(super) fn roll(&mut self) -> f32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    /// The rows the dice may touch: everything but the loudness, the
    /// doors and the switches that change what the room is.
    pub(super) fn rollable(&self, id: u32) -> bool {
        match self.subject {
            Subject::Scomp => !matches!(id, sp::LEVEL | sp::OPEN | sp::ROOT),
            Subject::Quad => !matches!(id, qp::LEVEL | qp::OVERSAMPLE | qp::MONO | qp::UNISON),
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

    /// QUAD's knobs as the device holds them.
    pub(super) fn quad_params_of(device: &Device) -> QuadParams {
        let mut params = QuadParams::default();
        for (id, value) in &device.overrides {
            params.set(*id, *value);
        }
        params
    }

    /// The rows the room shows: the subject's.
    pub(super) fn rows(&self) -> &'static [(&'static str, u32)] {
        match self.subject {
            Subject::Scomp => &ROWS,
            Subject::Quad => &QUAD_ROWS,
        }
    }

    /// Render again if the baked knobs moved. Returns whether it did.
    /// Only sCOMP has a take; QUAD's picture is its knobs.
    pub(super) fn refresh(&mut self, device: &Device) -> bool {
        if self.subject != Subject::Scomp {
            return false;
        }
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
        self.rows().get(self.row).map_or(0, |(_, id)| *id)
    }

    /// How many things the digits can put on show: sCOMP's passes,
    /// source included; QUAD's operators.
    pub(super) fn lanes(&self) -> usize {
        match self.subject {
            Subject::Scomp => self.take.as_ref().map_or(0, |take| take.passes.len()),
            Subject::Quad => qp::OPS,
        }
    }

    /// The pass — or operator — on show. sCOMP shows its last pass
    /// until told otherwise; QUAD its first operator.
    pub(super) fn shown(&self) -> usize {
        let lanes = self.lanes();
        match (self.subject, self.pass) {
            (_, Some(pass)) if pass < lanes => pass,
            (Subject::Scomp, _) => lanes.saturating_sub(1),
            (Subject::Quad, _) => 0,
        }
    }

    pub(super) fn step_row(&mut self, down: bool) -> bool {
        let next = if down {
            (self.row + 1).min(self.rows().len() - 1)
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
        let rows = self.rows();
        let here = rows.get(self.row).map(|(group, _)| *group);
        let mut index = self.row;
        for _ in 0..rows.len() {
            index = (index + 1) % rows.len();
            if rows.get(index).map(|(group, _)| *group) != here {
                let group = rows.get(index).map(|(group, _)| *group);
                // Walk back to the group's first row, in case the walk
                // landed mid-group after wrapping.
                while index > 0 && rows.get(index - 1).map(|(g, _)| *g) == group {
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
        // Every knob but the door, which is the band's, not the forge's.
        let mut expect: Vec<u32> = sp::TABLE
            .iter()
            .map(|def| def.id)
            .filter(|id| *id != sp::OPEN)
            .collect();
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
    fn the_dice_are_seeded_and_spare_the_rows_that_change_the_room() {
        let device = Device::new(DeviceId(9), DeviceKind::Quad);
        let mut a = Forge::open(0, device.id, Subject::Quad);
        let mut b = Forge::open(0, device.id, Subject::Quad);
        let rolls: Vec<f32> = (0..8).map(|_| a.roll()).collect();
        assert_eq!(rolls, (0..8).map(|_| b.roll()).collect::<Vec<_>>());
        assert!(rolls.iter().all(|r| (0.0..1.0).contains(r)));
        assert!(rolls.windows(2).any(|w| w[0] != w[1]));
        assert!(a.rollable(qp::CUTOFF) && !a.rollable(qp::LEVEL) && !a.rollable(qp::MONO));
        let c = Forge::open(0, DeviceId(1), Subject::Scomp);
        assert!(c.rollable(sp::DRIVE) && !c.rollable(sp::OPEN) && !c.rollable(sp::LEVEL));
    }

    #[test]
    fn a_quad_room_has_quads_rows_and_its_operators_on_show() {
        let device = Device::new(DeviceId(9), DeviceKind::Quad);
        let mut forge = Forge::open(0, device.id, Subject::Quad);
        assert!(!forge.refresh(&device), "QUAD renders no take");
        assert_eq!(forge.rows().len(), qp::TABLE.len());
        let mut ids: Vec<u32> = forge.rows().iter().map(|(_, id)| *id).collect();
        ids.sort_unstable();
        assert_eq!(ids, (0..qp::TABLE.len() as u32).collect::<Vec<_>>());
        assert_eq!(forge.param(), qp::op_param(0, qp::RATIO));
        assert_eq!((forge.lanes(), forge.shown()), (4, 0));
        assert!(forge.pick_pass(3) && forge.shown() == 3);
        assert!(!forge.pick_pass(4));
        assert!(forge.next_group());
        assert_eq!(forge.param(), qp::op_param(1, qp::RATIO));
    }

    #[test]
    fn the_take_is_rendered_once_per_change_of_the_baked_knobs() {
        let mut device = device();
        let mut forge = Forge::open(0, device.id, Subject::Scomp);
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
        let mut forge = Forge::open(0, device.id, Subject::Scomp);
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
