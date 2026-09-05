//! TONE's face: the response the engine is running, and the two things
//! this desk's equaliser does that a clean one does not.
//!
//! The curve is the control. It is drawn from `tone_curve::response_db`
//! — the same coefficients the core is running, read at a frequency —
//! so a kill looks like the 24 dB/octave cut it actually is rather than
//! like a shelf pulled down, and the mid's bell narrows on screen
//! because the kernel narrows it, not because the drawing was told to.
//!
//! Two readings on the right say what a picture cannot:
//!
//! - **Q**, live. The mid's width is proportional to its gain — broad at
//!   a nudge, aimed at a push, and half again as narrow on a cut, the
//!   way a passive desk cuts. Turning the gain moves this number.
//! - **IRON**. Past +6 dB the boosted band is driven through the
//!   transformer, harder with every dB. A big low boost on this desk is
//!   thick, not clean, and the heat says by how much.

use super::*;
use crate::ui::chrome;

/// One band's row in the right-hand column.
/// @tune 10..30 px
const ROW_H: f32 = 17.0;
/// The gap between the two columns.
/// @tune 2..24 px
const COLUMN_GAP: f32 = 7.0;
/// The share of the glass the response takes. It is the control.
/// @tune 0.4..0.8
const CURVE_SHARE: f32 = 0.56;
/// The room every row keeps for its word, before its picture starts.
/// The longest of them is IRON, and a bar that began before that word
/// ended would be drawn under it.
/// @tune 16..64 px
const GUTTER: f32 = 32.0;
/// The kill cell's width at the end of a band's row.
/// @tune 12..48 px
const KILL_W: f32 = 26.0;
/// The response's window, in dB either way.
const SPAN_DB: f32 = 18.0;
/// The rate the response is drawn at. The curve's shape over the audible
/// decades is what is being read, and it does not move with the device.
const DRAWN_AT: f32 = 48_000.0;
/// The decades the plot covers.
const HZ_MIN: f32 = 20.0;
const HZ_MAX: f32 = 20_000.0;

/// TONE's seven parameters and the rectangle each one owns.
#[derive(Clone, Copy, Debug)]
struct ToneFace {
    curve: egui::Rect,
    lo: egui::Rect,
    mid: egui::Rect,
    hi: egui::Rect,
    kill_lo: egui::Rect,
    kill_mid: egui::Rect,
    kill_hi: egui::Rect,
    mid_hz: egui::Rect,
    /// The two readings. Not controls: they are what the controls did.
    science: egui::Rect,
}

impl ToneFace {
    fn controls(self) -> [(u32, egui::Rect); 7] {
        use crate::params::console::tone as p;
        [
            (p::LO, self.lo),
            (p::MID, self.mid),
            (p::HI, self.hi),
            (p::MID_HZ, self.mid_hz),
            (p::KILL_LO, self.kill_lo),
            (p::KILL_MID, self.kill_mid),
            (p::KILL_HI, self.kill_hi),
        ]
    }
}

impl Layout for ToneFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        ToneFace::controls(*self).to_vec()
    }
}

fn tone_face(glass: egui::Rect) -> ToneFace {
    let x = glass.shrink2(egui::vec2(5.0, 2.0));
    let curve_w = (x.width() - COLUMN_GAP) * crate::tune!(CURVE_SHARE);
    let curve = egui::Rect::from_min_max(x.min, egui::pos2(x.left() + curve_w, x.bottom()));
    let right = egui::Rect::from_min_max(egui::pos2(curve.right() + COLUMN_GAP, x.top()), x.max);
    let row_h = crate::tune!(ROW_H);
    let band_row = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(right.left(), right.top() + i as f32 * (row_h + 2.0)),
            egui::vec2(right.width(), row_h),
        )
    };
    // A band's row is its gain, then its kill. Two controls side by
    // side, never one inside the other.
    let split = |row: egui::Rect| {
        let kill_w = crate::tune!(KILL_W).min(row.width() * 0.4);
        (
            egui::Rect::from_min_max(
                row.min,
                egui::pos2(row.right() - kill_w - 3.0, row.bottom()),
            ),
            egui::Rect::from_min_max(egui::pos2(row.right() - kill_w, row.top()), row.max),
        )
    };
    let (lo, kill_lo) = split(band_row(0));
    let (mid, kill_mid) = split(band_row(1));
    let (hi, kill_hi) = split(band_row(2));
    let mid_hz = band_row(3);
    let science =
        egui::Rect::from_min_max(egui::pos2(right.left(), mid_hz.bottom() + 4.0), right.max);
    ToneFace {
        curve,
        lo,
        mid,
        hi,
        kill_lo,
        kill_mid,
        kill_hi,
        mid_hz,
        science,
    }
}

/// Where a frequency stands across the plot, 0..1, on the log scale the
/// ear reads in.
fn place_of_hz(hz: f32) -> f32 {
    ((hz.max(1.0).log10() - HZ_MIN.log10()) / (HZ_MAX.log10() - HZ_MIN.log10())).clamp(0.0, 1.0)
}

pub(super) fn draw(face: &Face<'_>) {
    use crate::console::tone_curve as tc;
    use crate::params::console::tone as p;
    let painter = face.painter;
    let glass = face.glass;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
    let lay = tone_face(glass);
    let shape = tc::Shape::of(&face.params);
    let flat = shape.is_flat();
    // Each band keeps one hue, from the alphabet — never a colour picked
    // here — so a glance says which lever is which.
    let band_ink = [alpha.jeopardy_latent.color, ink, alpha.live.color];
    let mut shapes = Vec::new();

    // ---- The response: the control, and the whole point. -------------
    chrome::panel_variant(
        &mut shapes,
        lay.curve,
        Some(alpha.ground.color),
        alpha.well.color,
        Some((Weight::Hair, edge.gamma_multiply(0.62))),
        0,
    );
    let plot = egui::Rect::from_min_max(
        egui::pos2(lay.curve.left() + 7.0, lay.curve.top() + font.size + 5.0),
        egui::pos2(
            lay.curve.right() - 7.0,
            lay.curve.bottom() - font.size - 5.0,
        ),
    );
    let y_of = |db: f32| plot.center().y - (db / SPAN_DB).clamp(-1.0, 1.0) * plot.height() * 0.5;
    let x_of = |hz: f32| plot.left() + place_of_hz(hz) * plot.width();
    // The decades, and the line the curve is measured against.
    for hz in [100.0f32, 1_000.0, 10_000.0] {
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(x_of(hz), plot.top()),
                egui::pos2(x_of(hz), plot.bottom()),
            ],
            Weight::Hair,
            edge.gamma_multiply(0.30),
        );
    }
    for db in [-12.0f32, 12.0] {
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(plot.left(), y_of(db)),
                egui::pos2(plot.right(), y_of(db)),
            ],
            Weight::Hair,
            edge.gamma_multiply(0.30),
        );
    }
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(plot.left(), y_of(0.0)),
            egui::pos2(plot.right(), y_of(0.0)),
        ],
        Weight::Hair,
        edge.gamma_multiply(0.75),
    );
    let curve: Vec<egui::Pos2> = (0..=72)
        .map(|i| {
            let t = i as f32 / 72.0;
            let hz = HZ_MIN * (HZ_MAX / HZ_MIN).powf(t);
            egui::pos2(
                plot.left() + t * plot.width(),
                y_of(tc::response_db(&shape, DRAWN_AT, hz)),
            )
        })
        .collect();
    chrome::trace(
        &mut shapes,
        &curve,
        Weight::Heavy,
        if flat { edge.gamma_multiply(0.85) } else { ink },
    );
    // Each band's corner, standing on the curve it moves.
    for (i, hz) in [p::LO_HZ, shape.mid_hz, p::HI_HZ].into_iter().enumerate() {
        let killed = [shape.kill_lo, shape.kill_mid, shape.kill_hi][i];
        let at = egui::pos2(x_of(hz), y_of(tc::response_db(&shape, DRAWN_AT, hz)));
        chrome::pad(
            &mut shapes,
            at,
            chrome::PAD,
            if killed {
                alpha.jeopardy_active.color
            } else {
                band_ink[i]
            },
            true,
        );
    }
    painter.extend(std::mem::take(&mut shapes));

    // ---- The three bands, each a bipolar bar and a kill. -------------
    let rows = [
        ("LO", p::LO, shape.lo_db, lay.lo, lay.kill_lo, shape.kill_lo),
        (
            "MID",
            p::MID,
            shape.mid_db,
            lay.mid,
            lay.kill_mid,
            shape.kill_mid,
        ),
        ("HI", p::HI, shape.hi_db, lay.hi, lay.kill_hi, shape.kill_hi),
    ];
    for (i, (word, _id, db, rect, kill_rect, killed)) in rows.into_iter().enumerate() {
        let hue = band_ink[i];
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.left() + crate::tune!(GUTTER), rect.center().y - 4.0),
            egui::pos2(rect.right() - 40.0, rect.center().y + 4.0),
        );
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(bar.left(), bar.center().y),
                egui::pos2(bar.right(), bar.center().y),
            ],
            Weight::Hair,
            edge.gamma_multiply(0.45),
        );
        let centre = bar.center().x;
        let reach = (db / 15.0).clamp(-1.0, 1.0) * bar.width() * 0.5;
        if !killed && reach.abs() > 0.5 {
            shapes.push(egui::Shape::rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(centre.min(centre + reach), bar.top()),
                    egui::pos2(centre.max(centre + reach), bar.bottom()),
                ),
                0.0,
                hue,
            ));
        }
        // The centre post: where the band is doing nothing.
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(centre, bar.top() - 2.0),
                egui::pos2(centre, bar.bottom() + 2.0),
            ],
            Weight::Hair,
            edge,
        );
        // The kill: not a bypass, a 24 dB/octave cut. It reads as the
        // fault hue because a killed band is a band that is gone.
        chrome::panel_variant(
            &mut shapes,
            kill_rect,
            Some(if killed {
                alpha.jeopardy_active.color
            } else {
                alpha.ground.color
            }),
            alpha.ground.color,
            Some((
                Weight::Hair,
                if killed {
                    alpha.jeopardy_active.color
                } else {
                    edge
                },
            )),
            i as u8,
        );
        let _ = word;
    }

    // MID HZ: where the bell is aimed, on the same log scale as the plot.
    chrome::panel_frame_variant(&mut shapes, lay.mid_hz, Weight::Hair, edge, 2);
    let rail = egui::Rect::from_min_max(
        egui::pos2(
            lay.mid_hz.left() + crate::tune!(GUTTER),
            lay.mid_hz.center().y - 3.0,
        ),
        egui::pos2(lay.mid_hz.right() - 46.0, lay.mid_hz.center().y + 3.0),
    );
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(rail.left(), rail.center().y),
            egui::pos2(rail.right(), rail.center().y),
        ],
        Weight::Hair,
        edge.gamma_multiply(0.6),
    );
    let aim = rail.left() + place_of_hz(shape.mid_hz) * rail.width();
    chrome::trace(
        &mut shapes,
        &[
            egui::pos2(aim, rail.top() - 3.0),
            egui::pos2(aim, rail.bottom() + 3.0),
        ],
        Weight::Heavy,
        band_ink[1],
    );

    // ---- The science: proportional Q, and the iron a boost lights. ---
    let q = tc::mid_q(shape.mid_db);
    // How far past the knee any band is boosted: that is what drives the
    // transformer, and the desk does it harder with every dB.
    let over = [shape.lo_db, shape.mid_db, shape.hi_db]
        .into_iter()
        .fold(0.0f32, f32::max)
        - p::IRON_FROM_DB;
    let heat = (over / (15.0 - p::IRON_FROM_DB)).clamp(0.0, 1.0);
    let q_row = egui::Rect::from_min_max(
        lay.science.min,
        egui::pos2(lay.science.right(), lay.science.center().y - 1.0),
    );
    let iron_row = egui::Rect::from_min_max(
        egui::pos2(lay.science.left(), lay.science.center().y + 1.0),
        lay.science.max,
    );
    // Q as a width, not only a number: a bell drawn at the Q the kernel
    // will use, so "narrower on a cut" is a shape that changes.
    let bell = egui::Rect::from_min_max(
        egui::pos2(q_row.left() + crate::tune!(GUTTER), q_row.top() + 2.0),
        egui::pos2(q_row.right() - 40.0, q_row.bottom() - 2.0),
    );
    if bell.is_positive() {
        let width = (1.0 / q).clamp(0.15, 3.0);
        let bell_curve: Vec<egui::Pos2> = (0..=24)
            .map(|i| {
                let t = -1.0 + 2.0 * i as f32 / 24.0;
                let y = (-(t * t) / (width * width * 0.5)).exp();
                egui::pos2(
                    bell.left() + (t + 1.0) * 0.5 * bell.width(),
                    bell.bottom() - y * bell.height(),
                )
            })
            .collect();
        chrome::trace(&mut shapes, &bell_curve, Weight::Hair, band_ink[1]);
    }
    let heat_bar = egui::Rect::from_min_max(
        egui::pos2(
            iron_row.left() + crate::tune!(GUTTER),
            iron_row.center().y - 3.0,
        ),
        egui::pos2(iron_row.right() - 40.0, iron_row.center().y + 3.0),
    );
    if heat_bar.is_positive() {
        chrome::panel_frame_variant(&mut shapes, heat_bar.expand(1.0), Weight::Hair, edge, 1);
        if heat > 0.0 {
            shapes.push(egui::Shape::rect_filled(
                egui::Rect::from_min_max(
                    heat_bar.min,
                    egui::pos2(heat_bar.left() + heat * heat_bar.width(), heat_bar.bottom()),
                ),
                0.0,
                tool::mix_ink(ink, alpha.jeopardy_latent.color, heat),
            ));
        }
    }
    painter.extend(shapes);

    // ---- The labels. One size, measured before they are drawn. -------
    let label = |at: egui::Pos2, align: egui::Align2, text: String, ink| {
        painter.text(at, align, text, font.clone(), ink);
    };
    const PAD_X: f32 = 8.0;
    label(
        egui::pos2(lay.curve.left() + PAD_X, lay.curve.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "RESPONSE".to_owned(),
        edge,
    );
    label(
        egui::pos2(lay.curve.right() - PAD_X, lay.curve.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        if flat { "FLAT" } else { "IN" }.to_owned(),
        if flat { edge } else { alpha.live.color },
    );
    for (hz, word) in [(100.0f32, "100"), (1_000.0, "1k"), (10_000.0, "10k")] {
        label(
            egui::pos2(x_of(hz), lay.curve.bottom() - 3.0),
            egui::Align2::CENTER_BOTTOM,
            word.to_owned(),
            edge,
        );
    }
    for (i, (word, db, rect, kill_rect, killed)) in [
        ("LO", shape.lo_db, lay.lo, lay.kill_lo, shape.kill_lo),
        ("MID", shape.mid_db, lay.mid, lay.kill_mid, shape.kill_mid),
        ("HI", shape.hi_db, lay.hi, lay.kill_hi, shape.kill_hi),
    ]
    .into_iter()
    .enumerate()
    {
        label(
            egui::pos2(rect.left() + 2.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            word.to_owned(),
            band_ink[i],
        );
        label(
            egui::pos2(rect.right() - 3.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            if killed {
                "--".to_owned()
            } else {
                format!("{db:+.1}")
            },
            if killed { edge } else { ink },
        );
        label(
            kill_rect.center(),
            egui::Align2::CENTER_CENTER,
            "X".to_owned(),
            if killed { alpha.ground.color } else { edge },
        );
    }
    label(
        egui::pos2(lay.mid_hz.left() + 2.0, lay.mid_hz.center().y),
        egui::Align2::LEFT_CENTER,
        "AT".to_owned(),
        edge,
    );
    label(
        egui::pos2(lay.mid_hz.right() - 3.0, lay.mid_hz.center().y),
        egui::Align2::RIGHT_CENTER,
        if shape.mid_hz >= 1000.0 {
            format!("{:.1}k", shape.mid_hz / 1000.0)
        } else {
            format!("{:.0}", shape.mid_hz)
        },
        ink,
    );
    label(
        egui::pos2(q_row.left() + 2.0, q_row.center().y),
        egui::Align2::LEFT_CENTER,
        "Q".to_owned(),
        edge,
    );
    label(
        egui::pos2(q_row.right() - 3.0, q_row.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{q:.2}"),
        ink,
    );
    label(
        egui::pos2(iron_row.left() + 2.0, iron_row.center().y),
        egui::Align2::LEFT_CENTER,
        "IRON".to_owned(),
        edge,
    );
    label(
        egui::pos2(iron_row.right() - 3.0, iron_row.center().y),
        egui::Align2::RIGHT_CENTER,
        if heat > 0.0 {
            format!("{:.1}x", 1.0 + p::IRON_DRIVE * heat)
        } else {
            "--".to_owned()
        },
        if heat > 0.0 {
            tool::mix_ink(ink, alpha.jeopardy_latent.color, heat)
        } else {
            edge
        },
    );

    face.mark(&lay);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glass() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 210.0))
    }

    /// Every parameter has one instrument, none of them overlapping,
    /// and all of them on the glass.
    #[test]
    fn every_tone_parameter_has_its_own_instrument() {
        use crate::params::console::tone as p;
        let face = tone_face(glass());
        let controls = face.controls();
        assert_eq!(
            controls.map(|(id, _)| id),
            [
                p::LO,
                p::MID,
                p::HI,
                p::MID_HZ,
                p::KILL_LO,
                p::KILL_MID,
                p::KILL_HI
            ]
        );
        for (index, (id, rect)) in controls.iter().enumerate() {
            assert!(
                glass().contains_rect(*rect),
                "parameter {id} left the glass"
            );
            assert!(rect.is_positive(), "parameter {id} lost its instrument");
            assert!(
                !face.curve.intersects(*rect),
                "parameter {id} invaded the response"
            );
            for (other, other_rect) in &controls[index + 1..] {
                assert!(
                    !rect.intersects(*other_rect),
                    "parameters {id} and {other} overlap"
                );
            }
        }
    }

    /// The response is the control, so it is the biggest thing here.
    #[test]
    fn the_response_owns_most_of_the_glass() {
        let face = tone_face(glass());
        assert!(face.curve.width() > glass().width() * 0.45);
        assert!(face.curve.height() > face.lo.height() * 4.0);
        // A band's gain and its kill stand side by side, never nested.
        assert!(face.lo.right() <= face.kill_lo.left());
        assert_eq!(face.lo.y_range(), face.kill_lo.y_range());
    }

    /// The frequency scale is the one the ear reads in: a decade takes
    /// the same room wherever it falls.
    #[test]
    fn the_scale_is_logarithmic() {
        let a = place_of_hz(100.0) - place_of_hz(20.0);
        let b = place_of_hz(1_000.0) - place_of_hz(200.0);
        assert!((a - b).abs() < 0.01, "{a} against {b}");
        assert_eq!(place_of_hz(HZ_MIN), 0.0);
        assert_eq!(place_of_hz(HZ_MAX), 1.0);
    }
}
