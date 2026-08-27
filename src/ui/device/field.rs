//! The value field: a number you can drag OR type.
//!
//! Nothing else in the device layer can set an exact value. A knob is for
//! finding a setting by ear; a field is for the times you already know the
//! answer — "the delay is 250 ms", "cut 3 dB at 1.2 k" — and every one of
//! those is currently unreachable by any amount of careful dragging.
//!
//! # The manners it keeps
//!
//! - **Click to type, drag to sweep**, decided by whether the pointer
//!   moved. Press-and-release opens the editor; press-and-move changes the
//!   value. This is how a value field behaves in every DAW, and it works
//!   because the two gestures start identically and diverge on their own.
//! - **The text arrives selected**, so typing replaces it rather than
//!   appending to it. A field that makes you clear it first is a field
//!   that gets "250250" typed into it.
//! - **Enter commits. Escape reverts. Clicking away commits** — the same
//!   three answers every text field in every application gives.
//! - **Nonsense reverts**, it does not become zero. Typing "abc" and
//!   pressing Enter leaves the value alone, because a field that reads
//!   unparseable text as silence is a field that can destroy a mix with a
//!   typo.
//! - **Out of range clamps**, and the field then shows what it clamped to,
//!   so the disagreement is visible rather than silent.
//! - **Units are understood, not required.** "250", "250ms", "0.25s" and
//!   "250 ms" are the same value; so are "1.5k" and "1500" on a Hz
//!   parameter. Whatever the readout prints must parse back — there is a
//!   round-trip test over every unit for exactly that.
//! - **Shift is the fine modifier** while dragging, like everywhere else.
//!
//! # What it deliberately does NOT do
//!
//! No double-click-to-reset. On a knob that gesture is free, but here
//! double-click is how you *select a word* while editing, and choosing
//! between "reset the parameter" and "select a word" on one gesture would
//! get it wrong in both directions. Reset lives on the knob.

use crate::ui::device::adjust;
use crate::ui::device::design;
use crate::ui::device::metrics::{self, Footprint};
use crate::ui::device::param::{Param, Unit};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, stroke};
use eframe::egui;

// -------------------------------------------------------------- parse ---

/// Read a typed value as this parameter's NATURAL unit.
///
/// `None` means "leave the value alone" — the caller must not treat a
/// failed parse as a number. Pure, so the whole grammar is testable
/// without a window; this is the part of the widget most likely to be
/// subtly wrong, and the part a screenshot can never check.
pub fn parse(param: &Param, text: &str) -> Option<f32> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    // A discrete parameter accepts its own choice NAMES, which is the
    // whole reason someone would type into one. A bare number is taken as
    // the index, so both "hp" and "2" work.
    if let Unit::Choice(names) = param.unit {
        let lowered = text.to_ascii_lowercase();
        if let Some(i) = names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(&lowered))
            .or_else(|| {
                // A unique prefix is enough — "no" finds "notch" as long
                // as nothing else starts with it. Ambiguity falls through
                // rather than guessing.
                let hits: Vec<usize> = names
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| n.to_ascii_lowercase().starts_with(&lowered))
                    .map(|(i, _)| i)
                    .collect();
                (hits.len() == 1).then(|| hits[0])
            })
        {
            return Some(i as f32);
        }
    }

    let (number, suffix) = split_number(text)?;
    let suffix = suffix.trim().to_ascii_lowercase();
    let scale = scale_for(param.unit, &suffix)?;
    Some(number * scale)
}

/// Split "1.25 kHz" into `(1.25, "kHz")`.
///
/// Takes the LONGEST leading run that parses as a number, which is what
/// makes "1e3" and "-6.0" work without a hand-rolled grammar — and what
/// stops "1.2.3" from being read as 1.2.
fn split_number(text: &str) -> Option<(f32, &str)> {
    let mut end = 0usize;
    for (i, c) in text.char_indices() {
        let upto = i + c.len_utf8();
        if text[..upto].trim_end().parse::<f32>().is_ok() {
            end = upto;
        }
    }
    if end == 0 {
        return None;
    }
    let value: f32 = text[..end].trim_end().parse().ok()?;
    value.is_finite().then_some((value, &text[end..]))
}

/// What to multiply a typed number by, given the parameter's unit and the
/// suffix the user wrote. `None` rejects a suffix that means nothing here,
/// so "250 potato" reverts instead of quietly becoming 250.
fn scale_for(unit: Unit, suffix: &str) -> Option<f32> {
    // The empty suffix always means "the natural unit", for every kind of
    // parameter. Typing a bare number is the common case and must never
    // depend on knowing what the field wanted.
    if suffix.is_empty() {
        return Some(1.0);
    }
    match unit {
        // ORDER MATTERS on the time units: "ms" ends in "s", so a plain
        // `ends_with("s")` test would read milliseconds as seconds and be
        // wrong by a thousand.
        Unit::Ms => match suffix {
            "ms" | "msec" => Some(1.0),
            "s" | "sec" | "secs" => Some(1000.0),
            _ => None,
        },
        Unit::Seconds => match suffix {
            "s" | "sec" | "secs" => Some(1.0),
            "ms" | "msec" => Some(0.001),
            _ => None,
        },
        Unit::Hz => match suffix {
            "hz" => Some(1.0),
            "k" | "khz" => Some(1000.0),
            _ => None,
        },
        Unit::Db => matches!(suffix, "db").then_some(1.0),
        // A note number takes a bare number and nothing else. "60 C" is
        // not a unit, and accepting a name here would need a parser that
        // knows about sharps, octaves, and which C is middle C — a field
        // is not where that argument belongs.
        Unit::Note => None,
        Unit::Percent => matches!(suffix, "%" | "pct").then_some(1.0),
        Unit::Semitones => matches!(suffix, "st" | "semi" | "semitones").then_some(1.0),
        // "4x" and a bare "4" mean the same factor.
        Unit::Ratio => matches!(suffix, "x").then_some(1.0),
        Unit::Plain | Unit::Choice(_) => None,
    }
}

// --------------------------------------------------------------- view ---

/// The field's size contract: a box wide enough for the widest value this
/// parameter can ever show.
///
/// The WIDEST, not the current one — a field that resized as its own
/// digits changed would shove its neighbours around while being dragged,
/// which is precisely when the pointer needs everything to hold still.
pub fn footprint(ui: &egui::Ui, theme: &Theme, param: &Param) -> Footprint {
    let text = metrics::mono_w(ui, &param.widest_text(), font::VALUE);
    Footprint::new(
        text + design::field_pad(theme) * 2.0,
        metrics::line_h(ui, font::VALUE).max(metrics::interactive_min(ui).y * 0.75),
    )
}

/// What the field is doing between frames: the text being edited, and
/// whether it has been given the keyboard yet.
#[derive(Clone)]
struct Editing {
    text: String,
    focused: bool,
}

/// Draw the field. Returns true when `norm` changed this frame.
pub fn field(ui: &mut egui::Ui, theme: &Theme, param: &Param, norm: &mut f32) -> bool {
    let size = footprint(ui, theme, param).size;
    let id = ui.next_auto_id().with("field");
    ui.skip_ahead_auto_ids(1);
    let editing: Option<Editing> = ui.data(|d| d.get_temp(id));

    match editing {
        Some(state) => edit(ui, theme, param, norm, id, size, state),
        None => display(ui, theme, param, norm, id, size),
    }
}

/// The resting state: a value you can drag, or click to start typing.
fn display(
    ui: &mut egui::Ui,
    theme: &Theme,
    param: &Param,
    norm: &mut f32,
    id: egui::Id,
    size: egui::Vec2,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let mut changed = false;

    // Drag sweeps the range. `dragged()` and `clicked()` are mutually
    // exclusive in egui — it decides by how far the pointer moved — which
    // is exactly the distinction this widget needs and does not have to
    // make for itself.
    if response.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let travel = control::DRAG_TRAVEL / if fine { adjust::FINE } else { 1.0 };
        let delta = response.drag_delta().x / travel;
        if delta != 0.0 {
            let next = (*norm + delta).clamp(0.0, 1.0);
            if next != *norm {
                *norm = next;
                changed = true;
            }
        }
    }
    // Wheel and arrows, the same manners as every other device widget.
    let nudge = adjust::nudge(ui, &response);
    if nudge != 0.0 {
        let next = (*norm + nudge).clamp(0.0, 1.0);
        if next != *norm {
            *norm = next;
            changed = true;
        }
    }

    // A click that was NOT a drag opens the editor, pre-filled with what
    // the field is showing — so the number you saw is the number you edit.
    if response.clicked() {
        ui.data_mut(|d| {
            d.insert_temp(
                id,
                Editing {
                    text: param.format(*norm),
                    focused: false,
                },
            );
        });
        ui.ctx().request_repaint();
    }

    let hovered = response.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    let painter = ui.painter();
    // A box only on hover: at rest this is a readout, and a permanent
    // frame around every number turns a device into a spreadsheet. The
    // box appearing under the pointer is the affordance.
    if hovered {
        painter.rect_filled(rect, design::box_radius(), theme.surface_sunken);
        painter.rect_stroke(
            rect,
            design::box_radius(),
            egui::Stroke::new(stroke::HAIR, theme.outline),
            egui::StrokeKind::Inside,
        );
    }
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        param.format(*norm),
        egui::FontId::monospace(font::VALUE),
        theme.text_value,
    );
    if response.has_focus() {
        design::focus_ring(painter, theme, rect);
    }

    changed
}

/// The editing state: a text box over the value.
fn edit(
    ui: &mut egui::Ui,
    theme: &Theme,
    param: &Param,
    norm: &mut f32,
    id: egui::Id,
    size: egui::Vec2,
    mut state: Editing,
) -> bool {
    let mut output = egui::TextEdit::singleline(&mut state.text)
        .font(egui::FontId::monospace(font::VALUE))
        .horizontal_align(egui::Align::Center)
        .desired_width(size.x)
        .margin(design::field_margin(theme))
        .show(ui);

    if !state.focused {
        output.response.request_focus();
        // Arrive with everything selected, so the first keystroke replaces
        // the value instead of appending to it.
        let end = state.text.chars().count();
        output
            .state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(end),
            )));
        output.state.clone().store(ui.ctx(), output.response.id);
        state.focused = true;
    }

    // Escape first: it must win over anything else the frame delivers.
    let escaped = ui
        .ctx()
        .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    let entered =
        output.response.lost_focus() && ui.ctx().input(|i| i.key_pressed(egui::Key::Enter));
    // Clicking away commits, the same as every other text field anywhere.
    let left = output.response.lost_focus() && !entered;

    let mut changed = false;
    if escaped {
        ui.data_mut(|d| d.remove::<Editing>(id));
    } else if entered || left {
        if let Some(natural) = parse(param, &state.text) {
            // `to_norm` clamps, so an out-of-range number lands at the end
            // of the range and the readout shows what it became.
            let next = param.mapping.to_norm(natural);
            if next != *norm {
                *norm = next;
                changed = true;
            }
        }
        // A failed parse falls through: the value is untouched and the
        // editor closes, which reads as "that meant nothing" rather than
        // as a silent zero.
        ui.data_mut(|d| d.remove::<Editing>(id));
    } else {
        ui.data_mut(|d| d.insert_temp(id, state));
    }
    changed
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ui::device::param::Mapping;

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

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    /// A bare number always means the parameter's own unit. This is the
    /// common case and must never require knowing what the field wanted.
    #[test]
    fn a_bare_number_is_the_natural_unit() {
        assert_eq!(parse(&Param::ms("t", 1.0, 10_000.0), "250"), Some(250.0));
        assert_eq!(parse(&Param::hz("f", 20.0, 20_000.0), "440"), Some(440.0));
        assert_eq!(parse(&Param::db("g", -60.0, 6.0), "-6"), Some(-6.0));
        assert_eq!(parse(&Param::percent("m"), "50"), Some(50.0));
        // Signs, decimals and exponents all come free from the number
        // scanner rather than from a hand-rolled grammar.
        assert_eq!(parse(&Param::db("g", -60.0, 6.0), "+3.5"), Some(3.5));
        assert_eq!(parse(&Param::hz("f", 20.0, 20_000.0), "1e3"), Some(1000.0));
    }

    /// Units are understood when written, in the spellings people use.
    #[test]
    fn units_are_understood_when_written() {
        let ms = Param::ms("t", 1.0, 10_000.0);
        assert_eq!(parse(&ms, "250ms"), Some(250.0));
        assert_eq!(parse(&ms, "250 ms"), Some(250.0));
        assert_eq!(parse(&ms, "  250   MS  "), Some(250.0), "case and spacing");
        // THE trap: "ms" ends with "s". A naive suffix test reads
        // milliseconds as seconds and is wrong by a factor of a thousand.
        assert_eq!(parse(&ms, "0.25s"), Some(250.0));
        assert_eq!(parse(&ms, "2s"), Some(2000.0));

        let sec = Param::new(
            "t",
            Mapping::Linear {
                min: 0.0,
                max: 10.0,
            },
            Unit::Seconds,
        );
        assert_eq!(parse(&sec, "500ms"), Some(0.5));
        assert_eq!(parse(&sec, "1.5s"), Some(1.5));

        let hz = Param::hz("f", 20.0, 20_000.0);
        assert_eq!(parse(&hz, "440hz"), Some(440.0));
        assert_eq!(parse(&hz, "1.5k"), Some(1500.0));
        assert_eq!(parse(&hz, "1.25 kHz"), Some(1250.0));

        assert_eq!(parse(&Param::db("g", -60.0, 6.0), "-6 dB"), Some(-6.0));
        assert_eq!(parse(&Param::percent("m"), "50 %"), Some(50.0));
    }

    /// Nonsense is refused rather than read as zero. A field that turns a
    /// typo into silence can destroy a mix.
    #[test]
    fn nonsense_is_refused_not_zeroed() {
        let ms = Param::ms("t", 1.0, 10_000.0);
        for junk in ["", "   ", "abc", "-", ".", "ms", "+", "e"] {
            assert_eq!(parse(&ms, junk), None, "{junk:?} must not parse");
        }
        // A unit that means nothing HERE is refused too, rather than
        // having its number quietly accepted.
        assert_eq!(parse(&ms, "250 hz"), None);
        assert_eq!(parse(&Param::percent("m"), "50 db"), None);
        assert_eq!(parse(&Param::hz("f", 20.0, 20_000.0), "440 potato"), None);
        // A number with trailing junk is not a number: the scanner finds
        // "1.2", and ".3" is not a unit this parameter knows, so the
        // whole thing is refused rather than half-read.
        assert_eq!(parse(&ms, "1.2.3"), None);
    }

    /// A discrete parameter takes its own choice names — the only reason
    /// anyone would type into one.
    #[test]
    fn a_discrete_field_accepts_choice_names() {
        let mode = Param::choice("mode", &["lp", "bp", "hp", "notch"]);
        assert_eq!(parse(&mode, "hp"), Some(2.0));
        assert_eq!(parse(&mode, "HP"), Some(2.0), "case does not matter");
        assert_eq!(parse(&mode, "notch"), Some(3.0));
        // A unique prefix is enough; an ambiguous one is not a guess.
        assert_eq!(parse(&mode, "no"), Some(3.0));
        assert_eq!(parse(&mode, "p"), None, "matches nothing");
        // A bare number is the index, so both routes work.
        assert_eq!(parse(&mode, "1"), Some(1.0));
        assert_eq!(parse(&mode, "zzz"), None);
    }

    /// THE property that matters: opening the editor and pressing Enter
    /// without touching anything must not change what the field shows.
    ///
    /// Stated as a string round-trip rather than a numeric one on
    /// purpose. A numeric comparison needs a tolerance, and the honest
    /// tolerance is "half of the last digit the readout printed" — which
    /// varies per unit and per magnitude, so picking one number for it
    /// either passes things that visibly change or fails things that
    /// cannot. Comparing the RENDERED text asks the real question: does
    /// the user see the same value afterwards?
    #[test]
    fn a_readout_survives_a_round_trip_through_the_editor() {
        let params = [
            Param::hz("f", 20.0, 20_000.0),
            Param::db("g", -60.0, 6.0),
            Param::percent("m"),
            Param::ms("t", 0.05, 30_000.0),
            Param::new(
                "t",
                Mapping::Linear {
                    min: 0.0,
                    max: 10.0,
                },
                Unit::Seconds,
            ),
            Param::new(
                "p",
                Mapping::Linear {
                    min: -24.0,
                    max: 24.0,
                },
                Unit::Semitones,
            ),
            Param::new("x", Mapping::Linear { min: 0.0, max: 2.0 }, Unit::Plain),
            // Log-mapped, as a drive control actually is — so the
            // round trip is tested through the mapping that can lose
            // precision, not only through a linear one.
            Param::new(
                "drive",
                Mapping::Log {
                    min: 1.0,
                    max: 32.0,
                },
                Unit::Ratio,
            ),
            Param::choice("mode", &["lp", "bp", "hp", "notch"]),
        ];
        for param in &params {
            for i in 0..=40 {
                let norm = i as f32 / 40.0;
                let shown = param.format(norm);
                let parsed = parse(param, &shown).unwrap_or_else(|| {
                    panic!("'{shown}' came from {} and will not parse back", param.name)
                });
                let again = param.format(param.mapping.to_norm(parsed));
                assert_eq!(
                    again, shown,
                    "{}: '{shown}' became '{again}' after a no-op edit",
                    param.name
                );
            }
        }
    }

    /// And the value itself lands where it was, to the precision the
    /// readout was able to state.
    #[test]
    fn a_parsed_value_lands_where_the_readout_said() {
        let param = Param::hz("f", 20.0, 20_000.0);
        for i in 0..=40 {
            let norm = i as f32 / 40.0;
            let want = param.value(norm);
            let parsed = parse(&param, &param.format(norm)).unwrap();
            // Hz prints three significant figures at most, so a percent
            // of the value is the tightest honest bound.
            assert!(
                close(parsed, want, want.abs() * 0.01),
                "showed '{}' for {want}, parsed {parsed}",
                param.format(norm)
            );
        }
    }

    /// `Plain` has no unit to write, so it takes bare numbers only —
    /// stated as a test because "no suffix is valid" is easy to read as
    /// "any suffix is valid".
    #[test]
    fn a_plain_number_takes_no_suffix() {
        let plain = Param::new("x", Mapping::Linear { min: 0.0, max: 2.0 }, Unit::Plain);
        assert_eq!(parse(&plain, "1.25"), Some(1.25));
        assert_eq!(parse(&plain, "1.25 x"), None);
    }

    /// Out-of-range typing clamps into the parameter's range rather than
    /// being refused — the user said "as much as possible", and the field
    /// then shows what that turned out to be.
    #[test]
    fn out_of_range_clamps_into_the_parameter() {
        let hz = Param::hz("f", 20.0, 20_000.0);
        let parsed = parse(&hz, "999999").unwrap();
        let norm = hz.mapping.to_norm(parsed);
        assert_eq!(norm, 1.0);
        assert!(close(hz.value(norm), 20_000.0, 1.0), "clamped to the top");

        let low = hz.mapping.to_norm(parse(&hz, "1").unwrap());
        assert_eq!(low, 0.0);
    }

    /// Drive a real `field` headlessly through a scripted gesture.
    ///
    /// `steps` is one entry per frame; each is the events for that frame.
    /// Returns the normalized value afterwards.
    fn drive(param: &Param, start: f32, steps: &[Vec<egui::Event>]) -> f32 {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let host = egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(300.0, 60.0));
        let mut norm = start;
        for events in steps {
            let mut run = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    events: events.clone(),
                    ..Default::default()
                },
                |ui| {
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(host)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    field(&mut child, &theme, param, &mut norm);
                },
            );
            run.textures_delta.clear();
        }
        norm
    }

    /// The pointer sits over the field's box, which starts at the host's
    /// top-left corner.
    fn over() -> egui::Pos2 {
        egui::pos2(110.0, 106.0)
    }

    fn moved() -> Vec<egui::Event> {
        vec![egui::Event::PointerMoved(over())]
    }

    fn button(pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(over()),
            egui::Event::PointerButton {
                pos: over(),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]
    }

    fn typed(text: &str) -> Vec<egui::Event> {
        vec![egui::Event::Text(text.to_owned())]
    }

    fn key(k: egui::Key) -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }]
    }

    /// The whole point of the widget: click, type, Enter, and the value is
    /// exactly what you asked for.
    #[test]
    fn clicking_and_typing_sets_an_exact_value() {
        let hz = Param::hz("cutoff", 20.0, 20_000.0);
        let after = drive(
            &hz,
            0.5,
            &[
                moved(),
                button(true),
                button(false), // a click, not a drag: the editor opens
                vec![],        // the editor draws and takes the keyboard
                typed("1.5k"), // and the selection is replaced
                key(egui::Key::Enter),
            ],
        );
        assert!(
            close(hz.value(after), 1500.0, 1.0),
            "typing 1.5k should give 1500 Hz, got {}",
            hz.value(after)
        );
    }

    /// Escape reverts. Whatever was typed, the value is untouched.
    #[test]
    fn escape_reverts_the_edit() {
        let hz = Param::hz("cutoff", 20.0, 20_000.0);
        let before = 0.5f32;
        let after = drive(
            &hz,
            before,
            &[
                moved(),
                button(true),
                button(false),
                vec![],
                typed("9999"),
                key(egui::Key::Escape),
            ],
        );
        assert_eq!(after, before, "Escape must leave the value alone");
    }

    /// Nonsense committed with Enter leaves the value alone rather than
    /// becoming zero — the failure that can wreck a mix with a typo.
    #[test]
    fn typing_nonsense_leaves_the_value_alone() {
        let hz = Param::hz("cutoff", 20.0, 20_000.0);
        let before = 0.5f32;
        let after = drive(
            &hz,
            before,
            &[
                moved(),
                button(true),
                button(false),
                vec![],
                typed("banana"),
                key(egui::Key::Enter),
            ],
        );
        assert_eq!(after, before, "unparseable text is not a number");
    }

    /// A DRAG sweeps the value and does not open the editor — the other
    /// half of the click-versus-drag split, and the half that would be
    /// unusable if the editor popped open every time you tried to sweep.
    #[test]
    fn dragging_sweeps_without_opening_the_editor() {
        let hz = Param::hz("cutoff", 20.0, 20_000.0);
        let start = egui::pos2(110.0, 106.0);
        let far = egui::pos2(190.0, 106.0);
        let drag = |pos: egui::Pos2| vec![egui::Event::PointerMoved(pos)];
        let after = drive(
            &hz,
            0.5,
            &[
                drag(start),
                vec![
                    egui::Event::PointerMoved(start),
                    egui::Event::PointerButton {
                        pos: start,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ],
                drag(far),
                drag(far),
                vec![
                    egui::Event::PointerMoved(far),
                    egui::Event::PointerButton {
                        pos: far,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    },
                ],
                // If the editor had opened, this text would be committed.
                typed("42"),
                key(egui::Key::Enter),
            ],
        );
        assert!(after > 0.5, "dragging right raises the value, got {after}");
        assert!(
            !close(hz.value(after), 42.0, 1.0),
            "a drag must not have opened the editor"
        );
    }

    /// The box is sized for the WIDEST value, so it cannot resize while
    /// being dragged — which is exactly when the pointer needs everything
    /// to hold still.
    #[test]
    fn the_box_fits_the_widest_value_it_can_show() {
        ctx_ui(|ui| {
            let theme = Theme::dark();
            let ms = Param::ms("t", 0.05, 30_000.0);
            let fp = footprint(ui, &theme, &ms);
            let widest = metrics::mono_w(ui, &ms.widest_text(), font::VALUE);
            assert!(fp.width() >= widest, "the widest reading must fit");
            for i in 0..=20 {
                let shown = ms.format(i as f32 / 20.0);
                assert!(
                    metrics::mono_w(ui, &shown, font::VALUE) <= widest + 0.5,
                    "'{shown}' is wider than the reserved box"
                );
            }
        });
    }
}
