//! Generate a self-contained SLICE session and listenable engine renders.
//! cargo run --example slice_demo -- /home/naiqvist/Music/daw/slice-demo
use daw::{
    audio::{
        material::Material,
        sampler::{SamplerParams, SamplerVoices},
    },
    devices::DeviceKind,
    params::sampler as p,
    sequencing::{
        Clip, Note, PATTERN_STEP_TICKS, Pattern, PatternId, Slot, Song, TICKS_PER_BEAT, TrackKind,
    },
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

fn wav(path: &Path, left: &[f32], right: Option<&[f32]>) -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: if right.is_some() { 2 } else { 1 },
            sample_rate: 48000,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for (i, a) in left.iter().enumerate() {
        writer.write_sample((a.clamp(-1.0, 1.0) * 8388607.0) as i32)?;
        if let Some(r) = right {
            writer.write_sample((r[i].clamp(-1.0, 1.0) * 8388607.0) as i32)?;
        }
    }
    writer.finalize()?;
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "/tmp/slice-demo".into()),
    );
    std::fs::create_dir_all(&dir)?;
    // Original synthesized break: no third-party sample dependencies.
    let frames = 192000usize;
    let mut state = 0x589a0e3fu32;
    let samples: Vec<f32> = (0..frames)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let noise = state as f32 / u32::MAX as f32 * 2.0 - 1.0;
            let step = i / 6000;
            let local = (i % 6000) as f32 / 48000.0;
            let kick = if [0, 7, 10, 16, 19, 26].contains(&step) {
                (core::f32::consts::TAU * (49.0 * local + 6.0 * (1.0 - (-local * 30.0).exp())))
                    .sin()
                    * (-local * 23.0).exp()
                    * 0.7
            } else {
                0.0
            };
            let snare = if [4, 12, 20, 28, 30].contains(&step) {
                (noise * 0.62 + (core::f32::consts::TAU * 185.0 * local).sin() * 0.22)
                    * (-local * 32.0).exp()
            } else {
                0.0
            };
            let hat = noise
                * (-local * if step % 2 == 0 { 230.0 } else { 100.0 }).exp()
                * if step % 2 == 0 { 0.10 } else { 0.16 };
            (kick + snare + hat).clamp(-0.95, 0.95)
        })
        .collect();
    let source = dir.join("warehouse-break-120.wav");
    wav(&source, &samples, None)?;
    let material = Material {
        samples: Arc::new(samples),
        channels: 1,
        frames: frames as u64,
        source: source.clone(),
        sample_rate: 48000,
        original_rate: 48000,
        truncated: false,
    };
    let patches: Vec<(&str, Vec<(u32, f32)>)> = vec![
        (
            "CHOPS",
            vec![
                (p::MODE, 2.0),
                (p::CHOKE, 1.0),
                (p::AMP_A, 0.1),
                (p::AMP_R, 25.0),
                (p::RATE, 26000.0),
                (p::BITS, 12.0),
            ],
        ),
        (
            "SUSTAIN",
            vec![
                (p::PLAYBACK, 2.0),
                (p::SPEED, 0.0),
                (p::LOOP_MODE, 1.0),
                (p::LOOP_START, 0.37),
                (p::LOOP_SIZE, 0.18),
                (p::LOOP_FADE, 0.4),
                (p::AMP_A, 1000.0),
                (p::AMP_R, 4000.0),
                (p::CUTOFF, 2400.0),
                (p::FILTER_SLOPE, 1.0),
                (p::COMB_MIX, 0.2),
                (p::COMB_FEED, 0.7),
            ],
        ),
        (
            "SCAN",
            vec![
                (p::PLAYBACK, 3.0),
                (p::TIME, 800.0),
                (p::LOOP_MODE, 1.0),
                (p::LOOP_START, 0.2),
                (p::LOOP_SIZE, 0.08),
                (p::LOOP_FADE, 0.4),
                (p::SCAN, 0.1),
                (p::TRAVEL, 0.5),
                (p::AMP_A, 500.0),
                (p::AMP_R, 5000.0),
                (p::CUTOFF, 1500.0),
                (p::WINDOW, 70.0),
            ],
        ),
        (
            "STRETCH",
            vec![
                (p::PLAYBACK, 1.0),
                (p::TIME, 200.0),
                (p::TUNE, -5.0),
                (p::AMP_A, 0.1),
                (p::AMP_R, 30.0),
                (p::SOURCE_BEATS, 8.0),
            ],
        ),
    ];
    let mut song = Song::default();
    song.bpm = 120.0;
    song.patterns.clear();
    for track in &mut song.tracks {
        track.blocks.clear();
    }
    for scene in &mut song.session.scenes {
        scene.slots.clear();
    }
    for (index, (name, edits)) in patches.iter().enumerate() {
        if index > 0 {
            song.add_track(TrackKind::Instrument);
        }
        song.tracks[index].name = (*name).into();
        song.tracks[index].muted = index != 0;
        let id = song
            .add_device(index, DeviceKind::Sampler)
            .ok_or("sampler slot")?;
        let d = song.device_mut(id).ok_or("sampler")?;
        d.sample = Some(source.clone());
        d.set_slices((0..32).map(|i| i as f64 / 32.0));
        for (key, value) in [
            (p::GAIN, -9.0),
            (p::AMP_S, 1.0),
            (p::ROOT, 60.0),
            (p::SOURCE_BEATS, 8.0),
        ] {
            d.set(key, value);
        }
        for (key, value) in edits {
            d.set(*key, *value);
        }
        let sound = daw::sound::Sound::capture(&song.tracks[index]);
        daw::sound::save(&dir.join("sounds"), name, &sound)?;
        let mut params = SamplerParams::default();
        params.amp_sustain = 1.0;
        for (key, value) in edits {
            params.set(*key, *value);
        }
        let mut bank = SamplerVoices::new(48000.0, 256, params, material.clone());
        bank.set_slices(&(0..32).map(|i| i * 6000).collect::<Vec<_>>());
        let mut l = vec![0.0; 48000 * 8];
        let mut r = l.clone();
        bank.note_on(60, 110, 0);
        for at in (0..l.len()).step_by(256) {
            let n = (l.len() - at).min(256);
            if index == 0 && at % 6144 == 0 {
                bank.plock(p::SLICE, Some(((at / 6144 * 7) % 32 + 1) as f32));
                bank.note_on(60 + if at % 24576 == 0 { 12 } else { 0 }, 110, at as u64);
            }
            if at == 48000 / 256 * 256 * 6 {
                bank.note_off(60);
            }
            let mut gain = daw::audio::graph::Ramp::across(0.35, 0.35, n);
            bank.render(&mut l[at..at + n], 0, &mut gain);
            r[at..at + n].copy_from_slice(bank.right(n));
        }
        let peak = l.iter().chain(&r).fold(0.0f32, |peak, v| peak.max(v.abs()));
        assert!(l.iter().chain(&r).all(|v| v.is_finite()) && peak > 0.001);
        wav(
            &dir.join(format!("{}.wav", name.to_lowercase())),
            &l,
            Some(&r),
        )?;
        println!("{name}: peak {peak:.3}");
        let mut pattern = Pattern::empty(PatternId(0), name.to_lowercase());
        pattern.length_ticks = 16 * TICKS_PER_BEAT;
        if index == 0 {
            for step in 0..64 {
                let mut note = Note::new(
                    60,
                    if step % 8 == 7 {
                        PATTERN_STEP_TICKS / 2
                    } else {
                        PATTERN_STEP_TICKS
                    },
                    if step % 4 == 0 { 115 } else { 90 },
                );
                if step % 16 == 15 {
                    note.pitch = daw::pitch::Pitch::from_midi(72);
                }
                pattern.set_primary(step, note);
                pattern
                    .trig_mut(step)
                    .set_lock(p::SLICE, ((step * 7) % 32 + 1) as f32);
            }
        } else {
            pattern.set_primary(0, Note::new(60, 15 * TICKS_PER_BEAT, 110));
        }
        let block = song
            .adopt_pattern(pattern, index, 0, 16 * TICKS_PER_BEAT)
            .ok_or("pattern")?;
        let pattern = song.tracks[index]
            .blocks
            .iter()
            .find(|b| b.id == block)
            .ok_or("block")?
            .pattern_id;
        song.session.scenes[0].slots.push(Slot {
            track: song.tracks[index].id,
            clip: Clip::Pattern(pattern),
        });
    }
    let mut stage = daw::ui::stage::Stage::new();
    *stage.song_mut() = song;
    stage.save_as(dir.join("SLICE.stage.ron"))?;
    println!("{}", dir.join("SLICE.stage.ron").display());
    Ok(())
}
