//! The LAB: a tiling section of the app, in the manner of a tiling
//! window manager. Its windows are LAB INSTRUMENTS; the first is the
//! KILN, whose engines will bake sound off the audio thread. This file
//! is the core — the layout tree, the windows, the kiln's state and the
//! verbs the keys speak — and names no egui. The view draws what it
//! reads here.
//!
//! The layout is a binary tree of splits over the unit square: a window
//! is a leaf; opening a window splits the focused leaf, across or down,
//! alternating with depth; closing a leaf hands its room to its
//! sibling. Every rectangle the view draws comes from [`Layout::places`].
//!
//! Two scopes hold the keys while the lab is up: LAB, the tiler
//! (Escape leaves the lab, Enter goes into the focused window), and the
//! focused instrument's own (KILN), from which Escape returns to the
//! tiler. The tiler's chords all carry Alt and work from either.
//!
//! Synthesis runs in crate::kiln; this module holds only lab state.

use super::{RefusalReason, Stage, Step};

/// A window's identity, minted once and never reused in a session.
pub(super) type WindowId = usize;

/// Which way a split lays its two children.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dir {
    /// Side by side: `a` on the left, `b` on the right.
    Across,
    /// One over the other: `a` above, `b` below.
    Down,
}

impl Dir {
    fn other(self) -> Self {
        match self {
            Self::Across => Self::Down,
            Self::Down => Self::Across,
        }
    }
}

/// One node of the layout tree.
#[derive(Clone, Debug, PartialEq)]
enum Node {
    Leaf(WindowId),
    Split {
        dir: Dir,
        /// `a`'s share of the room, 0.15..0.85.
        ratio: f32,
        a: usize,
        b: usize,
    },
}

/// A window's room, in the unit square.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Place {
    fn centre(&self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
}

/// The tree, as an arena so a node can be found by index.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Layout {
    nodes: Vec<Option<Node>>,
    root: Option<usize>,
}

const RATIO_MIN: f32 = 0.15;
const RATIO_MAX: f32 = 0.85;
/// @tune 0.02..0.2
const RESIZE_STEP: f32 = 0.05;

impl Layout {
    fn push(&mut self, node: Node) -> usize {
        if let Some(free) = self.nodes.iter().position(Option::is_none) {
            self.nodes[free] = Some(node);
            free
        } else {
            self.nodes.push(Some(node));
            self.nodes.len() - 1
        }
    }

    fn leaf_of(&self, id: WindowId) -> Option<usize> {
        self.nodes
            .iter()
            .position(|node| matches!(node, Some(Node::Leaf(other)) if *other == id))
    }

    fn parent_of(&self, index: usize) -> Option<usize> {
        self.nodes.iter().position(
            |node| matches!(node, Some(Node::Split { a, b, .. }) if *a == index || *b == index),
        )
    }

    fn depth_of(&self, index: usize) -> usize {
        let mut depth = 0;
        let mut at = index;
        while let Some(parent) = self.parent_of(at) {
            depth += 1;
            at = parent;
        }
        depth
    }

    /// Open `id` beside `beside` (the focused window), splitting its
    /// leaf across at even depths and down at odd ones; with no windows
    /// at all, `id` fills the room.
    pub(super) fn open(&mut self, beside: Option<WindowId>, id: WindowId) {
        let Some(target) = beside.and_then(|b| self.leaf_of(b)) else {
            let leaf = self.push(Node::Leaf(id));
            self.root = Some(leaf);
            return;
        };
        let dir = if self.depth_of(target) % 2 == 0 {
            Dir::Across
        } else {
            Dir::Down
        };
        // The leaf moves into a fresh slot and the split takes its
        // place. Pushed BEFORE the slot is overwritten, so the arena's
        // free-slot reuse cannot hand the leaf its own index.
        let old = self.nodes[target].clone().unwrap_or(Node::Leaf(id));
        let a = self.push(old);
        let b = self.push(Node::Leaf(id));
        self.nodes[target] = Some(Node::Split {
            dir,
            ratio: 0.5,
            a,
            b,
        });
    }

    /// Close `id`: its sibling takes the parent's room.
    pub(super) fn close(&mut self, id: WindowId) {
        let Some(leaf) = self.leaf_of(id) else { return };
        match self.parent_of(leaf) {
            None => {
                self.nodes[leaf] = None;
                self.root = None;
            }
            Some(parent) => {
                let Some(Node::Split { a, b, .. }) = self.nodes[parent].clone() else {
                    return;
                };
                let sibling = if a == leaf { b } else { a };
                let kept = self.nodes[sibling].take();
                self.nodes[leaf] = None;
                self.nodes[parent] = kept;
            }
        }
    }

    /// Every window's room, in the unit square, in tree order.
    pub(super) fn places(&self) -> Vec<(WindowId, Place)> {
        let mut out = Vec::new();
        if let Some(root) = self.root {
            self.walk(
                root,
                Place {
                    x: 0.0,
                    y: 0.0,
                    w: 1.0,
                    h: 1.0,
                },
                &mut out,
            );
        }
        out
    }

    fn walk(&self, index: usize, place: Place, out: &mut Vec<(WindowId, Place)>) {
        match &self.nodes[index] {
            Some(Node::Leaf(id)) => out.push((*id, place)),
            Some(Node::Split { dir, ratio, a, b }) => {
                let (pa, pb) = match dir {
                    Dir::Across => (
                        Place {
                            w: place.w * ratio,
                            ..place
                        },
                        Place {
                            x: place.x + place.w * ratio,
                            w: place.w * (1.0 - ratio),
                            ..place
                        },
                    ),
                    Dir::Down => (
                        Place {
                            h: place.h * ratio,
                            ..place
                        },
                        Place {
                            y: place.y + place.h * ratio,
                            h: place.h * (1.0 - ratio),
                            ..place
                        },
                    ),
                };
                self.walk(*a, pa, out);
                self.walk(*b, pb, out);
            }
            None => {}
        }
    }

    /// Grow (`true`) or shrink the room of `id` against its sibling.
    pub(super) fn resize(&mut self, id: WindowId, grow: bool) -> bool {
        let Some(leaf) = self.leaf_of(id) else {
            return false;
        };
        let Some(parent) = self.parent_of(leaf) else {
            return false;
        };
        let Some(Node::Split { ratio, a, .. }) = &mut self.nodes[parent] else {
            return false;
        };
        let step = if (*a == leaf) == grow {
            RESIZE_STEP
        } else {
            -RESIZE_STEP
        };
        let next = (*ratio + step).clamp(RATIO_MIN, RATIO_MAX);
        let moved = next != *ratio;
        *ratio = next;
        moved
    }

    /// Turn the split that holds `id` the other way.
    pub(super) fn toggle_dir(&mut self, id: WindowId) -> bool {
        let Some(leaf) = self.leaf_of(id) else {
            return false;
        };
        let Some(parent) = self.parent_of(leaf) else {
            return false;
        };
        if let Some(Node::Split { dir, .. }) = &mut self.nodes[parent] {
            *dir = dir.other();
            return true;
        }
        false
    }

    /// Exchange two windows' rooms.
    pub(super) fn swap(&mut self, x: WindowId, y: WindowId) {
        let (Some(lx), Some(ly)) = (self.leaf_of(x), self.leaf_of(y)) else {
            return;
        };
        self.nodes[lx] = Some(Node::Leaf(y));
        self.nodes[ly] = Some(Node::Leaf(x));
    }

    /// The window nearest `from` in the direction `step`, by the
    /// centres of their rooms.
    pub(super) fn neighbour(&self, from: WindowId, step: Step) -> Option<WindowId> {
        let places = self.places();
        let (_, here) = places.iter().find(|(id, _)| *id == from)?;
        let (cx, cy) = here.centre();
        places
            .iter()
            .filter(|(id, _)| *id != from)
            .filter_map(|(id, place)| {
                let (px, py) = place.centre();
                let (dx, dy) = (px - cx, py - cy);
                let ahead = match step {
                    Step::Left => dx < -1e-3 && dy.abs() <= here.h * 0.5 + place.h * 0.5,
                    Step::Right => dx > 1e-3 && dy.abs() <= here.h * 0.5 + place.h * 0.5,
                    Step::Up => dy < -1e-3 && dx.abs() <= here.w * 0.5 + place.w * 0.5,
                    Step::Down => dy > 1e-3 && dx.abs() <= here.w * 0.5 + place.w * 0.5,
                };
                ahead.then_some((*id, dx * dx + dy * dy))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    pub(super) fn window_ids(&self) -> Vec<WindowId> {
        self.places().into_iter().map(|(id, _)| id).collect()
    }
}

// ------------------------------------------------------------------ kiln --

pub use crate::kiln::params::{ENGINES, MEMBRANE_MACROS, MEMBRANE_SLIDERS};

/// Which band of the kiln's window has the keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Band {
    /// The engine strip: E switches the engine.
    Engine,
    /// The sixteen macros, two rows of eight.
    #[default]
    Macros,
    /// The hundred sliders, a list.
    Sliders,
}

impl Band {
    fn next(self) -> Self {
        match self {
            Self::Engine => Self::Macros,
            Self::Macros => Self::Sliders,
            Self::Sliders => Self::Engine,
        }
    }
}

/// The kiln's state: which engine, where the cursor is, the macros and
/// sliders as values, and the slider filter.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Kiln {
    pub engine: usize,
    pub band: Band,
    pub macro_at: usize,
    pub slider_at: usize,
    pub filter: String,
    pub filtering: bool,
    pub macros: [f32; 16],
    pub sliders: Vec<f32>,
    pub by_hand: Vec<bool>,
    pub job: Option<std::sync::Arc<crate::kiln::job::Job>>,
    pub key: Option<u64>,
    pub submitted: Option<crate::kiln::job::Action>,
    pub changed: Option<std::time::Instant>,
    pub render: Option<std::sync::Arc<crate::kiln::membrane::Render>>,
    pub printed: Option<std::path::PathBuf>,
    pub last_sent: Option<crate::sequencing::TrackId>,
    pub played: Option<std::time::Instant>,
    pub hear: bool,
    pub status: String,
    pub camera: [f32; 3],
    pub scrub: Option<f32>,
}

impl Default for Kiln {
    fn default() -> Self {
        Self {
            engine: 0,
            band: Band::Macros,
            macro_at: 0,
            slider_at: 0,
            filter: String::new(),
            filtering: false,
            macros: [0.5; 16],
            sliders: MEMBRANE_SLIDERS.iter().map(|s| s.default).collect(),
            by_hand: vec![false; MEMBRANE_SLIDERS.len()],
            job: None,
            key: None,
            submitted: None,
            changed: None,
            render: None,
            printed: None,
            last_sent: None,
            played: None,
            hear: false,
            status: "ready".into(),
            camera: [-0.6, 0.62, 4.6],
            scrub: None,
        }
    }
}

impl Kiln {
    pub(super) fn patch(&self) -> crate::kiln::Patch {
        crate::kiln::Patch {
            sliders: self.sliders.clone(),
            macros: self.macros,
            by_hand: self.by_hand.clone(),
        }
    }
    pub(super) fn set_patch(&mut self, p: crate::kiln::Patch) {
        self.sliders = p.sliders;
        self.macros = p.macros;
        self.by_hand = p.by_hand;
        self.edited();
    }
    fn edited(&mut self) {
        if let Some(job) = &self.job {
            job.cancel();
        }
        self.key = None;
        self.submitted = None;
        self.changed = Some(std::time::Instant::now());
        self.printed = None;
        self.hear = false;
        self.status = "preview queued".into();
    }
    /// The sliders the filter leaves, by index into [`MEMBRANE_SLIDERS`].
    pub(super) fn shown(&self) -> Vec<usize> {
        let needle = self.filter.to_lowercase();
        MEMBRANE_SLIDERS
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                needle.is_empty()
                    || s.name.to_lowercase().contains(&needle)
                    || s.group.to_lowercase().contains(&needle)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// A slider's reading, in its unit.
    pub(super) fn reading(&self, index: usize) -> String {
        let Some(def) = MEMBRANE_SLIDERS.get(index) else {
            return String::new();
        };
        let value = self.sliders.get(index).copied().unwrap_or(def.default);
        let digits = if def.max - def.min >= 100.0 {
            0
        } else if def.max - def.min >= 2.0 {
            1
        } else if def.max - def.min >= 0.02 {
            3
        } else {
            5
        };
        format!("{value:.digits$}{}", def.unit)
    }

    fn turn_slider(&mut self, index: usize, up: bool, coarse: bool) -> bool {
        let Some(def) = MEMBRANE_SLIDERS.get(index) else {
            return false;
        };
        let Some(value) = self.sliders.get_mut(index) else {
            return false;
        };
        let step = (def.max - def.min) * if coarse { 0.1 } else { 0.01 };
        let next = (*value + if up { step } else { -step }).clamp(def.min, def.max);
        let moved = next != *value;
        *value = next;
        if moved {
            self.by_hand.resize(self.sliders.len(), false);
            self.by_hand[index] = true;
            self.edited();
        }
        moved
    }

    fn turn_macro(&mut self, index: usize, up: bool, coarse: bool) -> bool {
        let Some(value) = self.macros.get_mut(index) else {
            return false;
        };
        let step = if coarse { 0.1 } else { 0.01 };
        let next = (*value + if up { step } else { -step }).clamp(0.0, 1.0);
        let moved = next != *value;
        *value = next;
        if moved {
            let mut p = self.patch();
            p.turn_macro(index, next);
            self.sliders = p.sliders;
            self.by_hand = p.by_hand;
            self.edited();
        }
        moved
    }
}

/// One kind of EXTENSION a lab window can hold.
///
/// The lab is a tiler of extensions, and this table is what they are:
/// a word, a line about it, and how one opens. The kiln and the MIDI
/// lab are two entries rather than two special cases, so a third is a
/// row here and nothing else — and the menu that offers them is the one
/// door all of them come through.
#[derive(Clone, Copy)]
pub(super) struct ExtensionKind {
    pub word: &'static str,
    /// What it is for, in a line. The menu reads this out.
    pub note: &'static str,
    /// How one arrives. Not a value to construct: the MIDI lab has to
    /// find or make its draft in the song first, and an extension knows
    /// how to open itself.
    pub open: fn(&mut Stage) -> Result<(), RefusalReason>,
}

pub(super) const EXTENSIONS: &[ExtensionKind] = &[
    ExtensionKind {
        word: "KILN",
        note: "bake a deep instrument: engines, macros, a hundred sliders",
        open: |stage| {
            let id = stage.lab.open_window(Instrument::Kiln(Kiln::default()));
            stage.lab.fullscreen = false;
            stage.notice = Some(format!("kiln {}", id + 1));
            Ok(())
        },
    },
    ExtensionKind {
        word: "MIDI LAB",
        note: "compose a pattern: harmony, motif and drums against a clip",
        open: |stage| {
            stage.open_midi_lab("");
            Ok(())
        },
    },
];

/// What a lab window holds.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Instrument {
    Kiln(Kiln),
    Midi(super::midi_lab::MidiLab),
}

impl Instrument {
    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::Kiln(_) => "KILN",
            Self::Midi(_) => "MIDI LAB",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct LabWindow {
    pub id: WindowId,
    pub instrument: Instrument,
}

/// The lab: the section, its windows and their layout.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Lab {
    pub midi_stop: bool,
    /// The lab is the field.
    pub open: bool,
    /// The focused window has the keys (else the tiler does).
    pub inside: bool,
    /// The focused window fills the field.
    pub fullscreen: bool,
    pub layout: Layout,
    pub windows: Vec<LabWindow>,
    pub focus: Option<WindowId>,
    /// The extension menu, and the row it is on. Open, it owns the
    /// keys — the window underneath must not answer them.
    pub menu: Option<usize>,
    next_id: WindowId,
}

impl Lab {
    pub(super) fn window(&self, id: WindowId) -> Option<&LabWindow> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub(super) fn window_mut(&mut self, id: WindowId) -> Option<&mut LabWindow> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    pub(super) fn focused(&self) -> Option<&LabWindow> {
        self.focus.and_then(|id| self.window(id))
    }

    pub(super) fn focused_kiln_mut(&mut self) -> Option<&mut Kiln> {
        let id = self.focus?;
        match &mut self.window_mut(id)?.instrument {
            Instrument::Kiln(kiln) => Some(kiln),
            Instrument::Midi(_) => None,
        }
    }

    /// The windows' rooms, in the unit square: the focused one alone
    /// when fullscreen.
    pub(super) fn places(&self) -> Vec<(WindowId, Place)> {
        if self.fullscreen
            && let Some(id) = self.focus
        {
            return vec![(
                id,
                Place {
                    x: 0.0,
                    y: 0.0,
                    w: 1.0,
                    h: 1.0,
                },
            )];
        }
        self.layout.places()
    }

    pub(super) fn open_window(&mut self, instrument: Instrument) -> WindowId {
        let id = self.next_id;
        self.next_id += 1;
        self.layout.open(self.focus, id);
        self.windows.push(LabWindow { id, instrument });
        self.focus = Some(id);
        id
    }
}

// ------------------------------------------------------------- the verbs --

impl Stage {
    /// Ctrl+Shift+L, or `:lab`: the lab takes the field, with a kiln
    /// if it has no window yet; again, and the field comes back.
    pub(super) fn toggle_lab(&mut self) -> Result<(), RefusalReason> {
        if self.lab.open {
            for w in &mut self.lab.windows {
                if let Instrument::Midi(m) = &mut w.instrument {
                    m.cancel();
                }
            }
            self.lab.midi_stop = true;
            self.lab.open = false;
            self.lab.inside = false;
            self.notice = Some("session".to_owned());
            return Ok(());
        }
        self.leave_rooms();
        self.chain = None;
        self.deck.open = false;
        self.matrix.open = false;
        self.lab.open = true;
        if self.lab.windows.is_empty() {
            // The lab opens with the first kind in it; Alt+Enter is
            // where any other one comes from.
            let _ = self.lab_open_window();
        }
        self.lab.inside = self.lab.focus.is_some();
        self.notice = Some("lab".to_owned());
        Ok(())
    }

    /// Escape: out of the window to the tiler, then out of the lab.
    pub(super) fn lab_escape(&mut self) -> Result<(), RefusalReason> {
        if self.lab.inside {
            if let Some(kiln) = self.lab.focused_kiln_mut()
                && kiln.filtering
            {
                kiln.filtering = false;
                kiln.filter.clear();
                kiln.slider_at = 0;
                return Ok(());
            }
            if let Some(k) = self.lab.focused_kiln_mut() {
                if matches!(
                    k.submitted,
                    Some(
                        crate::kiln::job::Action::Print
                            | crate::kiln::job::Action::Send
                            | crate::kiln::job::Action::Replace
                    )
                ) {
                    if let Some(job) = &k.job {
                        job.cancel();
                    }
                    k.submitted = None;
                    k.status = "cancelled".into();
                    return Ok(());
                }
            }
            self.lab.inside = false;
            return Ok(());
        }
        self.toggle_lab()
    }

    /// Enter at the tiler: into the focused window.
    pub(super) fn lab_enter(&mut self) -> Result<(), RefusalReason> {
        if self.lab.inside {
            return Err(RefusalReason::Empty);
        }
        if self.lab.focus.is_none() {
            // An empty lab opens the first kind rather than asking: the
            // menu is for choosing, and there is nothing here to choose
            // between yet.
            let _ = self.lab_open_window();
        }
        self.lab.inside = true;
        Ok(())
    }

    /// The kiln, opened the way the menu opens it. Kept as a verb of its
    /// own because the empty lab still wants one on Enter.
    pub(super) fn lab_open_window(&mut self) -> Result<(), RefusalReason> {
        match EXTENSIONS.first() {
            Some(kind) => (kind.open)(self),
            None => Err(RefusalReason::Empty),
        }
    }

    /// Alt+Enter: the menu of extensions, over whatever is there.
    pub(super) fn lab_menu_open(&mut self) -> Result<(), RefusalReason> {
        if self.lab.menu.is_some() {
            self.lab.menu = None;
            return Ok(());
        }
        self.lab.menu = Some(0);
        Ok(())
    }

    /// Up and down the menu, wrapping: a list this short should not have
    /// an edge to hit.
    pub(super) fn lab_menu_step(&mut self, step: Step) -> Result<(), RefusalReason> {
        let at = self.lab.menu.ok_or(RefusalReason::Empty)?;
        let count = EXTENSIONS.len().max(1);
        let next = match step {
            Step::Up | Step::Left => (at + count - 1) % count,
            Step::Down | Step::Right => (at + 1) % count,
        };
        self.lab.menu = Some(next);
        Ok(())
    }

    /// Enter, or the extension's own number: open it and close the menu.
    pub(super) fn lab_menu_pick(&mut self, at: usize) -> Result<(), RefusalReason> {
        let kind = EXTENSIONS.get(at).ok_or(RefusalReason::Empty)?;
        self.lab.menu = None;
        (kind.open)(self)
    }

    /// Escape: the menu goes, and nothing was opened.
    pub(super) fn lab_menu_close(&mut self) -> bool {
        self.lab.menu.take().is_some()
    }

    /// Alt and a number: the nth window takes the keys, whichever
    /// extension it holds. A tiler needs somewhere to jump to.
    pub(super) fn lab_focus_nth(&mut self, at: usize) -> Result<(), RefusalReason> {
        let ids = self.lab.layout.window_ids();
        let id = ids.get(at).copied().ok_or(RefusalReason::Empty)?;
        if self.lab.focus == Some(id) {
            // Already there: a refusal, not a change with nothing behind
            // it. The keys that move must be honest about not moving.
            return Err(RefusalReason::Empty);
        }
        self.lab.focus = Some(id);
        let word = self
            .lab
            .window(id)
            .map_or("", |window| window.instrument.name());
        self.notice = Some(format!("{word} {}", at + 1));
        Ok(())
    }

    /// Alt+Q: close the focused window; the neighbour takes focus.
    pub(super) fn lab_close_window(&mut self) -> Result<(), RefusalReason> {
        let Some(id) = self.lab.focus else {
            return Err(RefusalReason::Empty);
        };
        let next = [Step::Left, Step::Up, Step::Right, Step::Down]
            .into_iter()
            .find_map(|step| self.lab.layout.neighbour(id, step))
            .or_else(|| self.lab.layout.window_ids().into_iter().find(|w| *w != id));
        self.lab.layout.close(id);
        if let Some(window) = self.lab.window(id)
            && let Instrument::Kiln(k) = &window.instrument
        {
            if let Some(job) = &k.job {
                job.cancel();
            }
        }
        if let Some(window) = self.lab.window_mut(id)
            && let Instrument::Midi(m) = &mut window.instrument
        {
            m.cancel();
            self.lab.midi_stop = true;
        }
        self.lab.windows.retain(|w| w.id != id);
        self.lab.focus = next;
        self.lab.fullscreen = false;
        if next.is_none() {
            self.lab.inside = false;
        }
        Ok(())
    }

    /// Alt+H/J/K/L: focus the window in that direction.
    pub(super) fn lab_focus(&mut self, step: Step) -> Result<(), RefusalReason> {
        let Some(id) = self.lab.focus else {
            return Err(RefusalReason::Empty);
        };
        let Some(next) = self.lab.layout.neighbour(id, step) else {
            return Err(RefusalReason::Edge(step));
        };
        self.lab.focus = Some(next);
        Ok(())
    }

    /// Alt+Shift+H/J/K/L: swap rooms with the window in that direction.
    pub(super) fn lab_swap(&mut self, step: Step) -> Result<(), RefusalReason> {
        let Some(id) = self.lab.focus else {
            return Err(RefusalReason::Empty);
        };
        let Some(other) = self.lab.layout.neighbour(id, step) else {
            return Err(RefusalReason::Edge(step));
        };
        self.lab.layout.swap(id, other);
        Ok(())
    }

    pub(super) fn lab_fullscreen(&mut self) -> Result<(), RefusalReason> {
        if self.lab.focus.is_none() {
            return Err(RefusalReason::Empty);
        }
        self.lab.fullscreen = !self.lab.fullscreen;
        Ok(())
    }

    /// Alt+[ and Alt+]: the focused window's room, smaller or larger.
    pub(super) fn lab_resize(&mut self, grow: bool) -> Result<(), RefusalReason> {
        let Some(id) = self.lab.focus else {
            return Err(RefusalReason::Empty);
        };
        if self.lab.layout.resize(id, grow) {
            Ok(())
        } else {
            Err(RefusalReason::Edge(if grow {
                Step::Right
            } else {
                Step::Left
            }))
        }
    }

    pub(super) fn lab_toggle_split(&mut self) -> Result<(), RefusalReason> {
        let Some(id) = self.lab.focus else {
            return Err(RefusalReason::Empty);
        };
        if self.lab.layout.toggle_dir(id) {
            Ok(())
        } else {
            Err(RefusalReason::Empty)
        }
    }

    /// Alt+Space: the next window, in tree order, wrapping.
    pub(super) fn lab_cycle(&mut self) -> Result<(), RefusalReason> {
        let ids = self.lab.layout.window_ids();
        if ids.len() < 2 {
            return Err(RefusalReason::Empty);
        }
        let at = self
            .lab
            .focus
            .and_then(|id| ids.iter().position(|w| *w == id))
            .unwrap_or(0);
        self.lab.focus = Some(ids[(at + 1) % ids.len()]);
        Ok(())
    }

    // --- the kiln's own -------------------------------------------------

    /// Tab: the next band of the window.
    pub(super) fn kiln_band(&mut self) -> Result<(), RefusalReason> {
        let kiln = self.lab.focused_kiln_mut().ok_or(RefusalReason::Empty)?;
        kiln.band = kiln.band.next();
        Ok(())
    }

    /// The arrows: in the macros, Left and Right walk and Up and Down
    /// turn; in the sliders, Up and Down walk and Left and Right turn;
    /// on the engine strip, Left and Right switch the engine.
    pub(super) fn kiln_step(&mut self, step: Step, coarse: bool) -> Result<(), RefusalReason> {
        let kiln = self.lab.focused_kiln_mut().ok_or(RefusalReason::Empty)?;
        match kiln.band {
            Band::Engine => match step {
                Step::Left | Step::Right => {
                    let n = ENGINES.len();
                    kiln.engine = (kiln.engine + if step == Step::Right { 1 } else { n - 1 }) % n;
                    Ok(())
                }
                _ => Err(RefusalReason::Edge(step)),
            },
            Band::Macros => match step {
                Step::Left => {
                    if kiln.macro_at == 0 {
                        return Err(RefusalReason::Edge(step));
                    }
                    kiln.macro_at -= 1;
                    Ok(())
                }
                Step::Right => {
                    if kiln.macro_at + 1 >= 16 {
                        return Err(RefusalReason::Edge(step));
                    }
                    kiln.macro_at += 1;
                    Ok(())
                }
                Step::Up | Step::Down => {
                    let at = kiln.macro_at;
                    if kiln.turn_macro(at, step == Step::Up, coarse) {
                        Ok(())
                    } else {
                        Err(RefusalReason::Edge(step))
                    }
                }
            },
            Band::Sliders => {
                let shown = kiln.shown();
                if shown.is_empty() {
                    return Err(RefusalReason::Empty);
                }
                match step {
                    Step::Up => {
                        if kiln.slider_at == 0 {
                            return Err(RefusalReason::Edge(step));
                        }
                        kiln.slider_at -= 1;
                        Ok(())
                    }
                    Step::Down => {
                        if kiln.slider_at + 1 >= shown.len() {
                            return Err(RefusalReason::Edge(step));
                        }
                        kiln.slider_at += 1;
                        Ok(())
                    }
                    Step::Left | Step::Right => {
                        let index = shown[kiln.slider_at.min(shown.len() - 1)];
                        if kiln.turn_slider(index, step == Step::Right, coarse) {
                            Ok(())
                        } else {
                            Err(RefusalReason::Edge(step))
                        }
                    }
                }
            }
        }
    }

    /// A digit: the macro of that number, the band following.
    pub(super) fn kiln_macro(&mut self, n: u8) -> Result<(), RefusalReason> {
        let kiln = self.lab.focused_kiln_mut().ok_or(RefusalReason::Empty)?;
        let at = usize::from(n);
        if at >= 16 {
            return Err(RefusalReason::Unavailable);
        }
        if kiln.band == Band::Macros && kiln.macro_at == at {
            // Already here: nothing to change, and the key says so.
            return Err(RefusalReason::Empty);
        }
        kiln.band = Band::Macros;
        kiln.macro_at = at;
        Ok(())
    }

    /// `/`: the slider filter opens (and the band follows); `/` again
    /// closes it and shows every slider.
    pub(super) fn kiln_filter(&mut self) -> Result<(), RefusalReason> {
        let kiln = self.lab.focused_kiln_mut().ok_or(RefusalReason::Empty)?;
        kiln.band = Band::Sliders;
        kiln.filtering = !kiln.filtering;
        if !kiln.filtering {
            kiln.filter.clear();
        }
        kiln.slider_at = 0;
        Ok(())
    }

    /// A typed character while the filter is open.
    pub(super) fn kiln_type(&mut self, ch: char) -> Result<(), RefusalReason> {
        let kiln = self.lab.focused_kiln_mut().ok_or(RefusalReason::Empty)?;
        if !kiln.filtering || ch.is_control() {
            return Err(RefusalReason::Unavailable);
        }
        kiln.filter.push(ch);
        kiln.slider_at = 0;
        Ok(())
    }

    pub(super) fn kiln_backspace(&mut self) -> Result<(), RefusalReason> {
        let kiln = self.lab.focused_kiln_mut().ok_or(RefusalReason::Empty)?;
        if !kiln.filtering || kiln.filter.pop().is_none() {
            return Err(RefusalReason::Empty);
        }
        kiln.slider_at = 0;
        Ok(())
    }

    /// E: the next engine.
    pub(super) fn kiln_engine(&mut self) -> Result<(), RefusalReason> {
        let kiln = self.lab.focused_kiln_mut().ok_or(RefusalReason::Empty)?;
        let next = (kiln.engine + 1) % ENGINES.len();
        if next == kiln.engine && kiln.band == Band::Engine {
            let name = ENGINES[kiln.engine].0;
            self.notice = Some(format!("engine · {name} is the only one"));
            return Err(RefusalReason::Empty);
        }
        kiln.engine = next;
        kiln.band = Band::Engine;
        let name = ENGINES[kiln.engine].0;
        self.notice = Some(format!("engine · {name}"));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::stage::key::{Key, Mods};
    use crate::ui::stage::keymap::ScopeContext;
    use crate::ui::stage::{ApplyOutcome, StageIntent};

    fn lab() -> Stage {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        assert_eq!(
            stage.handle_key(Mods::COMMAND.plus(Mods::SHIFT), Key::L),
            Some(ApplyOutcome::Changed)
        );
        stage
    }

    fn alt(stage: &mut Stage, key: Key) -> Option<ApplyOutcome> {
        stage.handle_key(Mods::ALT, key)
    }

    fn sum_area(places: &[(WindowId, Place)]) -> f32 {
        places.iter().map(|(_, p)| p.w * p.h).sum()
    }

    #[test]
    fn a_tree_of_splits_tiles_the_unit_square_and_closes_back() {
        let mut layout = Layout::default();
        layout.open(None, 0);
        assert_eq!(layout.places().len(), 1);
        layout.open(Some(0), 1);
        let places = layout.places();
        assert_eq!(places.len(), 2);
        assert!((sum_area(&places) - 1.0).abs() < 1e-6);
        // Side by side first: both rooms are full height.
        assert!(places.iter().all(|(_, p)| (p.h - 1.0).abs() < 1e-6));
        layout.open(Some(1), 2);
        let places = layout.places();
        assert_eq!(places.len(), 3);
        assert!((sum_area(&places) - 1.0).abs() < 1e-6);
        // The second split, one level down, goes the other way.
        let third = places.iter().find(|(id, _)| *id == 2).unwrap().1;
        assert!(third.h < 0.99);
        layout.close(1);
        let places = layout.places();
        assert_eq!(places.len(), 2);
        assert!((sum_area(&places) - 1.0).abs() < 1e-6);
        layout.close(0);
        layout.close(2);
        assert!(layout.places().is_empty());
    }

    #[test]
    fn neighbours_resize_swap_and_split_direction() {
        let mut layout = Layout::default();
        layout.open(None, 0);
        layout.open(Some(0), 1);
        assert_eq!(layout.neighbour(0, Step::Right), Some(1));
        assert_eq!(layout.neighbour(1, Step::Left), Some(0));
        assert_eq!(layout.neighbour(0, Step::Left), None);
        assert!(layout.resize(0, true));
        let places = layout.places();
        let left = places.iter().find(|(id, _)| *id == 0).unwrap().1;
        assert!(left.w > 0.5);
        for _ in 0..20 {
            layout.resize(0, true);
        }
        assert!(!layout.resize(0, true), "grew past the limit");
        layout.swap(0, 1);
        assert_eq!(layout.neighbour(1, Step::Right), Some(0));
        assert!(layout.toggle_dir(0));
        assert_eq!(layout.neighbour(1, Step::Down), Some(0));
    }

    #[test]
    fn the_lab_opens_with_a_kiln_and_the_keys_follow() {
        let mut stage = lab();
        assert!(stage.lab.open && stage.lab.inside);
        assert_eq!(stage.scope_context(), ScopeContext::Kiln);
        assert_eq!(stage.lab.windows.len(), 1);
        // Escape: to the tiler, then out.
        let _ = stage.handle_key(Mods::NONE, Key::Escape);
        assert_eq!(stage.scope_context(), ScopeContext::Lab);
        let _ = stage.handle_key(Mods::NONE, Key::Enter);
        assert_eq!(stage.scope_context(), ScopeContext::Kiln);
        let _ = stage.handle_key(Mods::NONE, Key::Escape);
        let _ = stage.handle_key(Mods::NONE, Key::Escape);
        assert!(!stage.lab.open);
        assert_eq!(stage.scope_context(), ScopeContext::Root);
        // Back in, the window is still there.
        let _ = stage.handle_key(Mods::COMMAND.plus(Mods::SHIFT), Key::L);
        assert_eq!(stage.lab.windows.len(), 1);
        // The palette's statement is the same door.
        let _ = stage.handle_key(Mods::NONE, Key::Escape);
        let _ = stage.handle_key(Mods::NONE, Key::Escape);
        assert!(stage.apply_timeline_command("lab"));
        assert!(stage.lab.open);
    }

    /// The menu is the one door every extension comes through: Alt+Enter
    /// offers the kinds, a number or Enter opens one, Escape opens
    /// nothing. While it stands it owns the keys, so the window under it
    /// cannot answer them.
    #[test]
    fn the_extension_menu_offers_every_kind_and_opens_one() {
        let mut stage = lab();
        let before = stage.lab.windows.len();
        assert_eq!(alt(&mut stage, Key::Enter), Some(ApplyOutcome::Changed));
        assert_eq!(stage.lab.menu, Some(0));
        assert_eq!(stage.scope_context(), ScopeContext::LabMenu);
        assert_eq!(stage.lab.windows.len(), before, "the menu opened a window");

        // Down and up walk it, and it wraps at both ends.
        let _ = stage.handle_key(Mods::NONE, Key::ArrowDown);
        assert_eq!(stage.lab.menu, Some(1));
        for _ in 1..EXTENSIONS.len() {
            let _ = stage.handle_key(Mods::NONE, Key::ArrowDown);
        }
        assert_eq!(stage.lab.menu, Some(0), "the menu did not wrap");
        let _ = stage.handle_key(Mods::NONE, Key::ArrowUp);
        assert_eq!(stage.lab.menu, Some(EXTENSIONS.len() - 1));

        // Escape puts it away and opens nothing.
        let _ = stage.handle_key(Mods::NONE, Key::Escape);
        assert_eq!(stage.lab.menu, None);
        assert_eq!(stage.lab.windows.len(), before);
        assert!(stage.lab.open, "escaping the menu left the lab");

        // Enter opens the kind under the cursor.
        let _ = alt(&mut stage, Key::Enter);
        let _ = stage.handle_key(Mods::NONE, Key::Enter);
        assert_eq!(stage.lab.menu, None);
        assert_eq!(stage.lab.windows.len(), before + 1);
        assert!(matches!(
            stage.lab.focused().map(|w| &w.instrument),
            Some(Instrument::Kiln(_))
        ));
    }

    /// Every kind in the table opens something, and what it opens is the
    /// kind it named. A row that cannot open is a menu entry that lies.
    #[test]
    fn every_extension_in_the_table_opens_its_own_kind() {
        for (at, kind) in EXTENSIONS.iter().enumerate() {
            let mut stage = lab();
            let before = stage.lab.windows.len();
            let _ = alt(&mut stage, Key::Enter);
            // The number picks it outright, without walking.
            let digit = [Key::Num1, Key::Num2, Key::Num3, Key::Num4][at.min(3)];
            let outcome = stage.handle_key(Mods::NONE, digit);
            assert_eq!(
                outcome,
                Some(ApplyOutcome::Changed),
                "{} did not open",
                kind.word
            );
            assert_eq!(stage.lab.menu, None, "{} left the menu up", kind.word);
            assert!(
                stage.lab.windows.len() > before
                    || stage
                        .lab
                        .focused()
                        .is_some_and(|w| w.instrument.name() == kind.word),
                "{} opened no window of its own",
                kind.word
            );
            if let Some(window) = stage.lab.focused() {
                assert_eq!(
                    window.instrument.name(),
                    kind.word,
                    "{} opened a {}",
                    kind.word,
                    window.instrument.name()
                );
            }
        }
    }

    /// Alt and a number goes straight to a window, whatever it holds.
    #[test]
    fn a_number_reaches_the_nth_window() {
        let mut stage = lab();
        let _ = alt(&mut stage, Key::Enter);
        let _ = stage.handle_key(Mods::NONE, Key::Enter);
        assert_eq!(stage.lab.windows.len(), 2);
        assert_eq!(stage.lab.focus, Some(1));
        assert_eq!(
            stage.handle_key(Mods::ALT, Key::Num1),
            Some(ApplyOutcome::Changed)
        );
        assert_eq!(stage.lab.focus, Some(0));
        assert_eq!(
            stage.handle_key(Mods::ALT, Key::Num2),
            Some(ApplyOutcome::Changed)
        );
        assert_eq!(stage.lab.focus, Some(1));
        // A number with no window under it refuses rather than moving.
        assert!(matches!(
            stage.handle_key(Mods::ALT, Key::Num8),
            Some(ApplyOutcome::Refused(_))
        ));
        assert_eq!(stage.lab.focus, Some(1));
    }

    #[test]
    fn the_tiler_opens_focuses_swaps_and_closes_windows() {
        let mut stage = lab();
        assert_eq!(alt(&mut stage, Key::Enter), Some(ApplyOutcome::Changed));
        // The menu, then the kiln it offers first.
        assert_eq!(
            stage.handle_key(Mods::NONE, Key::Enter),
            Some(ApplyOutcome::Changed)
        );
        assert_eq!(stage.lab.windows.len(), 2);
        assert_eq!(stage.lab.focus, Some(1));
        assert_eq!(alt(&mut stage, Key::H), Some(ApplyOutcome::Changed));
        assert_eq!(stage.lab.focus, Some(0));
        assert!(matches!(
            alt(&mut stage, Key::H),
            Some(ApplyOutcome::Refused(_))
        ));
        assert_eq!(
            stage.handle_key(Mods::ALT.plus(Mods::SHIFT), Key::L),
            Some(ApplyOutcome::Changed)
        );
        // After the swap, window 0 is on the right.
        assert_eq!(stage.lab.layout.neighbour(0, Step::Left), Some(1));
        assert_eq!(alt(&mut stage, Key::F), Some(ApplyOutcome::Changed));
        assert_eq!(stage.lab.places().len(), 1);
        let _ = alt(&mut stage, Key::F);
        assert_eq!(alt(&mut stage, Key::Space), Some(ApplyOutcome::Changed));
        assert_eq!(stage.lab.focus, Some(1));
        assert_eq!(alt(&mut stage, Key::Q), Some(ApplyOutcome::Changed));
        assert_eq!(stage.lab.windows.len(), 1);
        assert_eq!(stage.lab.focus, Some(0));
        let _ = alt(&mut stage, Key::Q);
        assert!(stage.lab.windows.is_empty());
        assert_eq!(stage.scope_context(), ScopeContext::Lab);
        // Enter at an empty tiler makes a kiln.
        let _ = stage.handle_key(Mods::NONE, Key::Enter);
        assert_eq!(stage.lab.windows.len(), 1);
    }

    #[test]
    fn the_kilns_bands_macros_and_sliders_answer_the_keys() {
        let mut stage = lab();
        let kiln = |stage: &Stage| match &stage.lab.focused().unwrap().instrument {
            Instrument::Kiln(k) => k.clone(),
            Instrument::Midi(_) => panic!("expected kiln"),
        };
        assert_eq!(kiln(&stage).band, Band::Macros);
        // A digit picks a macro; Up turns it.
        let _ = stage.handle_key(Mods::NONE, Key::Num4);
        assert_eq!(kiln(&stage).macro_at, 3);
        let before = kiln(&stage).macros[3];
        assert_eq!(
            stage.handle_key(Mods::NONE, Key::ArrowUp),
            Some(ApplyOutcome::Changed)
        );
        assert!(kiln(&stage).macros[3] > before);
        let _ = stage.handle_key(Mods::SHIFT, Key::Num4);
        assert_eq!(kiln(&stage).macro_at, 11);
        // Tab to the sliders; Down walks, Right turns.
        let _ = stage.handle_key(Mods::NONE, Key::Tab);
        assert_eq!(kiln(&stage).band, Band::Sliders);
        let _ = stage.handle_key(Mods::NONE, Key::ArrowDown);
        assert_eq!(kiln(&stage).slider_at, 1);
        let before = kiln(&stage).sliders[1];
        let _ = stage.handle_key(Mods::NONE, Key::ArrowRight);
        assert!(kiln(&stage).sliders[1] > before);
        // The filter narrows the list to the wires.
        let _ = stage.handle_key(Mods::NONE, Key::Slash);
        assert!(kiln(&stage).filtering);
        for ch in "wire".chars() {
            assert_eq!(
                stage.apply(StageIntent::TypeChar(ch)),
                ApplyOutcome::Changed
            );
        }
        let shown = kiln(&stage).shown();
        assert!(!shown.is_empty());
        assert!(
            shown.iter().all(|i| MEMBRANE_SLIDERS[*i].group == "wires"
                || MEMBRANE_SLIDERS[*i].name.contains("wire"))
        );
        // Escape closes the filter first, then leaves the window.
        let _ = stage.handle_key(Mods::NONE, Key::Escape);
        assert!(!kiln(&stage).filtering);
        assert_eq!(stage.scope_context(), ScopeContext::Kiln);
        for (key, action) in [
            (Key::H, crate::kiln::job::Action::Hear),
            (Key::P, crate::kiln::job::Action::Print),
            (Key::S, crate::kiln::job::Action::Send),
        ] {
            assert_eq!(
                stage.handle_key(Mods::NONE, key),
                Some(ApplyOutcome::Changed)
            );
            assert_eq!(kiln(&stage).submitted, Some(action));
        }
        if let Some(job) = &kiln(&stage).job {
            job.cancel();
        }
    }

    #[test]
    fn the_sliders_are_about_a_hundred_in_nine_groups_and_read_in_their_units() {
        assert!((90..=110).contains(&MEMBRANE_SLIDERS.len()));
        let mut groups: Vec<&str> = MEMBRANE_SLIDERS.iter().map(|s| s.group).collect();
        groups.dedup();
        assert_eq!(groups.len(), 9);
        for s in MEMBRANE_SLIDERS {
            assert!(
                s.min < s.max && s.default >= s.min && s.default <= s.max,
                "{}",
                s.name
            );
        }
        let kiln = Kiln::default();
        assert_eq!(kiln.reading(1), "3300 N/m");
        assert_eq!(MEMBRANE_MACROS.len(), 16);
    }
}
