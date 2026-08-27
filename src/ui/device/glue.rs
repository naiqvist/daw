//! The bus compressor's device card.
//!
//! Ids, ranges and defaults come from [`crate::params::glue`] — the one
//! table this widget, `Node::Glue`'s core and the app's edit routing all
//! read.
//!
//! # The anatomy
//!
//! ```text
//! ┌ glue ──────────────────────────────────────────────┐  context strip
//! │ ┌ screen ────────────────────────────────────────┐ │
//! │ │ gr -4.2  4s  peak -7.1                    -6   │ │  values at the edge
//! │ │ ─────────────────────────────────────────  -12 │ │  reduction, hanging
//! │ │    ███▓▓░        ████▓▓▓░░                     │ │
//! │ ├────────────────────────────────────────────────┤ │
//! │ │ ◄╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌  0   │ │  threshold, dragged
//! │ │   ▁▂▃▅▇█▇▅▃▂▁   ▁▂▃▅▇██▇▅▃▂▁              -12  │ │  level, from the floor
//! │ ├────────────────────────────────────────────────┤ │
//! │ │ -18.0dB  4:1  10 ms  auto  +3.0dB  100% …      │ │  value
//! │ │ THRESH  RATIO ATTACK RELEASE MAKEUP  MIX  …    │ │  name
//! │ │ ▁▁▁▁▁▁  ▁█▁   ▁▁▁▁█▁▁ ▁▁▁▁▁▁█ ▁▁▁▁▁▁  ▁▁▁▁▁   │ │  bar, or detents
//! │ └────────────────────────────────────────────────┘ │
//! └────────────────────────────────────────────────────┘
//! ```
//!
//! # Why a scope and not a transfer curve
//!
//! [`scope`](crate::ui::device::scope) makes the argument at length. In
//! short: this unit's ratio is a three-position switch, so a transfer
//! curve is a picture with three states, while the two things that make
//! it worth having — program-dependent auto release, and the feedback
//! loop bending the ratio — are only visible against time.
//!
//! # Why every control is a cell
//!
//! Attack and release were briefly RAILS — seven labelled segments each,
//! side by side, showing all the positions at once. That is a fine way
//! to draw a switch and a poor way to spend a card: the two of them took
//! a whole row across the full width to say two numbers, and the hero
//! above went short by exactly that height.
//!
//! A stepped cell says the same thing in a seventh of the width. The
//! tail that a continuous cell fills as a bar becomes one DETENT per
//! choice with the current one lit, so "which of seven" is still there
//! at a glance — and it arrives in the grammar every other control on
//! the card already speaks, which is what the screen guide means by
//! density coming from repetition rather than compression.
//!
use crate::params::glue as gp;
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{
    Footprint, Mapping, Param, Unit, Well, Wells, card, metrics, poly_widgets, scope,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

/// The cells, in the order they read: what it listens for, how it
/// reacts, what it does about it, then what comes out.
///
/// ALL of them, including the three switches and the clipper. They were
/// briefly a row of rails — seven labelled segments each — which is a
/// fine way to show a switch and a poor way to spend a card: two rails
/// took a whole row across the full width to say two numbers, and the
/// hero above them went short by exactly that much. A stepped cell says
/// the same thing in a seventh of the width, and says it in the grammar
/// every other control on the card already uses.
const CELLS: [u32; 9] = [
    gp::THRESHOLD,
    gp::RATIO,
    gp::ATTACK,
    gp::RELEASE,
    gp::MAKEUP,
    gp::DRY_WET,
    gp::RANGE,
    gp::SC_HP,
    gp::CLIP,
];

/// How many of the screen's rows the footer stands in: a value over a
/// name, and nothing else. Everything the card can change lives in one
/// row of cells, so the hero keeps the rest of the height.
const FOOTER_ROWS: usize = 2;

/// Knob positions of one compressor, normalized. Serialized into project
/// files, so they survive a reload.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GlueUi {
    pub threshold: f32,
    pub ratio: f32,
    pub attack: f32,
    pub release: f32,
    pub makeup: f32,
    pub dry_wet: f32,
    pub range: f32,
    pub clip: f32,
    pub sc_hp: f32,
}

impl Default for GlueUi {
    fn default() -> Self {
        // Every default is the TABLE's, run backwards through the same
        // mapping the controls use.
        let at = |id: u32| glue_norm(id, params::def(gp::TABLE, id).default);
        Self {
            threshold: at(gp::THRESHOLD),
            ratio: at(gp::RATIO),
            attack: at(gp::ATTACK),
            release: at(gp::RELEASE),
            makeup: at(gp::MAKEUP),
            dry_wet: at(gp::DRY_WET),
            range: at(gp::RANGE),
            clip: at(gp::CLIP),
            sc_hp: at(gp::SC_HP),
        }
    }
}

impl GlueUi {
    /// This state's knob position for a wire id, mutably. One place an id
    /// becomes a field, so a loop over the table cannot route an edit
    /// into the wrong control.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            gp::THRESHOLD => &mut self.threshold,
            gp::RATIO => &mut self.ratio,
            gp::ATTACK => &mut self.attack,
            gp::RELEASE => &mut self.release,
            gp::MAKEUP => &mut self.makeup,
            gp::DRY_WET => &mut self.dry_wet,
            gp::RANGE => &mut self.range,
            gp::CLIP => &mut self.clip,
            gp::SC_HP => &mut self.sc_hp,
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
    let def = params::def(gp::TABLE, id);
    let with = |p: Param| p.with_default(def.default);
    match id {
        // The three switches and the clipper. Their VALUES carry the
        // unit, since a cell prints only the position you are on.
        gp::ATTACK => with(Param::choice("attack", gp::ATTACK_NAMES)),
        gp::RELEASE => with(Param::choice("release", gp::RELEASE_NAMES)),
        gp::RATIO => with(Param::choice("ratio", gp::RATIO_NAMES)),
        gp::CLIP => with(Param::choice("clip", &["off", "on"])),
        gp::THRESHOLD => with(Param::db("thresh", def.min, def.max)),
        gp::MAKEUP => with(Param::db("makeup", def.min, def.max).bipolar()),
        gp::DRY_WET => with(Param::percent("mix")),
        // LOG: a sidechain corner is heard in ratios, like every other
        // frequency on every other card.
        gp::SC_HP => with(Param::hz("sc hp", def.min, def.max)),
        _ => with(Param::new(
            "range",
            Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            Unit::Db,
        )),
    }
}

/// The engine-facing value at a normalized position, by param id.
pub fn glue_value(param: u32, norm: f32) -> f32 {
    params::def(gp::TABLE, param).clamp(param_of(param).value(norm))
}

/// The inverse of [`glue_value`], for a state stored in engine units.
pub fn glue_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(value)
}

/// Whether a parameter snaps to named settings rather than sweeping.
pub fn glue_is_discrete(param: u32) -> bool {
    param_of(param).choices().is_some()
}

/// Whether a parameter lives on a LOG scale — so a modulation sweep of
/// the sidechain corner moves in ratios exactly as the control does.
pub fn glue_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, whether or not it moved.
pub fn glue_edits(state: &GlueUi) -> Vec<ParamEdit> {
    let mut state = *state;
    gp::TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: glue_value(def.id, *norm),
            })
        })
        .collect()
}

/// The narrowest a cell may be drawn: room for the widest thing it will
/// ever print, and nothing over.
///
/// XS either side, not SM: nine cells at eight points a side is a
/// hundred and forty points of nothing, and this card has none to
/// spare — the cells are what decide how wide it is.
fn cell_min_width(ui: &egui::Ui, theme: &Theme, p: &Param) -> f32 {
    let value = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &p.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

/// The width the whole face needs: its nine cells at their narrowest,
/// which is what a card must have before it starts clipping controls
/// away. Given more, the cells share it.
fn face_width(ui: &egui::Ui, theme: &Theme) -> f32 {
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

/// What the scope's corner prints: what it is doing now, how long the
/// window is, and the worst it has done since the hold was cleared.
fn tag(history: &scope::History) -> String {
    let now = history.latest().reduction_db;
    let peak = history.peak_db();
    format!(
        "gr {now:>5.1}   {}s   peak {peak:>5.1}",
        scope::HISTORY / 60
    )
}

/// Draw the compressor card. Returns the edits the user just made.
///
/// `history` is the rolling readout the scope draws, and it is the
/// CALLER's — a card is rebuilt from engine units every frame, so
/// anything it kept for itself would be forgotten before the next frame
/// drew it. See `notes/20260826-device-ui-contract.md`.
pub fn glue_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut GlueUi,
    history: &scope::History,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = face(ui, theme);

    card::card_sized(ui, theme, "glue", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => {
                        let threshold = glue_value(gp::THRESHOLD, state.threshold);
                        if let Some(db) = scope::scope(ui, theme, history, threshold, &tag(history))
                        {
                            state.threshold = glue_norm(gp::THRESHOLD, db);
                            edits.push(ParamEdit {
                                param: gp::THRESHOLD,
                                value: glue_value(gp::THRESHOLD, state.threshold),
                            });
                        }
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

/// One row of cells, and nothing else.
///
/// Every cell shares the width evenly rather than each claiming what it
/// would like: nine natural widths add up to more than the device panel
/// offers, and a card that overflows clips its last control away
/// entirely — which here would be the peak clipper.
fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut GlueUi, edits: &mut Vec<ParamEdit>) {
    let h = ui.available_height();
    ui.horizontal(|ui| {
        let gap = ui.spacing().item_spacing.x;
        for (drawn, id) in CELLS.iter().enumerate() {
            let id = *id;
            let param = param_of(id);
            // The share is recomputed from what is ACTUALLY left, cell by
            // cell, rather than divided up in advance. Worked out ahead,
            // any cell that needs more than its share spends the row's
            // remainder and the last one — the clipper — is pushed off
            // the card's right edge entirely.
            let left = (CELLS.len() - drawn) as f32;
            let room = ui.available_width() - gap * (left - 1.0).max(0.0);
            let w = (room / left).floor().max(1.0);
            let Some(norm) = state.slot(id) else { continue };
            ui.allocate_ui_with_layout(
                egui::vec2(w, h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(w);
                    ui.set_height(h);
                    // A STEPPED control gets detents; a continuous one
                    // gets its bar. One grammar, two tails — see
                    // `poly_widgets::labeled_cell_steps`.
                    let moved = if param.choices().is_some() {
                        poly_widgets::labeled_cell_steps(ui, theme, &param, norm, None)
                    } else {
                        poly_widgets::labeled_cell_bar(ui, theme, &param, norm, None)
                    };
                    if moved {
                        edits.push(ParamEdit {
                            param: id,
                            value: glue_value(id, *norm),
                        });
                    }
                },
            );
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // Narrow on purpose: the two rails decide it, and they are the
    // reason the segments carry bare numbers.
    const MAX_W: f32 = 580.0;
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
        let edits = glue_edits(&GlueUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = gp::TABLE.iter().map(|p| p.id).collect();
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
        for def in gp::TABLE {
            for i in 0..=40 {
                let value = glue_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
            }
            assert!((glue_value(def.id, 0.0) - def.min).abs() < (def.max - def.min) * 1e-3);
            assert!((glue_value(def.id, 1.0) - def.max).abs() < (def.max - def.min) * 1e-3);
        }
    }

    #[test]
    fn value_and_norm_round_trip() {
        for def in gp::TABLE {
            for i in 0..=40 {
                let value = glue_value(def.id, i as f32 / 40.0);
                let again = glue_value(def.id, glue_norm(def.id, value));
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
        for def in gp::TABLE {
            let mut state = GlueUi::default();
            let norm = *state.slot(def.id).unwrap();
            let value = glue_value(def.id, norm);
            assert!(
                (value - def.default).abs() < (def.max - def.min) * 1e-3,
                "{} opened at {value}, table says {}",
                def.name,
                def.default
            );
        }
    }

    /// The stepped cells read the positions the table declares — a cell
    /// that showed one list while the engine read another would be a
    /// control that lies about what it set.
    #[test]
    fn the_stepped_cells_show_the_engine_s_own_positions() {
        for (id, names, count) in [
            (gp::ATTACK, gp::ATTACK_NAMES, gp::ATTACK_MS.len()),
            (gp::RELEASE, gp::RELEASE_NAMES, gp::RELEASE_S.len() + 1),
        ] {
            let param = param_of(id);
            assert_eq!(param.choices(), Some(count as u32), "{}", param.name);
            for (i, name) in names.iter().enumerate() {
                let norm = param.at_index(i);
                assert_eq!(&param.format(norm), name, "{} position {i}", param.name);
                // And the position the engine would read back.
                assert_eq!(glue_value(id, norm).round() as usize, i);
            }
        }
    }

    /// The card fits its silhouette. Narrow on purpose: two rails and
    /// seven cells, and nothing else competing for the width.
    #[test]
    fn the_card_stays_inside_its_budget() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = GlueUi::default();
        let history = scope::History::default();
        let rect = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| glue_card(ui, &theme, &mut state, &history))
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

    /// THE CARD FITS A NARROW PANEL. Its rails are seven segments each,
    /// and their natural width is more than a device panel offers at an
    /// ordinary window size — so the release rail ran off the right edge
    /// and took its AUTO position with it, which is the one setting the
    /// unit is famous for.
    ///
    /// The budget test above draws into a wide screen and cannot see
    /// this. This one draws into the narrow rectangle the panel really
    /// hands over.
    ///
    /// The smallest case is the card's honest floor: nine cells at the
    /// width of the widest thing each will ever print. Below that there
    /// is nothing left to give back, and the right answer would be a
    /// narrower device panel rather than a cell that clips its own
    /// value.
    #[test]
    fn the_card_fits_the_panel_it_is_actually_given() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = GlueUi::default();
        let history = scope::History::default();

        for panel_w in [560.0f32, 640.0, 900.0] {
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
                glue_card(&mut child, &theme, &mut state, &history);
                child.min_rect()
            });
            assert!(
                used.width() <= panel_w + 1.0,
                "at a {panel_w:.0} pt panel the card drew {:.0} pt wide",
                used.width()
            );
            // ...and it still fills what it was given, rather than
            // shrinking to a stripe in the corner of the panel.
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
        let mut state = GlueUi::default();
        let history = scope::History::default();
        let before = state;
        for _ in 0..2 {
            let edits = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| glue_card(ui, &theme, &mut state, &history))
                    .inner
            });
            assert!(edits.is_empty(), "the card emitted {edits:?} at rest");
        }
        assert_eq!(state, before, "drawing the card moved a control");
    }
}
