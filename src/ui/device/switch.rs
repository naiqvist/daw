//! The segmented switch: the device layer's DISCRETE control.
//!
//! Every other widget here is continuous — a knob, a fader, a pad, a
//! curve. A filter mode, a waveform, a sync division are not, and rounding
//! a knob into three settings gives you a control whose two-thirds
//! position is a coin flip. This is the one that has choices.
//!
//! # The manners it keeps
//!
//! A segmented control is a well-worn thing and people arrive knowing how
//! it works. All of this is that expectation, written down:
//!
//! - **Click a segment to choose it.** Not the nearest edge, not a cycle —
//!   the thing under the pointer.
//! - **Drag across it** and the selection follows the pointer, so a
//!   press-and-slide lands where you let go. Costs nothing and is what
//!   fingers try.
//! - **Wheel over it** steps one choice per notch, clamped at both ends.
//! - **Arrows once focused**: Right/Up next, Left/Down previous, matching
//!   every other device widget through [`adjust`].
//! - **No wrap-around.** Past the last choice is the last choice. A
//!   selector that jumps back to the first when you press Right is one
//!   that loses your place mid-sweep.
//! - **No double-click reset**, deliberately, and this is the one place
//!   the switch parts company with the knob. Every choice is already one
//!   click away, so a reset gesture buys nothing — and since choosing IS
//!   clicking, a double-click on a segment would fire it by accident
//!   during ordinary use. A gesture that misfires during the primary
//!   action is worse than no gesture.
//!
//! [`adjust`]: crate::ui::device::adjust

use crate::ui::device::design;
use crate::ui::device::metrics::{self, Footprint};
use crate::ui::device::param::Param;
use crate::ui::device::{adjust, param::Mapping};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// Padding either side of a segment's label.
const SEGMENT_PAD: f32 = space::SM;

/// The switch's size contract: the param's name above, the track below.
///
/// No value readout, unlike [`knob`]: the selected segment IS the
/// readout, and a switch that also printed its value underneath would say
/// the same word twice.
///
/// Every segment is the width of the WIDEST label, so the track divides
/// evenly. Segments sized to their own text would make "hp" a smaller
/// target than "notch", and the eye reads uneven segments as an
/// unfinished layout rather than as information.
///
/// [`knob`]: crate::ui::device::knob
pub fn footprint(ui: &egui::Ui, theme: &Theme, param: &Param) -> Footprint {
    let label = Footprint::new(
        metrics::text_w(ui, param.name, font::LABEL),
        metrics::line_h(ui, font::LABEL),
    );
    label.stack(
        design::gap(theme),
        Footprint::from_size(track_size(ui, theme, param)),
    )
}

/// The track's exact size. Both `footprint` and `switch` call THIS rather
/// than each working it out — the drawn track was briefly four points
/// shorter than the reserved one, because the drawing side derived its
/// height by subtracting the label from the total and egui's own item
/// spacing is not the design system's gap.
fn track_size(ui: &egui::Ui, theme: &Theme, param: &Param) -> egui::Vec2 {
    let count = param.choices().unwrap_or(1).max(1) as f32;
    egui::vec2(
        segment_w(ui, param) * count,
        theme
            .sp(control::SWITCH_H)
            .max(metrics::interactive_min(ui).y),
    )
}

/// One segment's width: the widest choice plus padding, so every segment
/// is the same size whatever it says.
fn segment_w(ui: &egui::Ui, param: &Param) -> f32 {
    let widest = choices(param)
        .iter()
        .map(|text| metrics::text_w(ui, text, font::LABEL))
        .fold(0.0f32, f32::max);
    widest + SEGMENT_PAD * 2.0
}

/// The choice labels, or a single empty one for a param that is not
/// discrete — a switch over a continuous param is a programming error, and
/// drawing one dead segment finds it faster than a panic in a paint call.
fn choices(param: &Param) -> Vec<String> {
    match param.choices() {
        Some(count) => (0..count)
            .map(|i| param.format(param.at_index(i as usize)))
            .collect(),
        None => vec![String::new()],
    }
}

/// Draw the switch. `norm` is the normalized value like every other
/// widget's; the segment it lands on is the one drawn selected. Returns
/// true when the user changed it.
pub fn switch(ui: &mut egui::Ui, theme: &Theme, param: &Param, norm: &mut f32) -> bool {
    let fp = footprint(ui, theme, param);
    let mut changed = false;

    // Claim exactly the contract's width in a centred column, the same
    // shape `knob` uses — so a switch and a knob in neighbouring wells
    // line up on their labels instead of drifting apart.
    let w = fp.width();
    ui.allocate_ui_with_layout(
        egui::vec2(w, 0.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.set_width(w);
            // The design system's gap between the name and the track, not
            // egui's default item spacing — otherwise the stack is taller
            // than the contract said and everything below it shifts.
            ui.spacing_mut().item_spacing.y = design::gap(theme);
            ui.label(
                egui::RichText::new(param.name)
                    .size(font::LABEL)
                    .color(theme.text_muted),
            );

            let labels = choices(param);
            let count = labels.len().max(1);
            let (rect, response) =
                ui.allocate_exact_size(track_size(ui, theme, param), egui::Sense::click_and_drag());

            let current = param.index(*norm).min(count - 1);
            let mut want = current;

            // Click or drag: whichever segment the pointer is over.
            if let Some(pos) = response.interact_pointer_pos()
                && (response.clicked() || response.dragged())
            {
                want = segment_at(rect, count, pos.x);
            }
            // Wheel and arrows: one choice per notch or press, clamped.
            let stepped = adjust::steps(ui, &response);
            if stepped != 0 {
                let next = want as i32 + stepped;
                want = next.clamp(0, count as i32 - 1) as usize;
            }

            if want != current {
                *norm = param.at_index(want);
                changed = true;
            }

            let hovered = response
                .hover_pos()
                .map(|pos| segment_at(rect, count, pos.x));
            paint(ui, theme, rect, &labels, want, hovered, &response);
        },
    );

    changed
}

/// Which segment a given x lands in. Clamped, so a drag that leaves the
/// track keeps the end segment rather than snapping back.
fn segment_at(rect: egui::Rect, count: usize, x: f32) -> usize {
    if rect.width() <= 0.0 || count == 0 {
        return 0;
    }
    let t = ((x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    ((t * count as f32) as usize).min(count - 1)
}

fn paint(
    ui: &egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    labels: &[String],
    selected: usize,
    hovered: Option<usize>,
    response: &egui::Response,
) {
    let painter = ui.painter();
    let count = labels.len().max(1);
    let seg_w = rect.width() / count as f32;

    // The track.
    painter.rect_filled(rect, design::box_radius(), theme.surface_sunken);
    painter.rect_stroke(
        rect,
        design::box_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );

    let seg_rect = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(rect.left() + seg_w * i as f32, rect.top()),
            egui::vec2(seg_w, rect.height()),
        )
    };

    // The selection, inset by the track's own hairline so the pill sits
    // INSIDE the groove rather than covering its edge.
    //
    // `accent_muted`, not `accent`. At full strength the pill was the
    // loudest thing on a card — brighter than the values, brighter than
    // the curve a device is actually about — and a mode selector is not
    // the most important control on any device. Muted, it still reads as
    // selected at a glance, and it stops shouting over the panel it is
    // part of. The label carries the rest: full-strength text on the
    // pill, muted everywhere else.
    let pill = seg_rect(selected).shrink(stroke::HAIR);
    painter.rect_filled(pill, design::box_radius(), theme.accent_muted);

    // Separators, but only between two UNSELECTED segments — a divider
    // running into the pill reads as a crack across it. This is the
    // detail every stock segmented control gets right and every hand-made
    // one forgets.
    for i in 1..count {
        if i == selected || i == selected + 1 {
            continue;
        }
        let x = rect.left() + seg_w * i as f32;
        painter.line_segment(
            [
                egui::pos2(x, rect.top() + stroke::FOCUS),
                egui::pos2(x, rect.bottom() - stroke::FOCUS),
            ],
            egui::Stroke::new(stroke::HAIR, theme.divider),
        );
    }

    for (i, text) in labels.iter().enumerate() {
        // Selected takes full-strength text on the muted pill: the
        // brightness moved from the fill to the word, which is the part
        // worth reading. Hovering an unselected segment lifts it the
        // same way — the only feedback saying "this one is clickable".
        // Selected and hovered read the same because they mean the same
        // thing to the eye — "this word is live" — and the pill already
        // says which of the two it is. Two shades of bright would be a
        // distinction nobody could name.
        let color = if i == selected || hovered == Some(i) {
            theme.text
        } else {
            theme.text_muted
        };
        painter.text(
            seg_rect(i).center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(font::LABEL),
            color,
        );
    }

    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }
}

/// A switch over a param that is not discrete is a programming error.
/// Named here so a caller can assert it rather than discover it.
pub fn is_discrete(param: &Param) -> bool {
    matches!(param.mapping, Mapping::Steps { .. })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const MODES: &[&str] = &["lp", "bp", "hp", "notch"];

    fn frame<R>(ctx: &egui::Context, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
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

    /// Every choice round-trips: index → normalized → index. The endpoints
    /// especially, since those are what a host stores and reloads.
    #[test]
    fn every_choice_round_trips_through_normalized() {
        let p = Param::choice("mode", MODES);
        assert_eq!(p.choices(), Some(4));
        for i in 0..4 {
            let n = p.at_index(i);
            assert_eq!(p.index(n), i, "choice {i} did not survive the round trip");
        }
        assert_eq!(p.at_index(0), 0.0, "the first choice is exactly 0.0");
        assert_eq!(p.at_index(3), 1.0, "the last is exactly 1.0");
        // Out of range clamps rather than wrapping or panicking.
        assert_eq!(p.at_index(99), 1.0);
    }

    /// A normalized value BETWEEN two choices lands on the nearer one, and
    /// every choice owns an equal band. Flooring instead of rounding would
    /// give the last choice a band of zero width.
    #[test]
    fn a_value_between_choices_snaps_to_the_nearest() {
        let p = Param::choice("mode", MODES);
        // Thirds, because four choices sit at 0, 1/3, 2/3, 1.
        assert_eq!(p.index(0.0), 0);
        assert_eq!(p.index(0.1), 0);
        assert_eq!(p.index(0.2), 1, "past the midpoint of the first band");
        assert_eq!(p.index(0.5), 2, "the exact middle rounds up, once");
        assert_eq!(p.index(0.9), 3);
        assert_eq!(p.index(1.0), 3);
        // Every band is reachable, and none is empty.
        let seen: std::collections::BTreeSet<usize> =
            (0..=100).map(|i| p.index(i as f32 / 100.0)).collect();
        assert_eq!(seen.len(), 4, "every choice owns a band of the range");
    }

    /// The readout is the choice NAME, not a number, and the widest text
    /// the contract reserves is the longest name.
    #[test]
    fn a_choice_formats_as_its_name() {
        let p = Param::choice("mode", MODES);
        assert_eq!(p.format(p.at_index(0)), "lp");
        assert_eq!(p.format(p.at_index(3)), "notch");
        assert_eq!(p.widest_text(), "notch");
    }

    /// Segments are EQUAL, sized by the widest label — and the track is
    /// exactly that times the count, so it divides without a remainder.
    #[test]
    fn segments_are_equal_and_fit_the_widest_label() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        frame(&ctx, |ui| {
            let p = Param::choice("mode", MODES);
            let seg = segment_w(ui, &p);
            let widest = metrics::text_w(ui, "notch", font::LABEL);
            assert!(seg >= widest, "a segment must hold the widest label");
            let fp = footprint(ui, &theme, &p);
            assert!(
                fp.width() >= seg * 4.0 - 0.5,
                "the track is the count times one segment"
            );
            // And it is never narrower than its own name.
            let long = Param::choice("filter response mode", MODES);
            assert!(
                footprint(ui, &theme, &long).width()
                    >= metrics::text_w(ui, "filter response mode", font::LABEL)
            );
        });
    }

    /// The segment under the pointer is the one chosen — including at the
    /// very edges, where an off-by-one would pick the neighbour.
    #[test]
    fn the_segment_under_the_pointer_is_the_one_chosen() {
        let rect = egui::Rect::from_min_size(egui::pos2(100.0, 0.0), egui::vec2(400.0, 20.0));
        assert_eq!(segment_at(rect, 4, 100.0), 0, "the left edge");
        assert_eq!(segment_at(rect, 4, 199.0), 0);
        assert_eq!(segment_at(rect, 4, 201.0), 1);
        assert_eq!(segment_at(rect, 4, 499.0), 3);
        assert_eq!(segment_at(rect, 4, 500.0), 3, "the right edge, not past it");
        // A drag that leaves the track keeps the end segment.
        assert_eq!(segment_at(rect, 4, -50.0), 0);
        assert_eq!(segment_at(rect, 4, 9999.0), 3);
        // Degenerate shapes answer rather than divide by zero.
        assert_eq!(segment_at(egui::Rect::NOTHING, 4, 0.0), 0);
        assert_eq!(segment_at(rect, 0, 250.0), 0);
    }

    /// Drive `switch` headlessly at `pos` and report what it selected.
    ///
    /// THREE frames, not two, and this is the harness detail that matters:
    /// egui establishes hover from the previous frame's pointer position,
    /// so a press delivered on the same frame the pointer first appears
    /// lands on nothing. Move, then press, then release.
    fn click_at(ctx: &egui::Context, host: egui::Rect, pos: egui::Pos2, norm: &mut f32) -> bool {
        let theme = Theme::dark();
        let p = Param::choice("mode", MODES);
        let mut changed = false;
        for pressed in [None, Some(true), Some(false)] {
            let mut run = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    events: {
                        let mut events = vec![egui::Event::PointerMoved(pos)];
                        if let Some(pressed) = pressed {
                            events.push(egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: Default::default(),
                            });
                        }
                        events
                    },
                    ..Default::default()
                },
                |ui| {
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(host)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    changed |= switch(&mut child, &theme, &p, norm);
                },
            );
            run.textures_delta.clear();
        }
        changed
    }

    /// Where the track sits inside a host rect, by the same arithmetic the
    /// widget uses: the name's line, the design gap, then the track.
    fn track_rect(ctx: &egui::Context, host: egui::Rect) -> egui::Rect {
        let theme = Theme::dark();
        let p = Param::choice("mode", MODES);
        frame(ctx, |ui| {
            let top = host.top() + metrics::line_h(ui, font::LABEL) + design::gap(&theme);
            egui::Rect::from_min_size(egui::pos2(host.left(), top), track_size(ui, &theme, &p))
        })
    }

    /// Clicking a segment selects THAT segment — not the nearest end, not
    /// the next one in a cycle.
    #[test]
    fn clicking_a_segment_selects_it() {
        let ctx = egui::Context::default();
        let p = Param::choice("mode", MODES);
        let host = egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(400.0, 120.0));
        let track = track_rect(&ctx, host);

        // Aim at the middle of each segment in turn.
        for want in [2usize, 0, 3, 1] {
            let mut norm = p.at_index(if want == 0 { 3 } else { 0 });
            let x = track.left() + track.width() * (want as f32 + 0.5) / 4.0;
            let changed = click_at(&ctx, host, egui::pos2(x, track.center().y), &mut norm);
            assert!(changed, "a click on segment {want} must register");
            assert_eq!(
                p.index(norm),
                want,
                "clicked segment {want}, got {}",
                p.index(norm)
            );
        }
    }

    /// Clicking the segment that is ALREADY selected changes nothing and
    /// reports nothing — a no-op edit that still emitted would send a
    /// pointless parameter letter on every stray click.
    #[test]
    fn reclicking_the_current_segment_is_a_no_op() {
        let ctx = egui::Context::default();
        let p = Param::choice("mode", MODES);
        let host = egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(400.0, 120.0));
        let track = track_rect(&ctx, host);

        let mut norm = p.at_index(1);
        let x = track.left() + track.width() * 1.5 / 4.0;
        let changed = click_at(&ctx, host, egui::pos2(x, track.center().y), &mut norm);
        assert!(!changed, "re-selecting the current choice is not a change");
        assert_eq!(p.index(norm), 1);
    }
}
