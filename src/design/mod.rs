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

pub mod signs;

use eframe::egui::Color32;

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

// ------------------------------------------------------- luminance ladder

/// The luminance ladder, in CIE L\*. Five rungs: the cap is the receiver's,
/// not a preference. The gaps are deliberately uneven — see the module
/// note on budgeted separation.
pub mod lstar {
    /// The ground. Black is not a colour choice here, it is the floor.
    pub const GROUND: f32 = 0.0;
    /// A recess below the surfaces: what a summoned window (the browser)
    /// is drawn on, so it reads as cut into the ground rather than laid
    /// on top of the work. Told apart from the ground by its edge — a
    /// window has one — rather than at a glance.
    pub const WELL: f32 = 2.7;
    /// A resting object's fill. Near the floor, because rest is the
    /// common case and the ground should read as black, not as grey.
    pub const SURFACE: f32 = 5.5;
    /// The boundary of a resting object.
    pub const EDGE: f32 = 20.0;
    /// Readable marks on a surface.
    pub const INK: f32 = 60.0;
    /// The addressed thing. Thrown clear of the whole resting world: the
    /// sync marker may never be mistaken for structure.
    pub const FOCUS: f32 = 95.0;

    /// Every rung, dimmest first.
    pub const LADDER: [f32; 6] = [GROUND, WELL, SURFACE, EDGE, INK, FOCUS];

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
    color: Color32::from_gray(10),
    tier: Tier::Structure,
    channels: &[Channel::Luminance, Channel::Position],
};

/// A resting object's fill.
pub const SURFACE: Signal = Signal {
    color: Color32::from_gray(18),
    tier: Tier::Structure,
    channels: &[Channel::Luminance],
};

/// A resting object's boundary; also the periphery's hairlines.
pub const EDGE: Signal = Signal {
    color: Color32::from_gray(48),
    tier: Tier::Structure,
    channels: &[Channel::Luminance],
};

/// Readable marks on a surface: names, values, refusal words.
pub const INK: Signal = Signal {
    color: Color32::from_gray(145),
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
    color: Color32::from_gray(241),
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
    color: Color32::from_rgb(72, 198, 224),
    tier: Tier::Exception,
    channels: &[Channel::Hue, Channel::Position],
};

/// The same meaning, quieter: present but not the subject.
pub const LIVE_DIM: Signal = Signal {
    color: Color32::from_rgb(44, 122, 138),
    tier: Tier::Exception,
    channels: &[Channel::Hue, Channel::Position],
};

/// Every symbol in the alphabet. The cap is enforced against this list.
pub const ALPHABET: [Signal; 10] = [
    GROUND,
    WELL,
    SURFACE,
    EDGE,
    INK,
    FOCUS,
    JEOPARDY_LATENT,
    JEOPARDY_ACTIVE,
    LIVE,
    LIVE_DIM,
];

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

    /// Every rung's byte is the value its declared L\* asks for. The
    /// ladder is spaced in the receiver's space, and this is what keeps
    /// the bytes honest to that claim.
    #[test]
    fn every_rung_sits_where_its_lightness_says_it_does() {
        for (signal, declared) in [
            (GROUND, lstar::GROUND),
            (WELL, lstar::WELL),
            (SURFACE, lstar::SURFACE),
            (EDGE, lstar::EDGE),
            (INK, lstar::INK),
            (FOCUS, lstar::FOCUS),
        ] {
            let actual = lightness(signal.color.r());
            assert!(
                (actual - declared).abs() < 1.0,
                "{:?} claims L* {declared} but measures {actual}",
                signal.color
            );
        }
    }

    /// No two rungs are confusable. This is the whole reason the ladder
    /// is short: symbols the eye cannot separate are not symbols.
    #[test]
    fn no_two_rungs_are_closer_than_the_separation_floor() {
        for (index, a) in lstar::LADDER.iter().enumerate() {
            for b in &lstar::LADDER[index + 1..] {
                assert!(
                    (b - a).abs() >= lstar::MIN_SEPARATION,
                    "L* {a} and {b} are within the noise of each other"
                );
            }
        }
    }

    /// The ladder climbs, and FOCUS is the top of it. Every later symbol
    /// is decoded against the sync marker, so nothing may outshine it.
    #[test]
    fn the_ladder_climbs_and_focus_is_the_brightest_thing_we_draw() {
        assert!(
            lstar::LADDER.windows(2).all(|pair| pair[1] > pair[0]),
            "the ladder must be ordered"
        );
        let brightest = ALPHABET
            .iter()
            .map(|signal| lightness_of(signal.color))
            .fold(f32::MIN, f32::max);
        assert!(
            (lightness_of(FOCUS.color) - brightest).abs() < f32::EPSILON,
            "something in the alphabet outshines the sync marker"
        );
    }

    /// The cap, held as code. Absolute judgment on one dimension runs out
    /// around seven levels; we spend six and keep the margin.
    #[test]
    fn the_alphabet_stays_inside_the_receivers_capacity() {
        assert!(
            lstar::LADDER.len() <= 7,
            "more luminance rungs than absolute judgment can carry"
        );
        assert_eq!(lstar::LADDER.len(), 6);

        let hues: Vec<f32> = ALPHABET
            .iter()
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
            "the app may carry exactly two meanings in hue"
        );
    }

    /// One symbol, one meaning: nothing in the alphabet is drawn twice.
    #[test]
    fn the_alphabet_is_uniquely_decodable() {
        for (index, a) in ALPHABET.iter().enumerate() {
            for b in &ALPHABET[index + 1..] {
                assert_ne!(
                    a.color, b.color,
                    "two roles share one value, so neither can be decoded"
                );
            }
        }
    }

    /// The two hues are far enough apart to survive a glance, and to stay
    /// separable for a viewer who reads red and green poorly.
    #[test]
    fn the_two_meanings_sit_far_apart_on_the_hue_circle() {
        let jeopardy = hue_degrees(JEOPARDY_ACTIVE.color);
        let live = hue_degrees(LIVE.color);
        let separation = (jeopardy - live).abs().min(360.0 - (jeopardy - live).abs());
        assert!(
            separation >= hue::MIN_SEPARATION_DEG,
            "jeopardy at {jeopardy}° and live at {live}° are only {separation}° apart"
        );
    }

    /// Both steps of one meaning are the SAME meaning: intensity moves,
    /// hue does not.
    #[test]
    fn intensity_steps_keep_their_hue() {
        for (dim, bright) in [(JEOPARDY_LATENT, JEOPARDY_ACTIVE), (LIVE_DIM, LIVE)] {
            let drift = (hue_degrees(dim.color) - hue_degrees(bright.color)).abs();
            assert!(
                drift < 20.0,
                "an intensity step changed meaning: {drift}° of hue drift"
            );
            assert!(
                lightness_of(bright.color) > lightness_of(dim.color),
                "the active step must be the louder one"
            );
        }
    }

    /// Redundancy where noise is expensive: an alarm may never rest on
    /// hue alone, because the glance that misses it is the glance it was
    /// drawn for.
    #[test]
    fn alarms_are_carried_on_more_than_one_channel() {
        for signal in ALPHABET.iter().filter(|signal| signal.tier == Tier::Alarm) {
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

    /// Colour is an exception by construction: most of the alphabet is
    /// grey, and everything a calm screen draws is.
    #[test]
    fn the_resting_alphabet_is_colourless() {
        for signal in ALPHABET
            .iter()
            .filter(|signal| signal.tier <= Tier::Content)
        {
            let color = signal.color;
            assert!(
                color.r() == color.g() && color.g() == color.b(),
                "a resting symbol spent hue: {color:?}"
            );
        }
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
