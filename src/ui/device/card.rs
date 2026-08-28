//! The device card: the container every device UI lives in.
//!
//! A device chain is a horizontal strip of cards. Height is LOCKED to
//! [`control::DEVICE_H`] — every card in a rack is exactly as tall as its
//! neighbors, which is what makes a chain read as one strip instead of a
//! shelf of mismatched boxes. Width is the card's own business: it grows
//! with content, never below [`control::DEVICE_W_MIN`].
//!
//! The card is generic: a title strip and an empty body. Devices fill the
//! body with `device` widgets; a card with no content is a valid (if
//! silent) device.

use crate::ui::device::design;
use crate::ui::device::metrics::{self, Footprint};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

/// A titled, fixed-height, content-width card. Returns the closure's
/// result. `add` lays out the device body; pass a no-op for an empty
/// card.
pub fn card<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let mut page = 0;
    tabbed_card(ui, theme, name, 1, &mut page, |ui, _page| add(ui))
}

/// Dot radius, as a fraction of the dot's hit box.
const DOT_R: f32 = 0.3;

/// A card in flight, as a drag-and-drop payload.
///
/// The instance id and nothing else: where it came from is read off the
/// chain when it lands, because the chain may have been scrolled, folded
/// or rebuilt in between and a remembered index would name the wrong
/// device by then.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Carried(pub u64);

/// What the pointer did to one card's title strip.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Grip {
    /// Pressed: this card should become the selection.
    pub clicked: bool,
    /// Ctrl or Shift was held — add to the selection rather than replace
    /// it, which is what makes grouping more than one device possible at
    /// all.
    pub additive: bool,
    /// A carried card was released onto this one, and which one it was.
    /// The carried device belongs immediately BEFORE this one in signal
    /// order.
    pub dropped_from: Option<u64>,
    /// This card is the one being carried.
    pub carrying: bool,
}

/// How tall a card's title strip is: its two margins and one line of
/// text.
///
/// Derived rather than measured, because the strip has to be found from
/// OUTSIDE the card — a chain holds the card's rect and nothing else.
/// `the_title_band_is_where_the_card_actually_put_it` holds this to what
/// [`tabbed_card_gripped`] measures, so the derivation cannot drift away
/// from the thing it describes.
pub fn title_height(theme: &Theme) -> f32 {
    // Two margins, which scale with density, and one line of text, which
    // does not — a font token is a size in points and stays one. The
    // 1.75 is the row height egui gives a proportional face at this size,
    // measured rather than assumed; the test below is what keeps it
    // measured.
    theme.sp(space::XS) * 2.0 + font::LABEL * 1.75
}

/// A card's title strip, given the card.
pub fn title_band(theme: &Theme, card: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        card.min,
        egui::pos2(card.right(), card.top() + title_height(theme)),
    )
}

/// Claim a card's title strip as its handle.
///
/// The title strip and not the whole card, deliberately: a card's face is
/// covered in knobs, and a gesture that took the card would take every
/// press meant for one of them. The strip is the only part of a card that
/// belongs to the card.
///
/// The strip also PAINTS what it knows — selected, carried, about to be
/// landed on — because a device you have selected and a device you have
/// not must not look the same, and a drop with no line drawn is a drop
/// you find out about after it happens.
pub fn grip(
    ui: &mut egui::Ui,
    theme: &Theme,
    id: egui::Id,
    title: egui::Rect,
    instance: u64,
    selected: bool,
) -> Grip {
    use crate::ui::affordance::{Afford, Affords};

    let response = ui
        .interact(title, id, egui::Sense::click_and_drag())
        .affords(Affords::Carry);
    let mut out = Grip {
        clicked: response.clicked(),
        additive: ui.input(|input| input.modifiers.command || input.modifiers.shift),
        ..Grip::default()
    };
    if response.drag_started() {
        egui::DragAndDrop::set_payload(ui.ctx(), Carried(instance));
    }
    let carried = egui::DragAndDrop::payload::<Carried>(ui.ctx()).map(|payload| payload.0);
    out.carrying = carried == Some(instance);

    if selected || out.carrying {
        ui.painter().rect_filled(
            title,
            0.0,
            if out.carrying {
                theme.accent_muted
            } else {
                theme.surface_raised
            },
        );
        ui.painter().rect_stroke(
            title,
            0.0,
            egui::Stroke::new(crate::ui::tokens::stroke::HAIR, theme.accent),
            egui::StrokeKind::Inside,
        );
    }

    // A card in flight, hovering somewhere it could land: the line goes
    // on the LEADING edge, because that is where the carried device will
    // be — a drop that only highlighted the target would leave "before or
    // after" for the user to find out by doing it.
    if let Some(from) = carried
        && from != instance
        && response.hovered()
    {
        ui.painter().line_segment(
            [title.left_top(), egui::pos2(title.left(), title.bottom())],
            egui::Stroke::new(crate::ui::tokens::stroke::BOLD * 1.5, theme.accent),
        );
        if ui.input(|input| input.pointer.any_released()) {
            out.dropped_from = Some(from);
            egui::DragAndDrop::clear_payload(ui.ctx());
        }
    }
    out
}

/// A card whose body has `pages` tabs, switched by the row of dots at the
/// top-left of the title strip. The caller owns which page is open
/// (`page`, clamped into range); `add` lays out the body for the page it
/// is given. One dot per page — filled accent when open, dim otherwise —
/// and a single page draws no dots at all, which is what makes [`card`]
/// this function's trivial case.
pub fn tabbed_card<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    pages: usize,
    page: &mut usize,
    add: impl FnOnce(&mut egui::Ui, usize) -> R,
) -> R {
    tabbed_card_sized(ui, theme, name, control::DEVICE_H, pages, page, add)
}

/// A card at its own HEIGHT TOKEN — [`control::DEVICE_TALL_H`] for an
/// instrument, [`control::DEVICE_H`] for everything else. The token, not
/// a free number: two heights is a design system, N heights is a mess.
pub fn card_sized<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    height: f32,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let mut page = 0;
    tabbed_card_sized(ui, theme, name, height, 1, &mut page, |ui, _page| add(ui))
}

/// [`tabbed_card`] with the height spelled out. `height` is a token value
/// (pre-`sp` scaling), matching how the fixed variant reads its own.
pub fn tabbed_card_sized<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    height: f32,
    pages: usize,
    page: &mut usize,
    add: impl FnOnce(&mut egui::Ui, usize) -> R,
) -> R {
    tabbed_card_gripped(ui, theme, name, height, pages, page, add).0
}

/// [`tabbed_card_sized`], plus where the TITLE STRIP landed.
///
/// A chain that wants to pick a card up needs somewhere to take hold of
/// it, and the title strip is the only part of a card that belongs to
/// the card rather than to the controls on it. Handing the rect back is
/// the card guarding its own geometry — the device UI contract's third
/// rule — instead of a caller reconstructing the strip's height from the
/// font and the margins and drifting the first time either changes.
///
/// The band is returned, not claimed: whether it is a handle at all is
/// the chain's business, and a card drawn in the gallery has no chain.
#[allow(clippy::too_many_arguments)]
pub fn tabbed_card_gripped<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    height: f32,
    pages: usize,
    page: &mut usize,
    add: impl FnOnce(&mut egui::Ui, usize) -> R,
) -> (R, egui::Rect) {
    let pages = pages.max(1);
    *page = (*page).min(pages - 1);

    let mut rule_y = 0.0f32;
    let out = design::card_frame(theme).show(ui, |ui| {
        // Lock the outer height; width follows content.
        //
        // ALWAYS the full height — never `.min(available_height())`, which
        // is what this used to do. That looked defensive and was the
        // opposite: in a region shorter than the token the card would
        // claim the short height while its wells still drew at their
        // natural size, so the card under-reported how much room it had
        // taken and everything after it was laid out ON TOP of it. A card
        // is fixed-height by definition; if the region is too short the
        // honest outcome is a card clipped by its container, not a card
        // that lies about its size and takes the next widget with it.
        ui.set_height(theme.sp(height));
        ui.set_min_width(theme.sp(control::DEVICE_W_MIN));

        ui.vertical(|ui| {
            // The title and body are the two exact tiles of the card.
            // egui's ordinary vertical item gap would leave an unowned
            // strip between them, which reads as yet another body margin.
            ui.spacing_mut().item_spacing.y = 0.0;
            // Title strip: tab dots first (top-left), then the name.
            let strip = design::title_strip(theme).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if pages > 1 {
                        dot_row(ui, theme, pages, page);
                        ui.add_space(theme.sp(space::XS));
                    }
                    ui.label(
                        egui::RichText::new(name)
                            .size(font::LABEL)
                            .color(theme.text_muted),
                    );
                });
            });
            // The title rule is PAINTED across the card's final rect
            // after layout, never allocated: kit::rule takes
            // available_width, and inside a scroll area that is the
            // whole rack — one rule call and the card balloons.
            rule_y = strip.response.rect.bottom();

            // Body: every point below the title rule. Top-anchored, NOT
            // centered: sections size themselves from the remaining height,
            // and centering a child that is about to claim the full height
            // just shoves it downward by half the estimate error. Wells
            // supply their own INTERNAL padding; the body has no outer inset.
            design::body(theme)
                .show(ui, |ui| {
                    ui.set_height(ui.available_height());
                    ui.horizontal_top(|ui| add(ui, *page)).inner
                })
                .inner
        })
        .inner
    });

    let rect = out.response.rect;
    ui.painter().hline(
        rect.x_range(),
        rule_y,
        egui::Stroke::new(crate::ui::tokens::stroke::HAIR, theme.divider),
    );
    // The strip is everything above the rule the card just painted, so
    // the band and the line that marks it are the same measurement.
    let title = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), rule_y));
    (out.inner, title)
}

/// The clickable dots. Each dot is its own allocation, so ids stay unique
/// without ceremony.
fn dot_row(ui: &mut egui::Ui, theme: &Theme, pages: usize, page: &mut usize) {
    for i in 0..pages {
        let d = theme.sp(space::SM);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::click());
        if response.clicked() {
            *page = i;
        }
        let open = i == *page;
        let color = if open {
            theme.accent
        } else if response.hovered() {
            theme.text_muted
        } else {
            theme.outline
        };
        // The open page's dot is drawn a shade larger as well as brighter,
        // so the state survives squinting (and non-color vision).
        let r = rect.width() * if open { DOT_R + DOT_R * 0.5 } else { DOT_R };
        ui.painter().circle_filled(rect.center(), r, color);
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("page {} of {pages}", i + 1));
    }
}

/// An empty card: title strip over a blank body. The placeholder while a
/// device's UI does not exist yet — and the proof any card is never
/// zero-size.
pub fn empty_card(ui: &mut egui::Ui, theme: &Theme, name: &str) {
    card(ui, theme, name, |_ui| {});
}

// ------------------------------------------------------------- wells ---

/// One well in a card body: how much width it claims, and what its
/// content needs.
///
/// `span` is a WEIGHT, not a pixel count. A span-2 well beside a span-1
/// well is exactly twice as wide — that is what makes wells of different
/// sizes still read as evenly distributed, because every well is a whole
/// multiple of the same unit. Ragged widths are what happens when each
/// column hugs its own content instead.
///
/// `need` comes from the widget's own `footprint()`, so the well is sized
/// by the contract of what goes in it rather than by a guess. A well with
/// no declared need falls back to [`control::SECTION_W_MIN`] and to
/// whatever its content measured last frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Well {
    pub span: u16,
    pub need: Footprint,
    /// Does the content claim the WHOLE well?
    ///
    /// Normally a well centres what it holds, which is right for a single
    /// control. A nested group of sub-wells is the other case: it should
    /// subdivide its parent edge to edge, and centring it would leave a
    /// margin of parent showing around the subdivision — half-divided,
    /// which reads as a mistake rather than as a choice.
    ///
    /// It also breaks a feedback loop. Centring pads by half the leftover
    /// height, so content that sizes itself to what is *available* would
    /// measure smaller every frame, be padded less, measure larger, and
    /// creep for several frames before settling. Saying "this fills" up
    /// front means the pad is zero and there is nothing to converge.
    pub fill: bool,
    /// How this well is split, as one COLUMN COUNT PER ROW.
    ///
    /// `[3]` is a row of three. `[2, 2]` is a 2×2. `[3, 4]` is three
    /// across the top and four beneath — a formation a uniform
    /// `(cols, rows)` grid cannot express at all, and the reason this is a
    /// list rather than a pair. Empty, or a single `1`, means undivided.
    ///
    /// Rows of different counts do NOT share column edges, and that is the
    /// point: three over four is three thirds over four quarters, not
    /// twelve cells with merges. A formation whose rows must line up is a
    /// uniform grid, and [`Well::divided`] builds one.
    ///
    /// Both axes, because grouping is two-dimensional: two filter poles
    /// side by side is a row of two, an A/B pair stacked is two rows of
    /// one, four modulation slots are `[2, 2]`.
    divisions: Vec<u16>,
    /// An optional caption above this well's content.
    ///
    /// A tray can group controls but cannot say what the group IS, and
    /// "these three belong together" is only half the sentence — every
    /// hardware panel finishes it with a word. The header is where
    /// "envelope", "filter" or "room" goes.
    ///
    /// Its MEASURED size is carried with it, not just the text: the width
    /// and height it needs have to be part of the well's contract, and the
    /// contract is computed without a `Ui` to measure with. Measuring once
    /// at declaration is also the only way a header can be guaranteed not
    /// to crowd the controls under it.
    header: Option<Header>,
}

/// A well's caption, with the space it needs already measured.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Header {
    text: &'static str,
    size: egui::Vec2,
}

impl Default for Well {
    fn default() -> Self {
        Self {
            span: 1,
            need: Footprint::ZERO,
            fill: false,
            divisions: Vec::new(),
            header: None,
        }
    }
}

impl Well {
    /// A one-unit well.
    pub fn one() -> Self {
        Self::default()
    }

    /// A well `span` units wide. Zero is treated as one — a weightless
    /// well would be a zero-width well, which is a well nobody can see.
    pub fn span(span: u16) -> Self {
        Self {
            span: span.max(1),
            ..Self::default()
        }
    }

    /// A well divided into a `cols`×`rows` grid of equal sub-wells,
    /// spanning `cols` units.
    ///
    /// Tying the span to the COLUMN count is the reason this is the
    /// constructor rather than a plain builder: a well twice the width of
    /// its neighbours, split in two, gives sub-wells that read as siblings
    /// of the single-unit wells beside it rather than as a different size
    /// of thing. `divided(3, 1)`, `divided(4, 1)` and `divided(2, 2)` all
    /// follow the same rule.
    ///
    /// Argument order is cols-then-rows, matching [`Wells::uniform`] —
    /// x before y, everywhere in this module.
    ///
    /// A WARNING about rows, from measuring rather than guessing: a card's
    /// height is locked, and a labelled knob needs ~78pt with its padding.
    /// One row fits the ~129pt body comfortably; TWO need ~152pt and do
    /// not. Row divisions are therefore for short content — readouts,
    /// toggles, meters, unlabelled cells — or for a device that has argued
    /// its way to a taller card. [`Wells::min_height`] is the number to
    /// check, and it is what the fit tests assert against.
    ///
    /// Its sub-wells are LEAVES of the layout, in ROW-MAJOR order: they
    /// consume consecutive indices in the `add` closure, so a row of
    /// `[one, divided(2, 2)]` calls back with 0 for the plain well and
    /// 1..=4 for the grid, left to right and then down. The well structure
    /// is layout; it is not something the caller counts through.
    pub fn divided(cols: u16, rows: u16) -> Self {
        Self::span(cols).split(cols, rows)
    }

    /// Split this well into a `cols`×`rows` grid, independently of its
    /// span — a four-unit well split in two, say.
    pub fn split(mut self, cols: u16, rows: u16) -> Self {
        self.divisions = vec![cols.max(1); usize::from(rows.max(1))];
        self
    }

    /// A well split into rows of DIFFERENT widths — three across the top
    /// and four beneath is `rows_of([3, 4])`.
    ///
    /// The formation a uniform grid cannot express. Rows of unequal counts
    /// do not share column edges: three over four is three thirds over
    /// four quarters, which is what makes it a formation rather than a
    /// table with merged cells.
    ///
    /// The span is the widest row, so the well still lands on the card's
    /// unit grid at its widest point.
    pub fn rows_of(counts: impl IntoIterator<Item = u16>) -> Self {
        let counts: Vec<u16> = counts.into_iter().map(|c| c.max(1)).collect();
        let widest = counts.iter().copied().max().unwrap_or(1);
        Self::span(widest).split_rows(counts)
    }

    /// Set the per-row column counts directly, leaving the span alone.
    pub fn split_rows(mut self, counts: impl IntoIterator<Item = u16>) -> Self {
        self.divisions = counts.into_iter().map(|c| c.max(1)).collect();
        self
    }

    /// Give this well a caption.
    ///
    /// Independent of [`Well::fits`] and [`Well::each`], and callable in
    /// any order: the header is reserved ON TOP of whatever the content
    /// needs rather than folded into it, so titling a well can never eat
    /// the room its controls were promised. A well grows to fit its title;
    /// a title never shrinks the well.
    ///
    /// Takes a `Ui` because the text is measured here, once, rather than
    /// estimated from a character count at layout time — the same reason
    /// `metrics` measures every other string.
    pub fn titled(mut self, text: &'static str, ui: &egui::Ui, _theme: &Theme) -> Self {
        self.header = Some(Header {
            text,
            size: egui::vec2(
                metrics::text_w(ui, text, font::LABEL),
                metrics::line_h(ui, font::LABEL),
            ),
        });
        self
    }

    /// The room this well's header takes, including the gap below it.
    /// Zero when there is no header — an untitled well is exactly what it
    /// always was.
    fn header_h(&self, theme: &Theme) -> f32 {
        self.header.map_or(0.0, |h| h.size.y + design::gap(theme))
    }

    /// The width this well's header demands on its own.
    fn header_w(&self) -> f32 {
        self.header.map_or(0.0, |h| h.size.x)
    }

    /// Declare what EVERY division must hold. The well's own contract is
    /// then the whole grid of those — padding and gaps included, on both
    /// axes — so the card's size still comes from the widgets at the
    /// bottom however deep the grouping goes.
    pub fn each(mut self, need: Footprint, theme: &Theme) -> Self {
        self.need = division_group(&self.division_rows(), need).footprint(theme);
        self.fill = true;
        self
    }

    /// [`Well::each`] for a body that will be drawn compact.
    ///
    /// Separate because the contract has to be computed at the density it
    /// will be DRAWN at: a divided well sized with roomy padding and then
    /// drawn compact reserves more than it needs, and the card comes out
    /// wider than anything in it.
    pub fn each_compact(mut self, need: Footprint, theme: &Theme) -> Self {
        let mut group = division_group(&self.division_rows(), need);
        group.compact = true;
        self.need = group.footprint(theme);
        self.fill = true;
        self
    }

    /// This well's per-row column counts, normalized to at least one row.
    fn division_rows(&self) -> Vec<u16> {
        if self.divisions.is_empty() {
            vec![1]
        } else {
            self.divisions.clone()
        }
    }

    /// How many sub-wells this cell contributes. 1 when undivided.
    fn leaves(&self) -> usize {
        self.division_rows()
            .iter()
            .map(|c| usize::from(*c))
            .sum::<usize>()
            .max(1)
    }

    /// Is this cell actually split? A single cell is just a well.
    fn is_divided(&self) -> bool {
        self.leaves() > 1
    }

    /// The frame this cell wears at `level`.
    ///
    /// A divided well wears the tray whatever level it is at, because
    /// what it holds is wells rather than content. The padding side of
    /// this decision lives in `Wells::cell_pad`, and the two must agree —
    /// computing a size from one padding and drawing another is how
    /// divisions end up not fitting what they declared.
    fn frame(&self, level: Level, compact: bool) -> fn(&Theme) -> egui::Frame {
        match (compact, self.is_divided()) {
            (true, true) => design::mini_group_well,
            (true, false) => level.mini_frame,
            (false, true) => design::group_well,
            (false, false) => level.frame,
        }
    }

    /// Declare what the well must hold, from a widget's `footprint()`.
    pub fn fits(mut self, need: Footprint) -> Self {
        self.need = need;
        self
    }

    /// The content claims the whole well rather than being centred in it.
    pub fn filling(mut self) -> Self {
        self.fill = true;
        self
    }

    /// This well holds a nested group of sub-wells: take its size from the
    /// group's own contract, and let it fill.
    ///
    /// The nesting idiom, and the reason contracts compose — the group
    /// asks its sub-wells, which ask their widgets, and the answer travels
    /// out to the card's width. Draw the group with [`sub_wells`] and the
    /// SAME spec.
    pub fn holds(self, group: &Wells, theme: &Theme) -> Self {
        self.fits(group.footprint(theme)).filling()
    }
}

/// The spec a divided well expands to: `d` equal cells, each needing
/// `need`. One function so the CONTRACT (`Well::each`) and the PLACEMENT
/// (`place`) build the same thing — computing the size from one shape and
/// drawing another is how a division ends up not fitting what it declared.
fn division_group(counts: &[u16], need: Footprint) -> Wells {
    counts.iter().fold(Wells::new(), |w, c| {
        w.row((0..(*c).max(1)).map(|_| Well::one().fits(need)))
    })
}

/// A row of wells, and how much of the card's height it claims.
#[derive(Debug, Clone, PartialEq)]
struct WellRow {
    weight: u16,
    cells: Vec<Well>,
}

/// A card body's well layout: rows of weighted wells.
///
/// Rows share the body's height by weight and wells share their row's
/// width by span, so the whole body tiles exactly — no leftover strip on
/// the right, no accumulating gap drift, and every boundary lines up
/// whatever the mix of sizes.
///
/// ```ignore
/// Wells::new()
///     .row([Well::span(2).fits(env_fp), Well::one().fits(knob_fp)])
///     .row_weighted(1, [Well::one(), Well::one(), Well::one()])
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Wells {
    rows: Vec<WellRow>,
    compact: bool,
}

impl Wells {
    pub fn new() -> Self {
        Self::default()
    }

    /// Draw this body's wells in their COMPACT form: half-step padding,
    /// tighter gaps, no hairlines.
    ///
    /// A property of the whole body rather than of one well, and that is
    /// the point. Compactness is a rhythm — a card with one tight well
    /// among roomy ones does not read as dense, it reads as a mistake in
    /// the one well. Mixing the two was never the feature.
    ///
    /// For cards built from mini widgets, where standard chrome is a
    /// frame competing with its own content.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    pub fn is_compact(&self) -> bool {
        self.compact
    }

    /// The gap between this body's wells.
    fn gap(&self, theme: &Theme) -> f32 {
        if self.compact {
            design::mini_gap(theme)
        } else {
            design::gap(theme)
        }
    }

    /// The padding a cell of this body wears.
    fn cell_pad(&self, cell: &Well, theme: &Theme) -> f32 {
        match (self.compact, cell.is_divided()) {
            (true, true) => design::mini_group_pad(theme),
            (true, false) => design::mini_pad(theme),
            (false, true) => design::group_pad(theme),
            (false, false) => design::well_pad(theme),
        }
    }

    /// Append a row of unit height.
    pub fn row(self, cells: impl IntoIterator<Item = Well>) -> Self {
        self.row_weighted(1, cells)
    }

    /// Append a row claiming `weight` units of the body's height, so a
    /// weight-2 row is exactly twice as tall as a weight-1 row.
    pub fn row_weighted(mut self, weight: u16, cells: impl IntoIterator<Item = Well>) -> Self {
        let cells: Vec<Well> = cells.into_iter().collect();
        if !cells.is_empty() {
            self.rows.push(WellRow {
                weight: weight.max(1),
                cells,
            });
        }
        self
    }

    /// A plain `cols`×`rows` grid of equal wells — what `sections` builds.
    pub fn uniform(cols: usize, rows: usize) -> Self {
        let (cols, rows) = (cols.max(1), rows.max(1));
        (0..rows).fold(Self::new(), |w, _| w.row((0..cols).map(|_| Well::one())))
    }

    /// Total number of LEAVES, in the row-major order `wells` indexes them.
    ///
    /// A divided well counts as its divisions, not as one — that is the
    /// index the `add` closure receives, and the whole point of divisions
    /// being layout rather than something the caller counts through.
    pub fn len(&self) -> usize {
        self.rows
            .iter()
            .flat_map(|r| r.cells.iter())
            .map(|c| c.leaves())
            .sum()
    }

    /// Number of CELLS — wells drawn, counting a divided well once. The
    /// layout's own shape, as distinct from how many callbacks it makes.
    pub fn cells(&self) -> usize {
        self.rows.iter().map(|r| r.cells.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The narrowest the body can be with no well squeezing its content.
    ///
    /// This is what makes a card content-width AND honest: the card asks
    /// its wells, the wells ask their widgets, and the answer is a width
    /// at which nothing is clipped. A card in a rack is never stretched
    /// to fill, so this number is the card's actual width.
    pub fn min_width(&self, theme: &Theme) -> f32 {
        let g = self.gap(theme);
        // A compact well gets a compact floor. Leaving the full one in
        // place cancels the padding saving exactly — the floor is an
        // OUTER width, so tighter padding just buys more empty middle.
        let floor = theme.sp(if self.compact {
            control::SECTION_W_MIN_MINI
        } else {
            control::SECTION_W_MIN
        });
        self.rows
            .iter()
            .map(|row| {
                let total: f32 = row.cells.iter().map(|c| f32::from(c.span)).sum();
                let gaps = g * (row.cells.len() - 1) as f32;
                // Well i gets (W - gaps) * s_i / S. Solve each cell's
                // content requirement for W and take the largest.
                row.cells
                    .iter()
                    .map(|c| {
                        let p = self.cell_pad(c, theme);
                        // A header widens the well when it is the widest
                        // thing in it — a caption that clips is worse than
                        // no caption.
                        let content = c.need.width().max(c.header_w());
                        let outer = content.max(floor - p * 2.0) + p * 2.0;
                        outer * total / f32::from(c.span) + gaps
                    })
                    .fold(0.0f32, f32::max)
            })
            .fold(0.0f32, f32::max)
    }

    /// The shortest the body can be with no well squeezing its content.
    /// A card's height is LOCKED, so this is a number to assert against in
    /// a test rather than one the layout can act on — a device whose
    /// controls do not fit `control::DEVICE_H` is a design problem, and it
    /// should fail a test rather than quietly clip at runtime.
    /// This layout's own size contract, so a group of wells can be nested
    /// inside a single well of a bigger layout — see [`Well::holds`].
    ///
    /// This is what makes nesting safe rather than a guess: a sub-group is
    /// just another thing with a footprint, built from the footprints of
    /// the widgets inside it. Depth changes nothing about how sizing works.
    pub fn footprint(&self, theme: &Theme) -> Footprint {
        Footprint::new(self.min_width(theme), self.min_height(theme))
    }

    pub fn min_height(&self, theme: &Theme) -> f32 {
        let g = self.gap(theme);
        let total: f32 = self.rows.iter().map(|r| f32::from(r.weight)).sum();
        if total <= 0.0 {
            return 0.0;
        }
        let gaps = g * (self.rows.len() - 1) as f32;
        self.rows
            .iter()
            .map(|row| {
                let need = row
                    .cells
                    .iter()
                    .map(|c| c.need.height() + c.header_h(theme) + self.cell_pad(c, theme) * 2.0)
                    .fold(0.0f32, f32::max);
                need * total / f32::from(row.weight) + gaps
            })
            .fold(0.0f32, f32::max)
    }
}

/// Split `avail` into `n` parts by `weights`, with `gap` between each.
///
/// Returns (offset, size) per part. Edges are computed from the RUNNING
/// TOTAL and rounded once, never accumulated part by part: rounding each
/// width independently leaves a drifting seam that shows up as a one-pixel
/// gap under some wells and not others — the exact thing that makes a grid
/// look hand-placed.
fn distribute(avail: f32, gap: f32, weights: &[u16]) -> Vec<(f32, f32)> {
    let n = weights.len();
    if n == 0 {
        return Vec::new();
    }
    let total: f32 = weights.iter().map(|w| f32::from(*w)).sum();
    let track = (avail - gap * (n - 1) as f32).max(0.0);
    let mut out = Vec::with_capacity(n);
    let mut cum = 0.0f32;
    for (i, w) in weights.iter().enumerate() {
        let start = (track * cum / total).round();
        cum += f32::from(*w);
        let end = (track * cum / total).round();
        out.push((start + gap * i as f32, end - start));
    }
    out
}

/// Place `spec` into `area`, drawing each cell with `frame` and calling
/// `add` once per cell with its row-major index.
///
/// The shared body of [`wells`] and [`sub_wells`]: the only difference
/// between a well and a sub-well is which frame it wears and where its
/// rectangle came from, so the arithmetic lives once. A second copy of it
/// is how the two levels would drift apart.
/// Which nesting level is being drawn: the frame its cells wear, and how
/// deep they sit. Bundled because the two always travel together — a
/// sub-well frame at depth 0 would collide with the well ids above it.
#[derive(Clone, Copy)]
struct Level {
    frame: fn(&Theme) -> egui::Frame,
    /// The same level's compact surface. Carried alongside rather than
    /// chosen at the call site, so a level can never be drawn compact at
    /// one depth and roomy at another.
    mini_frame: fn(&Theme) -> egui::Frame,
    depth: usize,
}

impl Level {
    const WELL: Self = Self {
        frame: design::well,
        mini_frame: design::mini_well,
        depth: 0,
    };
    const SUB: Self = Self {
        frame: design::sub_well,
        mini_frame: design::mini_sub_well,
        depth: 1,
    };
}

fn place(
    ui: &mut egui::Ui,
    theme: &Theme,
    spec: &Wells,
    area: egui::Rect,
    level: Level,
    leaf: &mut usize,
    add: &mut impl FnMut(&mut egui::Ui, usize),
) {
    let g = spec.gap(theme);

    // The wells' remembered content sizes need a STABLE id — an auto id
    // can shift between frames, and then every frame reads as the first:
    // no stored measurement, no centring, controls pinned to the well top.
    // `depth` is in the key so a sub-group never collides with the well it
    // is drawn inside.
    let data_id = ui
        .id()
        .with(("wells", level.depth, spec.rows.len(), spec.cells()));

    let row_weights: Vec<u16> = spec.rows.iter().map(|r| r.weight).collect();
    let row_bands = distribute(area.height(), g, &row_weights);

    let mut index = 0usize;
    for (row, (y, row_h)) in spec.rows.iter().zip(row_bands) {
        let spans: Vec<u16> = row.cells.iter().map(|c| c.span).collect();
        let cols = distribute(area.width(), g, &spans);
        for (cell, (x, cell_w)) in row.cells.iter().zip(cols) {
            let i = index;
            index += 1;
            let rect =
                egui::Rect::from_min_size(area.min + egui::vec2(x, y), egui::vec2(cell_w, row_h));
            let p = spec.cell_pad(cell, theme);
            let inner = egui::vec2((cell_w - p * 2.0).max(0.0), (row_h - p * 2.0).max(0.0));

            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            (cell.frame(level, spec.compact))(theme).show(&mut child, |ui| {
                ui.set_min_size(inner);
                ui.set_max_size(inner);

                // The caption, then everything below it. Painted and then
                // SPACED, so the cursor moves past it and the content
                // area shrinks by exactly what the contract reserved.
                let mut inner = inner;
                if let Some(head) = cell.header {
                    ui.painter().text(
                        ui.max_rect().left_top(),
                        egui::Align2::LEFT_TOP,
                        head.text,
                        egui::FontId::proportional(font::LABEL),
                        theme.text_muted,
                    );
                    let taken = cell.header_h(theme);
                    ui.add_space(taken);
                    inner.y = (inner.y - taken).max(0.0);
                }

                // Centring uses last frame's measurement, SEEDED with the
                // declared need — so a well with a contract is centred on
                // frame ONE. With no contract there is nothing to seed
                // from, and guessing would be worse than not centring: a
                // zero seed pads by half the well and throws the content
                // DOWNWARD past centre, which is a visibly wrong first
                // frame rather than merely an unfinished one. Seeding with
                // the full inner height pads by nothing, so uncontracted
                // content starts at the top and settles into place.
                //
                // A filling cell skips all of it: its content IS the well.
                // A DIVIDED well is not a container for content — it is a
                // container for more wells. Draw its divisions straight
                // into its content area and take no part in centring:
                // they fill it, and their own contents are centred inside
                // them, one level down.
                if cell.is_divided() {
                    // What is LEFT after the caption, not the whole well.
                    let sub_area = ui.available_rect_before_wrap();
                    let mut group = division_group(&cell.division_rows(), Footprint::ZERO);
                    // Divisions inherit the body's rhythm: a compact card
                    // with roomy sub-wells would be compact only in the
                    // parts nobody looks at.
                    group.compact = spec.compact;
                    place(ui, theme, &group, sub_area, Level::SUB, leaf, add);
                    return;
                }

                let well_id = data_id.with(("well", i));
                let last: egui::Vec2 = if cell.fill {
                    inner
                } else {
                    ui.data(|d| d.get_temp(well_id)).unwrap_or({
                        if cell.need.height() > 0.0 {
                            cell.need.size
                        } else {
                            inner
                        }
                    })
                };

                let content = ui
                    .allocate_ui_with_layout(
                        inner,
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.add_space(((inner.y - last.y) * 0.5).max(0.0));
                            // Measure the CONTENT only: the pad shares
                            // this scope, and folding it into the stored
                            // height would feed back into next frame's pad.
                            let top = ui.min_rect().bottom();
                            add(ui, *leaf);
                            let r = ui.min_rect();
                            egui::vec2(r.width(), r.bottom() - top)
                        },
                    )
                    .inner;
                ui.data_mut(|d| d.insert_temp(well_id, content));
                *leaf += 1;
            });
        }
    }
}

/// Lay a card body out as `spec`, calling `add` once per well with its
/// row-major index.
///
/// Every well is a recessed frame sized to an exact rectangle, and its
/// content is centred in both axes. Leave a well empty and it stays a
/// visible, waiting frame rather than collapsing — the shelving is part of
/// the device's face, not a side effect of having content.
///
/// To subdivide one well further, declare it with [`Well::holds`] and draw
/// it with [`sub_wells`].
pub fn wells(
    ui: &mut egui::Ui,
    theme: &Theme,
    spec: &Wells,
    mut add: impl FnMut(&mut egui::Ui, usize),
) {
    if spec.is_empty() {
        return;
    }
    // Width is the spec's own intrinsic width, never `available_width`: a
    // card lives in a horizontally-scrolling rack, where "available" is
    // the whole rack and taking it would balloon the card across it.
    let w = spec.min_width(theme);
    let h = ui.available_height().max(0.0);
    let (area, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    place(ui, theme, spec, area, Level::WELL, &mut 0, &mut add);
}

/// Subdivide the well you are already inside, for separating controls that
/// are related to each other more closely than to the rest of the card.
///
/// Call this from a [`wells`] closure, on a cell declared with
/// [`Well::holds`] and the same `spec`. Sub-wells FILL the well they are
/// in — a partly-subdivided well reads as a mistake — so this takes the
/// whole available rectangle rather than computing its own width the way
/// [`wells`] does.
///
/// One level of nesting. Card, well, sub-well is as deep as the ground
/// ramp goes before a container would be the same colour as the controls
/// standing in it, and a group inside a group inside a group is a card
/// that wants to be two cards.
pub fn sub_wells(
    ui: &mut egui::Ui,
    theme: &Theme,
    spec: &Wells,
    mut add: impl FnMut(&mut egui::Ui, usize),
) {
    if spec.is_empty() {
        return;
    }
    let size = egui::vec2(
        ui.available_width().max(spec.min_width(theme)),
        ui.available_height().max(0.0),
    );
    let (area, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    place(ui, theme, spec, area, Level::SUB, &mut 0, &mut add);
}

/// Divide a card body into `cols`×`rows` equal sub-panels — the common
/// case of [`wells`], and the shape most devices want.
///
/// Every section is the same size. For wells of DIFFERENT sizes that still
/// tile evenly, build a [`Wells`] with spans and call [`wells`] directly.
pub fn sections(
    ui: &mut egui::Ui,
    theme: &Theme,
    cols: usize,
    rows: usize,
    add: impl FnMut(&mut egui::Ui, usize),
) {
    wells(ui, theme, &Wells::uniform(cols, rows), add);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod grip_tests {
    use super::*;

    /// THE DERIVED BAND IS WHERE THE CARD ACTUALLY PUT ITS TITLE.
    ///
    /// A chain holds a card's rect and nothing else, so the handle has to
    /// be derived from outside — and a derivation that drifted from the
    /// strip it describes would put the grab band over the top row of
    /// knobs, which is the exact bug the device UI contract's second rule
    /// is about. Measured against what the card reports so the two cannot
    /// separate.
    #[test]
    fn the_title_band_is_where_the_card_actually_put_it() {
        for density in [
            crate::ui::tokens::Density::Comfortable,
            crate::ui::tokens::Density::Compact,
        ] {
            check_band(density);
        }
    }

    fn check_band(density: crate::ui::tokens::Density) {
        let mut theme = Theme::dark();
        theme.set_density(density);
        let context = egui::Context::default();
        let mut measured = egui::Rect::NOTHING;
        let mut whole = egui::Rect::NOTHING;
        let mut run = context.run_ui(egui::RawInput::default(), |ui| {
            let mut page = 0;
            let (_, title) = tabbed_card_gripped(
                ui,
                &theme,
                "filter",
                crate::ui::tokens::control::DEVICE_H,
                1,
                &mut page,
                |ui, _| {
                    ui.label("body");
                },
            );
            measured = title;
            whole = ui.min_rect();
        });
        // The font atlas the label built has to be collected, or egui
        // panics on the way out of the test rather than in it.
        run.textures_delta.clear();
        assert!(measured.height() > 0.0, "the card reported no title strip");
        let derived = title_band(
            &theme,
            egui::Rect::from_min_size(measured.min, whole.size()),
        );
        assert!(
            (derived.height() - measured.height()).abs() <= 1.5,
            "the derived band is {} and the card's strip is {} — the handle \
             has drifted off the title and onto the controls",
            derived.height(),
            measured.height()
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn frame<R>(ctx: &egui::Context, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let mut f = Some(f);
        let mut out = None;
        let mut run = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                if let Some(f) = f.take() {
                    out = Some(f(ui));
                }
            },
        );
        run.textures_delta.clear();
        out.unwrap()
    }

    /// Collect every well's rects for a spec, on a settled frame, as
    /// (outer, inner): the frame the user sees, and the area its content
    /// gets. The closure only ever sees the inner Ui, so the outer is
    /// reconstructed by adding back the padding the frame took — which is
    /// exactly the relationship the layout math relies on, so a test that
    /// got it wrong would show up here first.
    fn well_rects(spec: &Wells) -> Vec<(egui::Rect, egui::Rect)> {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let pad = design::well_pad(&theme);
        let mut rects = Vec::new();
        for _ in 0..3 {
            rects.clear();
            frame(&ctx, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    wells(ui, &theme, spec, |ui, _| {
                        let inner = ui.max_rect();
                        rects.push((inner.expand(pad), inner));
                    });
                });
            });
        }
        rects
    }

    /// The title strip is the card's only outer chrome. Wells already pad
    /// their controls, so a body inset would create the empty perimeter the
    /// card contract explicitly removes.
    #[test]
    fn the_body_adds_no_second_margin_around_its_wells() {
        let theme = Theme::dark();
        let no_margin: egui::Margin = Default::default();
        assert_eq!(design::body(&theme).inner_margin, no_margin);
        assert_ne!(
            design::title_strip(&theme).inner_margin,
            no_margin,
            "the title keeps its own readable inset"
        );
    }

    /// THE distribution guarantee: a span-2 well is exactly twice a
    /// span-1 well, gaps are uniform, and the row tiles its width with
    /// nothing left over.
    ///
    /// "Evenly distributed wells of different sizes" is precisely this —
    /// different sizes that are still whole multiples of one unit. Widths
    /// that merely came out different are what you get when each column
    /// hugs its own content, and they read as an accident.
    #[test]
    fn wells_of_different_spans_tile_in_exact_proportion() {
        let theme = Theme::dark();
        let gap = design::gap(&theme);
        let spec = Wells::new().row([Well::span(2), Well::one(), Well::one()]);
        let r: Vec<egui::Rect> = well_rects(&spec).into_iter().map(|(o, _)| o).collect();
        assert_eq!(r.len(), 3);

        // Span 2 is twice span 1, to within the one pixel that rounding
        // to whole device pixels can cost.
        assert!(
            (r[0].width() - r[1].width() * 2.0).abs() <= 1.0,
            "span-2 {} should be twice span-1 {}",
            r[0].width(),
            r[1].width()
        );
        // The two unit wells are the same width as each other.
        assert!((r[1].width() - r[2].width()).abs() <= 1.0);
        // Uniform gaps, and no overlap.
        assert!((r[1].left() - r[0].right() - gap).abs() <= 0.5);
        assert!((r[2].left() - r[1].right() - gap).abs() <= 0.5);
        // Nothing left over: the wells plus the gaps ARE the total width.
        let total = r[2].right() - r[0].left();
        let sum: f32 = r.iter().map(|x| x.width()).sum();
        assert!(
            (total - (sum + gap * 2.0)).abs() <= 0.5,
            "the row must tile exactly: total {total}, wells {sum}"
        );
        // All one height, all one top.
        assert!(r.iter().all(|x| (x.top() - r[0].top()).abs() < 0.5));
        assert!(r.iter().all(|x| (x.height() - r[0].height()).abs() < 0.5));
    }

    /// Rows share the height by weight the same way columns share width,
    /// and rows of DIFFERENT column counts still line up on both edges.
    #[test]
    fn rows_share_height_by_weight_and_align_on_both_edges() {
        let theme = Theme::dark();
        let gap = design::gap(&theme);
        // A double-height row of two, over a single-height row of three.
        let spec = Wells::new()
            .row_weighted(2, [Well::one(), Well::one()])
            .row_weighted(1, [Well::one(), Well::one(), Well::one()]);
        let r: Vec<egui::Rect> = well_rects(&spec).into_iter().map(|(o, _)| o).collect();
        assert_eq!(r.len(), 5);

        let tall = r[0].height();
        let short = r[2].height();
        assert!(
            (tall - short * 2.0).abs() <= 1.0,
            "a weight-2 row is twice a weight-1 row: {tall} vs {short}"
        );
        assert!((r[2].top() - r[0].bottom() - gap).abs() <= 0.5, "row gap");

        // Both rows start and end on the same x — a body whose rows had
        // different widths would not read as one card face.
        assert!((r[0].left() - r[2].left()).abs() <= 0.5);
        assert!((r[1].right() - r[4].right()).abs() <= 0.5);
    }

    /// A well is never smaller than the contract of what goes in it. This
    /// is the anti-clipping guarantee: declare a footprint, and the well
    /// has room for it plus its own padding.
    #[test]
    fn a_well_is_never_smaller_than_its_declared_content() {
        let wide = Footprint::new(180.0, 40.0);
        let spec = Wells::new().row([Well::one().fits(wide), Well::one(), Well::one()]);
        let r = well_rects(&spec);

        for (i, (_, inner)) in r.iter().enumerate() {
            assert!(
                inner.width() >= wide.width() - 1.0,
                "well {i}'s content area ({}) cannot hold the declared {}",
                inner.width(),
                wide.width()
            );
        }
        // Equal spans stay equal: one greedy cell widens the UNIT, so all
        // three grow together rather than the row going ragged.
        assert!((r[0].0.width() - r[1].0.width()).abs() <= 1.0);
    }

    /// A span-2 well needs only half the unit width a span-1 well does for
    /// the same content — so putting the widest control in the widest well
    /// makes the card NARROWER, not wider.
    #[test]
    fn a_wider_span_costs_less_card_width_for_the_same_content() {
        let theme = Theme::dark();
        let big = Footprint::new(180.0, 40.0);
        let in_one = Wells::new().row([Well::one().fits(big), Well::one(), Well::one()]);
        let in_two = Wells::new().row([Well::span(2).fits(big), Well::one()]);
        assert!(
            in_two.min_width(&theme) < in_one.min_width(&theme),
            "a span-2 well should cost less total width: {} vs {}",
            in_two.min_width(&theme),
            in_one.min_width(&theme)
        );
    }

    /// `min_height` is the number a device is checked against, because a
    /// card's height is locked and cannot grow to fit.
    #[test]
    fn min_height_accounts_for_padding_and_row_weights() {
        let theme = Theme::dark();
        let pad = design::well_pad(&theme);
        let gap = design::gap(&theme);
        let tall = Footprint::new(20.0, 50.0);

        let one = Wells::new().row([Well::one().fits(tall)]);
        assert!((one.min_height(&theme) - (50.0 + pad * 2.0)).abs() < 0.5);

        // The same content in the SHORTER of two rows needs the body to be
        // twice as tall, plus the row gap.
        let split = Wells::new()
            .row_weighted(1, [Well::one().fits(tall)])
            .row_weighted(1, [Well::one()]);
        assert!((split.min_height(&theme) - ((50.0 + pad * 2.0) * 2.0 + gap)).abs() < 0.5);
    }

    /// Both shipping devices fit the locked card height. This is what
    /// `min_height` is FOR — a device whose controls do not fit
    /// `DEVICE_H` should fail here, not clip silently on someone's screen.
    #[test]
    fn the_shipping_devices_fit_the_locked_card_height() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        // The body is the card minus its title strip and body padding;
        // measuring the card itself is the honest check, so drive the real
        // cards and assert nothing overflowed their well.
        let (synth_h, reverb_h) = frame(&ctx, |ui| {
            let s = crate::ui::device::param::Param::ms("release", 1.0, 30_000.0);
            let k = crate::ui::device::knob::footprint(ui, &theme, &s);
            let three = Wells::new().row([
                Well::one().fits(k),
                Well::one().fits(k),
                Well::one().fits(k),
            ]);
            let mixed = Wells::new().row([
                Well::span(2).fits(k),
                Well::one().fits(k),
                Well::one().fits(k),
            ]);
            (mixed.min_height(&theme), three.min_height(&theme))
        });
        let card_h = theme.sp(control::DEVICE_H);
        assert!(
            synth_h < card_h && reverb_h < card_h,
            "a device must fit the locked card height: synth {synth_h}, \
             reverb {reverb_h}, card {card_h}"
        );
    }

    /// A well with a declared contract is centred on frame ONE. Without a
    /// contract there is nothing to seed from and it settles after two —
    /// which is the case the older test below covers.
    #[test]
    fn a_contracted_well_centres_on_the_first_frame() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let content = egui::vec2(40.0, 60.0);
        let spec = Wells::new().row([Well::one().fits(Footprint::from_size(content))]);

        let (rect, well) = frame(&ctx, |ui| {
            let mut out = None;
            egui::CentralPanel::default().show(ui, |ui| {
                wells(ui, &theme, &spec, |ui, _| {
                    let well = ui.max_rect();
                    let (r, _) = ui.allocate_exact_size(content, egui::Sense::hover());
                    out = Some((r, well));
                });
            });
            out.unwrap()
        });
        assert!(
            (rect.center().y - well.center().y).abs() < 2.0,
            "a declared footprint must centre immediately: {rect:?} in {well:?}"
        );
    }

    /// Collect (outer, inner) rects for a nested layout: the outer wells
    /// first, then the sub-wells of whichever cell `nest_at` names.
    fn nested_rects(
        outer: &Wells,
        inner_spec: &Wells,
        nest_at: usize,
    ) -> (Vec<egui::Rect>, Vec<egui::Rect>) {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let pad = design::well_pad(&theme);
        let (mut outers, mut inners) = (Vec::new(), Vec::new());
        for _ in 0..3 {
            outers.clear();
            inners.clear();
            frame(&ctx, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    wells(ui, &theme, outer, |ui, i| {
                        outers.push(ui.max_rect().expand(pad));
                        if i == nest_at {
                            sub_wells(ui, &theme, inner_spec, |ui, _| {
                                inners.push(ui.max_rect().expand(pad));
                            });
                        }
                    });
                });
            });
        }
        (outers, inners)
    }

    /// Every well rect a spec draws, outer and inner, on a settled frame.
    /// Divisions are included — they are wells too, one level down.
    fn drawn_rects(spec: &Wells) -> Vec<(egui::Rect, egui::Rect)> {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let pad = design::well_pad(&theme);
        let mut rects = Vec::new();
        for _ in 0..3 {
            rects.clear();
            frame(&ctx, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    wells(ui, &theme, spec, |ui, _| {
                        let inner = ui.max_rect();
                        rects.push((inner.expand(pad), inner));
                    });
                });
            });
        }
        rects
    }

    /// A well divides into equal parts on BOTH axes: 2, 3, 4 columns;
    /// 2, 3, 4 rows; and grids of the two together.
    ///
    /// "Equal" is the whole definition of a division: a well split into
    /// unequal parts is a nested layout with weights, which is what
    /// `holds` is for. Sweeping the shapes is what stops this from being
    /// tested at 3x1 and quietly wrong at 2x3.
    #[test]
    fn a_well_divides_into_an_equal_grid_at_any_shape() {
        let theme = Theme::dark();
        let gap = design::gap(&theme);
        let pad = design::well_pad(&theme);
        let leaf = Footprint::new(30.0, 20.0);

        for (cols, rows) in [
            (2u16, 1u16),
            (3, 1),
            (4, 1),
            (6, 1),
            (1, 2),
            (1, 3),
            (1, 4),
            (2, 2),
            (3, 2),
            (2, 3),
            (4, 3),
        ] {
            let shape = format!("{cols}x{rows}");
            let n = usize::from(cols) * usize::from(rows);
            let spec = Wells::new().row([
                Well::one().fits(leaf),
                Well::divided(cols, rows).each(leaf, &theme),
            ]);
            assert_eq!(
                spec.len(),
                1 + n,
                "{shape}: a divided well contributes {n} leaves, not one"
            );
            assert_eq!(spec.cells(), 2, "{shape}: but it is still one CELL");

            let r = drawn_rects(&spec);
            assert_eq!(r.len(), 1 + n, "{shape}");
            let divs: Vec<egui::Rect> = r[1..].iter().map(|(o, _)| *o).collect();

            // Every division is the same size as every other.
            for d in &divs {
                assert!(
                    (d.width() - divs[0].width()).abs() <= 1.0
                        && (d.height() - divs[0].height()).abs() <= 1.0,
                    "{shape}: divisions must be equal, got {:?}",
                    divs.iter().map(|d| d.size()).collect::<Vec<_>>()
                );
            }

            // ROW-MAJOR order, with uniform gaps along each axis.
            for r_i in 0..usize::from(rows) {
                for c_i in 0..usize::from(cols) {
                    let here = divs[r_i * usize::from(cols) + c_i];
                    if c_i > 0 {
                        let prev = divs[r_i * usize::from(cols) + c_i - 1];
                        assert!(
                            (here.left() - prev.right() - gap).abs() <= 0.5,
                            "{shape}: column seam at ({c_i},{r_i})"
                        );
                        assert!(
                            (here.top() - prev.top()).abs() <= 0.5,
                            "{shape}: a row must be level"
                        );
                    }
                    if r_i > 0 {
                        let above = divs[(r_i - 1) * usize::from(cols) + c_i];
                        assert!(
                            (here.top() - above.bottom() - gap).abs() <= 0.5,
                            "{shape}: row seam at ({c_i},{r_i})"
                        );
                        assert!(
                            (here.left() - above.left()).abs() <= 0.5,
                            "{shape}: a column must be plumb"
                        );
                    }
                }
            }

            // Each division has room for what it declared, both ways.
            for d in &divs {
                assert!(
                    d.width() - pad * 2.0 >= leaf.width() - 1.0
                        && d.height() - pad * 2.0 >= leaf.height() - 1.0,
                    "{shape}: division {:?} cannot hold the declared {:?}",
                    d.size(),
                    leaf.size
                );
            }
        }
    }

    /// Compact wells are genuinely tighter, on both axes, for the same
    /// content — and the saving is measured rather than assumed.
    #[test]
    fn compact_wells_take_less_room_for_the_same_content() {
        let theme = Theme::dark();
        let leaf = Footprint::new(30.0, 26.0);
        let plain = Wells::new().row([Well::one().fits(leaf), Well::one().fits(leaf)]);
        let tight = Wells::new()
            .row([Well::one().fits(leaf), Well::one().fits(leaf)])
            .compact();

        assert!(!plain.is_compact() && tight.is_compact());
        assert!(
            tight.min_width(&theme) < plain.min_width(&theme),
            "compact must be narrower: {} vs {}",
            tight.min_width(&theme),
            plain.min_width(&theme)
        );
        assert!(tight.min_height(&theme) < plain.min_height(&theme));
        // The saving is real, not a rounding difference: at minimum the
        // tighter gap plus the padding taken off each of the two wells.
        let saved = plain.min_width(&theme) - tight.min_width(&theme);
        let least = (design::gap(&theme) - design::mini_gap(&theme))
            + (design::well_pad(&theme) - design::mini_pad(&theme)) * 4.0;
        assert!(
            saved >= least - 0.5,
            "saved {saved}, expected at least {least}"
        );
    }

    /// A compact well still HOLDS what it declared. Tighter chrome must
    /// come out of the frame, never out of the content.
    #[test]
    fn a_compact_well_still_fits_its_contract() {
        let theme = Theme::dark();
        let leaf = Footprint::new(60.0, 40.0);
        let spec = Wells::new()
            .row([
                Well::one().fits(leaf),
                Well::divided(2, 1).each_compact(leaf, &theme),
            ])
            .compact();
        let ctx = egui::Context::default();
        let pad = design::mini_pad(&theme);
        let mut inners = Vec::new();
        for _ in 0..3 {
            inners.clear();
            frame(&ctx, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    wells(ui, &theme, &spec, |ui, _| inners.push(ui.max_rect()));
                });
            });
        }
        assert_eq!(inners.len(), 3, "one plain well and two divisions");
        for (i, r) in inners.iter().enumerate() {
            assert!(
                r.width() >= leaf.width() - 1.0,
                "well {i} ({}) cannot hold the declared {}",
                r.width(),
                leaf.width()
            );
        }
        let _ = pad;
    }

    /// Divisions INHERIT the body's density. A compact card with roomy
    /// sub-wells would be compact only in the parts nobody looks at, and
    /// the contract would over-reserve for a frame that never gets drawn.
    #[test]
    fn divisions_inherit_the_bodys_density() {
        let theme = Theme::dark();
        let leaf = Footprint::new(30.0, 26.0);
        let roomy = Wells::new().row([Well::divided(2, 2).each(leaf, &theme)]);
        let tight = Wells::new()
            .row([Well::divided(2, 2).each_compact(leaf, &theme)])
            .compact();
        assert!(
            tight.min_height(&theme) < roomy.min_height(&theme),
            "a compact 2x2 must be shorter: {} vs {}",
            tight.min_height(&theme),
            roomy.min_height(&theme)
        );
        assert!(tight.min_width(&theme) < roomy.min_width(&theme));

        // And `each` at the wrong density over-reserves — which is the
        // mistake `each_compact` exists to prevent.
        let mismatched = Wells::new()
            .row([Well::divided(2, 2).each(leaf, &theme)])
            .compact();
        assert!(
            mismatched.min_width(&theme) > tight.min_width(&theme),
            "sizing roomy and drawing compact reserves more than it needs"
        );
    }

    /// A header is reserved ON TOP of the content, never out of it.
    ///
    /// The failure this guards against is silent: a caption that eats the
    /// room its controls were promised does not error, it just crowds
    /// them, and only a screenshot at one density would ever show it.
    #[test]
    fn a_header_is_reserved_on_top_of_the_content() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let gap = design::gap(&theme);
        let leaf = Footprint::new(40.0, 30.0);

        let (bare_h, titled_h, line) = frame(&ctx, |ui| {
            let bare = Wells::new().row([Well::one().fits(leaf)]);
            let titled = Wells::new().row([Well::one().fits(leaf).titled("envelope", ui, &theme)]);
            (
                bare.min_height(&theme),
                titled.min_height(&theme),
                metrics::line_h(ui, font::LABEL),
            )
        });
        assert!(
            (titled_h - (bare_h + line + gap)).abs() < 0.5,
            "a header costs exactly a line plus a gap: {titled_h} vs {bare_h}"
        );
    }

    /// A long caption widens its well. A header that clips is worse than
    /// no header — and the well is the only thing that can grow.
    #[test]
    fn a_long_header_widens_its_well() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let pad = design::well_pad(&theme);
        let narrow = Footprint::new(10.0, 30.0);

        let (short_w, long_w, long_text_w) = frame(&ctx, |ui| {
            let short = Wells::new().row([Well::one().fits(narrow).titled("mix", ui, &theme)]);
            let long = Wells::new().row([Well::one().fits(narrow).titled(
                "amplitude envelope",
                ui,
                &theme,
            )]);
            (
                short.min_width(&theme),
                long.min_width(&theme),
                metrics::text_w(ui, "amplitude envelope", font::LABEL),
            )
        });
        assert!(long_w > short_w, "{long_w} should exceed {short_w}");
        assert!(
            long_w - pad * 2.0 >= long_text_w - 1.0,
            "the well ({long_w}) cannot show its own caption ({long_text_w})"
        );
    }

    /// The caption sits ABOVE the content, and the content keeps the room
    /// it declared — in a plain well and in a divided one alike.
    #[test]
    fn content_starts_below_the_header() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let pad = design::well_pad(&theme);
        let leaf = Footprint::new(40.0, 30.0);

        // A divided, titled tray: its divisions must clear the caption.
        let mut divs: Vec<egui::Rect> = Vec::new();
        let mut host = egui::Rect::NOTHING;
        for _ in 0..3 {
            divs.clear();
            frame(&ctx, |ui| {
                let spec = Wells::new().row([Well::divided(2, 1)
                    .each(leaf, &theme)
                    .titled("envelope", ui, &theme)]);
                egui::CentralPanel::default().show(ui, |ui| {
                    host = egui::Rect::NOTHING;
                    wells(ui, &theme, &spec, |ui, _| {
                        divs.push(ui.max_rect().expand(pad));
                    });
                });
            });
        }
        assert_eq!(divs.len(), 2);
        let line = frame(&ctx, |ui| metrics::line_h(ui, font::LABEL));
        let gap = design::gap(&theme);
        // The tray's own top is the divisions' top minus the tray padding
        // minus the header — reconstructing it proves the header was
        // actually taken out of the content area rather than drawn over it.
        let tray_pad = design::group_pad(&theme);
        let tray_top = divs[0].top() - tray_pad - line - gap;
        assert!(
            divs[0].top() - tray_top > line,
            "the divisions must clear the caption"
        );
        // Both divisions still hold what they declared.
        for d in &divs {
            assert!(d.height() - pad * 2.0 >= leaf.height() - 1.0);
        }
    }

    /// An untitled well is exactly what it always was — the header is
    /// genuinely optional, not a zero-height row that shifts everything.
    #[test]
    fn an_untitled_well_is_unchanged() {
        let theme = Theme::dark();
        let leaf = Footprint::new(40.0, 30.0);
        let w = Well::one().fits(leaf);
        assert_eq!(w.header_h(&theme), 0.0);
        assert_eq!(w.header_w(), 0.0);

        let spec = Wells::new().row([w.clone(), Well::divided(2, 1).each(leaf, &theme)]);
        let before = (spec.min_width(&theme), spec.min_height(&theme));
        // Same layout, built the same way, must measure the same.
        let again = Wells::new().row([w, Well::divided(2, 1).each(leaf, &theme)]);
        assert_eq!(before, (again.min_width(&theme), again.min_height(&theme)));
    }

    /// A FORMATION: rows of different widths, three over four.
    ///
    /// The thing a uniform grid cannot express. Each row divides the same
    /// width by its OWN count, so the rows deliberately do not share
    /// column edges — three thirds over four quarters. A test that only
    /// checked equal counts would never notice that.
    #[test]
    fn rows_of_different_widths_each_divide_the_full_width() {
        let theme = Theme::dark();
        let gap = design::gap(&theme);
        let leaf = Footprint::new(24.0, 18.0);

        for counts in [
            vec![3u16, 4],
            vec![4, 3],
            vec![1, 3],
            vec![2, 3, 4],
            vec![5, 1],
        ] {
            let name = format!("{counts:?}");
            let n: usize = counts.iter().map(|c| usize::from(*c)).sum();
            let spec = Wells::new().row([Well::rows_of(counts.clone()).each(leaf, &theme)]);
            assert_eq!(spec.len(), n, "{name}: one leaf per cell in every row");

            let r = drawn_rects(&spec);
            assert_eq!(r.len(), n, "{name}");
            let cells: Vec<egui::Rect> = r.iter().map(|(o, _)| *o).collect();

            // Walk the rows in the order the counts describe.
            let mut at = 0usize;
            let mut rows: Vec<Vec<egui::Rect>> = Vec::new();
            for c in &counts {
                rows.push(cells[at..at + usize::from(*c)].to_vec());
                at += usize::from(*c);
            }

            for (i, row) in rows.iter().enumerate() {
                // Equal WITHIN the row, and level.
                for cell in row {
                    assert!(
                        (cell.width() - row[0].width()).abs() <= 1.0,
                        "{name}: row {i} is not equally divided"
                    );
                    assert!(
                        (cell.top() - row[0].top()).abs() <= 0.5,
                        "{name}: row {i} level"
                    );
                }
                // Uniform gaps within the row.
                for k in 1..row.len() {
                    let seam = row[k].left() - row[k - 1].right();
                    assert!((seam - gap).abs() <= 0.5, "{name}: row {i} seam {seam}");
                }
            }

            // EVERY row spans the same width — that is what makes them
            // rows of one well rather than separate wells.
            let left = rows[0][0].left();
            let right = rows[0][rows[0].len() - 1].right();
            for (i, row) in rows.iter().enumerate() {
                assert!(
                    (row[0].left() - left).abs() <= 1.0,
                    "{name}: row {i} starts elsewhere"
                );
                assert!(
                    (row[row.len() - 1].right() - right).abs() <= 1.0,
                    "{name}: row {i} ends elsewhere"
                );
            }

            // Rows stack in order, one gap apart.
            for i in 1..rows.len() {
                let seam = rows[i][0].top() - rows[i - 1][0].bottom();
                assert!((seam - gap).abs() <= 0.5, "{name}: row seam {seam}");
            }

            // Rows of DIFFERENT counts must not share column edges — if
            // they did, this would be a table, not a formation.
            if counts.windows(2).any(|w| w[0] != w[1]) {
                let a = &rows[0];
                let b = rows.iter().find(|r| r.len() != a.len()).unwrap();
                assert!(
                    (a[0].width() - b[0].width()).abs() > 1.0,
                    "{name}: rows of different counts must divide differently"
                );
            }
        }
    }

    /// A formation's width is set by its WIDEST row, and its span follows,
    /// so the well still lands on the card's unit grid where it is widest.
    #[test]
    fn a_formation_is_sized_by_its_widest_row() {
        let theme = Theme::dark();
        let leaf = Footprint::new(24.0, 18.0);
        assert_eq!(Well::rows_of([3, 4]).span, 4);
        assert_eq!(Well::rows_of([5, 2]).span, 5);

        // Three over four needs what four across needs, not what three does.
        let three_four = Wells::new().row([Well::rows_of([3, 4]).each(leaf, &theme)]);
        let four = Wells::new().row([Well::divided(4, 1).each(leaf, &theme)]);
        let three = Wells::new().row([Well::divided(3, 1).each(leaf, &theme)]);
        assert!((three_four.min_width(&theme) - four.min_width(&theme)).abs() < 0.5);
        assert!(three_four.min_width(&theme) > three.min_width(&theme));

        // And it is two rows tall.
        let two_rows = Wells::new().row([Well::divided(4, 2).each(leaf, &theme)]);
        assert!((three_four.min_height(&theme) - two_rows.min_height(&theme)).abs() < 0.5);
    }

    /// Dividing costs size on the axis it divides, and only that axis:
    /// columns reserve width, rows reserve height. A layout where adding a
    /// row silently widened the card would make the contract useless for
    /// deciding whether a device fits.
    #[test]
    fn divisions_reserve_size_on_the_axis_they_divide() {
        let theme = Theme::dark();
        let pad = design::well_pad(&theme);
        let gap = design::gap(&theme);
        let leaf = Footprint::new(40.0, 30.0);
        let group =
            |cols: u16, rows: u16| Wells::new().row([Well::divided(cols, rows).each(leaf, &theme)]);

        // More columns: wider, same height.
        let (c2, c3, c4) = (group(2, 1), group(3, 1), group(4, 1));
        assert!(c2.min_width(&theme) < c3.min_width(&theme));
        assert!(c3.min_width(&theme) < c4.min_width(&theme));
        assert!((c2.min_height(&theme) - c4.min_height(&theme)).abs() < 0.5);

        // More rows: taller, same width.
        let (r2, r4) = (group(1, 2), group(1, 4));
        assert!(r2.min_height(&theme) < r4.min_height(&theme));
        assert!((r2.min_width(&theme) - r4.min_width(&theme)).abs() < 0.5);

        // Exactly: n divisions, each with its own well padding, plus the
        // gaps between them — and then the tray's OWN padding once around
        // the outside. A divided well wears the roomier tray (SM, not XS),
        // so the outer padding differs from the inner and the two must not
        // be conflated; doing so is how a nested measurement quietly
        // double-counts its container.
        let leaf_cell = leaf.height() + pad * 2.0;
        for n in 1..=4u16 {
            let g = group(1, n);
            let outer = if n > 1 {
                design::group_pad(&theme)
            } else {
                pad
            };
            let inner = g.min_height(&theme) - outer * 2.0;
            let want = leaf_cell * f32::from(n) + gap * f32::from(n - 1);
            assert!(
                (inner - want).abs() < 0.5,
                "{n} rows: content height {inner}, wanted {want}"
            );
        }

        // And every division still fits its leaf: the growth is real, not
        // a squeeze spread over more cells.
        for (cols, rows) in [(2u16, 1u16), (3, 1), (4, 1), (2, 2), (3, 2)] {
            let g = group(cols, rows);
            let per_w = (g.min_width(&theme) - gap * f32::from(cols - 1)) / f32::from(cols);
            let per_h = (g.min_height(&theme) - gap * f32::from(rows - 1)) / f32::from(rows);
            assert!(
                per_w - pad * 2.0 >= leaf.width() - 1.0,
                "{cols}x{rows} width"
            );
            assert!(
                per_h - pad * 2.0 >= leaf.height() - 1.0,
                "{cols}x{rows} height"
            );
        }
    }

    /// `divided(cols, rows)` ties the span to the COLUMN count, so its
    /// parts read as siblings of the single-unit wells beside them. Rows
    /// do not touch the span — they divide height, which the card's locked
    /// height already fixes. Span and grid can still be set apart.
    #[test]
    fn divided_ties_span_to_the_column_count_only() {
        let a = Well::divided(3, 1);
        assert_eq!(a.span, 3);
        assert_eq!(a.division_rows(), vec![3]);

        // Rows leave the span alone: a 2x3 is still two units wide.
        let g = Well::divided(2, 3);
        assert_eq!(g.span, 2, "rows divide height, not width");
        assert_eq!(g.division_rows(), vec![2, 2, 2]);

        // Independently, when asked.
        let b = Well::span(4).split(2, 2);
        assert_eq!(b.span, 4);
        assert_eq!(b.division_rows(), vec![2, 2]);

        // Undivided by default, and a 1x1 "grid" is not a division.
        assert_eq!(Well::one().division_rows(), vec![1]);
        let one = Wells::new().row([Well::one().split(1, 1)]);
        assert_eq!(one.len(), 1, "dividing in one is not dividing");
        // A zero on either axis is read as one, not as nothing.
        assert_eq!(Wells::new().row([Well::one().split(0, 0)]).len(), 1);

        // `each` implies filling — a division grid is never centred.
        assert!(
            Well::divided(2, 2)
                .each(Footprint::new(10.0, 10.0), &Theme::dark())
                .fill
        );
    }

    /// Sub-wells FILL the well they are in: they start and end on its
    /// content edges, with nothing of the parent showing past them.
    ///
    /// A partly-subdivided well reads as a mistake rather than a choice,
    /// and it is the visible symptom of the centring feedback loop that
    /// `Well::fill` exists to prevent.
    #[test]
    fn sub_wells_fill_the_well_they_divide() {
        let theme = Theme::dark();
        let pad = design::well_pad(&theme);
        let gap = design::gap(&theme);
        let leaf = Footprint::new(50.0, 40.0);

        let group = Wells::new().row([Well::one().fits(leaf), Well::one().fits(leaf)]);
        let outer = Wells::new().row([Well::one().fits(leaf), Well::span(2).holds(&group, &theme)]);
        let (o, i) = nested_rects(&outer, &group, 1);
        assert_eq!(o.len(), 2);
        assert_eq!(i.len(), 2, "the group drew both of its sub-wells");

        // The parent's CONTENT area — its rect minus its own padding — is
        // exactly what the sub-wells span.
        let host = o[1].shrink(pad);
        assert!(
            (i[0].left() - host.left()).abs() <= 1.0,
            "sub-wells start at the parent's content edge: {:?} in {host:?}",
            i[0]
        );
        assert!((i[1].right() - host.right()).abs() <= 1.0);
        assert!(
            (i[0].top() - host.top()).abs() <= 1.0 && (i[0].height() - host.height()).abs() <= 1.0,
            "a filling group takes the full height, uncentred: {:?} in {host:?}",
            i[0]
        );
        // And they tile between themselves the same way wells do.
        assert!((i[1].left() - i[0].right() - gap).abs() <= 0.5);
        assert!((i[0].width() - i[1].width()).abs() <= 1.0);
    }

    /// Sub-well spans work exactly like well spans — nesting changes the
    /// depth, not the rules.
    #[test]
    fn sub_wells_honour_spans_like_wells_do() {
        let theme = Theme::dark();
        let leaf = Footprint::new(40.0, 30.0);
        let group = Wells::new().row([Well::span(2).fits(leaf), Well::one().fits(leaf)]);
        let outer = Wells::new().row([Well::span(3).holds(&group, &theme)]);
        let (_, i) = nested_rects(&outer, &group, 0);
        assert_eq!(i.len(), 2);
        assert!(
            (i[0].width() - i[1].width() * 2.0).abs() <= 1.0,
            "a span-2 sub-well is twice a span-1: {} vs {}",
            i[0].width(),
            i[1].width()
        );
    }

    /// The contract travels OUT through the nesting: a sub-well's content
    /// widens its group, which widens the parent well, which widens the
    /// card. Nothing is clipped at either level.
    #[test]
    fn a_nested_contract_reaches_the_card_width() {
        let theme = Theme::dark();
        let pad = design::well_pad(&theme);
        let small = Footprint::new(30.0, 30.0);
        let big = Footprint::new(200.0, 30.0);

        let tight = Wells::new().row([Well::one().fits(small), Well::one().fits(small)]);
        let roomy = Wells::new().row([Well::one().fits(big), Well::one().fits(small)]);

        let with_tight =
            Wells::new().row([Well::one().fits(small), Well::span(2).holds(&tight, &theme)]);
        let with_roomy =
            Wells::new().row([Well::one().fits(small), Well::span(2).holds(&roomy, &theme)]);
        assert!(
            with_roomy.min_width(&theme) > with_tight.min_width(&theme),
            "a wide control two levels down must widen the card: {} vs {}",
            with_roomy.min_width(&theme),
            with_tight.min_width(&theme)
        );

        // And it actually fits when drawn: the sub-well holding `big` has
        // room for it, padding and all.
        let (_, i) = nested_rects(&with_roomy, &roomy, 1);
        assert!(
            i[0].width() - pad * 2.0 >= big.width() - 1.0,
            "the nested sub-well ({}) cannot hold its declared {}",
            i[0].width() - pad * 2.0,
            big.width()
        );
    }

    /// A group's footprint is its own `min_width`/`min_height` — which is
    /// what `Well::holds` reads, and why nesting needs no special sizing
    /// rule. It also sets `fill`, because a group is never centred.
    #[test]
    fn holds_takes_its_size_from_the_group() {
        let theme = Theme::dark();
        let leaf = Footprint::new(60.0, 44.0);
        let group = Wells::new().row([Well::one().fits(leaf), Well::one().fits(leaf)]);
        let cell = Well::span(2).holds(&group, &theme);

        assert_eq!(cell.need, group.footprint(&theme));
        assert_eq!(cell.need.width(), group.min_width(&theme));
        assert_eq!(cell.need.height(), group.min_height(&theme));
        assert!(cell.fill, "a group fills its well rather than centring");
        assert_eq!(cell.span, 2, "holds leaves the span alone");
        // A plain well does NOT fill.
        assert!(!Well::one().fits(leaf).fill);
    }

    /// The nested synth card still fits the locked card height. The whole
    /// point of `min_height` is that a device which no longer fits fails
    /// here rather than clipping on someone's screen — and adding a level
    /// of nesting is exactly the kind of change that could break it.
    #[test]
    fn the_nested_synth_layout_fits_the_locked_card_height() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let needed = frame(&ctx, |ui| {
            let gain = crate::ui::device::param::Param::new(
                "gain",
                crate::ui::device::Mapping::Linear { min: 0.0, max: 2.0 },
                crate::ui::device::Unit::Plain,
            );
            let release = crate::ui::device::param::Param::ms("release", 1.0, 30_000.0);
            let k = |p| crate::ui::device::knob::footprint(ui, &theme, p);
            let envelope =
                Wells::new().row([Well::one().fits(k(&release)), Well::one().fits(k(&release))]);
            Wells::new()
                .row([
                    Well::one().fits(k(&gain)),
                    Well::span(2).holds(&envelope, &theme),
                ])
                .min_height(&theme)
        });
        let card_h = theme.sp(control::DEVICE_H);
        assert!(
            needed < card_h,
            "the nested synth needs {needed} but a card is only {card_h} tall"
        );
    }

    /// `distribute` is the tiling primitive, and rounding is where these
    /// go wrong: widths rounded independently drift, leaving a seam under
    /// some wells and not others.
    #[test]
    fn distribute_tiles_exactly_at_any_weights() {
        for weights in [
            vec![1u16, 1, 1],
            vec![2, 1],
            vec![1, 2, 1],
            vec![3, 1, 1, 2],
            vec![1],
            vec![1, 1, 1, 1, 1, 1, 1],
        ] {
            let avail = 317.0; // deliberately not divisible by anything
            let gap = 4.0;
            let parts = distribute(avail, gap, &weights);
            assert_eq!(parts.len(), weights.len());

            // Every part is positive, and consecutive parts are separated
            // by exactly one gap.
            for i in 1..parts.len() {
                let seam = parts[i].0 - (parts[i - 1].0 + parts[i - 1].1);
                assert!(
                    (seam - gap).abs() < 0.01,
                    "weights {weights:?}: seam {seam} should be the gap {gap}"
                );
            }
            // The last part ends exactly at the far edge: no leftover.
            let end = parts[parts.len() - 1].0 + parts[parts.len() - 1].1;
            assert!(
                (end - avail).abs() < 0.01,
                "weights {weights:?}: ended at {end}, wanted {avail}"
            );
            // Proportions hold to within the rounding.
            let track = avail - gap * (weights.len() - 1) as f32;
            let total: f32 = weights.iter().map(|w| f32::from(*w)).sum();
            for (i, w) in weights.iter().enumerate() {
                let want = track * f32::from(*w) / total;
                assert!(
                    (parts[i].1 - want).abs() <= 1.0,
                    "weights {weights:?}: part {i} was {} not {want}",
                    parts[i].1
                );
            }
        }
        assert!(distribute(100.0, 4.0, &[]).is_empty());
    }

    /// Drive the well-centering two-pass headlessly: after the first
    /// settle frame, fixed-size content must sit vertically centered in
    /// its well, and stay put on later frames.
    #[test]
    fn wells_center_their_content_after_one_settle_frame() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };

        let mut tops: Vec<f32> = Vec::new();
        let mut well_tops: Vec<f32> = Vec::new();
        // (content rect, well rect) per frame, for the centering asserts.
        let mut pairs: Vec<(egui::Rect, egui::Rect)> = Vec::new();
        for _ in 0..4 {
            let mut out = ctx.run_ui(input.take(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    sections(ui, &theme, 1, 1, |ui, _| {
                        let well = ui.max_rect();
                        well_tops.push(well.top());
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(40.0, 80.0), egui::Sense::hover());
                        tops.push(r.top());
                        pairs.push((r, well));
                    });
                });
            });
            out.textures_delta.clear();
        }

        // Frame 1 has no measurement yet (content pinned high); by frame 3
        // the offset must be real and stable.
        assert!(
            tops[2] > tops[0] + 20.0,
            "no centering happened: tops = {tops:?}, wells = {well_tops:?}"
        );
        assert!(
            (tops[3] - tops[2]).abs() < 1.0,
            "centering did not settle: tops = {tops:?}"
        );

        // Both axes, on the settled frame: the content's center must sit
        // on the well's center. (The horizontal half regressed once — a
        // Grid cell is a horizontal Ui, so a `vertical()` child sizes to
        // its content and centering inside it centers nothing.)
        let (content, well) = pairs[3];
        let _ = &well_tops;
        assert!(
            (content.center().y - well.center().y).abs() < 2.0,
            "content not vertically centered: content {content:?} well {well:?}"
        );
        assert!(
            (content.center().x - well.center().x).abs() < 2.0,
            "content not horizontally centered: content {content:?} well {well:?}"
        );
    }

    /// The same guarantee with a REAL widget in the well. The bare-rect
    /// test above passed while actual knobs sat left of centre: `knob`
    /// wrapped its stack in `vertical()`, which claims the full width and
    /// then shrinks, pinning the stack left. Widgets, not rects, are what
    /// ship — so one test drives one.
    #[test]
    fn a_real_knob_lands_centered_in_its_well() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let param = crate::ui::device::Param::percent("mix");
        let mut norm = 0.5f32;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };

        let mut pairs: Vec<(egui::Rect, egui::Rect)> = Vec::new();
        for _ in 0..4 {
            let mut out = ctx.run_ui(input.take(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    sections(ui, &theme, 1, 1, |ui, _| {
                        let well = ui.max_rect();
                        let before = ui.min_rect().bottom();
                        crate::ui::device::knob::knob(ui, &theme, &param, &mut norm);
                        let r = ui.min_rect();
                        let content = egui::Rect::from_min_max(
                            egui::pos2(r.left(), before),
                            egui::pos2(r.right(), r.bottom()),
                        );
                        pairs.push((content, well));
                    });
                });
            });
            out.textures_delta.clear();
        }

        let (content, well) = pairs[3];
        assert!(
            (content.center().x - well.center().x).abs() < 2.0,
            "knob not horizontally centered: content {content:?} well {well:?}"
        );
        assert!(
            (content.center().y - well.center().y).abs() < 3.0,
            "knob not vertically centered: content {content:?} well {well:?}"
        );
    }
}
