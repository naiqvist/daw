//! Lenses: the sign systems pitch is read through.
//!
//! A lens maps addresses to signs and back. It is how YOU read, never
//! what the music IS — stored data carries no note names, numerals or
//! sargam; those live here, switchable per track, mintable as RON files
//! that live in the library beside `.scl` files. A lens that cannot
//! speak the current key refuses VISIBLY and falls back to the universal
//! `degrees` lens — never silently, never by guessing.
//! Contract: `notes/20260831-pitch-lens-spec.md` §3.

use crate::pitch::{Anchor, Key, cents_from_midi_table, nearest_midi};
use crate::ui::sequencer::sequence::NoteView;

/// The ambient key's sign for the transport bar: `D DORIAN`,
/// `264HZ 22SHRUTI/4`. Key is state, not a mode — the sign never
/// inverts, never shouts.
pub fn key_sign(key: &Key) -> String {
    let reference = key.reference_hz();
    let tonic = if cents_from_midi_table(reference).abs() < 0.5 {
        crate::theory::pitch_class_name(nearest_midi(reference)).to_owned()
    } else {
        format!("{reference:.0}HZ")
    };
    let scale = key.scale().name().to_ascii_uppercase();
    let mode = key.mode();
    // The diatonic set's rotations have spoken names; every other scale
    // says its rotation as a number.
    if scale == "DIATONIC" {
        let mode_name = [
            "MAJOR",
            "DORIAN",
            "PHRYGIAN",
            "LYDIAN",
            "MIXOLYDIAN",
            "MINOR",
            "LOCRIAN",
        ]
        .get(mode)
        .copied()
        .unwrap_or("MAJOR");
        format!("{tonic} {mode_name}")
    } else if mode == 0 {
        format!("{tonic} {scale}")
    } else {
        format!("{tonic} {scale}/{mode}")
    }
}

/// How a naming lens spells deviation. `RatioApprox` is reserved: the
/// format accepts it so files stay portable, but v1 refuses it out loud
/// rather than quietly rendering it as cents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
pub enum OffsetStyle {
    Cents,
    RatioApprox,
}

/// A user-mintable naming lens, exactly the RON file format:
///
/// ```ron
/// Lens(
///     name: "sargam",
///     degree_names: ["Sa","Re","Ga","Ma","Pa","Dha","Ni"],
///     period_mark: "'",
///     offset_style: Cents,
/// )
/// ```
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct Lens {
    pub name: String,
    /// One name per scale degree; a count that mismatches the active
    /// key's scale makes the lens fall back to degree integers.
    pub degree_names: Vec<String>,
    /// Appended once per period up (`Sa'`); periods down say `,`.
    pub period_mark: String,
    pub offset_style: OffsetStyle,
}

/// Parse a lens file. Refusals are words for the status line.
pub fn parse_lens(source: &str) -> Result<Lens, String> {
    let lens: Lens =
        ron::from_str(source).map_err(|error| format!("unreadable lens file: {error}"))?;
    if lens.degree_names.is_empty() {
        return Err("a lens needs at least one degree name".to_owned());
    }
    if lens.offset_style == OffsetStyle::RatioApprox {
        return Err("LENS: RatioApprox IS RESERVED FOR V2 — USE Cents".to_owned());
    }
    Ok(lens)
}

/// The built-in lens names, valid everywhere `:lens` looks.
pub const BUILTIN_LENSES: [&str; 4] = ["degrees", "notes", "cents", "ratio"];

/// A lens resolved against the active key: what the grid actually draws
/// through this frame. Fallback happens HERE, per frame, because the key
/// can change under any lens at any time.
#[derive(Clone, Debug, PartialEq)]
pub enum ActiveLens {
    /// `^1..^N` — total over every scale, the universal fallback.
    Degrees,
    /// `C, D, E…` — valid only where the key embeds in 12TET.
    Notes,
    /// Raw substrate: the resolved frequency itself.
    Cents,
    /// The scale's own ratio spellings, where the `.scl` provided them.
    Ratio,
    /// A named lens whose degree names fit the key.
    Named(Lens),
}

/// The harmonic context as the sequence strip reads it each frame: the
/// resolved lens, the key it resolves against, and the status-line words
/// (key sign, lens name, any fallback refusal — visible, always).
#[derive(Clone, Debug, PartialEq)]
pub struct LensView {
    pub active: ActiveLens,
    pub key: Key,
    pub status: String,
}

impl LensView {
    /// Resolve a requested lens against a key and compose the status
    /// words: `D DORIAN · SARGAM`, or the refusal when a fallback speaks.
    pub fn resolve(
        requested: &str,
        key: &Key,
        files: &dyn Fn(&str) -> Option<Result<Lens, String>>,
    ) -> Self {
        let (active, fallback) = resolve_lens(requested, key, files);
        let status = match fallback {
            Some(refusal) => format!("{} · {refusal} → DEGREES", key_sign(key)),
            None => format!("{} · {}", key_sign(key), lens_name(&active)),
        };
        Self {
            active,
            key: key.clone(),
            status,
        }
    }
}

impl Default for LensView {
    /// The lens the world starts under: 12TET note names against the
    /// default key — today's display, exactly.
    fn default() -> Self {
        let key = crate::pitch::default_key();
        Self::resolve("notes", &key, &|_| None)
    }
}

/// A requested lens held against the key. Returns the lens that will
/// actually speak, plus the refusal words when it is not the one asked
/// for (the fallback must be visible).
pub fn resolve_lens(
    requested: &str,
    key: &Key,
    files: &dyn Fn(&str) -> Option<Result<Lens, String>>,
) -> (ActiveLens, Option<String>) {
    match requested.to_ascii_lowercase().as_str() {
        "degrees" => (ActiveLens::Degrees, None),
        "cents" => (ActiveLens::Cents, None),
        "ratio" => (ActiveLens::Ratio, None),
        "notes" => {
            if key_embeds_in_twelve_tet(key) {
                (ActiveLens::Notes, None)
            } else {
                (
                    ActiveLens::Degrees,
                    Some("NOTES: KEY IS NOT 12TET".to_owned()),
                )
            }
        }
        name => match files(name) {
            Some(Ok(lens)) => {
                if lens.degree_names.len() == key.degree_count() {
                    (ActiveLens::Named(lens), None)
                } else {
                    (
                        ActiveLens::Degrees,
                        Some(format!(
                            "{}: {} NAMES, KEY HAS {}",
                            lens.name.to_ascii_uppercase(),
                            lens.degree_names.len(),
                            key.degree_count()
                        )),
                    )
                }
            }
            Some(Err(error)) => (ActiveLens::Degrees, Some(error)),
            None => (
                ActiveLens::Degrees,
                Some(format!("LENS: NO LENS NAMED {requested}")),
            ),
        },
    }
}

/// Whether every degree of the key lands on the 12TET grid (within a
/// half cent) — the validity condition of the `notes` lens.
pub fn key_embeds_in_twelve_tet(key: &Key) -> bool {
    if cents_from_midi_table(key.reference_hz()).abs() > 0.5 {
        return false;
    }
    (0..key.degree_count() as i32).all(|degree| {
        let cents = key.degree_cents(degree, 0);
        (cents - (cents / 100.0).round() * 100.0).abs() < 0.5
    })
}

/// The lens's name for an address — the cell text's core, before the
/// deviation ticks and the `≈` sign are added around it.
pub fn address_label(lens: &ActiveLens, note: &NoteView, key: &Key) -> String {
    match (lens, note.pitch.anchor) {
        (ActiveLens::Cents, _) => hz_short(note.hz),
        // Physics has no degree: every naming lens speaks an absolute
        // anchor in the plainest sign it has — the nearest 12TET name.
        (_, Anchor::Absolute(_)) => twelve_tet_name(note.midi),
        (ActiveLens::Degrees, Anchor::Degree { degree, period }) => degree_label(degree, period),
        (ActiveLens::Notes, Anchor::Degree { .. }) => twelve_tet_name(note.midi),
        (ActiveLens::Ratio, Anchor::Degree { degree, period }) => {
            let n = key.degree_count() as i64;
            let raw = (key.mode() as i64 + i64::from(degree)).rem_euclid(n) as usize;
            match key
                .scale()
                .degree_interval(raw)
                .and_then(|i| i.ratio_label())
            {
                Some(ratio) => format!("{ratio}{}", period_marks(period)),
                // The scale spelled this degree in cents; the ratio lens
                // has no name for it, so the fallback speaks.
                None => degree_label(degree, period),
            }
        }
        (ActiveLens::Named(lens), Anchor::Degree { degree, period }) => {
            let n = lens.degree_names.len() as i64;
            let total = i64::from(degree) + i64::from(period) * n;
            let index = total.rem_euclid(n) as usize;
            let periods = total.div_euclid(n);
            let marks = match periods.cmp(&0) {
                std::cmp::Ordering::Greater => lens
                    .period_mark
                    .repeat(periods.unsigned_abs().min(4) as usize),
                std::cmp::Ordering::Less => ",".repeat(periods.unsigned_abs().min(4) as usize),
                std::cmp::Ordering::Equal => String::new(),
            };
            format!("{}{marks}", lens.degree_names[index])
        }
    }
}

/// The lens's own name, for the status line beside the key sign.
pub fn lens_name(lens: &ActiveLens) -> String {
    match lens {
        ActiveLens::Degrees => "DEGREES".to_owned(),
        ActiveLens::Notes => "NOTES".to_owned(),
        ActiveLens::Cents => "CENTS".to_owned(),
        ActiveLens::Ratio => "RATIO".to_owned(),
        ActiveLens::Named(lens) => lens.name.to_ascii_uppercase(),
    }
}

/// A degree address in the universal `degrees` lens: `^3`, one period
/// up `^3'`, one down `^3,`. Degrees print one-based — musicians count
/// from one; storage counts from zero.
pub fn degree_label(degree: i32, period: i32) -> String {
    format!("^{}{}", i64::from(degree) + 1, period_marks(period))
}

fn period_marks(period: i32) -> String {
    match period.cmp(&0) {
        std::cmp::Ordering::Greater => "'".repeat(period.unsigned_abs().min(4) as usize),
        std::cmp::Ordering::Less => ",".repeat(period.unsigned_abs().min(4) as usize),
        std::cmp::Ordering::Equal => String::new(),
    }
}

fn twelve_tet_name(midi: u8) -> String {
    let octave = i16::from(midi / 12) - 1;
    format!("{}{octave}", crate::theory::pitch_class_name(midi))
}

/// The raw substrate, sized for a trig cell: `327`, `4.2K`.
fn hz_short(hz: f64) -> String {
    if hz >= 10_000.0 {
        format!("{:.1}K", hz / 1000.0)
    } else if hz >= 1000.0 {
        format!("{:.2}K", hz / 1000.0)
    } else {
        format!("{hz:.0}")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::pitch::{Pitch, Tuning, builtin_scale, default_key, midi_to_hz};

    fn no_files(_: &str) -> Option<Result<Lens, String>> {
        None
    }

    fn dorian() -> Key {
        Key::new(
            Tuning {
                reference_hz: midi_to_hz(62),
                scale: builtin_scale("diatonic").expect("built-in"),
            },
            1,
        )
        .expect("valid key")
    }

    fn shruti() -> Key {
        Key::new(
            Tuning {
                reference_hz: 264.0,
                scale: builtin_scale("22shruti").expect("built-in"),
            },
            0,
        )
        .expect("valid key")
    }

    fn degree_view(degree: i32, period: i32, key: &Key) -> NoteView {
        let pitch = Pitch::degree(degree, period);
        let hz = pitch.resolve(key);
        NoteView {
            pitch,
            hz,
            midi: crate::pitch::nearest_midi(hz),
            approx: false,
            start_ticks: 0,
            length_ticks: 12,
            micro_ticks: 0,
            velocity: 100,
            probability: 1.0,
            enabled: true,
            muted: false,
        }
    }

    #[test]
    fn the_key_sign_names_what_governs() {
        assert_eq!(key_sign(&default_key()), "C CHROMATIC");
        assert_eq!(key_sign(&dorian()), "D DORIAN");
        let shifted = Key::new(
            Tuning {
                reference_hz: 264.0,
                scale: builtin_scale("22shruti").expect("built-in"),
            },
            4,
        )
        .expect("valid key");
        assert_eq!(key_sign(&shifted), "264HZ 22SHRUTI/4");
    }

    /// The lens file format round-trips, and the reserved offset style
    /// refuses with words instead of silently meaning cents.
    #[test]
    fn lens_files_parse_and_refuse_with_words() {
        let sargam = parse_lens(
            r#"Lens(
                name: "sargam",
                degree_names: ["Sa","Re","Ga","Ma","Pa","Dha","Ni"],
                period_mark: "'",
                offset_style: Cents,
            )"#,
        )
        .unwrap();
        assert_eq!(sargam.name, "sargam");
        assert_eq!(sargam.degree_names.len(), 7);

        assert!(parse_lens("not a lens").is_err());
        assert!(
            parse_lens(
                r#"Lens(name: "x", degree_names: ["a"], period_mark: "'", offset_style: RatioApprox)"#
            )
            .unwrap_err()
            .contains("V2")
        );
    }

    /// The `notes` lens is valid exactly where the key embeds in 12TET;
    /// elsewhere it refuses visibly and degree integers speak.
    #[test]
    fn the_notes_lens_refuses_off_grid_keys() {
        let (active, fallback) = resolve_lens("notes", &dorian(), &no_files);
        assert_eq!(active, ActiveLens::Notes);
        assert_eq!(fallback, None);

        let (active, fallback) = resolve_lens("notes", &shruti(), &no_files);
        assert_eq!(active, ActiveLens::Degrees);
        assert_eq!(fallback.as_deref(), Some("NOTES: KEY IS NOT 12TET"));
    }

    /// A named lens whose name count mismatches the key falls back to
    /// degrees, visibly.
    #[test]
    fn a_mismatched_named_lens_falls_back_out_loud() {
        let sargam = Lens {
            name: "sargam".to_owned(),
            degree_names: ["Sa", "Re", "Ga", "Ma", "Pa", "Dha", "Ni"]
                .map(str::to_owned)
                .to_vec(),
            period_mark: "'".to_owned(),
            offset_style: OffsetStyle::Cents,
        };
        let files = move |name: &str| (name == "sargam").then(|| Ok(sargam.clone()));

        let (active, fallback) = resolve_lens("sargam", &dorian(), &files);
        assert!(matches!(active, ActiveLens::Named(_)));
        assert_eq!(fallback, None);

        let (active, fallback) = resolve_lens("sargam", &shruti(), &files);
        assert_eq!(active, ActiveLens::Degrees);
        assert_eq!(fallback.as_deref(), Some("SARGAM: 7 NAMES, KEY HAS 22"));

        let (_, fallback) = resolve_lens("nonsense", &dorian(), &no_files);
        assert_eq!(fallback.as_deref(), Some("LENS: NO LENS NAMED nonsense"));
    }

    /// One address, four spellings: `E3` / `^3` / `Ga` / the substrate —
    /// the same pitch through four sign systems.
    #[test]
    fn one_address_reads_through_every_lens() {
        let key = dorian();
        // Degree 2 of D dorian is F4.
        let note = degree_view(2, 0, &key);
        assert_eq!(address_label(&ActiveLens::Degrees, &note, &key), "^3");
        assert_eq!(address_label(&ActiveLens::Notes, &note, &key), "F4");
        assert_eq!(address_label(&ActiveLens::Cents, &note, &key), "349");
        let sargam = ActiveLens::Named(Lens {
            name: "sargam".to_owned(),
            degree_names: ["Sa", "Re", "Ga", "Ma", "Pa", "Dha", "Ni"]
                .map(str::to_owned)
                .to_vec(),
            period_mark: "'".to_owned(),
            offset_style: OffsetStyle::Cents,
        });
        assert_eq!(address_label(&sargam, &note, &key), "Ga");
        // A period up wears the lens's own mark.
        let high = degree_view(2, 1, &key);
        assert_eq!(address_label(&sargam, &high, &key), "Ga'");
        assert_eq!(address_label(&ActiveLens::Degrees, &high, &key), "^3'");
    }

    /// The ratio lens speaks the scale's own spellings where the `.scl`
    /// gave them, and falls back to degrees where it spelled cents.
    #[test]
    fn the_ratio_lens_speaks_the_scales_own_signs() {
        let key = shruti();
        let fifth = degree_view(13, 0, &key);
        assert_eq!(address_label(&ActiveLens::Ratio, &fifth, &key), "3/2");
        // The chromatic built-in is spelled in cents: no ratio to speak.
        let chromatic = default_key();
        let third = degree_view(4, 0, &chromatic);
        assert_eq!(address_label(&ActiveLens::Ratio, &third, &chromatic), "^5");
    }

    /// Physics has no degree: an absolute anchor reads as its nearest
    /// 12TET name under every naming lens, and as substrate under cents.
    #[test]
    fn absolute_anchors_read_as_physics() {
        let key = dorian();
        let note = NoteView::from_midi(69, 0, 12, 100, 1.0, true);
        assert_eq!(address_label(&ActiveLens::Degrees, &note, &key), "A4");
        assert_eq!(address_label(&ActiveLens::Notes, &note, &key), "A4");
        assert_eq!(address_label(&ActiveLens::Cents, &note, &key), "440");
    }
}
