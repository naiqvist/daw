use super::tests::{BLOCK, SR, bank, material, run, tone, transparent};
use super::*;

#[test]
fn slice_loops_sustain_and_the_note_still_transposes() {
    let mut p = transparent();
    p.mode = sp::MODE_SLICE;
    p.slice = 2.0;
    p.loop_mode = 1.0;
    p.loop_start = 0.2;
    p.loop_size = 0.25;
    let mut v = bank(
        p,
        material(4000, |i| if (1000..2000).contains(&i) { 0.25 } else { 0.9 }),
    );
    v.set_slices(&[0, 1000, 2000, 3000]);
    v.note_on(72, 127, 0);
    let (l, _) = run(&mut v, 4000, BLOCK);
    assert!(v.any_active());
    assert!(l[64..].iter().all(|s| (*s - 0.25).abs() < 1e-6));
    let voice = v.voices.iter().find(|v| v.active).unwrap();
    assert!((voice.inc - 2.0).abs() < 1e-9);
}

#[test]
fn position_and_size_are_independent_and_note_loop_tunes_once() {
    let mut p = transparent();
    p.loop_start = 0.2;
    p.loop_size = 0.1;
    let (a, b) = p.loop_region(100.0, 10100.0, SR, 60, 1.0 / 24000.0, 0.0, 0.0);
    p.loop_size = 0.3;
    let (c, d) = p.loop_region(100.0, 10100.0, SR, 60, 1.0 / 24000.0, 0.0, 0.0);
    assert_eq!(a, c);
    assert!((d - c - (b - a) * 3.0).abs() < 0.001);
    p.loop_units = 2.0;
    p.loop_size = 0.5;
    assert_eq!(
        p.loop_region(0.0, 10000.0, SR, 60, 1.0 / 24000.0, 0.0, 0.0),
        p.loop_region(0.0, 10000.0, SR, 72, 1.0 / 24000.0, 0.0, 0.0)
    );
}

#[test]
fn hold_starts_at_selected_loop_and_exit_reaches_recorded_tail() {
    let mut p = transparent();
    p.playback = 3.0;
    p.loop_mode = 1.0;
    p.loop_start = 0.5;
    p.loop_size = 0.1;
    p.speed = 0.0;
    p.loop_exit = 1.0;
    let mut v = bank(p, tone(4000));
    v.note_on(60, 127, 0);
    let (l, _) = run(&mut v, 2000, BLOCK);
    assert!(l.iter().any(|s| s.abs() > 0.1));
    let voice = v.voices.iter().find(|v| v.active).unwrap();
    assert_eq!(voice.pos, 2000.0);
    v.note_off(60);
    run(&mut v, 5000, BLOCK);
    assert!(!v.any_active());
}

#[test]
fn slip_rejoins_original_phrase_and_live_loop_motion_does_not_retrigger() {
    let mut p = transparent();
    p.playback = 3.0;
    p.slip = 1.0;
    p.loop_mode = 1.0;
    p.loop_size = 0.1;
    let mut v = bank(p, tone(20000));
    v.note_on(60, 127, 1);
    run(&mut v, 512, BLOCK);
    v.set_param(sp::SPEED, 0.0);
    run(&mut v, 512, BLOCK);
    v.set_param(sp::LOOP_START, 0.4);
    run(&mut v, 64, BLOCK);
    let voice = v.voices.iter().find(|v| v.active).unwrap();
    assert!(voice.gated);
    assert_eq!(voice.since_gate, 1088);
    v.set_param(sp::SPEED, 100.0);
    run(&mut v, 32, BLOCK);
    let voice = v.voices.iter().find(|v| v.active).unwrap();
    assert!((voice.pos - voice.reader.slip_position()).abs() < 1.0);
}

#[test]
fn a_new_lock_cannot_rewrite_an_older_tail() {
    let mut p = transparent();
    p.loop_mode = 1.0;
    p.loop_size = 0.1;
    let mut v = bank(p, tone(20000));
    v.plock(sp::TIME, Some(400.0));
    v.plock(sp::CUTOFF, Some(700.0));
    v.note_on(60, 127, 1);
    v.plock(sp::TIME, None);
    v.plock(sp::CUTOFF, Some(4000.0));
    v.note_on(67, 127, 2);
    v.set_param(sp::TIME, 200.0);
    v.set_param(sp::CUTOFF, 10000.0);
    let old = v.voices.iter().find(|v| v.age == 1 && v.active).unwrap();
    assert_eq!(old.patch.time, 400.0);
    assert_eq!(old.patch.cutoff_hz, 700.0);
    let new = v.voices.iter().find(|v| v.age == 2 && v.active).unwrap();
    assert_eq!(new.patch.time, 200.0);
    assert_eq!(new.patch.cutoff_hz, 4000.0);
    v.plock_glide(sp::CUTOFF, 0.5);
    let old = v.voices.iter().find(|v| v.age == 1 && v.active).unwrap();
    assert_eq!(old.patch.cutoff_hz, 700.0);
    let new = v.voices.iter().find(|v| v.age == 2 && v.active).unwrap();
    assert_eq!(new.patch.cutoff_hz, 7000.0);
}

#[test]
fn negative_speed_starts_at_the_end_and_gain_locks_are_note_local() {
    let mut p = transparent();
    p.speed = -100.0;
    p.playback = 2.0;
    let mut v = bank(p, tone(4800));
    v.note_on(60, 127, 1);
    run(&mut v, 256, BLOCK);
    assert!(v.any_active());
    let head = v.voices.iter().find(|v| v.active).unwrap();
    assert!((head.pos - 4543.0).abs() < 1.0);
    let mut a = bank(transparent(), tone(4800));
    let mut b = bank(transparent(), tone(4800));
    a.plock(sp::GAIN, Some(-6.0206));
    a.note_on(60, 127, 1);
    b.note_on(60, 127, 1);
    let (quiet, _) = run(&mut a, 512, BLOCK);
    let (loud, _) = run(&mut b, 512, BLOCK);
    for (x, y) in quiet.iter().zip(loud) {
        assert!((*x - y * 0.5).abs() < 1e-5);
    }
}

#[test]
fn advanced_polyphony_is_finite_allocation_free_and_split_exact() {
    for method in [0.0, 1.0, 2.0, 3.0] {
        let mut p = transparent();
        p.playback = method;
        p.loop_mode = 1.0;
        p.loop_start = 0.1;
        p.loop_size = 0.2;
        p.loop_fade = 0.3;
        p.time = 400.0;
        p.scan = 0.1;
        p.travel = 0.5;
        p.comb_mix = 0.2;
        p.comb_feed = -0.7;
        p.cutoff_hz = 3500.0;
        p.filter_slope = 1.0;
        p.attack_shape = -0.5;
        let mut a = bank(p, tone(4096));
        let mut b = bank(p, tone(4096));
        for note in 48..64 {
            a.note_on(note, 90, note as u64);
            b.note_on(note, 90, note as u64);
        }
        let (whole, _) = run(&mut a, 2048, BLOCK);
        let (split, _) = run(&mut b, 2048, 37);
        assert!(whole.iter().all(|v| v.is_finite()));
        assert_eq!(whole, split, "method {method}");
        let mut out = [0.0; BLOCK];
        let mut gain = Ramp::across(1.0, 1.0, BLOCK);
        assert_no_alloc::assert_no_alloc(|| {
            a.set_param(sp::LOOP_START, 0.3);
            a.note_on(72, 100, 80);
            a.render(&mut out, 0, &mut gain);
            a.all_sound_off();
        });
    }
}
