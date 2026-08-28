//! HAZE's card.
//!
//! Four pages, and on each one the HERO IS THE CONTROL — the screen
//! guide's second rule, taken as far as this card can take it. There is
//! no page here where the picture merely illustrates a knob that lives
//! somewhere else: the stack is dragged apart, the filter's corner is
//! dragged around, the envelope's corners are dragged, and the texture
//! field is dragged. The cells beneath give the precision a picture
//! cannot, which is the other half of that rule and not an afterthought.
//!
//! # Why four pages and not one strip
//!
//! Nineteen parameters in one footer is four rows, and the layout note's
//! own table says four rows leaves the plot thirty-four points — a line
//! of pixels rather than a picture. A pad synth whose hero is a line of
//! pixels has thrown away the only thing that makes a slow instrument
//! playable, which is being able to SEE the slow thing before you wait
//! for it.
//!
//! So the voice path is named in order on the card's own dot rail —
//! tone, filter, amp, air — and each page keeps a full-height hero and
//! one row of cells. Switching a page replaces what is inside the
//! reserved rectangle and moves nothing, which is the guide's rule about
//! layout motion.
//!
//! # About animation
//!
//! The guide forbids idle animation and allows it for playback. This
//! card has no telemetry — it is a pure function of knob positions — so
//! it cannot honestly animate what the voices are doing, and a loop that
//! LOOKED like voice activity would be a picture claiming to show state
//! it does not have.
//!
//! What it does instead is animate under the hand: while an envelope
//! handle is held, a sweep runs the shape so the timing being dialled
//! can be seen rather than counted. Motion where a gesture is happening,
//! stillness otherwise.

use super::{
    Footprint, Mapping, Param, ParamEdit, Unit, Well, Wells, card, design, metrics, poly_widgets,
};
use crate::params::{self, haze as hp};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// The card's stored knob positions, all normalized `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HazeUi {
    pub spread: f32,
    pub shape: f32,
    pub sub: f32,
    pub drift: f32,
    pub cutoff: f32,
    pub resonance: f32,
    pub track: f32,
    pub env: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub filter_attack: f32,
    pub filter_decay: f32,
    pub ensemble: f32,
    pub wow: f32,
    pub grain: f32,
    pub warmth: f32,
    pub level: f32,
}

impl Default for HazeUi {
    fn default() -> Self {
        Self::from_engine(|id| params::def(hp::TABLE, id).default)
    }
}

impl HazeUi {
    /// The card's state for a patch in engine units.
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| haze_norm(id, get(id));
        Self {
            spread: at(hp::SPREAD),
            shape: at(hp::SHAPE),
            sub: at(hp::SUB),
            drift: at(hp::DRIFT),
            cutoff: at(hp::CUTOFF),
            resonance: at(hp::RESONANCE),
            track: at(hp::TRACK),
            env: at(hp::ENV),
            attack: at(hp::ATTACK),
            decay: at(hp::DECAY),
            sustain: at(hp::SUSTAIN),
            release: at(hp::RELEASE),
            filter_attack: at(hp::FILTER_ATTACK),
            filter_decay: at(hp::FILTER_DECAY),
            ensemble: at(hp::ENSEMBLE),
            wow: at(hp::WOW),
            grain: at(hp::GRAIN),
            warmth: at(hp::WARMTH),
            level: at(hp::LEVEL),
        }
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            hp::SPREAD => &mut self.spread,
            hp::SHAPE => &mut self.shape,
            hp::SUB => &mut self.sub,
            hp::DRIFT => &mut self.drift,
            hp::CUTOFF => &mut self.cutoff,
            hp::RESONANCE => &mut self.resonance,
            hp::TRACK => &mut self.track,
            hp::ENV => &mut self.env,
            hp::ATTACK => &mut self.attack,
            hp::DECAY => &mut self.decay,
            hp::SUSTAIN => &mut self.sustain,
            hp::RELEASE => &mut self.release,
            hp::FILTER_ATTACK => &mut self.filter_attack,
            hp::FILTER_DECAY => &mut self.filter_decay,
            hp::ENSEMBLE => &mut self.ensemble,
            hp::WOW => &mut self.wow,
            hp::GRAIN => &mut self.grain,
            hp::WARMTH => &mut self.warmth,
            hp::LEVEL => &mut self.level,
            _ => return None,
        })
    }

    fn get(&self, param: u32) -> f32 {
        match param {
            hp::SPREAD => self.spread,
            hp::SHAPE => self.shape,
            hp::SUB => self.sub,
            hp::DRIFT => self.drift,
            hp::CUTOFF => self.cutoff,
            hp::RESONANCE => self.resonance,
            hp::TRACK => self.track,
            hp::ENV => self.env,
            hp::ATTACK => self.attack,
            hp::DECAY => self.decay,
            hp::SUSTAIN => self.sustain,
            hp::RELEASE => self.release,
            hp::FILTER_ATTACK => self.filter_attack,
            hp::FILTER_DECAY => self.filter_decay,
            hp::ENSEMBLE => self.ensemble,
            hp::WOW => self.wow,
            hp::GRAIN => self.grain,
            hp::WARMTH => self.warmth,
            _ => self.level,
        }
    }
}

/// Every parameter, in the order the card lays them out.
const CELLS: [u32; 19] = [
    hp::SPREAD,
    hp::SHAPE,
    hp::SUB,
    hp::DRIFT,
    hp::CUTOFF,
    hp::RESONANCE,
    hp::TRACK,
    hp::ENV,
    hp::FILTER_ATTACK,
    hp::FILTER_DECAY,
    hp::ATTACK,
    hp::DECAY,
    hp::SUSTAIN,
    hp::RELEASE,
    hp::ENSEMBLE,
    hp::WOW,
    hp::GRAIN,
    hp::WARMTH,
    hp::LEVEL,
];

/// One control by wire id, in NATURAL units end to end.
///
/// Times are LOG: on an instrument whose attack reaches eight seconds,
/// a linear knob spends its first third between 2.6 and 5.3 seconds and
/// its last few degrees crossing everything short. A pad is dialled at
/// the short end as often as the long one.
fn param_of(param: u32) -> Param {
    let def = params::def(hp::TABLE, param);
    let linear = |name: &'static str, unit| {
        Param::new(
            name,
            Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            unit,
        )
        .with_default(def.default)
    };
    let log = |name: &'static str, unit| {
        Param::new(
            name,
            Mapping::Log {
                min: def.min,
                max: def.max,
            },
            unit,
        )
        .with_default(def.default)
    };
    match param {
        hp::SPREAD => linear("spread", Unit::Plain),
        hp::SHAPE => Param::percent("shape").with_default(def.default * 100.0),
        hp::SUB => Param::percent("sub").with_default(def.default * 100.0),
        hp::DRIFT => Param::percent("drift").with_default(def.default * 100.0),
        hp::CUTOFF => log("cutoff", Unit::Hz),
        hp::RESONANCE => Param::percent("reso").with_default(def.default * 100.0),
        hp::TRACK => Param::percent("track").with_default(def.default * 100.0),
        hp::ENV => linear("env", Unit::Plain).bipolar(),
        hp::ATTACK => log("attack", Unit::Seconds),
        hp::DECAY => log("decay", Unit::Seconds),
        hp::SUSTAIN => Param::percent("sustain").with_default(def.default * 100.0),
        hp::RELEASE => log("release", Unit::Seconds),
        hp::FILTER_ATTACK => log("f.atk", Unit::Seconds),
        hp::FILTER_DECAY => log("f.dec", Unit::Seconds),
        hp::ENSEMBLE => Param::percent("ens").with_default(def.default * 100.0),
        hp::WOW => Param::percent("wow").with_default(def.default * 100.0),
        hp::GRAIN => Param::percent("grain").with_default(def.default * 100.0),
        hp::WARMTH => Param::percent("warmth").with_default(def.default * 100.0),
        _ => Param::percent("level").with_default(def.default * 50.0),
    }
}

/// What the widget SHOWS for an engine value.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        hp::SHAPE
        | hp::SUB
        | hp::DRIFT
        | hp::RESONANCE
        | hp::TRACK
        | hp::SUSTAIN
        | hp::ENSEMBLE
        | hp::WOW
        | hp::GRAIN
        | hp::WARMTH => value * 100.0,
        // Level runs to 2.0, so full scale is 200 % and unity is 100.
        hp::LEVEL => value * 50.0,
        _ => value,
    }
}

/// The engine value a shown one means.
fn natural(param: u32, value: f32) -> f32 {
    match param {
        hp::SHAPE
        | hp::SUB
        | hp::DRIFT
        | hp::RESONANCE
        | hp::TRACK
        | hp::SUSTAIN
        | hp::ENSEMBLE
        | hp::WOW
        | hp::GRAIN
        | hp::WARMTH => value / 100.0,
        hp::LEVEL => value / 50.0,
        _ => value,
    }
}

pub fn haze_value(param: u32, norm: f32) -> f32 {
    let def = params::def(hp::TABLE, param);
    natural(param, param_of(param).value(norm)).clamp(def.min, def.max)
}

pub fn haze_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

pub fn haze_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every knob as an edit — what the app writes when a patch loads.
pub fn haze_edits(state: &HazeUi) -> Vec<ParamEdit> {
    CELLS
        .into_iter()
        .map(|param| ParamEdit {
            param,
            value: haze_value(param, state.get(param)),
        })
        .collect()
}

/// The pages, in SIGNAL ORDER: what the note is, what shapes it, how it
/// arrives and leaves, and what the whole thing was recorded onto.
///
/// Named in order on the rail, which is the guide's check that a
/// first-time reader can find the voice path without being told it.
const PAGES: [(&str, &[u32]); 4] = [
    ("tone", &[hp::SPREAD, hp::SHAPE, hp::SUB, hp::DRIFT]),
    (
        "filter",
        &[
            hp::CUTOFF,
            hp::RESONANCE,
            hp::TRACK,
            hp::ENV,
            hp::FILTER_ATTACK,
            hp::FILTER_DECAY,
        ],
    ),
    ("amp", &[hp::ATTACK, hp::DECAY, hp::SUSTAIN, hp::RELEASE]),
    (
        "air",
        &[hp::ENSEMBLE, hp::WOW, hp::GRAIN, hp::WARMTH, hp::LEVEL],
    ),
];

/// A labelled cell is TWO lines: its value, then its name.
const CELL_UNITS: usize = 2;
/// One row of cells per page, so the hero keeps the height.
const FOOTER_ROWS: usize = CELL_UNITS;
/// The page rail's own height, in the same units.
const HEADER_ROWS: usize = 1;

fn cell_min_width(ui: &egui::Ui, theme: &Theme, param: &Param) -> f32 {
    let value = metrics::mono_w(ui, &param.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &param.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

/// The width the face needs: its WIDEST PAGE at its narrowest.
///
/// Every page, not the current one — a card that reserved for `tone` and
/// then showed `filter` would crush six cells into four cells' room the
/// moment the rail was clicked, and the silhouette must not change with
/// the page.
fn face_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    PAGES
        .iter()
        .map(|(_, row)| {
            let cells: f32 = row
                .iter()
                .map(|id| cell_min_width(ui, theme, &param_of(*id)))
                .sum();
            cells + ui.spacing().item_spacing.x * row.len().saturating_sub(1) as f32
        })
        .fold(0.0, f32::max)
}

/// Draw the haze card. Returns the edits the user just made.
pub fn haze_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut HazeUi,
    page: &mut usize,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);

    card::card_sized(ui, theme, "haze", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(
                ui,
                theme,
                None,
                0.0,
                HEADER_ROWS,
                FOOTER_ROWS,
                |ui, region| match region {
                    poly_widgets::CurveRegion::Header => rail(ui, theme, page),
                    poly_widgets::CurveRegion::Plot => {
                        hero(ui, theme, state, *page, &mut edits);
                    }
                    poly_widgets::CurveRegion::Footer => {
                        footer(ui, theme, state, *page, &mut edits);
                    }
                },
            );
        });
    });
    edits
}

/// The page rail: four names, the current one lit.
///
/// One interaction per tab rather than one over the strip with a
/// nearest-tab search — the device UI contract's first rule, and the
/// cheapest possible place to obey it.
fn rail(ui: &mut egui::Ui, theme: &Theme, page: &mut usize) {
    let rect = ui.available_rect_before_wrap();
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    *page = (*page).min(PAGES.len() - 1);
    let width = rect.width() / PAGES.len() as f32;
    for (index, (label, _)) in PAGES.iter().enumerate() {
        let tab = egui::Rect::from_min_size(
            egui::pos2(rect.left() + width * index as f32, rect.top()),
            egui::vec2(width, rect.height()),
        );
        let response = ui
            .interact(
                tab.shrink2(egui::vec2(1.0, 0.0)),
                ui.id().with(("haze_page", index)),
                egui::Sense::click(),
            )
            .affords(Affords::Press);
        if response.clicked() {
            *page = index;
        }
        let live = index == *page;
        if live {
            ui.painter()
                .rect_filled(tab.shrink2(egui::vec2(1.0, 0.0)), 0.0, theme.role_shape_dim);
        } else if response.hovered() {
            ui.painter()
                .rect_filled(tab.shrink2(egui::vec2(1.0, 0.0)), 0.0, theme.surface_raised);
        }
        ui.painter().text(
            tab.center(),
            egui::Align2::CENTER_CENTER,
            label.to_uppercase(),
            egui::FontId::proportional(font::MINI_LABEL),
            if live { theme.text } else { theme.text_muted },
        );
    }
}

/// The plot: whichever hero this page owns.
fn hero(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut HazeUi,
    page: usize,
    edits: &mut Vec<ParamEdit>,
) {
    let rect = ui.available_rect_before_wrap();
    if rect.width() <= 4.0 || rect.height() <= 4.0 {
        return;
    }
    // The plot's rectangle, published where anything that needs to
    // reason about this hero can read it.
    //
    // `dark_curve_panel` splits whatever height it is handed, so the
    // plot's rect is not derivable from the outside — and a pointer test
    // that guessed at it would be a test that passes by landing on the
    // wrong thing. One writer, one answer, the same rule the mixer
    // strip's layout follows.
    ui.ctx().data_mut(|data| data.insert_temp(plot_id(), rect));
    match page {
        0 => tone_hero(ui, theme, rect, state, edits),
        1 => filter_hero(ui, theme, rect, state, edits),
        2 => amp_hero(ui, theme, rect, state, edits),
        _ => air_hero(ui, theme, rect, state, edits),
    }
}

/// Where the last drawn hero's plot landed.
pub fn plot_id() -> egui::Id {
    egui::Id::new("haze_plot")
}

/// The amp envelope's four corners, given the plot they are drawn in.
///
/// Pure, and shared by the hero that draws them and the tests that press
/// them — one arithmetic, one answer. Each corner carries the parameter
/// it moves and whether that parameter runs across or up.
fn amp_corners(state: &HazeUi, plot: egui::Rect) -> [(egui::Pos2, u32, bool); 4] {
    let floor = plot.bottom();
    let ceiling = plot.top();
    // The four times share the width in proportion to themselves, so a
    // long release visibly OWNS the picture — which is what a long
    // release does to the sound.
    let span = (state.attack + state.decay + state.release).max(0.35);
    let a_x = plot.left() + plot.width() * (state.attack / span) * 0.75;
    let d_x = a_x + plot.width() * (state.decay / span) * 0.75;
    let s_y = floor - state.sustain * plot.height();
    let hold = d_x + plot.width() * 0.12;
    let r_x = (hold + plot.width() * (state.release / span) * 0.75).min(plot.right());
    [
        (egui::pos2(a_x, ceiling), hp::ATTACK, true),
        (egui::pos2(d_x, s_y), hp::DECAY, true),
        (egui::pos2(hold, s_y), hp::SUSTAIN, false),
        (egui::pos2(r_x, floor), hp::RELEASE, true),
    ]
}

/// The plot the amp envelope is drawn inside, given the hero's rect.
fn amp_plot(rect: egui::Rect) -> egui::Rect {
    rect.shrink2(egui::vec2(6.0, 8.0))
}

/// Emit an edit for one parameter from its current knob position.
fn emit(edits: &mut Vec<ParamEdit>, param: u32, norm: f32) {
    edits.push(ParamEdit {
        param,
        value: haze_value(param, norm),
    });
}

/// Drag a rectangle in two axes at once, each mapped to a knob.
///
/// One interaction, one id, one owner — and it returns the response so a
/// caller can paint the held state. Both axes move together because the
/// gesture is one gesture: a field, not two sliders stacked.
fn field_drag(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    x: Option<(&mut f32, f32)>,
    y: Option<(&mut f32, f32)>,
) -> (egui::Response, bool) {
    let response = ui
        .interact(rect, id, egui::Sense::click_and_drag())
        .affords(Affords::Steer);
    let mut moved = false;
    if response.dragged() {
        let delta = response.drag_delta();
        let fine = ui.input(|input| input.modifiers.shift);
        let scale = if fine { 0.2 } else { 1.0 };
        if let Some((slot, span)) = x
            && delta.x != 0.0
        {
            *slot = (*slot + delta.x / rect.width().max(1.0) * span * scale).clamp(0.0, 1.0);
            moved = true;
        }
        if let Some((slot, span)) = y
            && delta.y != 0.0
        {
            *slot = (*slot - delta.y / rect.height().max(1.0) * span * scale).clamp(0.0, 1.0);
            moved = true;
        }
    }
    (response, moved)
}

/// TONE — the stack, drawn as what it is.
///
/// Three traces, one per detuned copy, drifting apart across the plot as
/// `spread` opens: at zero they lie on each other and the picture is one
/// line, which is exactly what the sound is. `shape` bends them from a
/// triangle toward a saw, `sub` draws the octave underneath, and `drift`
/// unsteadies each trace's own line by its own amount.
fn tone_hero(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    state: &mut HazeUi,
    edits: &mut Vec<ParamEdit>,
) {
    let (_, moved) = field_drag(
        ui,
        rect,
        ui.id().with("haze_tone_field"),
        Some((&mut state.spread, 1.0)),
        Some((&mut state.shape, 1.0)),
    );
    if moved {
        emit(edits, hp::SPREAD, state.spread);
        emit(edits, hp::SHAPE, state.shape);
    }
    let painter = ui.painter_at(rect);
    let columns = (rect.width().ceil() as usize).clamp(8, 512);
    let centre = rect.center().y;

    // The sub, first and underneath: an octave down is half the
    // frequency, so it is drawn at half the cycles and dimmer.
    if state.sub > 0.001 {
        let mut points = Vec::with_capacity(columns);
        for column in 0..columns {
            let along = column as f32 / (columns - 1) as f32;
            let value = (along * core::f32::consts::TAU * 1.5).sin();
            points.push(egui::pos2(
                rect.left() + along * rect.width(),
                centre + value * rect.height() * 0.30 * state.sub,
            ));
        }
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(stroke::HAIR, theme.role_level_dim),
        ));
    }

    // The three copies. Their phase offset IS the spread: at zero they
    // are the same line, and the eye reads "one oscillator" without
    // being told.
    for copy in 0..3 {
        let offset = (copy as f32 - 1.0) * state.spread * 0.34;
        let wobble = (copy as f32 * 1.7).sin() * state.drift * 0.12;
        let mut points = Vec::with_capacity(columns);
        for column in 0..columns {
            let along = column as f32 / (columns - 1) as f32;
            let phase = (along + offset * 0.25) * core::f32::consts::TAU * 3.0;
            // Triangle toward saw: the same phase read two ways and
            // crossfaded, so the knob's middle is a real intermediate
            // shape rather than one of the two with the other faded on
            // top of it.
            let turn = phase.rem_euclid(core::f32::consts::TAU) / core::f32::consts::TAU;
            let triangle = 1.0 - (turn * 4.0 - 2.0).abs();
            let saw = turn * 2.0 - 1.0;
            let value = triangle * (1.0 - state.shape) + saw * state.shape;
            points.push(egui::pos2(
                rect.left() + along * rect.width(),
                centre + (value * 0.34 + wobble) * rect.height(),
            ));
        }
        let tint = if copy == 1 {
            theme.role_shape
        } else {
            theme.role_shape_dim
        };
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(stroke::HAIR, tint),
        ));
    }
    legend(&painter, theme, rect, "spread ↔   shape ↕");
}

/// FILTER — the response curve, with its corner as the handle.
///
/// Two curves: where the filter sits now, and where the envelope takes
/// it. The second one is the whole reason a pad has a filter envelope
/// and the hardest thing to show with a number.
fn filter_hero(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    state: &mut HazeUi,
    edits: &mut Vec<ParamEdit>,
) {
    let (_, moved) = field_drag(
        ui,
        rect,
        ui.id().with("haze_filter_field"),
        Some((&mut state.cutoff, 1.0)),
        Some((&mut state.resonance, 1.0)),
    );
    if moved {
        emit(edits, hp::CUTOFF, state.cutoff);
        emit(edits, hp::RESONANCE, state.resonance);
    }
    let painter = ui.painter_at(rect);
    let columns = (rect.width().ceil() as usize).clamp(8, 512);

    // A four-pole lowpass magnitude, drawn on the plot's own log axis.
    // Not the exact transfer function — the point is the SHAPE and where
    // the corner is, and a reader comparing this to a sweep would be
    // reading it for that.
    let curve = |corner: f32, resonance: f32, tint: egui::Color32, weight: f32| {
        let mut points = Vec::with_capacity(columns);
        for column in 0..columns {
            let along = column as f32 / (columns - 1) as f32;
            let ratio = (along - corner) * 6.0;
            let roll = 1.0 / (1.0 + (ratio.max(0.0)).powi(4)).sqrt();
            let bump = resonance * (-((along - corner) * 14.0).powi(2)).exp();
            let level = (roll + bump).clamp(0.0, 1.35);
            points.push(egui::pos2(
                rect.left() + along * rect.width(),
                rect.bottom() - level * rect.height() * 0.7,
            ));
        }
        painter.add(egui::Shape::line(points, egui::Stroke::new(weight, tint)));
    };

    // Where the envelope is taking it, drawn first and dimmer: the
    // ghost is the destination, the bright line is now.
    let swept = (state.cutoff + state.env * 0.34).clamp(0.0, 1.0);
    if (swept - state.cutoff).abs() > 0.002 {
        curve(swept, state.resonance, theme.role_time_dim, stroke::HAIR);
    }
    curve(state.cutoff, state.resonance, theme.role_time, stroke::BOLD);

    // The corner, as a mark you can see you are holding.
    let handle = egui::pos2(
        rect.left() + state.cutoff * rect.width(),
        rect.bottom() - (1.0 + state.resonance).min(1.35) * rect.height() * 0.7,
    );
    painter.circle_filled(handle, 3.0, theme.role_time);
    legend(&painter, theme, rect, "cutoff ↔   reso ↕");
}

/// AMP — the envelope, with a handle on every corner.
///
/// The one page where the picture is unarguably the control: an ADSR is
/// a shape, and four numbers describing a shape is the construction
/// problem the synth brief exists to avoid.
///
/// Each corner is its OWN interaction with its own id, so a drag that
/// began on the sustain corner stays sustain's past its neighbours and
/// past the end of its range — the contract's first rule, and the reason
/// it exists.
fn amp_hero(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    state: &mut HazeUi,
    edits: &mut Vec<ParamEdit>,
) {
    let plot = amp_plot(rect);
    let floor = plot.bottom();
    let ceiling = plot.top();
    let corners = amp_corners(state, plot);
    let a_x = corners[0].0.x;
    let d_x = corners[1].0.x;
    let s_y = corners[1].0.y;
    let hold = corners[2].0.x;
    let r_x = corners[3].0.x;

    // The body, painted before the handles so the handles win the
    // pointer where they overlap it.
    let body = vec![
        egui::pos2(plot.left(), floor),
        egui::pos2(a_x, ceiling),
        egui::pos2(d_x, s_y),
        egui::pos2(hold, s_y),
        egui::pos2(r_x, floor),
    ];
    ui.painter_at(rect).add(egui::Shape::convex_polygon(
        body.clone(),
        theme.role_level_dim.gamma_multiply(0.5),
        egui::Stroke::NONE,
    ));
    ui.painter_at(rect).add(egui::Shape::line(
        body,
        egui::Stroke::new(stroke::BOLD, theme.role_level),
    ));

    // One handle per corner, each its own interaction.
    let mut held = None;
    for (index, (at, param, horizontal)) in corners.into_iter().enumerate() {
        // A 20-point target around a 3-point mark: the guide's minimum,
        // and the difference between a control and a pixel hunt.
        let hit = egui::Rect::from_center_size(at, egui::vec2(20.0, 20.0));
        let response = ui
            .interact(
                hit,
                ui.id().with(("haze_amp_handle", index)),
                egui::Sense::click_and_drag(),
            )
            .affords(if horizontal {
                Affords::Sweep
            } else {
                Affords::Slide
            });
        if response.dragged() {
            held = Some(index);
            let delta = response.drag_delta();
            let fine = ui.input(|input| input.modifiers.shift);
            let scale = if fine { 0.2 } else { 1.0 };
            let Some(slot) = state.slot(param) else {
                continue;
            };
            let step = if horizontal {
                delta.x / plot.width().max(1.0)
            } else {
                -delta.y / plot.height().max(1.0)
            };
            *slot = (*slot + step * scale).clamp(0.0, 1.0);
            let norm = *slot;
            emit(edits, param, norm);
        }
        let lit = response.dragged() || response.hovered();
        ui.painter_at(rect).circle_filled(
            at,
            if lit { 4.0 } else { 3.0 },
            if lit { theme.text } else { theme.role_level },
        );
    }

    // THE SWEEP, and only while a handle is held. Motion where a gesture
    // is happening: the shape being dialled is a shape in TIME, and the
    // one thing a still picture cannot say is how long it takes.
    if held.is_some() {
        ui.ctx().request_repaint();
        let phase = (ui.input(|input| input.time) as f32 * 0.45).fract();
        let x = plot.left() + phase * (r_x - plot.left());
        ui.painter_at(rect).line_segment(
            [egui::pos2(x, ceiling), egui::pos2(x, floor)],
            egui::Stroke::new(stroke::HAIR, theme.playhead),
        );
    }
    legend(
        &ui.painter_at(rect),
        theme,
        rect,
        "drag the corners   shift for fine",
    );
}

/// AIR — the texture, drawn as what it does to a signal.
///
/// A band of the output: split by `ens`, wobbling with `wow`, stepped by
/// `grain` and squashed by `warmth`. Four knobs whose only honest
/// description is what they do to a waveform, so the waveform is the
/// description.
fn air_hero(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    state: &mut HazeUi,
    edits: &mut Vec<ParamEdit>,
) {
    let (_, moved) = field_drag(
        ui,
        rect,
        ui.id().with("haze_air_field"),
        Some((&mut state.ensemble, 1.0)),
        Some((&mut state.grain, 1.0)),
    );
    if moved {
        emit(edits, hp::ENSEMBLE, state.ensemble);
        emit(edits, hp::GRAIN, state.grain);
    }
    let painter = ui.painter_at(rect);
    let columns = (rect.width().ceil() as usize).clamp(8, 512);
    let centre = rect.center().y;

    for (channel, side) in [(0usize, -1.0f32), (1, 1.0)] {
        let mut points = Vec::with_capacity(columns);
        for column in 0..columns {
            let along = column as f32 / (columns - 1) as f32;
            // The two channels drift apart with the ensemble: same
            // signal, different phase, which is what three modulated
            // taps panned wide actually produce.
            let phase = (along + side * state.ensemble * 0.08) * core::f32::consts::TAU * 2.5;
            let mut value = phase.sin();
            // Wow bends the time axis — a pitch that is not quite steady
            // — so it is drawn as the wave stretching, not as an offset.
            if state.wow > 0.0 {
                value =
                    (phase + (along * core::f32::consts::TAU * 0.7).sin() * state.wow * 1.4).sin();
            }
            // Grain quantises the level into visible steps, which is
            // exactly what bit reduction is.
            if state.grain > 0.0 {
                let steps = (2.0 + (1.0 - state.grain) * 30.0).round().max(2.0);
                value = (value * steps).round() / steps;
            }
            // Warmth squashes the peaks toward the rails.
            if state.warmth > 0.0 {
                value = value.tanh() * (1.0 - state.warmth) + value.tanh() * 1.15 * state.warmth;
            }
            points.push(egui::pos2(
                rect.left() + along * rect.width(),
                centre + side * 0.18 * rect.height() + value * rect.height() * 0.22,
            ));
        }
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(
                stroke::HAIR,
                if channel == 0 {
                    theme.role_mod
                } else {
                    theme.role_mod_dim
                },
            ),
        ));
    }
    legend(&painter, theme, rect, "ens ↔   grain ↕");
}

/// The one line on a hero that says what the hero does.
///
/// Bottom-left, every page, same place — the guide's density rule is
/// that positions are invariant, and a legend that moved would be one
/// more thing to find.
fn legend(painter: &egui::Painter, theme: &Theme, rect: egui::Rect, text: &str) {
    painter.text(
        egui::pos2(rect.left() + 4.0, rect.bottom() - 2.0),
        egui::Align2::LEFT_BOTTOM,
        text,
        egui::FontId::proportional(font::MINI_LABEL),
        theme.text_muted.gamma_multiply(0.7),
    );
}

/// The value strip for whichever page is open.
fn footer(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut HazeUi,
    page: usize,
    edits: &mut Vec<ParamEdit>,
) {
    let Some((_, row)) = PAGES.get(page.min(PAGES.len() - 1)) else {
        return;
    };
    let height = ui.available_height().max(1.0);
    ui.horizontal(|ui| {
        let gap_x = ui.spacing().item_spacing.x;
        for (drawn, param) in row.iter().enumerate() {
            let spec = param_of(*param);
            // The share is recomputed from what is ACTUALLY left, cell
            // by cell — worked out in advance, any cell needing more
            // than its share spends the row's remainder and the last one
            // is pushed off the card's right edge.
            let left = (row.len() - drawn) as f32;
            let room = ui.available_width() - gap_x * (left - 1.0).max(0.0);
            let width = (room / left).floor().max(1.0);
            let Some(norm) = state.slot(*param) else {
                continue;
            };
            ui.allocate_ui_with_layout(
                egui::vec2(width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(width);
                    ui.set_height(height);
                    if poly_widgets::labeled_cell_bar(ui, theme, &spec, norm, None) {
                        let value = *norm;
                        emit(edits, *param, value);
                    }
                },
            );
        }
    });
    let _ = design::screen_radius();
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::ui::device::probe;

    fn view() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(520.0, 260.0))
    }

    fn draw(page: usize, path: &[probe::Step]) -> (Vec<Vec<ParamEdit>>, HazeUi) {
        let theme = Theme::dark();
        let context = egui::Context::default();
        let mut state = HazeUi::default();
        let mut page = page;
        let frames = probe::run(&context, view(), path, |ui| {
            haze_card(ui, &theme, &mut state, &mut page)
        });
        (frames, state)
    }

    fn flat(frames: &[Vec<ParamEdit>]) -> Vec<ParamEdit> {
        frames.iter().flatten().cloned().collect()
    }

    /// Where a control IS, found rather than assumed.
    ///
    /// The card's regions come out of `dark_curve_panel`, which splits
    /// whatever height it is given — so a test that hardcodes a y is a
    /// test that breaks when the card's chrome changes by a point, and
    /// worse, one that can pass by landing on the wrong thing. Sweeping
    /// asks the card where its controls are, which is the only question
    /// the test actually has.
    fn sweep_drag(page: usize, want: u32, to: egui::Vec2) -> Option<(egui::Pos2, Vec<ParamEdit>)> {
        let mut y = 4.0;
        while y < view().height() {
            let from = egui::pos2(view().width() * 0.5, y);
            let (frames, _) = draw(page, &probe::drag_path(from, from + to, 6));
            let edits = flat(&frames);
            if edits.iter().any(|edit| edit.param == want) {
                return Some((from, edits));
            }
            y += 4.0;
        }
        None
    }

    /// EVERY ROW ROUND-TRIPS between engine units and knob positions.
    #[test]
    fn value_and_norm_round_trip() {
        for def in hp::TABLE {
            for value in [def.min, def.default, def.max, (def.min + def.max) * 0.5] {
                let back = haze_value(def.id, haze_norm(def.id, value));
                let tolerance = (def.max - def.min).abs().max(1.0) * 1e-3;
                assert!(
                    (back - value).abs() <= tolerance,
                    "{} went {value} -> {back}",
                    def.name
                );
            }
        }
    }

    /// EVERY POSITION IS A LEGAL ENGINE VALUE, and the ends reach.
    #[test]
    fn every_position_is_a_legal_engine_value() {
        for def in hp::TABLE {
            for step in 0..=20 {
                let value = haze_value(def.id, step as f32 / 20.0);
                assert!(
                    value >= def.min - 1e-4 && value <= def.max + 1e-4,
                    "{} left its range at {step}: {value}",
                    def.name
                );
            }
            assert!((haze_value(def.id, 0.0) - def.min).abs() <= (def.max - def.min) * 1e-3);
            assert!((haze_value(def.id, 1.0) - def.max).abs() <= (def.max - def.min) * 1e-3);
        }
    }

    #[test]
    fn defaults_come_from_the_table() {
        let state = HazeUi::default();
        for def in hp::TABLE {
            let value = haze_value(def.id, state.get(def.id));
            let tolerance = (def.max - def.min).abs().max(1.0) * 1e-3;
            assert!(
                (value - def.default).abs() <= tolerance,
                "{} defaulted to {value}, not {}",
                def.name,
                def.default
            );
        }
    }

    /// THE CARD COVERS THE TABLE EXACTLY: every row is somewhere, and
    /// nothing is claimed twice.
    #[test]
    fn every_table_row_is_on_exactly_one_page() {
        let mut seen: Vec<u32> = PAGES
            .iter()
            .flat_map(|(_, row)| row.iter().copied())
            .collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "a parameter is on two pages");
        let mut table: Vec<u32> = hp::TABLE.iter().map(|def| def.id).collect();
        table.sort_unstable();
        assert_eq!(seen, table, "the pages and the table disagree");
        let mut cells = CELLS.to_vec();
        cells.sort_unstable();
        assert_eq!(cells, table, "CELLS and the table disagree");
    }

    /// A CARD THAT EMITS ON ITS FIRST FRAME writes its own defaults over
    /// a loaded patch.
    #[test]
    fn drawing_at_rest_emits_nothing() {
        for page in 0..PAGES.len() {
            let (frames, _) = draw(page, &[probe::Step::moved(egui::pos2(-40.0, -40.0))]);
            assert!(
                flat(&frames).is_empty(),
                "page {page} emitted while nobody touched it"
            );
        }
    }

    /// A HERO IS A CONTROL, on every page — the whole claim of this card.
    #[test]
    fn every_hero_is_draggable() {
        for (page, across, down) in [
            (0, hp::SPREAD, hp::SHAPE),
            (1, hp::CUTOFF, hp::RESONANCE),
            (3, hp::ENSEMBLE, hp::GRAIN),
        ] {
            let found = sweep_drag(page, across, egui::vec2(90.0, -40.0));
            let Some((_, edits)) = found else {
                panic!("page {page}: nothing on the card moved {across}");
            };
            // Both axes at once: a field is one gesture, not two
            // sliders stacked, and the picture moves in both.
            assert!(
                edits.iter().any(|edit| edit.param == down),
                "page {page} moved {across} but not {down} — the field is one-axis"
            );
        }
    }

    /// EACH ENVELOPE HANDLE OWNS ITS OWN GESTURE.
    ///
    /// The contract's first rule, and the one the equaliser shipped
    /// wrong three times: a drag that began on the attack corner stays
    /// attack's past its neighbours and past the end of its range. Only
    /// a pointer can tell.
    /// The plot the amp page actually drew in, read back from where the
    /// card published it.
    fn amp_plot_rect() -> egui::Rect {
        let theme = Theme::dark();
        let context = egui::Context::default();
        let mut state = HazeUi::default();
        let mut page = 2;
        let mut run = context.run_ui(egui::RawInput::default(), |ui| {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(view()));
            haze_card(&mut child, &theme, &mut state, &mut page);
        });
        run.textures_delta.clear();
        let rect = context
            .data(|data| data.get_temp::<egui::Rect>(plot_id()))
            .expect("the hero publishes its plot");
        amp_plot(rect)
    }

    /// Press one envelope corner and report what moved.
    fn drag_corner(index: usize, travel: egui::Vec2) -> (Vec<ParamEdit>, HazeUi) {
        let plot = amp_plot_rect();
        let state = HazeUi::default();
        let at = amp_corners(&state, plot)[index].0;
        let (frames, moved) = draw(2, &probe::drag_path(at, at + travel, 8));
        (flat(&frames), moved)
    }

    #[test]
    fn an_envelope_handle_keeps_its_own_parameter() {
        // A long drag, far enough to cross where its neighbours sit.
        let (edits, moved) = drag_corner(0, egui::vec2(200.0, 0.0));
        assert!(
            edits.iter().any(|edit| edit.param == hp::ATTACK),
            "the attack corner was never pressed"
        );
        assert!(moved.attack > HazeUi::default().attack, "it did not grow");
        assert!(
            !edits.iter().any(|edit| edit.param == hp::DECAY),
            "the drag leaked onto a handle it crossed: {edits:?}"
        );
        assert!(
            !edits.iter().any(|edit| edit.param == hp::RELEASE),
            "and onto another one"
        );
    }

    /// EVERY ENVELOPE CORNER IS REACHABLE, each by its own gesture.
    #[test]
    fn every_envelope_corner_answers() {
        for (index, travel) in [
            (0, egui::vec2(40.0, 0.0)),
            (1, egui::vec2(40.0, 0.0)),
            (2, egui::vec2(0.0, -40.0)),
            (3, egui::vec2(40.0, 0.0)),
        ] {
            let plot = amp_plot_rect();
            let expected = amp_corners(&HazeUi::default(), plot)[index].1;
            let (edits, _) = drag_corner(index, travel);
            assert!(
                edits.iter().any(|edit| edit.param == expected),
                "corner {index} does not move {expected}"
            );
        }
    }

    /// A PRESS ON EMPTY GROUND MOVES NOTHING.
    #[test]
    fn a_press_beside_the_card_moves_nothing() {
        for page in 0..PAGES.len() {
            let (frames, state) = draw(page, &probe::click_path(egui::pos2(-30.0, -30.0)));
            assert!(flat(&frames).is_empty(), "page {page}");
            assert_eq!(state, HazeUi::default(), "page {page} changed its state");
        }
    }

    /// THE RAIL SWITCHES PAGES, and it is the rail that does it — a
    /// press on the hero must not change page.
    #[test]
    fn the_rail_switches_pages_and_nothing_else_does() {
        let theme = Theme::dark();
        let context = egui::Context::default();
        let mut state = HazeUi::default();
        let mut page = 0;
        // The rail is somewhere near the top; found, not assumed.
        let mut y = 2.0;
        while y < view().height() && page == 0 {
            let at = egui::pos2(view().width() * 0.62, y);
            probe::run(&context, view(), &probe::click_path(at), |ui| {
                haze_card(ui, &theme, &mut state, &mut page)
            });
            y += 3.0;
        }
        assert_ne!(page, 0, "nothing on the card switches pages");
        assert!(y < view().height() * 0.5, "the rail is not near the top");

        let mut page = 1;
        probe::run(
            &context,
            view(),
            &probe::click_path(egui::pos2(200.0, 120.0)),
            |ui| haze_card(ui, &theme, &mut state, &mut page),
        );
        assert_eq!(page, 1, "pressing the hero changed the page");
    }

    /// THE SILHOUETTE DOES NOT CHANGE WITH THE PAGE.
    ///
    /// The guide's rule about layout motion: switching a tab may replace
    /// what is inside a reserved rectangle and may not move its
    /// neighbours. The card reserves for its WIDEST page, so the widest
    /// one has to be what `face_width` measures.
    #[test]
    fn the_card_reserves_for_its_widest_page() {
        let theme = Theme::dark();
        let context = egui::Context::default();
        let mut widths = Vec::new();
        let mut declared = 0.0;
        let mut run = context.run_ui(egui::RawInput::default(), |ui| {
            declared = face_width(ui, &theme);
            for (_, row) in PAGES {
                let cells: f32 = row
                    .iter()
                    .map(|id| cell_min_width(ui, &theme, &param_of(*id)))
                    .sum();
                widths.push(cells + ui.spacing().item_spacing.x * (row.len() - 1) as f32);
            }
        });
        run.textures_delta.clear();
        let widest = widths.iter().fold(0.0f32, |m, w| m.max(*w));
        assert!(
            declared >= widest - 0.01,
            "the card reserves {declared} and its widest page needs {widest}"
        );
        assert!(widths.iter().all(|w| *w > 0.0));
    }

    /// THE FOOTER RESERVES TWO LINES FOR ITS ROW OF CELLS, and the hero
    /// keeps the rest.
    ///
    /// The layout note's own arithmetic, restated: a labelled cell draws
    /// its value and its name on two lines, and a row given one unit
    /// prints them through each other.
    #[test]
    fn the_footer_reserves_two_lines_and_the_hero_keeps_the_height() {
        let theme = Theme::dark();
        assert_eq!(CELL_UNITS, 2, "a value and its name are two lines");
        assert_eq!(FOOTER_ROWS, CELL_UNITS);
        let unit = theme.sp(control::POLY_CELL_H);
        let gap = theme.sp(space::XXS);
        let footer = unit * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS - 1) as f32;
        let header = unit * HEADER_ROWS as f32;
        let plot = theme.sp(control::DEVICE_TALL_H) - footer - header - gap * 2.0;
        assert!(
            plot > unit * 3.0,
            "the hero is down to {plot} points — a line of pixels, not a picture"
        );
    }
}
