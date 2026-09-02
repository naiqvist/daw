//! Musical grid scale and Ableton-compatible resolution commands.
//!
//! Time is represented on a 192-tick bar. That common clock expresses both
//! straight sixty-fourths and every supported triplet division exactly, so a
//! resolution change changes the view and cursor stride without rewriting
//! note positions.

use eframe::egui;

pub(crate) const TICKS_PER_BAR: usize = 192;
const MIN_DENOMINATOR: u8 = 4;
const MAX_DENOMINATOR: u8 = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GridResolution {
    denominator: u8,
    triplet: bool,
}

impl Default for GridResolution {
    fn default() -> Self {
        Self {
            denominator: 16,
            triplet: false,
        }
    }
}

impl GridResolution {
    /// A resolution by name, for callers that already know it (tests,
    /// and a frame that restores one). Clamped to the supported range.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn at(denominator: u8, triplet: bool) -> Self {
        Self {
            denominator: denominator.clamp(MIN_DENOMINATOR, MAX_DENOMINATOR),
            triplet,
        }
    }

    pub(crate) fn update(&mut self, ctx: &egui::Context) {
        let narrow =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Num1));
        let widen = !narrow
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Num2));
        let toggle_triplets =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Num3));
        if narrow {
            self.narrow();
        } else if widen {
            self.widen();
        }
        if toggle_triplets {
            self.triplet = !self.triplet;
        }
    }

    pub(crate) fn step_ticks(self) -> usize {
        let straight = TICKS_PER_BAR / usize::from(self.denominator);
        if self.triplet {
            straight * 2 / 3
        } else {
            straight
        }
    }

    pub(crate) fn steps_per_bar(self) -> usize {
        TICKS_PER_BAR / self.step_ticks()
    }

    pub(crate) fn label(self) -> String {
        format!(
            "1/{}{}",
            self.denominator,
            if self.triplet { "T" } else { "" }
        )
    }

    fn narrow(&mut self) {
        self.denominator = self.denominator.saturating_mul(2).min(MAX_DENOMINATOR);
    }
    fn widen(&mut self) {
        self.denominator = (self.denominator / 2).max(MIN_DENOMINATOR);
    }
}

/// A clip's length as bars: `02B` when whole, and `1B+8` — bars, then
/// the steps past the last whole bar — when it is not, so a clip cut
/// mid-bar says exactly where it ends rather than rounding up.
pub(crate) fn bars_label(ticks: usize) -> String {
    let bars = ticks / TICKS_PER_BAR;
    let rest = ticks % TICKS_PER_BAR;
    if rest == 0 {
        format!("{bars:02}B")
    } else {
        let steps = rest.div_ceil(crate::sequencing::PATTERN_STEP_TICKS);
        format!("{bars}B+{steps}")
    }
}

pub(crate) fn length_label(ticks: usize) -> String {
    for denominator in [4_u8, 8, 16, 32, 64] {
        let straight = TICKS_PER_BAR / usize::from(denominator);
        if ticks == straight {
            return format!("1/{denominator}");
        }
        if ticks == straight * 2 / 3 {
            return format!("1/{denominator}T");
        }
    }
    format!("{ticks}TCK")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_grid_doubles_and_halves_density() {
        let mut grid = GridResolution::default();
        assert_eq!(grid.label(), "1/16");
        assert_eq!(grid.step_ticks(), 12);
        grid.narrow();
        assert_eq!(grid.label(), "1/32");
        assert_eq!(grid.step_ticks(), 6);
        grid.widen();
        assert_eq!(grid.label(), "1/16");
    }

    #[test]
    fn triplets_are_exact_on_the_shared_clock() {
        let mut grid = GridResolution::default();
        grid.triplet = true;
        assert_eq!(grid.label(), "1/16T");
        assert_eq!(grid.step_ticks(), 8);
        assert_eq!(grid.steps_per_bar(), 24);
    }

    #[test]
    fn resolution_is_bounded_to_supported_values() {
        let mut grid = GridResolution::default();
        for _ in 0..8 {
            grid.narrow();
        }
        assert_eq!(grid.label(), "1/64");
        for _ in 0..8 {
            grid.widen();
        }
        assert_eq!(grid.label(), "1/4");
    }

    #[test]
    fn a_clips_length_says_bars_and_the_steps_past_them() {
        use crate::sequencing::PATTERN_STEP_TICKS;
        assert_eq!(bars_label(TICKS_PER_BAR * 2), "02B");
        assert_eq!(bars_label(TICKS_PER_BAR + 8 * PATTERN_STEP_TICKS), "1B+8");
        assert_eq!(bars_label(4 * PATTERN_STEP_TICKS), "0B+4");
    }
}
