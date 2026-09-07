//! The browser: the device tree and the sample catalogue.
//!
//! Lifted out of `main.rs` whole. It was the most self-contained region
//! in that file — a search field, two trees, and one walk over a folder
//! tree — and the only thing tying it to its neighbours was a handful of
//! layout constants, which stay where they are and reach it through
//! `use super::*`.
//!
//! A child module can see its parent's private items, so nothing had to
//! be made public to move this; only the browser's own types needed
//! crate visibility, because now the parent is the one reaching in.

use super::*;

/// A top-level folder and its contents. Dummy data for now — the point is
/// the layout, not the library.
/// A browser entry that does something when activated. Only devices so
/// far — samples and presets arrive as more variants of what `load`
/// carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BrowserItem {
    pub(crate) name: &'static str,
    pub(crate) load: DeviceKind,
}

/// A folder INSIDE a folder: one effect family, and the devices in it.
///
/// The device tree was two levels deep and the effects folder had grown
/// to twenty rows — a flat list that long is a list you scan rather than
/// one you read, and it filled the tree's whole share of the panel the
/// moment it was opened. Grouping by family is what makes "where is the
/// compressor" a question about one heading rather than about twenty
/// names.
pub(crate) struct Group {
    pub(crate) name: &'static str,
    pub(crate) items: &'static [BrowserItem],
    pub(crate) open: bool,
    /// A miniature of what this family DOES — see `ui::glyph`. A name
    /// is a word you read; in a list scanned a hundred times a day the
    /// useful thing is a shape recognised before anything is read.
    pub(crate) mark: Glyph,
}

pub(crate) struct Folder {
    pub(crate) name: &'static str,
    /// Devices sitting directly under the folder, above its groups.
    pub(crate) items: &'static [BrowserItem],
    /// Families under the folder. Empty for a folder that has no reason
    /// to split — `Instruments` is one list and reads as one.
    pub(crate) groups: Vec<Group>,
    pub(crate) open: bool,
    pub(crate) mark: Glyph,
}

/// Everything the browser owns that outlives a frame.
pub(crate) struct Browser {
    pub(crate) query: String,
    pub(crate) folders: Vec<Folder>,
    /// None is the Ableton-style All view; otherwise this is a configured
    /// catalog location id, never an arbitrary filesystem path.
    pub(crate) location: Option<String>,
    /// Root-relative folder selected inside `location`.
    pub(crate) folder: Option<std::path::PathBuf>,
    /// Whether the `All samples` root is open. The root is a singleton with an
    /// obvious default, so it is a bool and starts open; everything below it
    /// lives in `expanded` and starts closed.
    pub(crate) root_open: bool,
    /// Which catalog locations and folders are open, keyed by [`folder_key`]
    /// (a location uses the empty relative path). A row is drawn only when
    /// every ancestor of it is in here, so the tree opens one level at a time
    /// instead of listing the whole library at once.
    pub(crate) expanded: std::collections::HashSet<String>,
    /// Pixel offset for the combined location, folder, and result list.
    pub(crate) catalog_scroll: f32,
    /// How far the DEVICE tree is scrolled, in points.
    ///
    /// Its own, beside the catalog's: the two lists share a band but not
    /// a position, and a wheel over one must not move the other.
    pub(crate) tree_scroll: f32,
}

impl Default for Browser {
    fn default() -> Self {
        // The built-in devices, which are NOT mockups any more: every row
        // here is a real node with a real parameter table behind it, and
        // dragging one in is how a track gets an instrument. The sample
        // library fills the catalog below; this tree is what the app itself
        // ships with, so it is built in rather than scanned.
        Self {
            query: String::new(),
            location: None,
            folder: None,
            root_open: true,
            expanded: std::collections::HashSet::new(),
            catalog_scroll: 0.0,
            tree_scroll: 0.0,
            folders: vec![
                Folder {
                    name: "Instruments",
                    mark: Glyph::Instrument,
                    // SPLIT, the way the effects are. Ten was already
                    // more than a glance takes, and the three families
                    // underneath answer different questions: what plays
                    // a note, what plays a hit, and what plays a file.
                    //
                    // Nothing loose — a device sitting outside the
                    // headings would read as the odd one out rather than
                    // as the uncategorised one.
                    items: &[],
                    groups: vec![
                        Group {
                            // voices built from nothing.
                            name: "Synths",
                            mark: Glyph::Saw,
                            items: &[
                                BrowserItem {
                                    name: "Poly Synth",
                                    load: DeviceKind::Poly,
                                },
                                BrowserItem {
                                    name: "Tine",
                                    load: DeviceKind::Tine,
                                },
                                BrowserItem {
                                    name: "sComp",
                                    load: DeviceKind::Scomp,
                                },
                                BrowserItem {
                                    name: "Haze",
                                    load: DeviceKind::Haze,
                                },
                                BrowserItem {
                                    name: "Loom",
                                    load: DeviceKind::Loom,
                                },
                                BrowserItem {
                                    name: "Acid",
                                    load: DeviceKind::Acid,
                                },
                                BrowserItem {
                                    name: "Sine Synth",
                                    load: DeviceKind::SineSynth,
                                },
                            ],
                            open: false,
                        },
                        Group {
                            // one strike each, and gone.
                            name: "Drums",
                            mark: Glyph::Transient,
                            items: &[
                                BrowserItem {
                                    name: "Kick",
                                    load: DeviceKind::Kick,
                                },
                                BrowserItem {
                                    name: "Snare",
                                    load: DeviceKind::Snare,
                                },
                                BrowserItem {
                                    name: "Tom",
                                    load: DeviceKind::Tom,
                                },
                                BrowserItem {
                                    name: "808 Hat",
                                    load: DeviceKind::Hat,
                                },
                                BrowserItem {
                                    name: "Clap",
                                    load: DeviceKind::Handclap,
                                },
                            ],
                            open: false,
                        },
                        Group {
                            // voices built from a recording.
                            name: "Sampling",
                            mark: Glyph::Sample,
                            items: &[BrowserItem {
                                name: "Sampler",
                                load: DeviceKind::Sampler,
                            }],
                            open: false,
                        },
                    ],
                    open: true,
                },
                Folder {
                    name: "Audio Effects",
                    // A container of families rather than a family: the
                    // one mark with no signal in it, which is the point.
                    mark: Glyph::Stack,
                    // Nothing loose: every effect belongs to a family,
                    // and one device sitting outside the headings would
                    // read as the odd one out rather than as the
                    // uncategorised one.
                    items: &[],
                    groups: vec![
                        Group {
                            // the level of a thing against itself.
                            name: "Dynamics",
                            mark: Glyph::Dynamics,
                            items: &[
                                BrowserItem {
                                    name: "Clamp",
                                    load: DeviceKind::Clamp,
                                },
                                BrowserItem {
                                    name: "Flint",
                                    load: DeviceKind::Flint,
                                },
                                BrowserItem {
                                    name: "Glue",
                                    load: DeviceKind::Glue,
                                },
                                BrowserItem {
                                    name: "Gate",
                                    load: DeviceKind::Gate,
                                },
                                BrowserItem {
                                    name: "Limiter",
                                    load: DeviceKind::Limiter,
                                },
                                BrowserItem {
                                    name: "Prism",
                                    load: DeviceKind::Prism,
                                },
                            ],
                            open: false,
                        },
                        Group {
                            // which frequencies, and how much of each.
                            name: "EQ & Filters",
                            mark: Glyph::Filter,
                            items: &[
                                BrowserItem {
                                    name: "EQ",
                                    load: DeviceKind::Eq,
                                },
                                BrowserItem {
                                    name: "Filter",
                                    load: DeviceKind::Filter,
                                },
                                BrowserItem {
                                    name: "Strip",
                                    load: DeviceKind::Strip,
                                },
                                BrowserItem {
                                    name: "Tilt",
                                    load: DeviceKind::Tilt,
                                },
                            ],
                            open: false,
                        },
                        Group {
                            // the same sound again, later.
                            name: "Delay & Reverb",
                            mark: Glyph::Time,
                            items: &[
                                BrowserItem {
                                    name: "Delay",
                                    load: DeviceKind::Echo,
                                },
                                BrowserItem {
                                    name: "Reverb",
                                    load: DeviceKind::Reverb,
                                },
                                BrowserItem {
                                    name: "Ferric",
                                    load: DeviceKind::Ferric,
                                },
                                BrowserItem {
                                    name: "Umbra",
                                    load: DeviceKind::Umbra,
                                },
                            ],
                            open: false,
                        },
                        Group {
                            // harmonics that were not in the input.
                            name: "Distortion",
                            mark: Glyph::Drive,
                            items: &[
                                BrowserItem {
                                    name: "Saturator",
                                    load: DeviceKind::Sat,
                                },
                                BrowserItem {
                                    name: "Lo-fi",
                                    load: DeviceKind::Lofi,
                                },
                                BrowserItem {
                                    name: "Sheen",
                                    load: DeviceKind::Sheen,
                                },
                            ],
                            open: false,
                        },
                        Group {
                            // something moving on its own.
                            name: "Modulation",
                            mark: Glyph::Modulation,
                            items: &[
                                BrowserItem {
                                    name: "Modulato",
                                    load: DeviceKind::Modulato,
                                },
                                BrowserItem {
                                    name: "Phaser",
                                    load: DeviceKind::Phaser,
                                },
                                BrowserItem {
                                    name: "Disperser",
                                    load: DeviceKind::Disperser,
                                },
                                BrowserItem {
                                    name: "Sigil",
                                    load: DeviceKind::Sigil,
                                },
                            ],
                            open: false,
                        },
                        Group {
                            // the sound taken apart and put back together.
                            name: "Spectral",
                            mark: Glyph::Spectral,
                            items: &[
                                BrowserItem {
                                    name: "Resyn",
                                    load: DeviceKind::Resyn,
                                },
                                BrowserItem {
                                    name: "Sibyl",
                                    load: DeviceKind::Sibyl,
                                },
                            ],
                            open: false,
                        },
                        Group {
                            // plumbing rather than sound.
                            name: "Utilities",
                            mark: Glyph::Utility,
                            items: &[
                                BrowserItem {
                                    name: "Utility",
                                    load: DeviceKind::Utility,
                                },
                                BrowserItem {
                                    name: "Gauge",
                                    load: DeviceKind::Gauge,
                                },
                                BrowserItem {
                                    name: "Tone",
                                    load: DeviceKind::Tone,
                                },
                                BrowserItem {
                                    name: "Rack",
                                    load: DeviceKind::Rack,
                                },
                            ],
                            open: false,
                        },
                    ],
                    open: true,
                },
            ],
        }
    }
}

/// Identifies one catalog folder across a location and its root-relative
/// path. A plain string so the open-set is a `HashSet<String>` and lookups
/// need no tuple juggling; the NUL keeps a location id from colliding with a
/// path that happens to start the same way.
pub(crate) fn folder_key(location_id: &str, relative: &std::path::Path) -> String {
    format!("{location_id}\u{0}{}", relative.display())
}

/// Browser requests stay in the app shell: rendering never scans or mutates
/// the filesystem, and sample activation does not touch the audio callback.
#[derive(Debug, Clone)]
pub(crate) enum BrowserEvent {
    LoadDevice(BrowserItem),
    SelectSample(std::path::PathBuf),
}

/// The rows the tree currently shows, as (indent depth, text, is_folder).
///
/// Pure: the whole tree is derived from the folder list, so what is on
/// screen and what the click handler thinks is on screen cannot drift apart.
/// The last child of a folder gets the elbow, the rest get tees.
/// One level of indent, in spaces. Two, matching what the flat tree
/// already drew for a folder's children.
pub(crate) const TREE_INDENT: &str = "  ";

/// The two character cells a heading leaves empty for its family mark,
/// plus the space after it. Spaces rather than a glyph: the mark is
/// PAINTED there — see `ui::glyph` for why a vector and not a
/// character.
pub(crate) const MARK_PAD: &str = "   ";

/// What a heading row toggles when it is pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TreeToggle {
    Folder(usize),
    Group(usize, usize),
}

/// What pressing a row does. Every row does exactly one of these, which
/// is the point of the type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RowAct {
    /// Open or close a container.
    Toggle(TreeToggle),
    /// Load a device.
    Load(BrowserItem),
    /// Nothing. The "no match" line, which is a message rather than a
    /// control — and has to be a row, because an empty list reads as a
    /// broken list rather than as an answer.
    Inert,
}

/// One line of the tree: what it says, what it does, and which of its
/// characters the query matched.
///
/// ONE WALKER, and that is a correctness property rather than tidiness.
/// This was three parallel lists — the text, the load target, the
/// toggle owner — each walking the same shape independently and each
/// having to agree with the others by index. Three walks that must
/// agree are a redundancy that can drift, and adding a filter would
/// have meant teaching the filter to all three identically.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
    pub(crate) text: String,
    pub(crate) act: RowAct,
    /// Where the query matched, in CHARACTERS from the start of `text`.
    /// The font is monospace, so a character offset is a pixel offset
    /// and the mark can be drawn without laying the string out twice.
    pub(crate) hit: Option<(usize, usize)>,
    /// The family mark and the character cell it sits in. Headings
    /// only: a device belongs to whatever family it is filed under and
    /// repeating that on every line would be forty copies of one fact.
    pub(crate) mark: Option<(Glyph, usize)>,
    /// How deep this row is, for the guides that connect a heading to
    /// its children.
    pub(crate) depth: usize,
}

impl Row {
    /// Test-only: the drawing asks `act` directly, because it needs to
    /// know WHICH of the three a row is rather than whether it is one.
    #[cfg(test)]
    pub(crate) fn is_heading(&self) -> bool {
        matches!(self.act, RowAct::Toggle(_))
    }
}

/// Does this device answer the query?
///
/// Name only. Matching the folder name as well sounds generous and is
/// the opposite: type "dyn" and every compressor arrives with no
/// indication of why, because the thing that matched is not on screen.
pub(crate) fn item_matches(item: &BrowserItem, query: &str) -> bool {
    item.name.to_lowercase().contains(query)
}

/// Where `query` sits inside `name`, in characters — offset by whatever
/// glyphs the row puts in front of it.
pub(crate) fn hit_at(name: &str, query: &str, lead: usize) -> Option<(usize, usize)> {
    if query.is_empty() {
        return None;
    }
    let lower = name.to_lowercase();
    let byte = lower.find(query)?;
    let start = lower[..byte].chars().count();
    Some((lead + start, query.chars().count()))
}

/// The count a container carries: how many devices are under it, and
/// how many of those the query left.
///
/// A closed container that says nothing about its contents costs a
/// press to answer the most common question anyone has about it. Three
/// characters answer it instead. Under a query it is `n/N`, which also
/// says how much is being hidden — the one number a filter owes you.
pub(crate) fn tally(shown: usize, total: usize, filtering: bool) -> String {
    if filtering {
        format!("  {shown}/{total}")
    } else {
        format!("  {total}")
    }
}

/// The rows of `items`, indented `depth` levels and elbowed on the last.
pub(crate) fn item_rows(items: &[BrowserItem], depth: usize, query: &str, rows: &mut Vec<Row>) {
    let pad = TREE_INDENT.repeat(depth);
    let keep: Vec<&BrowserItem> = items
        .iter()
        .filter(|item| query.is_empty() || item_matches(item, query))
        .collect();
    for (i, item) in keep.iter().enumerate() {
        let last = i + 1 == keep.len();
        let branch = if last { TREE_ELL } else { TREE_TEE };
        // The glyphs in front of the name, in CHARACTERS: the indent,
        // the two-character branch, and the space after it.
        let lead = pad.chars().count() + branch.chars().count() + 1;
        rows.push(Row {
            text: format!("{pad}{branch} {}", item.name),
            act: RowAct::Load(**item),
            hit: hit_at(item.name, query, lead),
            mark: None,
            depth,
        });
    }
}

/// How many of `items` the query leaves.
pub(crate) fn kept(items: &[BrowserItem], query: &str) -> usize {
    if query.is_empty() {
        return items.len();
    }
    items.iter().filter(|i| item_matches(i, query)).count()
}

/// Every visible line of the device tree, in order.
///
/// # What a query does
///
/// A container with no surviving device disappears; one with survivors
/// is FORCED OPEN whatever its stored state says, because a filter that
/// hid its own results behind a closed arrow would be answering a
/// question nobody could see the answer to.
///
/// The stored state is not touched. Clearing the query puts the tree
/// back exactly as it was — a filter that permanently reorganised your
/// browser would cost you your place every time you used it.
pub(crate) fn tree_rows(folders: &[Folder], query: &str) -> Vec<Row> {
    let query = query.trim().to_lowercase();
    let filtering = !query.is_empty();
    let mut rows = Vec::new();

    for (f, folder) in folders.iter().enumerate() {
        let under: usize =
            folder.groups.iter().map(|g| g.items.len()).sum::<usize>() + folder.items.len();
        let shown: usize = folder
            .groups
            .iter()
            .map(|g| kept(g.items, &query))
            .sum::<usize>()
            + kept(folder.items, &query);
        if filtering && shown == 0 {
            continue;
        }
        let open = folder.open || filtering;
        let arrow = if open { TREE_OPEN } else { TREE_SHUT };
        rows.push(Row {
            text: format!(
                "{arrow} {MARK_PAD}{}{}",
                folder.name,
                tally(shown, under, filtering)
            ),
            act: RowAct::Toggle(TreeToggle::Folder(f)),
            hit: None,
            mark: Some((folder.mark, 2)),
            depth: 0,
        });
        if !open {
            continue;
        }
        // Groups first, then whatever sits loose under the folder: a
        // heading below the things it does not contain reads as if it
        // did contain them.
        for (g, group) in folder.groups.iter().enumerate() {
            let shown = kept(group.items, &query);
            if filtering && shown == 0 {
                continue;
            }
            let open = group.open || filtering;
            let arrow = if open { TREE_OPEN } else { TREE_SHUT };
            rows.push(Row {
                text: format!(
                    "{TREE_INDENT}{arrow} {MARK_PAD}{}{}",
                    group.name,
                    tally(shown, group.items.len(), filtering)
                ),
                act: RowAct::Toggle(TreeToggle::Group(f, g)),
                hit: None,
                mark: Some((group.mark, TREE_INDENT.len() + 2)),
                depth: 1,
            });
            if open {
                item_rows(group.items, 2, &query, &mut rows);
            }
        }
        item_rows(folder.items, 1, &query, &mut rows);
    }

    // AN ANSWER, not an absence. A list that simply goes blank reads as
    // a browser that has broken rather than as a query that found
    // nothing, and the two want very different next actions.
    if filtering && rows.is_empty() {
        rows.push(Row {
            text: format!("{TREE_ELL} no device matches"),
            act: RowAct::Inert,
            hit: None,
            mark: None,
            depth: 0,
        });
    }
    rows
}

/// Every device the tree offers, whatever depth it sits at.
///
/// One walker, so a test asking "is this kind reachable" cannot answer
/// from a shape the tree no longer has — which is exactly what the flat
/// version did the moment the effects folder grew subfolders.
///
/// Test-only: nothing in the running browser wants a flat list, because
/// the tree draws the shape rather than a listing of it.
#[cfg(test)]
pub(crate) fn loadable(rows: &[Row]) -> Vec<BrowserItem> {
    rows.iter()
        .filter_map(|row| match row.act {
            RowAct::Load(item) => Some(item),
            _ => None,
        })
        .collect()
}

/// The search field's tally counts against this, which is why it is not
/// test-only: "of how many" has to mean every device there is, not
/// every device currently unfolded.
pub(crate) fn tree_items(folders: &[Folder]) -> Vec<BrowserItem> {
    folders
        .iter()
        .flat_map(|folder| {
            folder
                .groups
                .iter()
                .flat_map(|group| group.items.iter())
                .chain(folder.items.iter())
                .copied()
        })
        .collect()
}

/// Draw the tree under the search well, and toggle a folder when its row is
/// clicked. Rows past the bottom of the band are simply not drawn.
pub(crate) fn tree(
    ui: &mut egui::Ui,
    theme: &Theme,
    focus: &mut Focus,
    upper: egui::Rect,
    folders: &mut [Folder],
    query: &str,
    scroll: &mut f32,
) -> Option<BrowserItem> {
    let font = egui::FontId::new(TREE_TYPE, egui::FontFamily::Monospace);
    // Monospace, so one character is one width everywhere and the match
    // mark can be placed by counting characters rather than by laying
    // the string out a second time.
    let char_w = ui
        .painter()
        .layout_no_wrap("M".to_owned(), font.clone(), theme.text)
        .rect
        .width();
    let rows = tree_rows(folders, query);
    let viewport = tree_viewport(upper, rows.len());
    // The wheel belongs to whichever list it is OVER. Asking about the
    // whole band would mean a wheel anywhere in the browser moved both
    // lists, which is how a scroll ends up feeling like it is fighting.
    let reach = rows.len() as f32 * TREE_ROW_H - viewport.height();
    if ui.rect_contains_pointer(viewport) {
        let wheel = ui.input(|input| input.smooth_scroll_delta.y);
        *scroll = (*scroll - wheel).clamp(0.0, reach.max(0.0));
    } else {
        *scroll = scroll.min(reach.max(0.0)).max(0.0);
    }
    let top = viewport.top() - *scroll;

    // The catalog owns the empty state, because the device tree can be empty
    // while configured library folders are already present.
    if rows.is_empty() {
        return None;
    }
    let mut load: Option<BrowserItem> = None;
    let mut toggle = None;
    for (index, row) in rows.iter().enumerate() {
        let y = top + index as f32 * TREE_ROW_H;
        // Scrolled past, above or below. `continue` and NOT `break`: the
        // list is offset now, so the rows still to come are the ones on
        // screen — breaking here is what made everything past the fold
        // unreachable in the first place.
        if y + TREE_ROW_H <= viewport.top() || y >= viewport.bottom() {
            continue;
        }
        let line = egui::Rect::from_min_size(
            egui::pos2(upper.left(), y),
            egui::vec2(upper.width(), TREE_ROW_H),
        );

        // Every row is reachable by keyboard; only folders do anything when
        // pressed.
        let wid = ui.id().with(("tree_row", index));
        // The "no device matches" line is a MESSAGE. Registering it for
        // the keyboard would put a stop on the ring that does nothing
        // when pressed, which is worse than not being reachable at all.
        if row.act != RowAct::Inert {
            focus.register(wid, line);
            let response = ui
                .interact(line, wid, egui::Sense::click())
                .affords(Affords::Press);
            if response.hovered() {
                ui.painter().rect_filled(line, 0.0, theme.accent_muted);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            match row.act {
                RowAct::Toggle(which) => {
                    if response.clicked() || focus.activated(wid) {
                        toggle = Some(which);
                    }
                }
                // An item loads: double-click with the mouse, Enter with
                // the keyboard ring. Single click only points at it —
                // loading a device is a commitment, and a stray click on
                // a list should not rewire a track.
                RowAct::Load(item) => {
                    if response.double_clicked() || focus.activated(wid) {
                        load = Some(item);
                    }
                }
                RowAct::Inert => {}
            }
        }

        let painter = ui.painter().with_clip_rect(line);
        let left = line.left() + TREE_PAD_X;

        // GUIDES: the vertical that carries a heading down past its
        // children. The branch glyphs say "this belongs to something";
        // the guide says WHAT, which is the half you lose at depth two
        // once the heading has scrolled off the top.
        for level in 0..row.depth {
            let x = left + (level as f32 * TREE_INDENT.len() as f32 + 0.5) * char_w;
            painter.line_segment(
                [egui::pos2(x, line.top()), egui::pos2(x, line.bottom())],
                egui::Stroke::new(stroke::HAIR, theme.divider),
            );
        }

        // The family's mark, in the two cells its text left empty.
        if let Some((mark, at)) = row.mark {
            let cell = egui::Rect::from_min_size(
                egui::pos2(left + at as f32 * char_w, line.top()),
                egui::vec2(char_w * 2.0, line.height()),
            );
            let open = row.text.trim_start().starts_with(TREE_OPEN);
            daw::ui::glyph::paint(
                &painter,
                cell.shrink2(egui::vec2(0.0, TREE_ROW_H * 0.28)),
                mark,
                daw::ui::glyph::ink(theme, open),
            );
        }
        painter.text(
            egui::pos2(left, line.center().y),
            egui::Align2::LEFT_CENTER,
            &row.text,
            font.clone(),
            match row.act {
                RowAct::Toggle(_) => theme.text,
                RowAct::Load(_) => theme.text_muted,
                RowAct::Inert => theme.divider,
            },
        );

        // WHY THIS ROW IS HERE, under the characters that answered.
        //
        // A filtered list that does not say what it matched leaves you
        // reading every result to work out which part of it you typed.
        // The mark is a RULE rather than a colour on the glyphs
        // themselves: recolouring text spends its legibility to say
        // something about the text rather than in it.
        if let Some((at, len)) = row.hit
            && len > 0
        {
            let from = left + at as f32 * char_w;
            let y = line.center().y + font.size * 0.5;
            painter.line_segment(
                [
                    egui::pos2(from, y),
                    egui::pos2(from + len as f32 * char_w, y),
                ],
                egui::Stroke::new(stroke::HAIR, theme.accent),
            );
        }
    }

    match toggle {
        Some(TreeToggle::Folder(f)) => {
            if let Some(folder) = folders.get_mut(f) {
                folder.open = !folder.open;
            }
        }
        Some(TreeToggle::Group(f, g)) => {
            if let Some(group) = folders.get_mut(f).and_then(|f| f.groups.get_mut(g)) {
                group.open = !group.open;
            }
        }
        None => {}
    }
    load
}

/// The sample catalog scrolls in the space left below the fixed device tree.
/// Keeping this boundary explicit prevents a scrolled sample row from being
/// painted over Instruments or Audio Effects.
/// The device tree's own viewport: where it draws, and no further.
///
/// BOUNDED, which is the whole fix. The tree used to take a row per
/// device with no limit and no scroll — so a folder with more devices
/// than the panel was tall simply stopped drawing partway down, and the
/// ones past the fold could not be reached by any gesture. It also
/// pushed the sample catalog below it, far enough that opening two
/// folders left the catalog entirely off the bottom of the window.
///
/// Now it takes what it needs up to `TREE_SHARE` of the band, and
/// scrolls inside that. Sharing beats winning: a browser where one list
/// can starve the other is a browser with a list you cannot get to.
pub(crate) fn tree_viewport(upper: egui::Rect, device_rows: usize) -> egui::Rect {
    let top = upper.top() + SEARCH_H + TREE_TOP_GAP;
    let room = (upper.bottom() - top).max(0.0);
    let wanted = device_rows as f32 * TREE_ROW_H;
    // At least a couple of rows even in a very short panel: a list
    // cropped to nothing reads as a list that is not there.
    let floor = (TREE_ROW_H * 2.0).min(room);
    let height = wanted.min(room * TREE_SHARE).max(floor).min(room);
    egui::Rect::from_min_max(
        egui::pos2(upper.left(), top),
        egui::pos2(upper.right(), top + height),
    )
}

/// The catalog gets everything the device tree did not take.
pub(crate) fn catalog_viewport(upper: egui::Rect, device_rows: usize) -> egui::Rect {
    let top = tree_viewport(upper, device_rows)
        .bottom()
        .min(upper.bottom());
    egui::Rect::from_min_max(egui::pos2(upper.left(), top), upper.right_bottom())
}

/// Draw the configured catalog beneath the built-in device tree. The catalog
/// is already an immutable snapshot: this function does no filesystem work.
/// Has anyone asked to see samples?
///
/// A query, a location, or a folder. Whitespace is not a query — a
/// stray space in the field would otherwise dump the whole library and
/// look like a bug in the search rather than in the space.
pub(crate) fn catalog_asked(
    query: &str,
    location: Option<&str>,
    folder: Option<&std::path::Path>,
) -> bool {
    !query.trim().is_empty() || location.is_some() || folder.is_some()
}

/// The samples the catalog should list, which is NONE until asked.
///
/// `library::query_at` with no query and no location means "every asset
/// in the library", and that is what the panel drew on launch: a list
/// nobody requested, as long as the disk is deep, burying the locations
/// that would have let anyone ask a real question.
///
/// The root row is a CONTAINER rather than a selection — opening it
/// reveals the locations, and picking one of those is the ask.
pub(crate) fn catalog_results<'a>(
    snapshot: &'a LibrarySnapshot,
    location: Option<&str>,
    folder: Option<&std::path::Path>,
    query: &str,
) -> Vec<&'a library::AssetRecord> {
    if !catalog_asked(query, location, folder) {
        return Vec::new();
    }
    library::query_at(snapshot, location, folder, query)
}

pub(crate) fn catalog_tree(
    ui: &mut egui::Ui,
    theme: &Theme,
    focus: &mut Focus,
    upper: egui::Rect,
    browser: &mut Browser,
    snapshot: &LibrarySnapshot,
) -> Option<BrowserEvent> {
    let font = egui::FontId::new(TREE_TYPE, egui::FontFamily::Monospace);
    let device_rows = tree_rows(&browser.folders, &browser.query).len();
    let viewport = catalog_viewport(upper, device_rows);
    let mut row_index = 0;
    let content_top = viewport.top();
    // SAMPLES ARRIVE WHEN THEY ARE ASKED FOR.
    //
    // `query_at` with no query and no location is "every asset in the
    // library", and that is what this drew on launch: a list nobody
    // requested, as long as the disk is deep, under the two things
    // anyone actually opens this panel for. A library of ten thousand
    // files answered a question nobody had asked and buried the
    // locations that would have let them ask one.
    //
    // Asking is a query, or a location, or a folder. The root row is a
    // CONTAINER rather than a selection — opening it reveals the
    // locations, and picking one of those is the ask.
    let asked = catalog_asked(
        &browser.query,
        browser.location.as_deref(),
        browser.folder.as_deref(),
    );
    let results = catalog_results(
        snapshot,
        browser.location.as_deref(),
        browser.folder.as_deref(),
        &browser.query,
    );
    // A folder shows only when every ancestor of it is open. Without this the
    // pane draws every folder in the library before the first sample row, so
    // a library of any size buries its own audio files below the fold.
    let has_children: std::collections::HashSet<String> = snapshot
        .folders
        .iter()
        .filter_map(|folder| {
            folder
                .relative_path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .map(|parent| folder_key(&folder.location_id, parent))
        })
        .collect();
    let root_open = browser.root_open;
    let location_open: Vec<bool> = snapshot
        .locations
        .iter()
        .map(|location| {
            browser
                .expanded
                .contains(&folder_key(&location.id, std::path::Path::new("")))
        })
        .collect();
    let visible_folders: Vec<&library::LibraryFolder> = if !root_open {
        Vec::new()
    } else {
        snapshot
            .folders
            .iter()
            .filter(|folder| {
                if !browser
                    .expanded
                    .contains(&folder_key(&folder.location_id, std::path::Path::new("")))
                {
                    return false;
                }
                let mut ancestor = folder.relative_path.parent();
                while let Some(path) = ancestor {
                    if path.as_os_str().is_empty() {
                        break;
                    }
                    if !browser
                        .expanded
                        .contains(&folder_key(&folder.location_id, path))
                    {
                        return false;
                    }
                    ancestor = path.parent();
                }
                true
            })
            .collect()
    };
    let location_rows = if root_open {
        snapshot.locations.len()
    } else {
        0
    };
    let total_rows = 1 + location_rows + visible_folders.len() + results.len().max(1);
    let viewport_height = viewport.height();
    let max_scroll = (total_rows as f32 * TREE_ROW_H - viewport_height).max(0.0);
    if ui.rect_contains_pointer(viewport) {
        let scroll = ui.input(|input| input.smooth_scroll_delta.y);
        browser.catalog_scroll = (browser.catalog_scroll - scroll).clamp(0.0, max_scroll);
    } else {
        browser.catalog_scroll = browser.catalog_scroll.min(max_scroll);
    }
    let top = content_top - browser.catalog_scroll;
    let mut event = None;

    let mut draw_location = |label: String, id: Option<&str>, open: bool| {
        let y = top + row_index as f32 * TREE_ROW_H;
        row_index += 1;
        if y + TREE_ROW_H <= content_top || y + TREE_ROW_H > viewport.bottom() {
            return;
        }
        let row = egui::Rect::from_min_size(
            egui::pos2(upper.left(), y),
            egui::vec2(upper.width(), TREE_ROW_H),
        );
        let visible = row.intersect(viewport);
        let wid = ui.id().with(("catalog_location", id));
        focus.register(wid, visible);
        let active = browser.location.as_deref() == id;
        let response = ui
            .interact(visible, wid, egui::Sense::click())
            .affords(Affords::Press);
        if response.hovered() || active {
            ui.painter().rect_filled(
                visible,
                0.0,
                if active {
                    theme.accent_muted
                } else {
                    theme.surface_raised
                },
            );
        }
        if response.clicked() || focus.activated(wid) {
            // Same one-click rule as a folder row: it selects, and it opens or
            // closes. Closing the root folds the whole catalog away; the sample
            // list below is unaffected, because it answers to the search and the
            // selected location, not to what is open.
            match id {
                None => browser.root_open = !browser.root_open,
                Some(id) => {
                    let key = folder_key(id, std::path::Path::new(""));
                    if !browser.expanded.remove(&key) {
                        browser.expanded.insert(key);
                    }
                }
            }
            browser.location = id.map(str::to_owned);
            browser.folder = None;
        }
        let arrow = if open { "\u{25be}" } else { "\u{25b8}" };
        ui.painter().with_clip_rect(visible).text(
            egui::pos2(row.left() + TREE_PAD_X, row.center().y),
            egui::Align2::LEFT_CENTER,
            format!("{arrow} {label}"),
            font.clone(),
            theme.text,
        );
    };

    draw_location("All samples".to_owned(), None, root_open);
    if root_open {
        for (location, open) in snapshot.locations.iter().zip(location_open) {
            let label = if location.user_library {
                format!("  User: {}", location.label)
            } else {
                format!("  {}", location.label)
            };
            draw_location(label, Some(&location.id), open);
        }
    }
    for folder in visible_folders.iter().copied() {
        let y = top + row_index as f32 * TREE_ROW_H;
        row_index += 1;
        if y + TREE_ROW_H <= content_top {
            continue;
        }
        if y + TREE_ROW_H > viewport.bottom() {
            break;
        }
        let row = egui::Rect::from_min_size(
            egui::pos2(upper.left(), y),
            egui::vec2(upper.width(), TREE_ROW_H),
        );
        let visible = row.intersect(viewport);
        let wid = ui
            .id()
            .with(("catalog_folder", &folder.location_id, &folder.relative_path));
        focus.register(wid, visible);
        let active = browser.location.as_deref() == Some(&folder.location_id)
            && browser.folder.as_deref() == Some(folder.relative_path.as_path());
        let response = ui
            .interact(visible, wid, egui::Sense::click())
            .affords(Affords::Press);
        if response.hovered() || active {
            ui.painter().rect_filled(
                visible,
                0.0,
                if active {
                    theme.accent_muted
                } else {
                    theme.surface_raised
                },
            );
        }
        let key = folder_key(&folder.location_id, &folder.relative_path);
        let parent = has_children.contains(&key);
        let open = parent && browser.expanded.contains(&key);
        if response.clicked() || focus.activated(wid) {
            // One click both filters to the folder and, when it has children,
            // opens or closes it — the arrow is what the row looked like it
            // did all along.
            if parent && !browser.expanded.remove(&key) {
                browser.expanded.insert(key);
            }
            browser.location = Some(folder.location_id.clone());
            browser.folder = Some(folder.relative_path.clone());
        }
        let depth = folder.relative_path.components().count().saturating_sub(1);
        let arrow = if !parent {
            " "
        } else if open {
            "\u{25be}"
        } else {
            "\u{25b8}"
        };
        ui.painter().with_clip_rect(visible).text(
            egui::pos2(
                row.left() + TREE_PAD_X * (2.0 + depth as f32),
                row.center().y,
            ),
            egui::Align2::LEFT_CENTER,
            format!("{arrow} {}", folder.name),
            font.clone(),
            theme.text,
        );
    }

    if results.is_empty() {
        let y = top + row_index as f32 * TREE_ROW_H;
        if y + TREE_ROW_H > content_top && y + TREE_ROW_H <= viewport.bottom() {
            let row = egui::Rect::from_min_size(
                egui::pos2(viewport.left(), y),
                egui::vec2(viewport.width(), TREE_ROW_H),
            );
            ui.painter().with_clip_rect(row.intersect(viewport)).text(
                egui::pos2(upper.left() + TREE_PAD_X * 2.0, y + TREE_ROW_H * 0.5),
                egui::Align2::LEFT_CENTER,
                if snapshot.locations.is_empty() {
                    "Set a library folder in Preferences to index samples"
                } else if !asked {
                    // NOT an empty result — an unasked question. The
                    // two look identical as a blank line and want
                    // completely different things from whoever is
                    // reading them, so the row says which this is.
                    "Pick a location above, or type to search"
                } else if browser.query.is_empty() {
                    "No supported audio files in this location"
                } else {
                    "No samples match this search"
                },
                font.clone(),
                if asked {
                    theme.text_muted
                } else {
                    theme.divider
                },
            );
        }
    }
    for asset in results {
        let y = top + row_index as f32 * TREE_ROW_H;
        row_index += 1;
        if y + TREE_ROW_H <= content_top {
            continue;
        }
        if y + TREE_ROW_H > viewport.bottom() {
            break;
        }
        let row = egui::Rect::from_min_size(
            egui::pos2(upper.left(), y),
            egui::vec2(upper.width(), TREE_ROW_H),
        );
        let visible = row.intersect(viewport);
        let wid = ui.id().with(("catalog_asset", &asset.path));
        focus.register(wid, visible);
        let response = ui
            .interact(visible, wid, egui::Sense::click_and_drag())
            .affords(Affords::Carry);
        // A row can be pulled straight onto the timeline. The arrangement's
        // drop ghost is the drag visual, so the row itself stays put.
        response.dnd_set_drag_payload(SampleDrag {
            path: asset.path.clone(),
            origin: SampleDragOrigin::Browser,
        });
        if response.hovered() {
            ui.painter().rect_filled(visible, 0.0, theme.accent_muted);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.double_clicked() || focus.activated(wid) {
            event = Some(BrowserEvent::SelectSample(asset.path.clone()));
        }
        ui.painter().with_clip_rect(visible).text(
            egui::pos2(row.left() + TREE_PAD_X * 2.0, row.center().y),
            egui::Align2::LEFT_CENTER,
            format!("{} .{}", asset.name, asset.extension),
            font.clone(),
            theme.text_muted,
        );
    }
    event
}

/// The search well at the top of the upper band: a darker rectangle holding
/// the magnifying glass and the field.
pub(crate) fn search_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    focus: &mut Focus,
    upper: egui::Rect,
    query: &mut String,
    tally: Option<(usize, usize)>,
) {
    let well = egui::Rect::from_min_size(upper.min, egui::vec2(upper.width(), SEARCH_H));
    if well.width() <= SEARCH_PAD * 3.0 || upper.height() < SEARCH_H {
        // Too narrow to hold an icon and a field, or shorter than the well
        // itself — better absent than spilling out of its band.
        return;
    }
    // Enter on the well hands the keyboard to the field; Escape gives it back.
    let wid = ui.id().with("search");
    if focus.register(wid, well) && focus.activated(wid) {
        ui.ctx().memory_mut(|m| m.request_focus(wid.with("edit")));
    }
    ui.painter().rect_filled(well, 0.0, theme.surface_sunken);

    let icon_font = egui::FontId::new(SEARCH_TYPE, egui::FontFamily::Monospace);
    let icon_x = ui
        .painter()
        .text(
            egui::pos2(well.left() + SEARCH_PAD, well.center().y),
            egui::Align2::LEFT_CENTER,
            SEARCH_ICON,
            icon_font,
            theme.text_muted,
        )
        .right();

    // `ui.put` lays out centered_and_justified, so a field spanning the full
    // well stretches to 28px and draws its text at the TOP of that box. Give
    // it exactly one text row instead, centred on the well, and the justify
    // has nothing left to stretch.
    let text_font = egui::FontId::new(SEARCH_TYPE, egui::FontFamily::Proportional);
    let row = ui.ctx().fonts_mut(|f| f.row_height(&text_font));

    // WHAT THE QUERY LEFT, at the far end of the field it belongs to.
    //
    // The count is on the containers already, but scattered across
    // however many of them survived — and the question "is this query
    // too narrow" is about the whole tree, not about one family. One
    // figure, beside the thing that caused it.
    let mut tail = well.right() - SEARCH_PAD;
    if let Some((shown, total)) = tally.filter(|_| !query.trim().is_empty()) {
        let text = format!("{shown}/{total}");
        let mono = egui::FontId::new(TREE_TYPE, egui::FontFamily::Monospace);
        let mark = ui.painter().text(
            egui::pos2(tail, well.center().y),
            egui::Align2::RIGHT_CENTER,
            &text,
            mono,
            // A query that found nothing is not a small number, it is a
            // different answer — and the field is where you would look
            // to fix it.
            if shown == 0 {
                theme.warn
            } else {
                theme.text_muted
            },
        );
        tail = mark.left() - SEARCH_PAD;
    }
    let field = centred_band(well, icon_x + SEARCH_PAD, tail, row);
    ui.put(
        field,
        egui::TextEdit::singleline(query)
            .id(wid.with("edit"))
            .hint_text("Search samples or #tags")
            // The well is the background; a second frame on top of it would
            // read as a box inside a box, so hand it an empty one.
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .text_color(theme.text)
            .font(text_font),
    );
}

/// A `height`-tall strip spanning `left..right`, centred on `outer`'s middle.
///
/// Pure, so `the_search_field_is_centred_in_its_well` can check the centring
/// arithmetic without laying out any text.
pub(crate) fn centred_band(outer: egui::Rect, left: f32, right: f32, height: f32) -> egui::Rect {
    let mid = outer.center().y;
    egui::Rect::from_min_max(
        egui::pos2(left, mid - height * 0.5),
        egui::pos2(right, mid + height * 0.5),
    )
}

/// The browser's body: claim the space like every other region, then paint
/// the device tree and sample catalog through the full content height.
///
/// The interaction happens BEFORE the paint so a drag lands on the same
/// frame it was made — reading it back afterwards would put the bands one
/// frame behind the pointer.
pub(crate) fn browser_body(
    ui: &mut egui::Ui,
    theme: &Theme,
    focus: &mut Focus,
    browser: &mut Browser,
    snapshot: &LibrarySnapshot,
) -> Option<BrowserEvent> {
    let area = ui.max_rect();
    claim(ui);
    let upper = browser_content(area)?;
    let painter = ui.painter();
    painter.rect_filled(upper, 0.0, theme.surface);

    // The tally the field prints: how many devices the query left, out
    // of how many there are. Counted from the same rows the tree draws,
    // so the number cannot disagree with the list under it.
    let tally = {
        let shown = tree_rows(&browser.folders, &browser.query)
            .iter()
            .filter(|row| matches!(row.act, RowAct::Load(_)))
            .count();
        // Every device there is — NOT every row currently on screen.
        // Counting visible rows would make the total depend on which
        // folders happened to be open, so a shut browser would report
        // "3 of 0".
        (shown, tree_items(&browser.folders).len())
    };
    search_bar(ui, theme, focus, upper, &mut browser.query, Some(tally));
    let load = tree(
        ui,
        theme,
        focus,
        upper,
        &mut browser.folders,
        &browser.query,
        &mut browser.tree_scroll,
    );
    let catalog = catalog_tree(ui, theme, focus, upper, browser, snapshot);
    catalog.or(load.map(BrowserEvent::LoadDevice))
}
