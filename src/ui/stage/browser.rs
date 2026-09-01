//! The browser's contents: what the library can offer, and the cursor
//! standing in it.
//!
//! This module is the MODEL half — what is browsable, what matches what
//! has been typed, and which row the cursor is on. Drawing lives in the
//! stage beside it, so the question "what is in the library" never has to
//! be answered by the code that decides where pixels go.
//!
//! The interaction it is shaped for is TYPE TO FILTER rather than scroll
//! to find: naming a thing is logarithmic in the size of the library while
//! scanning it is linear, and this is a keyboard instrument. Arrows are
//! for the audition loop — hear one, no, hear the next — which is the one
//! browsing job that typing cannot do.

use super::grid::{FocusColumn, Step};
use crate::devices::{DEVICES, DeviceKind};
use crate::library::AssetRecord;
use std::path::PathBuf;

/// The glyph vocabulary, all of it VERIFIED present in bundled Terminus.
///
/// Terminus is a console font, so the ruling and block characters a
/// terminal interface is built from are native to it rather than borrowed
/// from a fallback face — which matters, because a fallback glyph arrives
/// at a different weight and breaks the monospace grid.
///
/// What it does NOT have, checked rather than assumed: the triangles
/// (`▶ ▸ ►`, U+25B6..) that a list cursor usually reaches for, and the
/// hollow square `□`. Reaching for those renders tofu. The filled marks
/// below are the substitutes, and the arrows U+2190..2195 are present.
///
/// Declared ahead of its renderer on purpose: this is the vocabulary a
/// terminal interface is drawn from, settled and checked once, so the
/// drawing code chooses from a verified set instead of reaching for
/// whatever character looks right and discovering the gap on screen.
#[allow(dead_code, reason = "the vocabulary is settled before its renderer")]
pub mod glyph {
    /// Rules. Light for structure inside a pane, double for its border —
    /// the 1990s convention, where weight ranks the division.
    pub const RULE: char = '─';
    pub const STILE: char = '│';
    pub const RULE_HEAVY: char = '═';
    pub const STILE_HEAVY: char = '║';

    /// Corners and junctions, light.
    pub const CORNER_TL: char = '┌';
    pub const CORNER_TR: char = '┐';
    pub const CORNER_BL: char = '└';
    pub const CORNER_BR: char = '┘';
    pub const TEE_L: char = '├';
    pub const TEE_R: char = '┤';

    /// Shading, for weight without ink: the texture a terminal used when
    /// it had two colours and needed three.
    pub const SHADE_LIGHT: char = '░';
    pub const SHADE_MID: char = '▒';
    pub const SHADE_HEAVY: char = '▓';

    /// Blocks. `CARET` is the typing cursor and `MARK` the row cursor —
    /// both filled, because the triangles are not in the font.
    pub const BLOCK: char = '█';
    pub const CARET: char = '▌';
    pub const MARK: char = '▮';
    pub const DOT: char = '■';

    /// Present, and the honest way to point.
    pub const ARROW_R: char = '→';
    pub const ARROW_D: char = '↓';
}

/// A shelf of the library. The top level of the browser, and the only
/// level that is fixed — everything under it is whatever was found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shelf {
    Devices,
    Samples,
    Projects,
}

impl Shelf {
    pub const ALL: [Self; 3] = [Self::Devices, Self::Samples, Self::Projects];

    pub fn label(self) -> &'static str {
        match self {
            Self::Devices => "DEVICES",
            Self::Samples => "SAMPLES",
            Self::Projects => "PROJECTS",
        }
    }
}

/// What a row actually is, once you press Enter on it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntryKind {
    /// Descend into a shelf.
    Shelf(Shelf),
    /// An instrument or effect, ready to head or join a chain.
    Device(DeviceKind),
    /// A file on disk that can be auditioned.
    Sample(PathBuf),
    /// A song that can be opened.
    Project(PathBuf),
}

/// One row of the library.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    /// What it is called, and what typing is matched against.
    pub label: String,
    pub kind: EntryKind,
}

/// Whether the rows under the current shelf are complete.
///
/// This is display truth, not progress theatre: the scanner either still
/// owns the answer, has delivered it, or no neutral source exists yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BrowserStatus {
    Ready,
    Scanning,
    Unavailable,
}

/// Every registered device, with instruments first and effects second.
/// The registry is the source of truth, so adding a device there makes it
/// browsable here without growing a second catalog.
pub(super) fn device_entries() -> Vec<Entry> {
    [true, false]
        .into_iter()
        .flat_map(|instrument| {
            DEVICES
                .iter()
                .filter(move |spec| spec.instrument == instrument)
                .map(|spec| Entry {
                    label: spec.name.to_uppercase(),
                    kind: EntryKind::Device(spec.kind),
                })
        })
        .collect()
}

/// One immutable scanner record becomes one browser row. No filesystem work
/// happens here; the path was already resolved by the green-zone service.
pub(super) fn sample_entries(assets: &[AssetRecord]) -> Vec<Entry> {
    assets
        .iter()
        .map(|asset| Entry {
            label: asset.relative_path.to_string_lossy().into_owned(),
            kind: EntryKind::Sample(asset.path.clone()),
        })
        .collect()
}

/// PROJECT SOURCE SEAM.
///
/// There is no frame-independent project catalog today. Keep this empty
/// until one exists; guessing a directory here would turn a machine-local
/// convention into an accidental data model.
pub(super) fn project_entries() -> Vec<Entry> {
    Vec::new()
}

impl Entry {
    pub fn shelf(shelf: Shelf) -> Self {
        Self {
            label: shelf.label().to_owned(),
            kind: EntryKind::Shelf(shelf),
        }
    }
}

/// The library, what has been typed, and where the cursor is standing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Browser {
    entries: Vec<Entry>,
    /// What has been typed. Never shown as state text — it is shown
    /// because it is being written, which is a different thing.
    query: String,
    /// Indices into `entries` that survive the query, in library order.
    matches: Vec<usize>,
    cursor: FocusColumn,
    shelf: Option<Shelf>,
    status: BrowserStatus,
}

impl Browser {
    /// The top of the library: the shelves, and nothing scanned yet.
    pub fn shelves() -> Self {
        Self::new(Shelf::ALL.into_iter().map(Entry::shelf).collect())
    }

    pub fn new(entries: Vec<Entry>) -> Self {
        let mut browser = Self {
            entries,
            query: String::new(),
            matches: Vec::new(),
            cursor: FocusColumn::new(0),
            shelf: None,
            status: BrowserStatus::Ready,
        };
        browser.refilter();
        browser
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub(super) fn shelf(&self) -> Option<Shelf> {
        self.shelf
    }

    pub(super) fn status(&self) -> BrowserStatus {
        self.status
    }

    /// Rows that survive the query, in library order.
    pub fn matches(&self) -> impl Iterator<Item = &Entry> {
        self.matches.iter().map(|index| &self.entries[*index])
    }

    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    pub fn cursor(&self) -> Option<usize> {
        self.cursor.cursor()
    }

    /// The row the cursor is on, if the query left it anything to stand on.
    pub fn selected(&self) -> Option<&Entry> {
        let index = self.cursor.cursor()?;
        self.matches.get(index).map(|entry| &self.entries[*entry])
    }

    pub fn step(&mut self, step: Step) -> bool {
        self.cursor.step(step)
    }

    /// Extend the query by one character.
    pub fn type_char(&mut self, ch: char) {
        self.query.push(ch);
        self.refilter();
    }

    /// Retract the last character. `false` means there was nothing to
    /// retract, which the caller reports rather than swallows.
    pub fn backspace(&mut self) -> bool {
        let popped = self.query.pop().is_some();
        if popped {
            self.refilter();
        }
        popped
    }

    /// Replace the library, keeping what has been typed. This is how a
    /// scan lands, and how descending into a shelf works.
    pub fn load(&mut self, entries: Vec<Entry>) {
        self.entries = entries;
        self.refilter();
    }

    /// Descend one layer. A shelf is a new list, so its query begins empty.
    pub(super) fn enter_shelf(&mut self, shelf: Shelf, entries: Vec<Entry>, status: BrowserStatus) {
        self.shelf = Some(shelf);
        self.status = status;
        self.query.clear();
        self.load(entries);
    }

    /// Replace a shelf after an asynchronous scan lands without inventing a
    /// navigation event. What was typed remains the filter on the new truth.
    pub(super) fn refresh_shelf(&mut self, entries: Vec<Entry>, status: BrowserStatus) {
        self.status = status;
        self.load(entries);
    }

    /// Climb from a shelf to the fixed top level. `false` means the browser
    /// is already at its root and Escape should dismiss it instead.
    pub(super) fn ascend(&mut self) -> bool {
        if self.shelf.is_none() {
            return false;
        }
        *self = Self::shelves();
        true
    }

    /// Recompute the surviving rows and put the cursor back on the first
    /// of them.
    ///
    /// The cursor RESETS rather than trying to follow its old row: the
    /// list it was standing in no longer exists, so there is nothing
    /// honest to follow. Typing narrows toward the top, which is where
    /// the eye already is.
    fn refilter(&mut self) {
        self.matches = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| matches_query(&entry.label, &self.query))
            .map(|(index, _)| index)
            .collect();
        self.cursor = FocusColumn::new(self.matches.len());
    }
}

/// Case-insensitive SUBSEQUENCE matching: the typed characters must
/// appear in order, but need not be adjacent, so `hcl` finds `HANDCLAP`.
///
/// Subsequence rather than substring because it is worth more per
/// keystroke — each character can cut the field by more than a prefix
/// can, which is the whole reason to type instead of scroll.
pub fn matches_query(label: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut wanted = query.chars().flat_map(char::to_lowercase).peekable();
    for have in label.chars().flat_map(char::to_lowercase) {
        if wanted.peek().is_some_and(|want| *want == have) {
            wanted.next();
        }
    }
    wanted.peek().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_query_keeps_the_whole_library() {
        let browser = Browser::shelves();
        assert_eq!(browser.matches().count(), Shelf::ALL.len());
        assert_eq!(browser.cursor(), Some(0));
    }

    #[test]
    fn typing_narrows_by_subsequence_not_by_prefix() {
        assert!(matches_query("HANDCLAP", "hcl"));
        assert!(matches_query("HANDCLAP", "HANDCLAP"));
        assert!(!matches_query("HANDCLAP", "hcx"));
        assert!(
            !matches_query("HANDCLAP", "pl"),
            "order is part of the match"
        );
    }

    #[test]
    fn the_cursor_lands_on_the_first_survivor_as_the_query_narrows() {
        let mut browser = Browser::shelves();
        browser.type_char('s');
        assert_eq!(browser.cursor(), Some(0));
        assert!(
            browser
                .matches()
                .all(|entry| matches_query(&entry.label, "s"))
        );
    }

    #[test]
    fn a_query_that_matches_nothing_leaves_nowhere_to_stand() {
        let mut browser = Browser::shelves();
        for ch in "zzzz".chars() {
            browser.type_char(ch);
        }
        assert!(browser.is_empty());
        assert_eq!(browser.cursor(), None);
        assert_eq!(browser.selected(), None);
        assert!(!browser.step(Step::Down), "an empty list refuses to move");
    }

    #[test]
    fn backspace_widens_again_and_reports_when_there_is_nothing_left() {
        let mut browser = Browser::shelves();
        browser.type_char('z');
        assert!(browser.is_empty());

        assert!(browser.backspace());
        assert_eq!(browser.matches().count(), Shelf::ALL.len());
        assert!(!browser.backspace(), "an empty query says so");
    }

    #[test]
    fn selection_follows_the_cursor_through_the_surviving_rows() {
        let mut browser = Browser::shelves();
        assert_eq!(
            browser.selected().map(|e| e.label.as_str()),
            Some("DEVICES")
        );
        assert!(browser.step(Step::Down));
        assert_eq!(
            browser.selected().map(|e| e.label.as_str()),
            Some("SAMPLES")
        );
    }

    #[test]
    fn the_device_shelf_is_the_registry_split_instruments_then_effects() {
        let entries = device_entries();
        assert_eq!(entries.len(), DEVICES.len());

        let kinds: Vec<_> = entries
            .iter()
            .map(|entry| match entry.kind {
                EntryKind::Device(kind) => kind,
                _ => panic!("the device shelf contained a non-device"),
            })
            .collect();
        assert!(
            kinds
                .windows(2)
                .all(|pair| pair[0].is_instrument() || !pair[1].is_instrument()),
            "an instrument appeared after the effects began"
        );
        for spec in DEVICES {
            assert_eq!(
                kinds.iter().filter(|kind| **kind == spec.kind).count(),
                1,
                "{:?} is missing or duplicated",
                spec.kind
            );
        }
    }

    #[test]
    fn samples_are_built_from_snapshot_records_without_touching_disk() {
        let record = AssetRecord {
            path: PathBuf::from("/library/drums/kick.wav"),
            relative_path: PathBuf::from("drums/kick.wav"),
            location_id: "library".to_owned(),
            name: "kick".to_owned(),
            extension: "wav".to_owned(),
            bytes: 42,
            modified_unix_secs: None,
            tags: vec!["drum".to_owned()],
        };
        let entries = sample_entries(std::slice::from_ref(&record));
        assert_eq!(entries[0].label, "drums/kick.wav");
        assert_eq!(entries[0].kind, EntryKind::Sample(record.path));
    }

    #[test]
    fn escape_climbs_from_a_shelf_to_the_fixed_top_level() {
        let mut browser = Browser::shelves();
        browser.enter_shelf(Shelf::Devices, device_entries(), BrowserStatus::Ready);
        browser.type_char('s');

        assert!(browser.ascend());
        assert_eq!(browser.shelf(), None);
        assert_eq!(browser.query(), "");
        assert_eq!(browser.matches().count(), Shelf::ALL.len());
        assert!(!browser.ascend(), "the browser root has no parent");
    }

    #[test]
    fn a_scan_can_land_without_erasing_the_filter() {
        let mut browser = Browser::shelves();
        browser.enter_shelf(Shelf::Samples, Vec::new(), BrowserStatus::Scanning);
        browser.type_char('k');
        assert_eq!(browser.status(), BrowserStatus::Scanning);

        browser.refresh_shelf(
            vec![Entry {
                label: "kick.wav".to_owned(),
                kind: EntryKind::Sample(PathBuf::from("/library/kick.wav")),
            }],
            BrowserStatus::Ready,
        );

        assert_eq!(browser.status(), BrowserStatus::Ready);
        assert_eq!(browser.query(), "k");
        assert_eq!(
            browser.selected().map(|entry| entry.label.as_str()),
            Some("kick.wav")
        );
    }

    #[test]
    fn projects_stay_an_explicit_empty_seam_until_a_neutral_catalog_exists() {
        assert!(project_entries().is_empty());
    }

    /// Every glyph the interface is built from is in the bundled font.
    /// A fallback glyph arrives at another weight and breaks the grid, so
    /// this is a layout guarantee rather than a typographic preference.
    #[test]
    fn the_glyph_vocabulary_stays_inside_what_terminus_carries() {
        for ch in [
            glyph::RULE,
            glyph::STILE,
            glyph::RULE_HEAVY,
            glyph::STILE_HEAVY,
            glyph::CORNER_TL,
            glyph::CORNER_TR,
            glyph::CORNER_BL,
            glyph::CORNER_BR,
            glyph::TEE_L,
            glyph::TEE_R,
            glyph::SHADE_LIGHT,
            glyph::SHADE_MID,
            glyph::SHADE_HEAVY,
            glyph::BLOCK,
            glyph::CARET,
            glyph::MARK,
            glyph::DOT,
            glyph::ARROW_R,
            glyph::ARROW_D,
        ] {
            let code = ch as u32;
            let covered = (0x2500..=0x2503).contains(&code)
                || (0x2508..=0x254b).contains(&code)
                || (0x2550..=0x2593).contains(&code)
                || (0x2596..=0x25a0).contains(&code)
                || code == 0x25ac
                || code == 0x25ae
                || (0x2190..=0x2195).contains(&code);
            assert!(covered, "{ch:?} (U+{code:04X}) is not in Terminus");
        }
    }
}
