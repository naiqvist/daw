//! One state, one symbol.
//!
//! An interface is a code: the app has states, the screen has symbols,
//! and the reader decodes one into the other. Everything worth saying
//! about that follows from a single requirement — **the encoding must be
//! injective**. Two states drawn identically are two states the reader
//! cannot tell apart, however carefully the rest of the design was done,
//! and no amount of layout or wording repairs it.
//!
//! That requirement is unusual among design rules in being CHECKABLE.
//! "Is this legible" needs an eye; "do these two states share a symbol"
//! needs a scan, and the tests below are that scan.
//!
//! # What is checked
//!
//! **Alphabets.** A `label()` that returns the same string for two
//! variants has spent two states on one symbol, leaving some other
//! channel — usually a fill — to carry a distinction the alphabet
//! claimed to make.
//!
//! **Palettes.** A theme that declares fifty-one roles and paints them
//! in forty-three colours is a theme whose vocabulary is partly
//! imaginary. The roles that collide are not random: they are the ones
//! that were added later and given "a colour like the one that means
//! roughly this", which is how a mute and a hot meter end up the same
//! yellow.
//!
//! # Why some sameness is fine
//!
//! Two roles may legitimately share a colour when they can never be
//! confused — when they live on different channels (a glyph's colour
//! against a panel's fill) or in places the eye never holds at once.
//! That is a JUDGEMENT, so it is written down in [`SYNONYMS`] rather
//! than inferred, and anything not written down has to be distinct.
//!
//! # Why the colour check asks for INEQUALITY and not for distance
//!
//! The obvious rule — "no two roles within a perceptual distance of each
//! other" — is wrong, and running it is what proved it wrong. A ground
//! ramp is SUPPOSED to be close: `bg`, `surface`, `surface_raised` and
//! `surface_sunken` are neighbours by design, and what distinguishes
//! them is their ORDER and their adjacency, not their separation. A
//! distance rule flags all of them and is right about none of them.
//!
//! Which pairs share a context, and therefore have to be told apart at a
//! glance, is a design judgement that needs an eye. What does not need
//! an eye is this: two roles painted the SAME COLOUR are one role with
//! two names, and every genuine defect the distance rule found was
//! already at distance zero — a mute and a hot meter, a clip and a
//! danger, the playhead and a warning. So the test asks only for
//! inequality, and [`distance`] is kept to say in the failure how far
//! apart a pair that survives actually is.

use eframe::egui::Color32;

/// Role pairs that are the same colour ON PURPOSE.
///
/// Each entry is a claim that these two can never be mistaken for each
/// other, and the claim is the reason it is allowed. Adding a row here
/// is how a legitimate synonym gets past the test; it should be harder
/// to write than a new colour.
pub const SYNONYMS: &[(&str, &str)] = &[
    // The timeline's lane ground IS the window's ground: the lanes are
    // where the background shows through, not a surface laid over it.
    ("bg", "timeline_lane"),
    // One is a glyph's colour and the other is a fill behind a clip.
    // They never touch, and a hover that matched the muted text it sits
    // beside is a hover nobody has ever misread.
    ("text_muted", "clip_hover"),
    // ONE LADDER, THREE NAMES. Quiet, loud, too loud is the same ramp as
    // fine, careful, wrong — and the engine's realtime zones are that
    // ramp again, applied to safety instead of to level. The names stay
    // separate so a scheme CAN pull them apart; the values coincide
    // because the meanings do, and writing that down is what stops a
    // future edit to one of them silently moving the others.
    ("meter_low", "ok"),
    ("meter_low", "green_zone"),
    ("ok", "green_zone"),
    ("meter_hot", "warn"),
    ("meter_clip", "danger"),
    ("meter_clip", "red_zone"),
    ("danger", "red_zone"),
];

/// A cheap perceptual distance between two colours.
///
/// The weighted Euclidean approximation, which costs three multiplies
/// and tracks human discrimination far better than the unweighted one —
/// green carries most of the eye's luminance response and blue almost
/// none, and an unweighted metric would call two blues that nobody can
/// separate "different".
#[must_use]
pub fn distance(a: Color32, b: Color32) -> f32 {
    let dr = f32::from(a.r()) - f32::from(b.r());
    let dg = f32::from(a.g()) - f32::from(b.g());
    let db = f32::from(a.b()) - f32::from(b.b());
    (2.0 * dr * dr + 4.0 * dg * dg + 3.0 * db * db).sqrt() / 3.0
}

/// Whether these two roles are allowed to look alike.
#[must_use]
pub fn synonymous(one: &str, two: &str) -> bool {
    SYNONYMS
        .iter()
        .any(|(a, b)| (*a == one && *b == two) || (*a == two && *b == one))
}

/// Every role in a palette that another role is too close to.
///
/// Returns the offending pairs with their distance, so a failure names
/// what to change rather than only that something is wrong.
#[must_use]
pub fn collisions(palette: &[(&'static str, Color32)]) -> Vec<(&'static str, &'static str, f32)> {
    let mut out = Vec::new();
    for (index, (one, first)) in palette.iter().enumerate() {
        for (two, second) in &palette[index + 1..] {
            if first == second && !synonymous(one, two) {
                out.push((*one, *two, distance(*first, *second)));
            }
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn distance_tracks_the_eye_rather_than_the_bytes() {
        let black = Color32::from_rgb(0, 0, 0);
        assert_eq!(distance(black, black), 0.0);
        // A one-step nudge is not a different colour, whatever `!=` says.
        assert!(distance(black, Color32::from_rgb(1, 1, 1)) < 2.0);
        // Green moves the eye further than blue, at the same step.
        let green = distance(black, Color32::from_rgb(0, 60, 0));
        let blue = distance(black, Color32::from_rgb(0, 0, 60));
        assert!(green > blue, "{green} should beat {blue}");
        assert!(distance(black, Color32::from_rgb(255, 255, 255)) > 100.0);
    }

    /// NO TWO STATES SHARE A SYMBOL.
    ///
    /// Read off the source, like the affordance rule next door, because
    /// what is being checked is a habit across every enum in the app and
    /// a habit is exactly what a reviewer stops noticing. A `label()`
    /// that answers the same for two variants has spent two states on
    /// one symbol and left a fill to carry the difference.
    #[test]
    fn no_two_states_share_a_symbol() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut collisions = Vec::new();
        let mut scanned = 0;
        for path in sources(std::path::Path::new(root)) {
            let text = std::fs::read_to_string(&path).expect("a source file reads");
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            // Functions that turn a state into a word.
            let mut at = 0;
            while let Some(found) = text[at..].find("-> &'static str {") {
                let start = at + found;
                let head = text[..start].rfind("fn ").unwrap_or(0);
                let signature = &text[head..start];
                if !signature.contains("self") {
                    at = start + 1;
                    continue;
                }
                let body = &text[start..];
                let end = body.find("\n    }").unwrap_or(body.len().min(1200));
                let body = &body[..end];
                let words: Vec<&str> = body
                    .match_indices("=> \"")
                    .map(|(index, _)| {
                        let rest = &body[index + 4..];
                        &rest[..rest.find('"').unwrap_or(0)]
                    })
                    .collect();
                if words.len() > 1 {
                    scanned += 1;
                    for word in &words {
                        if words.iter().filter(|other| *other == word).count() > 1 {
                            let line = text[..start].matches('\n').count() + 1;
                            let fn_name = signature.split_whitespace().nth(1).unwrap_or("?");
                            collisions.push(format!("{name}:{line} {fn_name} both say {word:?}"));
                        }
                    }
                }
                at = start + 1;
            }
        }
        assert!(
            scanned > 8,
            "the scan found only {scanned} alphabets — it has stopped matching"
        );
        collisions.sort();
        collisions.dedup();
        assert!(
            collisions.is_empty(),
            "states that cannot be told apart by their own label:\n{collisions:#?}"
        );
    }

    /// NO TWO ROLES SHARE A COLOUR, IN ANY SCHEME.
    ///
    /// A palette that declares fifty-one roles and paints them in
    /// forty-three has a vocabulary that is partly imaginary — and the
    /// roles that collapse are never random. They are the ones added
    /// later and given "a colour like the one that means roughly this",
    /// which is how a muted lane and a hot meter end up the same yellow.
    ///
    /// Checked PER SCHEME, because the failure this actually caught was
    /// worse than a collision: the same pair of roles was distinct in
    /// one theme and identical in another, so what a reader could tell
    /// apart depended on which skin they had chosen.
    #[test]
    fn no_two_roles_share_a_colour() {
        let mut bad = Vec::new();
        use crate::ui::theme::Theme;
        for (name, theme) in [
            ("dark", Theme::dark()),
            ("light", Theme::light()),
            ("cyberpunk", Theme::cyberpunk()),
        ] {
            for (one, two, apart) in collisions(&theme_palette(&theme)) {
                bad.push(format!("{name}: {one} and {two} are {apart:.0} apart"));
            }
        }
        for (one, two, apart) in collisions(&session_palette()) {
            bad.push(format!("session: {one} and {two} are {apart:.0} apart"));
        }
        bad.sort();
        assert!(
            bad.is_empty(),
            "roles the eye receives as one, and no synonym declared for them:\n{bad:#?}"
        );
    }

    /// Every colour role a theme carries, by name.
    ///
    /// Written out rather than derived, because a `Color32` field is the
    /// only thing that identifies a role and Rust will not enumerate
    /// them — and a list that went stale would silently stop checking
    /// the roles it had lost, which is the one failure a guard must not
    /// have. `every_theme_role_is_checked` holds it to the struct.
    fn theme_palette(theme: &crate::ui::theme::Theme) -> Vec<(&'static str, Color32)> {
        vec![
            ("bg", theme.bg),
            ("surface", theme.surface),
            ("surface_raised", theme.surface_raised),
            ("surface_sunken", theme.surface_sunken),
            ("text", theme.text),
            ("text_muted", theme.text_muted),
            ("text_value", theme.text_value),
            ("outline", theme.outline),
            ("divider", theme.divider),
            ("focus", theme.focus),
            ("accent", theme.accent),
            ("accent_muted", theme.accent_muted),
            ("ok", theme.ok),
            ("warn", theme.warn),
            ("danger", theme.danger),
            ("green_zone", theme.green_zone),
            ("red_zone", theme.red_zone),
            ("meter_low", theme.meter_low),
            ("meter_hot", theme.meter_hot),
            ("meter_clip", theme.meter_clip),
            ("playhead", theme.playhead),
            ("timeline_lane", theme.timeline_lane),
            ("clip_body", theme.clip_body),
            ("clip_hover", theme.clip_hover),
            ("clip_selected", theme.clip_selected),
            ("role_time", theme.role_time),
            ("role_level", theme.role_level),
            ("role_shape", theme.role_shape),
            ("role_mod", theme.role_mod),
            ("role_time_dim", theme.role_time_dim),
            ("role_level_dim", theme.role_level_dim),
            ("role_shape_dim", theme.role_shape_dim),
            ("role_mod_dim", theme.role_mod_dim),
            ("loop_region", theme.loop_region),
            ("loop_brace", theme.loop_brace),
            ("selection", theme.selection),
            ("grid_beat", theme.grid_beat),
            ("grid_bar", theme.grid_bar),
            ("grid_sub", theme.grid_sub),
            ("timeline_lane_alt", theme.timeline_lane_alt),
            ("timeline_lane_selected", theme.timeline_lane_selected),
            ("clip_midi", theme.clip_midi),
            ("clip_midi_header", theme.clip_midi_header),
            ("clip_audio", theme.clip_audio),
            ("clip_audio_header", theme.clip_audio_header),
            ("clip_note", theme.clip_note),
            ("note_fill", theme.note_fill),
            ("note_fill_selected", theme.note_fill_selected),
            ("note_edge", theme.note_edge),
            ("note_hover", theme.note_hover),
            ("note_ghost", theme.note_ghost),
        ]
    }

    fn session_palette() -> Vec<(&'static str, Color32)> {
        let colors = crate::ui::session_next::SessionColors::default();
        vec![
            ("bg", colors.bg),
            ("surface", colors.surface),
            ("raised", colors.raised),
            ("sunken", colors.sunken),
            ("text", colors.text),
            ("muted", colors.muted),
            ("divider", colors.divider),
            ("outline", colors.outline),
            ("focus", colors.focus),
            ("accent", colors.accent),
            ("accent_dim", colors.accent_dim),
            ("midi", colors.midi),
            ("audio", colors.audio),
            ("selected", colors.selected),
            ("ok", colors.ok),
            ("warn", colors.warn),
            ("danger", colors.danger),
            ("role_time", colors.role_time),
            ("role_level", colors.role_level),
            ("role_shape", colors.role_shape),
            ("role_mod", colors.role_mod),
            ("meter_low", colors.meter_low),
            ("meter_hot", colors.meter_hot),
            ("meter_clip", colors.meter_clip),
        ]
    }

    /// The lists above must not go stale: a role added to the struct and
    /// forgotten here would be a role nothing checks.
    #[test]
    fn every_theme_role_is_checked() {
        let source =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui/theme.rs"))
                .expect("theme.rs reads");
        let declared: Vec<&str> = source
            .lines()
            .filter_map(|line| line.trim().strip_prefix("pub "))
            .filter_map(|line| line.strip_suffix(": Color32,"))
            .collect();
        let checked = theme_palette(&crate::ui::theme::Theme::dark());
        let missing: Vec<&&str> = declared
            .iter()
            .filter(|role| !checked.iter().any(|(name, _)| name == *role))
            .collect();
        assert!(
            missing.is_empty(),
            "theme roles nothing checks for collisions: {missing:?}"
        );
    }

    fn sources(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(sources(&path));
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
        out
    }
}
