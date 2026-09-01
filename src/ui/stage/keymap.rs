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
}

impl ScopeContext {
    #[cfg(test)]
    pub(super) const ALL: [Self; 3] = [Self::Root, Self::Nested, Self::Browser];
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
    // Inside the browser the vocabulary is small and honest: move, open,
    // leave, erase, ask. Text is the pattern binding handled by `dispatch`
    // below because its payload is data rather than one enumerated key.
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
    Binding::new(ScopeContext::Browser, Key::Enter, StageIntent::Enter),
    Binding::new(ScopeContext::Browser, Key::Escape, StageIntent::Escape),
    Binding::new(
        ScopeContext::Browser,
        Key::Backspace,
        StageIntent::Backspace,
    ),
    Binding::new(ScopeContext::Browser, Key::Questionmark, StageIntent::Help),
    Binding::command(ScopeContext::Browser, Key::F, StageIntent::Browse),
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
    if modifiers.command {
        format!("^{}", key.symbol_or_name())
    } else {
        key.symbol_or_name().to_owned()
    }
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

/// Every chord named by the table, once, in table order. `show` uses this
/// to consume stage keys without carrying a second hardcoded list.
pub(super) fn bound_chords() -> impl Iterator<Item = (Modifiers, Key)> {
    BINDINGS.iter().enumerate().filter_map(|(index, binding)| {
        let first = BINDINGS[..index]
            .iter()
            .all(|earlier| (earlier.modifiers, earlier.key) != (binding.modifiers, binding.key));
        first.then_some((binding.modifiers, binding.key))
    })
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
}
