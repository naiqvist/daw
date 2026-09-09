//! Versioned composition, entirely in the control/green zone.
mod model;
pub use model::*;
pub mod generate;
pub mod motif;
pub mod rhythm;
pub mod voicing;
pub use generate::render;
pub mod alternatives;
pub mod bridge;
pub mod counterpoint;
pub mod form;
pub mod output;

#[cfg(test)]
mod acceptance;
pub mod capture;
mod ornament;
mod validation;
