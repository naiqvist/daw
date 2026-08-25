//! Param-aware rotary knobs: [`knob`] full size, [`mini`] compact.
//!
//! Both are the same control — the same dial, the same drag, the same
//! shift-for-fine and double-click-to-reset — drawn with different amounts
//! of chrome. The input handling lives in one place ([`drive`]) so they
//! cannot pick up different manners.
//!
//! # Why a mini exists
//!
//! A full knob is a name, a dial and a readout stacked: about 78 points.
//! A card body is about 129, which is why every device so far is one row
//! deep and a two-row layout does not fit ([`card::Well::divided`] says so
//! in as many words). A mini is about 37, so two rows fit with room over.
//!
//! # The one interesting decision
//!
//! A mini has ONE caption line, and it shows the name at rest and the
//! VALUE while you are touching it. You get both without paying for two
//! rows, and the swap happens exactly when the value is the thing you
//! want to know.
//!
//! The contract reserves the wider of the two, always. Reserving only the
//! name would make the whole row jump sideways the moment a value
//! appeared — while you are dragging, which is the worst possible moment
//! for the layout to move.
//!
//! [`card::Well::divided`]: crate::ui::device::card::Well::divided

use crate::ui::device::design;
use crate::ui::device::metrics::{self, Footprint};
use crate::ui::device::param::Param;
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// Sweep from 7 o'clock to 5 o'clock — the 270-degree gap-at-the-bottom
/// convention every hardware knob uses.
const START: f32 = std::f32::consts::PI * 0.75;
const SWEEP: f32 = std::f32::consts::PI * 1.5;
/// Full travel over roughly four knob-heights of drag; shift divides by 10.
const TRAVEL_KNOBS: f32 = 4.0;
const FINE: f32 = 0.1;

/// The knob's size contract: the dial, its name above, and the widest
/// readout it can ever show below.
///
/// The width is the WIDEST of the three rows, not just the dial. This is
/// the whole point of the contract: a 28pt dial under the name "release"
/// is a 28pt column, and the label then wraps or is clipped. Nothing about
/// the drawn result says which — it just quietly looks wrong.
pub fn footprint(ui: &egui::Ui, theme: &Theme, param: &Param) -> Footprint {
    let d = theme.sp(control::KNOB);
    metrics::labelled_control(ui, param, egui::vec2(d, d), design::gap(theme))
        .at_least(egui::vec2(metrics::interactive_min(ui).x, 0.0))
}

/// Handle a dial's input: drag, shift-fine, double-click reset, wheel and
/// arrows. Shared by both sizes so they cannot drift apart — a mini that
/// reset on double-click and a full one that did not would be two
/// controls wearing the same face.
fn drive(
    ui: &egui::Ui,
    response: &egui::Response,
    param: &Param,
    norm: &mut f32,
    diameter: f32,
) -> bool {
    let mut changed = false;
    if response.double_clicked() {
        if *norm != param.default_norm {
            *norm = param.default_norm;
            changed = true;
        }
    } else if response.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let travel = diameter * TRAVEL_KNOBS / if fine { FINE } else { 1.0 };
        let delta = -response.drag_delta().y / travel;
        if delta != 0.0 {
            let next = (*norm + delta).clamp(0.0, 1.0);
            if next != *norm {
                *norm = next;
                changed = true;
            }
        }
    }
    // Wheel while hovered, arrows once clicked; Shift is fine.
    let nudge = crate::ui::device::adjust::nudge(ui, response);
    if nudge != 0.0 {
        let next = (*norm + nudge).clamp(0.0, 1.0);
        if next != *norm {
            *norm = next;
            changed = true;
        }
    }
    changed
}

/// Is the user touching this control? What decides whether a mini shows
/// its name or its value.
fn engaged(response: &egui::Response) -> bool {
    response.hovered() || response.dragged() || response.has_focus()
}

/// The mini's size contract: the dial over ONE caption line, wide enough
/// for whichever of the name and the widest value is wider.
///
/// Both, because the caption swaps between them. Reserving only the name
/// is the bug this exists to prevent: the row would jump sideways the
/// instant a value appeared, which is while you are dragging it.
pub fn footprint_mini(ui: &egui::Ui, theme: &Theme, param: &Param) -> Footprint {
    let d = theme.sp(control::KNOB_MINI);
    let caption = metrics::text_w(ui, param.name, font::LABEL).max(metrics::mono_w(
        ui,
        &param.widest_text(),
        font::LABEL,
    ));
    Footprint::new(d, d).stack(
        design::gap(theme),
        Footprint::new(caption, metrics::line_h(ui, font::LABEL)),
    )
}

/// The caption a mini shows: its name at rest, its value while touched.
///
/// Pure, so the swap is testable without a pointer.
pub fn caption(param: &Param, norm: f32, engaged: bool) -> String {
    if engaged {
        param.format(norm)
    } else {
        param.name.to_owned()
    }
}

/// A compact knob: the dial over one caption line. Returns true when the
/// user changed `norm` this frame.
pub fn mini(ui: &mut egui::Ui, theme: &Theme, param: &Param, norm: &mut f32) -> bool {
    let d = theme.sp(control::KNOB_MINI);
    let w = footprint_mini(ui, theme, param).width();
    let mut changed = false;

    ui.allocate_ui_with_layout(
        egui::vec2(w, 0.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.set_width(w);
            // The design system's gap, not egui's item spacing, so the
            // drawn stack is exactly the height the contract reserved.
            ui.spacing_mut().item_spacing.y = design::gap(theme);

            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::click_and_drag());
            changed = drive(ui, &response, param, norm, d);
            paint(ui, theme, rect, param, *norm, &response);

            let live = engaged(&response);
            let text = egui::RichText::new(caption(param, *norm, live)).size(font::LABEL);
            // Monospace for the value so digits do not jitter, and the
            // value colour so the swap reads as "this is live" rather
            // than as the name having changed.
            ui.label(if live {
                text.monospace().color(theme.text_value)
            } else {
                text.color(theme.text_muted)
            });
        },
    );

    changed
}

/// Draw the knob with its name and formatted value. Returns true when the
/// user changed `norm` this frame.
pub fn knob(ui: &mut egui::Ui, theme: &Theme, param: &Param, norm: &mut f32) -> bool {
    let d = theme.sp(control::KNOB);
    let mut changed = false;

    // Claim EXACTLY the stack's width with a center-aligned column. A
    // plain `vertical()` here would take the full available width and
    // then shrink, which leaves the stack pinned left inside any
    // centering parent (a section well, say) — invisible until you look.
    //
    // The width comes from `footprint`, NOT from the dial: the contract is
    // what the container reserved, so drawing anything else would either
    // clip the label or leave a gap the layout already paid for.
    let w = footprint(ui, theme, param).width();
    ui.allocate_ui_with_layout(
        egui::vec2(w, 0.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.set_width(w);
            {
                ui.label(
                    egui::RichText::new(param.name)
                        .size(font::LABEL)
                        .color(theme.text_muted),
                );

                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::click_and_drag());

                changed = drive(ui, &response, param, norm, d);
                paint(ui, theme, rect, param, *norm, &response);

                ui.label(
                    egui::RichText::new(param.format(*norm))
                        .monospace()
                        .size(font::LABEL)
                        .color(theme.text_value),
                );
            }
        },
    );

    changed
}

fn paint(
    ui: &egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    param: &Param,
    norm: f32,
    response: &egui::Response,
) {
    let center = rect.center();
    let radius = rect.width() * 0.5 - stroke::BOLD;
    let painter = ui.painter();

    painter.circle_filled(center, radius, theme.surface_sunken);
    painter.circle_stroke(
        center,
        radius,
        egui::Stroke::new(stroke::HAIR, theme.outline),
    );

    let arc = |from: f32, to: f32, s: egui::Stroke| {
        const STEPS: usize = 24;
        let points: Vec<egui::Pos2> = (0..=STEPS)
            .map(|i| {
                let a = from + (to - from) * i as f32 / STEPS as f32;
                center + crate::ui::kit::knob_dir(a) * radius
            })
            .collect();
        painter.add(egui::Shape::line(points, s));
    };

    // The groove.
    arc(
        START,
        START + SWEEP,
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );

    // The fill: from the minimum, or from the center for bipolar params.
    let at = START + SWEEP * norm.clamp(0.0, 1.0);
    let fill = egui::Stroke::new(stroke::BOLD, theme.accent);
    if param.bipolar {
        let mid = START + SWEEP * 0.5;
        if at != mid {
            arc(mid, at, fill);
        }
    } else if norm > 0.0 {
        arc(START, at, fill);
    }

    // The pointer.
    let dir = crate::ui::kit::knob_dir(at);
    painter.line_segment(
        [center + dir * (radius * 0.4), center + dir * radius],
        egui::Stroke::new(stroke::BOLD, theme.text),
    );

    // Center detent tick for bipolar knobs, at 12 o'clock.
    if param.bipolar {
        let up = egui::vec2(0.0, -1.0);
        painter.line_segment(
            [
                center + up * (radius + stroke::FOCUS),
                center + up * (radius + theme.sp(space::XS)),
            ],
            egui::Stroke::new(stroke::HAIR, theme.text_muted),
        );
    }

    if response.has_focus() || response.dragged() {
        painter.circle_stroke(
            center,
            radius + stroke::FOCUS,
            egui::Stroke::new(stroke::FOCUS, theme.focus),
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ui::device::card::{Well, Wells};

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
        out.unwrap()
    }

    /// The caption swaps: the name at rest, the value while touched. One
    /// line doing the work of two.
    #[test]
    fn the_caption_shows_the_name_at_rest_and_the_value_when_touched() {
        let p = Param::ms("release", 1.0, 30_000.0);
        assert_eq!(caption(&p, 0.5, false), "release");
        assert_eq!(caption(&p, 0.5, true), p.format(0.5));
        assert_ne!(caption(&p, 0.2, true), caption(&p, 0.8, true));
    }

    /// The contract reserves the WIDER of the name and the widest value,
    /// because the caption swaps between them. Reserving only the name
    /// would make the row jump sideways the instant a value appeared —
    /// which is while it is being dragged.
    #[test]
    fn the_mini_reserves_room_for_both_captions() {
        ctx_ui(|ui| {
            let theme = Theme::dark();
            // A short name with long values, and the reverse, so neither
            // side can be the one that always happens to win.
            for p in [
                Param::ms("t", 1.0, 30_000.0),
                Param::percent("a very long parameter name"),
            ] {
                let w = footprint_mini(ui, &theme, &p).width();
                assert!(
                    w >= metrics::text_w(ui, p.name, font::LABEL),
                    "{}: the name must fit",
                    p.name
                );
                assert!(
                    w >= metrics::mono_w(ui, &p.widest_text(), font::LABEL),
                    "{}: every value must fit",
                    p.name
                );
                // And no value it can ever show is wider than reserved.
                for i in 0..=20 {
                    let shown = p.format(i as f32 / 20.0);
                    assert!(metrics::mono_w(ui, &shown, font::LABEL) <= w + 0.5);
                }
            }
        });
    }

    /// The mini is shorter than the full knob — the entire reason it
    /// exists — while staying at the interaction floor in width.
    #[test]
    fn the_mini_is_shorter_than_the_full_knob() {
        ctx_ui(|ui| {
            let theme = Theme::dark();
            let p = Param::percent("mix");
            let full = footprint(ui, &theme, &p);
            let small = footprint_mini(ui, &theme, &p);
            assert!(
                small.height() < full.height() * 0.6,
                "a mini should be well under half again: {} vs {}",
                small.height(),
                full.height()
            );
            assert!(
                theme.sp(control::KNOB_MINI) >= metrics::interactive_min(ui).y * 0.9,
                "the dial must stay a reliable drag target"
            );
        });
    }

    /// THE payoff, stated against the constraint it lifts: two rows of
    /// full knobs do not fit a card body, and two rows of minis do.
    ///
    /// The full-knob half of this is the measurement that made
    /// `Well::divided` warn about row splits in the first place; pinning
    /// both halves means the day a card gets taller, this test says so.
    #[test]
    fn two_rows_of_minis_fit_a_card_where_full_knobs_do_not() {
        ctx_ui(|ui| {
            let theme = Theme::dark();
            let p = Param::ms("release", 1.0, 30_000.0);
            // The body is the card minus its title strip and padding.
            let body = theme.sp(control::DEVICE_H)
                - metrics::line_h(ui, font::LABEL)
                - design::gap(&theme) * 2.0
                - theme.sp(space::SM) * 2.0;

            let rows = |need| {
                Wells::new()
                    .row([Well::divided(2, 2).each(need, &theme)])
                    .min_height(&theme)
            };
            let full = rows(footprint(ui, &theme, &p));
            let small = rows(footprint_mini(ui, &theme, &p));

            assert!(
                full > body,
                "two rows of FULL knobs should not fit: needs {full:.0}, body {body:.0}"
            );
            assert!(
                small < body,
                "two rows of MINI knobs should fit: needs {small:.0}, body {body:.0}"
            );
        });
    }
}
