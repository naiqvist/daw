//! The stage codebook: scope-conditioned keys translated into intents.
//!
//! Scope is part of every binding even while the calibration grid gives
//! every scope the same vocabulary. That keeps the dispatch shape honest for
//! the first real surface, where the same key may mean different things at
//! different depths.

use super::Step;
use eframe::egui::{Key, Modifiers};

/// The conditioning context in which a key is interpreted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScopeContext {
    Root,
    Nested,
    /// Focus is in the browser. A different place, not a deeper one —
    /// which is why it is a context of its own rather than a stack level.
    Browser,
    /// Focus is inside a clip: the sequencer is drawn in the field and its
    /// grammar owns the keyboard. The stage keeps only what is global —
    /// time, the codebook, the browser, making tracks — and the one way
    /// out. Arrows, Enter and the verbs are the sequencer's to consume.
    Clip,
}

impl ScopeContext {
    #[cfg(test)]
    pub(super) const ALL: [Self; 4] = [Self::Root, Self::Nested, Self::Browser, Self::Clip];
}

/// A semantic request to the stage state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageIntent {
    Step(Step),
    Enter,
    Escape,
    /// Toggle the song clock between parked and rolling.
    ToggleTransport,
    /// Return the song clock to the top without changing its motion.
    Rewind,
    /// Show or hide the codebook for the current scope.
    Help,
    /// Summon the browser, or dismiss it. Focus goes with it.
    Browse,
    /// Add one printable character to the browser's filter.
    TypeChar(char),
    /// Remove one character from the browser's filter.
    Backspace,
    /// Append an audio track to the song.
    NewAudioTrack,
    /// Append an instrument track to the song — a MIDI track, in the
    /// words the chord is described in.
    NewInstrumentTrack,
    /// Empty the session slot the cursor stands on.
    Clear,
}

impl StageIntent {
    /// What this intent is called on the help surface.
    ///
    /// A `match` rather than a lookup table on purpose: a new intent that
    /// forgets its name is a COMPILE ERROR, which is the only way the
    /// codebook stays unable to lie about itself.
    pub fn label(self) -> &'static str {
        match self {
            Self::Step(Step::Up) => "move up",
            Self::Step(Step::Down) => "move down",
            Self::Step(Step::Left) => "move left",
            Self::Step(Step::Right) => "move right",
            Self::Enter => "go in",
            Self::Escape => "go out",
            Self::ToggleTransport => "stop / roll",
            Self::Rewind => "return to top",
            Self::Help => "this list",
            Self::Browse => "browse",
            Self::TypeChar(_) => "type to filter",
            Self::Backspace => "erase filter",
            Self::NewAudioTrack => "new audio track",
            Self::NewInstrumentTrack => "new midi track",
            Self::Clear => "clear slot",
        }
    }
}

/// A physical chord or text emitted by egui. Both travel through the same
/// scope-conditioned dispatcher before either can become an intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StageInput {
    Chord(Modifiers, Key),
    Text(char),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Binding {
    scope: ScopeContext,
    modifiers: Modifiers,
    key: Key,
    intent: StageIntent,
}

impl Binding {
    /// An unmodified key. The short code, spent on what is done often.
    const fn new(scope: ScopeContext, key: Key, intent: StageIntent) -> Self {
        Self {
            scope,
            modifiers: Modifiers::NONE,
            key,
            intent,
        }
    }

    /// A held modifier is a longer code, for what is done less often —
    /// and for what must not fire by accident under the fingers.
    const fn command(scope: ScopeContext, key: Key, intent: StageIntent) -> Self {
        Self {
            scope,
            modifiers: Modifiers::COMMAND,
            key,
            intent,
        }
    }

    /// Command with shift: a longer code again, and the pair of them puts
    /// the two track kinds one modifier apart — the same verb, told which
    /// kind to make.
    const fn command_shift(scope: ScopeContext, key: Key, intent: StageIntent) -> Self {
        Self {
            scope,
            modifiers: Modifiers::COMMAND.plus(Modifiers::SHIFT),
            key,
            intent,
        }
    }
}

/// The single source of truth for the stage keyboard vocabulary.
const BINDINGS: &[Binding] = &[
    // Time is global rather than conditioned by where focus stands. It is
    // still repeated as table data for every scope: the dispatcher has no
    // hidden universal-key path, and the codebook can therefore report the
    // exact vocabulary available from wherever the cursor is.
    Binding::new(ScopeContext::Root, Key::Space, StageIntent::ToggleTransport),
    Binding::new(ScopeContext::Root, Key::Home, StageIntent::Rewind),
    Binding::new(
        ScopeContext::Nested,
        Key::Space,
        StageIntent::ToggleTransport,
    ),
    Binding::new(ScopeContext::Nested, Key::Home, StageIntent::Rewind),
    Binding::new(
        ScopeContext::Browser,
        Key::Space,
        StageIntent::ToggleTransport,
    ),
    Binding::new(ScopeContext::Browser, Key::Home, StageIntent::Rewind),
    Binding::new(
        ScopeContext::Root,
        Key::ArrowUp,
        StageIntent::Step(Step::Up),
    ),
    Binding::new(
        ScopeContext::Root,
        Key::ArrowDown,
        StageIntent::Step(Step::Down),
    ),
    Binding::new(
        ScopeContext::Root,
        Key::ArrowLeft,
        StageIntent::Step(Step::Left),
    ),
    Binding::new(
        ScopeContext::Root,
        Key::ArrowRight,
        StageIntent::Step(Step::Right),
    ),
    Binding::new(ScopeContext::Root, Key::Enter, StageIntent::Enter),
    Binding::new(ScopeContext::Root, Key::Escape, StageIntent::Escape),
    Binding::new(
        ScopeContext::Nested,
        Key::ArrowUp,
        StageIntent::Step(Step::Up),
    ),
    Binding::new(
        ScopeContext::Nested,
        Key::ArrowDown,
        StageIntent::Step(Step::Down),
    ),
    Binding::new(
        ScopeContext::Nested,
        Key::ArrowLeft,
        StageIntent::Step(Step::Left),
    ),
    Binding::new(
        ScopeContext::Nested,
        Key::ArrowRight,
        StageIntent::Step(Step::Right),
    ),
    Binding::new(ScopeContext::Nested, Key::Enter, StageIntent::Enter),
    Binding::new(ScopeContext::Nested, Key::Escape, StageIntent::Escape),
    Binding::new(ScopeContext::Root, Key::Questionmark, StageIntent::Help),
    Binding::new(ScopeContext::Nested, Key::Questionmark, StageIntent::Help),
    Binding::command(ScopeContext::Root, Key::F, StageIntent::Browse),
    Binding::command(ScopeContext::Nested, Key::F, StageIntent::Browse),
    // Making a track is one verb told which kind to make, so the two
    // chords differ by exactly the modifier that distinguishes them.
    Binding::command(ScopeContext::Root, Key::T, StageIntent::NewAudioTrack),
    Binding::command(ScopeContext::Nested, Key::T, StageIntent::NewAudioTrack),
    Binding::command_shift(ScopeContext::Root, Key::T, StageIntent::NewInstrumentTrack),
    Binding::command_shift(
        ScopeContext::Nested,
        Key::T,
        StageIntent::NewInstrumentTrack,
    ),
    // Clearing a slot is the one destructive verb on the session, and it
    // is unmodified on purpose: it destroys a PLACE-holder, not content —
    // the pattern stays in the song — so it may sit under the fingers.
    // Both erase keys, because a performer reaches for whichever their
    // hands know, and the two never mean different things here.
    Binding::new(ScopeContext::Root, Key::Delete, StageIntent::Clear),
    Binding::new(ScopeContext::Root, Key::Backspace, StageIntent::Clear),
    Binding::new(ScopeContext::Nested, Key::Delete, StageIntent::Clear),
    Binding::new(ScopeContext::Nested, Key::Backspace, StageIntent::Clear),
    // Inside the browser the vocabulary is small and honest: move, open,
    // close, leave, erase, ask. Text is the pattern binding handled by
    // `dispatch` below because its payload is data rather than one
    // enumerated key.
    //
    // The library is a TREE, so it has a horizontal axis: right opens a
    // heading and then walks into it, left closes one and then climbs out.
    // Same two keys, same two meanings, as everywhere else on the stage.
    Binding::new(
        ScopeContext::Browser,
        Key::ArrowUp,
        StageIntent::Step(Step::Up),
    ),
    Binding::new(
        ScopeContext::Browser,
        Key::ArrowDown,
        StageIntent::Step(Step::Down),
    ),
    Binding::new(
        ScopeContext::Browser,
        Key::ArrowLeft,
        StageIntent::Step(Step::Left),
    ),
    Binding::new(
        ScopeContext::Browser,
        Key::ArrowRight,
        StageIntent::Step(Step::Right),
    ),
    Binding::new(ScopeContext::Browser, Key::Enter, StageIntent::Enter),
    Binding::new(ScopeContext::Browser, Key::Escape, StageIntent::Escape),
    Binding::new(
        ScopeContext::Browser,
        Key::Backspace,
        StageIntent::Backspace,
    ),
    Binding::new(ScopeContext::Browser, Key::Questionmark, StageIntent::Help),
    Binding::command(ScopeContext::Browser, Key::F, StageIntent::Browse),
    // Inside a clip the stage says almost nothing: the sequencer's grammar
    // is the vocabulary, and every key not listed here reaches it.
    Binding::new(ScopeContext::Clip, Key::Space, StageIntent::ToggleTransport),
    Binding::new(ScopeContext::Clip, Key::Home, StageIntent::Rewind),
    Binding::new(ScopeContext::Clip, Key::Escape, StageIntent::Escape),
    Binding::new(ScopeContext::Clip, Key::Questionmark, StageIntent::Help),
    Binding::command(ScopeContext::Clip, Key::F, StageIntent::Browse),
    Binding::command(ScopeContext::Clip, Key::T, StageIntent::NewAudioTrack),
    Binding::command_shift(ScopeContext::Clip, Key::T, StageIntent::NewInstrumentTrack),
];

/// Every binding in one scope, in table order. The help surface reads
/// THIS — it is a projection of the codebook, never prose written beside
/// it, so it cannot describe a key the stage does not actually answer to.
pub(super) fn bindings_for(
    scope: ScopeContext,
) -> impl Iterator<Item = (Modifiers, Key, StageIntent)> {
    BINDINGS
        .iter()
        .filter(move |binding| binding.scope == scope)
        .map(|binding| (binding.modifiers, binding.key, binding.intent))
}

/// How a chord is written on the codebook.
pub(super) fn chord_name(modifiers: Modifiers, key: Key) -> String {
    // ASCII on purpose. The conventional shift mark is `⇧` (U+21E7),
    // which the bundled Terminus does not carry, and the arrow it does
    // carry (`↑`) already means Up — one sign, one meaning, so shift gets
    // a mark of its own rather than borrowing a direction's.
    let mut name = String::new();
    if modifiers.command {
        name.push('^');
    }
    if modifiers.shift {
        name.push('+');
    }
    name.push_str(key.symbol_or_name());
    name
}

/// Translate one physical key in one scope. This is the only stage key
/// lookup used by both the application and the headless sequence driver.
pub(super) fn dispatch(scope: ScopeContext, input: StageInput) -> Option<StageIntent> {
    match input {
        StageInput::Chord(modifiers, key) => BINDINGS
            .iter()
            .find(|binding| {
                binding.scope == scope && binding.modifiers == modifiers && binding.key == key
            })
            .map(|binding| binding.intent),
        StageInput::Text(ch) if scope == ScopeContext::Browser && !ch.is_control() => {
            Some(StageIntent::TypeChar(ch))
        }
        StageInput::Text(_) => None,
    }
}

/// Every chord named by the table, once, MOST SPECIFIC FIRST — and that
/// order is load-bearing. egui's `consume_key` matches modifiers
/// logically, which means a held Shift or Alt the pattern did not ask
/// for is ignored: a pattern of `^T` matches a press of `^+T`. So the
/// chord with more modifiers has to be offered first, or the plainer
/// chord eats it and `^+T` silently becomes `^T`. Within one level of
/// specificity the table's own order stands.
pub(super) fn bound_chords() -> impl Iterator<Item = (Modifiers, Key)> {
    let mut chords: Vec<(Modifiers, Key)> = BINDINGS
        .iter()
        .enumerate()
        .filter_map(|(index, binding)| {
            let first = BINDINGS[..index].iter().all(|earlier| {
                (earlier.modifiers, earlier.key) != (binding.modifiers, binding.key)
            });
            first.then_some((binding.modifiers, binding.key))
        })
        .collect();
    chords.sort_by_key(|(modifiers, _)| std::cmp::Reverse(specificity(*modifiers)));
    chords.into_iter()
}

/// How many modifiers a chord holds. Command and ctrl count once between
/// them, because egui treats them as one logical key off a Mac.
fn specificity(modifiers: Modifiers) -> usize {
    usize::from(modifiers.command || modifiers.ctrl || modifiers.mac_cmd)
        + usize::from(modifiers.shift)
        + usize::from(modifiers.alt)
}

/// Take every chord bound IN `scope` out of this frame's input, in the
/// order [`bound_chords`] dictates, so that no plainer chord shadows a
/// more specific one. Only this scope's chords: a key another scope binds
/// is not the stage's here, and taking it would swallow it — inside a
/// clip the arrows and Enter belong to the sequencer's grammar, and the
/// stage must leave them in the input for it. `yield_to_grammar` names
/// chords that are someone else's THIS frame even though the scope binds
/// them: a sentence in progress owns Escape.
pub(super) fn consume_chords(
    input: &mut eframe::egui::InputState,
    scope: ScopeContext,
    yield_to_grammar: impl Fn(Modifiers, Key) -> bool,
) -> Vec<(Modifiers, Key)> {
    bound_chords()
        .filter(|(modifiers, key)| {
            dispatch(scope, StageInput::Chord(*modifiers, *key)).is_some()
                && !yield_to_grammar(*modifiers, *key)
                && input.consume_key(*modifiers, *key)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_scope_and_chord_maps_to_two_intents() {
        for (index, binding) in BINDINGS.iter().enumerate() {
            assert!(
                BINDINGS[..index].iter().all(|earlier| {
                    (earlier.scope, earlier.modifiers, earlier.key)
                        != (binding.scope, binding.modifiers, binding.key)
                }),
                "duplicate stage binding for {:?} + {:?}",
                binding.scope,
                binding.key
            );
        }
    }

    #[test]
    fn calibration_scopes_bind_the_same_keys_without_erasing_scope() {
        for (modifiers, key) in bound_chords() {
            assert_eq!(
                dispatch(ScopeContext::Root, StageInput::Chord(modifiers, key)),
                dispatch(ScopeContext::Nested, StageInput::Chord(modifiers, key))
            );
        }
    }

    /// The browser is REACHABLE from everywhere the stage can stand, and
    /// leaves by the same key it arrived by — plus the universal one.
    #[test]
    fn the_browser_can_be_summoned_from_anywhere_and_left_from_inside() {
        for scope in [ScopeContext::Root, ScopeContext::Nested] {
            assert_eq!(
                dispatch(scope, StageInput::Chord(Modifiers::COMMAND, Key::F)),
                Some(StageIntent::Browse),
                "{scope:?} cannot reach the browser"
            );
        }
        assert_eq!(
            dispatch(
                ScopeContext::Browser,
                StageInput::Chord(Modifiers::COMMAND, Key::F)
            ),
            Some(StageIntent::Browse)
        );
        assert_eq!(
            dispatch(
                ScopeContext::Browser,
                StageInput::Chord(Modifiers::NONE, Key::Escape)
            ),
            Some(StageIntent::Escape)
        );
    }

    #[test]
    fn transport_keys_are_global_table_bindings() {
        for scope in ScopeContext::ALL {
            assert_eq!(
                dispatch(scope, StageInput::Chord(Modifiers::NONE, Key::Space)),
                Some(StageIntent::ToggleTransport),
                "{scope:?} cannot stop or roll the song"
            );
            assert_eq!(
                dispatch(scope, StageInput::Chord(Modifiers::NONE, Key::Home)),
                Some(StageIntent::Rewind),
                "{scope:?} cannot return the song to the top"
            );
        }
    }

    /// The help surface shows every key the scope answers to, and only
    /// those. A codebook that omits a symbol is worse than none, because
    /// it is believed.
    #[test]
    fn the_projection_matches_the_table_exactly() {
        for scope in ScopeContext::ALL {
            let listed: Vec<_> = bindings_for(scope).collect();
            for (modifiers, key, intent) in &listed {
                assert_eq!(
                    dispatch(scope, StageInput::Chord(*modifiers, *key)),
                    Some(*intent),
                    "the help surface would name a key the stage ignores"
                );
            }
            let bound = BINDINGS.iter().filter(|b| b.scope == scope).count();
            assert_eq!(listed.len(), bound, "the help surface would hide a key");
        }
    }

    /// Every intent the table can dispatch has a name to show. Guaranteed
    /// by exhaustiveness at compile time; asserted here so the guarantee
    /// is visible as a claim rather than an accident.
    #[test]
    fn every_bound_intent_can_name_itself() {
        for binding in BINDINGS {
            assert!(!binding.intent.label().is_empty());
        }
    }

    #[test]
    fn printable_text_is_a_browser_binding_not_a_side_channel() {
        assert_eq!(
            dispatch(ScopeContext::Browser, StageInput::Text('k')),
            Some(StageIntent::TypeChar('k'))
        );
        assert_eq!(dispatch(ScopeContext::Root, StageInput::Text('k')), None);
        assert_eq!(
            dispatch(ScopeContext::Browser, StageInput::Text('\n')),
            None,
            "control characters are not filter text"
        );
    }

    /// The bug this guards against shipped: `^+T` made an audio track,
    /// because `^T` was offered to egui first and egui's logical match
    /// ignores the extra Shift. A chord whose modifiers include another
    /// chord's, on the same key, must always be offered before it.
    #[test]
    fn a_more_specific_chord_is_always_offered_before_a_plainer_one() {
        let chords: Vec<_> = bound_chords().collect();
        for (i, (wide, key)) in chords.iter().enumerate() {
            for (narrow, other) in &chords[..i] {
                if key != other {
                    continue;
                }
                let narrow_within_wide = (!narrow.shift || wide.shift)
                    && (!narrow.alt || wide.alt)
                    && (!(narrow.command || narrow.ctrl) || (wide.command || wide.ctrl));
                assert!(
                    !(narrow_within_wide && specificity(*narrow) < specificity(*wide)),
                    "{narrow:?}+{key:?} is offered before {wide:?}+{key:?} and would eat it"
                );
            }
        }
    }

    /// The same, through egui itself rather than through our reading of
    /// it: a real `^+T` press consumed the way `show` consumes it must
    /// come out as `^+T`, not as `^T`.
    #[test]
    fn a_shifted_chord_survives_being_consumed_through_egui() {
        use eframe::egui::{Event, InputOptions, InputState, RawInput};
        let press = Modifiers {
            ctrl: true,
            command: true,
            shift: true,
            ..Modifiers::NONE
        };
        let raw = RawInput {
            events: vec![Event::Key {
                key: Key::T,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: press,
            }],
            ..RawInput::default()
        };
        let mut input = InputState::default().begin_pass(raw, false, 1.0, InputOptions::default());
        let consumed = consume_chords(&mut input, ScopeContext::Root, |_, _| false);
        assert_eq!(
            consumed,
            vec![(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::T)],
            "the shifted chord was consumed as something else"
        );
        assert_eq!(
            dispatch(
                ScopeContext::Root,
                StageInput::Chord(consumed[0].0, consumed[0].1)
            ),
            Some(StageIntent::NewInstrumentTrack)
        );
    }

    /// Inside a clip the arrows and Enter are the grammar's. The stage's
    /// consumption must leave them in the input — this shipped the other
    /// way, and every key the sequencer needed vanished before it looked.
    #[test]
    fn a_scope_only_consumes_the_chords_it_binds() {
        use eframe::egui::{Event, InputOptions, InputState, RawInput};
        let press = |key| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let raw = RawInput {
            events: vec![press(Key::ArrowRight), press(Key::Enter), press(Key::Space)],
            ..RawInput::default()
        };
        let mut input = InputState::default().begin_pass(raw, false, 1.0, InputOptions::default());
        let consumed = consume_chords(&mut input, ScopeContext::Clip, |_, _| false);
        assert_eq!(
            consumed,
            vec![(Modifiers::NONE, Key::Space)],
            "the clip scope took a key it does not bind"
        );
        assert!(
            input.consume_key(Modifiers::NONE, Key::ArrowRight),
            "the arrow was swallowed before the grammar could see it"
        );
        assert!(
            input.consume_key(Modifiers::NONE, Key::Enter),
            "Enter was swallowed before the grammar could see it"
        );
    }
}
