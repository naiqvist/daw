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

use super::grid::Step;
use crate::devices::{DEVICES, DeviceKind, Family, Section};
use crate::library::AssetRecord;
use crate::ui::glyph::Glyph;
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
///
/// # Rules are no longer typed
///
/// The ruling and corner characters below are kept and checked, but the
/// browser pane's frame is DRAWN — see `Stage::draw_browser`. A border
/// made of characters is one glyph per cell: it seams at every cell
/// boundary and rides a baseline that is not the cell's centre, and at
/// this size the result reads as hatching rather than as a line. A
/// segment is one stroke, pixel-aligned, and identical on every machine.
/// `ui::glyph` reached the same conclusion for the family marks.
///
/// What is still typed is what a character is genuinely better at: marks
/// that occupy a cell in a run of text, like the caret and the shading.
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

    /// Blocks. `MARK` is the row cursor — filled, because the triangles
    /// are not in the font.
    pub const BLOCK: char = '█';
    pub const MARK: char = '▮';
    /// The typing prompt: a shell's `>`, leading whatever has been typed.
    /// A prompt rather than a caret, because the field is append-only —
    /// there is no insertion point to mark, only a place to type.
    pub const PROMPT: char = '>';
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
            Self::Devices => "Devices",
            Self::Samples => "Samples",
            Self::Projects => "Projects",
        }
    }
}

/// What a row actually is, once you press Enter on it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntryKind {
    /// A top-level shelf. A branch whose children may arrive later.
    Shelf(Shelf),
    /// A heading — a section or a family. It holds rows and does nothing
    /// else, so opening it is the only thing pressing it could honestly
    /// mean.
    Group,
    /// An instrument or effect, ready to head or join a chain.
    Device(DeviceKind),
    /// A file on disk that can be auditioned.
    Sample(PathBuf),
    /// A song that can be opened.
    Project(PathBuf),
}

/// One row of the library, and whatever hangs beneath it.
///
/// A tree rather than a stack of lists. The library's SHAPE is
/// information — that a reverb is filed under DELAY & REVERB is worth
/// knowing while looking at it — and a drill-down hides every heading
/// except the one being stood in, so the reader has to remember the
/// structure instead of reading it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    pub label: String,
    pub kind: EntryKind,
    pub children: Vec<Node>,
    /// Whether the children are drawn. Everything starts closed: the top
    /// of the library has to fit in a glance, or the tree has bought
    /// structure at the cost of the thing structure is for.
    pub expanded: bool,
    /// A heading's own mark: a miniature of what the family DOES.
    ///
    /// An icon, in the semiotic sense — a sign that resembles its
    /// subject — where every other row here is a symbol, a word you have
    /// to have learned. In a list scanned a hundred times a day, a shape
    /// is recognised before a word is read, and the word underneath
    /// becomes confirmation rather than information.
    ///
    /// Leaves carry none: the heading above already said it, and a mark
    /// repeated on every row beneath it would be ornament by the
    /// subtraction test.
    pub mark: Option<Glyph>,
}

impl Node {
    pub fn leaf(label: impl Into<String>, kind: EntryKind) -> Self {
        Self {
            label: label.into(),
            kind,
            children: Vec::new(),
            expanded: false,
            mark: None,
        }
    }

    pub fn branch(label: impl Into<String>, kind: EntryKind, children: Vec<Node>) -> Self {
        Self {
            label: label.into(),
            kind,
            children,
            expanded: false,
            mark: None,
        }
    }

    /// Give a heading the mark of what it holds.
    pub fn marked(mut self, mark: Glyph) -> Self {
        self.mark = Some(mark);
        self
    }

    /// How many LEAVES hang beneath this row, at any depth.
    ///
    /// Leaves rather than immediate children because the reader's
    /// question is how much is in there, not how many doors are between
    /// them and it — and a heading holding three headings holding one
    /// device each is a small place wearing a large number.
    pub fn leaves(&self) -> usize {
        if self.children.is_empty() {
            return usize::from(!self.is_branch());
        }
        self.children.iter().map(Node::leaves).sum()
    }

    /// Whether this row opens. A shelf opens even while empty: its
    /// children are scanned rather than known, and a shelf that looked
    /// like a leaf until the scan landed would change shape under the
    /// cursor.
    pub fn is_branch(&self) -> bool {
        !self.children.is_empty() || matches!(self.kind, EntryKind::Shelf(_))
    }
}

/// Whether the rows under a shelf are complete.
///
/// This is display truth, not progress theatre: the scanner either still
/// owns the answer, has delivered it, or no neutral source exists yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BrowserStatus {
    Ready,
    Scanning,
    Unavailable,
}

/// The mark a family draws: the picture its own cards already draw,
/// shrunk to a cell.
///
/// Lives here rather than on `Family` so the registry stays a model: what
/// a device IS belongs in `devices`, what it LOOKS like belongs in a
/// frame's design system.
pub(super) fn family_mark(family: Family) -> Glyph {
    match family {
        Family::Synths => Glyph::Saw,
        Family::Drums => Glyph::Transient,
        Family::Sampling => Glyph::Sample,
        Family::Dynamics => Glyph::Dynamics,
        Family::EqAndFilters => Glyph::Filter,
        Family::DelayAndReverb => Glyph::Time,
        Family::Distortion => Glyph::Drive,
        Family::Modulation => Glyph::Modulation,
        Family::Spectral => Glyph::Spectral,
        Family::Utilities => Glyph::Utility,
    }
}

/// A section holds families rather than being one, so its mark is the one
/// with no signal in it.
fn section_mark(section: Section) -> Glyph {
    match section {
        Section::Instruments => Glyph::Instrument,
        Section::AudioEffects => Glyph::Stack,
    }
}

/// The device shelf, as the registry files it.
///
/// Built by walking `Section` and `Family` rather than from a list kept
/// here: the registry is the only catalog, so adding a device there makes
/// it browsable — and filed correctly — without touching this.
pub(super) fn device_nodes() -> Vec<Node> {
    Section::ALL
        .into_iter()
        .map(|section| {
            let families = Family::ALL
                .into_iter()
                .filter(|family| family.section() == section)
                .map(|family| {
                    let devices = DEVICES
                        .iter()
                        .filter(|spec| spec.family == family)
                        .map(|spec| {
                            // The registry already writes these as names
                            // a reader would say out loud — "808 hat",
                            // "poly synth" — so they are shown as written.
                            // Shouting them was the browser's own
                            // decoration, not the catalog's.
                            Node::leaf(spec.name, EntryKind::Device(spec.kind))
                        })
                        .collect();
                    Node::branch(family.label(), EntryKind::Group, devices)
                        .marked(family_mark(family))
                })
                .collect();
            Node::branch(section.label(), EntryKind::Group, families).marked(section_mark(section))
        })
        .collect()
}

/// One immutable scanner record becomes one row. No filesystem work
/// happens here; the path was already resolved by the green-zone service.
pub(super) fn sample_nodes(assets: &[AssetRecord]) -> Vec<Node> {
    assets
        .iter()
        .map(|asset| {
            Node::leaf(
                asset.relative_path.to_string_lossy().into_owned(),
                EntryKind::Sample(asset.path.clone()),
            )
        })
        .collect()
}

/// The songs in the stage's own folder, by name. `None` — a stage with
/// no songs folder — is an empty shelf, and the seam says so here rather
/// than being an anonymous empty vector: guessing a directory would turn
/// a machine-local convention into an accidental data model.
///
/// One directory, read when the browser is summoned. Green zone, and a
/// folder of songs is a few dozen names; a library of them is the
/// scanner's job, the day there is one.
pub(super) fn project_nodes(home: Option<&std::path::Path>) -> Vec<Node> {
    let Some(home) = home else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(home) else {
        return Vec::new();
    };
    let mut songs: Vec<(String, PathBuf)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            name.ends_with(".stage.ron") || name.ends_with(".daw.ron")
        })
        .map(|path| (super::document::title(&path), path))
        .collect();
    songs.sort();
    songs
        .into_iter()
        .map(|(title, path)| Node::leaf(title, EntryKind::Project(path)))
        .collect()
}

/// Where one visible row sits in the tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    /// Child indices from the root down. The address the cursor holds,
    /// so opening or closing a branch above cannot silently re-point it.
    pub path: Vec<usize>,
    pub depth: usize,
}

/// The library, what has been typed, and where the cursor is standing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Browser {
    roots: Vec<Node>,
    /// What has been typed. Never shown as state text — it is shown
    /// because it is being written, which is a different thing.
    query: String,
    /// Index into the currently visible rows.
    cursor: usize,
    /// Per shelf, because they complete independently: a finished device
    /// list says nothing about whether the sample scan has landed.
    status: [BrowserStatus; 3],
}

impl Browser {
    /// The top of the library: three closed shelves, nothing scanned yet.
    pub fn shelves() -> Self {
        let roots = Shelf::ALL
            .into_iter()
            .map(|shelf| {
                let children = match shelf {
                    Shelf::Devices => device_nodes(),
                    // Filled by the scanner when it lands.
                    Shelf::Samples => Vec::new(),
                    // Filled by the stage when it summons the browser,
                    // from the songs folder it was given.
                    Shelf::Projects => Vec::new(),
                };
                Node::branch(shelf.label(), EntryKind::Shelf(shelf), children)
            })
            .collect();
        Self {
            roots,
            query: String::new(),
            cursor: 0,
            status: [
                BrowserStatus::Ready,
                BrowserStatus::Scanning,
                BrowserStatus::Unavailable,
            ],
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub(super) fn status_of(&self, shelf: Shelf) -> BrowserStatus {
        self.status[shelf as usize]
    }

    /// Replace one shelf's children when a scan lands. What was typed
    /// stays the filter on the new truth, and the shelf keeps whether it
    /// was open — a scan is not a navigation event.
    pub(super) fn set_children(
        &mut self,
        shelf: Shelf,
        children: Vec<Node>,
        status: BrowserStatus,
    ) {
        self.status[shelf as usize] = status;
        if let Some(node) = self
            .roots
            .iter_mut()
            .find(|node| node.kind == EntryKind::Shelf(shelf))
        {
            node.children = children;
        }
        self.clamp_cursor();
    }

    /// Every row the tree currently shows, in reading order.
    ///
    /// While a query is live the tree opens itself along every path that
    /// leads to a match: a filter that left its results hidden inside
    /// closed branches would be reporting a count the reader cannot
    /// reach.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for (index, node) in self.roots.iter().enumerate() {
            self.collect(node, vec![index], 0, &mut rows);
        }
        rows
    }

    fn collect(&self, node: &Node, path: Vec<usize>, depth: usize, rows: &mut Vec<Row>) {
        if !self.kept(node) {
            return;
        }
        rows.push(Row {
            path: path.clone(),
            depth,
        });
        let open = node.expanded || !self.query.is_empty();
        if !open {
            return;
        }
        for (index, child) in node.children.iter().enumerate() {
            let mut child_path = path.clone();
            child_path.push(index);
            self.collect(child, child_path, depth + 1, rows);
        }
    }

    /// A row survives if it matches, or if anything beneath it does — so
    /// the headings that lead to a match come with it.
    fn kept(&self, node: &Node) -> bool {
        matches_query(&node.label, &self.query)
            || node.children.iter().any(|child| self.kept(child))
    }

    pub fn node_at(&self, path: &[usize]) -> Option<&Node> {
        let mut nodes = &self.roots;
        let mut found = None;
        for index in path {
            let node = nodes.get(*index)?;
            nodes = &node.children;
            found = Some(node);
        }
        found
    }

    fn node_at_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        let (first, rest) = path.split_first()?;
        let mut node = self.roots.get_mut(*first)?;
        for index in rest {
            node = node.children.get_mut(*index)?;
        }
        Some(node)
    }

    pub fn is_empty(&self) -> bool {
        self.rows().is_empty()
    }

    pub fn cursor(&self) -> Option<usize> {
        (!self.is_empty()).then_some(self.cursor)
    }

    /// The row the cursor is on, if the query left it anything to stand
    /// on.
    pub fn selected(&self) -> Option<&Node> {
        let rows = self.rows();
        let row = rows.get(self.cursor)?;
        self.node_at(&row.path)
    }

    fn selected_path(&self) -> Option<Vec<usize>> {
        self.rows().get(self.cursor).map(|row| row.path.clone())
    }

    /// Move, open, or climb. `false` means the tree had nowhere to go and
    /// the caller should SHOW that refusal rather than swallow it.
    pub fn step(&mut self, step: Step) -> bool {
        let rows = self.rows();
        if rows.is_empty() {
            return false;
        }
        match step {
            Step::Up => {
                if self.cursor == 0 {
                    return false;
                }
                self.cursor -= 1;
                true
            }
            Step::Down => {
                if self.cursor + 1 >= rows.len() {
                    return false;
                }
                self.cursor += 1;
                true
            }
            // Right OPENS, then walks in. Two presses to reach a child
            // rather than one, because opening and moving are different
            // events and a reader who only wanted to see inside should
            // not have lost their place doing it.
            Step::Right => {
                let Some(path) = self.selected_path() else {
                    return false;
                };
                let Some(node) = self.node_at_mut(&path) else {
                    return false;
                };
                if !node.is_branch() {
                    return false;
                }
                if !node.expanded {
                    node.expanded = true;
                    return true;
                }
                if node.children.is_empty() {
                    return false;
                }
                self.cursor += 1;
                true
            }
            // Left CLOSES, then climbs — the mirror of Right.
            Step::Left => {
                let Some(path) = self.selected_path() else {
                    return false;
                };
                let Some(node) = self.node_at_mut(&path) else {
                    return false;
                };
                if node.expanded && node.is_branch() {
                    node.expanded = false;
                    return true;
                }
                if path.len() < 2 {
                    return false;
                }
                let parent = &path[..path.len() - 1];
                let Some(index) = rows.iter().position(|row| row.path == parent) else {
                    return false;
                };
                self.cursor = index;
                true
            }
        }
    }

    /// Open or close the row the cursor is on. `false` means it is a leaf
    /// and there is nothing to open.
    pub fn toggle(&mut self) -> bool {
        let Some(path) = self.selected_path() else {
            return false;
        };
        let Some(node) = self.node_at_mut(&path) else {
            return false;
        };
        if !node.is_branch() {
            return false;
        }
        node.expanded = !node.expanded;
        true
    }

    /// Extend the query by one character.
    pub fn type_char(&mut self, ch: char) {
        self.query.push(ch);
        self.cursor = 0;
    }

    /// Retract the last character. `false` means there was nothing to
    /// retract, which the caller reports rather than swallows.
    pub fn backspace(&mut self) -> bool {
        let popped = self.query.pop().is_some();
        if popped {
            self.cursor = 0;
        }
        popped
    }

    /// How many leaves the query has left standing.
    ///
    /// The yield of what has been typed. A keystroke is worth spending
    /// only if it removes uncertainty, and this is the only place the
    /// reader can see whether the last one did.
    pub fn surviving_leaves(&self) -> usize {
        self.rows()
            .iter()
            .filter_map(|row| self.node_at(&row.path))
            .filter(|node| !node.is_branch())
            .count()
    }

    fn clamp_cursor(&mut self) {
        let rows = self.rows().len();
        self.cursor = self.cursor.min(rows.saturating_sub(1));
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

/// Which characters of `label` the query actually consumed.
///
/// The filter is a SUBSEQUENCE, so why a row survived is not obvious from
/// looking at it — `hcl` keeping `HANDCLAP` is a claim the reader has to
/// take on trust unless the match is shown. Returned parallel to the
/// label's characters.
///
/// Matched greedily and leftmost, exactly as [`matches_query`] consumes
/// them, so the marks can never disagree with the filter that produced
/// them.
pub fn match_positions(label: &str, query: &str) -> Vec<bool> {
    let mut wanted = query.chars().flat_map(char::to_lowercase).peekable();
    label
        .chars()
        .map(|have| {
            let hit = have
                .to_lowercase()
                .next()
                .is_some_and(|lowered| wanted.peek().is_some_and(|want| *want == lowered));
            if hit {
                wanted.next();
            }
            hit
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_heading_in_the_device_tree_carries_a_mark() {
        for section in device_nodes() {
            assert!(
                section.mark.is_some(),
                "the section {} has no mark",
                section.label
            );
            for family in &section.children {
                assert!(
                    family.mark.is_some(),
                    "the family {} has no mark",
                    family.label
                );
                for device in &family.children {
                    assert!(
                        device.mark.is_none(),
                        "the leaf {} repeats its family's mark",
                        device.label
                    );
                }
            }
        }
    }

    #[test]
    fn no_two_families_are_given_the_same_mark() {
        let marks: Vec<_> = Family::ALL.into_iter().map(family_mark).collect();
        for (index, mark) in marks.iter().enumerate() {
            for other in &marks[index + 1..] {
                assert_ne!(
                    mark, other,
                    "two families draw the same mark, so one sign means two things"
                );
            }
        }
    }

    #[test]
    fn a_section_never_wears_a_familys_mark() {
        let families: Vec<_> = Family::ALL.into_iter().map(family_mark).collect();
        for section in Section::ALL {
            assert!(
                !families.contains(&section_mark(section)),
                "the section {} borrowed a family's mark",
                section.label()
            );
        }
    }

    #[test]
    fn a_branch_counts_the_leaves_beneath_it_not_the_doors() {
        let browser = Browser::shelves();
        let devices = browser.node_at(&[0]).expect("the devices shelf");
        assert_eq!(
            devices.leaves(),
            DEVICES.len(),
            "the shelf miscounted what it holds"
        );

        let instruments = browser.node_at(&[0, 0]).expect("the instruments section");
        assert_eq!(
            instruments.leaves(),
            DEVICES.iter().filter(|spec| spec.instrument).count()
        );
    }

    #[test]
    fn an_empty_shelf_counts_nothing() {
        let browser = Browser::shelves();
        let projects = browser.node_at(&[2]).expect("the projects shelf");
        assert_eq!(projects.leaves(), 0);
    }

    #[test]
    fn the_yield_counts_what_the_query_left_standing() {
        let mut browser = Browser::shelves();
        assert_eq!(
            browser.surviving_leaves(),
            0,
            "a closed library reported leaves nobody can see"
        );

        for ch in "reverb".chars() {
            browser.type_char(ch);
        }
        assert!(
            browser.surviving_leaves() > 0,
            "the filter kept rows but reported no yield"
        );
        assert!(
            browser.surviving_leaves() < DEVICES.len(),
            "the filter reported the whole library as surviving"
        );
    }

    #[test]
    fn the_marked_characters_are_the_ones_the_filter_consumed() {
        let marks = match_positions("handclap", "hcl");
        assert_eq!(marks.iter().filter(|hit| **hit).count(), 3);
        // h-a-n-d-c-l-a-p: the h, then the first c, then the l.
        assert_eq!(marks, [true, false, false, false, true, true, false, false]);
    }

    #[test]
    fn nothing_is_marked_when_nothing_was_typed() {
        assert!(match_positions("reverb", "").iter().all(|hit| !hit));
    }

    #[test]
    fn a_row_that_survives_on_its_children_marks_none_of_itself() {
        // DELAY & REVERB survives the query `reverb` because a child does,
        // not because it matches — and the marks must not imply otherwise.
        let marks = match_positions("Delay & Reverb", "zzz");
        assert!(marks.iter().all(|hit| !hit));
    }

    /// Walk to a row by label, so a test says what it is standing on
    /// rather than counting rows that a taxonomy change would renumber.
    fn walk_to(browser: &mut Browser, labels: &[&str]) {
        for label in labels {
            let target = browser
                .rows()
                .iter()
                .position(|row| {
                    browser
                        .node_at(&row.path)
                        .is_some_and(|node| node.label == *label)
                })
                .unwrap_or_else(|| panic!("no row labelled {label:?}"));
            let here = browser.cursor().expect("the tree had nothing to stand on");
            let step = if target > here { Step::Down } else { Step::Up };
            for _ in 0..here.abs_diff(target) {
                assert!(browser.step(step), "ran out of rows reaching {label:?}");
            }
            if browser
                .selected()
                .is_some_and(|node| node.is_branch() && !node.expanded)
            {
                assert!(browser.step(Step::Right), "{label:?} would not open");
            }
        }
    }

    #[test]
    fn the_library_opens_closed_on_its_three_shelves() {
        let browser = Browser::shelves();
        let labels: Vec<_> = browser
            .rows()
            .iter()
            .map(|row| browser.node_at(&row.path).expect("row").label.clone())
            .collect();
        assert_eq!(labels, ["Devices", "Samples", "Projects"]);
    }

    #[test]
    fn right_opens_a_branch_before_it_walks_into_one() {
        let mut browser = Browser::shelves();
        assert!(browser.step(Step::Right), "DEVICES would not open");
        assert_eq!(
            browser.cursor(),
            Some(0),
            "opening a branch also moved the cursor"
        );
        assert_eq!(
            browser.selected().map(|node| node.label.as_str()),
            Some("Devices")
        );

        assert!(
            browser.step(Step::Right),
            "the open branch would not be entered"
        );
        assert_eq!(
            browser.selected().map(|node| node.label.as_str()),
            Some("Instruments")
        );
    }

    #[test]
    fn left_closes_a_branch_before_it_climbs_out_of_one() {
        let mut browser = Browser::shelves();
        walk_to(&mut browser, &["Devices", "Instruments", "Synths"]);
        assert!(browser.step(Step::Left), "SYNTHS would not close");
        assert_eq!(
            browser.selected().map(|node| node.label.as_str()),
            Some("Synths"),
            "closing a branch also moved the cursor"
        );

        assert!(browser.step(Step::Left), "the cursor would not climb");
        assert_eq!(
            browser.selected().map(|node| node.label.as_str()),
            Some("Instruments")
        );
    }

    #[test]
    fn the_root_refuses_to_climb_any_further() {
        let mut browser = Browser::shelves();
        assert!(
            !browser.step(Step::Left),
            "a closed root pretended there was somewhere above it"
        );
        assert!(!browser.step(Step::Up), "the first row stepped up");
    }

    #[test]
    fn a_leaf_refuses_to_open() {
        let mut browser = Browser::shelves();
        walk_to(
            &mut browser,
            &["Devices", "Instruments", "Synths", "poly synth"],
        );
        assert_eq!(
            browser.selected().map(|node| node.label.as_str()),
            Some("poly synth")
        );
        assert!(!browser.step(Step::Right), "a device opened like a folder");
        assert!(!browser.toggle(), "a device toggled like a folder");
    }

    #[test]
    fn the_device_tree_is_the_registry_filed_by_its_own_headings() {
        let mut browser = Browser::shelves();
        walk_to(&mut browser, &["Devices"]);
        let sections: Vec<_> = browser
            .rows()
            .iter()
            .filter(|row| row.depth == 1)
            .map(|row| browser.node_at(&row.path).expect("row").label.clone())
            .collect();
        assert_eq!(sections, ["Instruments", "Audio Effects"]);

        walk_to(&mut browser, &["Audio Effects"]);
        let families: Vec<_> = browser
            .rows()
            .iter()
            .filter(|row| row.depth == 2 && row.path[1] == 1)
            .map(|row| browser.node_at(&row.path).expect("row").label.clone())
            .collect();
        assert_eq!(
            families,
            [
                "Dynamics",
                "EQ & Filters",
                "Delay & Reverb",
                "Distortion",
                "Modulation",
                "Spectral",
                "Utilities"
            ]
        );
    }

    #[test]
    fn every_registered_device_is_reachable_in_the_tree() {
        let nodes = device_nodes();
        let mut found = 0;
        for section in &nodes {
            for family in &section.children {
                found += family.children.len();
                assert!(
                    !family.children.is_empty(),
                    "the family {} is a heading over nothing",
                    family.label
                );
            }
        }
        assert_eq!(found, DEVICES.len(), "a device is missing from the tree");
    }

    #[test]
    fn an_empty_query_shows_only_what_is_open() {
        let browser = Browser::shelves();
        assert_eq!(browser.rows().len(), Shelf::ALL.len());
    }

    #[test]
    fn typing_opens_the_tree_along_every_path_to_a_match() {
        let mut browser = Browser::shelves();
        for ch in "reverb".chars() {
            browser.type_char(ch);
        }
        let labels: Vec<_> = browser
            .rows()
            .iter()
            .map(|row| browser.node_at(&row.path).expect("row").label.clone())
            .collect();
        assert!(
            labels.iter().any(|label| label == "reverb"),
            "a match stayed hidden inside a closed branch: {labels:?}"
        );
        assert!(
            labels.iter().any(|label| label == "Delay & Reverb"),
            "the heading leading to the match was dropped: {labels:?}"
        );
        assert!(
            !labels.iter().any(|label| label == "poly synth"),
            "the filter kept a row that does not match: {labels:?}"
        );
    }

    #[test]
    fn typing_narrows_by_subsequence_not_by_prefix() {
        assert!(matches_query("HANDCLAP", "hcl"));
        assert!(matches_query("HANDCLAP", "clap"));
        assert!(!matches_query("HANDCLAP", "xyz"));
    }

    #[test]
    fn a_query_that_matches_nothing_leaves_nowhere_to_stand() {
        let mut browser = Browser::shelves();
        for ch in "zzzz".chars() {
            browser.type_char(ch);
        }
        assert!(browser.is_empty());
        assert_eq!(browser.cursor(), None);
        assert!(browser.selected().is_none());
        assert!(!browser.step(Step::Down), "an empty tree stepped");
    }

    #[test]
    fn backspace_widens_again_and_reports_when_there_is_nothing_left() {
        let mut browser = Browser::shelves();
        // `z` alone survives — HAZE carries one — so the query has to be
        // two characters to actually empty the tree.
        browser.type_char('z');
        browser.type_char('q');
        assert!(browser.is_empty(), "zq matched something");

        assert!(browser.backspace());
        assert!(!browser.is_empty(), "the tree did not come back");

        assert!(browser.backspace());
        assert!(
            !browser.backspace(),
            "backspace claimed to erase an empty query"
        );
    }

    #[test]
    fn a_scan_lands_on_a_closed_shelf_without_disturbing_the_cursor() {
        let mut browser = Browser::shelves();
        walk_to(&mut browser, &["Devices"]);
        let standing_on = browser.selected().map(|node| node.label.clone());

        browser.set_children(
            Shelf::Samples,
            vec![Node::leaf(
                "kick.wav",
                EntryKind::Sample(PathBuf::from("/k.wav")),
            )],
            BrowserStatus::Ready,
        );

        assert_eq!(
            browser.selected().map(|node| node.label.clone()),
            standing_on,
            "a scan moved the cursor"
        );
        assert_eq!(browser.status_of(Shelf::Samples), BrowserStatus::Ready);
    }

    #[test]
    fn samples_are_built_from_snapshot_records_without_touching_disk() {
        let assets = [AssetRecord {
            path: PathBuf::from("/library/kick.wav"),
            relative_path: PathBuf::from("kick.wav"),
            location_id: "library".to_owned(),
            name: "kick.wav".to_owned(),
            extension: "wav".to_owned(),
            bytes: 0,
            modified_unix_secs: None,
            tags: Vec::new(),
        }];
        let nodes = sample_nodes(&assets);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].label, "kick.wav");
        assert!(!nodes[0].is_branch(), "a sample opened like a folder");
    }

    #[test]
    fn the_songs_folder_lists_its_songs_by_name_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("daw-stage-shelf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a folder");
        for name in ["zed.stage.ron", "alpha.daw.ron", "notes.txt", "loose.ron"] {
            std::fs::write(dir.join(name), "").expect("writes");
        }
        let nodes = project_nodes(Some(&dir));
        let labels: Vec<&str> = nodes.iter().map(|node| node.label.as_str()).collect();
        assert_eq!(
            labels,
            ["alpha", "zed"],
            "the shelf is not the songs, sorted"
        );
        assert!(
            matches!(nodes[0].kind, EntryKind::Project(ref path) if path.ends_with("alpha.daw.ron"))
        );
        assert!(project_nodes(Some(&dir.join("missing"))).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn projects_stay_an_explicit_empty_seam_until_a_neutral_catalog_exists() {
        // With no songs folder there is nothing to list — and no
        // guessing at one.
        assert!(project_nodes(None).is_empty());
        let browser = Browser::shelves();
        assert_eq!(
            browser.status_of(Shelf::Projects),
            BrowserStatus::Unavailable,
            "the empty seam stopped saying why it is empty"
        );
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
            glyph::PROMPT,
            glyph::MARK,
            glyph::DOT,
            glyph::ARROW_R,
            glyph::ARROW_D,
        ] {
            let code = ch as u32;
            let covered = (0x20..=0x7e).contains(&code)
                || (0x2500..=0x2503).contains(&code)
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
