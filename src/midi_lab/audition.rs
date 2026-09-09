//! Offline preview through a snapshot of the destination instrument and desk.
//! Only the completed, rate-matched PCM enters the existing audition door.
use super::*;
use parking_lot::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub struct Audio {
    pub samples: Arc<Vec<f32>>,
    pub frames: usize,
    pub rate: u32,
}
pub struct Job {
    cancel: AtomicBool,
    result: Mutex<Option<Result<Audio, String>>>,
}
impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MIDI preview worker")
    }
}
impl Job {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn take(&self) -> Option<Result<Audio, String>> {
        self.result.lock().take()
    }
    pub fn start(
        song: crate::sequencing::Song,
        recipe: Recipe,
        destination: Destination,
        notes: Vec<Event>,
        rate: u32,
    ) -> Arc<Self> {
        let job = Arc::new(Self {
            cancel: AtomicBool::new(false),
            result: Mutex::new(None),
        });
        let worker = job.clone();
        std::thread::spawn(move || {
            let result = render(song, &recipe, destination, &notes, rate, &worker);
            if !worker.cancel.load(Ordering::Relaxed) {
                *worker.result.lock() = Some(result);
            }
        });
        job
    }
}

pub fn snapshot(
    mut song: crate::sequencing::Song,
    recipe: &Recipe,
    destination: Destination,
    notes: &[Event],
) -> Result<crate::sequencing::Song, String> {
    if let Some(composition) = &recipe.composition {
        let rendered = composer::render(composition)?;
        return composer::output::audition(song, recipe, destination, &rendered);
    }
    let track = song
        .tracks
        .iter()
        .position(|t| t.id == destination.track)
        .ok_or("Destination track was removed")?;
    let p = song
        .pattern_mut(destination.pattern)
        .ok_or("Destination clip was removed")?;
    write_pattern(p, recipe, notes);
    for t in &mut song.tracks {
        t.blocks.clear();
        t.audio_blocks.clear();
        t.automation.clear();
        t.solo = false;
        t.muted = false;
        t.armed = false;
        t.monitor = crate::sequencing::Monitor::Off;
    }
    song.tempo.clear();
    song.loop_on = false;
    song.place_block(track, destination.pattern, 0, recipe.length)
        .map_err(|e| format!("Could not place the preview clip: {e:?}"))?;
    Ok(song)
}
fn render(
    song: crate::sequencing::Song,
    recipe: &Recipe,
    destination: Destination,
    notes: &[Event],
    rate: u32,
    job: &Job,
) -> Result<Audio, String> {
    let song = snapshot(song, recipe, destination, notes)?;
    let (spec, _) = crate::song_graph::build_song(&song);
    let dir = std::env::temp_dir().join(format!(
        "daw-midi-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0.join("preview.wav"));
            let _ = std::fs::remove_dir(&self.0);
        }
    }
    let cleanup = Cleanup(dir);
    let path = cleanup.0.join("preview.wav");
    let options = crate::audio::bounce::BounceOptions {
        sample_rate: rate,
        block_frames: 256,
        bpm: song.bpm,
        length_beats: if let Some(composition) = &recipe.composition {
            composer::render(composition)?.length as f64 / 48. + 2.
        } else {
            recipe.length as f64 / 48. + 2.
        },
        start_beats: 0.,
        format: crate::audio::bounce::BounceFormat::Float32,
    };
    let timeline = crate::tempo::TempoTable::build(&song, f64::from(rate), song.bpm);
    crate::audio::bounce::bounce_automated_with_tempo_table(
        &spec,
        &options,
        &path,
        &timeline,
        |_, _| {},
        |_| !job.cancel.load(Ordering::Relaxed),
    )
    .map_err(|e| e.to_string())?;
    let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let interleaved = reader
        .samples::<f32>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let frames = interleaved.len() / 2;
    let mut samples = vec![0.; frames * 2];
    for (i, pair) in interleaved.chunks_exact(2).enumerate() {
        samples[i] = pair[0];
        samples[frames + i] = pair[1];
    }
    Ok(Audio {
        samples: Arc::new(samples),
        frames,
        rate,
    })
}
static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn composer_preview_renders_complete_tempo_aware_audio() {
        let mut recipe = Recipe::composed();
        let c = recipe.composition.as_mut().unwrap();
        c.progression("Cmaj7:4 Am7:4").unwrap();
        let song = crate::sequencing::Song::default();
        let destination = Destination {
            track: song.tracks[0].id,
            pattern: song.patterns[0].id,
        };
        let job = Job {
            cancel: AtomicBool::new(false),
            result: Mutex::new(None),
        };
        let audio = render(song, &recipe, destination, &[], 24_000, &job).unwrap();
        assert!(audio.frames >= 24_000 * 5);
        assert!(audio.samples.iter().all(|x| x.is_finite()));
        assert!(audio.samples.iter().any(|x| x.abs() > 0.001));
    }
    #[test]
    fn preview_uses_the_sent_pattern_and_produces_audio_through_its_instrument() {
        let mut recipe = Recipe::default();
        recipe.progression("Cmaj7:1").unwrap();
        let song = crate::sequencing::Song::default();
        let destination = Destination {
            track: song.tracks[0].id,
            pattern: song.patterns[0].id,
        };
        let notes = generate(&recipe).unwrap().notes;
        let preview = snapshot(song.clone(), &recipe, destination, &notes).unwrap();
        let mut sent = song.patterns[0].clone();
        write_pattern(&mut sent, &recipe, &notes);
        assert_eq!(preview.patterns[0], sent);
        assert_eq!(preview.tracks[0].machine, song.tracks[0].machine);
        let job = Job {
            cancel: AtomicBool::new(false),
            result: Mutex::new(None),
        };
        let audio = render(song, &recipe, destination, &notes, 48_000, &job).unwrap();
        assert!(audio.samples.iter().all(|x| x.is_finite()));
        assert!(audio.samples.iter().any(|x| x.abs() > 0.001));
        assert_eq!(audio.samples.len(), audio.frames * 2);
    }
}
