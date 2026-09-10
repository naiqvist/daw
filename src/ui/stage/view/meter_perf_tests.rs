//! Reproducible tracker CPU workload, including text layout and tessellation.
use super::super::{Stage, StageIntent, meter::take_derivation_counts};
use crate::devices::DeviceKind;
use crate::sequencing::{Note, PATTERN_STEP_TICKS, ParamLock};

fn dense_stage() -> Stage {
    let mut stage = Stage::new();
    stage.set_palette_open(false);
    for track in 0..12 {
        if track > 0 {
            let _ = stage.apply(StageIntent::NewInstrumentTrack);
        }
        stage.song.add_device(track, DeviceKind::Poly).unwrap();
        let id = stage.song.fill_slot(track, 0).unwrap();
        let pattern = stage.song.pattern_mut(id).unwrap();
        pattern.extend_timeline(128 * PATTERN_STEP_TICKS).unwrap();
        for step in (0..128).step_by(2) {
            let trig = pattern.trig_mut(step);
            trig.enabled = true;
            trig.notes.push(Note::new(36 + (step % 48) as u8, 12, 100));
            for def in DeviceKind::Poly.spec().params.iter().take(16) {
                trig.locks.push(ParamLock {
                    device: None,
                    param: def.id,
                    value: 0.5,
                    slide: step % 4 == 0,
                });
            }
        }
    }
    stage.playing = vec![Some(0); 12];
    stage.meter.open = true;
    stage
}

fn frame(stage: &Stage, ctx: &egui::Context, number: usize) -> [usize; 3] {
    take_derivation_counts();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(number as f64 / 60.0),
            ..Default::default()
        },
        |ui| stage.draw_meter(ui.painter(), screen),
    );
    let mesh = ctx.tessellate(output.shapes, output.pixels_per_point);
    output.textures_delta.clear();
    std::hint::black_box(mesh);
    take_derivation_counts()
}

#[test]
fn meter_draw_derives_once_and_only_for_visible_tracks() {
    let mut stage = dense_stage();
    let ctx = egui::Context::default();
    crate::install_stage_fonts(&ctx);
    stage.transport.seek(32 * PATTERN_STEP_TICKS);
    let before = stage.song.clone();
    for track in [0, 6, 11] {
        stage.meter.at = stage.meter_plan().start_of(track);
        let [plans, rows, cells] = frame(&stage, &ctx, track);
        assert_eq!(plans, 1, "drawing rebuilt the plan per cell or heading");
        assert_eq!(rows, 1, "the readout derived a second set of rows");
        assert!(
            cells > 0 && cells <= 3 * 72,
            "offscreen cells were derived: {cells}"
        );
    }
    assert_eq!(stage.song, before, "drawing changed musical state");
}

#[test]
fn meter_culling_preserves_visible_cells_and_fresh_edits() {
    let mut stage = dense_stage();
    let all = stage.meter_rows(-4, 24);
    let mut visible = vec![false; stage.song.tracks.len()];
    visible[3] = true;
    visible[9] = true;
    let culled = stage.meter_rows_visible(-4, 24, &visible);
    for (full, partial) in all.iter().zip(&culled) {
        assert_eq!(
            (full.step, full.bar, full.beat, full.in_bar),
            (partial.step, partial.bar, partial.beat, partial.in_bar)
        );
        for (track, shown) in visible.iter().enumerate() {
            if *shown {
                assert_eq!(partial.cells[track], full.cells[track]);
            } else {
                assert!(partial.cells[track].is_none());
            }
        }
    }
    let crate::sequencing::Clip::Pattern(id) = stage.song.session.scenes[0]
        .clip(stage.song.tracks[3].id)
        .unwrap();
    stage.song.pattern_mut(id).unwrap().trig_mut(0).notes[0].velocity = 17;
    let fresh = stage.meter_rows_visible(0, 1, &visible);
    assert_eq!(
        fresh[0].cells[3].as_ref().unwrap().note.as_ref().unwrap().1,
        17
    );
}

#[test]
fn meter_plan_offsets_and_add_parameter_share_the_same_columns() {
    let stage = dense_stage();
    let plan = stage.meter_plan();
    for (track, column) in plan.columns.iter().enumerate() {
        let start = plan.start_of(track);
        assert_eq!(plan.at(start), Some((track, column.fields[0])));
        for (offset, field) in column.fields.iter().enumerate() {
            assert_eq!(plan.at(start + offset), Some((track, *field)));
        }
        assert_eq!(
            stage.meter_add_param_from_plan(track, &plan),
            stage.meter_add_param(track)
        );
    }
}

#[test]
#[ignore = "manual CPU benchmark; no audio device or GPU needed"]
fn meter_draw_benchmark() {
    let mut stage = if let Some(path) = std::env::var_os("DAW_METER_BENCH_PROJECT") {
        let mut stage = Stage::new();
        stage.open(std::path::PathBuf::from(path)).unwrap();
        stage.song_view = true;
        stage
            .transport
            .set_mode(super::super::transport::Mode::Song);
        stage.meter.open = true;
        stage
    } else {
        dense_stage()
    };
    stage
        .transport
        .set_motion(super::super::transport::Motion::Rolling);
    let ctx = egui::Context::default();
    crate::install_stage_fonts(&ctx);
    let mut times = Vec::new();
    let mut counts = [0; 3];
    for number in 0..75 {
        stage.transport.seek(16 * PATTERN_STEP_TICKS + number * 2);
        let start = std::time::Instant::now();
        counts = frame(&stage, &ctx, number);
        if number >= 15 {
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    times.sort_by(f64::total_cmp);
    eprintln!(
        "METER CPU 1920x1080 · tracks={} · median={:.3}ms p95={:.3}ms · last-frame plan/rows/cells={counts:?}",
        stage.song.tracks.len(),
        times[times.len() / 2],
        times[times.len() * 95 / 100]
    );
}
