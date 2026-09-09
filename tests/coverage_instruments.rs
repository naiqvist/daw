//! End-to-end checks: a new instrument must survive the actual document,
//! sound library, page resolver, sequencer and realtime graph boundaries.
use daw::{
    devices::DeviceKind,
    pages::{self, PageKey, Subject},
    sequencing::{Note, Pattern, PatternId, Song, TICKS_PER_BEAT},
};

const MACHINES: [DeviceKind; 8] = [
    DeviceKind::Table,
    DeviceKind::Ring,
    DeviceKind::PrismVoice,
    DeviceKind::Mass,
    DeviceKind::Pluck,
    DeviceKind::Vox,
    DeviceKind::Pipe,
    DeviceKind::Glass,
];

#[test]
fn coverage_instruments_have_complete_pages_and_exactly_four_effects() {
    for kind in MACHINES {
        let mut song = Song::default();
        song.add_device(0, kind).unwrap();
        let track = &song.tracks[0];
        let mut exposed = Vec::new();
        for key in PageKey::ALL {
            for page in pages::resolve(track, key) {
                for slot in page.slots.into_iter().flatten() {
                    if let pages::Slot::Param {
                        subject: Subject::Machine,
                        id,
                    } = slot
                    {
                        exposed.push(id);
                    }
                }
            }
        }
        // ALL is an additional view of the existing cells, not another owner.
        exposed.sort_unstable();
        exposed.dedup();
        assert_eq!(
            exposed,
            kind.spec().params.iter().map(|p| p.id).collect::<Vec<_>>(),
            "{kind:?}"
        );
        let own = pages::key_table(kind).unwrap()[PageKey::Fx.index()]
            .as_ref()
            .map_or(0, |p| p.subpages.len());
        assert_eq!(own + pages::fx_sections(kind).unwrap().len(), 4, "{kind:?}");
        for def in kind.spec().params {
            assert!(def.default.is_finite());
            assert!(def.min <= def.default && def.default <= def.max);
        }
    }
}

#[test]
fn coverage_sound_and_document_roundtrip_reach_audible_realtime_graphs() {
    use daw::audio::graph::ProcessCtx;
    for kind in MACHINES {
        let mut song = Song::default();
        song.patterns.clear();
        for t in &mut song.tracks {
            t.blocks.clear();
        }
        let id = song.add_device(0, kind).unwrap();
        let walker = match kind {
            DeviceKind::Table => daw::params::table::WALK_X,
            DeviceKind::Ring => daw::params::ring::WALK_X,
            DeviceKind::PrismVoice => daw::params::prism_voice::WALK_X,
            DeviceKind::Mass => daw::params::mass::WALK_X,
            DeviceKind::Pluck => daw::params::pluck::WALK_X,
            DeviceKind::Vox => daw::params::vox::WALK_X,
            DeviceKind::Pipe => daw::params::pipe::WALK_X,
            DeviceKind::Glass => daw::params::glass::WALK_X,
            _ => unreachable!(),
        };
        let def = &kind.spec().params[walker as usize];
        let value = def.min + (def.max - def.min) * 0.25;
        song.device_mut(id).unwrap().set(walker, value);
        let sound = daw::sound::Sound::capture(&song.tracks[0]);
        let encoded = ron::ser::to_string(&sound).unwrap();
        let restored: daw::sound::Sound = ron::from_str(&encoded).unwrap();
        assert_eq!(restored.machine_device().unwrap().value(walker), value);
        let mut pattern = Pattern::empty(PatternId(0), "coverage".into());
        pattern.set_primary(
            0,
            Note::new(
                if kind == DeviceKind::Mass { 36 } else { 60 },
                TICKS_PER_BEAT,
                110,
            ),
        );
        pattern.trig_mut(0).set_lock(walker, value);
        song.adopt_pattern(pattern, 0, 0, 4 * TICKS_PER_BEAT)
            .unwrap();
        let encoded = ron::ser::to_string(&song).unwrap();
        let restored: Song = ron::from_str(&encoded).unwrap();
        assert_eq!(restored, song);
        let (spec, _) = daw::song_graph::build_song(&restored);
        let mut schedule = spec.compile(48000, 256).unwrap();
        let input = [0.0; 512];
        let mut out = [0.0; 512];
        let mut peak = 0.0f32;
        for block in 0..128 {
            let position = block * 256;
            assert_no_alloc::assert_no_alloc(|| {
                schedule.run(
                    &mut out,
                    &ProcessCtx {
                        device_input: &input,
                        in_channels: 2,
                        block_frames: 256,
                        offset: 0,
                        len: 256,
                        playing: true,
                        position,
                        beat: position as f64 / 24000.0,
                        beats_per_sample: 1.0 / 24000.0,
                        discontinuity: block == 0,
                    },
                )
            });
            assert!(
                out.iter().all(|s| s.is_finite()),
                "{kind:?} nonfinite graph"
            );
            peak = out.iter().map(|s| s.abs()).fold(peak, f32::max);
        }
        assert!(
            peak > 0.001,
            "{kind:?} silent graph after sound/document roundtrip, peak={peak}"
        );
    }
}
