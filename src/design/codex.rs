//! The codex: every sign the deck draws, and what it means.
//!
//! The vocabulary is AUTHORED and CLOSED. A sign enters through this
//! table with a name and three meanings — one per hand that touched the
//! machine — and a test holds the table to one sign, one meaning, and to
//! `notes/20260902-codex.md`, which explains each sign in prose. Nothing
//! on the stage draws a sigil that is not here.
//!
//! # The three hands
//!
//! - **Machine.** The deck's own layer: circuit traces and pads. What the
//!   sign is, electrically, to whoever built it.
//! - **Talisman.** The old humans read the traces as a Daoist register:
//!   tracks are generals, clips are registers, devices are seals, the
//!   transport is the Dipper. What the sign is to them.
//! - **Annotation.** A later, Rosicrucian hand, writing in the margins.
//!   What the sign is to someone trying to work the deck from notes.
//!
//! # Drawing
//!
//! A sign is polylines on a seven-by-seven lattice, moving only
//! orthogonally or at forty-five degrees — the board's own geometry — plus
//! pads. Written as text (`"0,0 6,0 | 3,3"`: two segments, the second a
//! pad) so a sign can be read in the table and drawn on paper.

use eframe::egui::{Color32, Pos2, Rect, Shape, pos2, vec2};

use super::glyph::{self, Glyph};
use super::kit::Weight;

/// The lattice's last coordinate. Signs are authored on 0..=6.
const LAST: u8 = 6;

/// A parameter's family: what kind of thing a control is, before its name.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ParamFamily {
    /// Pitch and time: rates, delays, tunings.
    Time,
    /// Level and amount: gains, mixes, depths.
    Level,
    /// Shape and choice: waves, modes, slopes.
    Shape,
    /// Modulation and the bipolar: wires, drive, anything that rewrites.
    Modulation,
}

impl ParamFamily {
    pub const ALL: [ParamFamily; 4] = [
        ParamFamily::Time,
        ParamFamily::Level,
        ParamFamily::Shape,
        ParamFamily::Modulation,
    ];

    pub const fn word(self) -> &'static str {
        match self {
            ParamFamily::Time => "TIME",
            ParamFamily::Level => "LEVEL",
            ParamFamily::Shape => "SHAPE",
            ParamFamily::Modulation => "MODULATION",
        }
    }
}

/// One sign.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Sign {
    /// A track's slot, 0..32: the general who holds that column.
    General(u8),
    /// A scene's row, 0..16: the register that row is written in.
    Register(u8),
    /// A scale degree, 0..7.
    Degree(u8),
    /// A parameter family.
    Family(ParamFamily),
    /// A device family's seal: its mark, sealed.
    Seal(Glyph),
    /// A codex numeral, 0..10.
    Numeral(u8),
    /// The clock: the Dipper, which the transport paces.
    Dipper,
    Play,
    Stop,
    Record,
    /// The master: Malkuth, where every bus arrives.
    Master,
    Mute,
    Solo,
    /// The engine: the maker's seal.
    Engine,
    /// A dropped block.
    Xrun,
    /// The browser.
    Archive,
    /// The help page.
    Codex,
}

/// What a sign means, to each hand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Layers {
    pub machine: &'static str,
    pub talisman: &'static str,
    pub annotation: &'static str,
}

/// An authored entry: a name, the strokes, the three meanings.
struct Entry {
    name: &'static str,
    strokes: &'static str,
    machine: &'static str,
    talisman: &'static str,
    annotation: &'static str,
}

const fn e(
    name: &'static str,
    strokes: &'static str,
    machine: &'static str,
    talisman: &'static str,
    annotation: &'static str,
) -> Entry {
    Entry {
        name,
        strokes,
        machine,
        talisman,
        annotation,
    }
}

/// The thirty-two generals, one per track slot.
const GENERALS: [Entry; 32] = [
    e(
        "GATE",
        "1,0 5,0 6,1 6,5 5,6 1,6 0,5 0,1 1,0 | 3,1 3,5",
        "bus 01 input gate",
        "the general who opens the gate",
        "the first column; all sound enters here",
    ),
    e(
        "TWIN",
        "0,0 0,6 | 6,0 6,6 | 0,3 6,3",
        "two rails bridged",
        "the twin generals who hold one door",
        "second column, paired with the first",
    ),
    e(
        "SPINE",
        "3,0 3,6 | 1,1 5,1 | 1,3 5,3 | 1,5 5,5",
        "a bus with three taps",
        "the general of the spine",
        "third column; the backbone of a song",
    ),
    e(
        "SHIELD",
        "1,0 5,0 5,4 3,6 1,4 1,0",
        "a shielded lane",
        "the general who bears the shield",
        "fourth column; guards the low end",
    ),
    e(
        "STAIR",
        "0,6 0,3 3,3 3,0 6,0",
        "a stepped trace",
        "the general who climbs",
        "fifth column; rising figures",
    ),
    e(
        "FORK",
        "3,6 3,3 | 3,3 0,0 | 3,3 6,0",
        "a trace that forks",
        "the general who divides the host",
        "sixth column; splits the signal",
    ),
    e(
        "EYE",
        "0,3 2,1 4,1 6,3 4,5 2,5 0,3 | 3,3",
        "a sense pad",
        "the general who watches",
        "seventh column; the deck's eye",
    ),
    e(
        "TOWER",
        "2,6 2,1 3,0 4,1 4,6 | 1,6 5,6",
        "a vertical riser",
        "the general of the tower",
        "eighth column; the highest voice",
    ),
    e(
        "RIVER",
        "0,0 2,2 0,4 2,6 | 4,0 6,2 4,4 6,6",
        "two meandering traces",
        "the general of the river",
        "ninth column; flowing parts",
    ),
    e(
        "KNOT",
        "0,0 6,6 | 6,0 0,6 | 3,0 3,6",
        "three traces crossing",
        "the general who ties the knot",
        "tenth column; where lines meet",
    ),
    e(
        "ANVIL",
        "0,2 6,2 | 1,2 1,5 5,5 5,2 | 3,5 3,6",
        "a heavy block on a rail",
        "the general of the anvil",
        "eleventh column; the strike",
    ),
    e(
        "CROWN",
        "0,6 0,2 1,3 2,2 3,3 4,2 5,3 6,2 6,6 0,6",
        "a crested pad",
        "the general who wears the crown",
        "twelfth column; the lead",
    ),
    e(
        "BOW",
        "0,0 3,3 0,6 | 3,3 6,3",
        "a bent trace with a tap",
        "the general who draws the bow",
        "thirteenth column; tension",
    ),
    e(
        "WELL",
        "1,1 5,1 5,5 1,5 1,1 | 0,0 1,1 | 6,0 5,1 | 0,6 1,5 | 6,6 5,5",
        "a tied pad",
        "the general of the well",
        "fourteenth column; depth",
    ),
    e(
        "COMB",
        "0,0 6,0 | 1,0 1,4 | 3,0 3,6 | 5,0 5,4",
        "a header with pins",
        "the general of the comb",
        "fifteenth column; many teeth",
    ),
    e(
        "SEAL",
        "1,1 5,1 5,5 1,5 1,1 | 2,2 4,2 4,4 2,4 2,2 | 3,3",
        "a via in a diamond",
        "the general who keeps the seal",
        "sixteenth column; closed",
    ),
    e(
        "COMPASS",
        "0,3 6,3 | 3,0 3,6 | 1,1 5,5",
        "a crossing with a bias",
        "the general of the compass",
        "seventeenth column; direction",
    ),
    e(
        "ARCH",
        "0,6 0,2 2,0 4,0 6,2 6,6 | 2,6 2,4 4,4 4,6",
        "an arched shield",
        "the general of the arch",
        "eighteenth column; the doorway",
    ),
    e(
        "SCALES",
        "3,0 3,6 | 0,2 6,2 | 0,2 1,3 | 6,2 5,3",
        "a balanced tap",
        "the general who weighs",
        "nineteenth column; balance",
    ),
    e(
        "SERPENT",
        "0,1 1,0 2,1 3,2 4,3 5,2 6,1 | 0,5 1,4 2,5 3,6 4,5 5,4 6,5",
        "two undulating traces",
        "the general of the serpent",
        "twentieth column; the coil",
    ),
    e(
        "ALTAR",
        "0,6 6,6 | 1,6 1,3 5,3 5,6 | 3,3 3,0 | 2,1 4,1",
        "a raised pad",
        "the general of the altar",
        "twenty-first column; the offering",
    ),
    e(
        "MIRROR",
        "0,1 3,4 6,1 | 0,5 3,2 6,5",
        "two traces meeting at a via",
        "the general who mirrors",
        "twenty-second column; reflection",
    ),
    e(
        "CAGE",
        "0,0 6,0 6,6 0,6 0,0 | 2,0 2,6 | 4,0 4,6",
        "a shielded pair",
        "the general of the cage",
        "twenty-third column; held",
    ),
    e(
        "SPIRAL",
        "6,0 0,0 0,6 6,6 6,2 2,2 2,4 4,4",
        "an inductor",
        "the general of the thunder spiral",
        "twenty-fourth column; turning",
    ),
    e(
        "TRIAD",
        "1,1 5,1 3,3 1,1 | 3,5",
        "a three-pad net",
        "the general of the triad",
        "twenty-fifth column; three",
    ),
    e(
        "BELL",
        "2,0 4,0 4,4 6,6 0,6 2,4 2,0",
        "a flared lane",
        "the general who rings",
        "twenty-sixth column; the bell",
    ),
    e(
        "CHAIN",
        "0,3 1,2 2,3 1,4 0,3 | 2,3 4,3 | 4,3 5,2 6,3 5,4 4,3",
        "two vias linked",
        "the general of the chain",
        "twenty-seventh column; linked",
    ),
    e(
        "STAR",
        "3,0 3,6 | 0,3 6,3 | 1,1 5,5 | 5,1 1,5",
        "an eight-way junction",
        "the general of the star",
        "twenty-eighth column; radiance",
    ),
    e(
        "HALF",
        "0,0 0,6 | 0,3 3,0 | 0,3 3,6",
        "a rail with a split",
        "the general of the half",
        "twenty-ninth column; one side",
    ),
    e(
        "VESSEL",
        "0,0 1,1 1,5 2,6 4,6 5,5 5,1 6,0 | 2,3 4,3",
        "a cup-shaped shield",
        "the general of the vessel",
        "thirtieth column; holds",
    ),
    e(
        "KEY",
        "0,0 2,2 4,0 6,2 | 3,3 3,6 | 1,6 5,6",
        "a keyed header",
        "the general who keeps the key",
        "thirty-first column; unlocks",
    ),
    e(
        "GRID",
        "0,0 6,0 | 0,6 6,6 | 3,0 3,6 | 1,3 5,3",
        "a full grid",
        "the general of the last gate",
        "thirty-second column; the end",
    ),
];

/// The sixteen registers, one per scene row.
const REGISTERS: [Entry; 16] = [
    e(
        "ONE RULE",
        "0,3 6,3 | 3,1 3,5",
        "a rail tapped once",
        "the register of the first rule",
        "row one; begin here",
    ),
    e(
        "TWO RULES",
        "0,2 6,2 | 0,4 6,4",
        "two rails",
        "the register of two rules",
        "row two",
    ),
    e(
        "THREE RULES",
        "0,1 6,1 | 0,3 6,3 | 0,5 6,5",
        "three rails",
        "the register of three rules",
        "row three",
    ),
    e(
        "RISING",
        "0,3 3,0 6,3",
        "a trace bent up",
        "the rising register",
        "row four; lifts",
    ),
    e(
        "FALLING",
        "0,3 3,6 6,3",
        "a trace bent down",
        "the falling register",
        "row five; settles",
    ),
    e(
        "DIAMOND",
        "0,3 3,0 6,3 3,6 0,3",
        "a closed loop",
        "the diamond register",
        "row six; complete",
    ),
    e(
        "CROSSED",
        "0,0 6,6 | 0,6 6,0",
        "two traces crossing",
        "the crossed register",
        "row seven; opposed",
    ),
    e(
        "SQUARE",
        "1,1 5,1 5,5 1,5 1,1 | 0,0 1,1 | 6,6 5,5",
        "a tied square",
        "the square register",
        "row eight; set",
    ),
    e(
        "THREE PADS",
        "0,3 6,3 | 1,3 | 3,3 | 5,3",
        "a rail with three pads",
        "the register of three pads",
        "row nine",
    ),
    e(
        "PEAK",
        "0,5 3,2 6,5 0,5",
        "a triangular net",
        "the peak register",
        "row ten; climax",
    ),
    e(
        "CROSS PAD",
        "3,0 3,6 | 0,3 6,3 | 1,1 | 5,1 | 1,5 | 5,5",
        "a crossing with a via",
        "the cross register",
        "row eleven; centre",
    ),
    e(
        "STANDING",
        "0,1 6,1 | 3,1 3,6 | 1,6 5,6",
        "a riser on a base",
        "the standing register",
        "row twelve; upright",
    ),
    e(
        "BROKEN",
        "0,3 2,1 4,3 6,1",
        "a jogged trace",
        "the broken register",
        "row thirteen; interrupted",
    ),
    e(
        "HALVED",
        "0,0 6,0 6,6 0,6 0,0 | 3,0 3,6",
        "a shield split",
        "the halved register",
        "row fourteen; two parts",
    ),
    e(
        "ARROW",
        "0,6 3,3 6,6 | 3,3 3,0 | 1,0 5,0",
        "a pointed trace",
        "the arrow register",
        "row fifteen; onward",
    ),
    e(
        "OCTAGON",
        "1,0 5,0 6,1 6,5 5,6 1,6 0,5 0,1 1,0",
        "a closed shield",
        "the last register",
        "row sixteen; sealed",
    ),
];

/// The seven degrees.
const DEGREES: [Entry; 7] = [
    e(
        "ROOT",
        "3,6 3,0 | 1,6 5,6",
        "a riser on ground",
        "the root",
        "the first degree; home",
    ),
    e(
        "STEP",
        "0,6 3,6 3,3 6,3",
        "a step up",
        "the step",
        "the second degree; away",
    ),
    e(
        "THIRD EYE",
        "0,3 3,0 6,3 3,6 0,3 | 1,3 5,3 | 3,3",
        "a diamond lens crossed by a bus and via",
        "the third eye",
        "the third degree; colour",
    ),
    e(
        "FOUNDATION",
        "0,6 6,6 | 0,4 6,4 | 3,4 3,0",
        "a double base with a riser",
        "the foundation",
        "the fourth degree; beneath",
    ),
    e(
        "PILLAR",
        "3,0 3,6 | 0,0 6,0 | 0,6 6,6",
        "a stile between two rails",
        "the pillar",
        "the fifth degree; holds",
    ),
    e(
        "SHADOW",
        "0,3 3,0 6,3 | 3,3 3,6",
        "a roof with a drop",
        "the shadow",
        "the sixth degree; behind",
    ),
    e(
        "THRESHOLD",
        "1,0 5,0 5,6 1,6 | 3,2 3,4",
        "an open gate",
        "the threshold",
        "the seventh degree; almost home",
    ),
];

/// The four parameter families.
const FAMILIES: [Entry; 4] = [
    e(
        "TIME",
        "0,0 6,0 | 6,0 0,6 | 0,6 6,6",
        "an hourglass trace",
        "the hour",
        "controls of rate, delay and tuning",
    ),
    e(
        "LEVEL",
        "0,6 6,6 | 0,4 4,4 | 0,2 2,2",
        "a stepped rail",
        "the measure",
        "controls of gain, mix and depth",
    ),
    e(
        "SHAPE",
        "1,1 5,1 5,5 1,5 1,1 | 3,3 | 3,0 3,1 | 3,5 3,6",
        "a diamond pad",
        "the form",
        "controls of wave, mode and slope",
    ),
    e(
        "MODULATION",
        "0,3 1,2 2,3 3,4 4,3 5,2 6,3",
        "a wave trace",
        "the breath",
        "controls that rewrite another",
    ),
];

/// The ten numerals.
const NUMERALS: [Entry; 10] = [
    e(
        "NOUGHT",
        "3,1 5,3 3,5 1,3 3,1",
        "an empty loop",
        "the tally of nothing",
        "zero",
    ),
    e("ONE", "3,0 3,6", "one riser", "the tally of one", "one"),
    e(
        "TWO",
        "2,0 2,6 | 4,0 4,6",
        "two risers",
        "the tally of two",
        "two",
    ),
    e(
        "THREE",
        "1,0 1,6 | 3,0 3,6 | 5,0 5,6",
        "three risers",
        "the tally of three",
        "three",
    ),
    e(
        "FOUR",
        "0,0 0,6 | 2,0 2,6 | 4,0 4,6 | 6,0 6,6",
        "four risers",
        "the tally of four",
        "four",
    ),
    e(
        "FIVE",
        "1,0 1,6 | 3,0 3,6 | 5,0 5,6 | 0,3 6,3 | 0,0 6,0",
        "three risers barred",
        "the tally of five",
        "five",
    ),
    e(
        "SIX",
        "1,1 5,1 5,5 1,5 1,1 | 1,1 5,5",
        "a square crossed",
        "the tally of six",
        "six",
    ),
    e(
        "SEVEN",
        "0,6 3,3 6,6",
        "a peak",
        "the tally of seven",
        "seven",
    ),
    e(
        "EIGHT",
        "0,6 3,3 6,6 0,6",
        "a closed peak",
        "the tally of eight",
        "eight",
    ),
    e(
        "NINE",
        "0,6 3,3 6,6 0,6 | 3,0 3,3 | 1,0 | 5,0",
        "a closed peak with a mast",
        "the tally of nine",
        "nine",
    ),
];

const DIPPER: Entry = e(
    "DIPPER",
    "6,0 5,1 4,1 3,2 3,5 0,5 0,2 3,2 | 6,0 | 5,1 | 4,1 | 3,2 | 3,5 | 0,5 | 0,2",
    "seven pads on a bent trace",
    "the Dipper the transport paces",
    "the clock",
);
const PLAY: Entry = e(
    "PLAY",
    "0,0 3,3 0,6 0,0",
    "a trace pointing on",
    "the general's advance",
    "play",
);
const STOP: Entry = e(
    "STOP",
    "0,0 6,0 6,6 0,6 0,0",
    "a hard perimeter",
    "the halt",
    "stop",
);
const RECORD: Entry = e(
    "RECORD",
    "2,0 4,0 6,2 6,4 4,6 2,6 0,4 0,2 2,0 | 3,3",
    "an octagon with a via",
    "the red seal",
    "record; something is at stake",
);
const MASTER: Entry = e(
    "MASTER",
    "1,0 5,0 6,1 6,5 5,6 1,6 0,5 0,1 1,0 | 3,0 3,6 | 0,3 6,3 | 0,0 | 6,0 | 0,6 | 6,6",
    "the sum pad",
    "Malkuth, where every bus arrives",
    "the master; all sound leaves here",
);
const MUTE: Entry = e(
    "MUTE",
    "0,0 6,0 6,6 0,6 0,0 | 0,0 6,6 | 6,0 0,6",
    "a shield crossed out",
    "the sealed mouth",
    "mute",
);
const SOLO: Entry = e(
    "SOLO",
    "0,0 6,0 6,6 0,6 0,0 | 3,3 | 1,3 2,3 | 4,3 5,3",
    "a shield within a shield",
    "the single eye",
    "solo",
);
const ENGINE: Entry = e(
    "ENGINE",
    "0,3 2,3 | 2,1 4,1 4,5 2,5 2,1 | 4,3 6,3",
    "a component on a rail",
    "the maker's seal",
    "the engine; the deck's heart",
);
const XRUN: Entry = e(
    "XRUN",
    "0,0 6,0 3,3 0,0 | 3,4 3,5 | 3,6",
    "a warning net",
    "a dropped offering",
    "a block was dropped and probably heard",
);
const ARCHIVE: Entry = e(
    "ARCHIVE",
    "0,1 6,1 | 0,3 6,3 | 0,5 6,5 | 0,0 0,6",
    "shelved rails",
    "the archive of seals",
    "the browser",
);
const CODEX: Entry = e(
    "CODEX",
    "1,0 5,0 5,6 1,6 1,0 | 2,2 4,2 | 2,4 4,4",
    "a bound pad",
    "the codex itself",
    "the help page",
);

impl Sign {
    /// Every sign, in codex order.
    pub fn every() -> Vec<Sign> {
        let mut all = Vec::with_capacity(96);
        all.extend((0..32).map(Sign::General));
        all.extend((0..16).map(Sign::Register));
        all.extend((0..7).map(Sign::Degree));
        all.extend(ParamFamily::ALL.map(Sign::Family));
        all.extend(Glyph::ALL.map(Sign::Seal));
        all.extend((0..10).map(Sign::Numeral));
        all.extend([
            Sign::Dipper,
            Sign::Play,
            Sign::Stop,
            Sign::Record,
            Sign::Master,
            Sign::Mute,
            Sign::Solo,
            Sign::Engine,
            Sign::Xrun,
            Sign::Archive,
            Sign::Codex,
        ]);
        all
    }

    fn entry(self) -> Option<&'static Entry> {
        Some(match self {
            Sign::General(n) => GENERALS.get(n as usize)?,
            Sign::Register(n) => REGISTERS.get(n as usize)?,
            Sign::Degree(n) => DEGREES.get(n as usize)?,
            Sign::Family(f) => &FAMILIES[ParamFamily::ALL.iter().position(|x| *x == f)?],
            Sign::Numeral(n) => NUMERALS.get(n as usize)?,
            Sign::Seal(_) => return None,
            Sign::Dipper => &DIPPER,
            Sign::Play => &PLAY,
            Sign::Stop => &STOP,
            Sign::Record => &RECORD,
            Sign::Master => &MASTER,
            Sign::Mute => &MUTE,
            Sign::Solo => &SOLO,
            Sign::Engine => &ENGINE,
            Sign::Xrun => &XRUN,
            Sign::Archive => &ARCHIVE,
            Sign::Codex => &CODEX,
        })
    }

    /// The sign's name, as the codex heads it.
    pub fn name(self) -> String {
        match self {
            Sign::General(n) => format!(
                "GENERAL {:02} · {}",
                n + 1,
                GENERALS.get(n as usize).map(|e| e.name).unwrap_or("?")
            ),
            Sign::Register(n) => format!(
                "REGISTER {:02} · {}",
                n + 1,
                REGISTERS.get(n as usize).map(|e| e.name).unwrap_or("?")
            ),
            Sign::Degree(n) => format!(
                "DEGREE {} · {}",
                n + 1,
                DEGREES.get(n as usize).map(|e| e.name).unwrap_or("?")
            ),
            Sign::Family(f) => format!("FAMILY · {}", f.word()),
            Sign::Seal(g) => format!("SEAL · {}", g.name()),
            Sign::Numeral(n) => format!(
                "NUMERAL {} · {}",
                n,
                NUMERALS.get(n as usize).map(|e| e.name).unwrap_or("?")
            ),
            _ => self.entry().map(|e| e.name.to_owned()).unwrap_or_default(),
        }
    }

    /// What the sign means to each of the three hands.
    pub fn layers(self) -> Layers {
        match self {
            Sign::Seal(g) => Layers {
                machine: match g {
                    Glyph::Dynamics => "a knee in a transfer trace",
                    Glyph::Filter => "a bell-shaped response",
                    Glyph::Time => "three decaying taps",
                    Glyph::Drive => "a wave with its peaks clipped",
                    Glyph::Modulation => "a whole wave",
                    Glyph::Spectral => "bins of unequal height",
                    Glyph::Utility => "a line through, one leaving",
                    Glyph::Instrument => "a stem and a head",
                    Glyph::Stack => "three stacked rules",
                    Glyph::Saw => "a sawtooth",
                    Glyph::Transient => "a strike and its decay",
                    Glyph::Sample => "a burst about a centre line",
                },
                talisman: match g {
                    Glyph::Dynamics => "the seal that bends the loud",
                    Glyph::Filter => "the seal of the veil",
                    Glyph::Time => "the seal of echoes",
                    Glyph::Drive => "the seal of fire",
                    Glyph::Modulation => "the seal of breath",
                    Glyph::Spectral => "the seal of the prism",
                    Glyph::Utility => "the seal of the crossroads",
                    Glyph::Instrument => "the seal of the voice",
                    Glyph::Stack => "the seal of the stack",
                    Glyph::Saw => "the seal of the blade",
                    Glyph::Transient => "the seal of the strike",
                    Glyph::Sample => "the seal of the captured voice",
                },
                annotation: match g {
                    Glyph::Dynamics => "compressors, limiters, gates",
                    Glyph::Filter => "filters and equalisers",
                    Glyph::Time => "delays and reverbs",
                    Glyph::Drive => "distortion and saturation",
                    Glyph::Modulation => "chorus, phaser, flanger",
                    Glyph::Spectral => "spectral processors",
                    Glyph::Utility => "utilities and routing",
                    Glyph::Instrument => "the instruments section",
                    Glyph::Stack => "a container of families",
                    Glyph::Saw => "synthesisers",
                    Glyph::Transient => "drums",
                    Glyph::Sample => "samplers",
                },
            },
            _ => {
                let e = self.entry().expect("every non-seal sign is authored");
                Layers {
                    machine: e.machine,
                    talisman: e.talisman,
                    annotation: e.annotation,
                }
            }
        }
    }

    /// The pads a sign carries, on the unit square.
    pub fn pads(self) -> Vec<(f32, f32)> {
        let Some(entry) = self.entry() else {
            return vec![];
        };
        parse(entry.strokes)
            .into_iter()
            .filter(|run| run.len() == 1)
            .map(|run| run[0])
            .collect()
    }

    /// The sign's polylines on the unit square, `(0,0)` top-left.
    pub fn strokes(self) -> Vec<Vec<(f32, f32)>> {
        match self {
            Sign::Seal(g) => {
                // the family mark, inset inside its seal ring
                glyph::strokes(g)
                    .into_iter()
                    .map(|run| {
                        run.into_iter()
                            .map(|(x, y)| (0.2 + x * 0.6, 0.2 + y * 0.6))
                            .collect()
                    })
                    .collect()
            }
            _ => parse(self.entry().expect("authored").strokes)
                .into_iter()
                .filter(|run| run.len() >= 2)
                .collect(),
        }
    }

    /// Draw the sign into `cell`, squared and centred.
    pub fn paint(self, out: &mut Vec<Shape>, cell: Rect, weight: Weight, ink: Color32) {
        let side = cell.width().min(cell.height());
        if side < 3.0 {
            return;
        }
        let pad = (side / 7.0).clamp(1.5, 4.0);
        // inset so a pad on the lattice edge and a stroke's own width
        // both land inside the cell
        let b = Rect::from_center_size(cell.center(), vec2(side, side))
            .shrink(pad.max(weight.px()) * 0.5);
        let side = b.width();
        let at = |(x, y): (f32, f32)| pos2(b.left() + x * side, b.top() + y * side);
        let stroke = eframe::egui::Stroke::new(weight.px(), ink);
        if let Sign::Seal(_) = self {
            // the ring that makes a mark a seal
            out.push(Shape::circle_stroke(b.center(), side * 0.48, stroke));
        }
        for run in self.strokes() {
            let pts: Vec<Pos2> = run.into_iter().map(at).collect();
            out.push(Shape::line(pts, stroke));
        }
        for p in self.pads() {
            let c = at(p);
            out.push(Shape::rect_filled(
                Rect::from_center_size(c, vec2(pad, pad)),
                0.0,
                ink,
            ));
        }
    }

    /// Draw through the mesh cache.
    pub fn painted(
        self,
        painter: &eframe::egui::Painter,
        id: eframe::egui::Id,
        cell: Rect,
        weight: Weight,
        ink: Color32,
    ) {
        super::kit::cached(painter, id, cell, (self, weight, ink), |out| {
            self.paint(out, cell, weight, ink);
        });
    }
}

/// `"0,0 6,0 | 3,3"` → runs of points on the unit square.
fn parse(text: &str) -> Vec<Vec<(f32, f32)>> {
    text.split('|')
        .map(|run| {
            run.split_whitespace()
                .filter_map(|p| {
                    let (x, y) = p.split_once(',')?;
                    let x: u8 = x.parse().ok()?;
                    let y: u8 = y.parse().ok()?;
                    Some((x as f32 / LAST as f32, y as f32 / LAST as f32))
                })
                .collect()
        })
        .filter(|run: &Vec<(f32, f32)>| !run.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lattice(text: &str) -> Vec<Vec<(i32, i32)>> {
        text.split('|')
            .map(|run| {
                run.split_whitespace()
                    .map(|p| {
                        let (x, y) = p.split_once(',').expect("x,y");
                        (x.parse().expect("x"), y.parse().expect("y"))
                    })
                    .collect()
            })
            .collect()
    }

    fn authored() -> Vec<(Sign, &'static Entry)> {
        Sign::every()
            .into_iter()
            .filter_map(|s| s.entry().map(|e| (s, e)))
            .collect()
    }

    #[test]
    fn every_sign_moves_only_along_the_board() {
        for (sign, entry) in authored() {
            for run in lattice(entry.strokes) {
                for p in &run {
                    assert!(
                        p.0 <= LAST as i32 && p.1 <= LAST as i32,
                        "{sign:?} leaves the lattice at {p:?}"
                    );
                }
                for w in run.windows(2) {
                    let dx = (w[1].0 - w[0].0).abs();
                    let dy = (w[1].1 - w[0].1).abs();
                    assert!(
                        dx == 0 || dy == 0 || dx == dy,
                        "{sign:?} bends off the board between {:?} and {:?}",
                        w[0],
                        w[1]
                    );
                    assert!(dx + dy > 0, "{sign:?} repeats a point");
                }
            }
        }
    }

    #[test]
    fn every_sign_has_one_name_and_one_meaning() {
        let all = Sign::every();
        let mut names = std::collections::HashSet::new();
        let mut meanings = std::collections::HashSet::new();
        for s in &all {
            let name = s.name();
            assert!(!name.is_empty(), "{s:?} has no name");
            assert!(names.insert(name.clone()), "{name} names two signs");
            let l = s.layers();
            for m in [l.machine, l.talisman, l.annotation] {
                assert!(!m.is_empty(), "{name} has an empty meaning");
                assert!(
                    meanings.insert(m),
                    "{name}: '{m}' is already another sign's meaning"
                );
            }
        }
    }

    /// Rasterised onto the lattice, no two signs are the same picture.
    #[test]
    fn no_two_signs_draw_the_same_strokes() {
        const N: usize = 7;
        let raster = |s: Sign| {
            let mut grid = [false; N * N];
            let mark = |grid: &mut [bool; N * N], x: f32, y: f32| {
                let cx = ((x * (N - 1) as f32).round() as usize).min(N - 1);
                let cy = ((y * (N - 1) as f32).round() as usize).min(N - 1);
                grid[cy * N + cx] = true;
            };
            for run in s.strokes() {
                for pair in run.windows(2) {
                    for step in 0..=24 {
                        let t = step as f32 / 24.0;
                        mark(
                            &mut grid,
                            pair[0].0 + (pair[1].0 - pair[0].0) * t,
                            pair[0].1 + (pair[1].1 - pair[0].1) * t,
                        );
                    }
                }
            }
            for (x, y) in s.pads() {
                mark(&mut grid, x, y);
            }
            grid
        };
        let grids: Vec<_> = Sign::every().into_iter().map(|s| (s, raster(s))).collect();
        let mut close = Vec::new();
        for (i, (a, ga)) in grids.iter().enumerate() {
            assert!(
                ga.iter().filter(|c| **c).count() >= 3,
                "{} draws almost nothing",
                a.name()
            );
            for (b, gb) in grids.iter().skip(i + 1) {
                if matches!((a, b), (Sign::Seal(_), Sign::Seal(_))) {
                    // the family marks are held apart at their own, finer
                    // resolution in `design::glyph`
                    continue;
                }
                let differ = ga.iter().zip(gb).filter(|(x, y)| x != y).count();
                if differ < 4 {
                    close.push(format!(
                        "{} and {} differ in {differ} cells",
                        a.name(),
                        b.name()
                    ));
                }
            }
        }
        assert!(close.is_empty(), "signs the eye would confuse:\n{close:#?}");
    }

    #[test]
    fn every_sign_stays_inside_its_cell() {
        let cell = Rect::from_min_size(pos2(10.0, 10.0), vec2(20.0, 14.0));
        for s in Sign::every() {
            let mut out = Vec::new();
            s.paint(&mut out, cell, Weight::Hair, Color32::WHITE);
            assert!(!out.is_empty(), "{} drew nothing", s.name());
            for shape in &out {
                for p in crate::design::kit::points_of(shape) {
                    assert!(cell.expand(0.5).contains(p), "{} reaches {p:?}", s.name());
                }
            }
        }
        let mut out = Vec::new();
        Sign::Master.paint(
            &mut out,
            Rect::from_min_size(pos2(0.0, 0.0), vec2(2.0, 2.0)),
            Weight::Hair,
            Color32::WHITE,
        );
        assert!(out.is_empty(), "a cell too small to read still drew");
    }

    /// The prose codex explains every sign the code can draw.
    #[test]
    fn the_codex_names_every_sign() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/notes/20260902-codex.md");
        let text = std::fs::read_to_string(path).expect("the codex note exists");
        let mut missing = Vec::new();
        for s in Sign::every() {
            let heading = format!("## {}", s.name());
            if !text.contains(&heading) {
                missing.push(heading);
            }
        }
        assert!(
            missing.is_empty(),
            "the codex has no entry for:\n{missing:#?}"
        );
    }

    #[test]
    fn the_same_sign_draws_the_same_shapes() {
        let draw = || {
            let mut out = Vec::new();
            for s in Sign::every() {
                s.paint(
                    &mut out,
                    Rect::from_min_size(pos2(0.0, 0.0), vec2(30.0, 30.0)),
                    Weight::Heavy,
                    Color32::WHITE,
                );
            }
            format!("{out:?}")
        };
        assert_eq!(draw(), draw());
    }
}
