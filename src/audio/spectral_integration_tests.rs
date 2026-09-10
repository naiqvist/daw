//! Native graph / PatternClock regression tests, not a separate synth harness.
use super::{graph::*, spectral::SpectralPatch};
use crate::params::spectral as p;
fn fixture() -> (GraphSpec, NodeId) {
    let mut spec = GraphSpec::default();
    let mut patch = SpectralPatch::default();
    patch.params.set(p::ATTACK, 1.0);
    patch.params.set(p::SUSTAIN, 1.0);
    patch.params.set(p::RELEASE, 1.0);
    patch.params.set(p::MORPH, 0.0);
    let notes = vec![Note {
        start_beats: 0.0,
        len_beats: 8.0,
        pitch: 24,
        vel: 100,
        plocks: vec![],
        fx_locks: vec![],
        prob: 1.0,
        cond: None,
    }];
    let id = spec.push(NodeSpec::Spectral {
        notes,
        subloops: vec![],
        loop_len_beats: None,
        patch: Box::new(patch),
    });
    spec.set_output(id);
    (spec, id)
}
fn context(position: u64, discontinuity: bool) -> ProcessCtx<'static> {
    ProcessCtx {
        device_input: &[],
        in_channels: 0,
        block_frames: 256,
        offset: 0,
        len: 256,
        playing: true,
        position,
        beat: position as f64 / 24000.0,
        beats_per_sample: 1.0 / 24000.0,
        discontinuity,
    }
}
#[test]
fn spectral_graph_plays_and_live_highest_bin_parameter_changes_audio_without_recompile() {
    let (spec, id) = fixture();
    let mut audible = spec.compile(48_000, 256).unwrap();
    let mut silent = spec.compile(48_000, 256).unwrap();
    let mut a = [0.0; 512];
    let mut b = a;
    audible.run(&mut a, &context(0, true));
    silent.run(&mut b, &context(0, true));
    assert_eq!(a, b);
    assert!(a.iter().any(|v| v.abs() > 0.001));
    assert_no_alloc::assert_no_alloc(|| {
        for h in 0..128 {
            silent.apply(ParamChange {
                node: id.to_bits(),
                param: p::amp(h),
                value: 0.0,
            });
        }
        silent.run(&mut b, &context(256, false));
        audible.run(&mut a, &context(256, false));
    });
    assert!(b.iter().all(|v| *v == 0.0));
    assert!(a.iter().any(|v| v.abs() > 0.001));
    silent.apply(ParamChange {
        node: id.to_bits(),
        param: p::amp(127),
        value: 1.0,
    });
    silent.run(&mut b, &context(512, false));
    assert!(b.iter().any(|v| v.abs() > 0.001));
    assert_no_alloc::assert_no_alloc(|| silent.run(&mut b, &context(5_000_000, true)));
    assert!(
        b.iter().all(|v| *v == 0.0),
        "seek beyond the notes must cut the voice"
    );
}
#[test]
fn spectral_graph_roundtrip_and_invalid_patch_refusal() {
    let (spec, _) = fixture();
    let text = ron::to_string(spec.iter_ordered().next().unwrap().1).unwrap();
    let mut recalled = GraphSpec::default();
    let id = recalled.push(ron::from_str::<NodeSpec>(&text).unwrap());
    recalled.set_output(id);
    let mut a = spec.compile(48_000, 256).unwrap();
    let mut b = recalled.compile(48_000, 256).unwrap();
    for block in 0..16 {
        let mut x = [0.0; 512];
        let mut y = x;
        let ctx = context(block * 256, block == 0);
        assert_no_alloc::assert_no_alloc(|| {
            a.run(&mut x, &ctx);
            b.run(&mut y, &ctx);
        });
        assert_eq!(x, y);
    }
    let mut bad = GraphSpec::default();
    let mut patch = SpectralPatch::default();
    patch
        .fx
        .routes
        .push(super::spectral_fx::Route::new("missing", "output", 1.0));
    let id = bad.push(NodeSpec::Spectral {
        notes: vec![],
        subloops: vec![],
        loop_len_beats: None,
        patch: Box::new(patch),
    });
    bad.set_output(id);
    assert!(matches!(
        bad.compile(48_000, 256),
        Err(CompileError::SpectralPrepare(_))
    ));
}
