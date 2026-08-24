//! The device card: the container every device UI lives in.
//!
//! A device chain is a horizontal strip of cards. Height is LOCKED to
//! [`control::DEVICE_H`] — every card in a rack is exactly as tall as its
//! neighbors, which is what makes a chain read as one strip instead of a
//! shelf of mismatched boxes. Width is the card's own business: it grows
//! with content, never below [`control::DEVICE_W_MIN`].
//!
//! The card is generic: a title strip and an empty body. Devices fill the
//! body with `device` widgets; a card with no content is a valid (if
//! silent) device.

use crate::ui::device::design;
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space};
use eframe::egui;

/// A titled, fixed-height, content-width card. Returns the closure's
/// result. `add` lays out the device body; pass a no-op for an empty
/// card.
pub fn card<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let mut page = 0;
    tabbed_card(ui, theme, name, 1, &mut page, |ui, _page| add(ui))
}

/// Dot radius, as a fraction of the dot's hit box.
const DOT_R: f32 = 0.3;

/// A card whose body has `pages` tabs, switched by the row of dots at the
/// top-left of the title strip. The caller owns which page is open
/// (`page`, clamped into range); `add` lays out the body for the page it
/// is given. One dot per page — filled accent when open, dim otherwise —
/// and a single page draws no dots at all, which is what makes [`card`]
/// this function's trivial case.
pub fn tabbed_card<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    pages: usize,
    page: &mut usize,
    add: impl FnOnce(&mut egui::Ui, usize) -> R,
) -> R {
    let pages = pages.max(1);
    *page = (*page).min(pages - 1);

    let mut rule_y = 0.0f32;
    let out = design::card_frame(theme).show(ui, |ui| {
        // Lock the outer height; width follows content. In a region
        // shorter than the token, clamp to what fits — every card in
        // the row sees the same available height, so the rack still
        // reads as one strip.
        ui.set_height(theme.sp(control::DEVICE_H).min(ui.available_height()));
        ui.set_min_width(theme.sp(control::DEVICE_W_MIN));

        ui.vertical(|ui| {
            // Title strip: tab dots first (top-left), then the name.
            let strip = design::title_strip(theme).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if pages > 1 {
                        dot_row(ui, theme, pages, page);
                        ui.add_space(theme.sp(space::XS));
                    }
                    ui.label(
                        egui::RichText::new(name)
                            .size(font::LABEL)
                            .color(theme.text_muted),
                    );
                });
            });
            // The title rule is PAINTED across the card's final rect
            // after layout, never allocated: kit::rule takes
            // available_width, and inside a scroll area that is the
            // whole rack — one rule call and the card balloons.
            rule_y = strip.response.rect.bottom();

            // Body: whatever remains of the locked height. Top-anchored,
            // NOT centered: sections size themselves from the remaining
            // height, and centering a child that is about to claim the
            // full height just shoves it downward by half the estimate
            // error. Wells center their own content; the body stays put.
            design::body(theme)
                .show(ui, |ui| {
                    ui.set_height(ui.available_height());
                    ui.horizontal_top(|ui| add(ui, *page)).inner
                })
                .inner
        })
        .inner
    });

    let rect = out.response.rect;
    ui.painter().hline(
        rect.x_range(),
        rule_y,
        egui::Stroke::new(crate::ui::tokens::stroke::HAIR, theme.divider),
    );
    out.inner
}

/// The clickable dots. Each dot is its own allocation, so ids stay unique
/// without ceremony.
fn dot_row(ui: &mut egui::Ui, theme: &Theme, pages: usize, page: &mut usize) {
    for i in 0..pages {
        let d = theme.sp(space::SM);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::click());
        if response.clicked() {
            *page = i;
        }
        let open = i == *page;
        let color = if open {
            theme.accent
        } else if response.hovered() {
            theme.text_muted
        } else {
            theme.outline
        };
        // The open page's dot is drawn a shade larger as well as brighter,
        // so the state survives squinting (and non-color vision).
        let r = rect.width() * if open { DOT_R + DOT_R * 0.5 } else { DOT_R };
        ui.painter().circle_filled(rect.center(), r, color);
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("page {} of {pages}", i + 1));
    }
}

/// An empty card: title strip over a blank body. The placeholder while a
/// device's UI does not exist yet — and the proof any card is never
/// zero-size.
pub fn empty_card(ui: &mut egui::Ui, theme: &Theme, name: &str) {
    card(ui, theme, name, |_ui| {});
}

/// Divide a card body into `cols`×`rows` sub-panels with visible
/// boundaries — the shelving a device's widgets sit on. Any division
/// count: 3×2, 2×2, 6×1, whatever the device's layout wants.
///
/// Each section is a sunken well (darker than the card, hairline edge).
/// Rows split the body height equally so the boundaries line up across
/// columns; each column is as wide as its widest content, never narrower
/// than [`control::SECTION_W_MIN`]. `add` is called once per section with
/// its row-major index — leave a section empty and it stays a visible,
/// waiting well rather than collapsing.
pub fn sections(
    ui: &mut egui::Ui,
    theme: &Theme,
    cols: usize,
    rows: usize,
    mut add: impl FnMut(&mut egui::Ui, usize),
) {
    let (cols, rows) = (cols.max(1), rows.max(1));
    let gap = theme.sp(space::XS);
    let row_h = ((ui.available_height() - gap * (rows - 1) as f32) / rows as f32).max(0.0);
    // Inner height: the row minus the well's own margins.
    let inner_h = (row_h - design::well_pad(theme) * 2.0).max(0.0);

    // The grid needs an id of its own so two `sections` calls in one card
    // never collide (the duplicate-widget-id lesson, learned once already).
    let id = ui.next_auto_id();
    ui.skip_ahead_auto_ids(1);
    // The wells' remembered content sizes need a STABLE id — an auto id
    // can shift between frames, and then every frame reads as the first:
    // no stored measurement, no centering, knobs pinned to the well top.
    let data_id = ui.id().with(("sections", cols, rows));

    egui::Grid::new(id)
        .spacing(egui::vec2(gap, gap))
        .show(ui, |ui| {
            for r in 0..rows {
                for c in 0..cols {
                    let i = r * cols + c;
                    design::well(theme).show(ui, |ui| {
                        // Center the content in the well, both axes,
                        // and make the well HUG the content: last
                        // frame's measured size sets this frame's well
                        // (the standard egui two-pass trick — one frame
                        // of settling, then optically stable). Without
                        // the width pin, `vertical_centered` greedily
                        // takes all available width and a three-knob
                        // card balloons across the whole rack.
                        let well_id = data_id.with(("well", i));
                        let min_w = theme.sp(control::SECTION_W_MIN);
                        let last: egui::Vec2 = ui
                            .data(|d| d.get_temp(well_id))
                            .unwrap_or(egui::vec2(min_w, inner_h));
                        let well_w = last.x.max(min_w);
                        ui.set_min_width(well_w);
                        ui.set_max_width(well_w);
                        ui.set_min_height(inner_h);
                        ui.set_max_height(inner_h);

                        // A Grid cell's Ui is HORIZONTAL, and a plain
                        // `vertical()` child inside one sizes to its
                        // CONTENT — so centering within it centers
                        // nothing. Allocate the well's exact area with a
                        // top-down, center-aligned layout instead: that
                        // one layout owns both axes (x from the align, y
                        // from the leading pad).
                        let content = ui
                            .allocate_ui_with_layout(
                                egui::vec2(well_w, inner_h),
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| {
                                    ui.add_space(((inner_h - last.y) * 0.5).max(0.0));
                                    // Measure the CONTENT only: the pad
                                    // shares this scope, and folding it
                                    // into the stored height would feed
                                    // back into next frame's pad.
                                    let top = ui.min_rect().bottom();
                                    add(ui, i);
                                    let r = ui.min_rect();
                                    egui::vec2(r.width(), r.bottom() - top)
                                },
                            )
                            .inner;
                        ui.data_mut(|d| d.insert_temp(well_id, content));
                    });
                }
                ui.end_row();
            }
        });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Drive the well-centering two-pass headlessly: after the first
    /// settle frame, fixed-size content must sit vertically centered in
    /// its well, and stay put on later frames.
    #[test]
    fn wells_center_their_content_after_one_settle_frame() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };

        let mut tops: Vec<f32> = Vec::new();
        let mut well_tops: Vec<f32> = Vec::new();
        // (content rect, well rect) per frame, for the centering asserts.
        let mut pairs: Vec<(egui::Rect, egui::Rect)> = Vec::new();
        for _ in 0..4 {
            let mut out = ctx.run_ui(input.take(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    sections(ui, &theme, 1, 1, |ui, _| {
                        let well = ui.max_rect();
                        well_tops.push(well.top());
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(40.0, 80.0), egui::Sense::hover());
                        tops.push(r.top());
                        pairs.push((r, well));
                    });
                });
            });
            out.textures_delta.clear();
        }

        // Frame 1 has no measurement yet (content pinned high); by frame 3
        // the offset must be real and stable.
        assert!(
            tops[2] > tops[0] + 20.0,
            "no centering happened: tops = {tops:?}, wells = {well_tops:?}"
        );
        assert!(
            (tops[3] - tops[2]).abs() < 1.0,
            "centering did not settle: tops = {tops:?}"
        );

        // Both axes, on the settled frame: the content's center must sit
        // on the well's center. (The horizontal half regressed once — a
        // Grid cell is a horizontal Ui, so a `vertical()` child sizes to
        // its content and centering inside it centers nothing.)
        let (content, well) = pairs[3];
        let _ = &well_tops;
        assert!(
            (content.center().y - well.center().y).abs() < 2.0,
            "content not vertically centered: content {content:?} well {well:?}"
        );
        assert!(
            (content.center().x - well.center().x).abs() < 2.0,
            "content not horizontally centered: content {content:?} well {well:?}"
        );
    }

    /// The same guarantee with a REAL widget in the well. The bare-rect
    /// test above passed while actual knobs sat left of centre: `knob`
    /// wrapped its stack in `vertical()`, which claims the full width and
    /// then shrinks, pinning the stack left. Widgets, not rects, are what
    /// ship — so one test drives one.
    #[test]
    fn a_real_knob_lands_centered_in_its_well() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let param = crate::ui::device::Param::percent("mix");
        let mut norm = 0.5f32;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };

        let mut pairs: Vec<(egui::Rect, egui::Rect)> = Vec::new();
        for _ in 0..4 {
            let mut out = ctx.run_ui(input.take(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    sections(ui, &theme, 1, 1, |ui, _| {
                        let well = ui.max_rect();
                        let before = ui.min_rect().bottom();
                        crate::ui::device::knob::knob(ui, &theme, &param, &mut norm);
                        let r = ui.min_rect();
                        let content = egui::Rect::from_min_max(
                            egui::pos2(r.left(), before),
                            egui::pos2(r.right(), r.bottom()),
                        );
                        pairs.push((content, well));
                    });
                });
            });
            out.textures_delta.clear();
        }

        let (content, well) = pairs[3];
        assert!(
            (content.center().x - well.center().x).abs() < 2.0,
            "knob not horizontally centered: content {content:?} well {well:?}"
        );
        assert!(
            (content.center().y - well.center().y).abs() < 3.0,
            "knob not vertically centered: content {content:?} well {well:?}"
        );
    }
}
