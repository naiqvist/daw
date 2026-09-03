//! CUT: not yet drawn.

use super::*;

pub(super) fn draw(face: &Face<'_>) {
    face.painter.text(
        face.glass.center(),
        egui::Align2::CENTER_CENTER,
        face.kind().blurb(),
        face.font(),
        face.edge(),
    );
}
