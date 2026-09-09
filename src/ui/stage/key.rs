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
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
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
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    Home,
    Minus,
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    OpenBracket,
    PageDown,
    PageUp,
    Period,
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
            Self::H => "H",
            Self::I => "I",
            Self::J => "J",
            Self::K => "K",
            Self::L => "L",
            Self::M => "M",
            Self::N => "N",
            Self::O => "O",
            Self::P => "P",
            Self::Q => "Q",
            Self::R => "R",
            Self::S => "S",
            Self::T => "T",
            Self::U => "U",
            Self::V => "V",
            Self::W => "W",
            Self::X => "X",
            Self::Y => "Y",
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
            Self::F1 => "F1",
            Self::F2 => "F2",
            Self::F3 => "F3",
            Self::F4 => "F4",
            Self::F5 => "F5",
            Self::F6 => "F6",
            Self::F7 => "F7",
            Self::F8 => "F8",
            Self::F9 => "F9",
            Self::Home => "Home",
            Self::Minus => "Minus",
            Self::Num0 => "0",
            Self::Num1 => "1",
            Self::Num2 => "2",
            Self::Num3 => "3",
            Self::Num4 => "4",
            Self::Num5 => "5",
            Self::Num6 => "6",
            Self::Num7 => "7",
            Self::Num8 => "8",
            Self::Num9 => "9",
            Self::OpenBracket => "OpenBracket",
            Self::PageDown => "PageDown",
            Self::PageUp => "PageUp",
            Self::Period => "Period",
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
            Self::Period => ".",
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
    /// The digit a number key carries, if it is one. `Num0` reads as
    /// ten, because the row is counted from one: the tenth thing sits
    /// under the key to the right of nine.
    pub fn digit(self) -> Option<usize> {
        Some(match self {
            Self::Num1 => 1,
            Self::Num2 => 2,
            Self::Num3 => 3,
            Self::Num4 => 4,
            Self::Num5 => 5,
            Self::Num6 => 6,
            Self::Num7 => 7,
            Self::Num8 => 8,
            Self::Num9 => 9,
            Self::Num0 => 10,
            _ => return None,
        })
    }
}

impl Key {
    /// Every key there is, for a test that starts from a toolkit's key
    /// and has to find ours.
    #[cfg(test)]
    pub const ALL: [Key; 68] = [
        Key::A,
        Key::B,
        Key::C,
        Key::D,
        Key::E,
        Key::F,
        Key::G,
        Key::H,
        Key::I,
        Key::J,
        Key::K,
        Key::L,
        Key::M,
        Key::N,
        Key::O,
        Key::P,
        Key::Q,
        Key::R,
        Key::S,
        Key::T,
        Key::U,
        Key::V,
        Key::W,
        Key::X,
        Key::Y,
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
        Key::F1,
        Key::F2,
        Key::F3,
        Key::F4,
        Key::F5,
        Key::F6,
        Key::F7,
        Key::F8,
        Key::F9,
        Key::Home,
        Key::Minus,
        Key::Num0,
        Key::Num1,
        Key::Num2,
        Key::Num3,
        Key::Num4,
        Key::Num5,
        Key::Num6,
        Key::Num7,
        Key::Num8,
        Key::Num9,
        Key::OpenBracket,
        Key::PageDown,
        Key::PageUp,
        Key::Period,
        Key::Plus,
        Key::Questionmark,
        Key::Slash,
        Key::Space,
        Key::Tab,
    ];
}

/// The modifiers a chord may hold. The codebook currently spends command
/// (ctrl off a Mac, and egui already treats the pair as one logical key)
/// and shift. Alt is carried as well even before it has a binding: losing it
/// here lets an Alt chord fall through to an unrelated unmodified command.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Mods {
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Self = Self {
        command: false,
        shift: false,
        alt: false,
    };
    pub const COMMAND: Self = Self {
        command: true,
        shift: false,
        alt: false,
    };
    pub const SHIFT: Self = Self {
        command: false,
        shift: true,
        alt: false,
    };
    pub const ALT: Self = Self {
        command: false,
        shift: false,
        alt: true,
    };

    /// Both sets held.
    pub const fn plus(self, rhs: Self) -> Self {
        Self {
            command: self.command || rhs.command,
            shift: self.shift || rhs.shift,
            alt: self.alt || rhs.alt,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plus_holds_both_and_none_holds_neither() {
        let all = Mods::COMMAND.plus(Mods::SHIFT).plus(Mods::ALT);
        assert!(all.command && all.shift && all.alt);
        assert_eq!(Mods::NONE.plus(Mods::NONE), Mods::NONE);
        assert_ne!(Mods::COMMAND, Mods::SHIFT);
        assert_ne!(Mods::ALT, Mods::NONE);
    }
}
