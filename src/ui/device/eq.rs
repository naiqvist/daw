//! The eight-band equaliser's device card — the response, drawn and
//! dragged.
//!
//! Ids, ranges and defaults come from [`crate::params::eq`] — the one
//! table this widget, `Node::Eq`'s core and the app's edit routing all
//! read.
//!
//! # The anatomy, from `notes/20260826-instrument-screen-design-guide.md`
//!
//! ```text
//! ┌ eq ─────────────────────────────────────────────┐  context strip
//! │ ┌ screen ─────────────────────────────────────┐ │
//! │ │ band 3  bell  440 Hz              +12       │ │  corner tag, dB rule
//! │ │        ╭───╮                                │ │
//! │ │ ───────╯   ╰──╮        ╭──④─────────────  0 │ │  the response
//! │ │  ①   ②    ③   ╰────────╯               -12 │ │
//! │ │ 100        1k        10k                    │ │  decade labels
//! │ ├─────────────────────────────────────────────┤ │
//! │ │ BAND  ON  TYPE  FREQ  GAIN   Q    OUT       │ │  the selected band
//! │ └─────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────┘
//! ```
//!
//! # Why one row of cells and not eight
//!
//! Eight bands times five controls is forty cells, and forty cells on one
//! card is a spreadsheet. The screen guide's answer is that the picture
//! IS the control: all eight bands are always visible as numbered handles
//! on the curve, and the row underneath belongs to whichever one is
//! selected. Selecting is clicking a handle — or stepping the BAND cell,
//! for when your hands are already on the row.
//!
//! That is EQ Eight's information architecture and it is the right one:
//! the thing you compare across bands is their SHAPE, which the curve
//! shows all at once, and the thing you set precisely is one band at a
//! time.
//!
//! # What the picture actually shows
//!
//! The response the audio will have, summed across every switched-on
//! band, evaluated on the unit circle at the device's own sample rate —
//! so it warps toward Nyquist the way the filters do. A band's bell or
//! shelf is drawn from [`crate::dsp::filters::EqBand::coeffs`], which is
//! the KERNEL's own transfer function: the curve cannot disagree with the
//! sound because it is not a second opinion about it.

use crate::params::eq as ep;
use crate::params::{self};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{
    Footprint, Mapping, Param, Unit, Well, Wells, adjust, card, design, filter, metrics,
    poly_widgets,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// How far above and below zero the display reaches, in dB.
///
/// Wider than a band's own ceiling on purpose: at `MAX_GAIN_DB` exactly,
/// a maxed band would draw along the frame and read as clipped rather
/// than as loud. Cuts fall past the bottom and are clamped there, which
/// is what every equaliser display does with an infinite rolloff.
const VIEW_DB: f32 = ep::MAX_GAIN_DB + 3.0;

/// The dB lines the grid draws, besides the bright zero.
const RULE_DB: [f32; 2] = [12.0, -12.0];

/// The decade labels along the bottom.
const LABEL_HZ: [f32; 3] = [100.0, 1_000.0, 10_000.0];

/// How wide a step to sample the curve at, in points. Two is under the
/// eye's resolution at this size and keeps a wide card's polyline short.
const CURVE_STEP_PX: f32 = 2.0;

/// How near a handle the pointer must come to grab it, in points. The
/// screen guide's 20-point floor: the drawn dot is smaller than this.
const GRAB_PT: f32 = 11.0;

/// The sample rate the curve is drawn at when the device is not running.
///
/// A response is a function of the rate, so the picture needs one. The
/// engine's real rate arrives with the card when there is an engine; this
/// is what a card drawn with no stream open uses, and 48 kHz is what the
/// project targets.
pub const ASSUMED_RATE: f32 = 48_000.0;

/// One band's knob positions, normalized. Serialized into project files.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BandUi {
    pub on: f32,
    pub shape: f32,
    pub freq: f32,
    pub gain: f32,
    pub q: f32,
}

impl Default for BandUi {
    fn default() -> Self {
        Self::at(0)
    }
}

impl BandUi {
    /// Band `band` at its table defaults, run backwards through the same
    /// mapping the controls use.
    pub fn at(band: usize) -> Self {
        let at = |slot: u32| {
            let id = ep::id(band, slot);
            eq_norm(id, params::def(ep::TABLE, id).default)
        };
        Self {
            on: at(ep::ON),
            shape: at(ep::TYPE),
            freq: at(ep::FREQ),
            gain: at(ep::GAIN),
            q: at(ep::Q),
        }
    }

    fn slot(&mut self, slot: u32) -> Option<&mut f32> {
        Some(match slot {
            ep::ON => &mut self.on,
            ep::TYPE => &mut self.shape,
            ep::FREQ => &mut self.freq,
            ep::GAIN => &mut self.gain,
            ep::Q => &mut self.q,
            _ => return None,
        })
    }
}

/// The whole card's state.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct EqUi {
    pub bands: [BandUi; ep::BANDS],
    pub out: f32,
    /// Which band the cell row edits. UI state, not a parameter: it
    /// changes nothing about the sound, and the engine has never heard
    /// of it.
    pub selected: usize,
}

impl Default for EqUi {
    fn default() -> Self {
        let mut bands = [BandUi::at(0); ep::BANDS];
        for (band, slot) in bands.iter_mut().enumerate() {
            *slot = BandUi::at(band);
        }
        Self {
            bands,
            out: eq_norm(ep::OUT, params::def(ep::TABLE, ep::OUT).default),
            // The first BELL, not band 1: opening on a low cut invites
            // the first drag to be a cut, and that is not what an EQ is
            // mostly for.
            selected: 3,
        }
    }
}

impl EqUi {
    /// This state's knob position for a wire id, mutably. One place an id
    /// becomes a field, so a loop over the table cannot route an edit
    /// into the wrong control.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        if param == ep::OUT {
            return Some(&mut self.out);
        }
        let (band, slot) = ep::split(param)?;
        self.bands.get_mut(band)?.slot(slot)
    }

    /// Put a control at a normalized position by wire id. What the app
    /// uses to reflect a loaded patch onto the card without knowing which
    /// field is which.
    pub fn set_norm(&mut self, param: u32, norm: f32) {
        if let Some(slot) = self.slot(param) {
            *slot = norm.clamp(0.0, 1.0);
        }
    }

    fn band(&self, band: usize) -> BandUi {
        self.bands.get(band).copied().unwrap_or_default()
    }

    /// Which band the row edits, always inside the array.
    fn current(&self) -> usize {
        self.selected.min(ep::BANDS - 1)
    }
}

/// One control by wire id — the single place an id becomes a [`Param`].
fn param_of(id: u32) -> Param {
    let def = params::def(ep::TABLE, id);
    if id == ep::OUT {
        return Param::db("out", def.min, def.max).with_default(def.default);
    }
    let slot = ep::split(id).map(|(_, slot)| slot).unwrap_or(ep::FREQ);
    match slot {
        ep::ON => Param::choice("on", &["off", "on"]).with_default(def.default),
        ep::TYPE => Param::choice("type", ep::TYPE_NAMES).with_default(def.default),
        // LOG, both of them: a corner frequency is heard in ratios, and
        // so is a Q — the interesting part of a Q control is all at the
        // bottom, and a linear dial spends its travel above it.
        ep::FREQ => Param::hz("freq", def.min, def.max).with_default(def.default),
        ep::GAIN => Param::db("gain", def.min, def.max)
            .bipolar()
            .with_default(def.default),
        _ => Param::new(
            "q",
            Mapping::Log {
                min: def.min,
                max: def.max,
            },
            Unit::Plain,
        )
        .with_default(def.default),
    }
}

/// The engine-facing value at a normalized position, by param id.
pub fn eq_value(param: u32, norm: f32) -> f32 {
    params::def(ep::TABLE, param).clamp(param_of(param).value(norm))
}

/// The inverse of [`eq_value`], for a state stored in engine units.
pub fn eq_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(value)
}

/// Whether a parameter snaps to named settings rather than sweeping.
pub fn eq_is_discrete(param: u32) -> bool {
    param_of(param).choices().is_some()
}

/// Whether a parameter lives on a LOG scale — so a modulation sweep of a
/// frequency or a Q moves in ratios exactly as the control does.
pub fn eq_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, whether or not it moved.
pub fn eq_edits(state: &EqUi) -> Vec<ParamEdit> {
    let mut state = *state;
    ep::TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: eq_value(def.id, *norm),
            })
        })
        .collect()
}

// ---------------------------------------------------------------- curve ---

/// One band's settings in ENGINE units, which is what a response is a
/// function of.
#[derive(Debug, Clone, Copy)]
struct BandView {
    on: bool,
    shape: u32,
    hz: f32,
    gain_db: f32,
    q: f32,
}

fn view_of(state: &EqUi, band: usize) -> BandView {
    let ui = state.band(band);
    let at = |slot: u32, norm: f32| eq_value(ep::id(band, slot), norm);
    BandView {
        on: at(ep::ON, ui.on) >= 0.5,
        shape: param_of(ep::id(band, ep::TYPE)).index(ui.shape) as u32,
        hz: at(ep::FREQ, ui.freq),
        gain_db: at(ep::GAIN, ui.gain),
        q: at(ep::Q, ui.q),
    }
}

/// One band's contribution at `hz`, in dB.
///
/// Every shape reads its response from the same place the AUDIO reads its
/// coefficients: bells and shelves from the kernel's own
/// [`coeffs`](crate::dsp::filters::EqBand::coeffs), cuts and notches from
/// the cookbook section the cascade is built out of. Nothing here is an
/// analogue approximation of a digital filter.
fn band_db(band: BandView, hz: f32, sample_rate: f32) -> f32 {
    if !band.on || !hz.is_finite() || hz <= 0.0 {
        return 0.0;
    }
    let nyquist = (sample_rate * 0.5).max(ep::MIN_HZ * 2.0);
    let hz = hz.min(nyquist * 0.999);
    let corner = band.hz.clamp(ep::MIN_HZ, nyquist * 0.999);
    let w = std::f32::consts::TAU * hz / sample_rate;
    let w0 = std::f32::consts::TAU * corner / sample_rate;

    let order = ep::cut_order(band.shape);
    let mag = if order > 0 {
        // A Butterworth cascade is its sections multiplied together, and
        // both cut orders here are even — no odd first-order stage to
        // account for.
        let mode = if ep::is_highpass(band.shape) {
            filter::Mode::Highpass
        } else {
            filter::Mode::Lowpass
        };
        (0..order / 2).fold(1.0, |acc, k| {
            acc * filter::section_magnitude(mode, w0, filter::cascade_section_q(order, k), w)
        })
    } else if band.shape == ep::TYPE_NOTCH {
        filter::section_magnitude(filter::Mode::Notch, w0, band.q, w)
    } else {
        let curve = match band.shape {
            ep::TYPE_LO_SHELF => crate::dsp::filters::BandShape::LowShelf,
            ep::TYPE_HI_SHELF => crate::dsp::filters::BandShape::HighShelf,
            _ => crate::dsp::filters::BandShape::Bell,
        };
        let mut kernel = crate::dsp::filters::EqBand::new();
        kernel.prepare(sample_rate, corner, band.q, band.gain_db, curve);
        filter::magnitude_of(kernel.coeffs(), w)
    };
    20.0 * mag.max(1e-6).log10()
}

/// The whole equaliser's response at `hz`, in dB — every band summed,
/// which in dB is what cascading them multiplies out to.
pub fn response_db(state: &EqUi, hz: f32, sample_rate: f32) -> f32 {
    let bands: f32 = (0..ep::BANDS)
        .map(|band| band_db(view_of(state, band), hz, sample_rate))
        .sum();
    bands + eq_value(ep::OUT, state.out)
}

/// Where `db` sits up the display, `0..=1`.
fn db_to_norm(db: f32) -> f32 {
    ((db + VIEW_DB) / (2.0 * VIEW_DB)).clamp(0.0, 1.0)
}

/// The inverse, for a drag.
fn norm_to_db(t: f32) -> f32 {
    (t.clamp(0.0, 1.0) * 2.0 - 1.0) * VIEW_DB
}

/// Where a band's handle sits in the display, in `0..=1` across and up.
fn handle_at(band: BandView) -> egui::Pos2 {
    let x = filter::hz_to_norm(band.hz);
    // A cut or a notch takes away rather than lifts, so its handle rides
    // the zero line: there is no gain for it to be at, and parking it at
    // the bottom of the display would put it where nothing is drawn.
    let y = if ep::has_gain(band.shape) {
        db_to_norm(band.gain_db)
    } else {
        db_to_norm(0.0)
    };
    egui::pos2(x, y)
}

// ----------------------------------------------------------------- card ---

/// The cells under the picture, in the order they read: which band, then
/// what it is, then where and how much.
const STRIP: [u32; 5] = [ep::ON, ep::TYPE, ep::FREQ, ep::GAIN, ep::Q];

/// How many of the screen's cell rows the value strip stands in — two,
/// because a cell draws a value over a name and one row cannot hold both
/// without them touching.
const VALUE_ROWS: usize = 2;

/// The width one slot's cell is drawn at: the widest that slot will ever
/// need across all eight bands, so stepping the picker never reflows the
/// row. Paired with `strip_width`, which reserves exactly this.
fn widest_cell(ui: &egui::Ui, theme: &Theme, slot: u32) -> f32 {
    (0..ep::BANDS)
        .map(|band| cell_width(ui, theme, &param_of(ep::id(band, slot))))
        .fold(0.0f32, f32::max)
}

fn cell_width(ui: &egui::Ui, theme: &Theme, p: &Param) -> f32 {
    let value = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, p.name, font::MICRO_LABEL);
    value.max(name) + theme.sp(space::SM) * 2.0
}

/// The width the whole cell row needs — the sum of what each cell needs,
/// never an equal share, so a readout cannot print through its neighbour.
fn strip_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    // Every band's cells are the same width, but not every band's are the
    // WIDEST: band 1's frequency prints "20 Hz" and band 8's "20.0 kHz",
    // and a row reserved for the narrow one has the wide one print
    // through its neighbour the moment the picker moves. So the
    // reservation is the widest each cell will ever have to be.
    let widest = |slot: u32| {
        (0..ep::BANDS)
            .map(|band| cell_width(ui, theme, &param_of(ep::id(band, slot))))
            .fold(0.0f32, f32::max)
    };
    let cells = STRIP.iter().map(|slot| widest(*slot)).sum::<f32>()
        + cell_width(ui, theme, &param_of(ep::OUT))
        + cell_width(ui, theme, &band_picker());
    cells + ui.spacing().item_spacing.x * (STRIP.len() + 1) as f32
}

/// The band selector, as a `Param` so it draws and steps like every other
/// cell. Not an engine parameter — see [`EqUi::selected`].
fn band_picker() -> Param {
    Param::choice("band", BAND_NAMES)
}

const BAND_NAMES: &[&str] = &["1", "2", "3", "4", "5", "6", "7", "8"];

/// The card's layout: one well, and the screen fills it.
fn face(ui: &egui::Ui, theme: &Theme) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(strip_width(ui, theme), 0.0))
        .filling()])
}

/// Draw the equaliser card. Returns the edits the user just made.
pub fn eq_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut EqUi,
    sample_rate: f32,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = face(ui, theme);

    card::card_sized(ui, theme, "eq", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, VALUE_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => {
                        for param in curve(ui, theme, state, sample_rate) {
                            if let Some(norm) = state.slot(param) {
                                edits.push(ParamEdit {
                                    param,
                                    value: eq_value(param, *norm),
                                });
                            }
                        }
                    }
                    poly_widgets::CurveRegion::Footer => {
                        cells(ui, theme, state, &mut edits);
                    }
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The row of cells: the band picker, then the selected band's five, then
/// the output trim.
fn cells(ui: &mut egui::Ui, theme: &Theme, state: &mut EqUi, edits: &mut Vec<ParamEdit>) {
    let h = ui.available_height();
    ui.horizontal(|ui| {
        // Which band. A cell rather than a rail: it belongs in the same
        // row as the values it governs, and the eight handles on the
        // picture are the rail.
        let picker = band_picker();
        let w = cell_width(ui, theme, &picker);
        let mut norm = picker.at_index(state.current());
        ui.allocate_ui_with_layout(
            egui::vec2(w, h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(w);
                ui.set_height(h);
                if poly_widgets::labeled_cell_bar(ui, theme, &picker, &mut norm, None) {
                    state.selected = picker.index(norm).min(ep::BANDS - 1);
                }
            },
        );

        // AFTER the picker, not before it. Read once at the top, this
        // would be the band the picker was showing a frame ago: step the
        // picker and the five cells beside it would keep printing the
        // band you just left, for exactly one frame, which reads as the
        // row not following the selection at all.
        let band = state.current();
        for slot in STRIP {
            let id = ep::id(band, slot);
            let param = param_of(id);
            let w = widest_cell(ui, theme, slot);
            let Some(norm) = state.slot(id) else {
                continue;
            };
            ui.allocate_ui_with_layout(
                egui::vec2(w, h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(w);
                    ui.set_height(h);
                    if poly_widgets::labeled_cell_bar(ui, theme, &param, norm, None) {
                        edits.push(ParamEdit {
                            param: id,
                            value: eq_value(id, *norm),
                        });
                    }
                },
            );
        }

        let param = param_of(ep::OUT);
        let w = cell_width(ui, theme, &param);
        ui.allocate_ui_with_layout(
            egui::vec2(w, h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(w);
                ui.set_height(h);
                if poly_widgets::labeled_cell_bar(ui, theme, &param, &mut state.out, None) {
                    edits.push(ParamEdit {
                        param: ep::OUT,
                        value: eq_value(ep::OUT, state.out),
                    });
                }
            },
        );
    });
}

/// The picture, and the drags that live on it. Returns the ids that moved.
fn curve(ui: &mut egui::Ui, theme: &Theme, state: &mut EqUi, sample_rate: f32) -> Vec<u32> {
    let size = egui::vec2(ui.available_width(), ui.available_height());
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let mut moved = Vec::new();
    let rate = if sample_rate.is_finite() && sample_rate > 1_000.0 {
        sample_rate
    } else {
        ASSUMED_RATE
    };

    // ONE EGUI INTERACTION PER HANDLE, laid over the display — the same
    // shape `envelope.rs` gives its three ADSR handles, and the reason
    // that widget has never had this bug.
    //
    // egui tracks an interaction by widget id, so a drag that began on
    // band 3 STAYS band 3's for as long as the button is down: past its
    // neighbour, past the end of its range, off the display entirely.
    // Hand-rolling that — one big widget, then "which handle is the
    // pointer nearest?" every frame — is what this did first, and it was
    // wrong in three separate ways at once: a press on empty ground
    // still moved whichever band was selected; a handle that hit the end
    // of its range stopped following, so the pointer ran away and the
    // gesture died halfway; and dragging a CUT vertically, a shape with
    // no gain for its handle to follow, walked the pointer off its own
    // handle and killed the horizontal drag with it.
    //
    // The handles are allocated AFTER the background, so they win the
    // pointer where the two overlap — egui gives a press to the last
    // widget added at that position.
    let reach = theme.sp(GRAB_PT);
    for band in 0..ep::BANDS {
        let at = handle_pos(state, rect, band);
        let hit = egui::Rect::from_center_size(at, egui::Vec2::splat(reach * 2.0));
        let handle = ui
            .interact(
                hit,
                response.id.with(("band", band)),
                egui::Sense::click_and_drag(),
            )
            .affords(Affords::Steer);
        if handle.drag_started() || handle.clicked() {
            state.selected = band;
        }
        if handle.dragged() {
            let view = view_of(state, band);
            let d = handle.drag_delta();
            if d.x != 0.0 {
                let hz = drag_freq(view.hz, d.x, rect.width());
                if push(state, band, ep::FREQ, hz, &mut moved) {
                    // Dragging a band is asking for it: a handle that
                    // moved and changed nothing would read as broken.
                    enable(state, band, &mut moved);
                }
            }
            if d.y != 0.0 && ep::has_gain(view.shape) {
                let db = drag_gain(view.gain_db, d.y, rect.height());
                if push(state, band, ep::GAIN, db, &mut moved) {
                    enable(state, band, &mut moved);
                }
            }
        }
        if handle.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
    }

    // A press on the open display selects the nearest band along the
    // FREQUENCY axis without moving anything — a click at 5 kHz means the
    // band nearest 5 kHz, wherever its gain has carried it vertically.
    //
    // Both halves of the press, not just `clicked()`: under
    // `click_and_drag` a press that wanders three pixels is a DRAG, and
    // `clicked()` never fires for it. Selecting only there meant the row
    // followed a perfectly still hand and ignored a normal one.
    if (response.drag_started() || response.clicked())
        && let Some(at) = response.interact_pointer_pos()
    {
        state.selected = nearest_by_freq(rect, at, state);
    }

    // The wheel is Q, on the selected band — the third dimension every
    // equaliser puts there, and the one a two-axis drag has no room for.
    let band = state.current();
    let view = view_of(state, band);
    let nudge = adjust::nudge(ui, &response);
    if nudge != 0.0 {
        let id = ep::id(band, ep::Q);
        let t = eq_norm(id, view.q) + nudge;
        let q = eq_value(id, t.clamp(0.0, 1.0));
        if push(state, band, ep::Q, q, &mut moved) {
            enable(state, band, &mut moved);
        }
    }

    paint(ui, theme, rect, state, rate, &response);
    moved
}

/// Where a band's handle sits on screen.
fn handle_pos(state: &EqUi, rect: egui::Rect, band: usize) -> egui::Pos2 {
    let p = handle_at(view_of(state, band));
    egui::pos2(
        rect.left() + rect.width() * p.x,
        rect.bottom() - rect.height() * p.y,
    )
}

/// Which band a press at `at` lands on: the nearest handle within reach,
/// or none for the open ground between handles.
///
/// egui decides this for real — each handle owns an interaction of its
/// own — so this exists to state the same geometry for the tests that
/// check the handles can be told apart at all.
#[cfg(test)]
fn grabbed_band(state: &EqUi, rect: egui::Rect, at: egui::Pos2, theme: &Theme) -> Option<usize> {
    (0..ep::BANDS)
        .map(|band| (band, handle_pos(state, rect, band).distance(at)))
        .filter(|(_, d)| *d <= theme.sp(GRAB_PT))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(band, _)| band)
}

/// The band nearest `at` along the frequency axis — what a click that
/// missed every handle selects. Never `None`: there are always eight
/// bands and a click is always somewhere.
fn nearest_by_freq(rect: egui::Rect, at: egui::Pos2, state: &EqUi) -> usize {
    (0..ep::BANDS)
        .min_by(|a, b| {
            let dx = |band: usize| (handle_pos(state, rect, band).x - at.x).abs();
            dx(*a).total_cmp(&dx(*b))
        })
        .unwrap_or(0)
}

/// Where a horizontal drag of `dx` points across a `width`-wide display
/// puts a corner currently at `hz`. Log, so a drag moves the same RATIO
/// wherever it starts — which is what the axis under it does.
fn drag_freq(hz: f32, dx: f32, width: f32) -> f32 {
    if width <= 0.0 {
        return hz;
    }
    let t = filter::hz_to_norm(hz) + dx / width;
    filter::norm_to_hz(t).clamp(ep::MIN_HZ, ep::MAX_HZ)
}

/// Where a vertical drag of `dy` points puts a gain currently at `db`.
///
/// Screen y grows DOWNWARD, so a pointer moving UP arrives here as a
/// NEGATIVE `dy` and has to come back as more gain. The sign is the one
/// thing about this that is easy to get backwards and impossible to
/// notice in a screenshot, which is why it is a function with a test
/// rather than an expression inside a drag handler.
fn drag_gain(db: f32, dy: f32, height: f32) -> f32 {
    if height <= 0.0 {
        return db;
    }
    let t = db_to_norm(db) - dy / height;
    norm_to_db(t).clamp(-ep::MAX_GAIN_DB, ep::MAX_GAIN_DB)
}

/// Write an engine value into a band's normalized slot. Returns whether
/// it moved.
fn push(state: &mut EqUi, band: usize, slot: u32, value: f32, moved: &mut Vec<u32>) -> bool {
    let id = ep::id(band, slot);
    let next = eq_norm(id, value);
    let Some(norm) = state.slot(id) else {
        return false;
    };
    if *norm == next {
        return false;
    }
    *norm = next;
    moved.push(id);
    true
}

/// Switch a band on because it was just edited.
fn enable(state: &mut EqUi, band: usize, moved: &mut Vec<u32>) {
    let id = ep::id(band, ep::ON);
    let on = param_of(id).at_index(1);
    let Some(norm) = state.slot(id) else {
        return;
    };
    if *norm != on {
        *norm = on;
        moved.push(id);
    }
}

fn paint(
    ui: &egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    state: &EqUi,
    sample_rate: f32,
    response: &egui::Response,
) {
    let painter = ui.painter();
    let x_of = |hz: f32| rect.left() + rect.width() * filter::hz_to_norm(hz);
    let y_of = |db: f32| rect.bottom() - rect.height() * db_to_norm(db);

    // --- the grid: decades across, the zero line along ---------------------
    for hz in LABEL_HZ {
        let x = x_of(hz);
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(stroke::HAIR, theme.grid_beat),
        );
        painter.text(
            egui::pos2(
                x + theme.sp(space::XXS),
                rect.bottom() - theme.sp(space::XXS),
            ),
            egui::Align2::LEFT_BOTTOM,
            if hz >= 1_000.0 {
                format!("{:.0}k", hz / 1_000.0)
            } else {
                format!("{hz:.0}")
            },
            egui::FontId::monospace(font::MICRO_LABEL),
            theme.text_muted,
        );
    }
    for db in RULE_DB {
        let y = y_of(db);
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(stroke::HAIR, theme.grid_beat),
        );
    }
    // Zero is brighter than the rest: a bipolar control needs a visible
    // zero, and every gain on this screen is read against it.
    let zero = y_of(0.0);
    painter.line_segment(
        [
            egui::pos2(rect.left(), zero),
            egui::pos2(rect.right(), zero),
        ],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );

    // --- the response ------------------------------------------------------
    let steps = ((rect.width() / CURVE_STEP_PX).ceil() as usize).clamp(2, 4_096);
    let points: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let hz = filter::norm_to_hz(t);
            egui::pos2(
                rect.left() + rect.width() * t,
                y_of(response_db(state, hz, sample_rate)),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(stroke::MARK, theme.role_level),
    ));

    // --- the handles -------------------------------------------------------
    let selected = state.current();
    let radius = theme.sp(space::XS);
    for band in 0..ep::BANDS {
        let view = view_of(state, band);
        let p = handle_at(view);
        let at = egui::pos2(
            rect.left() + rect.width() * p.x,
            rect.bottom() - rect.height() * p.y,
        );
        let live = band == selected;
        // Colour is an INDEX here, not atmosphere: a band's identity is
        // its shape family, so a cut reads as the destructive edge and a
        // gain band as level.
        let role = if ep::has_gain(view.shape) {
            theme.role_level
        } else {
            theme.role_mod
        };
        let color = if !view.on {
            theme.outline
        } else if live {
            role
        } else {
            theme.text_muted
        };
        if live {
            painter.circle_stroke(
                at,
                radius + theme.sp(space::XXS),
                egui::Stroke::new(stroke::HAIR, color),
            );
        }
        painter.circle_filled(at, radius, color);
        painter.text(
            at,
            egui::Align2::CENTER_CENTER,
            format!("{}", band + 1),
            egui::FontId::monospace(font::MICRO_LABEL),
            theme.surface_sunken,
        );
    }

    // --- the corner tag ----------------------------------------------------
    let view = view_of(state, selected);
    let type_param = param_of(ep::id(selected, ep::TYPE));
    let tag = if ep::has_gain(view.shape) {
        format!(
            "band {}  {}  {}  {}",
            selected + 1,
            type_param.format(state.band(selected).shape),
            param_of(ep::id(selected, ep::FREQ)).format(state.band(selected).freq),
            param_of(ep::id(selected, ep::GAIN)).format(state.band(selected).gain),
        )
    } else {
        format!(
            "band {}  {}  {}",
            selected + 1,
            type_param.format(state.band(selected).shape),
            param_of(ep::id(selected, ep::FREQ)).format(state.band(selected).freq),
        )
    };
    painter.text(
        rect.left_top() + egui::vec2(design::gap(theme), design::gap(theme)),
        egui::Align2::LEFT_TOP,
        tag,
        egui::FontId::monospace(font::LABEL),
        theme.text_muted,
    );
    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ui::device::probe;
    use crate::ui::tokens::control;

    const MAX_W: f32 = 1_100.0;
    const MIN_W: f32 = 300.0;

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
        out.unwrap()
    }

    #[test]
    fn every_table_row_leaves_as_an_edit() {
        let edits = eq_edits(&EqUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = ep::TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect, "the card does not cover the table");
    }

    /// Every position a control can be dragged to is a value the engine
    /// will accept, and the ends of each control reach the ends of its
    /// row — so clamping cannot be hiding a mapping that never gets
    /// there.
    #[test]
    fn every_position_is_a_legal_engine_value() {
        for def in ep::TABLE {
            for i in 0..=40 {
                let value = eq_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
            }
            assert!((eq_value(def.id, 0.0) - def.min).abs() < (def.max - def.min) * 1e-3);
            assert!((eq_value(def.id, 1.0) - def.max).abs() < (def.max - def.min) * 1e-3);
        }
    }

    /// A value that comes back from the engine must put the control where
    /// that value lives. Stated over VALUES rather than positions,
    /// because a discrete row quantizes on the way in — an `on` at 0.05
    /// is an `off`, and rightly so.
    #[test]
    fn value_and_norm_round_trip() {
        for def in ep::TABLE {
            for i in 0..=40 {
                let norm = i as f32 / 40.0;
                let value = eq_value(def.id, norm);
                let again = eq_value(def.id, eq_norm(def.id, value));
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
        let state = EqUi::default();
        for def in ep::TABLE {
            let mut state = state;
            let norm = *state.slot(def.id).unwrap();
            let value = eq_value(def.id, norm);
            assert!(
                (value - def.default).abs() < (def.max - def.min) * 1e-3,
                "{} opened at {value}, table says {}",
                def.name,
                def.default
            );
        }
    }

    /// A default equaliser draws a FLAT LINE. Every band opens switched
    /// off, and a picture that showed a curve where the audio has none
    /// would be the display lying about the sound.
    #[test]
    fn a_default_equaliser_draws_flat() {
        let state = EqUi::default();
        for hz in [20.0f32, 60.0, 200.0, 1_000.0, 5_000.0, 19_000.0] {
            let db = response_db(&state, hz, ASSUMED_RATE);
            assert!(db.abs() < 0.01, "{hz} Hz reads {db:+.3} dB on a flat EQ");
        }
    }

    /// The drawn curve agrees with what each band's kernel does, band by
    /// band: a bell peaks at its gain, a shelf settles at it, a cut falls
    /// away, and a notch digs a hole.
    #[test]
    fn the_curve_shows_what_each_shape_does() {
        let set = |state: &mut EqUi, band: usize, shape: u32, hz: f32, gain: f32, q: f32| {
            let put = |state: &mut EqUi, slot: u32, value: f32| {
                let id = ep::id(band, slot);
                let norm = eq_norm(id, value);
                if let Some(dst) = state.slot(id) {
                    *dst = norm;
                }
            };
            put(state, ep::ON, 1.0);
            put(state, ep::TYPE, shape as f32);
            put(state, ep::FREQ, hz);
            put(state, ep::GAIN, gain);
            put(state, ep::Q, q);
        };

        // A bell peaks at its centre and leaves the far ends alone.
        let mut state = EqUi::default();
        set(&mut state, 3, ep::TYPE_BELL, 1_000.0, 9.0, 2.0);
        let peak = response_db(&state, 1_000.0, ASSUMED_RATE);
        assert!((peak - 9.0).abs() < 0.2, "bell centre reads {peak:+.2}");
        assert!(response_db(&state, 60.0, ASSUMED_RATE).abs() < 0.5);

        // A low shelf lifts the bottom and leaves the top.
        let mut state = EqUi::default();
        set(&mut state, 1, ep::TYPE_LO_SHELF, 200.0, -6.0, ep::FLAT_Q);
        assert!((response_db(&state, 25.0, ASSUMED_RATE) + 6.0).abs() < 0.4);
        assert!(response_db(&state, 8_000.0, ASSUMED_RATE).abs() < 0.4);

        // A low cut takes the bottom away, steeply.
        let mut state = EqUi::default();
        set(&mut state, 0, ep::TYPE_LO_CUT_48, 500.0, 0.0, ep::FLAT_Q);
        let corner = response_db(&state, 500.0, ASSUMED_RATE);
        assert!((corner + 3.0).abs() < 0.6, "the corner reads {corner:+.2}");
        assert!(
            response_db(&state, 125.0, ASSUMED_RATE) < -40.0,
            "two octaves down"
        );
        assert!(
            response_db(&state, 4_000.0, ASSUMED_RATE).abs() < 0.3,
            "and passes above"
        );

        // A notch digs a hole and is flat either side of it.
        let mut state = EqUi::default();
        set(&mut state, 4, ep::TYPE_NOTCH, 1_000.0, 0.0, 8.0);
        assert!(response_db(&state, 1_000.0, ASSUMED_RATE) < -40.0);
        assert!(response_db(&state, 250.0, ASSUMED_RATE).abs() < 0.5);
        assert!(response_db(&state, 4_000.0, ASSUMED_RATE).abs() < 0.5);
    }

    /// Bands SUM. Two boosts at the same frequency are one bigger boost,
    /// which is what cascading them does to the audio.
    #[test]
    fn bands_add_up_the_way_a_chain_of_them_would() {
        let mut state = EqUi::default();
        for band in [2usize, 3] {
            let put = |state: &mut EqUi, slot: u32, value: f32| {
                let id = ep::id(band, slot);
                let norm = eq_norm(id, value);
                if let Some(dst) = state.slot(id) {
                    *dst = norm;
                }
            };
            put(&mut state, ep::ON, 1.0);
            put(&mut state, ep::TYPE, ep::TYPE_BELL as f32);
            put(&mut state, ep::FREQ, 1_000.0);
            put(&mut state, ep::GAIN, 5.0);
            put(&mut state, ep::Q, 1.0);
        }
        let both = response_db(&state, 1_000.0, ASSUMED_RATE);
        assert!((both - 10.0).abs() < 0.3, "two +5 dB bells read {both:+.2}");

        // And the output trim rides on top of all of it.
        state.out = eq_norm(ep::OUT, -4.0);
        let trimmed = response_db(&state, 1_000.0, ASSUMED_RATE);
        assert!((trimmed - (both - 4.0)).abs() < 0.01);
    }

    /// UP IS LOUDER. Screen y grows downward, so the one sign in this
    /// file that a picture cannot check is checked here instead.
    #[test]
    fn dragging_up_boosts_and_dragging_down_cuts() {
        let h = 265.0;
        let up = drag_gain(0.0, -52.0, h);
        let down = drag_gain(0.0, 52.0, h);
        assert!(up > 0.0, "dragging up must boost, gave {up:+.2} dB");
        assert!(down < 0.0, "dragging down must cut, gave {down:+.2} dB");
        assert!((up + down).abs() < 1e-3, "and the two must mirror");
        // The travel matches the axis it is drawn against: the full
        // height of the display is the full height of the view.
        let full = drag_gain(-VIEW_DB, -h, h);
        assert!(
            (full - ep::MAX_GAIN_DB).abs() < 0.01,
            "a full-height drag must reach the ceiling, gave {full:+.2}"
        );
        // And it cannot be dragged past what a band can do.
        assert!(drag_gain(0.0, -10.0 * h, h) <= ep::MAX_GAIN_DB);
        assert!(drag_gain(0.0, 10.0 * h, h) >= -ep::MAX_GAIN_DB);
        // A zero-size display is a no-op, not a division.
        assert_eq!(drag_gain(3.0, -50.0, 0.0), 3.0);
    }

    /// RIGHT IS HIGHER, in RATIOS — the same log axis the curve is drawn
    /// on, so a handle stays under the pointer wherever it is grabbed.
    #[test]
    fn dragging_sideways_moves_the_corner_in_ratios() {
        let w = 750.0;
        assert!(drag_freq(1_000.0, 50.0, w) > 1_000.0, "right is higher");
        assert!(drag_freq(1_000.0, -50.0, w) < 1_000.0, "left is lower");
        // The same push travels the same RATIO at either end of the axis.
        let low = drag_freq(100.0, 75.0, w) / 100.0;
        let high = drag_freq(5_000.0, 75.0, w) / 5_000.0;
        assert!(
            (low - high).abs() < 0.01,
            "equal travel must be equal ratio: {low:.3} against {high:.3}"
        );
        assert_eq!(drag_freq(440.0, 90.0, 0.0), 440.0);
        assert!(drag_freq(1_000.0, 10.0 * w, w) <= ep::MAX_HZ);
        assert!(drag_freq(1_000.0, -10.0 * w, w) >= ep::MIN_HZ);
    }

    /// A press grabs the handle it landed on, and NOTHING when it landed
    /// on empty ground — which is what stops a drag over the open part
    /// of the curve from dragging whichever band was last selected.
    #[test]
    fn a_press_grabs_the_handle_under_it_or_nothing_at_all() {
        let theme = Theme::dark();
        let state = EqUi::default();
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(750.0, 265.0));

        for band in 0..ep::BANDS {
            let on_it = handle_pos(&state, rect, band);
            assert_eq!(
                grabbed_band(&state, rect, on_it, &theme),
                Some(band),
                "band {} did not grab its own handle",
                band + 1
            );
        }
        // The top-left corner is a long way from every handle, all of
        // which sit on the zero line at rest.
        assert_eq!(
            grabbed_band(&state, rect, rect.left_top(), &theme),
            None,
            "empty ground must grab nothing"
        );
        // And so is a point well above the zero line, which is where a
        // drag over the open display happens.
        let above = egui::pos2(rect.center().x, rect.top() + 10.0);
        assert_eq!(grabbed_band(&state, rect, above, &theme), None);
    }

    // ---------------------------------------------------- with a pointer ---

    /// The rectangle the curve is driven in, and where a band's handle
    /// sits inside it. One place, so a gesture in a test aims at the
    /// same point the widget draws on.
    const PROBE: egui::Rect = egui::Rect {
        min: egui::pos2(0.0, 0.0),
        max: egui::pos2(750.0, 265.0),
    };

    /// Run the curve through a pointer gesture and report every
    /// parameter id it moved, in order.
    fn gesture(state: &mut EqUi, path: &[probe::Step]) -> Vec<u32> {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        probe::run(&ctx, PROBE, path, |ui| {
            curve(ui, &theme, state, ASSUMED_RATE)
        })
        .into_iter()
        .flatten()
        .collect()
    }

    /// Pressing a handle selects ITS band — the thing a pointer is for,
    /// and the thing no draw-once test can see.
    #[test]
    fn pressing_a_handle_selects_that_band() {
        for band in 0..ep::BANDS {
            let mut state = EqUi::default();
            // Start somewhere else, so "it was already selected" cannot
            // pass for "the press selected it".
            state.selected = (band + 4) % ep::BANDS;
            let at = handle_pos(&state, PROBE, band);
            gesture(&mut state, &probe::click_path(at));
            assert_eq!(
                state.selected,
                band,
                "pressing band {}'s handle selected band {}",
                band + 1,
                state.selected + 1
            );
        }
    }

    /// A DRAG BELONGS TO THE HANDLE IT STARTED ON — all the way past a
    /// neighbour, which is exactly where the hand-rolled version handed
    /// the gesture over and started moving the wrong band.
    #[test]
    fn a_drag_keeps_its_band_even_when_it_crosses_another() {
        let mut state = EqUi::default();
        let from = handle_pos(&state, PROBE, 2);
        // Far enough right to travel over bands 4, 5 and 6.
        let to = egui::pos2(handle_pos(&state, PROBE, 5).x, from.y - 40.0);
        let before = state.bands;

        let moved = gesture(&mut state, &probe::drag_path(from, to, 12));

        // Band 3 moved...
        assert_ne!(state.bands[2], before[2], "the grabbed band must move");
        assert!(
            moved
                .iter()
                .all(|id| ep::split(*id).map(|(b, _)| b) == Some(2)),
            "a gesture that began on band 3 wrote to another band: {moved:?}"
        );
        // ...and nobody else did, however far the pointer travelled.
        for (band, (now, was)) in state.bands.iter().zip(&before).enumerate() {
            if band != 2 {
                assert_eq!(now, was, "band {} moved during band 3's drag", band + 1);
            }
        }
    }

    /// A drag that runs a band into the end of its range keeps going —
    /// the handle stops following the pointer, and that must not end the
    /// gesture. The hand-rolled version lost the drag here, because the
    /// pointer had walked out of the handle's radius.
    #[test]
    fn a_handle_pinned_at_the_end_of_its_range_keeps_the_drag() {
        let mut state = EqUi::default();
        let from = handle_pos(&state, PROBE, 3);
        // Way past the right edge: the frequency pins at 20 kHz long
        // before the pointer stops.
        let pinned = egui::pos2(PROBE.right() + 400.0, from.y);
        gesture(&mut state, &probe::drag_path(from, pinned, 10));
        let hz = eq_value(ep::id(3, ep::FREQ), state.bands[3].freq);
        assert!(
            hz >= ep::MAX_HZ - 1.0,
            "the drag should have pinned at the top, reached {hz:.0} Hz"
        );

        // And back again, in one gesture, from the pinned position: the
        // handle is still there to be grabbed.
        let at = handle_pos(&state, PROBE, 3);
        gesture(
            &mut state,
            &probe::drag_path(at, egui::pos2(PROBE.left(), at.y), 10),
        );
        let hz = eq_value(ep::id(3, ep::FREQ), state.bands[3].freq);
        assert!(
            hz <= ep::MIN_HZ + 1.0,
            "and back to the bottom, reached {hz:.0} Hz"
        );
    }

    /// Dragging a CUT vertically moves nothing — a cut has no gain — but
    /// it must not lose the horizontal drag along with it. This is the
    /// third way the hand-rolled version broke: the handle could not
    /// follow the pointer vertically, so the pointer left it behind and
    /// the whole gesture stopped.
    #[test]
    fn a_cut_still_drags_sideways_while_the_pointer_moves_up() {
        let mut state = EqUi::default();
        // Band 1 opens as a low cut.
        let shape = view_of(&state, 0).shape;
        assert!(!ep::has_gain(shape), "band 1 should open as a cut");

        let from = handle_pos(&state, PROBE, 0);
        let before = eq_value(ep::id(0, ep::FREQ), state.bands[0].freq);
        // Up AND to the right: the vertical half is a no-op for a cut.
        let to = egui::pos2(from.x + 200.0, PROBE.top() + 8.0);
        gesture(&mut state, &probe::drag_path(from, to, 10));

        let after = eq_value(ep::id(0, ep::FREQ), state.bands[0].freq);
        assert!(
            after > before * 1.5,
            "the sideways drag was lost: {before:.0} Hz to {after:.0} Hz"
        );
        assert_eq!(
            eq_value(ep::id(0, ep::GAIN), state.bands[0].gain),
            0.0,
            "a cut has no gain to move"
        );
    }

    /// A press on the open display selects, and moves nothing at all.
    #[test]
    fn pressing_empty_ground_selects_without_moving_anything() {
        let mut state = EqUi::default();
        state.selected = 0;
        let before = state.bands;
        // Well above the zero line, between two handles.
        let at = egui::pos2(
            (handle_pos(&state, PROBE, 4).x + handle_pos(&state, PROBE, 5).x) * 0.5,
            PROBE.top() + 20.0,
        );
        let moved = gesture(&mut state, &probe::click_path(at));
        assert!(moved.is_empty(), "empty ground moved {moved:?}");
        assert_eq!(state.bands, before, "empty ground changed a band");
        assert!(
            state.selected == 4 || state.selected == 5,
            "a press between bands 5 and 6 selected band {}",
            state.selected + 1
        );
    }

    /// Dragging a handle switches its band ON — the edit list has to
    /// carry that, or the engine keeps a band the picture says is live.
    #[test]
    fn dragging_a_handle_switches_its_band_on_and_says_so() {
        let mut state = EqUi::default();
        assert!(!view_of(&state, 3).on, "bands open switched off");
        let from = handle_pos(&state, PROBE, 3);
        let moved = gesture(
            &mut state,
            &probe::drag_path(from, egui::pos2(from.x, from.y - 50.0), 8),
        );
        assert!(view_of(&state, 3).on, "the drag did not switch the band on");
        assert!(
            moved.contains(&ep::id(3, ep::ON)),
            "the ON change never left as an edit: {moved:?}"
        );
    }

    /// A click ALWAYS selects a band, so the row underneath is always
    /// about whichever band was last pointed at — an exact hit picks
    /// that handle, and a miss picks the nearest along the frequency
    /// axis rather than leaving the row where it was.
    #[test]
    fn a_click_always_selects_a_band() {
        let theme = Theme::dark();
        let state = EqUi::default();
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(750.0, 265.0));

        // An exact hit is that band, wherever the row was before.
        for band in 0..ep::BANDS {
            let on_it = handle_pos(&state, rect, band);
            let picked = grabbed_band(&state, rect, on_it, &theme)
                .unwrap_or_else(|| nearest_by_freq(rect, on_it, &state));
            assert_eq!(picked, band, "clicking band {} picked {picked}", band + 1);
        }

        // A miss still lands somewhere: directly above a handle, at the
        // top of the display, is that handle's band.
        for band in 0..ep::BANDS {
            let above = egui::pos2(handle_pos(&state, rect, band).x, rect.top() + 4.0);
            assert_eq!(
                nearest_by_freq(rect, above, &state),
                band,
                "a click above band {} picked another",
                band + 1
            );
        }

        // The far corners pick the outermost bands rather than nothing.
        assert_eq!(nearest_by_freq(rect, rect.left_top(), &state), 0);
        assert_eq!(
            nearest_by_freq(rect, rect.right_bottom(), &state),
            ep::BANDS - 1
        );
    }

    /// Handles are far enough apart at their defaults that a press can
    /// tell them apart — eight bands stacked on one zero line is exactly
    /// the case where a grab radius could swallow its neighbour.
    #[test]
    fn no_two_default_handles_sit_inside_one_grab_radius() {
        let theme = Theme::dark();
        let state = EqUi::default();
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(750.0, 265.0));
        let reach = theme.sp(GRAB_PT);
        for a in 0..ep::BANDS {
            for b in (a + 1)..ep::BANDS {
                let gap = handle_pos(&state, rect, a).distance(handle_pos(&state, rect, b));
                assert!(
                    gap > reach,
                    "bands {} and {} are {gap:.1} pt apart, inside the {reach:.1} pt grab",
                    a + 1,
                    b + 1
                );
            }
        }
    }

    /// The card fits its budget and draws at a sane width.
    #[test]
    fn the_card_stays_inside_its_budget() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = EqUi::default();
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| eq_card(ui, &theme, &mut state, ASSUMED_RATE))
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

    /// Drawing at rest must not move a control — a card that emits on its
    /// first frame writes its own defaults over a loaded patch.
    #[test]
    fn drawing_at_rest_emits_nothing() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = EqUi::default();
        let before = state;
        for _ in 0..2 {
            let edits = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| eq_card(ui, &theme, &mut state, ASSUMED_RATE))
                    .inner
            });
            assert!(edits.is_empty(), "the card emitted {edits:?} at rest");
        }
        assert_eq!(state, before, "drawing the card moved a control");
    }
}
