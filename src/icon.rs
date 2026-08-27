//! Transport icon geometry, as pure functions.
//!
//! Transport icons are DRAWN, not typed.
//!
//! `Align2::CENTER_CENTER` centres a glyph's advance box, not its ink, and
//! the Nerd Font media glyphs carry asymmetric side bearings — so a
//! correctly centred cell still puts the triangle off centre. Shapes are
//! centred by construction, and this is where the UI is headed anyway.
//!
//! Every function here takes the button's rect and returns shapes centred
//! in it — no `Ui`, no theme, no state — so the whole icon set is checkable
//! by geometry alone. Lifted out of `main.rs` unchanged.

/// Side of the square the icon is inscribed in.
pub const ICON: f32 = 11.0;
/// Pause bar width, and the gap between the two bars.
pub const PAUSE_BAR: f32 = 3.0;
pub const PAUSE_GAP: f32 = 3.0;
/// The return icon's bar, and the gap between it and the triangle.
pub const RETURN_BAR: f32 = 2.5;
pub const RETURN_GAP: f32 = 1.5;
/// The stop square, deliberately smaller than `ICON`.
///
/// A square filling the same box as the play triangle carries roughly twice
/// the ink and reads as much heavier beside it. Shrinking it is an optical
/// correction, not a measurement — set it to `ICON` if you want them
/// geometrically equal instead.
pub const STOP_SIDE: f32 = 9.0;

/// What a transport button draws. Adding a control is a variant here plus a
/// slot on the bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Return,
    Play,
    Pause,
    Stop,
    Record,
    Loop,
    Metronome,
    Follow,
    Power,
}

/// The power symbol: an arc-broken circle with a bar through the gap —
/// drawn as a full circle stroke plus the bar, which reads the same at 11px.
pub fn power_icon(rect: egui::Rect) -> (egui::Pos2, f32, [egui::Pos2; 2]) {
    let c = rect.center();
    let r = ICON * 0.5 * 0.9;
    (
        c,
        r,
        [egui::pos2(c.x, c.y - ICON * 0.5), egui::pos2(c.x, c.y)],
    )
}

/// The return icon: a bar against the left edge with a left-pointing
/// triangle beside it, together spanning `ICON` and centred on `rect`.
pub fn return_icon(rect: egui::Rect) -> (egui::Rect, [egui::Pos2; 3]) {
    let c = rect.center();
    let h = ICON * 0.5;
    let bar = egui::Rect::from_min_max(
        egui::pos2(c.x - h, c.y - h),
        egui::pos2(c.x - h + RETURN_BAR, c.y + h),
    );
    let apex = bar.right() + RETURN_GAP;
    (
        bar,
        [
            egui::pos2(c.x + h, c.y - h),
            egui::pos2(c.x + h, c.y + h),
            egui::pos2(apex, c.y),
        ],
    )
}

/// The record dot, centred on `rect`.
pub fn record_icon(rect: egui::Rect) -> (egui::Pos2, f32) {
    (rect.center(), ICON * 0.5 * 0.92)
}

/// The loop icon: a rounded track with an arrowhead riding its top edge.
pub fn loop_icon(rect: egui::Rect) -> (egui::Rect, [egui::Pos2; 3]) {
    let c = rect.center();
    let (w, h) = (ICON * 0.5, ICON * 0.36);
    let track = egui::Rect::from_center_size(c, egui::vec2(w * 2.0, h * 2.0));
    let tip = egui::pos2(track.right(), track.top());
    (
        track,
        [
            egui::pos2(tip.x - ICON * 0.26, tip.y - ICON * 0.20),
            egui::pos2(tip.x - ICON * 0.26, tip.y + ICON * 0.20),
            egui::pos2(tip.x + ICON * 0.12, tip.y),
        ],
    )
}

/// The metronome: a tapered body with the pendulum swung right.
pub fn metronome_icon(rect: egui::Rect) -> ([egui::Pos2; 3], [egui::Pos2; 2]) {
    let c = rect.center();
    let h = ICON * 0.5;
    (
        [
            egui::pos2(c.x - h * 0.78, c.y + h),
            egui::pos2(c.x + h * 0.78, c.y + h),
            egui::pos2(c.x, c.y - h),
        ],
        [
            egui::pos2(c.x, c.y + h * 0.55),
            egui::pos2(c.x + h * 0.62, c.y - h * 0.45),
        ],
    )
}

/// Follow: a playhead with the view chasing it rightwards.
pub fn follow_icon(rect: egui::Rect) -> (egui::Rect, [[egui::Pos2; 2]; 2]) {
    let c = rect.center();
    let h = ICON * 0.5;
    let bar = egui::Rect::from_min_max(
        egui::pos2(c.x - h, c.y - h),
        egui::pos2(c.x - h + RETURN_BAR, c.y + h),
    );
    let tip = egui::pos2(c.x + h, c.y);
    (
        bar,
        [
            [egui::pos2(tip.x - h * 0.7, c.y - h * 0.7), tip],
            [egui::pos2(tip.x - h * 0.7, c.y + h * 0.7), tip],
        ],
    )
}

/// The stop square, centred on `rect`.
pub fn stop_icon(rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(STOP_SIDE))
}

/// The play triangle: right-pointing, inscribed in an `ICON`-square centred
/// on `rect`. Returned as points so the centring is checkable.
pub fn play_icon(rect: egui::Rect) -> [egui::Pos2; 3] {
    let c = rect.center();
    let h = ICON * 0.5;
    [
        egui::pos2(c.x - h, c.y - h),
        egui::pos2(c.x - h, c.y + h),
        egui::pos2(c.x + h, c.y),
    ]
}

/// The pause bars: two `PAUSE_BAR`-wide bars either side of `rect`'s centre.
pub fn pause_icon(rect: egui::Rect) -> [egui::Rect; 2] {
    let c = rect.center();
    let h = ICON * 0.5;
    let inner = PAUSE_GAP * 0.5;
    [
        egui::Rect::from_min_max(
            egui::pos2(c.x - inner - PAUSE_BAR, c.y - h),
            egui::pos2(c.x - inner, c.y + h),
        ),
        egui::Rect::from_min_max(
            egui::pos2(c.x + inner, c.y - h),
            egui::pos2(c.x + inner + PAUSE_BAR, c.y + h),
        ),
    ]
}
