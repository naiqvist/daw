//! The stage's own keys: what the codebook binds, in no toolkit's words.
//!
//! The codebook translates chords into intents, and an intent may name
//! no keystroke — that rule is what keeps `crate::intent` outside every
//! frame. The chord itself needs the same protection one layer down: a
//! binding table written in egui's `Key` is a binding table that only an
//! egui view can drive, and the core's whole point is to be driven by any
//! view at all. So the keys are ours, and the view that reads a real
//! keyboard translates into them at its own edge (`view::input`).
//!
//! Only what the stage actually binds is here. A key nothing is bound to
//! is not a key this app has, and a list of every key a keyboard might
//! carry would only be somewhere for a binding to hide.
//!
//! The names a key prints as are egui's, character for character: the
//! codebook plaques carve them, and a renamed key would move a glyph.

/// A logical key, named the way the codebook writes it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Key {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    V,
    W,
    X,
    Z,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    Backspace,
    CloseBracket,
    Comma,
    Delete,
    End,
    Enter,
    Equals,
    Escape,
    F2,
    Home,
    Minus,
    OpenBracket,
    PageDown,
    PageUp,
    Plus,
    Questionmark,
    Slash,
    Space,
    Tab,
}

impl Key {
    /// The key's name, as egui spells it.
    pub fn name(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
            Self::E => "E",
            Self::F => "F",
            Self::G => "G",
            Self::L => "L",
            Self::M => "M",
            Self::N => "N",
            Self::O => "O",
            Self::P => "P",
            Self::Q => "Q",
            Self::R => "R",
            Self::S => "S",
            Self::T => "T",
            Self::V => "V",
            Self::W => "W",
            Self::X => "X",
            Self::Z => "Z",
            Self::ArrowDown => "Down",
            Self::ArrowLeft => "Left",
            Self::ArrowRight => "Right",
            Self::ArrowUp => "Up",
            Self::Backspace => "Backspace",
            Self::CloseBracket => "CloseBracket",
            Self::Comma => "Comma",
            Self::Delete => "Delete",
            Self::End => "End",
            Self::Enter => "Enter",
            Self::Equals => "Equals",
            Self::Escape => "Escape",
            Self::F2 => "F2",
            Self::Home => "Home",
            Self::Minus => "Minus",
            Self::OpenBracket => "OpenBracket",
            Self::PageDown => "PageDown",
            Self::PageUp => "PageUp",
            Self::Plus => "Plus",
            Self::Questionmark => "Questionmark",
            Self::Slash => "Slash",
            Self::Space => "Space",
            Self::Tab => "Tab",
        }
    }

    /// The key's symbol where it has one, else its name — egui's own
    /// choice of symbols, kept so the codebook plaques do not move.
    pub fn symbol_or_name(self) -> &'static str {
        match self {
            Self::ArrowDown => "⏷",
            Self::ArrowLeft => "⏴",
            Self::ArrowRight => "⏵",
            Self::ArrowUp => "⏶",
            Self::Comma => ",",
            Self::Minus => "−",
            Self::Plus => "+",
            Self::Equals => "=",
            Self::Slash => "/",
            Self::Questionmark => "?",
            Self::OpenBracket => "[",
            Self::CloseBracket => "]",
            _ => self.name(),
        }
    }
}

impl Key {
    /// Every key there is, for a test that starts from a toolkit's key
    /// and has to find ours.
    #[cfg(test)]
    pub const ALL: [Key; 43] = [
        Key::A,
        Key::B,
        Key::C,
        Key::D,
        Key::E,
        Key::F,
        Key::G,
        Key::L,
        Key::M,
        Key::N,
        Key::O,
        Key::P,
        Key::Q,
        Key::R,
        Key::S,
        Key::T,
        Key::V,
        Key::W,
        Key::X,
        Key::Z,
        Key::ArrowDown,
        Key::ArrowLeft,
        Key::ArrowRight,
        Key::ArrowUp,
        Key::Backspace,
        Key::CloseBracket,
        Key::Comma,
        Key::Delete,
        Key::End,
        Key::Enter,
        Key::Equals,
        Key::Escape,
        Key::F2,
        Key::Home,
        Key::Minus,
        Key::OpenBracket,
        Key::PageDown,
        Key::PageUp,
        Key::Plus,
        Key::Questionmark,
        Key::Slash,
        Key::Space,
        Key::Tab,
    ];
}

/// The modifiers a chord may hold. Two, because the codebook uses two:
/// command (ctrl off a Mac, and egui already treats the pair as one
/// logical key) and shift. Alt is not a code this app spends.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Mods {
    pub command: bool,
    pub shift: bool,
}

impl Mods {
    pub const NONE: Self = Self {
        command: false,
        shift: false,
    };
    pub const COMMAND: Self = Self {
        command: true,
        shift: false,
    };
    pub const SHIFT: Self = Self {
        command: false,
        shift: true,
    };

    /// Both held.
    pub const fn plus(self, rhs: Self) -> Self {
        Self {
            command: self.command || rhs.command,
            shift: self.shift || rhs.shift,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plus_holds_both_and_none_holds_neither() {
        let both = Mods::COMMAND.plus(Mods::SHIFT);
        assert!(both.command && both.shift);
        assert_eq!(Mods::NONE.plus(Mods::NONE), Mods::NONE);
        assert_ne!(Mods::COMMAND, Mods::SHIFT);
    }
}
