//! The redesign's standing design principles, as tests.
//!
//! The frame's beauty is a set of promises — quiet pixels, hierarchy by
//! value, a closed sign system — and promises drift unless something
//! fails when they are broken. Panel-local invariants live in their own
//! modules; the asserts here cut across the whole layer.

use super::{OUTLINE, SURFACE_FRAME, SURFACE_SEQUENCE, SURFACE_UTILITY};
use eframe::egui;

/// A color spends no hue: its channels are equal, so it can only speak
/// through value.
fn neutral(color: egui::Color32) -> bool {
    color.r() == color.g() && color.g() == color.b()
}

/// The shared surface vocabulary is grayscale, and its tonal ladder is
/// ordered: the editor recedes, the frame holds, utility steps forward,
/// and the outline is the loudest thing the system can say.
#[test]
fn hierarchy_comes_from_value_alone() {
    for color in [OUTLINE, SURFACE_FRAME, SURFACE_SEQUENCE, SURFACE_UTILITY] {
        assert!(neutral(color), "a shared surface color carries hue");
    }
    assert!(SURFACE_SEQUENCE.r() < SURFACE_FRAME.r());
    assert!(SURFACE_FRAME.r() < SURFACE_UTILITY.r());
    assert!(SURFACE_UTILITY.r() < OUTLINE.r());
    assert_eq!(
        OUTLINE,
        egui::Color32::WHITE,
        "the top of the ladder is pure white — nothing may outshine focus"
    );
}

/// Quiet pixels, enforced at the source: no file in the redesign layer may
/// construct a color that can carry hue. Value, weight and geometry are
/// the whole visual vocabulary; the day this test needs an exception is
/// the day one signal earns color deliberately — grant it here, by name.
#[test]
fn the_layer_is_monochrome_by_construction() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/redesign");
    let banned = [
        "from_rgb",
        "from_rgba",
        "from_hex",
        "Color32::RED",
        "Color32::GREEN",
        "Color32::BLUE",
        "Color32::YELLOW",
        "Color32::GOLD",
        "Color32::ORANGE",
        "Color32::BROWN",
        "Color32::KHAKI",
        "Color32::LIGHT_",
        "Color32::DARK_",
    ];
    let mut stack = vec![root];
    let mut seen = 0;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("redesign source dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            // The ban list itself is the one place the banned names appear.
            if path.file_name().is_some_and(|f| f == "principles.rs") {
                continue;
            }
            seen += 1;
            let source = std::fs::read_to_string(&path).expect("redesign source file");
            for pattern in banned {
                assert!(
                    !source.contains(pattern),
                    "{} reaches for hue via {pattern:?}",
                    path.display()
                );
            }
        }
    }
    assert!(seen > 10, "the scan missed the redesign sources");
}
