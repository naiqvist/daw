//! VCA's face: a governor's linkage crossed with a screw vice.
//!
//! The SSL bus compressor is a feedback loop, and a feedback loop is a
//! governor: it listens to what it has already done. So the card is one
//! machine that regulates itself. The key climbs two columns on the
//! left — what the sidechain hears before its high-pass and after it,
//! which is exactly what the gain computer was fed — and the threshold
//! is a line scribed across both. What crosses that line closes the
//! vice in the middle, and the vice's jaws stand exactly as far apart
//! as the gain the loop is actually holding.
//!
//! The three stepped switches are drawn as what they are: rings of
//! detents with a pointer, so how many positions there are is as
//! readable as which one is chosen.

use super::*;
use crate::console::SectionParams;
use crate::params::console::vca as p;
use crate::ui::nav_cursor;

/// The key columns' scale: silence at the foot, full scale at the head.
const KEY_FLOOR_DB: f32 = -48.0;
/// How far the vice can close, in dB of gain reduction.
const VICE_FULL_DB: f32 = 20.0;

struct Lay {
    /// The two key columns and the line scribed across them.
    key: egui::Rect,
    threshold: egui::Rect,
    sc_hp: egui::Rect,
    /// The vice, and the lever whose angle is the ratio.
    vice: egui::Rect,
    ratio: egui::Rect,
    /// The two stepped switches, the jack, and the blend.
    attack: egui::Rect,
    release: egui::Rect,
    makeup: egui::Rect,
    mix: egui::Rect,
}

impl Layout for Lay {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        vec![
            (p::THRESHOLD, self.threshold),
            (p::RATIO, self.ratio),
            (p::ATTACK, self.attack),
            (p::RELEASE, self.release),
            (p::MAKEUP, self.makeup),
            (p::SC_HP, self.sc_hp),
            (p::MIX, self.mix),
        ]
    }
}

fn lay(glass: egui::Rect, params: &SectionParams) -> Lay {
    let inner = glass.shrink2(egui::vec2(5.0, 4.0));
    let (w, h) = (inner.width(), inner.height());
    let foot_h = 34.0;
    let body = egui::Rect::from_min_max(
        inner.min,
        egui::pos2(inner.right(), inner.bottom() - foot_h - 4.0),
    );
    let key = egui::Rect::from_min_max(
        body.min,
        egui::pos2(body.left() + 0.30 * w, body.bottom() - 12.0),
    );
    let threshold_y = tool::level_y(key, params.value(p::THRESHOLD), KEY_FLOOR_DB);
    Lay {
        key,
        threshold: egui::Rect::from_min_max(
            egui::pos2(key.left() - 3.0, threshold_y - 5.0),
            egui::pos2(key.right() + 3.0, threshold_y + 5.0),
        ),
        sc_hp: egui::Rect::from_min_max(
            egui::pos2(key.left(), key.bottom() + 1.0),
            egui::pos2(key.right(), body.bottom()),
        ),
        vice: egui::Rect::from_min_max(
            egui::pos2(key.right() + 10.0, body.top() + 0.10 * h),
            egui::pos2(body.right() - 6.0, body.top() + 0.10 * h + 0.42 * h),
        ),
        ratio: egui::Rect::from_min_max(
            egui::pos2(key.right() + 10.0, body.top() + 0.54 * h),
            egui::pos2(key.right() + 52.0, body.bottom()),
        ),
        attack: egui::Rect::from_min_max(
            egui::pos2(key.right() + 58.0, body.top() + 0.54 * h),
            egui::pos2(key.right() + 100.0, body.bottom()),
        ),
        release: egui::Rect::from_min_max(
            egui::pos2(key.right() + 106.0, body.top() + 0.54 * h),
            egui::pos2((key.right() + 148.0).min(body.right()), body.bottom()),
        ),
        makeup: egui::Rect::from_min_max(
            egui::pos2(inner.left(), inner.bottom() - foot_h),
            egui::pos2(inner.left() + 0.44 * w, inner.bottom()),
        ),
        mix: egui::Rect::from_min_max(
            egui::pos2(inner.left() + 0.50 * w, inner.bottom() - foot_h),
            inner.max,
        ),
    }
}

pub(super) fn draw(face: &Face<'_>) {
    let lay = lay(face.glass, &face.params);
    let (ink, edge) = (face.ink(), face.edge());
    let said = face.said;
    let mut shapes = Vec::new();

    // THE KEY: two columns, the sidechain before its high-pass and
    // after it. The gap between them is what the filter held back —
    // the only honest picture of a sidechain filter there is.
    tool::well(&mut shapes, lay.key, face.ground(), edge);
    let inside = lay.key.shrink(3.0);
    let column_w = inside.width() * 0.38;
    for (index, (level, hue)) in [
        (said.bands[1], tool::fade(edge, 1.3)),
        (said.bands[2], face.live()),
    ]
    .into_iter()
    .enumerate()
    {
        let top = tool::level_y(
            inside,
            face.anim(
                if index == 0 { "key-pre" } else { "key-post" },
                level.clamp(KEY_FLOOR_DB, 6.0),
                0.07,
            ),
            KEY_FLOOR_DB,
        );
        let x = inside.left() + inside.width() * (0.10 + 0.46 * index as f32);
        let bar = egui::Rect::from_min_max(
            egui::pos2(x, top),
            egui::pos2(x + column_w, inside.bottom()),
        );
        if bar.is_positive() {
            shapes.push(egui::Shape::rect_filled(bar, 0.0, tool::fade(hue, 0.7)));
        }
    }
    // THE THRESHOLD: scribed across both columns, with a stub through
    // each wall so it stays readable under the key.
    let scribe = lay.threshold.center().y;
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(lay.key.left() - 4.0, scribe),
            egui::pos2(lay.key.right() + 4.0, scribe),
        ],
        Weight::Heavy,
        tool::mix_ink(ink, face.focus(), face.lit(p::THRESHOLD)),
    );
    tool::halo(
        &mut shapes,
        lay.threshold,
        face.lit(p::THRESHOLD),
        face.focus(),
    );

    // SC HP: a wedge under the columns that closes as the filter climbs,
    // drawn where the signal it removes would have been.
    let cut = tool::norm(face.value(p::SC_HP), 20.0, 500.0);
    let wedge = lay.sc_hp;
    circuit::trace(
        &mut shapes,
        &[
            wedge.left_bottom(),
            egui::pos2(wedge.left() + wedge.width() * cut.max(0.03), wedge.top()),
            egui::pos2(wedge.left() + wedge.width() * cut.max(0.03), wedge.bottom()),
        ],
        Weight::Hair,
        tool::mix_ink(tool::fade(edge, 0.8), ink, cut),
    );
    tool::halo(&mut shapes, wedge, face.lit(p::SC_HP), face.focus());

    // THE VICE: two jaws standing exactly as far apart as the gain the
    // loop is holding. Closed is compressed.
    let gain = face.anim(
        "gain",
        (said.bands[0].abs() / VICE_FULL_DB).clamp(0.0, 1.0),
        0.05,
    );
    let held = face.anim(
        "held",
        (said.reduction_db.abs() / VICE_FULL_DB).clamp(0.0, 1.0),
        0.5,
    );
    tool::iris(
        &mut shapes,
        lay.vice,
        1.0 - gain,
        tool::fade(ink, 0.75),
        edge,
    );
    // The peak hold: a hairline where the jaws got to at their closest.
    let mark = egui::lerp(
        lay.vice.center().y..=lay.vice.top() + 1.0,
        1.0 - held.clamp(0.0, 1.0),
    );
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(lay.vice.left(), mark),
            egui::pos2(lay.vice.right(), mark),
        ],
        Weight::Hair,
        face.hot(),
    );

    // THE THREE SWITCHES: rings of detents with a pointer, so how many
    // positions exist is as readable as which one is chosen.
    for (rect, param, steps, tag) in [
        (lay.ratio, p::RATIO, 3usize, "ratio"),
        (lay.attack, p::ATTACK, 6, "attack"),
        (lay.release, p::RELEASE, 5, "release"),
    ] {
        let at = face.anim(tag, face.value(param), 0.10);
        tool::rota(
            &mut shapes,
            egui::pos2(rect.center().x, rect.center().y + 3.0),
            (rect.width().min(rect.height()) * 0.42).max(7.0),
            steps,
            at,
            tool::mix_ink(ink, face.focus(), face.lit(param)),
            tool::fade(edge, 0.7),
        );
        tool::halo(&mut shapes, rect, face.lit(param), face.focus());
    }

    // MAKEUP: shims under the machine, one per two dB handed back.
    let shims = (face.value(p::MAKEUP) / 2.0).round().clamp(0.0, 12.0) as usize;
    tool::cells(
        &mut shapes,
        egui::pos2(lay.makeup.left() + 5.0, lay.makeup.center().y),
        egui::vec2(7.0, 0.0),
        12,
        shims,
        5.0,
        ink,
        tool::fade(edge, 0.5),
    );
    tool::halo(&mut shapes, lay.makeup, face.lit(p::MAKEUP), face.focus());

    // MIX: the two paths, dry and compressed, blended on one rail.
    tool::swing_rail(
        &mut shapes,
        lay.mix.shrink2(egui::vec2(6.0, 10.0)),
        face.place(p::MIX) * 2.0 - 1.0,
        13,
        tool::mix_ink(ink, face.focus(), face.lit(p::MIX)),
        tool::fade(edge, 0.6),
    );
    tool::halo(&mut shapes, lay.mix, face.lit(p::MIX), face.focus());

    face.painter.extend(shapes);

    // The mark is squeezed by the gain the loop is actually holding —
    // wherever on the card the keyboard is standing, because that is
    // the one thing the whole section is doing.
    face.mark_signed(&lay, nav_cursor::Signature::Squeeze(gain));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    fn laid(params: &SectionParams) -> (egui::Rect, Lay) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(strip::width_of(SectionKind::Vca), 250.0),
        );
        let glass = strip::recess_of(piece, SectionKind::Vca).shrink(3.0);
        (piece, lay(glass, params))
    }

    #[test]
    fn every_vca_parameter_has_one_instrument() {
        let params = SectionParams::of(SectionKind::Vca);
        let (piece, lay) = laid(&params);
        for def in SectionKind::Vca.table() {
            let rect = lay
                .control(def.id as usize)
                .unwrap_or_else(|| panic!("{} has no instrument", def.name));
            assert!(rect.is_positive(), "{} has no room", def.name);
            assert!(piece.contains_rect(rect), "{} left the piece", def.name);
        }
        assert_eq!(lay.controls().len(), 7);
        // No two instruments overlap.
        let all = lay.controls();
        for (i, (a, ra)) in all.iter().enumerate() {
            for (b, rb) in &all[i + 1..] {
                assert!(!ra.intersects(*rb), "{a} and {b} overlap");
            }
        }
    }

    /// The threshold is a line on the key's own scale, so lowering it
    /// walks it down into the columns.
    #[test]
    fn the_threshold_is_scribed_on_the_key_and_walks_with_it() {
        let mut high = SectionParams::of(SectionKind::Vca);
        high.set(p::THRESHOLD, 0.0);
        let mut low = SectionParams::of(SectionKind::Vca);
        low.set(p::THRESHOLD, -40.0);
        let (_, top) = laid(&high);
        let (_, bottom) = laid(&low);
        assert!(
            bottom.threshold.center().y > top.threshold.center().y + 20.0,
            "the threshold did not walk down the key"
        );
        // And it stays across the columns rather than beside them.
        assert!(top.threshold.left() <= top.key.left());
        assert!(top.threshold.right() >= top.key.right());
        // The sidechain filter's wedge sits under the columns, not over.
        assert!(top.sc_hp.top() >= top.key.bottom() - 0.01);
    }
}
