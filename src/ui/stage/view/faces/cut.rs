//! CUT's face: the one card on the desk drawn in characters.
//!
//! Every other section is vectors. This one is a terminal readout — the
//! response plotted into a grid of monospace cells, the meters written
//! as bracketed runs of hashes, everything at one micro size. A filter
//! is the section an engineer reads rather than watches, and a character
//! plot is what reading looks like.
//!
//! The glyph a column takes says what the curve is DOING there, not just
//! where it is: climbing, falling, flat, or standing on a resonance.
//!
//! Three things here are measured and not drawn from the knobs:
//!
//! - **RING**, per loop, from the band-pass state that IS the resonance
//!   current — 0 while a blade is parked, 1 while the loop sings.
//! - **HEAT**, how deep a loop is into its own tanh; the crunch the
//!   response curve cannot express, which is why the module note says
//!   the card must show it as heat.
//! - **LOSS**, the section's true cost in dB: what came in, less what
//!   went out.

use super::*;
use crate::ui::chrome;

/// One readout row on the right.
/// @tune 9..24 px
const ROW_H: f32 = 14.0;
/// The share of the glass the character plot takes.
/// @tune 0.4..0.8
const PLOT_SHARE: f32 = 0.56;
const COLUMN_GAP: f32 = 7.0;
/// The plot's window, in dB.
const TOP_DB: f32 = 12.0;
const FLOOR_DB: f32 = -48.0;
/// Its decades.
const HZ_MIN: f32 = 20.0;
const HZ_MAX: f32 = 20_000.0;
/// How many cells a bracketed meter runs to.
const METER_CELLS: usize = 8;

/// CUT's five parameters and the row each one owns.
#[derive(Clone, Copy, Debug)]
struct CutFace {
    plot: egui::Rect,
    hp_hz: egui::Rect,
    hp_res: egui::Rect,
    lp_hz: egui::Rect,
    lp_res: egui::Rect,
    crunch: egui::Rect,
    /// The measured block: ring, heat, loss. Not controls.
    said: egui::Rect,
}

impl CutFace {
    fn controls(self) -> [(u32, egui::Rect); 5] {
        use crate::params::console::cut as p;
        [
            (p::HP_HZ, self.hp_hz),
            (p::HP_RES, self.hp_res),
            (p::LP_HZ, self.lp_hz),
            (p::LP_RES, self.lp_res),
            (p::CRUNCH, self.crunch),
        ]
    }
}

impl Layout for CutFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        CutFace::controls(*self).to_vec()
    }
}

fn cut_face(glass: egui::Rect) -> CutFace {
    let x = glass.shrink2(egui::vec2(5.0, 2.0));
    let plot_w = (x.width() - COLUMN_GAP) * crate::tune!(PLOT_SHARE);
    let plot = egui::Rect::from_min_max(x.min, egui::pos2(x.left() + plot_w, x.bottom()));
    let right = egui::Rect::from_min_max(egui::pos2(plot.right() + COLUMN_GAP, x.top()), x.max);
    let row_h = crate::tune!(ROW_H);
    let row = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(right.left(), right.top() + i as f32 * (row_h + 1.0)),
            egui::vec2(right.width(), row_h),
        )
    };
    let said = egui::Rect::from_min_max(egui::pos2(right.left(), row(4).bottom() + 5.0), right.max);
    CutFace {
        plot,
        hp_hz: row(0),
        hp_res: row(1),
        lp_hz: row(2),
        lp_res: row(3),
        crunch: row(4),
        said,
    }
}

/// Where a frequency stands across the plot, on the log scale.
fn place_of_hz(hz: f32) -> f32 {
    ((hz.max(1.0).log10() - HZ_MIN.log10()) / (HZ_MAX.log10() - HZ_MIN.log10())).clamp(0.0, 1.0)
}

/// A bracketed run of cells, the way a terminal writes a meter.
fn meter(share: f32) -> String {
    meter_n(share, METER_CELLS)
}

/// The same, at a stated width. The two ring meters stand side by side
/// on one line and cannot each be as wide as a meter that stands alone.
fn meter_n(share: f32, cells: usize) -> String {
    let lit = (share.clamp(0.0, 1.0) * cells as f32).round() as usize;
    let mut out = String::with_capacity(cells + 2);
    out.push('[');
    for i in 0..cells {
        out.push(if i < lit { '#' } else { '.' });
    }
    out.push(']');
    out
}

/// How wide the paired ring meters run.
const RING_CELLS: usize = 5;

/// A frequency, said the short way.
fn hz_word(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{:.1}k", hz / 1000.0)
    } else {
        format!("{hz:.0}")
    }
}

/// The plot's GROUND, in character cells: the decades and the line the
/// curve is measured from.
///
/// The grid stays text — it is ruling, and ruling made of `:` and `-` is
/// what a terminal's graph paper looks like. The curve itself is not:
/// a filter skirt is a continuous thing and quantising it to a cell
/// grid threw away the very steepness the section exists to have.
fn ground_lines(cols: usize, rows: usize) -> Vec<String> {
    if cols == 0 || rows == 0 {
        return Vec::new();
    }
    let mut ground = vec![vec![' '; cols]; rows];
    let unity = row_of(0.0, rows);
    for cell in ground[unity].iter_mut() {
        *cell = '-';
    }
    for hz in [100.0f32, 1_000.0, 10_000.0] {
        let col = ((place_of_hz(hz) * (cols - 1) as f32).round() as usize).min(cols - 1);
        for (r, line) in ground.iter_mut().enumerate() {
            line[col] = if r == unity { '+' } else { ':' };
        }
    }
    ground
        .into_iter()
        .map(|line| line.into_iter().collect())
        .collect()
}

/// Which cell row a level falls on, for the ruling.
fn row_of(db: f32, rows: usize) -> usize {
    let t = ((TOP_DB - db) / (TOP_DB - FLOOR_DB)).clamp(0.0, 1.0);
    ((t * (rows.max(2) - 1) as f32).round() as usize).min(rows.saturating_sub(1))
}

pub(super) fn draw(face: &Face<'_>) {
    use crate::console::cut_curve as cc;
    use crate::params::console::cut as p;
    let painter = face.painter;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    // ONE size, and it is the micro one: this card is text.
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let lay = cut_face(face.glass);
    let shape = cc::Shape::of(&face.params);
    let value = |id: u32| face.value(id);
    let hp_res = value(p::HP_RES);
    let lp_res = value(p::LP_RES);
    let crunch = value(p::CRUNCH);
    // What the section measured of itself, last block.
    let hp_ring = face.said.bands[0];
    let lp_ring = face.said.bands[1];
    let heat = face.said.reduction_db.abs();
    let loss = (face.said.bands[2] - face.said.level_db).max(0.0);
    let quiet = face.said.level_db <= -119.0;

    let mut shapes = Vec::new();
    chrome::panel_variant(
        &mut shapes,
        lay.plot,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    painter.extend(shapes);

    // ---- The character plot. -----------------------------------------
    let cell = painter
        .layout_no_wrap("M".to_owned(), font.clone(), ink)
        .rect;
    let (cw, ch) = (cell.width().max(1.0), cell.height().max(1.0));
    let field = egui::Rect::from_min_max(
        egui::pos2(lay.plot.left() + 6.0, lay.plot.top() + ch + 3.0),
        egui::pos2(lay.plot.right() - 6.0, lay.plot.bottom() - ch - 3.0),
    );
    let cols = (field.width() / cw).floor().max(0.0) as usize;
    let rows = (field.height() / ch).floor().max(0.0) as usize;
    for (i, line) in ground_lines(cols, rows).iter().enumerate() {
        painter.text(
            egui::pos2(field.left(), field.top() + i as f32 * ch),
            egui::Align2::LEFT_TOP,
            line,
            font.clone(),
            edge.gamma_multiply(0.40),
        );
    }
    // The curve is a real curve, laid over the ruling. It takes the
    // crunch's heat as it is leaned on: the one thing about this
    // section a response cannot otherwise say.
    let curve_ink = tool::mix_ink(ink, alpha.jeopardy_latent.color, crunch / 100.0);
    // Plotted against the SAME grid the ruling uses, so the trace and
    // the line it is measured from cannot drift apart.
    let plotted = egui::Rect::from_min_max(
        field.min,
        egui::pos2(
            field.left() + cols.max(1) as f32 * cw,
            field.top() + rows.max(1) as f32 * ch,
        ),
    );
    // A smooth y, not the cell's: the ruling is quantised, the curve is
    // not, and the two agree at 0 dB because that is where the line is.
    let y_smooth = |db: f32| {
        let t = ((TOP_DB - db) / (TOP_DB - FLOOR_DB)).clamp(0.0, 1.0);
        plotted.top() + (t * (rows.max(2) - 1) as f32 + 0.5) * ch
    };
    let steps = (cols.max(8) * 3).min(360);
    let trace: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let hz = HZ_MIN * (HZ_MAX / HZ_MIN).powf(t);
            egui::pos2(
                plotted.left() + t * (plotted.width() - cw) + cw * 0.5,
                y_smooth(cc::response_db(&shape, hz)),
            )
        })
        .collect();
    let mut curve_shapes = Vec::new();
    chrome::trace(&mut curve_shapes, &trace, Weight::Heavy, curve_ink);
    // Where each blade stands, on the curve it bends.
    for (hz, off) in [(shape.hp_hz, shape.hp_off()), (shape.lp_hz, shape.lp_off())] {
        if off {
            continue;
        }
        chrome::pad(
            &mut curve_shapes,
            egui::pos2(
                plotted.left() + place_of_hz(hz) * (plotted.width() - cw) + cw * 0.5,
                y_smooth(cc::response_db(&shape, hz)),
            ),
            chrome::PAD,
            alpha.live.color,
            true,
        );
    }
    painter.extend(curve_shapes);

    // ---- Everything else is a line of text. --------------------------
    let label = |at: egui::Pos2, align: egui::Align2, text: String, ink| {
        painter.text(at, align, text, font.clone(), ink);
    };
    label(
        egui::pos2(lay.plot.left() + 6.0, lay.plot.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "CUT/RESPONSE".to_owned(),
        edge,
    );
    label(
        egui::pos2(lay.plot.right() - 6.0, lay.plot.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if shape.is_off() {
            "WIRE".to_owned()
        } else {
            format!("{TOP_DB:+.0}/{FLOOR_DB:.0}dB")
        },
        if shape.is_off() {
            edge
        } else {
            alpha.live.color
        },
    );
    label(
        egui::pos2(field.left(), lay.plot.bottom() - 2.0),
        egui::Align2::LEFT_BOTTOM,
        "20".to_owned(),
        edge,
    );
    label(
        egui::pos2(field.center().x, lay.plot.bottom() - 2.0),
        egui::Align2::CENTER_BOTTOM,
        "1k".to_owned(),
        edge,
    );
    label(
        egui::pos2(field.right(), lay.plot.bottom() - 2.0),
        egui::Align2::RIGHT_BOTTOM,
        "20k".to_owned(),
        edge,
    );

    // The five controls, each one line: word, figure, meter.
    let parked = |off: bool| if off { edge } else { ink };
    for (rect, word, said, share, dim) in [
        (
            lay.hp_hz,
            "HP",
            hz_word(shape.hp_hz),
            place_of_hz(shape.hp_hz),
            shape.hp_off(),
        ),
        (
            lay.hp_res,
            " Q",
            format!("{:.1}", cc::q_of(hp_res / 100.0)),
            hp_res / 100.0,
            shape.hp_off(),
        ),
        (
            lay.lp_hz,
            "LP",
            hz_word(shape.lp_hz),
            place_of_hz(shape.lp_hz),
            shape.lp_off(),
        ),
        (
            lay.lp_res,
            " Q",
            format!("{:.1}", cc::q_of(lp_res / 100.0)),
            lp_res / 100.0,
            shape.lp_off(),
        ),
        (
            lay.crunch,
            "CR",
            format!("{crunch:.0}"),
            crunch / 100.0,
            crunch <= 0.0,
        ),
    ] {
        let tone = parked(dim);
        label(
            egui::pos2(rect.left() + 2.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            word.to_owned(),
            edge,
        );
        label(
            egui::pos2(rect.left() + cw * 3.5, rect.center().y),
            egui::Align2::LEFT_CENTER,
            meter(share),
            tone,
        );
        label(
            egui::pos2(rect.right() - 3.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            said,
            tone,
        );
    }

    // The measured block. Nothing here comes from a knob.
    let mut y = lay.said.top() + 1.0;
    let line = |painter: &egui::Painter, y: f32, left: String, right: String, ink| {
        painter.text(
            egui::pos2(lay.said.left() + 2.0, y),
            egui::Align2::LEFT_TOP,
            left,
            font.clone(),
            edge,
        );
        painter.text(
            egui::pos2(lay.said.right() - 3.0, y),
            egui::Align2::RIGHT_TOP,
            right,
            font.clone(),
            ink,
        );
    };
    for (word, said, tone) in [
        (
            "RNG".to_owned(),
            format!(
                "{}{}",
                meter_n(hp_ring, RING_CELLS),
                meter_n(lp_ring, RING_CELLS)
            ),
            if hp_ring.max(lp_ring) > 0.6 {
                alpha.live.color
            } else {
                ink
            },
        ),
        (
            "HEAT".to_owned(),
            meter(heat),
            tool::mix_ink(ink, alpha.jeopardy_latent.color, heat),
        ),
        (
            "LOSS".to_owned(),
            if quiet {
                "--".to_owned()
            } else {
                format!("-{loss:.1}dB")
            },
            if quiet { edge } else { ink },
        ),
    ] {
        if y + ch > lay.said.bottom() {
            break;
        }
        line(painter, y, word, said, tone);
        y += ch + 1.0;
    }

    face.mark(&lay);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glass() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(440.0, 210.0))
    }

    #[test]
    fn every_cut_parameter_has_its_own_row() {
        let face = cut_face(glass());
        let controls = face.controls();
        for (index, (id, rect)) in controls.iter().enumerate() {
            assert!(
                glass().contains_rect(*rect),
                "parameter {id} left the glass"
            );
            assert!(rect.is_positive(), "parameter {id} lost its row");
            assert!(
                !face.plot.intersects(*rect),
                "parameter {id} invaded the plot"
            );
            for (other, other_rect) in &controls[index + 1..] {
                assert!(
                    !rect.intersects(*other_rect),
                    "parameters {id} and {other} overlap"
                );
            }
        }
        assert!(!face.said.intersects(face.crunch));
    }

    /// A bracketed meter is always the same width, whatever it reads —
    /// a column of them that jittered would be unreadable.
    #[test]
    fn every_meter_is_the_same_width() {
        let widths: Vec<usize> = [0.0, 0.3, 0.5, 1.0, 2.0, -1.0]
            .into_iter()
            .map(|share| meter(share).chars().count())
            .collect();
        assert!(widths.iter().all(|w| *w == METER_CELLS + 2), "{widths:?}");
        assert_eq!(meter(0.0), "[........]");
        assert_eq!(meter(1.0), "[########]");
        // The paired ring meters keep their own, narrower width.
        assert_eq!(meter_n(0.0, RING_CELLS).chars().count(), RING_CELLS + 2);
        assert_eq!(meter_n(1.0, RING_CELLS), "[#####]");
    }

    /// The ruling is a rectangle of cells: every line the same length,
    /// and the line the curve is measured from is on every one of them.
    #[test]
    fn the_ruling_is_a_rectangle_of_cells() {
        let ground = ground_lines(40, 12);
        assert_eq!(ground.len(), 12);
        for line in &ground {
            assert_eq!(line.chars().count(), 40, "a line came out ragged");
        }
        // The unity line runs the width of the plot.
        let unity = &ground[row_of(0.0, 12)];
        assert!(
            unity.chars().filter(|c| *c == '-' || *c == '+').count() > 30,
            "the unity line is not drawn across: {unity:?}"
        );
        // Three decades are ruled, top to bottom.
        for line in &ground {
            assert_eq!(
                line.chars().filter(|c| *c == ':' || *c == '+').count(),
                3,
                "a row is missing its decades"
            );
        }
    }

    /// A level's row falls where it should, and never off the grid.
    #[test]
    fn a_level_lands_on_its_own_row() {
        assert_eq!(row_of(TOP_DB, 12), 0);
        assert_eq!(row_of(FLOOR_DB, 12), 11);
        assert!(row_of(0.0, 12) > 0 && row_of(0.0, 12) < 11);
        // Off either end of the window is clamped onto the grid.
        assert_eq!(row_of(999.0, 12), 0);
        assert_eq!(row_of(-999.0, 12), 11);
    }

    /// The trace is drawn from the section's own response: parked, it
    /// is a wire at 0 dB; brought in, it is not.
    #[test]
    fn the_trace_follows_the_response_the_core_runs() {
        use crate::console::SectionKind;
        use crate::console::cut_curve as cc;
        use crate::params::console::cut as p;
        let mut params = crate::console::SectionParams::of(SectionKind::Cut);
        let flat = cc::Shape::of(&params);
        assert!(flat.is_off());
        for hz in [20.0f32, 200.0, 2_000.0, 20_000.0] {
            assert!(
                cc::response_db(&flat, hz).abs() < 0.01,
                "a parked pair of blades bent the wire at {hz} Hz"
            );
        }
        params.set(p::HP_HZ, 1_000.0);
        let cut = cc::Shape::of(&params);
        assert!(
            cc::response_db(&cut, 50.0) < -12.0,
            "a high-pass at 1k left 50 Hz alone"
        );
        // Not zero up top: the low-pass parked at 20 kHz is a two-pole
        // at 20 kHz, so its skirt is already worth about a dB at 10 —
        // the analytic response says so and the card draws what it says.
        assert!(
            cc::response_db(&cut, 10_000.0).abs() < 2.0,
            "the passband moved further than the parked blade explains"
        );
    }
}
