//! Application panels. RULES OF THIS DIRECTORY:
//!
//! - No numeric literals — speak `ui::tokens`, `ui::theme`, and `ui::kit`
//!   only. The escape hatch is a `// magic: <reason>` tag on the line, which
//!   is greppable forever.
//! - No `crate::audio`. A panel renders the `ViewState` it is handed and
//!   returns `UiAction`s; the app layer owns the Engine and translates.
//! - No `ui.painter()` and no `Color32::` — custom painting and raw color
//!   live in `kit` and `theme` respectively.
//! - One panel per file, implementing `ui::host::Panel`, and declared here.
//!
//! All four are enforced by tests in `ui::mod`, including the last one: a
//! `.rs` file in this directory that nobody declared is a test failure, not a
//! file that quietly never compiles.

pub mod arrangement;
pub mod status;
pub mod transport;
