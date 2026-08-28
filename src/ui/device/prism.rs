//! PRISM's card — three bands as three columns of light.
//!
//! # Why a spectrum and not three copies of a compressor
//!
//! A multiband is usually drawn as N compressor strips side by side, and
//! the picture then says nothing the numbers did not: it is a table with
//! knobs in it. The one thing a multiband has that a compressor does not
//! is a FREQUENCY AXIS — where the seams are, how wide each band is, and
//! which of the three is doing the work right now. So that is what this
//! draws, and everything else is a cell underneath it.
//!
//! ```text
//!  ┌──────────────────────────────────────────────────┐
//!  │ ████████████ │ ███████████████ │ ███████████████ │  the prism strip:
//!  ├──────────────┼─────────────────┼─────────────────┤  which band is working
//!  │              │        ▁▁▁      │                 │
//!  │  ══════════  │  ═════════════  │  ═════════════  │  thresholds, draggable
//!  │      ▓▓      │       ▓▓▓▓      │                 │  the beam: gain moving
//!  │              ╎                 ╎                 │  seams, draggable
//!  └──── 100 ─────┴───── 1k ────────┴──── 10k ────────┘
//! ```
//!
//! # Colour indexes DIRECTION, not band
//!
//! The three bands are told apart by POSITION and by their labels, the
//! way the house style asks. What changes colour is what the band is
//! DOING: holding down is the mod role, lifting up is the level role,
//! and a band at rest is neither. That is the device's one signed knob,
//! drawn — and the beam also goes down or up from the threshold rule, so
//! the direction survives being read in grey.
//!
//! # The gestures
//!
//! Five draggable targets, each its own egui interaction, per
//! `notes/20260826-device-ui-contract.md`: three threshold rules that
//! slide vertically, and two crossover seams that sweep sideways. The
//! seams are allocated LAST so they win the few pixels where a rule
//! crosses one — a seam is four points wide and a rule is a third of the
//! card, and losing the narrow one to the wide one would make the seam
//! unpressable wherever a threshold happened to sit.

use super::{
    Footprint, Mapping, Param, ParamEdit, Unit, Well, Wells, card, metrics, poly_widgets, scope,
};
use crate::params::{self, prism as pp};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// The plot's vertical range, in dBFS. Wider than the threshold's own
/// range at both ends: a rule pinned against the top or bottom edge of
/// its own travel cannot be told from one that has run out of picture.
const LEVEL_FLOOR_DB: f32 = -66.0;
const LEVEL_CEIL_DB: f32 = 6.0;

/// The plot's horizontal range, in Hz. The whole audible band, so the
/// seams sit where a person expects them rather than where the two
/// crossover knobs happen to end.
const HZ_FLOOR: f32 = 20.0;
const HZ_CEIL: f32 = 20_000.0;

/// How far from a target a press still counts, in points. The house
/// minimum for a dense screen is twenty points of hit area; these are
/// half-extents, so a rule is twenty points tall to the pointer while
/// being one point tall to the eye.
const GRAB_PT: f32 = 10.0;

/// The prism strip along the top of the plot: how tall, in points.
const STRIP_PT: f32 = 7.0;

/// The frequency ruler along the bottom.
const RULER_PT: f32 = 11.0;

const BAND_NAMES: &[&str] = &["low", "mid", "high"];

/// The card's knob positions, normalized `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrismUi {
    /// Which band the cell row edits.
    ///
    /// UI-only state with no engine counterpart, so it rides
    /// `DeviceInstance::page` in and out — a card is rebuilt from engine
    /// units every frame and would forget it otherwise. Rule 3 of the
    /// device UI contract, and `eq.rs` takes the same road.
    pub selected: usize,
    pub low_x: f32,
    pub high_x: f32,
    pub grip: f32,
    pub mix: f32,
    pub output: f32,
    /// `[band][which]`, indexed by `pp::band::*`.
    pub bands: [[f32; pp::band::COUNT as usize]; pp::BANDS],
}

impl Default for PrismUi {
    fn default() -> Self {
        Self::from_engine(0, |id| params::def(pp::TABLE, id).default)
    }
}

impl PrismUi {
    pub fn from_engine(selected: usize, get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| prism_norm(id, get(id));
        let band = |b: usize| {
            let mut row = [0.0f32; pp::band::COUNT as usize];
            for (which, slot) in row.iter_mut().enumerate() {
                *slot = at(pp::param(b, which as u32));
            }
            row
        };
        Self {
            selected: selected.min(pp::BANDS - 1),
            low_x: at(pp::LOW_X),
            high_x: at(pp::HIGH_X),
            grip: at(pp::GRIP),
            mix: at(pp::MIX),
            output: at(pp::OUTPUT),
            bands: [band(0), band(1), band(2)],
        }
    }

    pub fn slot(&mut self, param: u32) -> Option<&mut f32> {
        if let Some((band, which)) = pp::split(param) {
            return self
                .bands
                .get_mut(band)
                .and_then(|row| row.get_mut(which as usize));
        }
        Some(match param {
            pp::LOW_X => &mut self.low_x,
            pp::HIGH_X => &mut self.high_x,
            pp::GRIP => &mut self.grip,
            pp::MIX => &mut self.mix,
            pp::OUTPUT => &mut self.output,
            _ => return None,
        })
    }

    pub fn get(&self, param: u32) -> f32 {
        if let Some((band, which)) = pp::split(param) {
            return self
                .bands
                .get(band)
                .and_then(|row| row.get(which as usize))
                .copied()
                .unwrap_or(0.0);
        }
        match param {
            pp::LOW_X => self.low_x,
            pp::HIGH_X => self.high_x,
            pp::GRIP => self.grip,
            pp::MIX => self.mix,
            pp::OUTPUT => self.output,
            _ => 0.0,
        }
    }

    fn current(&self) -> usize {
        self.selected.min(pp::BANDS - 1)
    }

    /// The two corners in Hz, as the card holds them.
    fn corners(&self) -> (f32, f32) {
        (
            prism_value(pp::LOW_X, self.low_x),
            prism_value(pp::HIGH_X, self.high_x),
        )
    }
}

/// One control by wire id, in natural units.
///
/// The two corners and the times are LOG: the difference between 40 and
/// 80 Hz is an octave and the difference between 11 and 12 kHz is
/// nothing.
fn param_of(param: u32) -> Param {
    let def = params::def(pp::TABLE, param);
    let scaled = |name: &'static str| {
        let mut p = Param::new(
            name,
            Mapping::Linear {
                min: def.min * 100.0,
                max: def.max * 100.0,
            },
            Unit::Percent,
        )
        .with_default(def.default * 100.0);
        if def.min < 0.0 {
            p = p.bipolar();
        }
        p
    };
    if let Some((_, which)) = pp::split(param) {
        return match which {
            pp::band::THRESHOLD => Param::db("thresh", def.min, def.max).with_default(def.default),
            pp::band::AMOUNT => scaled("amount"),
            pp::band::HEAT => scaled("heat"),
            _ => Param::db("trim", def.min, def.max)
                .with_default(def.default)
                .bipolar(),
        };
    }
    match param {
        pp::LOW_X => Param::hz("low x", def.min, def.max).with_default(def.default),
        pp::HIGH_X => Param::hz("high x", def.min, def.max).with_default(def.default),
        pp::GRIP => scaled("grip"),
        pp::MIX => scaled("mix"),
        _ => Param::db("out", def.min, def.max)
            .with_default(def.default)
            .bipolar(),
    }
}

/// Which rows are stored as a fraction and shown as a percentage.
fn is_percent(param: u32) -> bool {
    matches!(
        pp::split(param),
        Some((_, pp::band::AMOUNT | pp::band::HEAT))
    ) || matches!(param, pp::GRIP | pp::MIX)
}

fn shown(param: u32, value: f32) -> f32 {
    if is_percent(param) {
        value * 100.0
    } else {
        value
    }
}

fn natural(param: u32, value: f32) -> f32 {
    if is_percent(param) {
        value / 100.0
    } else {
        value
    }
}

pub fn prism_value(param: u32, norm: f32) -> f32 {
    let def = params::def(pp::TABLE, param);
    natural(param, param_of(param).value(norm)).clamp(def.min, def.max)
}

pub fn prism_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

pub fn prism_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// The band selector, as a `Param` so it draws and steps like every
/// other cell. Not an engine parameter — see [`PrismUi::selected`].
fn band_picker() -> Param {
    Param::choice("band", BAND_NAMES)
}

/// Row one is the SELECTED BAND, row two is the whole device. The band
/// picker leads row one because it says what the four cells after it
/// belong to, and a row whose subject is written at its end is a row you
/// have to read twice.
const BAND_CELLS: [u32; pp::band::COUNT as usize] = [
    pp::band::THRESHOLD,
    pp::band::AMOUNT,
    pp::band::HEAT,
    pp::band::TRIM,
];
const GLOBAL_CELLS: [u32; 5] = [pp::LOW_X, pp::HIGH_X, pp::GRIP, pp::MIX, pp::OUTPUT];

/// A labelled cell is TWO lines: its value, then its name.
const CELL_UNITS: usize = 2;
const ROWS: usize = 2;
const FOOTER_ROWS: usize = ROWS * CELL_UNITS;

/// Every wire id the face shows, for the coverage test.
pub fn face_ids() -> Vec<u32> {
    let mut ids: Vec<u32> = GLOBAL_CELLS.to_vec();
    for band in 0..pp::BANDS {
        for which in BAND_CELLS {
            ids.push(pp::param(band, which));
        }
    }
    ids
}

pub fn prism_edits(state: &PrismUi) -> Vec<ParamEdit> {
    face_ids()
        .into_iter()
        .map(|param| ParamEdit {
            param,
            value: prism_value(param, state.get(param)),
        })
        .collect()
}

fn cell_min_width(ui: &egui::Ui, theme: &Theme, param: &Param) -> f32 {
    let value = metrics::mono_w(ui, &param.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &param.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

/// The width the face needs: its widest row at its narrowest.
///
/// The band row is measured across ALL THREE bands, not just the one
/// showing — the cells are the same width in every band, and reserving
/// for whichever band happens to be selected would make the card resize
/// itself when the picker steps.
fn face_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    let gap = ui.spacing().item_spacing.x;
    let band_row = {
        let picker = cell_min_width(ui, theme, &band_picker());
        let cells: f32 = BAND_CELLS
            .iter()
            .map(|which| {
                (0..pp::BANDS)
                    .map(|b| cell_min_width(ui, theme, &param_of(pp::param(b, *which))))
                    .fold(0.0f32, f32::max)
            })
            .sum();
        picker + cells + gap * BAND_CELLS.len() as f32
    };
    let global_row = {
        let cells: f32 = GLOBAL_CELLS
            .iter()
            .map(|id| cell_min_width(ui, theme, &param_of(*id)))
            .sum();
        cells + gap * (GLOBAL_CELLS.len() - 1) as f32
    };
    band_row.max(global_row)
}

// ------------------------------------------------------------- geometry ---

/// Where `hz` sits across the plot, `0..=1`, on a log axis.
fn axis_t(hz: f32) -> f32 {
    let hz = if hz.is_finite() { hz } else { HZ_FLOOR };
    let hz = hz.clamp(HZ_FLOOR, HZ_CEIL);
    (hz / HZ_FLOOR).log10() / (HZ_CEIL / HZ_FLOOR).log10()
}

fn axis_x(rect: egui::Rect, hz: f32) -> f32 {
    rect.left() + rect.width() * axis_t(hz)
}

/// The inverse: what frequency a press at `x` means.
fn axis_hz(rect: egui::Rect, x: f32) -> f32 {
    let t = if rect.width() > 0.0 {
        ((x - rect.left()) / rect.width()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    HZ_FLOOR * (HZ_CEIL / HZ_FLOOR).powf(t)
}

/// Where `db` sits down the plot.
fn level_y(rect: egui::Rect, db: f32) -> f32 {
    let db = if db.is_finite() { db } else { LEVEL_FLOOR_DB };
    let t = ((db - LEVEL_FLOOR_DB) / (LEVEL_CEIL_DB - LEVEL_FLOOR_DB)).clamp(0.0, 1.0);
    rect.bottom() - rect.height() * t
}

/// How many points one dB is worth, for drawing a beam.
fn points_per_db(rect: egui::Rect) -> f32 {
    rect.height() / (LEVEL_CEIL_DB - LEVEL_FLOOR_DB)
}

/// The part of the plot the bands are drawn in — the strip and the ruler
/// take their own slices off the top and bottom first.
fn field_of(ui: &egui::Ui, theme: &Theme, rect: egui::Rect) -> egui::Rect {
    let _ = ui;
    let strip = theme.sp(STRIP_PT);
    let ruler = theme.sp(RULER_PT);
    egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.top() + strip),
        egui::pos2(
            rect.right(),
            (rect.bottom() - ruler).max(rect.top() + strip),
        ),
    )
}

/// Each band's horizontal span inside `field`.
fn spans(field: egui::Rect, state: &PrismUi) -> [(f32, f32); pp::BANDS] {
    let (low, high) = state.corners();
    let a = axis_x(field, low);
    let b = axis_x(field, high).max(a);
    [(field.left(), a), (a, b), (b, field.right().max(b))]
}

/// A band's threshold rule, as a pointer target.
fn threshold_hit(field: egui::Rect, state: &PrismUi, theme: &Theme, band: usize) -> egui::Rect {
    let span = spans(field, state)
        .get(band)
        .copied()
        .unwrap_or((field.left(), field.right()));
    let db = prism_value(pp::param(band, pp::band::THRESHOLD), {
        state.get(pp::param(band, pp::band::THRESHOLD))
    });
    let y = level_y(field, db);
    let reach = theme.sp(GRAB_PT);
    egui::Rect::from_min_max(
        egui::pos2(span.0, y - reach),
        egui::pos2(span.1.max(span.0 + 1.0), y + reach),
    )
}

/// A crossover seam, as a pointer target.
fn seam_hit(field: egui::Rect, state: &PrismUi, theme: &Theme, seam: usize) -> egui::Rect {
    let (low, high) = state.corners();
    let hz = if seam == 0 { low } else { high };
    let x = axis_x(field, hz);
    let reach = theme.sp(GRAB_PT) * 0.6;
    egui::Rect::from_min_max(
        egui::pos2(x - reach, field.top()),
        egui::pos2(x + reach, field.bottom()),
    )
}

/// Which band a press at `x` lands in.
fn band_at(field: egui::Rect, state: &PrismUi, x: f32) -> usize {
    let (low, high) = state.corners();
    let hz = axis_hz(field, x);
    if hz < low {
        0
    } else if hz < high {
        1
    } else {
        2
    }
}

/// A sideways drag on a seam, in RATIOS — a corner is a frequency and a
/// frequency moves geometrically or it crawls at the bottom and bolts at
/// the top.
fn drag_hz(hz: f32, dx: f32, width: f32) -> f32 {
    if width <= 0.0 || !dx.is_finite() {
        return hz;
    }
    let decades = (HZ_CEIL / HZ_FLOOR).log10();
    hz * 10f32.powf(decades * dx / width)
}

// ------------------------------------------------------------------ card ---

/// Draw the prism card. `said` is what the engine is doing right now —
/// its per-band figures are what the beams are.
pub fn prism_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut PrismUi,
    said: scope::Reading,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);

    card::card_sized(ui, theme, "prism", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => {
                        for param in hero(ui, theme, state, said) {
                            edits.push(ParamEdit {
                                param,
                                value: prism_value(param, state.get(param)),
                            });
                        }
                    }
                    poly_widgets::CurveRegion::Footer => footer(ui, theme, state, &mut edits),
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The picture, and the five gestures on it. Returns the wire ids the
/// user just moved.
fn hero(ui: &mut egui::Ui, theme: &Theme, state: &mut PrismUi, said: scope::Reading) -> Vec<u32> {
    let size = egui::vec2(ui.available_width(), ui.available_height());
    let (rect, background) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let field = field_of(ui, theme, rect);
    let mut moved = Vec::new();

    // ONE EGUI INTERACTION PER TARGET, laid over the display. egui tracks
    // an interaction by widget id, so a drag that began on the mid
    // band's threshold STAYS the mid band's until the button comes up —
    // past its neighbour, past the end of its range, off the card. See
    // `eq.rs`, which learned this the expensive way.
    for band in 0..pp::BANDS {
        let id = pp::param(band, pp::band::THRESHOLD);
        let handle = ui
            .interact(
                threshold_hit(field, state, theme, band),
                background.id.with(("threshold", band)),
                egui::Sense::click_and_drag(),
            )
            .affords(Affords::Slide);
        if handle.drag_started() || handle.clicked() {
            state.selected = band;
        }
        if handle.dragged() {
            let dy = handle.drag_delta().y;
            if dy != 0.0 && field.height() > 0.0 {
                let span = LEVEL_CEIL_DB - LEVEL_FLOOR_DB;
                let db = prism_value(id, state.get(id)) - dy * span / field.height();
                if let Some(slot) = state.slot(id) {
                    *slot = prism_norm(id, db).clamp(0.0, 1.0);
                    moved.push(id);
                }
            }
        }
    }

    // The seams go on LAST so they win the pixels where a threshold rule
    // crosses one: a seam is a few points wide and a rule is a third of
    // the card, and the wide one swallowing the narrow one would make a
    // seam unpressable wherever a threshold happened to sit.
    for seam in 0..2 {
        let id = if seam == 0 { pp::LOW_X } else { pp::HIGH_X };
        let handle = ui
            .interact(
                seam_hit(field, state, theme, seam),
                background.id.with(("seam", seam)),
                egui::Sense::click_and_drag(),
            )
            .affords(Affords::SeamX);
        if handle.dragged() {
            let dx = handle.drag_delta().x;
            if dx != 0.0 {
                let hz = drag_hz(prism_value(id, state.get(id)), dx, field.width());
                if let Some(slot) = state.slot(id) {
                    *slot = prism_norm(id, hz).clamp(0.0, 1.0);
                    moved.push(id);
                }
            }
        }
    }

    // A press on open ground picks the band it landed in and moves
    // nothing. Both halves of the press, not just `clicked()`: under
    // `click_and_drag` a press that wanders three pixels is a DRAG and
    // `clicked()` never fires for it.
    if (background.drag_started() || background.clicked())
        && let Some(at) = background.interact_pointer_pos()
    {
        state.selected = band_at(field, state, at.x);
    }

    paint(ui, theme, rect, field, state, said);
    moved
}

/// What a band's gain movement colours it. Direction, not identity — see
/// the module header.
fn direction_color(theme: &Theme, moved_db: f32) -> egui::Color32 {
    if moved_db < 0.0 {
        theme.role_mod
    } else {
        theme.role_level
    }
}

fn paint(
    ui: &egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    field: egui::Rect,
    state: &PrismUi,
    said: scope::Reading,
) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let paint = ui.painter();
    let spans = spans(field, state);
    let hair = stroke::HAIR;
    let selected = state.current();

    // --- the frequency ruler, first, so everything sits on top of it ---
    for hz in [100.0f32, 1_000.0, 10_000.0] {
        let x = axis_x(field, hz);
        paint.line_segment(
            [egui::pos2(x, field.top()), egui::pos2(x, field.bottom())],
            egui::Stroke::new(hair, theme.divider),
        );
        paint.text(
            egui::pos2(x, rect.bottom() - theme.sp(RULER_PT) * 0.5),
            egui::Align2::CENTER_CENTER,
            if hz >= 1_000.0 {
                format!("{:.0}k", hz / 1_000.0)
            } else {
                format!("{hz:.0}")
            },
            egui::FontId::monospace(font::MINI_LABEL),
            theme.text_muted,
        );
    }
    // The unity rule: 0 dBFS, so a threshold's height means something
    // absolute rather than "somewhere up the box".
    let unity = level_y(field, 0.0);
    paint.line_segment(
        [
            egui::pos2(field.left(), unity),
            egui::pos2(field.right(), unity),
        ],
        egui::Stroke::new(hair, theme.divider),
    );

    let per_db = points_per_db(field);
    for (band, (from, to)) in spans.into_iter().enumerate() {
        if to <= from {
            continue;
        }
        let column = egui::Rect::from_min_max(
            egui::pos2(from, field.top()),
            egui::pos2(to, field.bottom()),
        );
        let moved_db = said.bands.get(band).copied().unwrap_or(0.0);
        let working = (moved_db.abs() / pp::HEAT_FULL_DB).clamp(0.0, 1.0);
        let colour = direction_color(theme, moved_db);

        // The selected band's ground is a shade up from its neighbours',
        // which is the whole of the selection mark: no border around a
        // border, per the house style.
        if band == selected {
            paint.rect_filled(column, 0.0, theme.surface);
        }

        // --- the prism strip: which band is working, and which way ----
        let strip = egui::Rect::from_min_max(
            egui::pos2(from, rect.top()),
            egui::pos2(to, rect.top() + theme.sp(STRIP_PT)),
        );
        paint.rect_filled(strip, 0.0, theme.surface_raised);
        if working > 0.0 {
            let lit = egui::Rect::from_min_max(
                egui::pos2(strip.left(), strip.top()),
                egui::pos2(strip.left() + strip.width() * working, strip.bottom()),
            );
            paint.rect_filled(lit, 0.0, colour);
        }

        // --- the threshold rule --------------------------------------
        let id = pp::param(band, pp::band::THRESHOLD);
        let y = level_y(field, prism_value(id, state.get(id)));
        let rule = if band == selected {
            egui::Stroke::new(hair * 2.0, theme.text)
        } else {
            egui::Stroke::new(hair, theme.text_muted)
        };
        paint.line_segment([egui::pos2(from, y), egui::pos2(to, y)], rule);

        // --- the beam: gain moving, from the rule, in real decibels ---
        //
        // Down for reduction and up for lift, so the direction reads in
        // grey. The beam is inset from the seams: a column of colour
        // running edge to edge would touch its neighbour's and the two
        // would look like one band.
        if moved_db != 0.0 {
            let inset = (column.width() * 0.18).min(theme.sp(space::SM));
            let depth = moved_db.abs() * per_db;
            let beam = if moved_db < 0.0 {
                egui::Rect::from_min_max(
                    egui::pos2(from + inset, y),
                    egui::pos2(to - inset, (y + depth).min(field.bottom())),
                )
            } else {
                egui::Rect::from_min_max(
                    egui::pos2(from + inset, (y - depth).max(field.top())),
                    egui::pos2(to - inset, y),
                )
            };
            if beam.width() > 0.0 && beam.height() > 0.0 {
                paint.rect_filled(beam, 0.0, colour);
            }
        }

        // --- the band's name, and what it is doing -------------------
        let label = BAND_NAMES.get(band).copied().unwrap_or("");
        paint.text(
            egui::pos2(
                from + theme.sp(space::XS),
                field.top() + theme.sp(space::XS),
            ),
            egui::Align2::LEFT_TOP,
            label.to_uppercase(),
            egui::FontId::proportional(font::MINI_LABEL),
            if band == selected {
                theme.text
            } else {
                theme.text_muted
            },
        );
        if moved_db.abs() >= 0.1 {
            paint.text(
                egui::pos2(to - theme.sp(space::XS), field.top() + theme.sp(space::XS)),
                egui::Align2::RIGHT_TOP,
                format!("{moved_db:+.1}"),
                egui::FontId::monospace(font::MINI_LABEL),
                colour,
            );
        }
    }

    // --- the seams, over everything -------------------------------
    for (from, _) in spans.into_iter().skip(1) {
        paint.line_segment(
            [
                egui::pos2(from, rect.top()),
                egui::pos2(from, field.bottom()),
            ],
            egui::Stroke::new(hair * 2.0, theme.accent),
        );
    }
}

/// Two rows: the selected band, then the whole device.
fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut PrismUi, edits: &mut Vec<ParamEdit>) {
    // The row height the PANEL reserved, restated rather than divided
    // out of what is available — dividing hands each row a share of the
    // gaps as well and walks the rows down the card until they overlap.
    let gap = theme.sp(space::XXS);
    let height = ((ui.available_height() - gap * (ROWS as f32 - 1.0)) / ROWS as f32).max(1.0);
    ui.spacing_mut().item_spacing.y = gap;

    let band = state.current();
    let mut row_ids: Vec<Option<u32>> = vec![None];
    row_ids.extend(BAND_CELLS.iter().map(|w| Some(pp::param(band, *w))));
    let rows: [Vec<Option<u32>>; ROWS] =
        [row_ids, GLOBAL_CELLS.iter().map(|id| Some(*id)).collect()];

    for row in rows {
        ui.horizontal(|ui| {
            let gap_x = ui.spacing().item_spacing.x;
            for (drawn, cell) in row.iter().enumerate() {
                // The share is recomputed from what is ACTUALLY left,
                // cell by cell: worked out in advance, any cell needing
                // more than its share spends the row's remainder and the
                // last one is pushed off the card's right edge.
                let left = (row.len() - drawn) as f32;
                let room = ui.available_width() - gap_x * (left - 1.0).max(0.0);
                let width = (room / left).floor().max(1.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(width);
                        ui.set_height(height);
                        match cell {
                            // The band picker: not an engine parameter,
                            // so it writes the card's own state and
                            // emits nothing.
                            None => {
                                let spec = band_picker();
                                let mut norm = spec.at_index(state.current());
                                if poly_widgets::labeled_cell_steps(
                                    ui, theme, &spec, &mut norm, None,
                                ) {
                                    state.selected = spec.index(norm).min(pp::BANDS - 1);
                                }
                            }
                            Some(param) => {
                                let spec = param_of(*param);
                                let Some(norm) = state.slot(*param) else {
                                    return;
                                };
                                if poly_widgets::labeled_cell_bar(ui, theme, &spec, norm, None) {
                                    let value = prism_value(*param, *norm);
                                    edits.push(ParamEdit {
                                        param: *param,
                                        value,
                                    });
                                }
                            }
                        }
                    },
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::device::probe;

    /// Ten cells across two rows and a picture over them.
    const MAX_W: f32 = 760.0;
    const MIN_W: f32 = 320.0;

    fn frame<R>(ctx: &egui::Context, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let mut f = Some(f);
        let mut out = None;
        let mut run = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 400.0),
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
        out.expect("the frame ran")
    }

    /// The rectangle every pointer test drives, and the field inside it.
    const PLOT: egui::Rect = egui::Rect {
        min: egui::pos2(20.0, 20.0),
        max: egui::pos2(300.0, 190.0),
    };

    fn field(theme: &Theme) -> egui::Rect {
        let strip = theme.sp(STRIP_PT);
        let ruler = theme.sp(RULER_PT);
        egui::Rect::from_min_max(
            egui::pos2(PLOT.left(), PLOT.top() + strip),
            egui::pos2(PLOT.right(), PLOT.bottom() - ruler),
        )
    }

    /// Where a band's threshold rule is drawn, for a press to land on.
    fn rule_at(theme: &Theme, state: &PrismUi, band: usize) -> egui::Pos2 {
        let field = field(theme);
        let (from, to) = spans(field, state)
            .get(band)
            .copied()
            .unwrap_or((field.left(), field.right()));
        let id = pp::param(band, pp::band::THRESHOLD);
        egui::pos2(
            (from + to) * 0.5,
            level_y(field, prism_value(id, state.get(id))),
        )
    }

    fn gesture(state: &mut PrismUi, path: &[probe::Step]) -> Vec<u32> {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        probe::run(&ctx, PLOT, path, |ui| {
            hero(ui, &theme, state, scope::Reading::default())
        })
        .concat()
    }

    // ---------------------------------------------- the standing tests ---

    #[test]
    fn every_table_row_leaves_as_an_edit() {
        let edits = prism_edits(&PrismUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = pp::TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect, "the card does not cover the table");

        // And the face reaches every one of them — the band rows behind
        // the picker included, which is the half a single-page card
        // cannot get wrong and a paged one can.
        let mut face = face_ids();
        face.sort_unstable();
        assert_eq!(face, expect, "the face does not cover the table");
    }

    #[test]
    fn every_position_is_a_legal_engine_value() {
        for def in pp::TABLE {
            for i in 0..=40 {
                let value = prism_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
            }
            let span = def.max - def.min;
            assert!((prism_value(def.id, 0.0) - def.min).abs() < span * 1e-3);
            assert!((prism_value(def.id, 1.0) - def.max).abs() < span * 1e-3);
        }
    }

    #[test]
    fn value_and_norm_round_trip() {
        for def in pp::TABLE {
            for i in 0..=40 {
                let value = prism_value(def.id, i as f32 / 40.0);
                let again = prism_value(def.id, prism_norm(def.id, value));
                assert!(
                    (again - value).abs() <= value.abs() * 1e-3 + 1e-4,
                    "{}: {value} came back as {again}",
                    def.name
                );
            }
        }
    }

    #[test]
    fn defaults_come_from_the_table() {
        for def in pp::TABLE {
            let mut state = PrismUi::default();
            let norm = *state.slot(def.id).expect("every row has a slot");
            let value = prism_value(def.id, norm);
            assert!(
                (value - def.default).abs() < (def.max - def.min) * 1e-3,
                "{} opened at {value}, table says {}",
                def.name,
                def.default
            );
        }
    }

    #[test]
    fn the_card_stays_inside_its_budget() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = PrismUi::default();
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        prism_card(ui, &theme, &mut state, scope::Reading::default())
                    })
                    .response
                    .rect
                })
                .inner
        });
        let (w, h) = (rect.width(), rect.height());
        assert!(w <= MAX_W, "the card is {w:.0} pt wide, over {MAX_W:.0}");
        assert!(w >= MIN_W, "the card is only {w:.0} pt wide — did it draw?");
        let budget = theme.sp(control::DEVICE_TALL_H) + stroke::HAIR * 2.0;
        assert!(
            h <= budget,
            "the card is {h:.0} pt tall, over its {budget:.0} pt budget"
        );
    }

    #[test]
    fn the_card_fits_the_panel_it_is_actually_given() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = PrismUi::default();
        for panel_w in [620.0f32, 720.0, 900.0] {
            let host = egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(panel_w, theme.sp(control::DEVICE_TALL_H) + 8.0),
            );
            let used = frame(&ctx, |ui| {
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(host)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                child.set_width(host.width());
                prism_card(&mut child, &theme, &mut state, scope::Reading::default());
                child.min_rect()
            });
            assert!(
                used.width() <= panel_w + 1.0,
                "at a {panel_w:.0} pt panel the card drew {:.0} pt wide",
                used.width()
            );
            assert!(
                used.width() > panel_w * 0.7,
                "at a {panel_w:.0} pt panel the card drew only {:.0} pt",
                used.width()
            );
        }
    }

    /// Drawing at rest must not move a control — and a card whose
    /// picture is fed live telemetry is exactly the shape that gets this
    /// wrong, so it is drawn with the engine working as well as idle.
    #[test]
    fn drawing_at_rest_emits_nothing() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = PrismUi::default();
        let before = state;
        for said in [
            scope::Reading::default(),
            scope::Reading {
                level_db: -6.0,
                reduction_db: -9.0,
                bands: [-9.0, 2.0, 0.0],
            },
        ] {
            let edits = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| prism_card(ui, &theme, &mut state, said))
                    .inner
            });
            assert!(edits.is_empty(), "the card emitted {edits:?} at rest");
        }
        assert_eq!(state, before, "drawing the card moved a control");
    }

    /// The footer reserves TWO `POLY_CELL_H` units for every row of
    /// cells, and the plot still gets a picture rather than a line of
    /// pixels. The layout note's own arithmetic, restated.
    #[test]
    fn the_footer_reserves_two_lines_for_every_row_of_cells() {
        let theme = Theme::dark();
        let gap = theme.sp(space::XXS);
        let unit = theme.sp(control::POLY_CELL_H);
        let footer = unit * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS - 1) as f32;
        let rows_need = ROWS as f32 * (unit * CELL_UNITS as f32) + gap * (ROWS as f32 - 1.0);
        assert!(
            footer + 0.5 >= rows_need,
            "the footer reserves {footer:.0} pt for rows needing {rows_need:.0}"
        );
        let plot = theme.sp(control::DEVICE_TALL_H) - footer;
        assert!(
            plot > unit * 3.0,
            "the plot is left {plot:.0} pt, which is not a picture"
        );
    }

    // ------------------------------------------------ the pointer tests ---

    /// Pressing a band's threshold rule selects THAT band, and dragging
    /// it moves THAT band's threshold.
    #[test]
    fn dragging_a_threshold_moves_its_own_band() {
        let theme = Theme::dark();
        for band in 0..pp::BANDS {
            let mut state = PrismUi::default();
            let at = rule_at(&theme, &state, band);
            let moved = gesture(
                &mut state,
                &probe::drag_path(at, at + egui::vec2(0.0, 20.0), 4),
            );

            assert_eq!(state.selected, band, "the press did not select band {band}");
            let want = pp::param(band, pp::band::THRESHOLD);
            assert!(moved.contains(&want), "band {band} emitted {moved:?}");
            let now = prism_value(want, state.get(want));
            assert!(now < -18.5, "band {band}'s threshold only reached {now}");

            // ...and nobody else moved.
            for other in 0..pp::BANDS {
                if other == band {
                    continue;
                }
                let id = pp::param(other, pp::band::THRESHOLD);
                assert!(
                    (prism_value(id, state.get(id)) + 18.0).abs() < 0.01,
                    "dragging band {band} moved band {other}"
                );
            }
        }
    }

    /// A drag that wanders into a neighbour's column keeps its own band
    /// — the property one interaction per target buys, and the one a
    /// nearest-handle search cannot have.
    #[test]
    fn a_drag_keeps_its_band_even_when_it_crosses_another() {
        let theme = Theme::dark();
        let mut state = PrismUi::default();
        let from = rule_at(&theme, &state, 1);
        // Left, well past the low seam, and down.
        let to = egui::pos2(field(&theme).left() + 4.0, from.y + 24.0);
        let moved = gesture(&mut state, &probe::drag_path(from, to, 6));

        let mid = pp::param(1, pp::band::THRESHOLD);
        let low = pp::param(0, pp::band::THRESHOLD);
        assert!(
            moved.contains(&mid),
            "the mid band lost its drag: {moved:?}"
        );
        assert!(
            !moved.contains(&low),
            "the drag leaked onto the low band: {moved:?}"
        );
        assert_eq!(
            state.selected, 1,
            "the drag changed which band was selected"
        );
    }

    /// A threshold pinned at the bottom of its range keeps the drag —
    /// the pointer runs away from the rule and the gesture must not die
    /// with it.
    #[test]
    fn a_threshold_pinned_at_its_floor_keeps_the_drag() {
        let theme = Theme::dark();
        let mut state = PrismUi::default();
        let from = rule_at(&theme, &state, 2);
        let far = egui::pos2(from.x, from.y + 400.0);
        gesture(&mut state, &probe::drag_path(from, far, 8));
        let id = pp::param(2, pp::band::THRESHOLD);
        let now = prism_value(id, state.get(id));
        assert!(
            (now - pp::THRESHOLD_MIN_DB).abs() < 0.5,
            "the threshold stopped at {now} instead of its floor"
        );
        assert_eq!(state.selected, 2);
    }

    /// A seam sweeps sideways, and moves the corner in RATIOS.
    #[test]
    fn dragging_a_seam_moves_its_crossover() {
        let theme = Theme::dark();
        for (seam, id) in [(0usize, pp::LOW_X), (1, pp::HIGH_X)] {
            let mut state = PrismUi::default();
            let field = field(&theme);
            let hz = prism_value(id, state.get(id));
            // Below the threshold rules, so only the seam is there.
            let at = egui::pos2(axis_x(field, hz), field.bottom() - 6.0);
            let moved = gesture(
                &mut state,
                &probe::drag_path(at, at + egui::vec2(24.0, 0.0), 4),
            );

            assert!(moved.contains(&id), "seam {seam} emitted {moved:?}");
            let now = prism_value(id, state.get(id));
            assert!(
                now > hz * 1.05,
                "seam {seam} went from {hz} to {now} — rightward should raise it"
            );
        }
    }

    /// THE SEAM WINS where a threshold rule crosses it. A seam is a few
    /// points wide and a rule is a third of the card; if the wide target
    /// took the press, a seam would be unusable at whatever height its
    /// neighbours' thresholds happened to sit.
    #[test]
    fn a_seam_wins_the_pixels_a_threshold_rule_crosses() {
        let theme = Theme::dark();
        let mut state = PrismUi::default();
        let field = field(&theme);
        let hz = prism_value(pp::LOW_X, state.get(pp::LOW_X));
        // Exactly on the seam, at exactly the height of the rules.
        let at = egui::pos2(axis_x(field, hz), rule_at(&theme, &state, 1).y);
        let moved = gesture(
            &mut state,
            &probe::drag_path(at, at + egui::vec2(20.0, 0.0), 4),
        );

        assert!(
            moved.contains(&pp::LOW_X),
            "the seam did not get the press: {moved:?}"
        );
        for band in 0..pp::BANDS {
            let id = pp::param(band, pp::band::THRESHOLD);
            assert!(
                (prism_value(id, state.get(id)) + 18.0).abs() < 0.01,
                "band {band}'s threshold moved on a seam drag"
            );
        }
    }

    /// A press on open ground picks the band it landed in and moves
    /// nothing at all.
    #[test]
    fn pressing_open_ground_selects_without_moving_anything() {
        let theme = Theme::dark();
        let field = field(&theme);
        let before = PrismUi::default();
        for (band, x) in [
            (0usize, field.left() + 6.0),
            (1, field.center().x),
            (2, field.right() - 6.0),
        ] {
            let mut state = PrismUi::default();
            // Well below every rule, and away from both seams.
            let at = egui::pos2(x, field.bottom() - 3.0);
            let moved = gesture(&mut state, &probe::click_path(at));
            assert!(moved.is_empty(), "a click on ground emitted {moved:?}");
            assert_eq!(state.selected, band, "a click at {x} chose the wrong band");
            let mut same = state;
            same.selected = before.selected;
            assert_eq!(same, before, "a click on ground moved a control");
        }
    }

    /// The band picker is card state with no engine counterpart, so it
    /// has to survive the trip out to the instance and back — rule 3 of
    /// the device UI contract.
    #[test]
    fn the_selected_band_survives_a_rebuild() {
        let engine = PrismUi::default();
        for band in 0..pp::BANDS {
            let rebuilt = PrismUi::from_engine(band, |id| prism_value(id, engine.get(id)));
            assert_eq!(rebuilt.selected, band);
        }
        // And an impossible page folds into a real band rather than
        // panicking or drawing an empty row.
        let rebuilt = PrismUi::from_engine(99, |id| prism_value(id, engine.get(id)));
        assert!(rebuilt.selected < pp::BANDS);
    }

    /// The axis agrees with itself: what is drawn at a frequency reads
    /// back as that frequency.
    #[test]
    fn the_frequency_axis_round_trips() {
        let theme = Theme::dark();
        let field = field(&theme);
        for hz in [20.0f32, 100.0, 1_000.0, 5_000.0, 20_000.0] {
            let back = axis_hz(field, axis_x(field, hz));
            assert!(
                (back / hz - 1.0).abs() < 0.02,
                "{hz} Hz drew at a point that reads as {back}"
            );
        }
        // And the bands are told apart by that axis, in order.
        let state = PrismUi::default();
        let [low, mid, high] = spans(field, &state);
        assert!(low.1 <= mid.0 && mid.1 <= high.0, "the spans overlap");
    }
}
