//! Pitch: the substrate, its striations, and nothing else.
//!
//! The ground truth of pitch is frequency on a log axis; scales, keys and
//! modes are REMOVABLE overlays that change how a pitch is addressed,
//! never how it sounds. Deviation from a striation is data, carried in an
//! explicit cents offset, never corrected away. No theory vocabulary
//! lives here — note names, numerals and sargam are lenses, and lenses
//! are UI. Contract: `notes/20260831-pitch-lens-spec.md`, striation
//! detail in `notes/20260831-key-scale-brief.md`.
//!
//! Pure, green-zone, total: nothing in this module panics on any input.
//! Garbage refuses at construction — `resolve`, `quantize_to` and `free`
//! are defined over every value that can exist.

#![deny(clippy::unwrap_used, clippy::expect_used)]

pub const A4_HZ: f64 = 440.0;
/// One octave in cents. A unit of measure (1200·log2), not a period
/// assumption: Bohlen–Pierce's 3/1 period is simply ~1901.955 of these.
const OCTAVE_CENTS: f64 = 1200.0;

/// MIDI note number to frequency, A4 = 440. The bridge-era floor every
/// absolute anchor is minted from.
pub fn midi_to_hz(midi: u8) -> f64 {
    A4_HZ * ((f64::from(midi) - 69.0) / 12.0).exp2()
}

/// Frequency to fractional MIDI number, total over garbage: zero and
/// negative frequencies pin to the bottom of the table instead of NaN.
pub fn hz_to_midi(hz: f64) -> f64 {
    if hz <= 0.0 || hz.is_nan() {
        return 0.0;
    }
    69.0 + 12.0 * (hz / A4_HZ).log2()
}

/// The nearest legacy MIDI pitch — the Phase-1 compile target.
pub fn nearest_midi(hz: f64) -> u8 {
    let midi = hz_to_midi(hz).round();
    if midi.is_nan() {
        return 0;
    }
    midi.clamp(0.0, 127.0) as u8
}

/// Signed cents between a frequency and its nearest MIDI table entry.
/// Zero (within hair's width) means the legacy path reproduces the pitch
/// exactly; anything else is a playback approximation the UI must show.
pub fn cents_from_midi_table(hz: f64) -> f64 {
    if hz <= 0.0 || hz.is_nan() {
        return 0.0;
    }
    let midi = hz_to_midi(hz);
    (midi - midi.round()) * 100.0
}

// ----------------------------------------------------------- intervals ---

/// Exactly `.scl`'s two spellings: a ratio, or a cents value.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum Interval {
    Ratio(u32, u32),
    Cents(f64),
}

impl Interval {
    /// Size in cents. A malformed ratio (zero anywhere) reads as unison
    /// rather than infinity; construction refuses it before it gets here.
    pub fn cents(&self) -> f64 {
        match *self {
            Interval::Ratio(n, d) => {
                if n == 0 || d == 0 {
                    0.0
                } else {
                    OCTAVE_CENTS * (f64::from(n) / f64::from(d)).log2()
                }
            }
            Interval::Cents(c) => c,
        }
    }

    /// The scale's own spelling when it gave one: `3/2` is a compressed
    /// sign worth keeping; a cents value has no shorter honest name.
    pub fn ratio_label(&self) -> Option<String> {
        match *self {
            Interval::Ratio(n, d) => Some(format!("{n}/{d}")),
            Interval::Cents(_) => None,
        }
    }
}

// -------------------------------------------------------------- scales ---

/// A scale of N degrees per period: degree 0 is the unison, degrees
/// 1..N-1 are `intervals`, and `period` is where the ladder repeats —
/// data, not an assumption; nothing below the lens layer says 2/1.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Scale {
    name: String,
    intervals: Vec<Interval>,
    period: Interval,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScaleError {
    /// Zero numerator or denominator in a ratio.
    ZeroRatio,
    /// A cents value that is not a finite number.
    UnspeakableCents,
    /// The period must ascend: its size has to be positive.
    PeriodDoesNotAscend,
    /// Every interval must sit strictly inside (unison, period).
    IntervalOutOfOrder,
}

impl std::fmt::Display for ScaleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScaleError::ZeroRatio => write!(f, "a ratio with a zero is not a pitch"),
            ScaleError::UnspeakableCents => write!(f, "a cents value must be a finite number"),
            ScaleError::PeriodDoesNotAscend => write!(f, "the period must ascend"),
            ScaleError::IntervalOutOfOrder => write!(
                f,
                "intervals must ascend strictly between the unison and the period"
            ),
        }
    }
}

impl Scale {
    /// The one door in. An unbuildable scale refuses HERE, with words,
    /// so resolution never meets one (law L5).
    pub fn new(
        name: impl Into<String>,
        intervals: Vec<Interval>,
        period: Interval,
    ) -> Result<Self, ScaleError> {
        for interval in intervals.iter().chain(std::iter::once(&period)) {
            match *interval {
                Interval::Ratio(n, d) if n == 0 || d == 0 => return Err(ScaleError::ZeroRatio),
                Interval::Cents(c) if !c.is_finite() => return Err(ScaleError::UnspeakableCents),
                _ => {}
            }
        }
        let period_cents = period.cents();
        if period_cents <= 0.0 {
            return Err(ScaleError::PeriodDoesNotAscend);
        }
        let mut previous = 0.0;
        for interval in &intervals {
            let cents = interval.cents();
            if cents <= previous || cents >= period_cents {
                return Err(ScaleError::IntervalOutOfOrder);
            }
            previous = cents;
        }
        Ok(Self {
            name: name.into(),
            intervals,
            period,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Degrees per period. Never zero: the unison is always a degree.
    pub fn degree_count(&self) -> usize {
        self.intervals.len() + 1
    }

    pub fn period(&self) -> Interval {
        self.period
    }

    pub fn period_cents(&self) -> f64 {
        self.period.cents()
    }

    /// Cents of an UNROTATED degree in 0..degree_count. Out-of-range
    /// degrees wrap through periods — total, never panicking.
    fn raw_degree_cents(&self, degree: i64) -> f64 {
        let n = self.degree_count() as i64;
        let period = degree.div_euclid(n);
        let degree = degree.rem_euclid(n) as usize;
        let base = if degree == 0 {
            0.0
        } else {
            // In range by construction: 1 <= degree <= intervals.len().
            self.intervals
                .get(degree - 1)
                .map(Interval::cents)
                .unwrap_or(0.0)
        };
        base + period as f64 * self.period_cents()
    }

    /// The interval the scale wrote for a degree, for the ratio lens.
    /// Degree 0 is the unison (1/1 by definition).
    pub fn degree_interval(&self, degree: usize) -> Option<Interval> {
        if degree == 0 {
            Some(Interval::Ratio(1, 1))
        } else {
            self.intervals.get(degree - 1).copied()
        }
    }
}

// ---------------------------------------------------------------- keys ---

/// A tuning anchors a scale to physics: degree 0, period 0 sounds at
/// `reference_hz`.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Tuning {
    pub reference_hz: f64,
    pub scale: Scale,
}

/// The harmonic ambient context: a tuning plus a mode, where a mode is a
/// ROTATION of the scale's degrees — dorian is rotation 1 of the diatonic
/// set, and the same operation applies unchanged to any scale.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Key {
    tuning: Tuning,
    mode: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum KeyError {
    /// The rotation names a degree the scale does not have.
    ModeOutOfRange { degrees: usize },
    /// The reference must be a positive, finite frequency.
    UnspeakableReference,
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyError::ModeOutOfRange { degrees } => {
                write!(f, "MODE: SCALE HAS {degrees} DEGREES")
            }
            KeyError::UnspeakableReference => {
                write!(f, "KEY: THE REFERENCE MUST BE A POSITIVE FREQUENCY")
            }
        }
    }
}

impl Key {
    pub fn new(tuning: Tuning, mode: usize) -> Result<Self, KeyError> {
        if !(tuning.reference_hz.is_finite() && tuning.reference_hz > 0.0) {
            return Err(KeyError::UnspeakableReference);
        }
        if mode >= tuning.scale.degree_count() {
            return Err(KeyError::ModeOutOfRange {
                degrees: tuning.scale.degree_count(),
            });
        }
        Ok(Self { tuning, mode })
    }

    pub fn tuning(&self) -> &Tuning {
        &self.tuning
    }

    pub fn scale(&self) -> &Scale {
        &self.tuning.scale
    }

    pub fn mode(&self) -> usize {
        self.mode
    }

    pub fn degree_count(&self) -> usize {
        self.tuning.scale.degree_count()
    }

    pub fn reference_hz(&self) -> f64 {
        self.tuning.reference_hz
    }

    /// Cents of a key degree above the key's own tonic. Signed degree and
    /// period compose into one position on the rotated ladder; every
    /// input is a valid address (law L5).
    pub fn degree_cents(&self, degree: i32, period: i32) -> f64 {
        let n = self.degree_count() as i64;
        let total = i64::from(period)
            .saturating_mul(n)
            .saturating_add(i64::from(degree));
        self.total_cents(total)
    }

    /// The substrate value of a key degree.
    pub fn degree_hz(&self, degree: i32, period: i32) -> f64 {
        self.tuning.reference_hz * (self.degree_cents(degree, period) / OCTAVE_CENTS).exp2()
    }

    /// The nearest key degree to a frequency, with the exact remainder in
    /// cents: `(degree, period, offset_cents)`. Sound-preserving by
    /// construction — `degree_hz(d, p)` shifted by the offset is the
    /// input again.
    pub fn nearest_degree(&self, hz: f64) -> (i32, i32, f64) {
        let hz = if hz > 0.0 {
            hz.min(f64::MAX)
        } else {
            f64::MIN_POSITIVE
        };
        let target = OCTAVE_CENTS * (hz / self.tuning.reference_hz).log2();
        let n = self.degree_count() as i64;
        let period_cents = self.tuning.scale.period_cents();
        let center = (target / period_cents).floor().clamp(-1e15, 1e15) as i64;
        let mut best = (0i64, f64::MAX);
        for period in [center - 1, center, center + 1] {
            for degree in 0..n {
                let total = period.saturating_mul(n).saturating_add(degree);
                let cents = self.total_cents(total);
                let distance = (target - cents).abs();
                if distance < best.1 {
                    best = (total, distance);
                }
            }
        }
        let (total, _) = best;
        let degree = total.rem_euclid(n);
        let period = total.div_euclid(n);
        let offset = target - self.total_cents(total);
        (
            degree.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            period.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            offset,
        )
    }

    fn total_cents(&self, total: i64) -> f64 {
        let mode = self.mode as i64;
        self.tuning
            .scale
            .raw_degree_cents(mode.saturating_add(total))
            - self.tuning.scale.raw_degree_cents(mode)
    }
}

// --------------------------------------------------------------- pitch ---

/// Where a pitch's identity lives.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum Anchor {
    /// Physics: a frequency. Self-contained; survives every key change
    /// untouched. What a drum, a sample, a field recording is.
    Absolute(f64),
    /// Intent: the Nth degree, P periods up, of whatever key governs the
    /// note. Reflows when the key changes. What a melody usually is.
    Degree { degree: i32, period: i32 },
}

/// The stored identity of a pitch: an address plus an explicit deviation.
/// `resolve()` produces the substrate value; ONLY resolved values ever
/// reach compilation. `offset_cents` is data, never error — the bent
/// third stays bent.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Pitch {
    pub anchor: Anchor,
    pub offset_cents: f32,
}

impl Pitch {
    pub fn absolute(hz: f64) -> Self {
        Self {
            anchor: Anchor::Absolute(hz),
            offset_cents: 0.0,
        }
    }

    pub fn degree(degree: i32, period: i32) -> Self {
        Self {
            anchor: Anchor::Degree { degree, period },
            offset_cents: 0.0,
        }
    }

    /// The bridge-era mint: what every legacy MIDI pitch becomes.
    pub fn from_midi(midi: u8) -> Self {
        Self::absolute(midi_to_hz(midi))
    }

    /// Substrate value in Hz. Absolute ignores the key; Degree reads it.
    pub fn resolve(&self, key: &Key) -> f64 {
        let anchor_hz = match self.anchor {
            Anchor::Absolute(hz) => hz,
            Anchor::Degree { degree, period } => key.degree_hz(degree, period),
        };
        anchor_hz * (f64::from(self.offset_cents) / OCTAVE_CENTS).exp2()
    }

    /// Move by chromatic semitones without changing the kind of address.
    /// Twelve semitones is exactly one octave.
    ///
    /// An ABSOLUTE pitch moves its anchor: the note IS the new note, and
    /// every surface names it so, with its bend (the offset) carried
    /// along untouched. A scale-degree pitch has no chromatic step
    /// without its key, so it keeps its address and takes the shift as
    /// an offset — and therefore still follows later key changes.
    pub fn shifted_semitones(self, delta: isize) -> Self {
        match self.anchor {
            Anchor::Absolute(hz) => Self {
                anchor: Anchor::Absolute(hz * (delta as f64 / 12.0).exp2()),
                offset_cents: self.offset_cents,
            },
            Anchor::Degree { .. } => {
                let cents = f64::from(self.offset_cents) + delta as f64 * 100.0;
                Self {
                    anchor: self.anchor,
                    offset_cents: cents.clamp(f64::from(f32::MIN), f64::from(f32::MAX)) as f32,
                }
            }
        }
    }

    /// Territorialize: re-address onto the key, SOUND-PRESERVING. The
    /// nearest degree becomes the anchor; the exact remainder lands in
    /// `offset_cents`. `resolve()` before equals `resolve()` after (L1).
    pub fn quantize_to(&self, key: &Key) -> Pitch {
        let hz = self.resolve(key);
        let (degree, period, offset) = key.nearest_degree(hz);
        Pitch {
            anchor: Anchor::Degree { degree, period },
            offset_cents: offset as f32,
        }
    }

    /// Deterritorialize: release from the key, SOUND-PRESERVING. The
    /// anchor becomes the resolved frequency, the offset zero (L1).
    pub fn free(&self, key: &Key) -> Pitch {
        Pitch::absolute(self.resolve(key))
    }

    /// Deterministic display order for chord stacking. Musical order
    /// within one anchor class; across classes it is a stable convention
    /// (absolutes first), because a degree's true height needs a key and
    /// a sort must not.
    pub fn stack_order(&self, other: &Pitch) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self.anchor, other.anchor) {
            (Anchor::Absolute(a), Anchor::Absolute(b)) => a
                .total_cmp(&b)
                .then(self.offset_cents.total_cmp(&other.offset_cents)),
            (
                Anchor::Degree { degree, period },
                Anchor::Degree {
                    degree: other_degree,
                    period: other_period,
                },
            ) => period
                .cmp(&other_period)
                .then(degree.cmp(&other_degree))
                .then(self.offset_cents.total_cmp(&other.offset_cents)),
            (Anchor::Absolute(_), Anchor::Degree { .. }) => Ordering::Less,
            (Anchor::Degree { .. }, Anchor::Absolute(_)) => Ordering::Greater,
        }
    }
}

// ------------------------------------------------------------- .scl ---

#[derive(Clone, Debug, PartialEq)]
pub enum SclError {
    MissingCount,
    BadCount(String),
    /// Fewer pitch lines than the count promised.
    TooFewPitches {
        promised: usize,
        found: usize,
    },
    BadPitch(String),
    /// A structurally readable file whose scale refuses construction.
    BadScale(ScaleError),
    /// A scale needs at least the period line.
    Empty,
}

impl std::fmt::Display for SclError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SclError::MissingCount => write!(f, "no note-count line"),
            SclError::BadCount(line) => write!(f, "unreadable note count: {line:?}"),
            SclError::TooFewPitches { promised, found } => {
                write!(f, "the file promises {promised} pitches but holds {found}")
            }
            SclError::BadPitch(line) => write!(f, "unreadable pitch line: {line:?}"),
            SclError::BadScale(error) => write!(f, "{error}"),
            SclError::Empty => write!(f, "a scale needs at least its period"),
        }
    }
}

/// Parse the real Scala `.scl` format: `!` comment lines, a description
/// line, a count line, then one interval per line. A value containing
/// `.` is CENTS; otherwise it is a RATIO (`3/2`, bare `2` meaning 2/1).
/// The last interval is the period. Total: garbage refuses with words.
pub fn parse_scl(name: &str, source: &str) -> Result<Scale, SclError> {
    let mut lines = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('!'));
    // The description line: present by format, its content is free text.
    let _description = lines.next().ok_or(SclError::MissingCount)?;
    let count_line = loop {
        match lines.next() {
            Some("") => continue,
            Some(line) => break line,
            None => return Err(SclError::MissingCount),
        }
    };
    let count: usize = count_line
        .split_whitespace()
        .next()
        .ok_or_else(|| SclError::BadCount(count_line.to_owned()))?
        .parse()
        .map_err(|_| SclError::BadCount(count_line.to_owned()))?;
    if count == 0 {
        return Err(SclError::Empty);
    }

    let mut pitches = Vec::with_capacity(count);
    for line in lines {
        if pitches.len() == count {
            break;
        }
        if line.is_empty() {
            continue;
        }
        pitches.push(parse_scl_pitch(line)?);
    }
    if pitches.len() < count {
        return Err(SclError::TooFewPitches {
            promised: count,
            found: pitches.len(),
        });
    }
    let period = pitches.pop().ok_or(SclError::Empty)?;
    Scale::new(name, pitches, period).map_err(SclError::BadScale)
}

/// One pitch line: the first token is the value, the rest is commentary.
fn parse_scl_pitch(line: &str) -> Result<Interval, SclError> {
    let token = line
        .split_whitespace()
        .next()
        .ok_or_else(|| SclError::BadPitch(line.to_owned()))?;
    let bad = || SclError::BadPitch(line.to_owned());
    if token.contains('.') {
        let cents: f64 = token.parse().map_err(|_| bad())?;
        if !cents.is_finite() {
            return Err(bad());
        }
        Ok(Interval::Cents(cents))
    } else if let Some((numerator, denominator)) = token.split_once('/') {
        let numerator: u32 = numerator.parse().map_err(|_| bad())?;
        let denominator: u32 = denominator.parse().map_err(|_| bad())?;
        if numerator == 0 || denominator == 0 {
            return Err(bad());
        }
        Ok(Interval::Ratio(numerator, denominator))
    } else {
        let numerator: u32 = token.parse().map_err(|_| bad())?;
        if numerator == 0 {
            return Err(bad());
        }
        Ok(Interval::Ratio(numerator, 1))
    }
}

// ----------------------------------------------------------- built-ins ---

/// The compiled-in scale set: the familiar case costs nothing, the
/// exotic case costs no MORE. Every file is real `.scl`, parsed by the
/// same parser user files go through.
const BUILTIN_SCL: &[(&str, &str)] = &[
    (
        "chromatic",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/scales/chromatic.scl"
        )),
    ),
    (
        "diatonic",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/scales/diatonic.scl"
        )),
    ),
    (
        "pentatonic",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/scales/pentatonic.scl"
        )),
    ),
    (
        "19edo",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/scales/19edo.scl"
        )),
    ),
    (
        "22shruti",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/scales/22shruti.scl"
        )),
    ),
    (
        "bohlen-pierce",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/scales/bohlen-pierce.scl"
        )),
    ),
    (
        "just-major",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/scales/just-major.scl"
        )),
    ),
    (
        "just-minor",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/scales/just-minor.scl"
        )),
    ),
];

/// A built-in scale by name. Total: an unknown name is `None`, and a
/// built-in that failed to parse would be caught by the round-trip test,
/// so the quiet `.ok()` here can never hide one at runtime.
pub fn builtin_scale(name: &str) -> Option<Scale> {
    BUILTIN_SCL
        .iter()
        .find(|(builtin, _)| builtin.eq_ignore_ascii_case(name))
        .and_then(|(builtin, source)| parse_scl(builtin, source).ok())
}

pub fn builtin_scale_names() -> impl Iterator<Item = &'static str> {
    BUILTIN_SCL.iter().map(|(name, _)| *name)
}

/// The workhorse default: chromatic 12TET rooted a middle C, mode 0.
/// Built from the same parser and constructors as everything else; the
/// `unwrap_or` arms are unreachable while the built-in set parses, which
/// the tests pin.
pub fn default_key() -> Key {
    let scale = builtin_scale("chromatic").unwrap_or_else(|| Scale {
        name: "chromatic".to_owned(),
        intervals: (1..12).map(|k| Interval::Cents(k as f64 * 100.0)).collect(),
        period: Interval::Ratio(2, 1),
    });
    Key::new(
        Tuning {
            reference_hz: midi_to_hz(60),
            scale,
        },
        0,
    )
    .unwrap_or_else(|_| Key {
        tuning: Tuning {
            reference_hz: A4_HZ,
            scale: Scale {
                name: "chromatic".to_owned(),
                intervals: (1..12).map(|k| Interval::Cents(k as f64 * 100.0)).collect(),
                period: Interval::Ratio(2, 1),
            },
        },
        mode: 0,
    })
}

// ------------------------------------------------------- :key parsing ---

/// The church-mode aliases: names for rotations of the diatonic set.
/// One entry per rotation; `minor` is the spoken form of aeolian.
const DIATONIC_MODES: &[(&str, usize)] = &[
    ("major", 0),
    ("ionian", 0),
    ("dorian", 1),
    ("phrygian", 2),
    ("lydian", 3),
    ("mixolydian", 4),
    ("minor", 5),
    ("aeolian", 5),
    ("locrian", 6),
];

/// `:key <tonic> [<scale> [mode N]]` — the palette long form, parsed
/// against the current key (a bare tonic re-roots the scale you already
/// have) and a scale lookup (built-ins first, then the library's `.scl`
/// files, supplied by the caller). Every failure is words for the
/// status line; nothing here panics.
pub fn parse_key_command(
    args: &[&str],
    current: &Key,
    lookup: &dyn Fn(&str) -> Option<Result<Scale, String>>,
) -> Result<Key, String> {
    let mut args = args.iter().copied();
    let tonic = args
        .next()
        .ok_or_else(|| "KEY: NAME A TONIC — :key d dorian".to_owned())?;
    let reference_hz = parse_tonic(tonic)?;

    let mut scale = current.scale().clone();
    let mut mode = current.mode();
    if let Some(word) = args.next() {
        let word_lower = word.to_ascii_lowercase();
        if let Some((_, rotation)) = DIATONIC_MODES.iter().find(|(name, _)| *name == word_lower) {
            scale = builtin_scale("diatonic")
                .ok_or_else(|| "KEY: THE DIATONIC BUILT-IN IS MISSING".to_owned())?;
            mode = *rotation;
        } else {
            let stem = word_lower.trim_end_matches(".scl");
            scale = match lookup(stem) {
                Some(Ok(scale)) => scale,
                Some(Err(error)) => return Err(format!("KEY: {word} — {error}")),
                None => return Err(format!("KEY: NO SCALE NAMED {word}")),
            };
            mode = 0;
        }
    }
    match (args.next(), args.next()) {
        (None, _) => {}
        (Some(keyword), Some(number)) if keyword.eq_ignore_ascii_case("mode") => {
            mode = number
                .parse()
                .map_err(|_| format!("MODE: UNREADABLE NUMBER {number}"))?;
        }
        _ => return Err("KEY: AFTER THE SCALE, ONLY `mode N`".to_owned()),
    }

    Key::new(
        Tuning {
            reference_hz,
            scale,
        },
        mode,
    )
    .map_err(|error| error.to_string())
}

/// A tonic is a 12TET note name (`d`, `f#3`, `bb`), a frequency
/// (`264hz`), or a ratio against A4 = 440 (`3/2`). Note names without an
/// octave mean octave 4 — the middle of the keyboard, not of the model.
pub fn parse_tonic(token: &str) -> Result<f64, String> {
    let lower = token.to_ascii_lowercase();
    if let Some(hz) = lower.strip_suffix("hz") {
        let hz: f64 = hz
            .parse()
            .map_err(|_| format!("KEY: UNREADABLE FREQUENCY {token}"))?;
        if !(hz.is_finite() && hz > 0.0) {
            return Err("KEY: THE REFERENCE MUST BE A POSITIVE FREQUENCY".to_owned());
        }
        return Ok(hz);
    }
    if let Some((numerator, denominator)) = lower.split_once('/') {
        let numerator: f64 = numerator
            .parse()
            .map_err(|_| format!("KEY: UNREADABLE RATIO {token}"))?;
        let denominator: f64 = denominator
            .parse()
            .map_err(|_| format!("KEY: UNREADABLE RATIO {token}"))?;
        if !(numerator > 0.0 && denominator > 0.0) {
            return Err(format!(
                "KEY: A RATIO NEEDS TWO POSITIVE NUMBERS, NOT {token}"
            ));
        }
        return Ok(A4_HZ * numerator / denominator);
    }

    let mut chars = lower.chars();
    let letter = chars.next().ok_or_else(|| "KEY: NAME A TONIC".to_owned())?;
    let class: i32 = match letter {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return Err(format!("KEY: NO TONIC NAMED {token}")),
    };
    let rest: String = chars.collect();
    let (accidental, octave_text) = if let Some(rest) = rest.strip_prefix('#') {
        (1, rest)
    } else if let Some(rest) = rest.strip_prefix('s') {
        (1, rest)
    } else if let Some(rest) = rest.strip_prefix('b') {
        (-1, rest)
    } else {
        (0, rest.as_str())
    };
    let octave: i32 = if octave_text.is_empty() {
        4
    } else {
        octave_text
            .parse()
            .map_err(|_| format!("KEY: NO TONIC NAMED {token}"))?
    };
    let midi = (octave + 1) * 12 + class + accidental;
    if !(0..=127).contains(&midi) {
        return Err(format!("KEY: {token} FALLS OFF THE TABLE"));
    }
    Ok(midi_to_hz(midi as u8))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn twelve_tet(reference_hz: f64) -> Key {
        Key::new(
            Tuning {
                reference_hz,
                scale: builtin_scale("chromatic").unwrap(),
            },
            0,
        )
        .unwrap()
    }

    fn bohlen_pierce(reference_hz: f64) -> Key {
        Key::new(
            Tuning {
                reference_hz,
                scale: builtin_scale("bohlen-pierce").unwrap(),
            },
            0,
        )
        .unwrap()
    }

    fn close(a: f64, b: f64, relative: f64) -> bool {
        (a - b).abs() <= relative * b.abs().max(1e-12)
    }

    #[test]
    fn chromatic_shift_keeps_the_kind_of_address_and_twelve_semitones_is_an_octave() {
        let key = twelve_tet(midi_to_hz(60));
        for pitch in [Pitch::absolute(327.03), Pitch::degree(4, 1)] {
            let up = pitch.shifted_semitones(12);
            let down = pitch.shifted_semitones(-12);
            // The KIND of address survives: a degree stays a degree, an
            // absolute stays absolute (and moves, which is the point).
            assert_eq!(
                std::mem::discriminant(&up.anchor),
                std::mem::discriminant(&pitch.anchor)
            );
            if let Anchor::Degree { .. } = pitch.anchor {
                assert_eq!(up.anchor, pitch.anchor);
                assert_eq!(down.anchor, pitch.anchor);
            }
            assert!(close(up.resolve(&key), pitch.resolve(&key) * 2.0, 1e-6));
            assert!(close(down.resolve(&key), pitch.resolve(&key) / 2.0, 1e-6));
        }
    }

    /// L1 — round trip: striation is removable. Quantize-then-free and
    /// free-then-quantize both resolve back to the original sound.
    #[test]
    fn l1_round_trip_striation_is_removable() {
        let key = twelve_tet(midi_to_hz(60));
        for pitch in [
            Pitch::absolute(327.03),
            Pitch::from_midi(64),
            Pitch {
                anchor: Anchor::Degree {
                    degree: 4,
                    period: -1,
                },
                offset_cents: 13.7,
            },
            Pitch {
                anchor: Anchor::Absolute(455.5),
                offset_cents: -22.0,
            },
        ] {
            let original = pitch.resolve(&key);
            let there = pitch.quantize_to(&key).free(&key).resolve(&key);
            let back = pitch.free(&key).quantize_to(&key).resolve(&key);
            assert!(close(there, original, 1e-9), "{there} vs {original}");
            assert!(close(back, original, 1e-9), "{back} vs {original}");
        }
    }

    /// L2 — zero identity: a 12TET degree with a zero offset lands on the
    /// MIDI frequency table, bit-close, A4 = 440.
    #[test]
    fn l2_zero_identity_twelve_tet_degrees_hit_the_midi_table() {
        let key = twelve_tet(midi_to_hz(60));
        for (degree, period, midi) in [(0, 0, 60u8), (9, 0, 69), (0, 1, 72), (11, -2, 47)] {
            let pitch = Pitch::degree(degree, period);
            let hz = pitch.resolve(&key);
            assert!(
                close(hz, midi_to_hz(midi), 1e-12),
                "degree {degree} period {period}: {hz} vs {}",
                midi_to_hz(midi)
            );
        }
        assert!(close(Pitch::degree(9, 0).resolve(&key), 440.0, 1e-12));
    }

    /// L3 — key-change semantics: degrees move, absolutes hold still, and
    /// offsets ride along unchanged on both.
    #[test]
    fn l3_key_change_moves_degrees_not_absolutes_offsets_ride() {
        let c = twelve_tet(midi_to_hz(60));
        let d = twelve_tet(midi_to_hz(62));
        let degree = Pitch {
            anchor: Anchor::Degree {
                degree: 4,
                period: 0,
            },
            offset_cents: 14.0,
        };
        let absolute = Pitch {
            anchor: Anchor::Absolute(330.0),
            offset_cents: 14.0,
        };
        let ratio = degree.resolve(&d) / degree.resolve(&c);
        assert!(
            close(ratio, (200.0f64 / 1200.0).exp2(), 1e-12),
            "the degree moved by exactly the tonic shift"
        );
        assert!(close(absolute.resolve(&d), absolute.resolve(&c), 1e-15));
        // The offset is carried, not resolved away: removing it changes
        // both readings by the same 14 cents.
        let flat = Pitch {
            offset_cents: 0.0,
            ..degree
        };
        assert!(close(
            degree.resolve(&c) / flat.resolve(&c),
            (14.0f64 / 1200.0).exp2(),
            1e-12
        ));
    }

    /// L4 — period generality: Bohlen–Pierce (3/1) round-trips L1–L3.
    #[test]
    fn l4_bohlen_pierce_period_generality() {
        let key = bohlen_pierce(220.0);
        assert_eq!(key.degree_count(), 13);
        // The period is 3/1 exactly: one period up trebles the frequency.
        assert!(close(Pitch::degree(0, 1).resolve(&key), 660.0, 1e-9));
        // L1 in BP.
        for pitch in [Pitch::absolute(500.0), Pitch::degree(7, 0)] {
            let original = pitch.resolve(&key);
            let there = pitch.quantize_to(&key).free(&key).resolve(&key);
            assert!(close(there, original, 1e-9));
        }
        // L3 in BP: a tonic shift moves degrees by the same ratio.
        let shifted = bohlen_pierce(233.0);
        let pitch = Pitch::degree(3, 0);
        assert!(close(
            pitch.resolve(&shifted) / pitch.resolve(&key),
            233.0 / 220.0,
            1e-12
        ));
    }

    /// L5 — no panic: resolve/quantize/free are total over garbage, and
    /// an empty or disordered scale refuses at construction.
    #[test]
    fn l5_no_panic_total_over_garbage_keys() {
        assert_eq!(
            Scale::new("empty", Vec::new(), Interval::Cents(0.0)),
            Err(ScaleError::PeriodDoesNotAscend)
        );
        assert_eq!(
            Scale::new(
                "backwards",
                vec![Interval::Cents(700.0), Interval::Cents(200.0)],
                Interval::Ratio(2, 1)
            ),
            Err(ScaleError::IntervalOutOfOrder)
        );
        assert_eq!(
            Scale::new("zero", vec![Interval::Ratio(0, 1)], Interval::Ratio(2, 1)),
            Err(ScaleError::ZeroRatio)
        );
        assert!(matches!(
            Key::new(
                Tuning {
                    reference_hz: 0.0,
                    scale: builtin_scale("chromatic").unwrap(),
                },
                0,
            ),
            Err(KeyError::UnspeakableReference)
        ));
        assert!(matches!(
            Key::new(
                Tuning {
                    reference_hz: 440.0,
                    scale: builtin_scale("diatonic").unwrap(),
                },
                7,
            ),
            Err(KeyError::ModeOutOfRange { degrees: 7 })
        ));

        // Garbage PITCHES against a healthy key: total, finite-or-pinned.
        let key = twelve_tet(440.0);
        for pitch in [
            Pitch::absolute(0.0),
            Pitch::absolute(-5.0),
            Pitch::absolute(f64::MAX),
            Pitch {
                anchor: Anchor::Degree {
                    degree: i32::MAX,
                    period: 0,
                },
                offset_cents: 0.0,
            },
            Pitch {
                anchor: Anchor::Degree {
                    degree: i32::MIN,
                    period: i32::MIN,
                },
                offset_cents: f32::MAX,
            },
        ] {
            let _ = pitch.resolve(&key);
            let _ = pitch.quantize_to(&key);
            let _ = pitch.free(&key);
            let _ = nearest_midi(pitch.resolve(&key));
        }
    }

    #[test]
    fn mode_rotation_of_diatonic_yields_dorians_step_pattern() {
        let dorian = Key::new(
            Tuning {
                reference_hz: midi_to_hz(62),
                scale: builtin_scale("diatonic").unwrap(),
            },
            1,
        )
        .unwrap();
        let steps: Vec<f64> = (0..7)
            .map(|d| dorian.degree_cents(d + 1, 0) - dorian.degree_cents(d, 0))
            .collect();
        let expected = [200.0, 100.0, 200.0, 200.0, 200.0, 100.0, 200.0];
        for (step, want) in steps.iter().zip(expected) {
            assert!((step - want).abs() < 1e-9, "{steps:?}");
        }
        // Dorian's degree 2 is a minor third above the tonic: D dorian
        // reads F where D major reads F sharp.
        assert!(close(
            Pitch::degree(2, 0).resolve(&dorian),
            midi_to_hz(65),
            1e-12
        ));
    }

    #[test]
    fn nearest_degree_finds_the_exact_remainder() {
        let key = twelve_tet(midi_to_hz(60));
        // 14 cents above E4.
        let hz = midi_to_hz(64) * (14.0f64 / 1200.0).exp2();
        let (degree, period, offset) = key.nearest_degree(hz);
        assert_eq!((degree, period), (4, 0));
        assert!((offset - 14.0).abs() < 1e-9);
        // Just below the tonic wraps into the period below.
        let (degree, period, offset) = key.nearest_degree(midi_to_hz(59));
        assert_eq!((degree, period), (11, -1));
        assert!(offset.abs() < 1e-9);
    }

    #[test]
    fn scl_parser_reads_the_real_format() {
        let source = "! meanwhile, a comment\n\
                      A worked example\n\
                      3\n\
                      ! the pitches\n\
                      9/8\n\
                      701.955 a labelled fifth\n\
                      2\n";
        let scale = parse_scl("example", source).unwrap();
        assert_eq!(scale.degree_count(), 3);
        assert_eq!(scale.period(), Interval::Ratio(2, 1));
        assert_eq!(scale.degree_interval(1), Some(Interval::Ratio(9, 8)));
        assert_eq!(scale.degree_interval(2), Some(Interval::Cents(701.955)));
    }

    #[test]
    fn scl_parser_refuses_garbage_with_words() {
        assert!(matches!(parse_scl("x", ""), Err(SclError::MissingCount)));
        assert!(matches!(
            parse_scl("x", "desc\nnot-a-number\n"),
            Err(SclError::BadCount(_))
        ));
        assert!(matches!(
            parse_scl("x", "desc\n3\n9/8\n2\n"),
            Err(SclError::TooFewPitches {
                promised: 3,
                found: 2
            })
        ));
        assert!(matches!(
            parse_scl("x", "desc\n2\nwhat\n2\n"),
            Err(SclError::BadPitch(_))
        ));
        assert!(matches!(
            parse_scl("x", "desc\n1\n0/3\n"),
            Err(SclError::BadPitch(_))
        ));
        assert!(matches!(
            parse_scl("x", "desc\n2\n800.0\n700.0\n"),
            Err(SclError::BadScale(ScaleError::IntervalOutOfOrder))
        ));
        // The error carries words for the status line.
        let error = parse_scl("x", "desc\nnope\n").unwrap_err();
        assert!(!error.to_string().is_empty());
    }

    #[test]
    fn every_builtin_parses_and_the_expected_shapes_hold() {
        for name in builtin_scale_names() {
            let scale =
                builtin_scale(name).unwrap_or_else(|| panic!("builtin scale {name} must parse"));
            assert!(scale.degree_count() >= 5, "{name}");
        }
        assert_eq!(builtin_scale("chromatic").unwrap().degree_count(), 12);
        assert_eq!(builtin_scale("diatonic").unwrap().degree_count(), 7);
        assert_eq!(builtin_scale("pentatonic").unwrap().degree_count(), 5);
        assert_eq!(builtin_scale("19edo").unwrap().degree_count(), 19);
        assert_eq!(builtin_scale("22shruti").unwrap().degree_count(), 22);
        assert_eq!(builtin_scale("bohlen-pierce").unwrap().degree_count(), 13);
        assert!(close(
            builtin_scale("bohlen-pierce").unwrap().period_cents(),
            OCTAVE_CENTS * 3.0f64.log2(),
            1e-12
        ));
    }

    #[test]
    fn the_default_key_speaks_twelve_tet_from_middle_c() {
        let key = default_key();
        assert_eq!(key.degree_count(), 12);
        assert!(close(key.reference_hz(), midi_to_hz(60), 1e-15));
        assert!(close(Pitch::degree(9, 0).resolve(&key), 440.0, 1e-12));
    }

    #[test]
    fn the_bridge_mint_and_the_midi_table_agree() {
        for midi in [0u8, 47, 60, 69, 127] {
            assert_eq!(nearest_midi(midi_to_hz(midi)), midi);
            assert!(cents_from_midi_table(midi_to_hz(midi)).abs() < 1e-9);
        }
        assert_eq!(nearest_midi(0.0), 0);
        assert_eq!(nearest_midi(f64::NAN), 0);
        assert_eq!(nearest_midi(1e9), 127);
        // A pitch between the cracks reports its honest distance.
        let bent = midi_to_hz(64) * (30.0f64 / 1200.0).exp2();
        assert!((cents_from_midi_table(bent) - 30.0).abs() < 1e-9);
    }

    #[test]
    fn the_key_command_speaks_tonics_scales_and_modes() {
        let current = default_key();
        let lookup = |name: &str| -> Option<Result<Scale, String>> { builtin_scale(name).map(Ok) };

        let dorian = parse_key_command(&["d", "dorian"], &current, &lookup).unwrap();
        assert!(close(dorian.reference_hz(), midi_to_hz(62), 1e-12));
        assert_eq!(dorian.mode(), 1);
        assert_eq!(dorian.degree_count(), 7);

        let shruti =
            parse_key_command(&["264hz", "22shruti", "mode", "4"], &current, &lookup).unwrap();
        assert!(close(shruti.reference_hz(), 264.0, 1e-12));
        assert_eq!(shruti.mode(), 4);
        assert_eq!(shruti.degree_count(), 22);

        // A bare tonic re-roots the key you already have.
        let retonic = parse_key_command(&["a3"], &current, &lookup).unwrap();
        assert!(close(retonic.reference_hz(), midi_to_hz(57), 1e-12));
        assert_eq!(retonic.degree_count(), current.degree_count());

        // The `.scl` suffix is spelling, not identity.
        let by_file = parse_key_command(&["c", "19edo.scl"], &current, &lookup).unwrap();
        assert_eq!(by_file.degree_count(), 19);

        // Refusals carry words.
        assert_eq!(
            parse_key_command(&["d", "diatonic", "mode", "9"], &current, &lookup).unwrap_err(),
            "MODE: SCALE HAS 7 DEGREES"
        );
        assert!(
            parse_key_command(&["d", "nonsense"], &current, &lookup)
                .unwrap_err()
                .contains("NO SCALE NAMED")
        );
        assert!(
            parse_key_command(&[], &current, &lookup)
                .unwrap_err()
                .contains("NAME A TONIC")
        );
    }

    #[test]
    fn tonics_come_as_names_frequencies_and_ratios() {
        assert!(close(parse_tonic("a").unwrap(), 440.0, 1e-12));
        assert!(close(parse_tonic("d").unwrap(), midi_to_hz(62), 1e-12));
        assert!(close(parse_tonic("f#3").unwrap(), midi_to_hz(54), 1e-12));
        assert!(close(parse_tonic("bb2").unwrap(), midi_to_hz(46), 1e-12));
        assert!(close(parse_tonic("264hz").unwrap(), 264.0, 1e-12));
        assert!(close(parse_tonic("3/2").unwrap(), 660.0, 1e-12));
        assert!(parse_tonic("h").is_err());
        assert!(parse_tonic("0hz").is_err());
        assert!(parse_tonic("c99").is_err());
    }

    #[test]
    fn stack_order_is_total_and_deterministic() {
        let mut pitches = [
            Pitch::degree(3, 0),
            Pitch::absolute(880.0),
            Pitch::degree(1, -1),
            Pitch::absolute(220.0),
        ];
        pitches.sort_by(Pitch::stack_order);
        assert_eq!(pitches[0], Pitch::absolute(220.0));
        assert_eq!(pitches[1], Pitch::absolute(880.0));
        assert_eq!(pitches[2], Pitch::degree(1, -1));
        assert_eq!(pitches[3], Pitch::degree(3, 0));
    }

    #[test]
    fn a_semitone_shift_moves_an_absolute_note_and_offsets_a_degree() {
        let c4 = Pitch::from_midi(60);
        let up = c4.shifted_semitones(7);
        assert!(matches!(up.anchor, Anchor::Absolute(hz) if (hz - midi_to_hz(67)).abs() < 1e-6));
        assert_eq!(up.offset_cents, 0.0, "the bend rides along, unchanged");
        let bent = Pitch {
            anchor: Anchor::Absolute(midi_to_hz(60)),
            offset_cents: 20.0,
        };
        assert_eq!(bent.shifted_semitones(-12).offset_cents, 20.0);
        let degree = Pitch::degree(2, 0).shifted_semitones(1);
        assert!(matches!(
            degree.anchor,
            Anchor::Degree {
                degree: 2,
                period: 0
            }
        ));
        assert_eq!(degree.offset_cents, 100.0);
    }
}
