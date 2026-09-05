//! The sequencer — the step grid, the piano roll, the trig inspector, and
//! the command grammar they are spoken through. Frame-independent.
//!
//! Born inside `ui::redesign` (the second frame) and lifted out whole once
//! a third frame wanted it, for the reason `crate::intent` was lifted: a
//! surface owned by one frame is one a second frame must either import
//! from the frame it is replacing, or mint again and let drift. Both
//! frames now draw THIS sequencer, and its grammar, verb table and
//! registers are the one keyboard vocabulary for editing a pattern.
//!
//! What it may depend on: the model (`sequencing`, `pitch`), the design
//! system (`design::signs`, `ui::tokens`, `ui::affordance`), and egui.
//! What it may not: any frame. Nothing here names `redesign` or `stage`.
//!
//! Contracts: `notes/20260831-command-grammar.md` (the sentence shape,
//! verbs, registers, refusals), `notes/20260831-pitch-lens-spec.md` (how
//! pitch is read and entered), and the sequencing contract for what the
//! edits compile to.

pub mod chrome;
pub mod grammar;
pub mod grid_resolution;
pub mod layout_grid;
pub mod lens;
pub mod midi_typing;
pub mod registers;
pub mod roll;
pub mod sequence;
pub mod sequence_grid;
pub mod trig_info;
pub mod verbs;

use crate::design;
use crate::pitch::Key;
use crate::sequencing::{
    DEFAULT_PATTERN_TICKS, Note, PATTERN_STEP_TICKS, PATTERN_STEPS, Pattern, PatternId, Song,
    TICKS_PER_BEAT,
};
use eframe::egui;
use sequence::NoteView;

/// A grey the sequencer chose against BLACK, projected onto the ground it
/// is actually being drawn on.
///
/// Every value in this surface was picked as "a level above the ground":
/// a panel a little above it, a rule further up, a label further still,
/// and the certain thing at the top. That is a relationship, not a
/// colour — so on a paper ground the same relationship runs the other
/// way, and the level is mirrored rather than reused.
///
/// The mirror is PERCEPTUAL: a level's distance from black in CIE L\*
/// becomes the same distance from paper. Mirroring the byte instead would
/// bunch every dark rung into a narrow band of near-white, because sRGB
/// bytes are not a perceptual space — which is the same argument the
/// design alphabet's ladder is built on.
pub fn shade(level: u8, ground: design::Polarity) -> egui::Color32 {
    let level = match ground {
        design::Polarity::Dark => level,
        design::Polarity::Light => MIRROR[level as usize],
    };
    // A frame that projects the ladder through its own ground gets every
    // level as a shade of that ground; otherwise chrome, not grey — the
    // same cold tint the alphabet's ladder wears.
    match SHADE.read().ok().and_then(|s| *s) {
        Some(lift) => lift(f32::from(level) / 255.0),
        None => design::chrome(level),
    }
}

/// A wash: a thin veil that lifts what is under it one step TOWARD the
/// figure and away from the ground.
///
/// White on black, black on paper. A wash that lightened both would make
/// a light surface recede exactly where it was meant to come forward.
pub fn wash(alpha: u8, ground: design::Polarity) -> egui::Color32 {
    match ground {
        design::Polarity::Dark => egui::Color32::from_white_alpha(alpha),
        design::Polarity::Light => egui::Color32::from_black_alpha(alpha),
    }
}

/// Every level's paper twin, computed once.
///
/// A table rather than arithmetic at each call: this is asked hundreds of
/// times a frame — once per cell — and a cube root per grid cell is a
/// cost with nothing to show for it.
static MIRROR: std::sync::LazyLock<[u8; 256]> = std::sync::LazyLock::new(|| {
    let mut table = [0u8; 256];
    for (level, out) in table.iter_mut().enumerate() {
        let from_black = design::lightness(level as u8);
        let target = design::lstar::light::GROUND - from_black;
        // The nearest byte to the mirrored lightness. 256 candidates,
        // 256 times, once in the life of the process.
        *out = (0u8..=255)
            .min_by(|a, b| {
                let da = (design::lightness(*a) - target).abs();
                let db = (design::lightness(*b) - target).abs();
                da.total_cmp(&db)
            })
            .unwrap_or(0);
    }
    table
});

/// The sequencer's loudest value: the ink every certain thing is drawn
/// in, and the cursor. Hierarchy in this surface comes from value alone
/// (the second frame's charter, kept), and this is the top of it.
pub const INK: egui::Color32 = egui::Color32::WHITE;

/// The same value as a LEVEL, for surfaces that project it onto the
/// ground they are drawn on rather than assuming black.
pub const INK_LEVEL: u8 = 255;

/// The sequencer receives the playhead as a tick, not a frame clock. Its
/// motion therefore comes from the same musical time it draws: parked
/// without a playhead, and one beat of phase while the pattern rolls.
pub(crate) fn phase_of(playhead: Option<usize>) -> design::motion::Phase {
    design::motion::Phase::of(
        playhead.is_some(),
        playhead.map_or(0.0, |tick| {
            (tick % TICKS_PER_BEAT) as f32 / TICKS_PER_BEAT as f32
        }),
    )
}

/// Above this many cents from the nearest twelve-tone pitch, the bridge-
/// era playback path is approximating, and the view says so.
const APPROX_CENTS: f64 = 0.05;

/// How long `id` runs on the timeline, or a pattern's natural length when
/// nothing has placed it — a session clip's case.
pub fn pattern_length(song: &Song, id: PatternId) -> usize {
    song.tracks
        .iter()
        .flat_map(|track| &track.blocks)
        .find(|block| block.pattern_id == id)
        .map(|block| block.length_ticks)
        .or_else(|| song.pattern(id).map(|pattern| pattern.length_ticks))
        .unwrap_or(DEFAULT_PATTERN_TICKS)
}

/// Every sounding note of `pattern`, resolved against `key` for the
/// grid to draw. Green-zone resolution, once per frame: the view carries
/// the finished numbers, and the honest flag that the legacy path is
/// approximating.
pub fn note_views(pattern: &Pattern, key: &Key) -> Vec<NoteView> {
    // Sorted by start so a cell's first note is its earliest.
    let mut views: Vec<NoteView> = note_views_unsorted(pattern, key);
    views.sort_by_key(|view| view.start_ticks);
    views
}

fn note_views_unsorted(pattern: &Pattern, key: &Key) -> Vec<NoteView> {
    (0..PATTERN_STEPS)
        .flat_map(|step| {
            let trig = pattern.trig(step);
            trig.enabled
                .then_some(trig)
                .into_iter()
                .flat_map(move |trig| {
                    trig.notes.iter().map(move |note| {
                        // Where the note actually starts: its step and its
                        // own offset into it, the way the projection plays it.
                        let start = (step * PATTERN_STEP_TICKS)
                            .saturating_add_signed(isize::from(note.micro_ticks));
                        let mut view = note_view(note, start, trig.probability, trig.enabled, key);
                        view.locks = trig.locks.len().min(u8::MAX as usize) as u8;
                        view.slice = trig
                            .lock(crate::params::sampler::SLICE)
                            .map(|slice| slice.round().clamp(1.0, 64.0) as u8);
                        view
                    })
                })
        })
        .collect()
}

/// One note as the sequencer sees it.
pub fn note_view(
    note: &Note,
    start_ticks: usize,
    probability: f32,
    enabled: bool,
    key: &Key,
) -> NoteView {
    let hz = note.pitch.resolve(key);
    NoteView {
        pitch: note.pitch,
        hz,
        midi: crate::pitch::nearest_midi(hz),
        approx: crate::pitch::cents_from_midi_table(hz).abs() > APPROX_CENTS,
        start_ticks,
        length_ticks: note.length_ticks,
        micro_ticks: note.micro_ticks,
        velocity: note.velocity,
        probability,
        enabled,
        muted: note.muted,
        locks: 0,
        slice: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{INK_LEVEL, shade, wash};
    use crate::design;
    use eframe::egui;

    /// A trig's lock count rides every note of that trig into the view,
    /// and a trig without locks reads zero.
    #[test]
    fn a_trigs_locks_ride_its_notes_into_the_views() {
        use crate::sequencing::{Note, PATTERN_STEP_TICKS, Pattern};
        let mut pattern = Pattern::default();
        pattern.toggle(0, Note::new(60, PATTERN_STEP_TICKS, 100));
        pattern.add_tone(0, Note::new(64, PATTERN_STEP_TICKS, 90));
        pattern.toggle(4, Note::new(62, PATTERN_STEP_TICKS, 100));
        pattern.trig_mut(0).set_lock(3, 0.5);
        pattern.trig_mut(0).set_lock(5, 0.1);
        let views = super::note_views(&pattern, &crate::pitch::default_key());
        let at = |tick: usize| views.iter().filter(move |v| v.start_ticks == tick);
        assert_eq!(at(0).count(), 2);
        assert!(
            at(0).all(|view| view.locks == 2),
            "a lock left one note behind"
        );
        assert!(at(4 * PATTERN_STEP_TICKS).all(|view| view.locks == 0));
    }

    /// The sequencer is frame-independent, which is the entire reason it
    /// was lifted. A frame name here would rebuild the coupling quietly.
    #[test]
    fn no_frame_reaches_into_the_sequencer() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/sequencer");
        for entry in std::fs::read_dir(root).expect("sequencer source dir") {
            let path = entry.expect("dir entry").path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("sequencer source file");
            // Judge the code, not the prose about the code: the module
            // doc says where this came from, and this test names what it
            // forbids.
            let body = source.split("#[cfg(test)]").next().unwrap_or(&source);
            let code: String = body
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            for frame in ["ui::redesign", "ui::stage"] {
                assert!(
                    !code.contains(frame),
                    "{} names a frame ({frame})",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn the_dark_ground_gets_the_level_it_was_given() {
        // The projection must be a no-op on the ground these values were
        // chosen against, or the second frame would have been redesigned
        // by a refactor that promised not to touch it.
        for level in [0u8, 5, 10, 18, 48, 112, 145, 255] {
            let shade = shade(level, design::Polarity::Dark);
            assert_eq!(shade.r(), level, "the dark projection moved level {level}");
            assert!(design::is_tint(shade), "a level spent hue: {shade:?}");
        }
    }

    #[test]
    fn paper_sits_the_same_distance_from_its_ground_as_black_does_from_its() {
        for level in [0u8, 5, 10, 18, 32, 48, 88, 112, 145, 200] {
            let from_black = design::lightness(level);
            let mirrored = shade(level, design::Polarity::Light);
            let from_paper = design::lstar::light::GROUND - design::lightness(mirrored.r());
            assert!(
                (from_paper - from_black).abs() < 1.5,
                "level {level} sits {from_black} above black but {from_paper} below paper"
            );
        }

        // Above the paper ground's own lightness the mirror SATURATES
        // rather than wrapping: white is 100 above black and paper is only
        // 96 above it, so the loudest thing paper can carry is black. That
        // is a real limit of the ground and not a rounding error — an
        // emissive screen simply has more room upward than a page has
        // downward.
        let over = shade(255, design::Polarity::Light);
        assert_eq!(design::lightness(over.r()), 0.0);
    }

    #[test]
    fn the_certain_thing_is_farthest_from_whichever_ground_it_is_on() {
        // White on black and near-black on paper: the same MEANING, which
        // is what the projection is for.
        assert_eq!(shade(INK_LEVEL, design::Polarity::Dark).r(), 255);
        let paper_ink = shade(INK_LEVEL, design::Polarity::Light);
        assert!(
            design::lightness(paper_ink.r()) < 5.0,
            "the certain thing is not certain on paper ({paper_ink:?})"
        );
    }

    #[test]
    fn the_ladder_keeps_its_order_on_both_grounds() {
        // Whatever else a projection does, it may not reorder the rungs:
        // a panel must stay nearer the ground than the rule above it.
        let levels = [0u8, 10, 18, 48, 112, 145, 255];
        for pair in levels.windows(2) {
            for ground in [design::Polarity::Dark, design::Polarity::Light] {
                let lower = design::lightness(shade(pair[0], ground).r());
                let upper = design::lightness(shade(pair[1], ground).r());
                let ground_l = design::lightness(shade(0, ground).r());
                assert!(
                    (upper - ground_l).abs() > (lower - ground_l).abs(),
                    "{ground:?}: level {} did not stay nearer the ground than {}",
                    pair[0],
                    pair[1]
                );
            }
        }
    }

    #[test]
    fn a_wash_lifts_toward_the_figure_on_either_ground() {
        // The same wash lightens a black ground and darkens a paper one:
        // both times it lifts what is under it AWAY from the ground.
        assert_eq!(
            wash(14, design::Polarity::Dark),
            egui::Color32::from_white_alpha(14)
        );
        assert_eq!(
            wash(14, design::Polarity::Light),
            egui::Color32::from_black_alpha(14)
        );
    }
}

// ------------------------------------------------------------- projection
//
// The sequencer draws in the design alphabet. A frame that projects the
// alphabet through its own palette registers a lift here, and every
// colour the grid and the roll resolve passes through it. Unset, it is
// the alphabet itself, so a frame that never calls this sees no change.

static PROJECTION: std::sync::RwLock<
    Option<fn(crate::design::Alphabet) -> crate::design::Alphabet>,
> = std::sync::RwLock::new(None);

static SHADE: std::sync::RwLock<Option<fn(f32) -> egui::Color32>> = std::sync::RwLock::new(None);

/// Register (or clear) the ladder the sequencer's levels are read on:
/// `0` the ground, `1` the reading surface.
pub fn set_shade(lift: Option<fn(f32) -> egui::Color32>) {
    if let Ok(mut w) = SHADE.write() {
        *w = lift;
    }
}

/// Register (or clear) the lift the sequencer's colours pass through.
pub fn set_projection(lift: Option<fn(crate::design::Alphabet) -> crate::design::Alphabet>) {
    if let Ok(mut w) = PROJECTION.write() {
        *w = lift;
    }
}

/// The alphabet the sequencer draws in, on this ground, lifted.
pub fn alphabet(ground: crate::design::Polarity) -> crate::design::Alphabet {
    let base = *crate::design::Alphabet::for_polarity(ground);
    match PROJECTION.read().ok().and_then(|p| *p) {
        Some(lift) => lift(base),
        None => base,
    }
}
