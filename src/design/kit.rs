//! The drawing kit's ground rules: a seed, a cache, and three weights.
//!
//! Every figure the circuit kit draws is a pure function of what it
//! decorates. Two things make that cheap enough to draw everywhere:
//!
//! 1. **A seed, not a clock.** Anything that looks random — a barcode, a
//!    grain field — takes its randomness from a seed derived from the
//!    thing it belongs to, so it is the same on every frame and every
//!    machine. Ornament that changes between frames is a channel nobody
//!    budgeted for.
//! 2. **A mesh cache.** A figure is tessellated once per (place, size,
//!    key) into a mesh that later frames replay. The stage draws sixty
//!    frames a second; the ornament costs the same with as without.

use std::sync::Arc;

use eframe::egui::{self, Id, Pos2, Rect, Shape, epaint, pos2};

/// Line weights. Three, and no more: the alphabet carries hierarchy in
/// value, and weight only says what KIND of line this is.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Weight {
    /// Traces, cells, rules, ruling: the periphery's hairline.
    Hair,
    /// Primary frames and rails.
    Heavy,
    /// The cursor and alarm marks — the two things that must outweigh a
    /// frame.
    Bold,
}

impl Weight {
    pub const fn px(self) -> f32 {
        match self {
            Weight::Hair => 1.0,
            Weight::Heavy => 2.0,
            Weight::Bold => 3.0,
        }
    }
}

/// Pull a coordinate to the pixel grid so a hairline lands on one row of
/// pixels rather than smearing across two.
pub fn snap(v: f32) -> f32 {
    v.floor() + 0.5
}

pub fn snap_pos(p: Pos2) -> Pos2 {
    pos2(snap(p.x), snap(p.y))
}

/// SplitMix64. Small, fast, and — the property that matters here — the
/// same sequence for the same seed on every machine and every frame.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    /// A seed from anything hashable: a name, a track index, a rect.
    pub fn seeded(key: impl std::hash::Hash) -> Self {
        use std::hash::Hasher;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        Self::new(hasher.finish())
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in 0..1.
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        if b <= a { a } else { a + (b - a) * self.f32() }
    }

    pub fn int(&mut self, a: i32, b: i32) -> i32 {
        if b <= a {
            a
        } else {
            a + (self.next_u64() % (b - a) as u64) as i32
        }
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }
}

/// Paint a figure through the mesh cache.
///
/// `key` is what the figure depends on besides its rectangle — a colour,
/// a count, a name. The builder runs once per distinct (id, rect, key)
/// and its shapes are tessellated into one mesh that later frames
/// replay. Rects are keyed at half points, so a sub-pixel drift under a
/// resize does not rebuild the figure every frame. A key must never carry
/// a phase or a clock: what moves is drawn outside the cache.
pub fn cached(
    painter: &egui::Painter,
    id: Id,
    rect: Rect,
    key: impl std::hash::Hash + std::fmt::Debug,
    build: impl FnOnce(&mut Vec<Shape>),
) {
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let quant = |v: f32| (v * 2.0).round() as i32;
    let full = id.with((
        quant(rect.min.x),
        quant(rect.min.y),
        quant(rect.max.x),
        quant(rect.max.y),
        quant(ppp),
        key,
    ));
    let mesh: Arc<epaint::Mesh> = ctx
        .data_mut(|d| d.get_temp::<Arc<epaint::Mesh>>(full))
        .unwrap_or_else(|| {
            let mut shapes = Vec::with_capacity(256);
            build(&mut shapes);
            // No prepared discs: the kit draws its own dots as polygons,
            // and an empty list is what the tessellator's docs suggest.
            let tex = ctx.fonts(|f| f.font_image_size());
            let mut tess = epaint::Tessellator::new(
                ppp,
                epaint::TessellationOptions::default(),
                tex,
                Vec::new(),
            );
            let mut mesh = epaint::Mesh::default();
            for s in shapes {
                tess.tessellate_shape(s, &mut mesh);
            }
            let mesh = Arc::new(mesh);
            ctx.data_mut(|d| d.insert_temp(full, mesh.clone()));
            mesh
        });
    painter.add(Shape::Mesh(mesh));
}

/// Every point a shape touches. For the kit's tests: a figure is checked
/// by where it puts its points, not by a screenshot.
#[cfg(test)]
pub(crate) fn points_of(shape: &Shape) -> Vec<Pos2> {
    match shape {
        Shape::Path(p) => p.points.clone(),
        Shape::LineSegment { points, .. } => points.to_vec(),
        Shape::Circle(c) => vec![
            c.center - egui::vec2(c.radius, c.radius),
            c.center + egui::vec2(c.radius, c.radius),
        ],
        Shape::Rect(r) => vec![r.rect.min, r.rect.max],
        Shape::Vec(v) => v.iter().flat_map(points_of).collect(),
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_draws_the_same_numbers() {
        let mut a = Rng::seeded(("track", 3usize));
        let mut b = Rng::seeded(("track", 3usize));
        for _ in 0..64 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut c = Rng::seeded(("track", 4usize));
        assert_ne!(a.next_u64(), c.next_u64());
    }

    #[test]
    fn a_range_stays_inside_itself() {
        let mut rng = Rng::new(9);
        for _ in 0..1000 {
            let v = rng.range(2.0, 5.0);
            assert!((2.0..5.0).contains(&v));
            let i = rng.int(3, 7);
            assert!((3..7).contains(&i));
        }
        assert_eq!(rng.range(5.0, 2.0), 5.0, "an empty range returns its start");
        assert_eq!(rng.int(7, 3), 7);
    }

    #[test]
    fn weights_are_the_three_the_kit_promises() {
        assert_eq!(Weight::Hair.px(), 1.0);
        assert_eq!(Weight::Heavy.px(), 2.0);
        assert_eq!(Weight::Bold.px(), 3.0);
    }

    #[test]
    fn a_snapped_hairline_sits_on_a_pixel_centre() {
        assert_eq!(snap(10.0), 10.5);
        assert_eq!(snap(10.4), 10.5);
        assert_eq!(snap(10.9), 10.5);
    }
}
