//! Where the console's own files live: `~/Corpus`, the one folder the
//! theme, the tune overrides and the sound library share. Spelled here
//! once so a second console on the same machine finds the same shelf.

use std::path::PathBuf;

/// `$HOME/Corpus`. An unset `HOME` yields a relative `Corpus`, which is
/// wrong but not a panic; every reader treats a missing folder as empty.
pub fn dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    home.unwrap_or_default().join("Corpus")
}
