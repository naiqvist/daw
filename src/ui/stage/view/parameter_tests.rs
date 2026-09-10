//! Real palette input belongs with the view, not the toolkit-free Stage core.
use super::super::Stage;
use crate::devices::DeviceKind;
fn stage() -> Stage {
    let mut stage = Stage::new();
    stage.song.add_device(0, DeviceKind::Table).unwrap();
    stage.settle();
    stage
}
fn value(stage: &Stage, param: u32) -> f32 {
    stage.song.tracks[0].machine.as_ref().unwrap().value(param)
}
#[test]
fn consecutive_commands_through_real_palette_frames() {
    let mut s = stage();
    s.set_palette_open(false);
    let ctx = egui::Context::default();
    crate::install_stage_fonts(&ctx);
    let chord = |key, modifiers| {
        [true, false]
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers,
            })
            .to_vec()
    };
    for command in [
        "param machine.attack = 1.6s; room.mix = 35%",
        "param machine.release = 2.5s",
        "param machine.release /= 2",
    ] {
        for events in [
            chord(egui::Key::P, egui::Modifiers::CTRL | egui::Modifiers::SHIFT),
            vec![egui::Event::Text(command.into())],
            chord(egui::Key::Enter, egui::Modifiers::NONE),
        ] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| s.show(ui),
            );
            output.textures_delta.clear();
        }
        assert!(!s.palette.is_open());
        assert!(
            s.notice
                .as_deref()
                .unwrap_or_default()
                .contains("committed"),
            "{:?}",
            s.notice
        );
    }
    assert_eq!(value(&s, crate::params::table::ATTACK), 1600.0);
    assert_eq!(value(&s, crate::params::table::RELEASE), 1250.0);
}

#[test]
fn equals_on_a_knob_accepts_only_the_value_and_escape_is_nonmutating() {
    let mut s = stage();
    s.set_palette_open(false);
    s.deck.open = true;
    s.deck.lit = Some(crate::pages::PageKey::Amp);
    s.deck.slot = 0;
    let ctx = egui::Context::default();
    crate::install_stage_fonts(&ctx);
    let mut frame = |s: &mut Stage, events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ui| s.show(ui),
        );
        output.textures_delta.clear();
    };
    let key = |key| {
        [true, false]
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .to_vec()
    };
    for text in ["180ms", "1.6s"] {
        frame(&mut s, key(egui::Key::Equals));
        assert!(s.palette.is_open());
        frame(&mut s, vec![egui::Event::Text(text.into())]);
        frame(&mut s, key(egui::Key::Enter));
        assert!(!s.palette.is_open());
        assert!(
            s.notice
                .as_deref()
                .unwrap_or_default()
                .contains("committed"),
            "{:?}",
            s.notice
        );
    }
    assert_eq!(value(&s, crate::params::table::ATTACK), 1600.0);
    let before = s.song.clone();
    frame(&mut s, key(egui::Key::Equals));
    frame(&mut s, vec![egui::Event::Text("3s".into())]);
    frame(&mut s, key(egui::Key::Escape));
    assert_eq!(s.song, before);
}
