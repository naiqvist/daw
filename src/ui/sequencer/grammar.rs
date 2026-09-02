//! The sentence machinery: `[count] VERB [motion]`.
//!
//! Every keyboard action in the redesign is a sentence assembled here.
//! Digits accumulate a count, a verb key either acts on the spot or waits
//! for its motion, an arrow supplies the motion (or moves the cursor bare),
//! and Escape abandons the sentence. The panel that owns focus pulls at
//! most one finished `Utterance` per frame and interprets it against its
//! own nouns — the grammar knows words, never meanings.
//!
//! The sentence-in-progress is state the user is holding in their head;
//! `display` exists so a status line can hold it on screen instead
//! (contract: sentence visibility, `notes/20260831-command-grammar.md`).

use crate::ui::sequencer::registers::Registers;
use crate::ui::sequencer::verbs::{COMMAND_TABLE, SHIFT_TABLE, TABLE, Verb};
use eframe::egui;

/// Everything a panel needs to speak: the frame's one sentence and its
/// registers, borrowed together so a panel signature stays one parameter
/// as the grammar grows.
pub(crate) struct Voice<'a> {
    pub(crate) sentence: &'a mut Sentence,
    pub(crate) registers: &'a mut Registers,
}

/// Counts above this are typos, not intents.
const MAX_COUNT: usize = 999;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Motion {
    Left,
    Right,
    Up,
    Down,
}

const ARROWS: [(egui::Key, Motion); 4] = [
    (egui::Key::ArrowLeft, Motion::Left),
    (egui::Key::ArrowRight, Motion::Right),
    (egui::Key::ArrowUp, Motion::Up),
    (egui::Key::ArrowDown, Motion::Down),
];

const DIGITS: [egui::Key; 10] = [
    egui::Key::Num0,
    egui::Key::Num1,
    egui::Key::Num2,
    egui::Key::Num3,
    egui::Key::Num4,
    egui::Key::Num5,
    egui::Key::Num6,
    egui::Key::Num7,
    egui::Key::Num8,
    egui::Key::Num9,
];

/// One finished sentence. `verb` is `None` for a bare motion (cursor
/// travel); `motion` is `None` for an on-the-spot verb. Never both.
///
/// `held` is the hold-as-preposition qualifier on a MOTION: the same arrow
/// spoken while holding the trig qualifier addresses the trig's own values
/// instead of travelling. Shift on Q/W/E is different: it names the
/// explicit stack yank/nudge/put verbs before the motion arrives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Utterance {
    pub(crate) count: usize,
    pub(crate) verb: Option<Verb>,
    pub(crate) motion: Option<Motion>,
    pub(crate) held: bool,
}

/// The sentence-in-progress. Owned by the frame's keyboard layer so a
/// count started before a focus change does not silently vanish.
#[derive(Default)]
pub(crate) struct Sentence {
    count: usize,
    pending: Option<Verb>,
}

impl Sentence {
    /// Drain this frame's keys into at most one finished utterance.
    /// Call only from the focused panel — the grammar consumes keys.
    pub(crate) fn consume(&mut self, ctx: &egui::Context) -> Option<Utterance> {
        for (digit, key) in DIGITS.iter().enumerate() {
            while ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, *key)) {
                self.feed_digit(digit);
            }
        }
        if !self.is_empty()
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.abandon();
            return None;
        }
        for (verb, key, _) in COMMAND_TABLE {
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, *key))
                && let Some(utterance) = self.feed_verb(*verb)
            {
                return Some(utterance);
            }
        }
        // Most-specific first. egui matches a no-modifier chord under
        // Shift, so the shifted stack words must get the first chance.
        for (verb, key, _) in SHIFT_TABLE {
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::SHIFT, *key))
                && let Some(utterance) = self.feed_verb(*verb)
            {
                return Some(utterance);
            }
        }
        for (verb, key, _) in TABLE {
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, *key))
                && let Some(utterance) = self.feed_verb(*verb)
            {
                return Some(utterance);
            }
        }
        for (key, motion) in ARROWS {
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::SHIFT, key)) {
                return Some(self.feed_motion(motion, true));
            }
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key)) {
                return Some(self.feed_motion(motion, false));
            }
        }
        None
    }

    /// The sentence so far, for the status line. Empty when at rest.
    pub(crate) fn display(&self) -> String {
        match (self.count, self.pending) {
            (0, None) => String::new(),
            (0, Some(verb)) => format!("{} …", verb.name()),
            (count, None) => format!("{count} …"),
            (count, Some(verb)) => format!("{count} {} …", verb.name()),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.count == 0 && self.pending.is_none()
    }

    fn feed_digit(&mut self, digit: usize) {
        self.count = (self.count * 10 + digit).min(MAX_COUNT);
    }

    fn feed_verb(&mut self, verb: Verb) -> Option<Utterance> {
        if verb.needs_motion() {
            // A second motion verb replaces the first: the newest
            // intention wins, the old one was never spoken.
            self.pending = Some(verb);
            return None;
        }
        self.pending = None;
        Some(Utterance {
            count: self.take_count(),
            verb: Some(verb),
            motion: None,
            held: false,
        })
    }

    fn feed_motion(&mut self, motion: Motion, held: bool) -> Utterance {
        Utterance {
            count: self.take_count(),
            verb: self.pending.take(),
            motion: Some(motion),
            held,
        }
    }

    fn abandon(&mut self) {
        self.count = 0;
        self.pending = None;
    }

    /// No count spoken means once.
    fn take_count(&mut self) -> usize {
        let count = self.count.max(1);
        self.count = 0;
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_accumulate_into_one_count() {
        let mut sentence = Sentence::default();
        sentence.feed_digit(4);
        sentence.feed_digit(2);
        let utterance = sentence.feed_motion(Motion::Right, false);
        assert_eq!(utterance.count, 42);
        assert_eq!(utterance.verb, None);
        assert!(sentence.is_empty());
    }

    #[test]
    fn a_motion_verb_waits_for_its_motion() {
        let mut sentence = Sentence::default();
        assert_eq!(sentence.feed_verb(Verb::Nudge), None);
        let utterance = sentence.feed_motion(Motion::Left, false);
        assert_eq!(utterance.verb, Some(Verb::Nudge));
        assert_eq!(utterance.motion, Some(Motion::Left));
        assert_eq!(utterance.count, 1);
    }

    #[test]
    fn stack_nudge_is_its_own_motion_verb() {
        let mut sentence = Sentence::default();
        assert_eq!(sentence.feed_verb(Verb::StackNudge), None);
        let utterance = sentence.feed_motion(Motion::Right, false);
        assert_eq!(utterance.verb, Some(Verb::StackNudge));
        assert_eq!(utterance.motion, Some(Motion::Right));
    }

    #[test]
    fn clip_resize_is_a_motion_verb() {
        let mut sentence = Sentence::default();
        assert_eq!(sentence.feed_verb(Verb::ClipResize), None);
        let utterance = sentence.feed_motion(Motion::Left, false);
        assert_eq!(utterance.verb, Some(Verb::ClipResize));
        assert_eq!(utterance.motion, Some(Motion::Left));
    }

    #[test]
    fn shift_w_is_consumed_as_stack_nudge_not_plain_nudge() {
        let ctx = egui::Context::default();
        let mut sentence = Sentence::default();
        let key = |key, modifiers| egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            ..Default::default()
        };
        let mut first = None;
        let mut run = ctx.run_ui(key(egui::Key::W, egui::Modifiers::SHIFT), |ui| {
            first = sentence.consume(ui.ctx());
        });
        run.textures_delta.clear();
        assert_eq!(first, None, "a motion verb acted before its arrow");
        assert_eq!(sentence.display(), "STACK NUDGE …");

        let mut second = None;
        let mut run = ctx.run_ui(key(egui::Key::ArrowRight, egui::Modifiers::NONE), |ui| {
            second = sentence.consume(ui.ctx());
        });
        run.textures_delta.clear();
        assert_eq!(
            second.map(|utterance| utterance.verb),
            Some(Some(Verb::StackNudge))
        );
    }

    #[test]
    fn an_immediate_verb_takes_the_count_with_it() {
        let mut sentence = Sentence::default();
        sentence.feed_digit(3);
        let utterance = sentence.feed_verb(Verb::Act).expect("acts on the spot");
        assert_eq!(utterance.count, 3);
        assert_eq!(utterance.motion, None);
        assert!(sentence.is_empty());
    }

    #[test]
    fn arm_and_monitor_are_immediate_global_verbs() {
        let mut sentence = Sentence::default();
        let arm = sentence.feed_verb(Verb::Arm).expect("arm acts on the spot");
        let monitor = sentence
            .feed_verb(Verb::Monitor)
            .expect("monitor acts on the spot");
        assert_eq!(arm.verb, Some(Verb::Arm));
        assert_eq!(monitor.verb, Some(Verb::Monitor));
        assert_eq!(arm.motion, None);
        assert_eq!(monitor.motion, None);
    }

    #[test]
    fn command_a_speaks_the_reusable_select_all_verb() {
        let ctx = egui::Context::default();
        let mut sentence = Sentence::default();
        let mut utterance = None;
        let mut run = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                }],
                ..Default::default()
            },
            |ui| utterance = sentence.consume(ui.ctx()),
        );
        run.textures_delta.clear();
        assert_eq!(
            utterance.and_then(|words| words.verb),
            Some(Verb::SelectAll)
        );
    }

    #[test]
    fn a_new_verb_replaces_a_pending_one() {
        let mut sentence = Sentence::default();
        assert_eq!(sentence.feed_verb(Verb::Nudge), None);
        assert_eq!(sentence.feed_verb(Verb::Resize), None);
        let utterance = sentence.feed_motion(Motion::Right, false);
        assert_eq!(utterance.verb, Some(Verb::Resize));
    }

    #[test]
    fn abandon_erases_the_whole_sentence() {
        let mut sentence = Sentence::default();
        sentence.feed_digit(7);
        sentence.feed_verb(Verb::Nudge);
        sentence.abandon();
        assert!(sentence.is_empty());
        assert_eq!(sentence.feed_motion(Motion::Down, false).count, 1);
    }

    #[test]
    fn the_sentence_in_progress_is_always_speakable() {
        let mut sentence = Sentence::default();
        assert_eq!(sentence.display(), "");
        sentence.feed_digit(4);
        assert_eq!(sentence.display(), "4 …");
        sentence.feed_verb(Verb::Nudge);
        assert_eq!(sentence.display(), "4 NUDGE …");
    }

    #[test]
    fn counts_saturate_instead_of_overflowing() {
        let mut sentence = Sentence::default();
        for _ in 0..10 {
            sentence.feed_digit(9);
        }
        assert_eq!(sentence.feed_motion(Motion::Right, false).count, MAX_COUNT);
    }
}
