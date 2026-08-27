//! Filter response curve: the magnitude a filter applies, drawn the way
//! every modern EQ draws it.
//!
//! # What "modern standards" means here, concretely
//!
//! - **Log frequency, linear dB.** 20 Hz to 20 kHz across, ±dB up. Both
//!   axes match how hearing works, which is why every analyser since the
//!   1970s uses them and why a linear-frequency plot is unreadable.
//! - **The DIGITAL response, not the analogue prototype.** The curve is
//!   evaluated on the unit circle from bilinear-transformed coefficients,
//!   so it shows the real thing — including the way a lowpass steepens and
//!   pins toward Nyquist. A textbook `1/sqrt(1+w^2)` curve is a lie about
//!   what the audio will do at 18 kHz.
//! - **A filled curve** under the line, a stressed 0 dB reference, octave
//!   grid lines with sparse labels, and a draggable node at the cutoff.
//! - **Cascaded Butterworth sections** for the slope, with the resonance
//!   applied to the last section only — which is what puts the peak AT
//!   the corner instead of smearing it across the whole cascade.
//!
//! # Slopes
//!
//! 6 dB/octave per pole, so the list is orders 1 through 8. Odd orders
//! are one real pole plus biquads; even orders are biquads alone, and the
//! two cases need DIFFERENT pole angles — see [`butterworth_q`], where
//! using one formula for both is a mistake that looks tidy and puts a
//! third-order filter 7.8 dB down at its own corner. The last section is
//! the resonant one.
//!
//! # Nonlinear distortion — and what a magnitude curve can honestly show
//!
//! A driven filter is not a linear system, so it does not strictly HAVE a
//! transfer function. Two of its effects are real and visible on a
//! measured response, and this draws both:
//!
//! 1. **Resonance compression.** The saturating element sits inside the
//!    feedback path, so as drive rises the peak squashes and the effective
//!    Q falls. This is the difference between a digital filter screaming
//!    at self-oscillation and an analogue one growling.
//! 2. **Stopband fill.** The nonlinearity generates harmonics that were
//!    not in the input, so an analyser sweeping a driven filter never
//!    measures the clean 48 dB/octave the coefficients promise — the
//!    stopband floors out where the distortion products live.
//!
//! What it does NOT draw is a wiggle standing in for "distortion". Where
//! the harmonics land depends on the input, which a response curve does
//! not know, and inventing a shape for it would be decoration.

use crate::params::filter as fp;
use crate::ui::device::metrics::Footprint;
use crate::ui::device::{
    Mapping, Param, ParamEdit, Unit, Well, Wells, adjust, card, design, metrics, poly_widgets,
    switch,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// The view: 20 Hz to 20 kHz, and the dB window above and below unity.
pub const VIEW_MIN_HZ: f32 = 20.0;
pub const VIEW_MAX_HZ: f32 = 20_000.0;
pub const VIEW_TOP_DB: f32 = 24.0;
pub const VIEW_BOTTOM_DB: f32 = -36.0;

/// The sparse frequencies that get a label — the THUMBNAIL's, which
/// still draws an axis because it stands alone in a rack card rather
/// than under its own labelled value.
///
/// The full display's grid constants went with the Elektron pass: that
/// box has no rules inside it, so there was nothing left to space.
const LABEL_HZ: [f32; 3] = [100.0, 1_000.0, 10_000.0];

/// Curve resolution. One point every this many screen points: fine enough
/// that a 48 dB/octave corner is a corner and not a chamfer.
const CURVE_STEP_PX: f32 = 2.0;

/// Butterworth Q, which is where a cascade with no resonance sits. Shared
/// with the engine through the param table so the drawing and the audio
/// cannot disagree about where flat is.
const FLAT_Q: f32 = crate::params::filter::FLAT_Q;
/// Where the stopband floors out at full drive, in dB. Real driven
/// filters measure somewhere around here; a clean one keeps falling.
const DRIVE_FLOOR_DB: f32 = -42.0;
/// The bottom of the arithmetic: below this the magnitude is called
/// silence. Deep enough that an eight-pole cascade three octaves past its
/// corner is still a real number rather than a clamp — measuring a slope
/// against a clamped value reads 35 dB/octave for a 48 dB/octave filter,
/// which is how `each_slope_falls_at_the_rate_it_claims` found it.
const SILENT_DB: f32 = -240.0;
const SILENT: f32 = 1e-12;

// --------------------------------------------------------------- spec ---

/// What the filter is doing. Everything the curve needs and nothing about
/// how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Filter {
    pub mode: Mode,
    pub slope: Slope,
    /// Corner frequency in Hz.
    pub cutoff_hz: f32,
    /// Resonance as a Q. [`FLAT_Q`] is Butterworth — no peak.
    pub q: f32,
    /// Nonlinearity, `0..=1`. 0 is a clean linear filter.
    pub drive: f32,
    /// Which VOICING, as an index into
    /// [`CHARACTERS`](crate::params::filter::CHARACTERS).
    ///
    /// Two of the four numbers a character carries are visible on a
    /// magnitude curve — how hard the drive squashes the resonance, and
    /// how much level the resonance eats — so both are drawn. The other
    /// two say which saturating curve and how asymmetric it is, which is
    /// where the harmonics land rather than what the response does, and a
    /// magnitude plot has no honest way to show either.
    pub character: u32,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            mode: Mode::Lowpass,
            slope: Slope::Db24,
            cutoff_hz: 1_000.0,
            q: FLAT_Q,
            drive: 0.0,
            character: crate::params::filter::CHAR_CLEAN,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
}

impl Mode {
    pub const ALL: [Self; 4] = [Self::Lowpass, Self::Highpass, Self::Bandpass, Self::Notch];
    pub const NAMES: &'static [&'static str] = &["lp", "hp", "bp", "notch"];

    pub fn from_index(i: usize) -> Self {
        Self::ALL[i.min(Self::ALL.len() - 1)]
    }

    /// Bandpass and notch are defined by a single resonant section; there
    /// is no "24 dB/octave notch" in the sense the slope list means, so
    /// they ignore it rather than pretending.
    pub fn uses_slope(self) -> bool {
        matches!(self, Self::Lowpass | Self::Highpass)
    }

    /// This mode's index on the wire — the number the engine's parameter
    /// table names it by. Same order as [`ALL`](Self::ALL), which is what
    /// lets the card turn a choice cell straight into a mode.
    pub fn wire(self) -> u32 {
        use crate::params::filter as fp;
        match self {
            Self::Lowpass => fp::MODE_LP,
            Self::Highpass => fp::MODE_HP,
            Self::Bandpass => fp::MODE_BP,
            Self::Notch => fp::MODE_NOTCH,
        }
    }
}

/// Filter steepness. Six dB per octave per pole, which is what makes the
/// list orders one through eight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slope {
    Db6,
    Db12,
    Db18,
    Db24,
    Db36,
    Db48,
}

impl Slope {
    pub const ALL: [Self; 6] = [
        Self::Db6,
        Self::Db12,
        Self::Db18,
        Self::Db24,
        Self::Db36,
        Self::Db48,
    ];
    pub const NAMES: &'static [&'static str] = &["6", "12", "18", "24", "36", "48"];

    pub fn from_index(i: usize) -> Self {
        Self::ALL[i.min(Self::ALL.len() - 1)]
    }

    pub fn db_per_octave(self) -> f32 {
        self.order() as f32 * 6.0
    }

    /// Filter order — the number of poles.
    pub fn order(self) -> u32 {
        match self {
            Self::Db6 => 1,
            Self::Db12 => 2,
            Self::Db18 => 3,
            Self::Db24 => 4,
            Self::Db36 => 6,
            Self::Db48 => 8,
        }
    }
}

// --------------------------------------------------------------- math ---

/// One biquad's coefficients, normalized so `a0` is 1.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Biquad {
    /// Magnitude at digital angular frequency `w` (radians per sample).
    ///
    /// Evaluated on the unit circle rather than from an analogue formula:
    /// `H(e^{jw})` is what the audio actually gets, warping near Nyquist
    /// included.
    fn magnitude(&self, w: f32) -> f32 {
        let (c1, s1) = (w.cos(), w.sin());
        let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
        // e^{-jw} = cos w - j sin w, so the imaginary parts carry a minus.
        let num_re = self.b0 + self.b1 * c1 + self.b2 * c2;
        let num_im = -(self.b1 * s1 + self.b2 * s2);
        let den_re = 1.0 + self.a1 * c1 + self.a2 * c2;
        let den_im = -(self.a1 * s1 + self.a2 * s2);
        let num = (num_re * num_re + num_im * num_im).sqrt();
        let den = (den_re * den_re + den_im * den_im).sqrt();
        if den <= f32::MIN_POSITIVE {
            0.0
        } else {
            num / den
        }
    }
}

/// RBJ cookbook coefficients — the ones every digital filter in every DAW
/// is built from, so the picture matches the sound.
fn biquad(mode: Mode, w0: f32, q: f32) -> Biquad {
    let q = q.max(0.05);
    let (sin0, cos0) = (w0.sin(), w0.cos());
    let alpha = sin0 / (2.0 * q);
    let a0 = 1.0 + alpha;
    let norm = |v: f32| v / a0;
    let (b0, b1, b2) = match mode {
        Mode::Lowpass => {
            let k = (1.0 - cos0) * 0.5;
            (k, 1.0 - cos0, k)
        }
        Mode::Highpass => {
            let k = (1.0 + cos0) * 0.5;
            (k, -(1.0 + cos0), k)
        }
        // Constant PEAK gain (unity at the corner), which is the one an
        // EQ display wants: the other normalization peaks at Q and makes
        // a high-resonance bandpass look like a boost.
        Mode::Bandpass => (alpha, 0.0, -alpha),
        Mode::Notch => (1.0, -2.0 * cos0, 1.0),
    };
    Biquad {
        b0: norm(b0),
        b1: norm(b1),
        b2: norm(b2),
        a1: norm(-2.0 * cos0),
        a2: norm(1.0 - alpha),
    }
}

/// One RBJ section's magnitude at `w`, for a display that is not this
/// filter's.
///
/// The equaliser's cuts and notches are the same sections at the same
/// coefficients, so they read the response from here rather than growing
/// a second cookbook that could disagree with this one. Its bells and
/// shelves come from [`crate::dsp::filters::EqBand::coeffs`] instead —
/// there the kernel itself is the authority, because it is the kernel
/// that carries the gain.
pub(crate) fn section_magnitude(mode: Mode, w0: f32, q: f32, w: f32) -> f32 {
    biquad(mode, w0, q).magnitude(w)
}

/// `|H(e^{jw})|` of a normalised biquad `[b0, b1, b2, a1, a2]`.
pub(crate) fn magnitude_of(c: [f32; 5], w: f32) -> f32 {
    Biquad {
        b0: c[0],
        b1: c[1],
        b2: c[2],
        a1: c[3],
        a2: c[4],
    }
    .magnitude(w)
}

/// The Q of quadratic section `k` in an order-`n` Butterworth cascade,
/// for callers outside this module. See [`butterworth_q`].
pub(crate) fn cascade_section_q(order: u32, k: u32) -> f32 {
    butterworth_q(order, k)
}

/// A first-order section, for odd orders. Bilinear-transformed the same
/// way, so it lines up with the biquads it is cascaded with.
fn one_pole_magnitude(mode: Mode, w0: f32, w: f32) -> f32 {
    // Pre-warp so the corner lands where it was asked for.
    let k = (w0 * 0.5).tan();
    let (b0, b1, a1) = match mode {
        Mode::Highpass => (1.0 / (1.0 + k), -1.0 / (1.0 + k), (k - 1.0) / (k + 1.0)),
        _ => (k / (1.0 + k), k / (1.0 + k), (k - 1.0) / (k + 1.0)),
    };
    let (c1, s1) = (w.cos(), w.sin());
    let num_re = b0 + b1 * c1;
    let num_im = -(b1 * s1);
    let den_re = 1.0 + a1 * c1;
    let den_im = -(a1 * s1);
    let den = (den_re * den_re + den_im * den_im).sqrt();
    if den <= f32::MIN_POSITIVE {
        0.0
    } else {
        (num_re * num_re + num_im * num_im).sqrt() / den
    }
}

/// The Q of quadratic section `k` in an order-`n` Butterworth cascade.
///
/// `1 / (2 cos φ)`, where φ is the pole's angle from the negative real
/// axis — and φ is NOT the same expression for even and odd orders. An
/// even order has no real pole, so its pairs sit at `(2k+1)π/2n`; an odd
/// order spends one pole on the real axis and its pairs sit at `(k+1)π/n`.
///
/// Using the even formula throughout is the tidy-looking mistake: it gives
/// a third-order filter Q = 0.577 instead of 1.0, which is −7.8 dB at the
/// corner instead of −3. Every textbook table lists 1.0 for that section,
/// and `a_flat_cascade_is_three_db_down_at_the_corner` is what caught it.
fn butterworth_q(order: u32, k: u32) -> f32 {
    let n = order.max(1) as f32;
    let angle = if order.is_multiple_of(2) {
        (2.0 * k as f32 + 1.0) * std::f32::consts::PI / (2.0 * n)
    } else {
        (k as f32 + 1.0) * std::f32::consts::PI / n
    };
    1.0 / (2.0 * angle.cos())
}

/// The resonant section's Q, after the user's resonance and the
/// nonlinearity have both had their say. The mapping itself lives in
/// [`crate::params::filter`], because the audio path applies the very same
/// function to its resonant section — agreement by construction.
fn resonant_q(q: f32, base: f32, drive: f32, character: u32) -> f32 {
    crate::params::filter::resonant_q_for(q, base, drive, character)
}

/// The filter's magnitude at `hz`, in dB.
///
/// Pure and sample-rate aware: the same numbers the audio would produce,
/// which is the only reason to draw a curve at all.
pub fn magnitude_db(filter: &Filter, hz: f32, sample_rate: f32) -> f32 {
    let nyquist = sample_rate * 0.5;
    // NaN fails every comparison, so this rejects it too — which is the
    // point: a NaN frequency would become a NaN point and egui draws that
    // as nothing at all, i.e. a curve that silently disappears.
    if !hz.is_finite() || hz <= 0.0 || !sample_rate.is_finite() || sample_rate <= 0.0 {
        return VIEW_BOTTOM_DB;
    }
    // Above Nyquist there is nothing to say; hold the last real value
    // rather than wrapping around the unit circle and drawing a mirror
    // image, which is the classic way these plots go wrong.
    let hz = hz.min(nyquist * 0.999);
    let cutoff = filter.cutoff_hz.clamp(1.0, nyquist * 0.999);
    let w = std::f32::consts::TAU * hz / sample_rate;
    let w0 = std::f32::consts::TAU * cutoff / sample_rate;

    let order = if filter.mode.uses_slope() {
        filter.slope.order()
    } else {
        2
    };
    let biquads = order / 2;
    let mut mag = 1.0f32;

    for k in 0..biquads {
        let base = butterworth_q(order, k);
        // The LAST section carries the resonance. Spreading it over every
        // section widens the peak into a hump and stops it sitting at the
        // corner, which is exactly what a resonant filter must not do.
        let q = if k + 1 == biquads {
            resonant_q(filter.q, base, filter.drive, filter.character)
        } else {
            base
        };
        mag *= biquad(filter.mode, w0, q).magnitude(w);
    }
    if !order.is_multiple_of(2) {
        mag *= one_pole_magnitude(filter.mode, w0, w);
    }
    // What the voicing gives up as the resonance comes up. The node
    // multiplies its filtered block by this very function, so the two
    // cannot drift.
    mag *= crate::params::filter::resonance_loss(filter.q, filter.character, filter.mode.wire());

    let db = 20.0 * mag.max(SILENT).log10();
    // Stopband fill: the nonlinearity puts energy where the coefficients
    // say there is none, so a measured curve floors out.
    //
    // Interpolated UP from silence, not down from the view: a clean filter
    // has no floor at all, and full drive puts one at DRIVE_FLOOR_DB.
    // Running it the other way (from the bottom of the view down to the
    // drive floor) makes more drive mean a LOWER floor, which is backwards
    // and reads as drive cleaning the filter up.
    let drive = filter.drive.clamp(0.0, 1.0);
    if drive > 0.0 {
        db.max(SILENT_DB + (DRIVE_FLOOR_DB - SILENT_DB) * drive)
    } else {
        db
    }
}

// ------------------------------------------------------------ geometry ---

/// Where `hz` sits across the view, `0..=1`, on a log axis.
pub fn hz_to_norm(hz: f32) -> f32 {
    let hz = hz.clamp(VIEW_MIN_HZ, VIEW_MAX_HZ);
    (hz / VIEW_MIN_HZ).ln() / (VIEW_MAX_HZ / VIEW_MIN_HZ).ln()
}

/// The inverse, for turning a pointer position back into a frequency.
pub fn norm_to_hz(t: f32) -> f32 {
    VIEW_MIN_HZ * (VIEW_MAX_HZ / VIEW_MIN_HZ).powf(t.clamp(0.0, 1.0))
}

/// The dB window a curve is drawn in.
///
/// There is ONE, and the mini shares it. I tried giving the thumbnail a
/// tighter window on the theory that 60 dB squeezed into forty points
/// turns every steep filter into the same cliff — then measured it, and
/// the theory did not survive. A Q of 8 peaks at +18.1 dB and a Q of 12
/// at +21.6, so any window tight enough to be worth having clips ordinary
/// resonance settings; and the cliff, where it exists, comes from the
/// WIDTH — ten octaves in 96 points — which a taller window would not fix
/// either.
///
/// A thumbnail that disagreed with the full display about vertical scale
/// would also be a thumbnail that lies about what you get when you open
/// the big one. This stays a type because it makes the clamping explicit
/// and testable, and because a zoomable axis is the obvious next ask.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub top_db: f32,
    pub bottom_db: f32,
}

impl View {
    /// The full display: enough range to read a rolloff into the floor.
    pub const FULL: Self = Self {
        top_db: VIEW_TOP_DB,
        bottom_db: VIEW_BOTTOM_DB,
    };
    /// Where `db` sits up this view, `0..=1`.
    pub fn db_to_norm(self, db: f32) -> f32 {
        let span = self.top_db - self.bottom_db;
        if span <= 0.0 {
            return 0.0;
        }
        ((db - self.bottom_db) / span).clamp(0.0, 1.0)
    }
}

/// Where `db` sits up the full view, `0..=1`.
pub fn db_to_norm(db: f32) -> f32 {
    View::FULL.db_to_norm(db)
}

// --------------------------------------------------------------- view ---

/// The display's size contract: at least an XY pad wide, a spectrum tall.
/// A minimum — it stretches into whatever width it is given, and a
/// response curve wants every pixel it can get.
pub fn footprint(theme: &Theme) -> Footprint {
    Footprint::new(theme.sp(control::XY_PAD), theme.sp(control::SPECTRUM_H))
}

/// Draw the curve, with a draggable node at the corner.
///
/// Horizontal drag moves the cutoff, vertical drag moves the resonance —
/// the gesture every filter display uses, and the reason the node is
/// worth having at all. Wheel adjusts resonance. Returns true when the
/// user changed something.
pub fn filter_curve(
    ui: &mut egui::Ui,
    theme: &Theme,
    filter: &mut Filter,
    sample_rate: f32,
) -> bool {
    let min = footprint(theme);
    let size = egui::vec2(ui.available_width().max(min.width()), min.height());
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let changed = edit_curve(ui, &response, rect, filter, sample_rate);

    paint(ui, theme, rect, filter, sample_rate, &response);
    changed
}

/// A still-editable response curve for the poly synth's single-page face.
/// It keeps the full curve's drag grammar, but uses the thumbnail's quiet
/// grid so the short display does not turn into a knot of axis labels.
pub fn filter_curve_compact(
    ui: &mut egui::Ui,
    theme: &Theme,
    filter: &mut Filter,
    sample_rate: f32,
) -> bool {
    let size = egui::vec2(
        ui.available_width().max(theme.sp(control::CURVE_MINI_W)),
        theme.sp(control::CURVE_COMPACT_H),
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let changed = edit_curve(ui, &response, rect, filter, sample_rate);
    paint_compact(ui, theme, rect, filter, sample_rate, &response);
    changed
}

/// The compact response renderer stretched into an exact parent surface.
///
/// The poly synth embeds its parameter cells in the same dark panel, so the
/// curve takes the full plot rectangle left above that footer instead of
/// keeping the old fixed thumbnail height.
pub fn filter_curve_fill(
    ui: &mut egui::Ui,
    theme: &Theme,
    filter: &mut Filter,
    sample_rate: f32,
) -> bool {
    let size = egui::vec2(ui.available_width(), ui.available_height());
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let changed = edit_curve(ui, &response, rect, filter, sample_rate);
    paint_compact(ui, theme, rect, filter, sample_rate, &response);
    changed
}

fn edit_curve(
    ui: &egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
    filter: &mut Filter,
    sample_rate: f32,
) -> bool {
    let nyquist = (sample_rate * 0.5).max(VIEW_MIN_HZ * 2.0);
    let mut changed = false;

    if response.dragged() {
        let d = response.drag_delta();
        if d.x != 0.0 {
            let t = hz_to_norm(filter.cutoff_hz) + d.x / rect.width();
            let next = norm_to_hz(t).clamp(VIEW_MIN_HZ, nyquist.min(VIEW_MAX_HZ));
            if next != filter.cutoff_hz {
                filter.cutoff_hz = next;
                changed = true;
            }
        }
        if d.y != 0.0 {
            // Up is more resonant. The range runs from flat to steep
            // enough to ring, logarithmically, because the interesting
            // part of a Q control is all at the bottom.
            let t = q_to_norm(filter.q) - d.y / rect.height();
            let next = norm_to_q(t);
            if next != filter.q {
                filter.q = next;
                changed = true;
            }
        }
    }
    let nudge = adjust::nudge(ui, response);
    if nudge != 0.0 {
        let next = norm_to_q(q_to_norm(filter.q) + nudge);
        if next != filter.q {
            filter.q = next;
            changed = true;
        }
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    changed
}

/// Resonance as a `0..=1` position. Log-mapped: a Q control spends most
/// of its travel between 0.5 and 2, and almost none of it above 10.
const Q_MIN: f32 = crate::params::filter::TABLE[crate::params::filter::RES as usize].min;
const Q_MAX: f32 = crate::params::filter::TABLE[crate::params::filter::RES as usize].max;

fn q_to_norm(q: f32) -> f32 {
    let q = q.clamp(Q_MIN, Q_MAX);
    (q / Q_MIN).ln() / (Q_MAX / Q_MIN).ln()
}

fn norm_to_q(t: f32) -> f32 {
    Q_MIN * (Q_MAX / Q_MIN).powf(t.clamp(0.0, 1.0))
}

/// The curve as screen points across `rect`, in `view`.
///
/// Shared by both renderings: the mini is the same filter, the same maths
/// and the same axis — only the chrome and the window differ. Two copies
/// of this would be two filters that drift apart.
fn curve_points(
    rect: egui::Rect,
    filter: &Filter,
    sample_rate: f32,
    view: View,
) -> Vec<egui::Pos2> {
    let steps = ((rect.width() / CURVE_STEP_PX).ceil() as usize).max(2);
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let db = magnitude_db(filter, norm_to_hz(t), sample_rate);
            egui::pos2(
                rect.left() + rect.width() * t,
                rect.bottom() - rect.height() * view.db_to_norm(db),
            )
        })
        .collect()
}

/// Fill under a curve, as ONE mesh.
///
/// Not a polygon: a filter response is not convex — a resonant peak alone
/// rules that out — and `convex_polygon` on a concave outline does not
/// fail, it draws a wrong shape, a wedge slicing across the plot. Not one
/// quad per segment either: the fill is translucent, so neighbouring
/// quads double-blend along the edge they share and the whole area comes
/// out striped. A single mesh shares its vertices, so there are no
/// interior edges to blend twice.
fn fill_under(painter: &egui::Painter, points: &[egui::Pos2], bottom: f32, color: egui::Color32) {
    let mut mesh = egui::Mesh::default();
    for p in points {
        mesh.colored_vertex(*p, color);
        mesh.colored_vertex(egui::pos2(p.x, bottom), color);
    }
    for i in 0..points.len().saturating_sub(1) {
        let top = (i * 2) as u32;
        mesh.add_triangle(top, top + 1, top + 2);
        mesh.add_triangle(top + 1, top + 3, top + 2);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// The mini's size contract: a fixed thumbnail, not a stretchy display.
///
/// Fixed on purpose. The full curve takes every pixel it can get because
/// more width is more resolution; a thumbnail sitting in a well beside
/// its knobs wants to be the same size as the thing next to it, every
/// time, on every device.
pub fn footprint_mini(theme: &Theme) -> Footprint {
    Footprint::new(
        theme.sp(control::CURVE_MINI_W),
        theme.sp(control::CURVE_MINI_H),
    )
}

/// A thumbnail of the response: the shape, and nothing else.
///
/// DISPLAY ONLY, deliberately. A mini lives in a device well with the
/// cutoff and resonance knobs an inch away, and forty points of height is
/// a worse Q control than the knob beside it — offering the gesture
/// anyway would mean offering a bad version of something already good.
/// The full [`filter_curve`] is the one you drag.
pub fn mini(ui: &mut egui::Ui, theme: &Theme, filter: &Filter, sample_rate: f32) {
    let (rect, _) = ui.allocate_exact_size(footprint_mini(theme).size, egui::Sense::hover());
    let painter = ui.painter();
    // The SAME window as the full display, so the thumbnail cannot
    // disagree with what opening it will show.
    let view = View::FULL;
    painter.rect_filled(rect, design::box_radius(), theme.surface_sunken);

    // Three grid lines and no labels. At this size a label is a smudge,
    // and the decade lines alone are enough to say which way is treble.
    for hz in LABEL_HZ {
        let x = rect.left() + rect.width() * hz_to_norm(hz);
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(stroke::HAIR, theme.grid_sub),
        );
    }
    let unity = rect.bottom() - rect.height() * view.db_to_norm(0.0);
    painter.line_segment(
        [
            egui::pos2(rect.left(), unity),
            egui::pos2(rect.right(), unity),
        ],
        egui::Stroke::new(stroke::HAIR, theme.grid_beat),
    );

    let points = curve_points(rect, filter, sample_rate, view);
    fill_under(painter, &points, rect.bottom(), theme.accent_muted);
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(stroke::HAIR, theme.accent),
    ));

    // A dot at the corner instead of a handle: it says where the cutoff
    // is without pretending to be draggable.
    let node = egui::pos2(
        rect.left() + rect.width() * hz_to_norm(filter.cutoff_hz),
        rect.bottom()
            - rect.height() * view.db_to_norm(magnitude_db(filter, filter.cutoff_hz, sample_rate)),
    );
    painter.circle_filled(node, stroke::FOCUS, theme.accent);

    painter.rect_stroke(
        rect,
        design::box_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
}

fn paint_compact(
    ui: &egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    filter: &Filter,
    sample_rate: f32,
    response: &egui::Response,
) {
    let painter = ui.painter();
    let view = View::FULL;
    // ELEKTRON'S MINI FILTER, as the Digitone actually draws it.
    //
    // A framed box and ONE hairline inside it. Everything else we had
    // is absent from theirs on purpose:
    //
    // - No FILL under the curve. The line is the response; a filled
    //   wedge is a different claim (energy) drawn in the same place.
    // - No frequency grid. At this size their screen has no axis at
    //   all — the shape is read against the BOX, and vertical rules
    //   behind a curve that crosses them read as a cage.
    // - No unity rule. The passband's own flat run states unity by
    //   sitting where it sits.
    // - A 1 px BORDER, which is the thing that makes it a readable
    //   little instrument rather than a drawing floating on a panel.
    //   Their envelope above has no border; the filter box does.
    painter.rect_filled(rect, 0.0, theme.surface_sunken);
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(stroke::HAIR, theme.role_shape_dim),
        egui::StrokeKind::Inside,
    );
    let points = curve_points(rect, filter, sample_rate, view);
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(stroke::HAIR, theme.role_shape),
    ));
    let node = egui::pos2(
        rect.left() + rect.width() * hz_to_norm(filter.cutoff_hz),
        rect.bottom()
            - rect.height() * view.db_to_norm(magnitude_db(filter, filter.cutoff_hz, sample_rate)),
    );
    painter.circle_filled(node, theme.sp(control::HANDLE), theme.accent);
    painter.circle_stroke(
        node,
        theme.sp(control::HANDLE),
        egui::Stroke::new(stroke::HAIR, theme.surface_sunken),
    );
    painter.rect_stroke(
        rect,
        design::box_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }
}

fn paint(
    ui: &egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    filter: &Filter,
    sample_rate: f32,
    response: &egui::Response,
) {
    let painter = ui.painter();
    // THE FRAMED BOX. On a Digitone the mini filter is a rectangle with
    // a one-pixel border and a single hairline inside it — the border is
    // what makes it read as a little instrument rather than a drawing
    // adrift on a panel. Square corners: their screen has no radii.
    painter.rect_filled(rect, 0.0, theme.surface_sunken);
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(stroke::HAIR, theme.role_shape_dim),
        egui::StrokeKind::Inside,
    );

    let x_at = |hz: f32| rect.left() + rect.width() * hz_to_norm(hz);
    let y_at = |db: f32| rect.bottom() - rect.height() * db_to_norm(db);

    // NO GRID, NO AXIS LABELS, NO FILL — all three are absent from
    // Elektron's, and each for its own reason:
    //
    // - The grid: their box has no rules inside it at all. Vertical
    //   lines behind a curve that crosses them read as a cage, and the
    //   shape is meant to be read against the BOX.
    // - The labels: "100 / 1k / 10k" printed inside the plot is a
    //   plugin habit. The Digitone prints the cutoff ONCE, as a value
    //   beside its label under the box, which we do in the panel.
    // - The fill: a filled wedge is a claim about ENERGY drawn in the
    //   place where the line already makes a claim about response.
    //   One line, one meaning.

    // --- the curve -------------------------------------------------------
    let points = curve_points(rect, filter, sample_rate, View::FULL);
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(stroke::HAIR, theme.role_shape),
    ));

    // --- the corner node -------------------------------------------------
    // A small HOLLOW square, not a filled disc: Elektron's handles are
    // outlined boxes, and hollow reads as "grab me" where a solid dot
    // reads as "this is a datum".
    let node = egui::pos2(
        x_at(filter.cutoff_hz),
        y_at(magnitude_db(filter, filter.cutoff_hz, sample_rate)),
    );
    let r = theme.sp(control::HANDLE) * 0.8;
    let node_rect = egui::Rect::from_center_size(node, egui::vec2(r, r) * 2.0);
    painter.rect_filled(node_rect, 0.0, theme.surface_sunken);
    painter.rect_stroke(
        node_rect,
        0.0,
        egui::Stroke::new(stroke::HAIR, theme.role_shape),
        egui::StrokeKind::Middle,
    );

    // --- the reading -----------------------------------------------------
    // What the node is set to, in words, top-left where it cannot be
    // confused with an axis label.
    let slope = if filter.mode.uses_slope() {
        format!("  {:.0} dB/oct", filter.slope.db_per_octave())
    } else {
        String::new()
    };
    let drive = if filter.drive > 0.0 {
        format!("  drive {:.0}%", filter.drive * 100.0)
    } else {
        String::new()
    };
    painter.text(
        rect.left_top() + egui::vec2(design::gap(theme), design::gap(theme)),
        egui::Align2::LEFT_TOP,
        format!(
            "{}  {}  Q {:.2}{slope}{drive}",
            Mode::NAMES[Mode::ALL
                .iter()
                .position(|m| *m == filter.mode)
                .unwrap_or(0)],
            hz_text(filter.cutoff_hz),
            filter.q
        ),
        egui::FontId::monospace(font::LABEL),
        theme.text_muted,
    );

    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }
}

fn hz_text(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{:.2} kHz", hz / 1000.0)
    } else {
        format!("{hz:.0} Hz")
    }
}

// ---------------------------------------------------------------- card ---

// The filter device's card: the response curve as the hero, its own
// controls underneath.
//
// The curve is not decoration here — it is the control. Cutoff and
// resonance are dragged ON it, which is the gesture every filter display
// has offered since the first one drew a line, and the cells beneath say
// what the rest of the device is doing.
//
// What the picture cannot show it does not draw: the SPREAD leans the
// two channels' corners in opposite directions, so a single line is
// already an average of two, and the character's saturating curve puts
// harmonics where a magnitude plot has no axis for them. Both keep their
// cells and their honest numbers instead.

/// The card's stored knob positions, all normalized `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilterUi {
    pub mode: f32,
    pub slope: f32,
    pub cutoff: f32,
    pub res: f32,
    pub drive: f32,
    pub character: f32,
    pub spread: f32,
}

impl Default for FilterUi {
    /// Every knob at the TABLE's default, so a fresh card and a fresh
    /// device agree without either asking the other.
    fn default() -> Self {
        Self::from_engine(|id| crate::params::def(fp::TABLE, id).default)
    }
}

impl FilterUi {
    /// The card's state for a patch in engine units.
    ///
    /// Takes a READER rather than the engine's params struct, because a
    /// widget module must not know the engine — that is the UI layer
    /// contract. The app knows both sides and hands the values across.
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| filter_norm(id, get(id));
        Self {
            mode: at(fp::MODE),
            slope: at(fp::SLOPE),
            cutoff: at(fp::CUTOFF),
            res: at(fp::RES),
            drive: at(fp::DRIVE),
            character: at(fp::CHARACTER),
            spread: at(fp::SPREAD),
        }
    }

    /// One knob position by wire id, so a strip can walk ids rather than
    /// naming every field twice.
    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            fp::MODE => &mut self.mode,
            fp::SLOPE => &mut self.slope,
            fp::CUTOFF => &mut self.cutoff,
            fp::RES => &mut self.res,
            fp::DRIVE => &mut self.drive,
            fp::CHARACTER => &mut self.character,
            fp::SPREAD => &mut self.spread,
            _ => return None,
        })
    }

    fn get(&self, param: u32) -> f32 {
        match param {
            fp::MODE => self.mode,
            fp::SLOPE => self.slope,
            fp::CUTOFF => self.cutoff,
            fp::RES => self.res,
            fp::DRIVE => self.drive,
            fp::CHARACTER => self.character,
            _ => self.spread,
        }
    }

    /// The state as the curve's own spec — the ONE place the card's
    /// normalized positions become the thing that gets drawn.
    pub fn spec(&self) -> Filter {
        Filter {
            mode: Mode::from_index(filter_value(fp::MODE, self.mode).round().max(0.0) as usize),
            slope: Slope::from_index(filter_value(fp::SLOPE, self.slope).round().max(0.0) as usize),
            cutoff_hz: filter_value(fp::CUTOFF, self.cutoff),
            q: filter_value(fp::RES, self.res),
            drive: filter_value(fp::DRIVE, self.drive),
            character: filter_value(fp::CHARACTER, self.character).round().max(0.0) as u32,
        }
    }
}

/// Every parameter, in the order the card lays them out.
const CELLS: [u32; 7] = [
    fp::MODE,
    fp::SLOPE,
    fp::CUTOFF,
    fp::RES,
    fp::DRIVE,
    fp::CHARACTER,
    fp::SPREAD,
];

/// One control by wire id. The single place an id becomes a `Param`.
///
/// NATURAL UNITS end to end: hertz, semitones and per cent, never
/// `0..1`.
fn param_of(param: u32) -> Param {
    let def = crate::params::def(fp::TABLE, param);
    match param {
        fp::MODE => Param::choice("mode", Mode::NAMES).with_default(def.default),
        fp::SLOPE => Param::choice("dB/oct", Slope::NAMES).with_default(def.default),
        // Log, both of them: an octave of cutoff is an octave wherever it
        // sits, and a resonance control spends almost all its travel
        // under Q 2 — which is exactly where the audible part of it is.
        fp::CUTOFF => Param::new(
            "cutoff",
            Mapping::Log {
                min: def.min,
                max: def.max,
            },
            Unit::Hz,
        )
        .with_default(def.default),
        fp::RES => Param::new(
            "res",
            Mapping::Log {
                min: def.min,
                max: def.max,
            },
            Unit::Plain,
        )
        .with_default(def.default),
        fp::DRIVE => Param::percent("drive").with_default(def.default * 100.0),
        fp::CHARACTER => Param::choice("character", crate::params::filter::CHARACTER_NAMES)
            .with_default(def.default),
        // Semitones, not hertz: the useful amount of spread is a musical
        // interval, and the engine's own control says so in its name.
        _ => Param::new(
            "spread",
            Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            Unit::Semitones,
        )
        .with_default(def.default),
    }
}

/// What the widget SHOWS for an engine value. Only the drive differs: it
/// is stored `0..1` and read as a percentage.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        fp::DRIVE => value * 100.0,
        _ => value,
    }
}

/// What the ENGINE receives for a shown value, clamped through the table
/// so a knob at either stop cannot emit a letter the engine has to bin.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        fp::DRIVE => value / 100.0,
        fp::MODE | fp::SLOPE | fp::CHARACTER => value.round(),
        _ => value,
    };
    crate::params::def(fp::TABLE, param).clamp(raw)
}

/// The engine-facing value at a normalized knob position.
pub fn filter_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`filter_value`], for a state stored in engine units.
pub fn filter_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter snaps rather than sweeping. The three switches do.
pub fn filter_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale — the cutoff and the
/// resonance, and nothing else.
pub fn filter_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// How a value reads on the card, for the editor's own rows.
pub fn filter_format(param: u32, value: f32) -> String {
    param_of(param).unit.format(shown(param, value))
}

/// How many settings a parameter has, if it is discrete.
pub fn filter_choices(param: u32) -> Option<u32> {
    param_of(param).choices()
}

/// Every parameter as an edit, for a reset, a preset recall, or the
/// moment the device is first loaded.
pub fn filter_edits(state: &FilterUi) -> Vec<ParamEdit> {
    CELLS
        .into_iter()
        .map(|param| ParamEdit {
            param,
            value: filter_value(param, state.get(param)),
        })
        .collect()
}

/// The value strip's rows: what SHAPE the filter is, then what it does to
/// the sound on the way through.
const ROWS: [&[u32]; 2] = [
    &[fp::MODE, fp::SLOPE, fp::CUTOFF, fp::RES],
    &[fp::DRIVE, fp::CHARACTER, fp::SPREAD],
];

/// How many `POLY_CELL_H` units one row of labelled cells needs.
///
/// TWO: a labelled cell prints its value on one line and its name on the
/// next, so a row given one unit prints them through each other.
const CELL_UNITS: usize = 2;

/// How many rows of height the footer reserves.
const FOOTER_ROWS: usize = ROWS.len() * CELL_UNITS;

/// The NARROWEST a cell may be drawn.
fn cell_min_width(ui: &egui::Ui, theme: &Theme, param: &Param) -> f32 {
    let value = metrics::mono_w(ui, &param.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &param.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

/// The width the whole face needs: its WIDEST ROW at its narrowest.
fn face_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    ROWS.iter()
        .map(|row| {
            let cells: f32 = row
                .iter()
                .map(|id| cell_min_width(ui, theme, &param_of(*id)))
                .sum();
            cells + ui.spacing().item_spacing.x * row.len().saturating_sub(1) as f32
        })
        .fold(0.0, f32::max)
}

/// Draw the filter card. Returns the edits the user just made.
pub fn filter_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut FilterUi,
    sample_rate: f32,
) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);

    card::card_sized(ui, theme, "filter", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => {
                        plot(ui, theme, state, sample_rate, &mut edits)
                    }
                    poly_widgets::CurveRegion::Footer => footer(ui, theme, state, &mut edits),
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

/// The hero, and the two controls that live on it.
///
/// The drag writes back through the SAME mapping the cells use, so the
/// cutoff cell reads what the curve was just dragged to rather than
/// something a second conversion invented.
fn plot(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut FilterUi,
    sample_rate: f32,
    edits: &mut Vec<ParamEdit>,
) {
    let mut spec = state.spec();
    if !filter_curve_fill(ui, theme, &mut spec, sample_rate) {
        return;
    }
    for (param, value) in [(fp::CUTOFF, spec.cutoff_hz), (fp::RES, spec.q)] {
        let value = crate::params::def(fp::TABLE, param).clamp(value);
        if let Some(slot) = state.slot(param) {
            *slot = filter_norm(param, value);
        }
        edits.push(ParamEdit { param, value });
    }
}

/// The value strip.
fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut FilterUi, edits: &mut Vec<ParamEdit>) {
    // The row height the PANEL reserved, with the gaps taken off FIRST —
    // dividing the whole height by the row count hands each row a share
    // of the gaps as well and walks them down the card until they
    // overlap.
    let gap = theme.sp(space::XXS);
    let rows = ROWS.len() as f32;
    let height = ((ui.available_height() - gap * (rows - 1.0)) / rows).max(1.0);
    ui.spacing_mut().item_spacing.y = gap;
    for row in ROWS {
        ui.horizontal(|ui| {
            let gap_x = ui.spacing().item_spacing.x;
            for (drawn, param) in row.iter().enumerate() {
                let spec = param_of(*param);
                // The share is recomputed from what is ACTUALLY left,
                // cell by cell, rather than divided up in advance.
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
                        // A switch gets the STEPPED cell: one sweeping
                        // smoothly through settings it cannot take would
                        // be a control lying about what it does.
                        let moved = if filter_is_discrete(*param) {
                            poly_widgets::labeled_cell_steps(ui, theme, &spec, norm, None)
                        } else {
                            poly_widgets::labeled_cell_bar(ui, theme, &spec, norm, None)
                        };
                        if moved {
                            edits.push(ParamEdit {
                                param: *param,
                                value: filter_value(*param, *norm),
                            });
                        }
                    },
                );
            }
        });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ui::device::probe;

    const FS: f32 = 48_000.0;

    fn lp(slope: Slope, cutoff: f32, q: f32) -> Filter {
        Filter {
            mode: Mode::Lowpass,
            slope,
            cutoff_hz: cutoff,
            q,
            drive: 0.0,
            character: crate::params::filter::CHAR_CLEAN,
        }
    }

    /// The passband is unity and the stopband is not. The first thing to
    /// get right and the easiest to get backwards.
    #[test]
    fn a_lowpass_passes_below_and_stops_above() {
        let f = lp(Slope::Db24, 1_000.0, FLAT_Q);
        assert!(magnitude_db(&f, 50.0, FS).abs() < 0.1, "flat well below");
        assert!(magnitude_db(&f, 100.0, FS).abs() < 0.1);
        assert!(magnitude_db(&f, 8_000.0, FS) < -60.0, "gone well above");
        // And monotonically down through the corner, with no resonance.
        let mut last = f32::INFINITY;
        for i in 0..=60 {
            let hz = 1_000.0 * 2f32.powf(i as f32 / 10.0);
            let db = magnitude_db(&f, hz, FS);
            assert!(db <= last + 0.01, "not monotonic at {hz} Hz");
            last = db;
        }
    }

    /// A highpass is the mirror image, and a bandpass and notch do what
    /// their names say at the corner.
    #[test]
    fn every_mode_does_what_its_name_says() {
        let corner = 1_000.0;
        let hp = Filter {
            mode: Mode::Highpass,
            ..lp(Slope::Db24, corner, FLAT_Q)
        };
        assert!(magnitude_db(&hp, 50.0, FS) < -40.0, "blocks below");
        assert!(magnitude_db(&hp, 10_000.0, FS).abs() < 0.5, "passes above");

        let bp = Filter {
            mode: Mode::Bandpass,
            q: 2.0,
            ..lp(Slope::Db12, corner, 2.0)
        };
        assert!(
            magnitude_db(&bp, corner, FS).abs() < 0.5,
            "unity AT the corner — constant peak gain, so high Q is not a boost"
        );
        assert!(magnitude_db(&bp, corner / 8.0, FS) < -20.0);
        assert!(magnitude_db(&bp, corner * 8.0, FS) < -20.0);

        let notch = Filter {
            mode: Mode::Notch,
            ..lp(Slope::Db12, corner, 4.0)
        };
        assert!(
            magnitude_db(&notch, corner, FS) < -40.0,
            "a hole at the corner"
        );
        assert!(magnitude_db(&notch, corner / 16.0, FS).abs() < 0.5);
        assert!(magnitude_db(&notch, corner * 16.0, FS).abs() < 0.5);
    }

    /// THE test for this widget: every slope actually falls at the dB per
    /// octave it advertises.
    ///
    /// Measured in the stopband but well below Nyquist, where the response
    /// is a straight line on log-log axes. A curve whose 48 dB/octave
    /// setting really rolls off at 24 would look plausible in a screenshot
    /// and be wrong in every mix decision made from it.
    #[test]
    fn each_slope_falls_at_the_rate_it_claims() {
        for slope in Slope::ALL {
            let f = lp(slope, 200.0, FLAT_Q);
            // Two and three octaves past the corner: into the asymptote,
            // far below Nyquist so nothing is warped, and shallow enough
            // that even eight poles have not reached the silence floor.
            let a = magnitude_db(&f, 800.0, FS);
            let b = magnitude_db(&f, 1_600.0, FS);
            let measured = a - b;
            assert!(
                (measured - slope.db_per_octave()).abs() < 1.0,
                "{:?} claims {} dB/oct, measured {measured:.1}",
                slope,
                slope.db_per_octave()
            );
        }
    }

    /// Resonance puts a peak AT the corner, and a taller one for higher Q.
    /// Applying the resonance to every section instead of the last would
    /// widen it into a hump and move it off the corner.
    #[test]
    fn resonance_peaks_at_the_corner() {
        let corner = 1_000.0;
        let flat = magnitude_db(&lp(Slope::Db24, corner, FLAT_Q), corner, FS);
        let some = magnitude_db(&lp(Slope::Db24, corner, 2.0), corner, FS);
        let lots = magnitude_db(&lp(Slope::Db24, corner, 8.0), corner, FS);
        assert!(some > flat + 3.0, "resonance lifts the corner");
        assert!(lots > some + 6.0, "and more resonance lifts it further");

        // The peak is AT the corner, not beside it.
        let f = lp(Slope::Db24, corner, 8.0);
        let at = magnitude_db(&f, corner, FS);
        for away in [0.25, 0.5, 2.0, 4.0] {
            assert!(
                magnitude_db(&f, corner * away, FS) < at,
                "the peak drifted off the corner"
            );
        }
    }

    /// A Butterworth cascade is −3 dB at its corner, whatever the order.
    /// The number every filter table in every textbook agrees on, so it is
    /// the one worth checking the cascade Q values against.
    #[test]
    fn a_flat_cascade_is_three_db_down_at_the_corner() {
        for slope in Slope::ALL {
            let db = magnitude_db(&lp(slope, 1_000.0, FLAT_Q), 1_000.0, FS);
            assert!(
                (db + 3.0).abs() < 0.6,
                "{slope:?} is {db:.2} dB at its corner, not -3"
            );
        }
    }

    /// Drive squashes the resonant peak — the audible difference between a
    /// digital filter screaming and an analogue one growling.
    #[test]
    fn drive_compresses_the_resonant_peak() {
        let corner = 1_000.0;
        let clean = Filter {
            drive: 0.0,
            ..lp(Slope::Db24, corner, 12.0)
        };
        let driven = Filter {
            drive: 1.0,
            ..clean
        };
        let a = magnitude_db(&clean, corner, FS);
        let b = magnitude_db(&driven, corner, FS);
        assert!(a > b + 6.0, "drive must squash the peak: {a:.1} -> {b:.1}");
        assert!(b > 0.0, "but not flatten it entirely");

        // It squashes only the resonance, not the filter. A flat cascade
        // driven hard keeps its corner.
        let flat_clean = magnitude_db(&lp(Slope::Db24, corner, FLAT_Q), corner, FS);
        let flat_driven = magnitude_db(
            &Filter {
                drive: 1.0,
                ..lp(Slope::Db24, corner, FLAT_Q)
            },
            corner,
            FS,
        );
        assert!((flat_clean - flat_driven).abs() < 0.5);
    }

    /// Drive fills the stopband: a nonlinear filter never measures the
    /// clean rolloff its coefficients promise.
    #[test]
    fn drive_floors_the_stopband() {
        let f = lp(Slope::Db48, 200.0, FLAT_Q);
        let clean = magnitude_db(&f, 4_000.0, FS);
        assert!(clean < -100.0, "a clean 48 dB/oct really does vanish");

        let driven = magnitude_db(&Filter { drive: 1.0, ..f }, 4_000.0, FS);
        assert!(
            driven > clean + 40.0,
            "distortion products fill the stopband: {clean:.0} -> {driven:.0}"
        );
        // And the floor rises WITH drive rather than snapping in.
        let half = magnitude_db(&Filter { drive: 0.5, ..f }, 4_000.0, FS);
        assert!(half < driven && half > clean, "the floor moves gradually");
    }

    /// The curve is the DIGITAL response: it must not mirror around
    /// Nyquist, which is what an analogue formula plotted on a digital
    /// axis does and the classic way these displays go wrong.
    #[test]
    fn nothing_mirrors_around_nyquist() {
        let f = lp(Slope::Db24, 1_000.0, FLAT_Q);
        let mut last = f32::INFINITY;
        // Right up to the edge of the view at a 44.1k rate, where Nyquist
        // is inside the plotted range.
        for i in 0..=200 {
            let hz = norm_to_hz(i as f32 / 200.0);
            let db = magnitude_db(&f, hz, 44_100.0);
            assert!(db.is_finite(), "{hz} Hz gave {db}");
            assert!(db <= last + 0.01, "response rose again at {hz} Hz");
            last = db;
        }
    }

    /// Nonsense inputs answer a number. A NaN here becomes a NaN point,
    /// which egui draws as nothing — a curve that silently vanishes.
    #[test]
    fn nonsense_inputs_stay_finite() {
        let f = lp(Slope::Db24, 1_000.0, FLAT_Q);
        for hz in [0.0, -100.0, f32::INFINITY, 1e9] {
            assert!(magnitude_db(&f, hz, FS).is_finite(), "hz {hz}");
        }
        for fs in [0.0, -1.0] {
            assert!(magnitude_db(&f, 1_000.0, fs).is_finite(), "fs {fs}");
        }
        let silly = Filter {
            cutoff_hz: 1e9,
            q: 0.0,
            drive: 5.0,
            ..f
        };
        assert!(magnitude_db(&silly, 1_000.0, FS).is_finite());
    }

    /// The mini is the same filter drawn smaller: same maths, same axis,
    /// same window — only the chrome differs.
    #[test]
    fn the_mini_agrees_with_the_full_display() {
        let theme = Theme::dark();
        let full = footprint(&theme);
        let small = footprint_mini(&theme);
        assert!(
            small.width() < full.width() && small.height() < full.height(),
            "the thumbnail must actually be smaller"
        );

        // Same vertical scale, so the thumbnail cannot promise a shape the
        // full display then contradicts.
        let f = lp(Slope::Db24, 1_000.0, 6.0);
        for hz in [50.0, 500.0, 1_000.0, 2_000.0, 12_000.0] {
            let db = magnitude_db(&f, hz, FS);
            assert_eq!(View::FULL.db_to_norm(db), db_to_norm(db), "{hz} Hz");
        }
    }

    /// The window clamps, so nothing a filter can do puts a point outside
    /// its box — a curve escaping its own rectangle is the one failure a
    /// thumbnail cannot hide.
    #[test]
    fn the_window_clamps_whatever_the_filter_does() {
        for db in [-1e6, -240.0, 0.0, 60.0, 1e6, f32::MAX] {
            let n = View::FULL.db_to_norm(db);
            assert!((0.0..=1.0).contains(&n), "{db} dB gave {n}");
        }
        assert_eq!(View::FULL.db_to_norm(View::FULL.bottom_db), 0.0);
        assert_eq!(View::FULL.db_to_norm(View::FULL.top_db), 1.0);

        // A degenerate window answers rather than dividing by zero.
        let flat = View {
            top_db: 0.0,
            bottom_db: 0.0,
        };
        assert_eq!(flat.db_to_norm(5.0), 0.0);
    }

    /// The window holds the resonance settings people actually use.
    /// Measured, not assumed: a Q of 8 peaks at +18.1 dB and a Q of 12 at
    /// +21.6, which is what ruled out a tighter window for the mini.
    #[test]
    fn the_window_holds_ordinary_resonance() {
        for (q, ceiling) in [(4.0, 12.5), (8.0, 18.5), (12.0, 22.0)] {
            let peak = magnitude_db(&lp(Slope::Db24, 1_000.0, q), 1_000.0, FS);
            assert!(peak < ceiling, "Q {q} peaked at {peak:.1}, over {ceiling}");
            assert!(
                peak < View::FULL.top_db,
                "Q {q} peaks at {peak:.1} and would clip the view"
            );
        }
    }

    /// The axes are log in frequency and linear in dB, and both invert.
    #[test]
    fn the_axes_map_and_invert() {
        for hz in [20.0, 100.0, 440.0, 1_000.0, 20_000.0] {
            let back = norm_to_hz(hz_to_norm(hz));
            assert!((back - hz).abs() < hz * 0.001, "{hz} -> {back}");
        }
        // Log means an octave is the same distance wherever it is.
        let low = hz_to_norm(200.0) - hz_to_norm(100.0);
        let high = hz_to_norm(8_000.0) - hz_to_norm(4_000.0);
        assert!(
            (low - high).abs() < 1e-4,
            "octaves must be even: {low} {high}"
        );

        assert_eq!(db_to_norm(VIEW_BOTTOM_DB), 0.0);
        assert_eq!(db_to_norm(VIEW_TOP_DB), 1.0);
        assert!(
            (db_to_norm(0.0) - 0.6).abs() < 0.01,
            "unity sits above centre"
        );
    }

    // ------------------------------------------------------------ card ---

    /// EVERY ROW ROUND-TRIPS between engine units and knob positions.
    #[test]
    fn every_parameter_round_trips_through_the_card() {
        for def in fp::TABLE {
            for at in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
                let value = filter_value(def.id, at);
                assert!(
                    value >= def.min - 1e-3 && value <= def.max + 1e-3,
                    "{} at {at} left the table's range: {value}",
                    def.name
                );
                let back = filter_norm(def.id, value);
                let again = filter_value(def.id, back);
                assert!(
                    (again - value).abs() <= (value.abs() * 1e-3).max(1e-3),
                    "{} did not round-trip: {value} -> {back} -> {again}",
                    def.name
                );
            }
        }
    }

    /// The card's defaults ARE the table's defaults — and for this device
    /// that means it opens TRANSPARENT. A filter that started cutting the
    /// moment it was dropped on a track would be a device that destroys
    /// the mix on load, which is the one thing an insert must never do.
    #[test]
    fn the_cards_defaults_match_the_engine_table_and_open_out_of_the_way() {
        let card = FilterUi::default();
        for def in fp::TABLE {
            let shown = filter_value(def.id, card.get(def.id));
            assert!(
                (shown - def.default).abs() <= (def.default.abs() * 1e-3).max(1e-3),
                "{} defaults to {shown}, table says {}",
                def.name,
                def.default
            );
        }
        let spec = card.spec();
        assert_eq!(spec.mode, Mode::Lowpass);
        assert!(spec.cutoff_hz >= 20_000.0 - 1.0, "the filter loads closed");
        assert_eq!(spec.drive, 0.0, "and driven");
        // Which the CURVE has to agree about: a lowpass parked at the top
        // of the range passes the band it is drawn over.
        for hz in [100.0f32, 1_000.0, 8_000.0] {
            let db = magnitude_db(&spec, hz, 48_000.0);
            assert!(db.abs() < 1.0, "the default curve cuts {db} dB at {hz}");
        }
    }

    /// Every parameter is REACHABLE, exactly once.
    #[test]
    fn the_rows_cover_every_parameter_exactly_once() {
        let mut seen: Vec<u32> = ROWS.iter().flat_map(|row| row.iter().copied()).collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "a parameter is on the card twice");
        assert_eq!(seen.len(), fp::TABLE.len(), "a parameter is missing");
        for def in fp::TABLE {
            assert!(seen.contains(&def.id), "{} has no cell", def.name);
        }
        assert_eq!(CELLS.len(), fp::TABLE.len());
        assert_eq!(filter_edits(&FilterUi::default()).len(), fp::TABLE.len());
    }

    /// THE THREE SWITCHES SNAP and every one of their settings is
    /// reachable; the sweeps do not snap, and the two that live in
    /// octaves say so.
    #[test]
    fn the_switches_step_through_their_names_and_the_sweeps_do_not() {
        for (param, names) in [
            (fp::MODE, Mode::NAMES),
            (fp::SLOPE, Slope::NAMES),
            (fp::CHARACTER, fp::CHARACTER_NAMES),
        ] {
            assert!(filter_is_discrete(param));
            assert_eq!(filter_choices(param), Some(names.len() as u32));
            for (i, name) in names.iter().enumerate() {
                let norm = param_of(param).at_index(i);
                let value = filter_value(param, norm);
                assert_eq!(value, i as f32, "{name} is not reachable");
                assert!(filter_format(param, value).contains(name));
            }
        }
        for param in [fp::CUTOFF, fp::RES, fp::DRIVE, fp::SPREAD] {
            assert!(
                !filter_is_discrete(param),
                "{} is continuous but snaps",
                crate::params::def(fp::TABLE, param).name
            );
        }
        // Frequency and resonance sweep in RATIOS — an octave of cutoff
        // is an octave wherever it sits, and a Q control spends almost
        // all of its useful travel at the bottom.
        assert!(filter_is_log(fp::CUTOFF));
        assert!(filter_is_log(fp::RES));
        assert!(!filter_is_log(fp::DRIVE));
        assert!(!filter_is_log(fp::SPREAD));
    }

    /// THE CARD'S STATE AND THE DRAWN CURVE ARE THE SAME SETTINGS. The
    /// cells and the picture read one state or they are two devices in
    /// one window.
    #[test]
    fn the_drawn_curve_is_the_state_the_cells_hold() {
        let card = FilterUi::from_engine(|id| match id {
            fp::MODE => fp::MODE_HP as f32,
            fp::SLOPE => 5.0,
            fp::CUTOFF => 640.0,
            fp::RES => 6.0,
            fp::DRIVE => 0.5,
            fp::CHARACTER => fp::CHAR_DIODE as f32,
            _ => 7.0,
        });
        let spec = card.spec();
        assert_eq!(spec.mode, Mode::Highpass);
        assert_eq!(spec.slope, Slope::Db48);
        assert!((spec.cutoff_hz - 640.0).abs() < 1.0);
        assert!((spec.q - 6.0).abs() < 0.01);
        assert!((spec.drive - 0.5).abs() < 0.01);
        assert_eq!(spec.character, fp::CHAR_DIODE);
        // And the spread, which the single line cannot draw, is still on
        // the card: it round-trips even though the curve ignores it.
        assert!((filter_value(fp::SPREAD, card.spread) - 7.0).abs() < 0.01);
    }

    /// THE FOOTER RESERVES TWO LINES PER ROW OF CELLS, and the plot keeps
    /// enough height to be a picture.
    #[test]
    fn the_footer_reserves_two_lines_for_every_row_of_cells() {
        assert_eq!(CELL_UNITS, 2, "a value and its name are two lines");
        assert_eq!(FOOTER_ROWS, ROWS.len() * CELL_UNITS);

        let theme = Theme::dark();
        let gap = theme.sp(space::XXS);
        let unit = theme.sp(control::POLY_CELL_H);
        let reserved = unit * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS - 1) as f32;
        let needed = unit * CELL_UNITS as f32 * ROWS.len() as f32 + gap * (ROWS.len() - 1) as f32;
        assert!(
            reserved >= needed,
            "the footer reserves {reserved} for rows needing {needed}"
        );
        let plot = theme.sp(control::DEVICE_TALL_H) - reserved;
        assert!(plot > unit * 3.0, "the plot was squeezed to {plot}");
    }

    /// EVERY ROW FITS THE WIDTH THE CARD ASKS FOR.
    #[test]
    fn every_row_fits_the_width_the_card_asks_for() {
        let context = egui::Context::default();
        let theme = Theme::dark();
        let mut run = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                let declared = face_width(ui, &theme);
                assert!(declared > 0.0, "the card declared no width at all");
                for row in ROWS {
                    let natural: f32 = row
                        .iter()
                        .map(|id| cell_min_width(ui, &theme, &param_of(*id)))
                        .sum::<f32>()
                        + ui.spacing().item_spacing.x * row.len().saturating_sub(1) as f32;
                    assert!(
                        natural <= declared + 0.5,
                        "a row needs {natural} but the card asks for {declared}"
                    );
                    for id in row {
                        let param = param_of(*id);
                        let wanted = metrics::mono_w(ui, &param.widest_text(), font::VALUE);
                        assert!(
                            cell_min_width(ui, &theme, &param) >= wanted,
                            "{} cannot print its own value",
                            param.name
                        );
                    }
                }
            },
        );
        run.textures_delta.clear();
    }

    /// The card draws headlessly and emits nothing at rest.
    #[test]
    fn drawing_at_rest_emits_nothing() {
        let context = egui::Context::default();
        let theme = Theme::dark();
        let mut state = FilterUi::default();
        let before = state;
        let mut edits = Vec::new();
        let mut run = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                edits = filter_card(ui, &theme, &mut state, 48_000.0);
            },
        );
        run.textures_delta.clear();
        assert!(edits.is_empty(), "the card moved on its own: {edits:?}");
        assert_eq!(state, before, "and it rewrote its own state");
    }

    /// A POINTER DRAGS THE CURVE, and what it drags comes back as an
    /// edit the engine can take.
    ///
    /// The card's one draggable target, so the one the contract asks to
    /// be pressed rather than merely drawn: the cells beneath are the
    /// shared `labeled_cell` widgets, and the plot is the only geometry
    /// this module invented. A drag that moved the picture without
    /// emitting an edit would look right in the window and do nothing to
    /// the audio — which is exactly the failure a draw-once test cannot
    /// see.
    #[test]
    fn dragging_the_curve_moves_the_cutoff_and_says_so() {
        let context = egui::Context::default();
        let theme = Theme::dark();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(520.0, 360.0));
        // Start from a corner in the middle of the range, so the drag has
        // somewhere to go in both directions.
        let mut state = FilterUi::from_engine(|id| match id {
            fp::CUTOFF => 1_000.0,
            id => crate::params::def(fp::TABLE, id).default,
        });
        let opened = filter_value(fp::CUTOFF, state.cutoff);

        // Well inside the plot: below the card's title, above the two
        // rows of cells.
        let y = rect.top() + 90.0;
        let path = probe::drag_path(
            egui::pos2(rect.left() + 160.0, y),
            egui::pos2(rect.left() + 300.0, y),
            6,
        );
        let mut edits = Vec::new();
        probe::run(&context, rect, &path, |ui| {
            edits.extend(filter_card(ui, &theme, &mut state, 48_000.0));
        });

        let moved = filter_value(fp::CUTOFF, state.cutoff);
        assert!(
            moved > opened * 1.2,
            "dragging right did not open the filter: {opened} -> {moved}"
        );
        let last = edits
            .iter()
            .rev()
            .find(|edit| edit.param == fp::CUTOFF)
            .expect("the drag emitted no cutoff edit");
        assert!(
            (last.value - moved).abs() < moved * 1e-3,
            "the edit says {} and the card says {moved}",
            last.value
        );
        // And the card kept what it was handed: a card that forgets
        // mid-gesture snaps back on the next frame.
        assert!(
            (filter_norm(fp::CUTOFF, moved) - state.cutoff).abs() < 1e-4,
            "the card's own state and its engine value disagree"
        );
    }
}
