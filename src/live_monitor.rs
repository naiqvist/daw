//! The green-zone half of live monitoring: who is being played, and what
//! is still held.
//!
//! [`crate::audio::graph::LiveNote`] is the letter; this decides which
//! letters to write. It exists as its own module because the interesting
//! part of monitoring is not the ring — it is the bookkeeping that keeps a
//! note from hanging forever, and that is worth testing without an audio
//! device in the room.
//!
//! The whole of the design, including why a live note is not sequence data
//! under contract rule 4, is `notes/20260901-live-monitoring-decision.md`.

use crate::audio::graph::{LiveKind, LiveNote, NodeId};

/// How many pitches MIDI can name.
const PITCHES: usize = 128;

/// Which instrument is under the hands, and what it is holding.
///
/// ONE target at a time, deliberately. Monitoring more than one instrument
/// from one keyboard is a layering feature, and a layering feature invented
/// accidentally — by letting this hold a set instead of an option — is one
/// nobody designed and nobody can turn off.
#[derive(Debug)]
pub struct LiveMonitor {
    target: Option<NodeId>,
    /// Held pitches on `target`, and ONLY on `target`. Retargeting releases
    /// before it switches, so this can never describe two nodes at once —
    /// which is the bug it exists to prevent: an off owed to a node the
    /// performer already navigated away from.
    held: [bool; PITCHES],
    count: usize,
}

// Hand-written because `[bool; 128]` is past the arity `Default` derives
// for. The array is the point — a fixed 128 is bounded, `Copy`, and needs
// no allocation to answer "is this pitch held".
impl Default for LiveMonitor {
    fn default() -> Self {
        Self {
            target: None,
            held: [false; PITCHES],
            count: 0,
        }
    }
}

impl LiveMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn target(&self) -> Option<NodeId> {
        self.target
    }

    pub fn held_count(&self) -> usize {
        self.count
    }

    pub fn is_held(&self, pitch: u8) -> bool {
        usize::from(pitch) < PITCHES && self.held[usize::from(pitch)]
    }

    /// Point the keyboard at a different instrument — or at none.
    ///
    /// Returns the letter that must be sent FIRST, before anything is
    /// played on the new target: everything held belongs to the OLD node,
    /// and no key-up will ever arrive for it there, because the release
    /// will be addressed to the new one. Without this, every track change
    /// mid-chord leaves a chord sounding forever.
    ///
    /// Retargeting to the node already selected is not a change and
    /// releases nothing — a redundant all-off would cut a note the
    /// performer is still holding.
    #[must_use = "the old target's notes hang forever if this letter is not sent"]
    pub fn retarget(&mut self, node: Option<NodeId>) -> Option<LiveNote> {
        if node == self.target {
            return None;
        }
        let release = self.release_all();
        self.target = node;
        release
    }

    /// A key went down. `None` when there is nothing to play it on, which
    /// is a real state and not an error: no track is selected, or the
    /// selected one has no instrument.
    ///
    /// A repeated on for a pitch already held is passed through rather than
    /// swallowed. Controllers do send them, and the voice bank's own
    /// stealing rule is the right authority on what a re-strike means —
    /// this module does not get to decide that an instrument cannot be
    /// re-triggered.
    #[must_use]
    pub fn note_on(&mut self, pitch: u8, vel: u8) -> Option<LiveNote> {
        let node = self.target?;
        let index = usize::from(pitch);
        if index >= PITCHES {
            return None;
        }
        if !self.held[index] {
            self.held[index] = true;
            self.count += 1;
        }
        Some(LiveNote {
            node: node.to_bits(),
            kind: LiveKind::On,
            pitch,
            vel,
        })
    }

    /// A key came up. `None` for a pitch that is not held — a release with
    /// no press is not an event, and forwarding it would ask the voice bank
    /// to release a note some OTHER gesture is holding.
    #[must_use]
    pub fn note_off(&mut self, pitch: u8) -> Option<LiveNote> {
        let node = self.target?;
        let index = usize::from(pitch);
        if index >= PITCHES || !self.held[index] {
            return None;
        }
        self.held[index] = false;
        self.count -= 1;
        Some(LiveNote {
            node: node.to_bits(),
            kind: LiveKind::Off,
            pitch,
            vel: 0,
        })
    }

    /// Cut everything held on the current target.
    ///
    /// This is the recovery path, and the caller MUST reach for it when
    /// [`crate::audio::Engine::live_note`] reports a letter undelivered. A
    /// dropped note-on merely did not sound; a dropped note-OFF is a note
    /// that sounds forever, and the only cure that does not depend on
    /// another letter of the same kind arriving is to cut the lot.
    ///
    /// Also the right answer on transport stop, on losing the controller,
    /// and anywhere else a key-up can no longer be relied upon.
    #[must_use]
    pub fn release_all(&mut self) -> Option<LiveNote> {
        let node = self.target?;
        if self.count == 0 {
            return None;
        }
        self.held = [false; PITCHES];
        self.count = 0;
        Some(LiveNote {
            node: node.to_bits(),
            kind: LiveKind::AllOff,
            pitch: 0,
            vel: 0,
        })
    }

    /// Forget what is held WITHOUT emitting anything.
    ///
    /// For the one case where the notes are already gone and a letter would
    /// be addressed at nothing: the graph cut them itself. A discontinuity
    /// runs all-sound-off inside the callback over the same voice bank, so
    /// after a seek this module's idea of what is held is stale and an
    /// all-off letter would be noise.
    pub fn forget(&mut self) {
        self.held = [false; PITCHES];
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two distinct node tags. Generation is the high 32 bits and is never
    /// zero, matching what thunderdome mints.
    fn node(slot: u32) -> NodeId {
        NodeId::from_bits((1u64 << 32) | u64::from(slot)).expect("generation 1 is a real tag")
    }

    fn monitor_on(node_slot: u32) -> LiveMonitor {
        let mut m = LiveMonitor::new();
        assert!(
            m.retarget(Some(node(node_slot))).is_none(),
            "the first target releases nothing"
        );
        m
    }

    #[test]
    fn with_no_target_nothing_is_played_and_nothing_panics() {
        let mut m = LiveMonitor::new();
        assert!(m.note_on(60, 100).is_none(), "no instrument, no letter");
        assert!(m.note_off(60).is_none());
        assert!(m.release_all().is_none());
        assert_eq!(m.held_count(), 0);
    }

    #[test]
    fn a_press_and_a_release_are_one_letter_each() {
        let mut m = monitor_on(7);
        let on = m.note_on(60, 100).expect("a target means a letter");
        assert_eq!(on.kind, LiveKind::On);
        assert_eq!(on.pitch, 60);
        assert_eq!(on.vel, 100);
        assert!(m.is_held(60));

        let off = m.note_off(60).expect("a held note releases");
        assert_eq!(off.kind, LiveKind::Off);
        assert_eq!(off.pitch, 60);
        assert!(!m.is_held(60));
        assert_eq!(m.held_count(), 0);
    }

    /// A release with no press must not travel. Forwarded, it would ask the
    /// voice bank to release a note some other gesture is holding.
    #[test]
    fn releasing_a_pitch_that_was_never_pressed_says_nothing() {
        let mut m = monitor_on(7);
        assert!(m.note_off(60).is_none());
        let _ = m.note_on(60, 100);
        assert!(
            m.note_off(64).is_none(),
            "a different pitch is still not held"
        );
        assert!(m.is_held(60), "and the held one is untouched");
    }

    /// THE bug this module exists to prevent. Change instrument with a
    /// chord down, and every one of those notes is owed a release the new
    /// target will never be asked for.
    #[test]
    fn retargeting_releases_the_old_node_before_switching() {
        let mut m = monitor_on(7);
        let _ = m.note_on(60, 100);
        let _ = m.note_on(64, 100);
        let _ = m.note_on(67, 100);
        assert_eq!(m.held_count(), 3);

        let release = m
            .retarget(Some(node(9)))
            .expect("the old chord must be cut");
        assert_eq!(release.kind, LiveKind::AllOff);
        assert_eq!(
            release.node,
            node(7).to_bits(),
            "the release is owed to the node that was PLAYING it, not the new one"
        );
        assert_eq!(m.held_count(), 0);
        assert_eq!(m.target(), Some(node(9)));
    }

    /// Retargeting to where we already are is not a change. Emitting an
    /// all-off would cut a note the performer is still holding down.
    #[test]
    fn retargeting_to_the_same_node_cuts_nothing() {
        let mut m = monitor_on(7);
        let _ = m.note_on(60, 100);
        assert!(m.retarget(Some(node(7))).is_none());
        assert!(m.is_held(60), "the held note survives a no-op retarget");
    }

    #[test]
    fn retargeting_to_nothing_still_releases() {
        let mut m = monitor_on(7);
        let _ = m.note_on(60, 100);
        let release = m.retarget(None).expect("notes were held");
        assert_eq!(release.kind, LiveKind::AllOff);
        assert_eq!(release.node, node(7).to_bits());
        assert!(m.target().is_none());
        assert!(m.note_on(60, 100).is_none(), "and nothing plays after");
    }

    #[test]
    fn releasing_all_with_nothing_held_says_nothing() {
        let mut m = monitor_on(7);
        assert!(
            m.release_all().is_none(),
            "an all-off letter with nothing to cut is noise"
        );
    }

    /// A repeated on for a held pitch travels. The voice bank's stealing
    /// rule owns what a re-strike means; this module does not get to decide
    /// an instrument cannot be re-triggered.
    #[test]
    fn a_repeated_press_is_passed_through_and_counted_once() {
        let mut m = monitor_on(7);
        let _ = m.note_on(60, 100);
        assert!(m.note_on(60, 120).is_some(), "the re-strike must travel");
        assert_eq!(m.held_count(), 1, "but it is still one held pitch");
        let _ = m.note_off(60);
        assert_eq!(m.held_count(), 0, "and one release clears it");
    }

    /// `forget` is for the case where the graph already cut the notes —
    /// a discontinuity runs all-sound-off over the same voice bank, so an
    /// all-off letter afterwards would be addressed at nothing.
    #[test]
    fn forgetting_clears_without_emitting() {
        let mut m = monitor_on(7);
        let _ = m.note_on(60, 100);
        m.forget();
        assert_eq!(m.held_count(), 0);
        assert!(
            m.release_all().is_none(),
            "nothing is held, so nothing is owed"
        );
        assert_eq!(m.target(), Some(node(7)), "but the target is unchanged");
    }

    /// Every pitch MIDI can name is addressable, and the array bound is
    /// never the thing that panics.
    #[test]
    fn the_full_pitch_range_is_held_and_released() {
        let mut m = monitor_on(7);
        for pitch in 0..=127u8 {
            assert!(m.note_on(pitch, 64).is_some(), "pitch {pitch} must play");
        }
        assert_eq!(m.held_count(), 128);
        let release = m.release_all().expect("128 notes are held");
        assert_eq!(release.kind, LiveKind::AllOff);
        assert_eq!(m.held_count(), 0);
    }
}
