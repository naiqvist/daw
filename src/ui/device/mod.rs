//! Device widgets: the synth/FX control vocabulary, and the wiring layer
//! that binds a widget to an audio parameter without either knowing the
//! other.
//!
//! # Where this sits
//!
//! `device` is `kit`'s sibling at layer 3 — it may use tokens, theme, vm,
//! action, kit, and egui, and it is the second of exactly two places where
//! custom painting (`ui.painter()`) is allowed. It must never import
//! panels, the host, or the engine (enforced by the layer test in
//! `ui::tests`). Direction between the siblings is one-way: `device` may
//! call `kit`; `kit` must never know `device`.
//!
//! # The wiring
//!
//! Every widget speaks NORMALIZED values (`f32` in `0..=1`) and a
//! [`Param`] descriptor that owns the mapping to the natural unit
//! (Hz, dB, ms, ...) and its formatting. Panels hold the normalized value,
//! hand it to a widget, and return the edit as an action; the app layer
//! translates to engine commands. Widgets never see engine types, and the
//! engine never sees widgets — the `Param` is the entire interface.
//!
//! Normalized-value convention (same reason `kit::meter` takes `norm`):
//! parameter *scaling* is decided once, in the `Param`, instead of once per
//! widget per panel.
//!
//! # What lives here
//!
//! - [`param`]  — `Param`/`Mapping`/`Unit`: the wiring vocabulary.
//! - [`card`]   — the fixed-height, content-width container every device
//!   UI lives in, and the `Wells` layout that divides a card body into
//!   weighted, exactly-tiling sub-panels — `sections` for a plain grid,
//!   `sub_wells` for one more level of grouping inside a well. A chain of
//!   cards is a device rack.
//! - [`bezier`] — pure curve math (no egui): eval, y-at-x, polylines,
//!   bendable segments. Shared by the envelope editor and any future
//!   curve display.
//! - [`metrics`] — `Footprint`: the SIZE CONTRACT every widget publishes,
//!   so a container can reserve room for a control's text before the
//!   control draws. Every widget module exposes a `footprint()` beside its
//!   draw function, and the draw function allocates exactly that.
//! - [`knob`]   — param-aware rotary, unipolar and bipolar.
//! - [`fader`]  — vertical fader and horizontal slider.
//! - [`xy`]     — XY pad over two params.
//! - [`envelope`] — ADSR editor with draggable handles.
//! - [`shaper`]   — waveshaper transfer curve: hard/soft clip, cubic,
//!   wavefold and bitcrush, on linear axes. The one curve where folding
//!   back is a feature rather than a bug.
//! - [`spectrum`] — log-frequency spectrum grid display.
//! - [`dynamics`] — the transfer curve a compressor, limiter, gate or
//!   expander applies, plus the gain-reduction meter that goes beside it.
//!   One widget: a limiter is a compressor with a high ratio, and a gate
//!   is an expander with one.
//! - [`filter`]   — filter response curve: cascaded Butterworth sections
//!   at 6-48 dB/octave, drawn as the DIGITAL response, with a nonlinear
//!   drive model that squashes resonance and floors the stopband.
//! - [`readout`]  — typed value text: static and drag-editable.
//! - [`field`]    — the value you can drag OR type into, with a
//!   unit-aware parser: "250ms", "1.5k", "-6 dB".
//! - [`switch`]   — the segmented DISCRETE control: filter modes, wave
//!   shapes, sync divisions. Every other widget here is continuous.
//! - [`meter`]    — dB-scaled peak meter: instant attack, slow release,
//!   peak hold, latching clip light.
//! - [`synth`]    — the sine synth device card, the first real device.
//! - [`reverb`]   — the reverb device card, the first effect.

pub mod adjust;
pub mod bezier;
pub mod card;
pub mod design;
pub mod dynamics;
pub mod envelope;
pub mod fader;
pub mod field;
pub mod filter;
pub mod knob;
pub mod meter;
pub mod metrics;
pub mod param;
pub mod readout;
pub mod reverb;
pub mod shaper;
pub mod spectrum;
pub mod switch;
pub mod synth;
pub mod xy;

pub use bezier::{Cubic, Pt};
pub use card::{Well, Wells, card, empty_card, sections, sub_wells, tabbed_card, wells};
pub use envelope::Adsr;
pub use metrics::Footprint;
pub use param::{Mapping, Param, Unit};
pub use reverb::{ReverbUi, reverb_card, reverb_edits, reverb_norm, reverb_value};
pub use synth::{
    ParamEdit, SineSynthUi, sine_synth_card, sine_synth_edits, sine_synth_norm, sine_synth_value,
};
