//! Paper grain: the one texture on the surface.
//!
//! A faint field of speckle over the whole ground, so black reads as a
//! material rather than as the absence of a picture. It says nothing —
//! which is exactly why it may cover everything — and it never moves: the
//! field is built once from a fixed seed, uploaded once, and tiled.
//!
//! It follows the ground. On black the grain is a scatter of pale
//! points; on paper it is a scatter of dark ones, the same field turned
//! over, so the two projections stay one code.

use eframe::egui::{self, Color32, ColorImage, Rect, TextureHandle, TextureOptions, pos2};

use super::Polarity;
use super::kit::Rng;

/// The tile's side, in pixels.
const TILE: usize = 256;
/// The strongest a speck may be. Structure-rung quiet: the grain must sit
/// below the well, or the ground has grown a pattern.
const ALPHA_MAX: u8 = 34;
/// One speck in this many pixels.
const SPARSITY: f32 = 0.16;

fn build(polarity: Polarity) -> ColorImage {
    let mut rng = Rng::seeded(("grain", polarity == Polarity::Light));
    let mut pixels = vec![Color32::TRANSPARENT; TILE * TILE];
    for px in pixels.iter_mut() {
        if rng.chance(SPARSITY) {
            let a = rng.int(6, ALPHA_MAX as i32 + 1) as u8;
            *px = match polarity {
                Polarity::Dark => Color32::from_white_alpha(a),
                Polarity::Light => Color32::from_black_alpha(a),
            };
        }
    }
    ColorImage {
        size: [TILE, TILE],
        source_size: egui::vec2(TILE as f32, TILE as f32),
        pixels,
    }
}

fn texture(ctx: &egui::Context, polarity: Polarity) -> TextureHandle {
    let id = egui::Id::new(("design-grain", polarity == Polarity::Light));
    if let Some(handle) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return handle;
    }
    let handle = ctx.load_texture(
        if polarity == Polarity::Dark {
            "design-grain-dark"
        } else {
            "design-grain-light"
        },
        build(polarity),
        TextureOptions::NEAREST,
    );
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    handle
}

/// Lay the grain over `area`, tiled at one texel per pixel.
pub fn overlay(painter: &egui::Painter, area: Rect, polarity: Polarity) {
    if !area.is_positive() {
        return;
    }
    let ctx = painter.ctx();
    let handle = texture(ctx, polarity);
    let tile = TILE as f32 / ctx.pixels_per_point().max(0.5);
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    let painter = painter.with_clip_rect(area);
    let mut y = area.min.y;
    while y < area.max.y {
        let mut x = area.min.x;
        while x < area.max.x {
            painter.image(
                handle.id(),
                Rect::from_min_size(pos2(x, y), egui::vec2(tile, tile)),
                uv,
                Color32::WHITE,
            );
            x += tile;
        }
        y += tile;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grain_is_sparse_quiet_and_the_same_every_time() {
        let a = build(Polarity::Dark);
        let b = build(Polarity::Dark);
        assert_eq!(a.pixels, b.pixels, "the field changed between builds");
        let specks = a.pixels.iter().filter(|p| p.a() > 0).count();
        let share = specks as f32 / (TILE * TILE) as f32;
        assert!((0.10..0.22).contains(&share), "grain density {share}");
        assert!(a.pixels.iter().all(|p| p.a() <= ALPHA_MAX));
    }

    #[test]
    fn paper_grain_is_dark_and_black_grain_is_pale() {
        let dark = build(Polarity::Dark);
        let light = build(Polarity::Light);
        let pale = dark.pixels.iter().find(|p| p.a() > 0).expect("a speck");
        let ink = light.pixels.iter().find(|p| p.a() > 0).expect("a speck");
        assert!(pale.r() > 0, "a speck on black should be pale");
        assert_eq!(ink.r(), 0, "a speck on paper should be dark");
    }
}
