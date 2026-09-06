//! SPECTRA's face: the frame, the bins, and the three numbers that
//! describe what is in them.
//!
//! This is the only section on the desk that is not analogue in its
//! bones. It cuts the sound into frames, transforms them, changes them
//! as numbers and puts them back — so the card is drawn as the machine
//! it is, in cells and in fixed-width figures, on the bin axis rather
//! than on the ear's.
//!
//! # What it does NOT draw
//!
//! It does not draw a spectrum. The section reports three descriptors of
//! its analysed frame — the CENTROID, where the spectrum's mass sits;
//! the SPREAD, how far it is scattered about that; and the FLUX, how
//! fast it is changing — and it does not report the bins themselves.
//!
//! A card could take those three and paint a plausible bell across the
//! axis, and it would look like a spectrum analyser and be a lie: it
//! would show peaks that were never measured. So the field shows exactly
//! what was measured and no more — a marked bin for the centroid, a lit
//! run of bins for the spread either side of it, and the rest of the
//! grid dark. What is dark is not silence. It is unmeasured, which is a
//! different thing and worth being able to tell apart.
//!
//! # The frame block
//!
//! The rest is arithmetic the section cannot hide: the window, the hop,
//! the overlap that falls out of the two, how many bins that makes, how
//! much of the spectrum one bin covers, and the latency the transform
//! costs — which the graph really does pay back.

use super::*;
use crate::ui::chrome;

/// One control's row.
/// @tune 12..26 px
pub(super) const ROW_H: f32 = 18.0;
const COLUMN_GAP: f32 = 8.0;
/// The bin field's cells across and down.
/// @tune 16..96
const CELLS_ACROSS: usize = 48;
/// @tune 3..16
const CELLS_DOWN: usize = 7;
/// The rate the frame's arithmetic is reported at.
const DRAWN_AT: f32 = 48_000.0;
/// The five modes, in the order the parameter numbers them.
const MODES: [&str; 5] = ["FREEZE", "BLUR", "PITCH", "CHOIR", "ROBOT"];

/// SPECTRA's seven controls.
#[derive(Clone, Copy, Debug)]
struct SpectraFace {
    mode: egui::Rect,
    field: egui::Rect,
    freeze: egui::Rect,
    blur: egui::Rect,
    pitch: egui::Rect,
    voice_a: egui::Rect,
    voice_b: egui::Rect,
    mix: egui::Rect,
}

impl Layout for SpectraFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        use crate::params::console::spectra as p;
        vec![
            (p::MODE, self.mode),
            (p::FREEZE, self.freeze),
            (p::BLUR, self.blur),
            (p::PITCH, self.pitch),
            (p::VOICE_A, self.voice_a),
            (p::VOICE_B, self.voice_b),
            (p::MIX, self.mix),
        ]
    }
}

fn spectra_face(glass: egui::Rect) -> SpectraFace {
    let x = egui::Rect::from_min_max(
        egui::pos2(glass.left() + 5.0, glass.top() + 3.0),
        egui::pos2(glass.right() - 20.0, glass.bottom() - 12.0),
    );
    let mode = egui::Rect::from_min_size(x.min, egui::vec2(x.width(), ROW_H));
    let body = egui::Rect::from_min_max(egui::pos2(x.left(), mode.bottom() + 4.0), x.max);
    // Six knobs need six rows and the field needs height, so they take a
    // column each. The frame's arithmetic does not get a panel of its
    // own — it is fixed, it never moves, and it reads better as one line
    // of figures along the field's foot than as a table nobody consults.
    let right_w = (body.width() * 0.42).clamp(150.0, 230.0);
    let field = egui::Rect::from_min_max(
        body.min,
        egui::pos2(body.right() - right_w - COLUMN_GAP, body.bottom()),
    );
    let right = egui::Rect::from_min_max(egui::pos2(body.right() - right_w, body.top()), body.max);
    let rows_h = ROW_H * 6.0 + 5.0;
    let top = (right.bottom() - rows_h).max(right.top());
    let row = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(right.left(), top + i as f32 * (ROW_H + 1.0)),
            egui::vec2(right.width(), ROW_H),
        )
    };
    SpectraFace {
        mode,
        field,
        freeze: row(0),
        blur: row(1),
        pitch: row(2),
        voice_a: row(3),
        voice_b: row(4),
        mix: row(5),
    }
}

/// The frame's arithmetic, which is fixed by the transform and not by
/// any knob: bins, what one bin covers in hertz, the overlap, and the
/// latency the graph pays back.
fn frame_facts(rate: f32) -> (usize, f32, usize, f32) {
    use crate::params::console::spectra as p;
    let bins = p::SIZE / 2 + 1;
    let per_bin = rate / p::SIZE as f32;
    let overlap = p::SIZE / p::HOP.max(1);
    let latency_ms = p::SIZE as f32 / rate * 1e3;
    (bins, per_bin, overlap, latency_ms)
}

/// Which cells of the field a centroid and spread light.
///
/// Returns the cell the centroid falls in and the run either side of it
/// the spread covers, both on the same 0..1 bin axis the section
/// measures on. Nothing outside that run is claimed to be anything.
fn lit_cells(centroid: f32, spread: f32, across: usize) -> (usize, usize, usize) {
    let across = across.max(1);
    let last = across - 1;
    let at = ((centroid.clamp(0.0, 1.0) * last as f32).round() as usize).min(last);
    let reach = (spread.clamp(0.0, 1.0) * last as f32).round() as usize;
    (at, at.saturating_sub(reach), (at + reach).min(last))
}

pub(super) fn draw(face: &Face<'_>) {
    use crate::params::console::spectra as p;
    let painter = face.painter;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let lay = spectra_face(face.glass);
    let mode = face.value(p::MODE).round().clamp(0.0, 4.0) as usize;
    let freeze = face.value(p::FREEZE) >= 0.5;
    let blur = face.value(p::BLUR) / 100.0;
    let pitch = face.value(p::PITCH);
    let voice_a = face.value(p::VOICE_A);
    let voice_b = face.value(p::VOICE_B);
    let mix = face.value(p::MIX) / 100.0;
    let wire = mix <= 0.0;
    // The three descriptors, and the frame's own loudest bin.
    let centroid = face.said.bands[0];
    let spread = face.said.bands[1];
    let flux = face.said.bands[2];
    let peak_db = face.said.reduction_db;
    let measured = centroid > 0.0 || spread > 0.0;

    let mut shapes = Vec::new();

    // ---- The mode strip. ---------------------------------------------
    chrome::panel_frame_variant(&mut shapes, lay.mode, Weight::Hair, edge, 2);
    let mode_w = lay.mode.width() / MODES.len() as f32;
    let mode_cell = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(lay.mode.left() + i as f32 * mode_w, lay.mode.top()),
            egui::vec2(mode_w, lay.mode.height()),
        )
    };
    chrome::brackets(
        &mut shapes,
        mode_cell(mode).shrink(2.0),
        4.0,
        Weight::Hair,
        ink,
    );

    // ---- The bin field. ----------------------------------------------
    chrome::panel_variant(
        &mut shapes,
        lay.field,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let grid = egui::Rect::from_min_max(
        egui::pos2(lay.field.left() + 8.0, lay.field.top() + font.size + 6.0),
        egui::pos2(
            lay.field.right() - 8.0,
            lay.field.bottom() - font.size - 6.0,
        ),
    );
    let across = crate::tune!(CELLS_ACROSS).max(1);
    let down = crate::tune!(CELLS_DOWN).max(1);
    let cw = grid.width() / across as f32;
    let ch = grid.height() / down as f32;
    let (at, from, to) = lit_cells(centroid, spread, across);
    for col in 0..across {
        for row in 0..down {
            let cell = egui::Rect::from_min_size(
                egui::pos2(grid.left() + col as f32 * cw, grid.top() + row as f32 * ch),
                egui::vec2(cw - 1.0, ch - 1.0),
            );
            // Dark is UNMEASURED, not silent. Only the run the section
            // actually reported is lit, and the centre column of it is
            // the centroid itself.
            let lit = measured && !wire && col >= from && col <= to;
            let ink_here = if !lit {
                edge.gamma_multiply(0.30)
            } else if col == at {
                alpha.live.color
            } else {
                // Away from the centroid the run fades, which is the
                // spread being a deviation and not a boundary.
                let reach = (to - from).max(1) as f32 * 0.5;
                let away = (col as f32 - at as f32).abs() / reach;
                tool::mix_ink(alpha.live.color, alpha.live_dim.color, away.clamp(0.0, 1.0))
                    .gamma_multiply(1.0 - 0.45 * away.clamp(0.0, 1.0))
            };
            shapes.push(egui::Shape::rect_filled(cell, 0.0, ink_here));
        }
    }
    // The flux, as a bar under the grid: how fast the frame is changing.
    let flux_bar = egui::Rect::from_min_max(
        egui::pos2(grid.left(), grid.bottom() + 2.0),
        egui::pos2(grid.right(), grid.bottom() + 4.0),
    );
    if flux_bar.is_positive() {
        shapes.push(egui::Shape::rect_filled(
            flux_bar,
            0.0,
            edge.gamma_multiply(0.35),
        ));
        if flux > 0.0 && !wire {
            // Quantised to the same cells as the grid: this card counts
            // rather than measures.
            let cells = (flux.clamp(0.0, 1.0) * across as f32).round().max(1.0);
            shapes.push(egui::Shape::rect_filled(
                egui::Rect::from_min_max(
                    flux_bar.min,
                    egui::pos2(flux_bar.left() + cells * cw, flux_bar.bottom()),
                ),
                0.0,
                alpha.jeopardy_latent.color,
            ));
        }
    }

    painter.extend(shapes);

    // ---- The words. --------------------------------------------------
    let mut words = tool::Ledger::new(painter, font.clone(), "SPECTRA");
    let span = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), ink)
            .rect
            .width()
    };
    let (_bins, per_bin, overlap, latency_ms) = frame_facts(DRAWN_AT);
    for (i, word) in MODES.into_iter().enumerate() {
        words.text(
            mode_cell(i).center(),
            egui::Align2::CENTER_CENTER,
            word,
            if i == mode { ink } else { edge },
        );
    }
    words.text(
        egui::pos2(lay.field.left() + 8.0, lay.field.top() + 2.0),
        egui::Align2::LEFT_TOP,
        format!("BINS DC-{:.0}k", DRAWN_AT / 2000.0),
        edge,
    );
    // What the field IS, said plainly, because it is not a spectrum.
    words.text(
        egui::pos2(lay.field.right() - 8.0, lay.field.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if wire {
            "WIRE".to_owned()
        } else if measured {
            format!(
                "C{:03} S{:03}",
                (centroid * 999.0) as u32,
                (spread * 999.0) as u32
            )
        } else {
            "NO FRAME".to_owned()
        },
        if wire || !measured {
            edge
        } else {
            alpha.live.color
        },
    );
    // The frame's arithmetic, on one line along the foot: fixed by the
    // transform, none of it a knob, and none of it worth a table.
    words.text(
        egui::pos2(grid.left(), lay.field.bottom() - 2.0),
        egui::Align2::LEFT_BOTTOM,
        format!(
            "{}/{} {overlap}x {per_bin:.0}Hz {latency_ms:.0}ms",
            p::SIZE,
            p::HOP
        ),
        edge,
    );
    // The knobs. The one the mode actually uses is the bright one; the
    // rest are still there and still say what they are set to, because
    // changing mode should not make a number vanish.
    let gutter = ["FREEZE", "BLUR", "PITCH", "VOICE A", "VOICE B", "MIX"]
        .into_iter()
        .map(span)
        .fold(0.0f32, f32::max)
        + 6.0;
    let figure = span("-12.0") + 6.0;
    for (rect, word, share, said, used) in [
        (
            lay.freeze,
            "FREEZE",
            if freeze { 1.0 } else { 0.0 },
            if freeze { "HELD" } else { "OFF" }.to_owned(),
            mode == p::MODE_FREEZE as usize,
        ),
        (
            lay.blur,
            "BLUR",
            blur,
            format!("{:.0}", blur * 100.0),
            mode == p::MODE_BLUR as usize,
        ),
        (
            lay.pitch,
            "PITCH",
            (pitch + 24.0) / 48.0,
            format!("{pitch:+.0}"),
            mode == p::MODE_PITCH as usize,
        ),
        (
            lay.voice_a,
            "VOICE A",
            (voice_a + 12.0) / 24.0,
            format!("{voice_a:+.0}"),
            mode == p::MODE_CHOIR as usize,
        ),
        (
            lay.voice_b,
            "VOICE B",
            (voice_b + 12.0) / 24.0,
            format!("{voice_b:+.0}"),
            mode == p::MODE_CHOIR as usize,
        ),
        (lay.mix, "MIX", mix, format!("{:.0}", mix * 100.0), true),
    ] {
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.left() + gutter, rect.center().y - 2.5),
            egui::pos2(rect.right() - figure, rect.center().y + 2.5),
        );
        if bar.is_positive() {
            painter.rect_filled(bar, 0.0, edge.gamma_multiply(0.3));
            if share > 0.0 {
                // Quantised into cells, like everything else here.
                let steps = 24.0;
                let lit = (share.clamp(0.0, 1.0) * steps).round().max(1.0);
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        bar.min,
                        egui::pos2(bar.left() + lit / steps * bar.width(), bar.bottom()),
                    ),
                    0.0,
                    if used { alpha.live.color } else { edge },
                );
            }
        }
        words.text(
            egui::pos2(rect.left() + 2.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            word,
            if used { ink } else { edge },
        );
        words.text(
            egui::pos2(rect.right() - 2.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            said,
            if used { ink } else { edge },
        );
    }
    words.finish();

    face.mark(&lay);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glass() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(480.0, 210.0))
    }

    #[test]
    fn every_spectra_control_has_its_own_place() {
        let face = spectra_face(glass());
        let controls = face.controls();
        assert_eq!(controls.len(), 7);
        for (index, (id, rect)) in controls.iter().enumerate() {
            assert!(glass().contains_rect(*rect), "control {id} left the glass");
            assert!(rect.is_positive(), "control {id} lost its place");
            for (other, other_rect) in &controls[index + 1..] {
                assert!(!rect.intersects(*other_rect), "{id} and {other} overlap");
            }
        }
        assert!(!face.field.intersects(face.mix));
    }

    /// The frame's arithmetic is arithmetic, not a table of numbers
    /// somebody typed: the overlap falls out of the window and the hop,
    /// the bin count out of the window, and the latency out of the
    /// window and the rate.
    #[test]
    fn the_frame_block_is_computed_from_the_transform() {
        use crate::params::console::spectra as p;
        let (bins, per_bin, overlap, latency) = frame_facts(48_000.0);
        assert_eq!(bins, p::SIZE / 2 + 1);
        assert_eq!(overlap, p::SIZE / p::HOP);
        assert!((per_bin - 48_000.0 / p::SIZE as f32).abs() < 1e-3);
        assert!((latency - p::SIZE as f32 / 48.0).abs() < 1e-3);
        // At twice the rate a bin is twice as wide and the latency half.
        let (_, wide, _, quick) = frame_facts(96_000.0);
        assert!((wide - per_bin * 2.0).abs() < 1e-3);
        assert!((quick - latency * 0.5).abs() < 1e-3);
    }

    /// The field lights only what was measured. A card that painted a
    /// bell across the axis from these two numbers would be showing
    /// peaks nobody reported, so the run is exactly the spread either
    /// side of the centroid and no wider.
    #[test]
    fn the_field_lights_only_the_run_that_was_measured() {
        let across = 48;
        // A lone partial: spread near zero, so one cell or thereabouts.
        let (at, from, to) = lit_cells(0.5, 0.0, across);
        assert_eq!((from, to), (at, at), "a spreadless frame lit a range");
        // Broadband: a wide run about where the mass is.
        let (at, from, to) = lit_cells(0.5, 0.20, across);
        assert!(from < at && to > at);
        assert_eq!(at - from, to - at, "the run was not centred");
        // Against an edge it is NOT centred, and should not be: there
        // are no bins below DC to light, so the run stops. Drawing the
        // missing half anyway would be inventing spectrum again.
        let (at, from, to) = lit_cells(0.05, 0.30, across);
        assert_eq!(from, 0, "the run ran past DC");
        assert!(to - at > at - from, "the run did not stop at the edge");
        // Silence reports exactly zero and lights the first cell only.
        let (at, from, to) = lit_cells(0.0, 0.0, across);
        assert_eq!((at, from, to), (0, 0, 0));
        // Nothing ever runs off the grid, at any reading.
        for (c, s) in [(0.0f32, 1.0f32), (1.0, 1.0), (0.9, 0.5), (1.5, 2.0)] {
            let (at, from, to) = lit_cells(c, s, across);
            assert!(at < across && from <= at && to < across);
        }
    }
}
