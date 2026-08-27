//! The disperser device card — the impulse response as an instrument
//! screen.
//!
//! Same shape as the lo-fi and sheen cards: normalized knob state here,
//! natural values out as [`ParamEdit`]s. Ids, ranges and defaults come
//! from [`crate::params::disperser`] — the one table this widget,
//! `Node::Disperser`'s apply arm and the app's edit routing all read.
//!
//! # The anatomy
//!
//! ```text
//! ┌ disperser ──────────────────────────────┐
//! │                              12 ms smear│
//! │  ▌▄▖▗▄▖▗▖▁▁                             │  ← hero
//! │                                         │
//! │  amount      freq        pinch          │
//! └─────────────────────────────────────────┘
//! ```
//!
//! # Why the hero is an impulse
//!
//! Because the impulse response IS the effect. A disperser has a flat
//! magnitude at every setting, so a spectrum would be a straight line at
//! every setting — the one picture guaranteed to say nothing. What it
//! does is spread a click out in time, highs first and lows trailing, and
//! that is a thing you can only see on a time axis.
//!
//! Nothing is normalised. As the smear grows the peak DROPS and the tail
//! extends, because an allpass chain moves energy around without adding
//! any — and watching the spike flatten into a chirp is watching exactly
//! that. A plot scaled to fill its lane would hide the one quantity the
//! amount knob changes.
//!
//! # The picture is the kernel, not a drawing of it
//!
//! [`trace`] runs a real [`Disperser`](crate::dsp::filters::Disperser)
//! over a real impulse, the way the lo-fi card runs its converter. There
//! is no group-delay formula in this file to fall out of step with the
//! filters that actually run.
//!
//! # Modelled on Kilohearts' Disperser
//!
//! Which is the reference everyone has heard, and whose three controls
//! map exactly onto what the kernel already takes: how many sections,
//! where they are tuned, and how tightly the phase turns there. Two
//! deliberate departures, both argued in [`crate::params::disperser`]:
//! there is no mix knob, because blending a phase-shifted copy with the
//! dry is a comb filter and would break the flat-magnitude promise; and
//! the amount starts at zero rather than one, because our kernel says
//! zero sections is a wire and an off switch you can reach is one you can
//! measure.

use crate::params::disperser::{
    AMOUNT, FREQ, FREQ_MAX_HZ, FREQ_MIN_HZ, PINCH, PINCH_MAX, PINCH_MIN, TABLE,
};
use crate::params::{self};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{
    Footprint, Mapping, Param, Unit, Well, Wells, card, metrics, poly_widgets, switch,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// Section counts, as the strip prints them.
///
/// A `Choice` rather than a number with a `Plain` unit, because sections
/// are integral and `Plain` would print `8.00`. Indexed from zero, so the
/// index IS the count and [`shown`] has nothing to convert.
const AMOUNT_NAMES: &[&str] = &[
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
    "17", "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31", "32",
];

/// Knob positions of one disperser, normalized. Serialized into project
/// files, so knob positions survive a reload.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DisperserUi {
    pub amount: f32,
    pub freq: f32,
    pub pinch: f32,
}

impl Default for DisperserUi {
    fn default() -> Self {
        Self {
            amount: disperser_norm(AMOUNT, params::def(TABLE, AMOUNT).default),
            freq: disperser_norm(FREQ, params::def(TABLE, FREQ).default),
            pinch: disperser_norm(PINCH, params::def(TABLE, PINCH).default),
        }
    }
}

impl DisperserUi {
    /// This state's knob position for a wire id, mutably.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            AMOUNT => &mut self.amount,
            FREQ => &mut self.freq,
            PINCH => &mut self.pinch,
            _ => return None,
        })
    }
}

struct Spec {
    amount: Param,
    freq: Param,
    pinch: Param,
}

/// The three controls, described once.
fn spec() -> Spec {
    let default =
        |param: Param, id: u32| param.with_default(shown(id, params::def(TABLE, id).default));
    Spec {
        amount: default(Param::choice("amount", AMOUNT_NAMES), AMOUNT),
        // LOG-mapped, by `Param::hz`. Three decades of travel, and the
        // whole gesture with this device is sweeping it.
        freq: default(Param::hz("freq", FREQ_MIN_HZ, FREQ_MAX_HZ), FREQ),
        // LOG too: a Q is a ratio, and the difference between 0.1 and 0.2
        // is the same amount of "wider" as between 4 and 8.
        pinch: default(
            Param::new(
                "pinch",
                Mapping::Log {
                    min: PINCH_MIN,
                    max: PINCH_MAX,
                },
                Unit::Plain,
            ),
            PINCH,
        ),
    }
}

/// What the widget SHOWS for an engine-facing value.
///
/// Every row here is already in a unit a musician could say out loud —
/// a count, a frequency and a Q — so unusually for one of these cards
/// this is the identity. Kept as a function anyway, so the pair with
/// [`natural`] stays visible and a future unit has one place to go.
fn shown(_param: u32, value: f32) -> f32 {
    value
}

/// What the ENGINE receives for a value the widget shows. Clamped through
/// the row, so a knob at either stop cannot emit a letter the engine has
/// to bin.
fn natural(param: u32, value: f32) -> f32 {
    params::def(TABLE, param).clamp(value)
}

/// One control by wire id.
fn param_of(param: u32) -> Param {
    let s = spec();
    match param {
        AMOUNT => s.amount,
        FREQ => s.freq,
        _ => s.pinch,
    }
}

/// The engine-facing value at a normalized knob position, by param id.
pub fn disperser_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`disperser_value`], for a state stored in engine units.
pub fn disperser_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter snaps to named settings. The section count does —
/// there is no such thing as eight and a half allpasses.
pub fn disperser_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale. The corner and the Q both
/// do, which is what makes a modulation sweep of either move in ratios
/// the way the knob does.
pub fn disperser_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, whether or not it moved.
pub fn disperser_edits(state: &DisperserUi) -> Vec<ParamEdit> {
    [
        (AMOUNT, state.amount),
        (FREQ, state.freq),
        (PINCH, state.pinch),
    ]
    .into_iter()
    .map(|(param, norm)| ParamEdit {
        param,
        value: disperser_value(param, norm),
    })
    .collect()
}

/// The reference rate the picture is drawn at. A real audio rate, not a
/// convenient small one: the corner reaches 20 kHz, and at any rate low
/// enough to plot cheaply that would be above Nyquist.
const PLOT_SR: f32 = 48_000.0;

/// How long a window the picture covers, in milliseconds.
///
/// A FIXED window, so that turning the amount up visibly lengthens the
/// smear instead of rescaling the axis under it. Long settings run off
/// the right edge on purpose — the corner tag reports the real figure,
/// and a picture that always fits is a picture that cannot show growth.
const PLOT_MS: f32 = 25.0;

/// Samples the kernel actually runs over.
const PLOT_N: usize = (PLOT_SR * PLOT_MS / 1000.0) as usize;

/// Columns drawn: each one a min/max over its bucket, the way any
/// waveform overview is drawn.
const PLOT_COLS: usize = 300;

/// Where the tail is called over, as a fraction of the response's peak.
const TAIL_FLOOR: f32 = 0.02;

/// How much the drawing is magnified, and the lane says so.
///
/// An impulse response is mostly onset: the first sample is full scale
/// and the chirp that follows it — the part the device is FOR — is a
/// tenth of that. Drawn at unity the hero is a spike with a hairline
/// after it. Magnified, the onset clips against the top and bottom of
/// the lane, which costs nothing, because "the click is loud" was never
/// the thing worth reading.
///
/// A FIXED figure, for the reason the sheen's is: an auto-fit would make
/// the picture look identical at every setting, and the peak dropping as
/// the smear grows is one of the two things the amount knob does.
const PLOT_GAIN: f32 = 4.0;

/// One column of the picture.
#[derive(Clone, Copy, Default)]
struct Column {
    lo: f32,
    hi: f32,
}

/// Run the real kernel on a real impulse and reduce it to columns.
///
/// Returns the columns and the smear: how far into the window the
/// response is still above [`TAIL_FLOOR`] of its own peak, in
/// milliseconds, or `None` when it never fell that far — the ">" case the
/// tag prints.
fn trace(state: &DisperserUi) -> (Vec<Column>, Option<f32>) {
    let mut io = vec![0.0f32; PLOT_N];
    io[0] = 1.0;

    let mut chain = crate::dsp::filters::Disperser::new();
    chain.prepare(
        PLOT_SR,
        disperser_value(FREQ, state.freq),
        disperser_value(PINCH, state.pinch),
        disperser_value(AMOUNT, state.amount).round().max(0.0) as u32,
    );
    chain.process(&mut io);

    let peak = io.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    let floor = peak * TAIL_FLOOR;
    let last = io.iter().rposition(|s| s.abs() > floor);
    // The tail is "unknown" only when it is still going at the very last
    // sample of the window — anything short of that is a real figure.
    let smear = match last {
        Some(i) if i + 1 < PLOT_N => Some(i as f32 / PLOT_SR * 1000.0),
        _ => None,
    };

    let mut columns = vec![Column::default(); PLOT_COLS];
    for (c, column) in columns.iter_mut().enumerate() {
        let from = c * PLOT_N / PLOT_COLS;
        let to = ((c + 1) * PLOT_N / PLOT_COLS).min(PLOT_N).max(from + 1);
        for s in &io[from..to] {
            column.lo = column.lo.min(*s);
            column.hi = column.hi.max(*s);
        }
    }
    (columns, smear)
}

/// The hero: what a click becomes.
fn impulse(ui: &mut egui::Ui, theme: &Theme, state: &DisperserUi) {
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let plot = rect.shrink(pad);
    let mid = plot.center().y;
    let half = plot.height() * 0.5;

    let (columns, smear) = trace(state);

    // Zero, so the chirp has something to sit on.
    painter.line_segment(
        [egui::pos2(plot.left(), mid), egui::pos2(plot.right(), mid)],
        egui::Stroke::new(stroke::HAIR, theme.surface_sunken),
    );

    for (c, column) in columns.iter().enumerate() {
        let x = plot.left() + plot.width() * c as f32 / (PLOT_COLS - 1) as f32;
        let y0 = mid - (column.hi * PLOT_GAIN).clamp(-1.0, 1.0) * half;
        let y1 = mid - (column.lo * PLOT_GAIN).clamp(-1.0, 1.0) * half;
        // A column that reduces to nothing still gets a mark, so silence
        // reads as a line rather than as a gap in the drawing.
        painter.line_segment(
            [egui::pos2(x, y0), egui::pos2(x, y1.max(y0 + 0.5))],
            egui::Stroke::new(stroke::HAIR, theme.role_mod),
        );
    }

    // The corner tag, top RIGHT: the impulse lands at the very left, and
    // a left-anchored tag would print through the one moment the picture
    // is about.
    let stages = disperser_value(AMOUNT, state.amount).round() as u32;
    let tag = if stages == 0 {
        // The kernel's own words: zero sections is a wire. Said out loud
        // for the reason the lo-fi says `clean` — an off switch you can
        // see is one you can trust.
        "wire".to_owned()
    } else {
        match smear {
            Some(ms) => format!("{ms:.0} ms smear"),
            None => format!("> {PLOT_MS:.0} ms smear"),
        }
    };
    painter.text(
        egui::pos2(plot.right(), plot.top()),
        egui::Align2::RIGHT_TOP,
        tag,
        egui::FontId::proportional(font::MICRO_LABEL),
        theme.text_muted,
    );

    // What the lane is, and at what magnification — the same disclosure
    // the sheen's added lane makes, and for the same reason.
    painter.text(
        plot.left_bottom(),
        egui::Align2::LEFT_BOTTOM,
        format!("impulse ×{PLOT_GAIN:.0}"),
        egui::FontId::proportional(font::MICRO_LABEL),
        theme.text_muted,
    );
}

/// What ONE cell needs: room for the widest thing it will ever print.
fn cell_width(ui: &egui::Ui, theme: &Theme, p: &Param) -> f32 {
    let value = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &p.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::SM)
}

/// The three cells of the strip, in the order they are drawn.
fn strip(s: &Spec) -> [(&Param, u32); 3] {
    [(&s.amount, AMOUNT), (&s.freq, FREQ), (&s.pinch, PINCH)]
}

/// The width the strip needs, summed from the same per-cell figure the
/// strip draws each cell at — a share is not a sum.
fn strip_width(ui: &egui::Ui, theme: &Theme, s: &Spec) -> f32 {
    let cells = strip(s);
    let sum: f32 = cells.iter().map(|(p, _)| cell_width(ui, theme, p)).sum();
    sum + ui.spacing().item_spacing.x * (cells.len() - 1) as f32
}

/// How many of the screen's cell rows the value strip stands in. TWO,
/// because a labelled cell draws a value over a name — see
/// `notes/20260827-device-card-layout.md`, rule 1.
const VALUE_ROWS: usize = 2;

/// The card's only width contract.
fn face(ui: &egui::Ui, theme: &Theme, s: &Spec) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(strip_width(ui, theme, s), 0.0))
        .filling()])
}

/// Draw the disperser card. Returns the edits the user just made.
pub fn disperser_card(ui: &mut egui::Ui, theme: &Theme, state: &mut DisperserUi) -> Vec<ParamEdit> {
    let s = spec();
    let mut edits = Vec::new();
    let layout = face(ui, theme, &s).row([]);

    card::card_sized(ui, theme, "disperser", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, VALUE_ROWS, |ui, region| {
                match region {
                    // A READOUT, not a draggable target: a drag could only
                    // mean "one of three knobs, depending which way I
                    // moved", which is the ownerless gesture rule 1 of the
                    // device-UI contract exists to prevent.
                    poly_widgets::CurveRegion::Plot => impulse(ui, theme, state),
                    poly_widgets::CurveRegion::Footer => {
                        let h = ui.available_height();
                        ui.horizontal(|ui| {
                            for (param, id) in strip(&s) {
                                let w = cell_width(ui, theme, param);
                                let Some(norm) = state.slot(id) else {
                                    continue;
                                };
                                ui.allocate_ui_with_layout(
                                    egui::vec2(w, h),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.set_width(w);
                                        ui.set_height(h);
                                        if poly_widgets::labeled_cell_bar(
                                            ui, theme, param, norm, None,
                                        ) {
                                            edits.push(ParamEdit {
                                                param: id,
                                                value: disperser_value(id, *norm),
                                            });
                                        }
                                    },
                                );
                            }
                        });
                    }
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::disperser::AMOUNT_MAX;

    #[test]
    fn every_table_row_leaves_as_an_edit() {
        let edits = disperser_edits(&DisperserUi::default());
        assert_eq!(edits.len(), TABLE.len());
        for row in TABLE {
            assert!(
                edits.iter().any(|e| e.param == row.id),
                "`{}` never leaves the card",
                row.name
            );
        }
    }

    #[test]
    fn every_position_is_a_legal_engine_value() {
        for row in TABLE {
            for step in 0..=64 {
                let norm = step as f32 / 64.0;
                let value = disperser_value(row.id, norm);
                assert!(
                    value >= row.min - 1e-3 && value <= row.max + 1e-3,
                    "`{}` at {norm} gave {value}, outside {}..={}",
                    row.name,
                    row.min,
                    row.max
                );
            }
            assert!(
                (disperser_value(row.id, 0.0) - row.min).abs() < 1e-3,
                "{}",
                row.name
            );
            assert!(
                (disperser_value(row.id, 1.0) - row.max).abs() < 1e-3,
                "{}",
                row.name
            );
        }
    }

    /// Stated over VALUES, since the section count quantizes.
    #[test]
    fn value_and_norm_round_trip() {
        for row in TABLE {
            for step in 0..=32 {
                let value = row.min + (row.max - row.min) * step as f32 / 32.0;
                let back = disperser_value(row.id, disperser_norm(row.id, value));
                let tol = if row.id == AMOUNT {
                    0.51
                } else {
                    1e-2 * row.max.abs().max(1.0)
                };
                assert!(
                    (back - value).abs() <= tol,
                    "`{}`: {value} came back as {back}",
                    row.name
                );
            }
        }
    }

    #[test]
    fn defaults_come_from_the_table() {
        let state = DisperserUi::default();
        for (id, norm) in [
            (AMOUNT, state.amount),
            (FREQ, state.freq),
            (PINCH, state.pinch),
        ] {
            let want = params::def(TABLE, id).default;
            let got = disperser_value(id, norm);
            assert!(
                (got - want).abs() <= 1e-2 * want.abs().max(1.0),
                "`{}` defaults to {got}, table says {want}",
                params::def(TABLE, id).name
            );
        }
    }

    /// Every step of the amount strip names the section count it sets, so
    /// a cell reading `8` cannot be running seven.
    #[test]
    fn the_amount_names_are_the_section_counts() {
        assert_eq!(AMOUNT_NAMES.len() as f32, AMOUNT_MAX + 1.0);
        for (index, name) in AMOUNT_NAMES.iter().enumerate() {
            let norm = index as f32 / (AMOUNT_NAMES.len() - 1) as f32;
            let stages = disperser_value(AMOUNT, norm);
            assert_eq!(
                name.parse::<f32>().expect("a count is a number"),
                stages,
                "step {index} prints `{name}` and sets {stages}"
            );
        }
    }

    /// The bottom of the amount knob is the kernel's wire, and it really
    /// is one: an impulse comes back as an impulse.
    #[test]
    fn no_sections_is_a_wire() {
        let state = DisperserUi {
            amount: disperser_norm(AMOUNT, 0.0),
            ..DisperserUi::default()
        };
        let mut chain = crate::dsp::filters::Disperser::new();
        chain.prepare(
            PLOT_SR,
            disperser_value(FREQ, state.freq),
            disperser_value(PINCH, state.pinch),
            0,
        );
        let mut io = vec![0.0f32; 64];
        io[0] = 1.0;
        let before = io.clone();
        chain.process(&mut io);
        assert_eq!(io, before, "zero sections altered the block");
    }

    /// The point of the device, asserted: more sections spread the click
    /// further in time, and the peak drops as it does — an allpass chain
    /// moves energy about rather than adding any.
    ///
    /// This is also what the hero is a picture of, which is why it is
    /// worth pinning here rather than trusting the drawing.
    #[test]
    fn more_sections_smear_further_and_flatter() {
        let light = DisperserUi {
            amount: disperser_norm(AMOUNT, 2.0),
            ..DisperserUi::default()
        };
        let heavy = DisperserUi {
            amount: disperser_norm(AMOUNT, 24.0),
            ..DisperserUi::default()
        };

        let peak_of = |cols: &[Column]| {
            cols.iter()
                .fold(0.0f32, |a, c| a.max(c.hi.abs()).max(c.lo.abs()))
        };
        let (light_cols, light_ms) = trace(&light);
        let (heavy_cols, heavy_ms) = trace(&heavy);

        assert!(
            peak_of(&heavy_cols) < peak_of(&light_cols),
            "24 sections peaked at {} and 2 at {}",
            peak_of(&heavy_cols),
            peak_of(&light_cols)
        );
        // `None` means the tail outran the window, which is further than
        // any figure the window can report.
        let further = match (light_ms, heavy_ms) {
            (Some(l), Some(h)) => h > l,
            (Some(_), None) => true,
            _ => false,
        };
        assert!(
            further,
            "24 sections smeared {heavy_ms:?}, 2 gave {light_ms:?}"
        );
    }

    /// A disperser's whole safety property: it moves phase and NOT
    /// magnitude. Checked as the kernel's own doc states it — the energy
    /// of the impulse response is preserved, whatever the settings.
    #[test]
    fn the_magnitude_response_stays_flat() {
        let energy = |state: &DisperserUi| {
            let mut io = vec![0.0f32; 1 << 15];
            io[0] = 1.0;
            let mut chain = crate::dsp::filters::Disperser::new();
            chain.prepare(
                PLOT_SR,
                disperser_value(FREQ, state.freq),
                disperser_value(PINCH, state.pinch),
                disperser_value(AMOUNT, state.amount).round() as u32,
            );
            chain.process(&mut io);
            io.iter().map(|s| s * s).sum::<f32>()
        };
        let mut state = DisperserUi::default();
        for stages in [0.0, 1.0, 8.0, 32.0] {
            state.amount = disperser_norm(AMOUNT, stages);
            let e = energy(&state);
            assert!(
                (e - 1.0).abs() < 0.02,
                "{stages} sections changed the energy to {e}, so the magnitude moved"
            );
        }
    }
}
