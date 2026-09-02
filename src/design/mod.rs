//! The design alphabet: the symbols this app is allowed to transmit in.
//!
//! Not a theme. A theme is a set of preferences; this is a CODE, and it is
//! built on the claim that a screen is a channel with finite capacity and a
//! receiver (the eye) whose limits are measurable. Every rule below falls
//! out of that claim rather than out of taste.
//!
//! **The alphabet is capped, because absolute judgment is.** A single
//! perceptual dimension carries only two to three bits of absolute judgment
//! — five to seven levels told apart reliably with nothing to compare them
//! against. So the luminance ladder has FIVE rungs and there are TWO hues.
//! Extra symbols are not extra expressiveness: they are symbols the
//! receiver cannot decode, and they make every neighbouring symbol less
//! reliable too.
//!
//! **Steps are perceptual, not numeric.** sRGB bytes are not a perceptual
//! space, so a ladder spaced evenly in bytes is spaced unevenly in the only
//! space that matters. Every rung here is declared by its CIE L\* and the
//! byte is a consequence — `lightness` recovers it, and the tests check the
//! whole ladder against it.
//!
//! **Separation is budgeted, not uniform.** Equal spacing would maximise
//! the symbol count in a range, but our roles are not equiprobable: what a
//! screen shows at rest is common and what it shows when marked is rare and
//! costly to miss. So the resting rungs sit close and low, and FOCUS is
//! thrown far clear of all of them. That is the same logic that gives a
//! frequent symbol a short code and a rare one a long one, applied to
//! contrast instead of length.
//!
//! **Redundancy is spent where noise is expensive.** The noise here is
//! real: peripheral vision, a glance, ambient light, an unfamiliar display.
//! Signals that must survive it are carried on more than one channel at
//! once — never hue alone. [`Signal::channels`] records that, and a test
//! holds alarm-tier signals to at least two.
//!
//! **One symbol, one meaning.** A code whose symbol maps to two meanings is
//! not uniquely decodable, and context does not rescue it under noise. No
//! two roles here share a value, and a test says so.
//!
//! **What this cannot tell us.** Shannon set meaning aside on purpose: the
//! theory covers transmission, not semantics. It says how many signals fit
//! and how far apart they must sit. It does NOT say that jeopardy deserves
//! red, or that being armed matters more than clipping. Those are human
//! judgements, made by the person whose app this is, and they are recorded
//! here as decisions rather than derived as results.

pub mod block;
pub mod circuit;
pub mod codex;
pub mod glyph;
pub mod grain;
pub mod kit;
pub mod motion;
pub mod signs;

use eframe::egui::Color32;

// ------------------------------------------------------------------ tint

/// The bone tint: the ladder is warm, the way traces on a black board are
/// bone rather than white. Red is the channel the ladder is measured on,
/// so it keeps the grey's byte and the others sit a little under it.
/// Chroma stays far below the point where the eye would call it a hue.
pub const fn bone(level: u8) -> Color32 {
    let r = level as u32;
    let g = (r * 965 + 500) / 1000;
    let b = (r * 900 + 500) / 1000;
    Color32::from_rgb(level, g as u8, b as u8)
}

/// The most chroma a resting rung may carry, as a share of its brightest
/// channel. A tint, not a hue: the colourless test allows this much and
/// no more.
pub const TINT_CHROMA_MAX: f32 = 0.11;

/// Whether a colour is neutral or merely tinted — carries no hue the eye
/// would name.
pub fn is_tint(color: Color32) -> bool {
    let max = color.r().max(color.g()).max(color.b());
    let min = color.r().min(color.g()).min(color.b());
    (max - min) as f32 <= TINT_CHROMA_MAX * max as f32 + 2.0
}

// ---------------------------------------------------------------- tiers

/// How loudly a symbol speaks. Naming the tier makes the salience budget
/// COUNTABLE: a screen's cost is how much high-tier ink it spends, which
/// is a number rather than a feeling.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Tier {
    /// The void behind everything.
    Ground,
    /// Where things are: fills, edges, rules. Carries structure, not news.
    Structure,
    /// What things say: names, values, marks.
    Content,
    /// The marked one. Rare by construction — if it is everywhere it has
    /// stopped being an exception.
    Exception,
    /// Something is at stake. May preempt anything.
    Alarm,
}

/// A perceptual channel a signal can ride on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Channel {
    Hue,
    Luminance,
    Position,
    Geometry,
    Text,
}

/// A symbol in the alphabet: its value, how loud it is, and — the part
/// that matters under noise — which channels are required to carry it.
#[derive(Clone, Copy, Debug)]
pub struct Signal {
    pub color: Color32,
    pub tier: Tier,
    /// Every channel this signal MUST be carried on. More than one is
    /// deliberate redundancy, bought where being missed is expensive.
    pub channels: &'static [Channel],
}

/// Which side of the luminance field the ground occupies.
///
/// Polarity changes the direction of contrast, never the meaning of a
/// symbol. Dark is the house default; light is the daylight/e-paper
/// alternative.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Polarity {
    #[default]
    Dark,
    Light,
}

/// One design code projected onto one ground.
///
/// Keeping both polarities in one value prevents the light side from
/// becoming a second vocabulary: every role retains its tier and required
/// channels, and only its visible value changes direction around the ground.
#[derive(Clone, Copy, Debug)]
pub struct Alphabet {
    pub polarity: Polarity,
    pub ground: Signal,
    pub well: Signal,
    pub surface: Signal,
    pub edge: Signal,
    pub ink: Signal,
    pub focus: Signal,
    pub jeopardy_latent: Signal,
    pub jeopardy_active: Signal,
    pub live: Signal,
    pub live_dim: Signal,
}

impl Alphabet {
    /// The requested projection of the one alphabet.
    pub const fn for_polarity(polarity: Polarity) -> &'static Self {
        match polarity {
            Polarity::Dark => &DARK,
            Polarity::Light => &LIGHT,
        }
    }

    /// Every symbol in this projection. Kept as a value so all invariants
    /// can be checked independently on both grounds.
    pub const fn signals(self) -> [Signal; 10] {
        [
            self.ground,
            self.well,
            self.surface,
            self.edge,
            self.ink,
            self.focus,
            self.jeopardy_latent,
            self.jeopardy_active,
            self.live,
            self.live_dim,
        ]
    }
}

// ------------------------------------------------------- luminance ladder

/// The luminance ladder, in CIE L\*. Five rungs: the cap is the receiver's,
/// not a preference. The gaps are deliberately uneven — see the module
/// note on budgeted separation.
pub mod lstar {
    /// The original black-ground projection. These values are stable: dark
    /// remains the default and is not redesigned to make room for light.
    pub mod dark {
        pub const GROUND: f32 = 0.0;
        pub const WELL: f32 = 2.7;
        pub const SURFACE: f32 = 5.5;
        pub const EDGE: f32 = 20.0;
        pub const INK: f32 = 60.0;
        pub const FOCUS: f32 = 95.0;
        pub const LADDER: [f32; 6] = [GROUND, WELL, SURFACE, EDGE, INK, FOCUS];
    }

    /// The paper-ground projection, in semantic order from ground to focus.
    ///
    /// Every rung sits the SAME PERCEPTUAL DISTANCE from the ground as its
    /// dark twin does, measured away from paper instead of toward light.
    /// That is what makes this one code in two projections rather than two
    /// palettes: the separation budget — resting rungs close and low, focus
    /// thrown far clear of all of them — is the argument the alphabet is
    /// built on, and an inversion that kept the direction but shrank the
    /// distances would keep the look and throw the reasoning away.
    ///
    /// An earlier version compressed the span deliberately, for e-paper
    /// glare. It was tried and rejected in use: at a 92 -> 24 span the
    /// three resting rungs sit within six L* of each other and the
    /// structure of a screen stops being visible at all. Glare is a real
    /// cost, but it is paid by the GROUND being paper rather than by the
    /// marks on it failing to separate.
    pub mod light {
        pub const GROUND: f32 = 96.0;
        pub const WELL: f32 = 93.3;
        pub const SURFACE: f32 = 90.5;
        pub const EDGE: f32 = 76.0;
        pub const INK: f32 = 36.0;
        pub const FOCUS: f32 = 1.0;
        pub const LADDER: [f32; 6] = [GROUND, WELL, SURFACE, EDGE, INK, FOCUS];
    }

    // Compatibility names are the default, dark projection. Existing
    // surfaces remain dark until they explicitly accept an `Alphabet`.
    pub const GROUND: f32 = dark::GROUND;
    pub const WELL: f32 = dark::WELL;
    pub const SURFACE: f32 = dark::SURFACE;
    pub const EDGE: f32 = dark::EDGE;
    pub const INK: f32 = dark::INK;
    pub const FOCUS: f32 = dark::FOCUS;
    pub const LADDER: [f32; 6] = dark::LADDER;

    /// The smallest perceptual gap the alphabet tolerates between two
    /// rungs meant to be told apart, under glance conditions rather than
    /// under study. The bottom of the ladder was pushed toward black on
    /// purpose, and now packs three rungs under L* 6: a resting fill is
    /// meant to be barely there, and a well barely below that. Those two
    /// pairs are the only ones that sit near the floor; everything meant
    /// to be read at a glance is separated by ten or more.
    pub const MIN_SEPARATION: f32 = 2.5;
}

/// The ground: what the screen is when nothing has been said.
pub const GROUND: Signal = Signal {
    color: Color32::from_gray(0),
    tier: Tier::Ground,
    channels: &[Channel::Luminance],
};

/// A recess below the surfaces. See [`lstar::WELL`].
pub const WELL: Signal = Signal {
    color: bone(10),
    tier: Tier::Structure,
    channels: &[Channel::Luminance, Channel::Position],
};

/// A resting object's fill.
pub const SURFACE: Signal = Signal {
    color: bone(18),
    tier: Tier::Structure,
    channels: &[Channel::Luminance],
};

/// A resting object's boundary; also the periphery's hairlines.
pub const EDGE: Signal = Signal {
    color: bone(48),
    tier: Tier::Structure,
    channels: &[Channel::Luminance],
};

/// Readable marks on a surface: names, values, refusal words.
pub const INK: Signal = Signal {
    color: bone(145),
    tier: Tier::Content,
    channels: &[Channel::Luminance],
};

/// The addressed thing, and the brightest value the app may draw.
///
/// Carried on luminance AND position on purpose: the sync marker is what
/// every later symbol is decoded against, so it is the one signal that
/// must survive a bad glance. Inversion (this as fill, [`GROUND`] as ink)
/// is its validated form.
pub const FOCUS: Signal = Signal {
    color: bone(241),
    tier: Tier::Exception,
    channels: &[Channel::Luminance, Channel::Position],
};

// The light neutral ladder. Neutral grey is intentional: the paper feeling
// comes from value and the display around it, while hue remains reserved for
// jeopardy and the sounding present.
const LIGHT_GROUND: Signal = Signal {
    color: bone(243),
    tier: Tier::Ground,
    channels: &[Channel::Luminance],
};

const LIGHT_WELL: Signal = Signal {
    color: bone(236),
    tier: Tier::Structure,
    channels: &[Channel::Luminance, Channel::Position],
};

const LIGHT_SURFACE: Signal = Signal {
    color: bone(228),
    tier: Tier::Structure,
    channels: &[Channel::Luminance],
};

const LIGHT_EDGE: Signal = Signal {
    color: bone(187),
    tier: Tier::Structure,
    channels: &[Channel::Luminance],
};

const LIGHT_INK: Signal = Signal {
    color: bone(85),
    tier: Tier::Content,
    channels: &[Channel::Luminance],
};

const LIGHT_FOCUS: Signal = Signal {
    color: bone(4),
    tier: Tier::Exception,
    channels: &[Channel::Luminance, Channel::Position],
};

// ------------------------------------------------------------------ hues

/// Two hues, and no more. Each names ONE meaning for the whole app; a hue
/// that means two things means nothing in both.
///
/// Hue is spent here rather than elsewhere because it is pre-attentive —
/// it is seen without being looked at — so it belongs to what must not be
/// missed while the eye is somewhere else. Track identity, by contrast, is
/// already carried by position, and spending hue on it would buy nothing
/// and cost the channel permanently.
///
/// The two sit almost opposite on the hue circle, which keeps them apart
/// for viewers who separate red from green poorly. Nothing here says
/// "fine" in green: the calm state is the ABSENCE of colour, so there is
/// no red/green pair to confuse in the first place.
pub mod hue {
    /// Degrees on the hue circle. Kept as data so the separation between
    /// the two meanings is checkable rather than assumed.
    pub const JEOPARDY_DEG: f32 = 11.0;
    pub const LIVE_DEG: f32 = 190.0;
    /// How far apart two meanings must sit to stay distinct in a glance.
    pub const MIN_SEPARATION_DEG: f32 = 90.0;
}

/// Something is at stake and inaction has a cost: armed, recording,
/// clipping, an xrun, a refused take.
///
/// The latent step is the loaded spring — nothing lost yet, but the next
/// event matters. Never carried on hue alone.
pub const JEOPARDY_LATENT: Signal = Signal {
    color: Color32::from_rgb(166, 62, 38),
    tier: Tier::Alarm,
    channels: &[Channel::Hue, Channel::Text],
};

/// The same meaning, discharging: it is happening now.
pub const JEOPARDY_ACTIVE: Signal = Signal {
    color: Color32::from_rgb(255, 94, 58),
    tier: Tier::Alarm,
    channels: &[Channel::Hue, Channel::Text, Channel::Luminance],
};

/// The sounding present: the playhead, what is making sound right now,
/// meters in motion. Not an alarm — this is where the music is.
pub const LIVE: Signal = Signal {
    // Pale, nearly white: the glow of a rune that has woken, not a
    // coloured light. Still the same hue, still its own meaning.
    color: Color32::from_rgb(186, 232, 240),
    tier: Tier::Exception,
    channels: &[Channel::Hue, Channel::Position],
};

/// The same meaning, quieter: present but not the subject.
pub const LIVE_DIM: Signal = Signal {
    color: Color32::from_rgb(96, 140, 148),
    tier: Tier::Exception,
    channels: &[Channel::Hue, Channel::Position],
};

/// Paper-ground jeopardy: pigment rather than emitted light. The active
/// step goes darker, increasing its distance from the paper.
const LIGHT_JEOPARDY_LATENT: Signal = Signal {
    color: Color32::from_rgb(179, 106, 90),
    tier: Tier::Alarm,
    channels: &[Channel::Hue, Channel::Text],
};

const LIGHT_JEOPARDY_ACTIVE: Signal = Signal {
    color: Color32::from_rgb(142, 56, 43),
    tier: Tier::Alarm,
    channels: &[Channel::Hue, Channel::Text, Channel::Luminance],
};

/// Paper-ground sounding present. Like jeopardy, greater intensity means
/// more ink rather than more emitted light.
const LIGHT_LIVE: Signal = Signal {
    color: Color32::from_rgb(59, 112, 121),
    tier: Tier::Exception,
    channels: &[Channel::Hue, Channel::Position],
};

const LIGHT_LIVE_DIM: Signal = Signal {
    color: Color32::from_rgb(104, 145, 151),
    tier: Tier::Exception,
    channels: &[Channel::Hue, Channel::Position],
};

/// The default black-ground projection.
pub const DARK: Alphabet = Alphabet {
    polarity: Polarity::Dark,
    ground: GROUND,
    well: WELL,
    surface: SURFACE,
    edge: EDGE,
    ink: INK,
    focus: FOCUS,
    jeopardy_latent: JEOPARDY_LATENT,
    jeopardy_active: JEOPARDY_ACTIVE,
    live: LIVE,
    live_dim: LIVE_DIM,
};

/// The lower-contrast paper-ground projection.
pub const LIGHT: Alphabet = Alphabet {
    polarity: Polarity::Light,
    ground: LIGHT_GROUND,
    well: LIGHT_WELL,
    surface: LIGHT_SURFACE,
    edge: LIGHT_EDGE,
    ink: LIGHT_INK,
    focus: LIGHT_FOCUS,
    jeopardy_latent: LIGHT_JEOPARDY_LATENT,
    jeopardy_active: LIGHT_JEOPARDY_ACTIVE,
    live: LIGHT_LIVE,
    live_dim: LIGHT_LIVE_DIM,
};

/// Every symbol in the alphabet. The cap is enforced against this list.
pub const ALPHABET: [Signal; 10] = DARK.signals();

// ----------------------------------------------------------------- scales

/// Space, as a ratio ladder.
///
/// Ratios rather than fixed increments because magnitude is perceived
/// roughly logarithmically: equal RATIOS are equal perceptual steps, while
/// equal increments crowd at the top and sprawl at the bottom. Base 8,
/// ratio 3/2, rounded to whole pixels.
pub mod space {
    pub const RATIO: f32 = 1.5;
    pub const BASE: f32 = 8.0;

    /// Half the base, and deliberately NOT a rung: it is the inset a
    /// hairline needs, not a spacing decision anybody makes. Kept out of
    /// [`LADDER`] so the ladder stays a true ratio ladder.
    pub const HAIR: f32 = 4.0;

    pub const SNUG: f32 = 8.0;
    pub const STEP: f32 = 12.0;
    pub const ROOM: f32 = 18.0;
    pub const OPEN: f32 = 27.0;
    pub const VAST: f32 = 40.0;

    pub const LADDER: [f32; 5] = [SNUG, STEP, ROOM, OPEN, VAST];
}

/// Type, as a ratio ladder. Monospace throughout — not an aesthetic
/// preference but a coding one: fixed advance means a column position is
/// the same distance on every row, so alignment carries information for
/// free.
///
/// Base 16 rather than the old system's 9–13: that ladder was drawn for
/// dense cards on a standard display, and this one is sized for the
/// display it will actually be read on.
pub mod type_scale {
    pub const RATIO: f32 = 1.25;
    pub const BASE: f32 = 16.0;

    pub const MICRO: f32 = 13.0;
    pub const BODY: f32 = 16.0;
    pub const TITLE: f32 = 20.0;
    pub const DISPLAY: f32 = 25.0;

    pub const LADDER: [f32; 4] = [MICRO, BODY, TITLE, DISPLAY];
}

/// The one multiplier for display density. Every dimension the app draws
/// passes through here, so "everything is slightly too small" is one
/// number rather than an archaeology of constants.
pub const SCALE: f32 = 1.0;

/// A scale-corrected dimension.
pub fn px(value: f32) -> f32 {
    (value * SCALE).round()
}

// ------------------------------------------------------------ perception

/// CIE L\* of an sRGB channel value, 0..=255. The receiver's scale, which
/// is the only one the ladder's spacing is meaningful in.
pub fn lightness(byte: u8) -> f32 {
    let c = f32::from(byte) / 255.0;
    let linear = if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    };
    if linear > 0.008_856 {
        116.0 * linear.cbrt() - 16.0
    } else {
        903.3 * linear
    }
}

/// Relative luminance of a colour, then its L\*. For the grey ladder this
/// is just [`lightness`]; for the hues it weights the channels the way the
/// eye does.
pub fn lightness_of(color: Color32) -> f32 {
    let linear = |byte: u8| {
        let c = f32::from(byte) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let y = 0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b());
    if y > 0.008_856 {
        116.0 * y.cbrt() - 16.0
    } else {
        903.3 * y
    }
}

/// Hue angle in degrees, 0..360. Used to check that the two meanings are
/// far enough apart to stay two meanings.
pub fn hue_degrees(color: Color32) -> f32 {
    let (r, g, b) = (
        f32::from(color.r()) / 255.0,
        f32::from(color.g()) / 255.0,
        f32::from(color.b()) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    if delta <= f32::EPSILON {
        return 0.0;
    }
    let hue = if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    if hue < 0.0 { hue + 360.0 } else { hue }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: [Alphabet; 2] = [DARK, LIGHT];

    fn neutral_rungs(alphabet: Alphabet) -> [Signal; 6] {
        [
            alphabet.ground,
            alphabet.well,
            alphabet.surface,
            alphabet.edge,
            alphabet.ink,
            alphabet.focus,
        ]
    }

    fn declared_ladder(polarity: Polarity) -> [f32; 6] {
        match polarity {
            Polarity::Dark => lstar::dark::LADDER,
            Polarity::Light => lstar::light::LADDER,
        }
    }

    fn distance_from_ground(alphabet: Alphabet, signal: Signal) -> f32 {
        (lightness_of(signal.color) - lightness_of(alphabet.ground.color)).abs()
    }

    /// Every rung's byte is the value its declared L\* asks for. The
    /// ladder is spaced in the receiver's space, and this is what keeps
    /// the bytes honest to that claim.
    #[test]
    fn every_rung_sits_where_its_lightness_says_it_does() {
        for alphabet in BOTH {
            for (signal, declared) in neutral_rungs(alphabet)
                .into_iter()
                .zip(declared_ladder(alphabet.polarity))
            {
                let actual = lightness(signal.color.r());
                assert!(
                    (actual - declared).abs() < 1.0,
                    "{:?} {:?} claims L* {declared} but measures {actual}",
                    alphabet.polarity,
                    signal.color
                );
            }
        }
    }

    /// No two rungs are confusable. This is the whole reason the ladder
    /// is short: symbols the eye cannot separate are not symbols.
    #[test]
    fn no_two_rungs_are_closer_than_the_separation_floor() {
        for alphabet in BOTH {
            let ladder = declared_ladder(alphabet.polarity);
            for (index, a) in ladder.iter().enumerate() {
                for b in &ladder[index + 1..] {
                    assert!(
                        (b - a).abs() >= lstar::MIN_SEPARATION,
                        "{:?}: L* {a} and {b} are within the noise of each other",
                        alphabet.polarity
                    );
                }
            }
        }
    }

    /// Contrast changes direction with the ground, but role order does not:
    /// every step travels farther from the page and FOCUS travels farthest.
    #[test]
    fn every_ladder_moves_away_from_ground_and_focus_is_loudest() {
        for alphabet in BOTH {
            let distances = neutral_rungs(alphabet).map(|signal| {
                (lightness_of(signal.color) - lightness_of(alphabet.ground.color)).abs()
            });
            assert!(
                distances.windows(2).all(|pair| pair[1] > pair[0]),
                "{:?} ladder does not move away from its ground: {distances:?}",
                alphabet.polarity
            );
            let loudest = alphabet
                .signals()
                .into_iter()
                .map(|signal| distance_from_ground(alphabet, signal))
                .fold(f32::MIN, f32::max);
            assert!(
                (distance_from_ground(alphabet, alphabet.focus) - loudest).abs() < f32::EPSILON,
                "something in {:?} speaks louder than focus",
                alphabet.polarity
            );
        }
    }

    /// Paper is not a negative photograph. Its ground is below bare white,
    /// its focus is above black, and its total span is intentionally quieter.
    #[test]
    fn the_light_ground_is_the_dark_ladder_measured_the_other_way() {
        // Rung for rung, the same distance from the ground. This is the
        // whole claim of "one code, two projections": if the light side
        // may shrink its separations it is a second palette wearing the
        // first one's names.
        for (light, dark) in lstar::light::LADDER.iter().zip(lstar::dark::LADDER) {
            let from_paper = lstar::light::GROUND - light;
            let from_black = dark - lstar::dark::GROUND;
            assert!(
                (from_paper - from_black).abs() < 1.0,
                "light sits {from_paper} from its ground where dark sits {from_black}"
            );
        }
        // And the ground really is paper, with the marked one nearly black.
        assert!(lstar::light::GROUND > 90.0);
        assert!(lstar::light::FOCUS < 10.0);
    }

    /// The cap, held as code. Absolute judgment on one dimension runs out
    /// around seven levels; we spend six and keep the margin.
    #[test]
    fn the_alphabet_stays_inside_the_receivers_capacity() {
        for alphabet in BOTH {
            let ladder = declared_ladder(alphabet.polarity);
            assert!(
                ladder.len() <= 7,
                "more luminance rungs than absolute judgment can carry"
            );
            assert_eq!(ladder.len(), 6);

            let hues: Vec<f32> = alphabet
                .signals()
                .iter()
                .filter(|signal| !is_tint(signal.color))
                .map(|signal| hue_degrees(signal.color))
                .filter(|degrees| *degrees > 0.0)
                .collect();
            let distinct = hues.iter().fold(Vec::new(), |mut acc: Vec<f32>, degrees| {
                if !acc.iter().any(|seen| (seen - degrees).abs() < 20.0) {
                    acc.push(*degrees);
                }
                acc
            });
            assert_eq!(
                distinct.len(),
                2,
                "{:?} may carry exactly two meanings in hue",
                alphabet.polarity
            );
        }
    }

    /// One symbol, one meaning: nothing in the alphabet is drawn twice.
    #[test]
    fn the_alphabet_is_uniquely_decodable() {
        for alphabet in BOTH {
            let signals = alphabet.signals();
            for (index, a) in signals.iter().enumerate() {
                for b in &signals[index + 1..] {
                    assert_ne!(
                        a.color, b.color,
                        "{:?}: two roles share one value, so neither can be decoded",
                        alphabet.polarity
                    );
                }
            }
        }
    }

    /// The two hues are far enough apart to survive a glance, and to stay
    /// separable for a viewer who reads red and green poorly.
    #[test]
    fn the_two_meanings_sit_far_apart_on_the_hue_circle() {
        for alphabet in BOTH {
            let jeopardy = hue_degrees(alphabet.jeopardy_active.color);
            let live = hue_degrees(alphabet.live.color);
            let separation = (jeopardy - live).abs().min(360.0 - (jeopardy - live).abs());
            assert!(
                separation >= hue::MIN_SEPARATION_DEG,
                "{:?}: jeopardy at {jeopardy}° and live at {live}° are only {separation}° apart",
                alphabet.polarity
            );
        }
    }

    /// Both steps of one meaning are the SAME meaning: intensity moves,
    /// hue does not.
    #[test]
    fn intensity_steps_keep_their_hue() {
        for alphabet in BOTH {
            for (dim, active) in [
                (alphabet.jeopardy_latent, alphabet.jeopardy_active),
                (alphabet.live_dim, alphabet.live),
            ] {
                let drift = (hue_degrees(dim.color) - hue_degrees(active.color)).abs();
                assert!(
                    drift < 20.0,
                    "an intensity step changed meaning: {drift}° of hue drift"
                );
                assert!(
                    distance_from_ground(alphabet, active) > distance_from_ground(alphabet, dim),
                    "the active step must be the louder one on {:?}",
                    alphabet.polarity
                );
            }
        }
    }

    /// Redundancy where noise is expensive: an alarm may never rest on
    /// hue alone, because the glance that misses it is the glance it was
    /// drawn for.
    #[test]
    fn alarms_are_carried_on_more_than_one_channel() {
        for alphabet in BOTH {
            for signal in alphabet
                .signals()
                .iter()
                .filter(|signal| signal.tier == Tier::Alarm)
            {
                assert!(
                    signal.channels.len() >= 2,
                    "an alarm rests on a single channel"
                );
                assert!(
                    signal.channels.contains(&Channel::Hue),
                    "alarms are the hue budget; one that spends none is miscategorised"
                );
            }
        }
    }

    /// Colour is an exception by construction: most of the alphabet is
    /// grey, and everything a calm screen draws is. The grey is BONE — a
    /// warm tint, the same on every rung — but a tint is not a hue: its
    /// chroma stays under the floor at which the eye would name a colour,
    /// and the hue it does carry is one warm hue for the whole ladder.
    #[test]
    fn the_resting_alphabet_is_only_tinted() {
        for alphabet in BOTH {
            for signal in alphabet
                .signals()
                .iter()
                .filter(|signal| signal.tier <= Tier::Content)
            {
                let color = signal.color;
                assert!(is_tint(color), "a resting symbol spent hue: {color:?}");
                if color.r() > 8 {
                    let h = hue_degrees(color);
                    assert!(
                        (20.0..=60.0).contains(&h),
                        "the bone tint drifted off warm: {color:?} at {h}°"
                    );
                }
            }
        }
        // the tint really is one tint: a hue, when there is one, is the same
        // for every rung of the dark ladder
        // (measured above the rungs where byte rounding makes hue noise)
        let hues: Vec<f32> = DARK
            .signals()
            .iter()
            .filter(|s| s.tier <= Tier::Content && s.color.r() > 40)
            .map(|s| hue_degrees(s.color))
            .collect();
        for pair in hues.windows(2) {
            assert!(
                (pair[0] - pair[1]).abs() < 12.0,
                "the ladder's warmth is uneven: {hues:?}"
            );
        }
    }

    #[test]
    fn dark_remains_the_default_projection() {
        assert_eq!(Polarity::default(), Polarity::Dark);
        assert_eq!(
            Alphabet::for_polarity(Polarity::Dark).ground.color,
            GROUND.color
        );
        assert_eq!(ALPHABET[5].color, FOCUS.color);
    }

    /// Both ladders are ratio ladders, so their steps are perceptually
    /// even rather than numerically even.
    #[test]
    fn the_scales_are_ratio_ladders() {
        for (ladder, ratio) in [
            (space::LADDER.as_slice(), space::RATIO),
            (type_scale::LADDER.as_slice(), type_scale::RATIO),
        ] {
            assert!(ladder.windows(2).all(|pair| pair[1] > pair[0]));
            for pair in ladder.windows(2) {
                let actual = pair[1] / pair[0];
                assert!(
                    (actual - ratio).abs() < 0.35,
                    "step {} -> {} is a ratio of {actual}, not {ratio}",
                    pair[0],
                    pair[1]
                );
            }
        }
    }

    /// The scale multiplier is the only place density is decided.
    #[test]
    fn the_scale_multiplier_moves_every_dimension_together() {
        assert_eq!(px(space::STEP), (space::STEP * SCALE).round());
        assert_eq!(px(type_scale::BODY), (type_scale::BODY * SCALE).round());
    }
}
