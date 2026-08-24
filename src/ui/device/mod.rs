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
//!   UI lives in, and `sections`, which divides a card body into cols×rows
//!   sub-panels with visible boundaries; a chain of cards is a device rack.
//! - [`bezier`] — pure curve math (no egui): eval, y-at-x, polylines,
//!   bendable segments. Shared by the envelope editor and any future
//!   curve display.
//! - [`knob`]   — param-aware rotary, unipolar and bipolar.
//! - [`fader`]  — vertical fader and horizontal slider.
//! - [`xy`]     — XY pad over two params.
//! - [`envelope`] — ADSR editor with draggable handles.
//! - [`spectrum`] — log-frequency spectrum grid display.
//! - [`readout`]  — typed value text: static and drag-editable.
//! - [`synth`]    — the sine synth device card, the first real device.
//! - [`reverb`]   — the reverb device card, the first effect.

pub mod adjust;
pub mod bezier;
pub mod card;
pub mod design;
pub mod envelope;
pub mod fader;
pub mod knob;
pub mod param;
pub mod readout;
pub mod reverb;
pub mod spectrum;
pub mod synth;
pub mod xy;

pub use bezier::{Cubic, Pt};
pub use card::{card, empty_card, sections, tabbed_card};
pub use envelope::Adsr;
pub use param::{Mapping, Param, Unit};
pub use reverb::{ReverbUi, reverb_card, reverb_edits};
pub use synth::{ParamEdit, SineSynthUi, sine_synth_card, sine_synth_edits};
