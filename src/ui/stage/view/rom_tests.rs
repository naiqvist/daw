//! ROM's tall panel under a pointer: a row is a target, a heading is
//! not, and a tool button is the key it stands for.
use super::deck::{ListLayout, ListLine, list_lines};
use super::*;
use crate::pages::PageKey;
use crate::params::rom as p;
use crate::ui::device::probe;
use crate::ui::stage::StageIntent;

const FIELD: fn() -> egui::Rect =
    || egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(1120.0, 680.0));

fn rom() -> Stage {
    let mut stage = Stage::new();
    let _ = stage
        .song_mut()
        .add_device(0, crate::devices::DeviceKind::Rom)
        .unwrap();
    let _ = stage.apply(StageIntent::Page(PageKey::Src));
    stage
}

fn pcm(stage: &Stage, param: u32) -> f32 {
    stage.song().tracks[0]
        .machine
        .as_ref()
        .map_or(0.0, |machine| machine.value(param))
}

/// The drawn line of one row, from the same geometry the paint uses.
fn line_of(stage: &Stage, row: usize) -> egui::Rect {
    let list = stage.deck_hero_list().expect("the bank");
    let layout = ListLayout::of(stage.deck_wave_rect(FIELD()));
    let lines = list_lines(&list.rows);
    let first = layout.window(&lines, list.selected);
    let slot = lines
        .iter()
        .skip(first)
        .position(|line| *line == ListLine::Row(row))
        .expect("the row is on screen");
    layout.line_rect(slot)
}

#[test]
fn every_pcm_row_owns_its_click_and_undoes_once() {
    for row in [1usize, 3, 4] {
        let mut stage = rom();
        let before = stage.song().clone();
        let at = line_of(&stage, row).center();
        let ctx = egui::Context::default();
        probe::run(&ctx, FIELD(), &probe::click_path(at), |ui| {
            stage.interact_deck_hero(ui, FIELD())
        });
        assert_eq!(pcm(&stage, p::PCM1), row as f32, "row {row} did not answer");
        assert_eq!(pcm(&stage, p::PCM2), 0.0, "the click reached osc 2");
        let _ = stage.apply(StageIntent::Undo);
        assert_eq!(
            stage.song(),
            &before,
            "one undo must restore the whole pick"
        );
    }
}

/// A heading is a label, not a target: clicking one changes nothing.
#[test]
fn a_category_heading_is_not_a_target() {
    let mut stage = rom();
    let before = stage.song().clone();
    let list = stage.deck_hero_list().expect("the bank");
    let layout = ListLayout::of(stage.deck_wave_rect(FIELD()));
    let lines = list_lines(&list.rows);
    let first = layout.window(&lines, list.selected);
    let slot = lines
        .iter()
        .skip(first)
        .position(|line| matches!(line, ListLine::Head(_)))
        .expect("the bank has categories");
    let ctx = egui::Context::default();
    probe::run(
        &ctx,
        FIELD(),
        &probe::click_path(layout.line_rect(slot).center()),
        |ui| stage.interact_deck_hero(ui, FIELD()),
    );
    assert_eq!(stage.song(), &before);
}

/// The tool row is clickable, and each cell is the key it prints.
#[test]
fn a_tool_button_does_what_its_key_does() {
    let mut stage = rom();
    let tools = stage.deck_hero_tools().to_vec();
    let at = tools
        .iter()
        .position(|tool| tool.word == "CAT >")
        .expect("ROM offers the category tools");
    let layout = ListLayout::of(stage.deck_wave_rect(FIELD()));
    let width = layout.tools.width() / tools.len() as f32;
    let cell = egui::Rect::from_min_size(
        layout.tools.min + egui::vec2(at as f32 * width, 0.0),
        egui::vec2(width, layout.tools.height()),
    );
    let ctx = egui::Context::default();
    probe::run(&ctx, FIELD(), &probe::click_path(cell.center()), |ui| {
        stage.interact_deck_hero(ui, FIELD())
    });
    // Where CAT > lands is the bank's business: the first row of the
    // group after the one the cell is on.
    let multis = crate::audio::rom::bank::MULTIS;
    let group = multis.first().map_or("", |multi| multi.category);
    let wanted = multis
        .iter()
        .position(|multi| multi.category != group)
        .unwrap_or(0) as f32;
    assert_eq!(pcm(&stage, p::PCM1), wanted, "the tool button did not fire");
}

/// The panel's two halves never overlap, at any width the window takes.
#[test]
fn the_list_and_the_picture_keep_off_each_other() {
    for width in [420.0f32, 720.0, 1120.0, 1920.0] {
        let field = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(width, 700.0));
        let plot = egui::Rect::from_min_size(field.min + egui::vec2(8.0, 8.0), field.size() * 0.5);
        let layout = ListLayout::of(plot);
        assert!(
            layout.list.right() < layout.map.left(),
            "at {width} the list and the picture collide"
        );
        assert!(layout.list.bottom() <= layout.tools.top());
        assert!(layout.map.bottom() <= layout.tools.top());
        assert!(layout.visible() >= 1);
    }
}
