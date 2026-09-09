//! Install eight starter sounds and render their musical interaction demos.
//! RUSTC_WRAPPER="" cargo run --example coverage_demo -- /path/to/demo
use daw::{
    audio::graph::Ramp,
    devices::DeviceKind,
    sequencing::{Clip, Note, Pattern, PatternId, Slot, Song, TICKS_PER_BEAT, TrackKind},
};
use std::path::{Path, PathBuf};

fn wav(path: &Path, left: &[f32], right: &[f32]) -> Result<(), Box<dyn std::error::Error>> {
    let mut w = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for (&l, &r) in left.iter().zip(right) {
        w.write_sample(l)?;
        w.write_sample(r)?;
    }
    w.finalize()?;
    Ok(())
}

macro_rules! audition {
    ($module:ident, $params:ident, $voices:ident, $edits:expr, $pitch:expr) => {{
        use daw::audio::$module::{$params, $voices};
        let mut patch = $params::default();
        for &(id, value) in $edits {
            patch.set(id, value);
        }
        let mut bank = $voices::new();
        bank.prepare(48000.0, 256, patch);
        let mut left = vec![0.0; 48000 * 8];
        let mut right = left.clone();
        // Three short notes, then one held note: the same patch at two time scales.
        let events = [
            (0, true, $pitch),
            (18000, false, $pitch),
            (24000, true, $pitch + 7),
            (42000, false, $pitch + 7),
            (48000, true, $pitch + 12),
            (66000, false, $pitch + 12),
            (96000, true, $pitch),
            (288000, false, $pitch),
        ];
        let mut event = 0;
        let mut at = 0;
        while at < left.len() {
            while event < events.len() && events[event].0 == at {
                let (_, on, pitch) = events[event];
                if on {
                    bank.note_on(pitch, 106, at as u64);
                } else {
                    bank.note_off(pitch);
                }
                event += 1;
            }
            let next = events.get(event).map_or(left.len(), |e| e.0);
            let n = 256.min(left.len() - at).min(next - at);
            let level = 0.65;
            bank.render(&mut left[at..at + n], 0, &mut Ramp::across(level, level, n));
            right[at..at + n].copy_from_slice(bank.right(n));
            at += n;
        }
        (left, right)
    }};
}

fn render(kind: DeviceKind, edits: &[(u32, f32)], pitch: u8) -> (Vec<f32>, Vec<f32>) {
    match kind {
        DeviceKind::Table => audition!(table, TableParams, TableVoices, edits, pitch),
        DeviceKind::Ring => audition!(ring, RingParams, RingVoices, edits, pitch),
        DeviceKind::PrismVoice => audition!(
            prism_voice,
            PrismVoiceParams,
            PrismVoiceVoices,
            edits,
            pitch
        ),
        DeviceKind::Mass => audition!(mass, MassParams, MassVoices, edits, pitch),
        DeviceKind::Pluck => audition!(pluck, PluckParams, PluckVoices, edits, pitch),
        DeviceKind::Vox => audition!(vox, VoxParams, VoxVoices, edits, pitch),
        DeviceKind::Pipe => audition!(pipe, PipeParams, PipeVoices, edits, pitch),
        DeviceKind::Glass => audition!(glass, GlassParams, GlassVoices, edits, pitch),
        _ => unreachable!("demo has only the eight coverage instruments"),
    }
}

fn walker(kind: DeviceKind) -> u32 {
    match kind {
        DeviceKind::Table => daw::params::table::WALK_X,
        DeviceKind::Ring => daw::params::ring::WALK_X,
        DeviceKind::PrismVoice => daw::params::prism_voice::WALK_X,
        DeviceKind::Mass => daw::params::mass::WALK_X,
        DeviceKind::Pluck => daw::params::pluck::WALK_X,
        DeviceKind::Vox => daw::params::vox::WALK_X,
        DeviceKind::Pipe => daw::params::pipe::WALK_X,
        DeviceKind::Glass => daw::params::glass::WALK_X,
        _ => unreachable!("lock demo has only the eight coverage instruments"),
    }
}

fn lock_session(song: &Song, machines: &[(DeviceKind, u8)]) -> Result<Song, String> {
    let mut song = song.clone();
    for (index, &(kind, pitch)) in machines.iter().enumerate() {
        let block = song.tracks[index].blocks.first().ok_or("missing block")?;
        let id = block.pattern_id;
        let tag = song.pattern(id).ok_or("missing pattern")?.tag.clone();
        let mut pattern = Pattern::empty(id, format!("{} source locks", kind.spec().name));
        pattern.tag = tag;
        pattern.length_ticks = 8 * TICKS_PER_BEAT;
        for step in [0, 8, 16, 24] {
            pattern.set_primary(step, Note::new(pitch, TICKS_PER_BEAT, 110));
        }
        let id = walker(kind);
        let def = kind
            .spec()
            .params
            .iter()
            .find(|p| p.id == id)
            .ok_or("walker")?;
        for (step, fraction) in [(8, 0.85), (16, 0.15)] {
            pattern
                .trig_mut(step)
                .set_lock(id, def.min + (def.max - def.min) * fraction);
        }
        let pattern_id = pattern.id;
        *song.pattern_mut(pattern_id).ok_or("missing pattern")? = pattern;
    }
    Ok(song)
}

fn verify_locks(song: &Song, machines: &[(DeviceKind, u8)]) -> Result<(), String> {
    for (index, &(kind, pitch)) in machines.iter().enumerate() {
        let track = &song.tracks[index];
        let pattern_id = track.blocks.first().ok_or("missing block")?.pattern_id;
        let pattern = song.pattern(pattern_id).ok_or("missing pattern")?;
        let scene = &song.session.scenes[index];
        if scene.slots.len() != 1 || scene.clip(track.id) != Some(Clip::Pattern(pattern_id)) {
            return Err(format!("{kind:?}: scene must contain its own phrase only"));
        }
        let id = walker(kind);
        let def = kind
            .spec()
            .params
            .iter()
            .find(|p| p.id == id)
            .ok_or("walker")?;
        for step in 0..daw::sequencing::PATTERN_STEPS {
            let trig = pattern.trig(step);
            if [0, 8, 16, 24].contains(&step) {
                if !trig.enabled || trig.notes != [Note::new(pitch, TICKS_PER_BEAT, 110)] {
                    return Err(format!(
                        "{kind:?}: step {step} changed its note or velocity"
                    ));
                }
            } else if trig.enabled || !trig.notes.is_empty() {
                return Err(format!("{kind:?}: unexpected note at step {step}"));
            }
            let fraction = match step {
                8 => Some(0.85),
                16 => Some(0.15),
                _ => None,
            };
            if let Some(fraction) = fraction {
                if trig.locks.len() != 1
                    || trig.locks[0].param != id
                    || trig.locks[0].device.is_some()
                    || trig.locks[0].value != def.min + (def.max - def.min) * fraction
                {
                    return Err(format!("{kind:?}: wrong source lock at step {step}"));
                }
            } else if !trig.locks.is_empty() {
                return Err(format!(
                    "{kind:?}: base/restored step {step} still has a lock"
                ));
            }
        }
    }
    Ok(())
}

fn verify_lock_graph(song: &Song) -> Result<(), Box<dyn std::error::Error>> {
    use daw::audio::graph::ProcessCtx;
    let (spec, _) = daw::song_graph::build_song(song);
    let mut schedule = spec.compile(48000, 256)?;
    let input = [0.; 512];
    let mut output = [0.; 512];
    let beats_per_sample = song.bpm / (60. * 48000.);
    let frames = (64. / beats_per_sample).ceil() as usize;
    let mut peaks = [[0_f32; 4]; 8];
    for position in (0..frames).step_by(256) {
        schedule.run(
            &mut output,
            &ProcessCtx {
                device_input: &input,
                in_channels: 2,
                block_frames: 256,
                offset: 0,
                len: 256,
                playing: true,
                position: position as u64,
                beat: position as f64 * beats_per_sample,
                beats_per_sample,
                discontinuity: position == 0,
            },
        );
        for (offset, sample) in output.chunks_exact(2).enumerate() {
            if !sample.iter().all(|s| s.is_finite()) {
                return Err("LOCKS graph produced nonfinite audio".into());
            }
            let beat = (position + offset) as f64 * beats_per_sample;
            let track = (beat / 8.) as usize;
            let note = ((beat % 8.) / 2.) as usize;
            if track < 8 {
                peaks[track][note] = sample
                    .iter()
                    .map(|s| s.abs())
                    .fold(peaks[track][note], f32::max);
            }
        }
    }
    for (index, notes) in peaks.iter().enumerate() {
        if notes.iter().any(|peak| *peak < 0.0001) {
            return Err(format!(
                "LOCKS graph track {} has a silent note: {notes:?}",
                index + 1
            )
            .into());
        }
        println!(
            "LOCKS {}: four finite audible notes, peaks {notes:?}",
            song.tracks[index].name
        );
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Offline renders use the callback's floating-point treatment of tails.
    #[cfg(target_arch = "x86_64")]
    // SAFETY: only this thread's handling of subnormal values changes.
    unsafe {
        let mut mxcsr = 0u32;
        std::arch::asm!("stmxcsr [{ptr}]", "or dword ptr [{ptr}], 0x8040", "ldmxcsr [{ptr}]",
            ptr = in(reg) &mut mxcsr, options(nostack));
    }
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "/tmp/daw-coverage-demo".into()),
    );
    std::fs::create_dir_all(&dir)?;
    let machines = [
        (DeviceKind::Table, daw::params::table::DEMOS, 60),
        (DeviceKind::Ring, daw::params::ring::DEMOS, 60),
        (DeviceKind::PrismVoice, daw::params::prism_voice::DEMOS, 60),
        (DeviceKind::Mass, daw::params::mass::DEMOS, 36),
        (DeviceKind::Pluck, daw::params::pluck::DEMOS, 60),
        (DeviceKind::Vox, daw::params::vox::DEMOS, 48),
        (DeviceKind::Pipe, daw::params::pipe::DEMOS, 60),
        (DeviceKind::Glass, daw::params::glass::DEMOS, 60),
    ];
    let mut song = Song::default();
    song.bpm = 130.0;
    song.patterns.clear();
    for track in &mut song.tracks {
        track.blocks.clear();
    }
    for scene in &mut song.session.scenes {
        scene.slots.clear();
    }
    let mut report = String::from(
        "Eight coverage instruments\n\nThe session plays one machine after another at 130 BPM. Each scene auditions one instrument. Tab switches to the timeline, which plays the machines in sequence. The WAV files play three short notes, then one held note. Sounds are installed in the plain lane shelf; no lane types are added.\n\n",
    );
    for (index, &(kind, demos, pitch)) in machines.iter().enumerate() {
        if index > 0 {
            song.add_track(TrackKind::Instrument);
        }
        song.tracks[index].name = kind.spec().name.to_uppercase();
        song.tracks[index].muted = false;
        let id = song.add_device(index, kind).ok_or("machine slot")?;
        let starter = daw::sound::Sound::capture(&song.tracks[index]);
        let name = format!("{} starter", kind.spec().name);
        // A rerun refreshes the local artifact; it preserves an existing library sound.
        daw::sound::save(&dir.join("sounds"), &name, &starter)?;
        let library = daw::sound::dir();
        if !daw::sound::path_of(&library, "plain", &name).exists() {
            daw::sound::save(&library, &name, &starter)?;
        }
        for (patch_name, edits) in
            std::iter::once(("starter", &[][..])).chain(demos.iter().copied())
        {
            let mut sound = starter.clone();
            sound.machine.as_mut().ok_or("sound machine")?.overrides = edits.to_vec();
            let name = format!("{} {patch_name}", kind.spec().name);
            daw::sound::save(&dir.join("sounds"), &name, &sound)?;
            if !daw::sound::path_of(&library, "plain", &name).exists() {
                daw::sound::save(&library, &name, &sound)?;
            }
            let (l, r) = render(kind, edits, pitch);
            let peak = l.iter().chain(&r).map(|s| s.abs()).fold(0.0f32, f32::max);
            if !l.iter().chain(&r).all(|s| s.is_finite()) || peak < 0.0001 {
                return Err(format!("{name}: render is nonfinite or silent (peak {peak})").into());
            }
            wav(&dir.join(format!("{name}.wav")), &l, &r)?;
            let rms = (l
                .iter()
                .chain(&r)
                .map(|s| f64::from(*s).powi(2))
                .sum::<f64>()
                / (l.len() + r.len()) as f64)
                .sqrt();
            report += &format!("{name}: peak {peak:.4}, RMS {rms:.5}\n");
            println!("{name}: peak {peak:.4}, RMS {rms:.5}");
        }
        // Independent sequential phrases make it easy to hear each instrument's identity.
        let mut pattern = Pattern::empty(PatternId(0), kind.spec().name.into());
        pattern.length_ticks = 8 * TICKS_PER_BEAT;
        for (step, note, len, vel) in [
            (0, pitch, 1, 110),
            (4, pitch + 7, 1, 85),
            (8, pitch + 12, 1, 100),
            (16, pitch, 3, 110),
        ] {
            pattern.set_primary(step, Note::new(note, len * TICKS_PER_BEAT, vel));
        }
        let block = song
            .adopt_pattern(
                pattern,
                index,
                index * 8 * TICKS_PER_BEAT,
                8 * TICKS_PER_BEAT,
            )
            .ok_or("pattern")?;
        let pattern = song.tracks[index]
            .blocks
            .iter()
            .find(|b| b.id == block)
            .ok_or("block")?
            .pattern_id;
        song.session.scenes[index].slots.push(Slot {
            track: song.tracks[index].id,
            clip: Clip::Pattern(pattern),
        });
        let _ = id;
    }
    let mut stage = daw::ui::stage::Stage::new();
    *stage.song_mut() = song;
    stage.save_as(dir.join("COVERAGE.stage.ron"))?;
    let lock_machines: Vec<_> = machines
        .iter()
        .map(|(kind, _, pitch)| (*kind, *pitch))
        .collect();
    let locks = lock_session(stage.song(), &lock_machines)?;
    *stage.song_mut() = locks;
    let lock_path = dir.join("LOCKS.stage.ron");
    stage.save_as(&lock_path)?;
    stage.open(&lock_path)?;
    verify_locks(stage.song(), &lock_machines)?;
    verify_lock_graph(stage.song())?;
    report += "\nLOCKS.stage.ron: each scene repeats one pitch at one velocity: base, source walker locked to 85% of its range, locked to 15%, then unlocked base. The saved document contains 32 notes and 16 ordinary step locks; its real song graph rendered all four notes audibly for every instrument.\n";
    std::fs::write(dir.join("README.txt"), report)?;
    println!("{}", dir.join("COVERAGE.stage.ron").display());
    println!("{}", lock_path.display());
    Ok(())
}
