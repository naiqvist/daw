use super::deck::WaveLayout;
use super::*;
use crate::pages::PageKey;
use crate::params::sampler as p;
use crate::ui::device::probe;
use crate::ui::stage::{SampleData, StageIntent};

fn sampler(loop_page: bool) -> Stage {
    let mut stage = Stage::new();
    let id = stage
        .song_mut()
        .add_device(0, crate::devices::DeviceKind::Sampler)
        .unwrap();
    let path = std::path::PathBuf::from("pointer.wav");
    let d = stage.song_mut().device_mut(id).unwrap();
    d.sample = Some(path.clone());
    d.set(p::START, 0.1);
    d.set(p::END, 0.9);
    d.set(p::LOOP_MODE, 1.0);
    d.set(p::LOOP_START, 0.2);
    d.set(p::LOOP_SIZE, 0.25);
    stage.set_sample(SampleData::from_planar(
        path,
        std::sync::Arc::new(vec![0.25; 48000]),
        1,
        48000,
        48000,
    ));
    stage.apply(StageIntent::Page(PageKey::Src));
    if loop_page {
        stage.apply(StageIntent::Page(PageKey::Src));
    }
    stage
}

#[test]
fn every_waveform_bracket_owns_its_drag_and_undoes_once() {
    for (loop_page, param, target) in [
        (false, p::START, 0.25),
        (false, p::END, 0.75),
        (true, p::LOOP_START, 0.40),
        (true, p::LOOP_SIZE, 0.65),
    ] {
        let mut stage = sampler(loop_page);
        let before = stage.song().clone();
        let field = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(1120.0, 680.0));
        let wave = stage.deck_hero().unwrap().waveform.unwrap();
        let handle = wave.handles.iter().find(|h| h.param == param).unwrap();
        let rect = stage.deck_wave_rect(field);
        let layout = WaveLayout::of(rect);
        let from = egui::pos2(
            layout.wave.left() + handle.at * layout.wave.width(),
            layout.wave.center().y,
        );
        let to = egui::pos2(
            layout.wave.left() + target * layout.wave.width(),
            layout.wave.center().y,
        );
        let ctx = egui::Context::default();
        probe::run(&ctx, field, &probe::drag_path(from, to, 8), |ui| {
            stage.interact_deck_hero(ui, field)
        });
        let d = stage.song().tracks[0].machine.as_ref().unwrap();
        let old = before.tracks[0].machine.as_ref().unwrap();
        assert!(
            (d.value(param) - old.value(param)).abs() > 0.02,
            "handle {param} did not move"
        );
        for h in &wave.handles {
            if h.param != param {
                assert_eq!(
                    d.value(h.param),
                    old.value(h.param),
                    "drag stole neighbouring handle"
                );
            }
        }
        stage.apply(StageIntent::Undo);
        assert_eq!(
            stage.song(),
            &before,
            "one undo must restore the whole gesture"
        );
    }
}

#[test]
fn profile_is_clickable_and_preserves_the_loaded_source() {
    let mut stage = sampler(false);
    let field = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(1120.0, 680.0));
    let layout = WaveLayout::of(stage.deck_wave_rect(field));
    let at = layout.tools.min + egui::vec2(layout.tools.width() / 6.0 * 2.5, 30.0);
    let ctx = egui::Context::default();
    probe::run(&ctx, field, &probe::click_path(at), |ui| {
        stage.interact_deck_hero(ui, field)
    });
    let d = stage.song().tracks[0].machine.as_ref().unwrap();
    assert_eq!(d.value(p::SPEED), 0.0);
    assert_eq!(d.value(p::PLAYBACK), 2.0);
    assert_eq!(d.sample.as_ref().unwrap().to_string_lossy(), "pointer.wav");
}
