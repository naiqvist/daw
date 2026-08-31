//! Flint's device card — the transient shaper.
//!
//! Ids, ranges and defaults come from [`crate::params::flint`] — the one
//! table this widget, `Node::Flint`'s core and the app's edit routing all
//! read.
//!
//! # The anatomy
//!
//! ```text
//! ┌ flint ─────────────────────────────────────────────┐
//! │ ┌ screen ────────────────────────────────────────┐ │
//! │ │ strike +4.5   body -1.5   split 12ms      ▓▓░  │ │  live weight
//! │ │      ╱▔▔╲                                      │ │  what it becomes
//! │ │    ╱█████▓▓▒▒░░░░________                      │ │  strike ▓ body ░
//! │ ├────────────────────────────────────────────────┤ │
//! │ │ +4.5dB  -1.5dB  12 ms   65%    100%   0.0dB    │ │  value
//! │ │ STRIKE   BODY   SPLIT  COLOUR   MIX    OUT     │ │  name
//! │ └────────────────────────────────────────────────┘ │
//! └────────────────────────────────────────────────────┘
//! ```
//!
//! # The plot ASKS the kernel
//!
//! The hero is a reference hit — instant onset, exponential decay — with
//! the strike half filled in one colour and the body half in another.
//! The boundary between them is not drawn from a formula written twice:
//! it is [`TransientSplit`](crate::dsp::dynamics::TransientSplit), the
//! same kernel the audio thread runs, driven over the reference envelope
//! at whatever SPLIT currently says. Move the knob and the colours move
//! because the detector moved.
//!
//! That matters more here than on most cards. "How much of this counts as
//! the attack" is the one question the device answers, and a picture that
//! approximated it would be a second opinion about the only thing the
//! user is trying to see. It is also self-checking: if the kernel and the
//! picture ever disagree, there is nowhere for the disagreement to hide.
//!
//! The line ON TOP is the shaped result — the same envelope after STRIKE
//! and BODY have been applied — so the card shows the before and the
//! after in one figure rather than asking anyone to imagine the after.
//!
//! # Colour is the vocabulary, not decoration
//!
//! Strike takes `role_mod` and body takes `role_time`, which is the
//! theme's own split between "the destructive edge" and "what persists".
//! The two halves of this device are exactly those two things, so the
//! card borrows the words rather than inventing a palette.

use crate::dsp::dynamics::TransientSplit;
use crate::params::flint as fp;
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{Footprint, Param, Well, Wells, card, metrics, poly_widgets};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

/// The cells, in the order they read: what the device does to each half
/// of a hit, where it cuts between them, how much character it adds,
/// then how much of it you hear and how loud.
const CELLS: [u32; 6] = [
    fp::STRIKE,
    fp::BODY,
    fp::SPLIT,
    fp::COLOUR,
    fp::MIX,
    fp::OUT,
];

/// One row of cells, and a labelled cell is two `POLY_CELL_H` units —
/// see `notes/20260827-device-card-layout.md`.
const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = CELL_UNITS;

/// The reference hit the plot draws, in milliseconds.
const PREVIEW_MS: f32 = 140.0;
/// How many points the preview is sampled at. Enough for a smooth fill
/// at any card width, cheap enough to rebuild every frame.
const PREVIEW_POINTS: usize = 220;
/// The reference hit's decay, in milliseconds. A snare, roughly.
const PREVIEW_DECAY_MS: f32 = 45.0;
/// The least headroom the plot ever shows above unity, as a multiple.
/// The plot scales to whatever the shaped envelope reaches, but at rest
/// there is nothing to scale to, and a trace pinned to the top edge
/// reads as clipped rather than as flat.
pub const PLOT_FLOOR_CEILING: f32 = 1.15;
/// The decibel guides, in order. Only those inside the current scale are
/// drawn — a ladder that runs off the top is a ladder nobody can read.
const DB_GUIDES: [f32; 5] = [0.0, 3.0, 6.0, 12.0, 18.0];
/// The rate the preview is computed at. Not the engine's — the picture
/// is a shape, not a signal, and it must look the same at every rate.
const PREVIEW_RATE: f32 = 48_000.0;

/// Knob positions of one shaper, normalized. Serialized into project
/// files, so they survive a reload.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FlintUi {
    pub strike: f32,
    pub body: f32,
    pub split: f32,
    pub colour: f32,
    pub mix: f32,
    pub out: f32,
}

impl Default for FlintUi {
    fn default() -> Self {
        // Every default is the TABLE's, run backwards through the same
        // mapping the controls use.
        let at = |id: u32| flint_norm(id, params::def(fp::TABLE, id).default);
        Self {
            strike: at(fp::STRIKE),
            body: at(fp::BODY),
            split: at(fp::SPLIT),
            colour: at(fp::COLOUR),
            mix: at(fp::MIX),
            out: at(fp::OUT),
        }
    }
}

impl FlintUi {
    /// Reflect a patch's engine units onto the card's knobs.
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| flint_norm(id, get(id));
        Self {
            strike: at(fp::STRIKE),
            body: at(fp::BODY),
            split: at(fp::SPLIT),
            colour: at(fp::COLOUR),
            mix: at(fp::MIX),
            out: at(fp::OUT),
        }
    }

    /// This state's knob position for a wire id, mutably. One place an id
    /// becomes a field, so a loop over the table cannot route an edit
    /// into the wrong control.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            fp::STRIKE => &mut self.strike,
            fp::BODY => &mut self.body,
            fp::SPLIT => &mut self.split,
            fp::COLOUR => &mut self.colour,
            fp::MIX => &mut self.mix,
            fp::OUT => &mut self.out,
            _ => return None,
        })
    }

    /// Put a control at a normalized position by wire id — what the app
    /// uses to reflect a loaded patch onto the card.
    pub fn set_norm(&mut self, param: u32, norm: f32) {
        if let Some(slot) = self.slot(param) {
            *slot = norm.clamp(0.0, 1.0);
        }
    }
}

/// One control by wire id — the single place an id becomes a [`Param`].
fn param_of(id: u32) -> Param {
    let def = params::def(fp::TABLE, id);
    let with = |p: Param| p.with_default(def.default);
    match id {
        // BIPOLAR, both of them: zero is the middle and means "leave this
        // half alone", which is the fact the whole device rests on. A
        // unipolar bar would put "do nothing" at the far left and hide it.
        fp::STRIKE => with(Param::db("strike", def.min, def.max).bipolar()),
        fp::BODY => with(Param::db("body", def.min, def.max).bipolar()),
        fp::SPLIT => with(Param::ms("split", def.min, def.max)),
        fp::COLOUR => with(Param::percent("colour")),
        fp::MIX => with(Param::percent("mix")),
        _ => with(Param::db("out", fp::OUT_MIN_DB, fp::OUT_MAX_DB)),
    }
}

/// What the WIDGET shows for an engine-facing value — half of the one
/// mapping this card owns.
///
/// Two rows need it. COLOUR and MIX are fractions to the engine and
/// PERCENTAGES on the face, because "65 %" is what a person reads and
/// `0.65` is what a multiply wants. OUT is linear gain in the table and
/// decibels on the knob, for the reason `lofi::natural` gives: the two
/// ends of one range written down twice is how they drift apart.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        fp::COLOUR | fp::MIX => value * 100.0,
        fp::OUT => crate::dsp::arith::gain_to_db(value.max(1e-6)),
        _ => value,
    }
}

/// What the ENGINE receives for a value the widget shows: the inverse of
/// [`shown`], clamped through the row.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        fp::COLOUR | fp::MIX => value / 100.0,
        fp::OUT => crate::dsp::arith::db_to_gain(value),
        _ => value,
    };
    params::def(fp::TABLE, param).clamp(raw)
}

/// The engine-facing value at a normalized position, by param id.
pub fn flint_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`flint_value`], for a state stored in engine units.
pub fn flint_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Nothing here snaps: every row is a sweep.
pub fn flint_is_discrete(_param: u32) -> bool {
    false
}

/// Every parameter as an edit, whether or not it moved.
pub fn flint_edits(state: &FlintUi) -> Vec<ParamEdit> {
    let mut state = *state;
    fp::TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: flint_value(def.id, *norm),
            })
        })
        .collect()
}

/// The narrowest a cell may be drawn: room for the widest thing it will
/// ever print, and nothing over. XS either side, not SM — see the layout
/// note; six cells at eight points a side is ninety-six points of
/// nothing, and a card beside a browser has none to spare.
fn cell_min_width(ui: &egui::Ui, theme: &Theme, p: &Param) -> f32 {
    let value = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &p.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

/// The width the whole face needs: its six cells at their narrowest,
/// which is what the card must have before it starts clipping controls
/// away. Given more, the cells share it.
pub fn face_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    CELLS
        .iter()
        .map(|id| cell_min_width(ui, theme, &param_of(*id)))
        .sum::<f32>()
        + ui.spacing().item_spacing.x * (CELLS.len() - 1) as f32
}

/// The card's layout: one well, and the screen fills it.
fn face(ui: &egui::Ui, theme: &Theme) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()])
}

/// One point of the preview: where the reference envelope is, how much
/// of it the detector calls strike, and where the shaped envelope lands.
#[derive(Debug, Clone, Copy)]
pub struct PreviewPoint {
    pub envelope: f32,
    pub weight: f32,
    pub shaped: f32,
}

/// Run the REAL detector over a reference hit.
///
/// Green zone, rebuilt per frame: a couple of hundred samples through two
/// one-poles is nothing beside the text this card lays out, and the
/// alternative is a second implementation of the one thing this device
/// does. See the module header.
pub fn preview(split_ms: f32, strike_db: f32, body_db: f32) -> Vec<PreviewPoint> {
    let total = (PREVIEW_MS * 1e-3 * PREVIEW_RATE) as usize;
    let step = (total / PREVIEW_POINTS).max(1);
    let tau = PREVIEW_DECAY_MS * 1e-3 * PREVIEW_RATE;

    let mut split = TransientSplit::new();
    split.prepare(PREVIEW_RATE, split_ms);

    let mut out = Vec::with_capacity(PREVIEW_POINTS + 1);
    let mut weight = [0.0f32; 1];
    for i in 0..total {
        // The reference hit: instant onset, exponential decay. Fed as a
        // rectified envelope, which is what the detector sees anyway.
        let env = (-(i as f32) / tau).exp();
        split.process(&[env], &mut weight);
        if i % step == 0 {
            let w = weight.first().copied().unwrap_or(0.0);
            let gain = crate::dsp::arith::db_to_gain(strike_db * w + body_db * (1.0 - w));
            out.push(PreviewPoint {
                envelope: env,
                weight: w,
                shaped: env * gain,
            });
        }
    }
    out
}

/// Where the strike ACTUALLY ends, in milliseconds — the first moment
/// the detector calls a sample more body than strike.
///
/// Derived from the preview rather than read off the knob, and they are
/// not the same number: SPLIT sets the slow envelope's attack, and where
/// the weight then crosses a half depends on the shape of the hit as well.
/// The knob is the setting; this is the consequence, which is what a
/// person actually wants to see marked on a picture of a drum.
pub fn crossover_ms(points: &[PreviewPoint]) -> Option<f32> {
    let n = points.len();
    if n < 2 {
        return None;
    }
    let hit = points.iter().position(|p| p.weight < 0.5)?;
    Some(PREVIEW_MS * hit as f32 / (n - 1) as f32)
}

/// The most the shaping LIFTS and the most it CUTS anywhere on the
/// reference hit, in dB.
///
/// Two numbers rather than one. A single "peak change" picked whichever
/// was larger in magnitude, so a card set to +18 strike and -14 body
/// reported `-14.0` beside a strike cell reading `+18.0`, which is true
/// and useless. They are also not the knobs: the weight never quite
/// reaches 1 or 0, so what is actually applied at the peak is a little
/// less than what was asked for, and that gap is the thing worth seeing.
pub fn lift_and_cut_db(points: &[PreviewPoint]) -> (f32, f32) {
    let mut lift = 0.0f32;
    let mut cut = 0.0f32;
    for p in points.iter().filter(|p| p.envelope > 1e-4) {
        let db = crate::dsp::arith::gain_to_db(p.shaped / p.envelope);
        lift = lift.max(db);
        cut = cut.min(db);
    }
    (lift, cut)
}

/// What the plot's corner prints: the two amounts and where the cut is.
fn tag(state: &FlintUi) -> String {
    let points = preview(
        flint_value(fp::SPLIT, state.split),
        flint_value(fp::STRIKE, state.strike),
        flint_value(fp::BODY, state.body),
    );
    let (lift, cut) = lift_and_cut_db(&points);
    format!(
        "LIFT {lift:>+5.1}   CUT {cut:>+5.1}   XOVER {:>4.0} ms",
        crossover_ms(&points).unwrap_or(0.0),
    )
}

/// Draw the transient shaper's card. Returns the edits the user made.
///
/// `weight` is the engine's live strike reading, `0..=1` — the card is
/// rebuilt from engine units every frame, so it is the CALLER's, like
/// every other piece of state with no knob attached. See
/// `notes/20260826-device-ui-contract.md`.
pub fn flint_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut FlintUi,
    weight: f32,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = face(ui, theme);

    card::card_sized(ui, theme, "flint", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => {
                        plot(ui, theme, state, weight);
                    }
                    poly_widgets::CurveRegion::Footer => {
                        footer(ui, theme, state, &mut edits);
                    }
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The hero: a reference hit, split into its two halves by the actual
/// detector, with the shaped result drawn over it and the whole thing
/// measured.
///
/// Every mark here answers a question the card could not otherwise
/// answer, which is the rule the rest of the app's angular furniture is
/// held to:
///
/// - the **millisecond ruler** exists because SPLIT is set in
///   milliseconds and nothing on the plot said where one was;
/// - the **crossover marker** is where the strike actually ends, which
///   is a consequence of the knob rather than the knob's own number;
/// - the **decibel scale** turns "the line went up a bit" into a
///   reading;
/// - the **comb** between the two curves has one tick per sampled
///   column and each tick's LENGTH is the gain change at that instant,
///   so the work the device is doing is a shape rather than an
///   inference;
/// - the **corner brackets** mark the measured region, in the same
///   vocabulary [`hud`](crate::ui::hud) uses everywhere else.
fn plot(ui: &mut egui::Ui, theme: &Theme, state: &FlintUi, weight: f32) {
    let rect = ui.available_rect_before_wrap();
    let (rect, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
    if rect.width() < 4.0 || rect.height() < 4.0 {
        return;
    }
    let painter = ui.painter_at(rect);

    let points = preview(
        flint_value(fp::SPLIT, state.split),
        flint_value(fp::STRIKE, state.strike),
        flint_value(fp::BODY, state.body),
    );
    if points.is_empty() {
        return;
    }

    let pad = theme.sp(space::XS);
    // The ruler stands below the trace, and the dB labels beside it, so
    // neither prints through the picture. Its height is the tick plus
    // the label plus a gap — measured, not guessed: reserving `SM` cut
    // every number on the ruler in half.
    let ruler_h = 5.0 + font::MICRO_LABEL + 2.0;
    let scale_w = theme.sp(space::LG);
    let plot = egui::Rect::from_min_max(
        // Clear of the header line above: the readings are drawn at the
        // card's top edge, and the bracket arm was landing on them.
        rect.min + egui::vec2(pad, font::MICRO_LABEL + pad),
        rect.max - egui::vec2(pad + scale_w, pad + ruler_h),
    );
    if plot.width() < 8.0 || plot.height() < 8.0 {
        return;
    }
    // The scale follows the trace. A fixed ceiling clipped the shaped
    // line flat along the top edge the moment STRIKE went past +6 dB,
    // which reads as a broken plot rather than a loud one.
    let ceiling = points
        .iter()
        .fold(0.0f32, |m, p| m.max(p.shaped))
        .max(PLOT_FLOOR_CEILING)
        * 1.06;
    let at = |i: usize, v: f32| {
        let x = plot.left() + plot.width() * (i as f32 / (points.len() - 1).max(1) as f32);
        let y = plot.bottom() - plot.height() * (v / ceiling).clamp(0.0, 1.0);
        egui::pos2(x, y)
    };
    let y_of = |v: f32| plot.bottom() - plot.height() * (v / ceiling).clamp(0.0, 1.0);
    let x_of_ms = |ms: f32| plot.left() + plot.width() * (ms / PREVIEW_MS).clamp(0.0, 1.0);

    let hair = egui::Stroke::new(1.0, theme.divider);
    let mini = egui::FontId::monospace(font::MICRO_LABEL);

    // ---- the decibel scale, down the right edge ---------------------
    // Top down, skipping any rung whose label would land on the one
    // above it. The ladder is unevenly spaced by construction — the
    // axis is linear gain and the rungs are decibels — so at a wide
    // scale the bottom three arrive on top of each other.
    let mut last_label_y = f32::NEG_INFINITY;
    for db in DB_GUIDES.iter().rev().copied() {
        let gain = crate::dsp::arith::db_to_gain(db);
        if gain > ceiling {
            continue;
        }
        let label = if db == 0.0 {
            " 0".to_string()
        } else {
            format!("+{db:.0}")
        };
        let y = y_of(gain);
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            egui::Stroke::new(
                1.0,
                if db == 0.0 {
                    theme.outline
                } else {
                    theme.divider
                },
            ),
        );
        // The RULE is always drawn — it is the reading. Only the label
        // is dropped when there is no room for it.
        if (y - last_label_y).abs() >= font::MICRO_LABEL {
            painter.text(
                egui::pos2(plot.right() + 3.0, y),
                egui::Align2::LEFT_CENTER,
                &label,
                mini.clone(),
                theme.text_muted,
            );
            last_label_y = y;
        }
    }

    // ---- the DRY envelope, coloured by what the detector called it ---
    for (i, p) in points.iter().enumerate() {
        if i + 1 >= points.len() {
            break;
        }
        let colour = blend(theme.role_time_dim, theme.role_mod, p.weight);
        let a = at(i, p.envelope);
        let b = at(i + 1, points[i + 1].envelope);
        painter.add(egui::Shape::convex_polygon(
            vec![
                a,
                b,
                egui::pos2(b.x, plot.bottom()),
                egui::pos2(a.x, plot.bottom()),
            ],
            colour,
            egui::Stroke::NONE,
        ));
    }

    // ---- the comb: one tick per column, length = the gain change -----
    //
    // Drawn UNDER the shaped line so the line stays the clean edge and
    // the comb reads as measurement against it.
    for (i, p) in points.iter().enumerate().step_by(4) {
        let (a, b) = (at(i, p.envelope), at(i, p.shaped));
        if (a.y - b.y).abs() < 1.0 {
            continue;
        }
        // ONE colour, and not either of the fill's. Red and blue already
        // mean strike and body — which PART of the hit this is — so a
        // comb that also went red for "lifting" would be the same ink
        // saying two things. `role_level` is the theme's own word for
        // level and amount, which is exactly what a tick measures, and
        // the direction is already carried by which side of the fill the
        // tick stands on.
        painter.line_segment(
            [a, b],
            egui::Stroke::new(1.0, theme.role_level.gamma_multiply(0.45)),
        );
    }

    // ---- the SHAPED envelope: what the device makes of it ------------
    let shaped: Vec<egui::Pos2> = points
        .iter()
        .enumerate()
        .map(|(i, p)| at(i, p.shaped))
        .collect();
    painter.add(egui::Shape::line(
        shaped,
        egui::Stroke::new(1.5, theme.role_shape),
    ));

    // ---- the crossover: where the strike actually ends ---------------
    if let Some(ms) = crossover_ms(&points) {
        let x = x_of_ms(ms);
        // A dashed rule rather than a solid one: it is a BOUNDARY, not a
        // signal, and the two must not read alike.
        let mut y = plot.top();
        while y < plot.bottom() {
            let end = (y + 3.0).min(plot.bottom());
            painter.line_segment(
                [egui::pos2(x, y), egui::pos2(x, end)],
                egui::Stroke::new(1.0, theme.role_mod.gamma_multiply(0.8)),
            );
            y += 6.0;
        }
        painter.text(
            egui::pos2(x + 3.0, plot.top() + 1.0),
            egui::Align2::LEFT_TOP,
            format!("{ms:.0}ms"),
            mini.clone(),
            theme.role_mod,
        );
    }

    // ---- the millisecond ruler ---------------------------------------
    painter.line_segment(
        [
            egui::pos2(plot.left(), plot.bottom()),
            egui::pos2(plot.right(), plot.bottom()),
        ],
        hair,
    );
    let mut ms = 0.0f32;
    while ms <= PREVIEW_MS {
        let x = x_of_ms(ms);
        let labelled = (ms as i32) % 40 == 0;
        let len = if labelled { 4.0 } else { 2.0 };
        painter.line_segment(
            [
                egui::pos2(x, plot.bottom()),
                egui::pos2(x, plot.bottom() + len),
            ],
            hair,
        );
        if labelled && ms > 0.0 {
            painter.text(
                egui::pos2(x, plot.bottom() + len + 1.0),
                egui::Align2::CENTER_TOP,
                format!("{ms:.0}"),
                mini.clone(),
                theme.text_muted,
            );
        }
        ms += 10.0;
    }
    painter.text(
        egui::pos2(plot.left(), plot.bottom() + 5.0),
        egui::Align2::LEFT_TOP,
        "ms",
        mini.clone(),
        theme.text_muted,
    );

    // ---- the frame, in the app's own vocabulary ----------------------
    crate::ui::hud::brackets(&painter, plot, egui::Stroke::new(1.0, theme.outline));

    // ---- the readings ------------------------------------------------
    painter.text(
        rect.min + egui::vec2(pad, 0.0),
        egui::Align2::LEFT_TOP,
        tag(state),
        mini.clone(),
        theme.text_muted,
    );
    // The live strike weight, as a bar AND as a number: the bar is read
    // at a glance while playing, the number when comparing two settings.
    let meter = egui::Rect::from_min_size(
        egui::pos2(rect.right() - pad - 40.0, rect.top() + 2.0),
        egui::vec2(40.0, 4.0),
    );
    painter.rect_filled(meter, 0.0, theme.surface_sunken);
    let lit = meter.width() * weight.clamp(0.0, 1.0);
    painter.rect_filled(
        egui::Rect::from_min_size(meter.min, egui::vec2(lit, meter.height())),
        0.0,
        theme.role_mod,
    );
    painter.text(
        egui::pos2(meter.left() - 4.0, meter.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{:>3.0}%", weight.clamp(0.0, 1.0) * 100.0),
        mini,
        theme.text_muted,
    );
}

/// Mix two colours. `t` at 0 is `a`, at 1 is `b`.
fn blend(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    egui::Color32::from_rgba_unmultiplied(
        lerp(a.r(), b.r()),
        lerp(a.g(), b.g()),
        lerp(a.b(), b.b()),
        lerp(a.a(), b.a()),
    )
}

/// One row of cells, and nothing else.
///
/// Every cell shares the width PROGRESSIVELY rather than each claiming a
/// share worked out in advance: divided up front, any cell needing more
/// than its share spends the row's remainder and the last one is pushed
/// off the right edge. `glue.rs` found this first.
fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut FlintUi, edits: &mut Vec<ParamEdit>) {
    let gap = theme.sp(space::XXS);
    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
    let height = ui.available_height().max(1.0);

    ui.horizontal(|ui| {
        for (drawn, id) in CELLS.iter().enumerate() {
            let left = (CELLS.len() - drawn) as f32;
            let room = ui.available_width() - gap * (left - 1.0).max(0.0);
            let width = (room / left).floor().max(1.0);
            let p = param_of(*id);
            let Some(slot) = state.slot(*id) else {
                continue;
            };
            let before = *slot;
            ui.allocate_ui_with_layout(
                egui::vec2(width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    poly_widgets::labeled_cell_bar(ui, theme, &p, slot, None);
                },
            );
            if *slot != before {
                edits.push(ParamEdit {
                    param: *id,
                    value: flint_value(*id, *slot),
                });
            }
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // Six cells, all of them narrow: the widest thing any of them prints
    // is "-18.0dB", so this card is comfortably inside a device panel.
    const MAX_W: f32 = 520.0;
    const MIN_W: f32 = 260.0;

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
        let edits = flint_edits(&FlintUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = fp::TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect, "the card does not cover the table");

        // And the FACE shows every one of them: a row the engine has and
        // the card does not is a parameter nobody can reach.
        let mut shown = CELLS.to_vec();
        shown.sort_unstable();
        assert_eq!(shown, expect, "the face does not cover the table");
    }

    #[test]
    fn every_position_is_a_legal_engine_value() {
        for def in fp::TABLE {
            for i in 0..=40 {
                let value = flint_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
            }
            let span = def.max - def.min;
            assert!(
                (flint_value(def.id, 0.0) - def.min).abs() < span * 1e-3,
                "{} does not reach its floor",
                def.name
            );
            assert!(
                (flint_value(def.id, 1.0) - def.max).abs() < span * 1e-3,
                "{} does not reach its ceiling",
                def.name
            );
        }
    }

    #[test]
    fn value_and_norm_round_trip() {
        for def in fp::TABLE {
            for i in 0..=40 {
                let value = flint_value(def.id, i as f32 / 40.0);
                let again = flint_value(def.id, flint_norm(def.id, value));
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
        for def in fp::TABLE {
            let mut state = FlintUi::default();
            let norm = *state.slot(def.id).unwrap();
            let value = flint_value(def.id, norm);
            assert!(
                (value - def.default).abs() < (def.max - def.min) * 1e-3,
                "{} opened at {value}, table says {}",
                def.name,
                def.default
            );
        }
    }

    /// The plot asks the KERNEL, so the picture cannot drift from the
    /// device. Two things must hold for it to be worth drawing at all:
    /// the strike is at the FRONT of the hit, and the split knob moves
    /// the boundary.
    #[test]
    fn the_preview_is_the_kernels_own_opinion() {
        let points = preview(12.0, 0.0, 0.0);
        assert!(!points.is_empty(), "the preview drew nothing");

        let head = points.get(..12).unwrap();
        let tail = points.get(points.len() - 12..).unwrap();
        let front = head.iter().fold(0.0f32, |m, p| m.max(p.weight));
        let back = tail.iter().fold(0.0f32, |m, p| m.max(p.weight));
        assert!(front > 0.5, "the strike is not at the front: {front}");
        assert!(back < 0.1, "the strike never ends: {back}");

        // A wider split holds it open longer — the knob's whole meaning.
        let lit = |ms: f32| {
            preview(ms, 0.0, 0.0)
                .iter()
                .filter(|p| p.weight > 0.25)
                .count()
        };
        assert!(lit(40.0) > lit(2.0), "the split knob does nothing");

        // With both halves at zero the shaped line IS the envelope — the
        // card must show the identity as an identity.
        for p in points.iter() {
            assert!(
                (p.shaped - p.envelope).abs() < 1e-6,
                "at rest the shaped line left the envelope"
            );
        }
    }

    /// The footer reserves two lines for its row of cells, and the plot
    /// keeps enough height to be a picture. See
    /// `notes/20260827-device-card-layout.md`.
    #[test]
    fn the_footer_reserves_two_lines_for_the_row_of_cells() {
        let theme = Theme::dark();
        let gap = theme.sp(space::XXS);
        let cell = theme.sp(crate::ui::tokens::control::POLY_CELL_H);
        let footer = cell * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS - 1) as f32;
        assert!(
            footer >= cell * CELL_UNITS as f32,
            "a labelled cell is two units and the footer reserved {footer}"
        );
        let plot = theme.sp(control::DEVICE_TALL_H) - footer;
        assert!(
            plot > cell * 3.0,
            "the plot is left {plot} pt, which is a line rather than a picture"
        );
    }

    /// Every cell can print its own widest reading inside the width the
    /// card declares — the guard against a value printing through its
    /// neighbour the moment a knob moves.
    #[test]
    fn every_cell_fits_the_width_the_card_asks_for() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        frame(&ctx, |ui| {
            let declared = face_width(ui, &theme);
            let needed: f32 = CELLS
                .iter()
                .map(|id| cell_min_width(ui, &theme, &param_of(*id)))
                .sum::<f32>()
                + ui.spacing().item_spacing.x * (CELLS.len() - 1) as f32;
            assert!(
                declared >= needed - 0.5,
                "the card asks for {declared:.0} pt and its cells need {needed:.0}"
            );
            for id in CELLS {
                let p = param_of(id);
                let w = cell_min_width(ui, &theme, &p);
                let widest = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
                assert!(
                    w >= widest,
                    "{} reserves {w:.0} pt and prints {widest:.0}",
                    p.name
                );
            }
        });
    }

    #[test]
    fn the_card_stays_inside_its_budget() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = FlintUi::default();
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| flint_card(ui, &theme, &mut state, 0.0))
                        .response
                        .rect
                })
                .inner
        });
        let (w, h) = (rect.width(), rect.height());
        assert!(w <= MAX_W, "the card is {w:.0} pt wide, over {MAX_W:.0}");
        assert!(w >= MIN_W, "the card is only {w:.0} pt wide — did it draw?");
        let budget = theme.sp(control::DEVICE_TALL_H) + crate::ui::tokens::stroke::HAIR * 2.0;
        assert!(
            h <= budget,
            "the card is {h:.0} pt tall, over its {budget:.0} pt budget"
        );
    }

    /// The card fits the narrow rectangle a device panel really hands
    /// over, and still fills it rather than shrinking into a corner.
    #[test]
    fn the_card_fits_the_panel_it_is_actually_given() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = FlintUi::default();
        // The floor is the card's OWN declared width — six cells at the
        // widest each will ever print. Below that there is nothing left
        // to give back, and the right answer is a narrower panel rather
        // than a cell that clips its value. Asking for less than the
        // floor is how this test failed the first time.
        // `face_width` is the FACE — the cells and the gaps between
        // them. The card's own frame sits OUTSIDE that, so the honest
        // floor is a few points wider; measured, it is about five. Asking
        // for exactly the face width is how this test failed the first
        // time, and the answer was not to loosen the assertion below.
        let floor = frame(&ctx, |ui| face_width(ui, &theme)) + theme.sp(space::SM) * 2.0;
        for panel_w in [floor.ceil(), floor * 1.4, floor * 2.0] {
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
                flint_card(&mut child, &theme, &mut state, 0.0);
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

    /// Drawing at rest must not move a control — a card that emits on its
    /// first frame writes its own defaults over a loaded patch.
    #[test]
    fn drawing_at_rest_emits_nothing() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = FlintUi::default();
        let before = state;
        for _ in 0..2 {
            let edits = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| flint_card(ui, &theme, &mut state, 0.0))
                    .inner
            });
            assert!(edits.is_empty(), "the card emitted {edits:?} at rest");
        }
        assert_eq!(state, before, "drawing the card moved a control");
    }

    /// POINTER: every cell is reachable, and a drag edits exactly one.
    ///
    /// The standing tests above are all about parameters and none of them
    /// presses anything — a card whose six cells all routed to the first
    /// parameter would pass every one. This one drags across the footer
    /// and demands two things: no gesture may move more than one control,
    /// and between them the gestures must reach ALL six.
    ///
    /// Deliberately geometry-independent. Working out where a cell landed
    /// means reimplementing the card's own layout inside its test, and
    /// the first version of this test did exactly that, guessed the
    /// footer's inset wrong, and reported a routing bug that was not
    /// there. What the card owes the user is that every control can be
    /// grabbed and that grabbing one does not move another; neither claim
    /// needs a coordinate.
    #[test]
    fn every_cell_is_reachable_and_a_drag_moves_only_one() {
        use crate::ui::device::probe;
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let width = frame(&ctx, |ui| face_width(ui, &theme)).ceil() * 1.2;
        let rect = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, theme.sp(control::DEVICE_TALL_H)),
        );
        let cell_h = theme.sp(crate::ui::tokens::control::POLY_CELL_H);

        let mut reached = std::collections::BTreeSet::new();
        // Sweep the footer densely enough that every cell is landed on,
        // whatever the inset turns out to be.
        for step in 0..48 {
            let mut state = FlintUi::default();
            let x = rect.left() + rect.width() * (step as f32 + 0.5) / 48.0;
            let y = rect.bottom() - cell_h * 0.75;
            let from = egui::pos2(x, y);
            // DOWNWARD, which decreases. Upward found five cells and
            // missed MIX — it opens at 100 %, so a drag that raises it
            // has nothing to raise and emits nothing. Every cell here
            // opens above its floor, so down always has somewhere to go.
            // (`sibyl`'s twin sweeps BOTH ways, because its KEY cell
            // opens at the bottom of its list and down cannot move it.)
            let path = probe::drag_path(from, from + egui::vec2(0.0, 20.0), 4);

            let edits = probe::run(&ctx, rect, &path, |ui| {
                flint_card(ui, &theme, &mut state, 0.0)
            });
            let touched: std::collections::BTreeSet<u32> =
                edits.into_iter().flatten().map(|e| e.param).collect();
            assert!(
                touched.len() <= 1,
                "one drag at x={x:.0} moved {} controls: {touched:?}",
                touched.len()
            );
            reached.extend(touched);
        }

        let expect: std::collections::BTreeSet<u32> = CELLS.iter().copied().collect();
        assert_eq!(
            reached,
            expect,
            "a sweep across the footer never reached {:?}",
            expect.difference(&reached).collect::<Vec<_>>()
        );
    }
}
