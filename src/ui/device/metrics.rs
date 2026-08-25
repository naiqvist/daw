//! Footprints: the SIZE CONTRACT every device widget publishes.
//!
//! # Why a widget must state its size before it draws
//!
//! A device card is content-width inside a horizontally-scrolling rack, and
//! its wells are laid out to exact rectangles. Both need to know how much
//! room a control wants BEFORE the control draws — otherwise the choice is
//! between guessing (labels clip, readouts wrap) and measuring after the
//! fact (the card resizes for a frame, or forever, under the pointer).
//!
//! So every widget in `device` exposes a `footprint()` beside its draw
//! function, and the draw function allocates exactly that. The two cannot
//! drift, because the draw function calls the footprint rather than
//! recomputing the numbers — and `ui::tests` fails the build if a widget
//! module ships a draw function with no footprint next to it.
//!
//! # What a footprint includes
//!
//! Everything the widget puts on screen: the control itself, its label, its
//! value readout, and the gaps between them. It does NOT include the well's
//! own padding — that belongs to the container, and [`card::wells`] adds it.
//!
//! [`card::wells`]: crate::ui::device::card::wells
//!
//! # Text
//!
//! Text width is MEASURED, never estimated from a character count: the
//! font is the only thing that knows how wide "release" is, and a
//! per-character guess is wrong by enough to clip. Value readouts reserve
//! [`Param::widest_text`] rather than the current value, so a column cannot
//! change width as the value changes.
//!
//! [`Param::widest_text`]: crate::ui::device::param::Param::widest_text

use crate::ui::device::param::Param;
use crate::ui::theme::Theme;
use crate::ui::tokens::font;
use eframe::egui;

/// The space a widget needs to draw itself legibly.
///
/// A plain size, but a NAMED one: passing `Vec2` around loses the
/// distinction between "how big this is" and "where this is", and the
/// well layout cares about exactly that difference.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Footprint {
    pub size: egui::Vec2,
}

impl Footprint {
    pub const ZERO: Self = Self {
        size: egui::Vec2::ZERO,
    };

    pub fn new(w: f32, h: f32) -> Self {
        Self {
            size: egui::vec2(w.max(0.0), h.max(0.0)),
        }
    }

    pub fn from_size(size: egui::Vec2) -> Self {
        Self::new(size.x, size.y)
    }

    pub fn width(self) -> f32 {
        self.size.x
    }

    pub fn height(self) -> f32 {
        self.size.y
    }

    /// The footprint that holds either — for a widget whose label may be
    /// wider than its control, which is most of them.
    pub fn union(self, other: Self) -> Self {
        Self::new(self.size.x.max(other.size.x), self.size.y.max(other.size.y))
    }

    /// Stack `other` below `self` with `gap` between: heights add, widths
    /// take the wider. The label-over-control-over-readout shape.
    pub fn stack(self, gap: f32, other: Self) -> Self {
        Self::new(
            self.size.x.max(other.size.x),
            self.size.y + gap + other.size.y,
        )
    }

    /// Place `other` beside `self` with `gap` between: widths add, heights
    /// take the taller.
    pub fn beside(self, gap: f32, other: Self) -> Self {
        Self::new(
            self.size.x + gap + other.size.x,
            self.size.y.max(other.size.y),
        )
    }

    /// Grow to at least this size in both axes.
    pub fn at_least(self, size: egui::Vec2) -> Self {
        Self::new(self.size.x.max(size.x), self.size.y.max(size.y))
    }

    /// Grow by `pad` on all four sides — what a container adds for its own
    /// padding.
    pub fn padded(self, pad: f32) -> Self {
        Self::new(self.size.x + pad * 2.0, self.size.y + pad * 2.0)
    }
}

/// Width of `text` at `size`, in the proportional UI font.
///
/// Measured through the font atlas, because that is the only thing that
/// knows. `layout_no_wrap` with infinite width answers "how wide would
/// this be on one line", which is exactly the reservation a label needs.
pub fn text_w(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    galley_size(ui, text, egui::FontId::proportional(size)).x
}

/// Width of `text` at `size` in the MONOSPACE font — value readouts, which
/// are monospace so digits do not jitter as they change.
pub fn mono_w(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    galley_size(ui, text, egui::FontId::monospace(size)).x
}

/// Height of one line at `size`. Line height is not the font size — it
/// includes the font's own ascent and descent — so it is asked for, not
/// assumed.
pub fn line_h(ui: &egui::Ui, size: f32) -> f32 {
    galley_size(ui, "Xg", egui::FontId::proportional(size)).y
}

fn galley_size(ui: &egui::Ui, text: &str, font: egui::FontId) -> egui::Vec2 {
    ui.ctx().fonts_mut(|f| {
        f.layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER)
            .size()
    })
}

/// A control of `control` size with a [`font::LABEL`] name above it and its
/// widest possible value readout below.
///
/// The shape almost every device widget takes, so the arithmetic lives
/// once. `gap` is the vertical spacing between the three, which the caller
/// reads from the design system rather than choosing.
pub fn labelled_control(ui: &egui::Ui, param: &Param, control: egui::Vec2, gap: f32) -> Footprint {
    let label = Footprint::new(text_w(ui, param.name, font::LABEL), line_h(ui, font::LABEL));
    let value = Footprint::new(
        mono_w(ui, &param.widest_text(), font::LABEL),
        line_h(ui, font::LABEL),
    );
    label
        .stack(gap, Footprint::from_size(control))
        .stack(gap, value)
}

/// The interaction floor: no control may be smaller than what egui
/// considers clickable, whatever its own token says.
pub fn interactive_min(ui: &egui::Ui) -> egui::Vec2 {
    ui.spacing().interact_size
}

/// A footprint for a bare block of `size`, with no text at all — the XY
/// pad, the spectrum, the envelope editor.
pub fn block(theme: &Theme, size: egui::Vec2) -> Footprint {
    let _ = theme;
    Footprint::from_size(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_ui<R>(f: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let ctx = egui::Context::default();
        let mut f = Some(f);
        let mut out = None;
        let mut run = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
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
        out.expect("the closure runs once")
    }

    #[test]
    fn stacking_adds_heights_and_keeps_the_wider() {
        let a = Footprint::new(30.0, 10.0);
        let b = Footprint::new(50.0, 20.0);
        let s = a.stack(4.0, b);
        assert_eq!(s.width(), 50.0, "the wider of the two wins");
        assert_eq!(s.height(), 34.0, "10 + gap 4 + 20");

        let r = a.beside(4.0, b);
        assert_eq!(r.width(), 84.0);
        assert_eq!(r.height(), 20.0);

        assert_eq!(a.union(b), Footprint::new(50.0, 20.0));
        assert_eq!(a.padded(5.0), Footprint::new(40.0, 20.0));
        // Never negative, however it is built.
        assert_eq!(Footprint::new(-5.0, -5.0), Footprint::ZERO);
    }

    /// Text is measured, not estimated. The point of the contract is that
    /// a long name reserves more room than a short one — a per-character
    /// guess gets this wrong by enough to clip.
    #[test]
    fn text_width_follows_the_actual_string() {
        ctx_ui(|ui| {
            let short = text_w(ui, "mix", font::LABEL);
            let long = text_w(ui, "release", font::LABEL);
            assert!(long > short, "'release' is wider than 'mix'");
            assert!(short > 0.0, "measured, not zero");
            assert!(line_h(ui, font::LABEL) >= font::LABEL, "line height ≥ size");
        });
    }

    /// A readout reserves the widest value the param can ever show, so a
    /// column cannot change width as the user turns the knob.
    #[test]
    fn a_readout_reserves_its_widest_value() {
        let ms = Param::ms("release", 1.0, 30_000.0);
        let widest = ms.widest_text();
        ctx_ui(|ui| {
            let reserved = mono_w(ui, &widest, font::LABEL);
            for i in 0..=20 {
                let at = ms.format(i as f32 / 20.0);
                assert!(
                    mono_w(ui, &at, font::LABEL) <= reserved + 0.5,
                    "'{at}' is wider than the reserved '{widest}'"
                );
            }
        });
    }

    /// A labelled control is at least as wide as its own text — this is
    /// the clipping bug the contract exists to prevent.
    #[test]
    fn a_labelled_control_is_never_narrower_than_its_text() {
        let param = Param::ms("release", 1.0, 30_000.0);
        ctx_ui(|ui| {
            // A deliberately tiny control: the text must still fit.
            let fp = labelled_control(ui, &param, egui::vec2(8.0, 8.0), 4.0);
            assert!(fp.width() >= text_w(ui, "release", font::LABEL));
            assert!(fp.width() >= mono_w(ui, &param.widest_text(), font::LABEL));
            // Three stacked rows plus two gaps.
            let line = line_h(ui, font::LABEL);
            assert!((fp.height() - (line * 2.0 + 8.0 + 8.0)).abs() < 0.5);
        });
    }
}
