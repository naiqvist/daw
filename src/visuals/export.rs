//! Offline audio + GPU video on an explicitly requested worker. No live taps.
use super::{Compiled, Score, gpu::Offline};
use crate::{
    audio::bounce::{BounceFormat, BounceOptions, bounce_automated_with_tempo_table},
    sequencing::Song,
    tempo::TempoTable,
};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

pub struct Job {
    pub cancel: Arc<AtomicBool>,
    pub progress: Arc<AtomicU32>,
    encoder: Arc<Mutex<Option<Child>>>,
    worker: Option<std::thread::JoinHandle<Result<String, String>>>,
}
impl Job {
    pub fn start(
        song: Song,
        score: Score,
        path: PathBuf,
        size: [u32; 2],
        fps: u32,
    ) -> Result<Self, String> {
        validate(&path, size, fps)?;
        Compiled::new(&score)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU32::new(0));
        let encoder = Arc::new(Mutex::new(None));
        let c = cancel.clone();
        let p = progress.clone();
        let child = encoder.clone();
        let worker = std::thread::Builder::new()
            .name("visual-export".into())
            .spawn(move || run(&song, &score, &path, size, fps, &c, &p, child))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            cancel,
            progress,
            encoder,
            worker: Some(worker),
        })
    }
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        // Killing our encoder unblocks a pending pipe write. Never hold this
        // mutex while waiting for process completion or writing video bytes.
        if let Ok(mut child) = self.encoder.lock()
            && let Some(child) = child.as_mut()
        {
            let _ = child.kill();
        }
    }
    pub fn poll(&mut self) -> Option<Result<String, String>> {
        if !self.worker.as_ref()?.is_finished() {
            return None;
        }
        Some(
            self.worker
                .take()?
                .join()
                .unwrap_or_else(|_| Err("visual export worker panicked".into())),
        )
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.request_cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn validate(path: &Path, size: [u32; 2], fps: u32) -> Result<(), String> {
    if path.exists() {
        return Err("video output exists; choose a new name".into());
    }
    if path.extension().and_then(|s| s.to_str()) != Some("mp4") {
        return Err("video output must end in .mp4".into());
    }
    if size.iter().any(|v| !(16..=3840).contains(v) || v % 2 != 0)
        || ![24, 25, 30, 60].contains(&fps)
    {
        return Err("even dimensions 16..3840; fps 24,25,30,60".into());
    }
    Ok(())
}
struct Encoder(Arc<Mutex<Option<Child>>>);
impl Drop for Encoder {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.0.lock()
            && let Some(mut child) = slot.take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        // Only files created inside this export's unique scratch directory.
        for name in ["audio.wav", "movie.mp4", "encoder.log"] {
            let _ = std::fs::remove_file(self.0.join(name));
        }
        let _ = std::fs::remove_dir(&self.0);
    }
}

fn run(
    song: &Song,
    score: &Score,
    path: &Path,
    size: [u32; 2],
    fps: u32,
    cancel: &AtomicBool,
    progress: &AtomicU32,
    child: Arc<Mutex<Option<Child>>>,
) -> Result<String, String> {
    validate(path, size, fps)?;
    let compiled = Compiled::new(score)?;
    let end = (song.end_tick() as u64).max(compiled.end_tick());
    if end == 0 {
        return Err("nothing arranged to export".into());
    }
    let bpm = song.base_bpm();
    let timeline = TempoTable::build(song, 48000.0, bpm);
    let samples = timeline.sample_at(end as usize);
    // Every frame is mapped independently, using integer rational time.
    let frames = samples.saturating_mul(fps as u64).div_ceil(48000);
    if frames == 0 || frames > fps as u64 * 3600 {
        return Err("export budget: up to one hour".into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let dir = parent.join(format!(".visual-export-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let scratch = Scratch(dir);
    let (spec, _) = crate::song_graph::build_song(song);
    let opts = BounceOptions {
        sample_rate: 48000,
        block_frames: 256,
        bpm,
        length_beats: end as f64 / 48.0,
        start_beats: 0.0,
        format: BounceFormat::Int24,
    };
    bounce_automated_with_tempo_table(
        &spec,
        &opts,
        &scratch.0.join("audio.wav"),
        &timeline,
        |_, _| {},
        |amount| {
            progress.store((amount * 150.0) as u32, Ordering::Relaxed);
            !cancel.load(Ordering::Relaxed)
        },
    )
    .map_err(|e| e.to_string())?;
    if cancel.load(Ordering::Relaxed) {
        return Err("visual export cancelled".into());
    }
    let mut gpu = Offline::new(size)?;
    let log = std::fs::File::create(scratch.0.join("encoder.log")).map_err(|e| e.to_string())?;
    let encoder = Encoder(child);
    let mut process = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-n",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            &format!("{}x{}", size[0], size[1]),
            "-framerate",
            &fps.to_string(),
            "-i",
            "pipe:0",
            "-i",
        ])
        .arg(scratch.0.join("audio.wav"))
        .args([
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "256k",
            "-movflags",
            "+faststart",
            "-shortest",
        ])
        .arg(scratch.0.join("movie.mp4"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    let mut input = process.stdin.take().ok_or("encoder has no input")?;
    *encoder.0.lock().map_err(|_| "encoder lock failed")? = Some(process);
    for index in 0..frames {
        if cancel.load(Ordering::Relaxed) {
            return Err("visual export cancelled".into());
        }
        let sample = frame_sample(index, fps);
        let frame = compiled.frame(
            timeline.beat_at_sample(sample) * 48.0,
            size[0] as f32 / size[1] as f32,
        );
        let bytes = gpu.frame(&frame)?;
        input.write_all(&bytes).map_err(|e| {
            format!(
                "encoder pipe: {e}; {}",
                std::fs::read_to_string(scratch.0.join("encoder.log")).unwrap_or_default()
            )
        })?;
        progress.store(150 + (850 * (index + 1) / frames) as u32, Ordering::Relaxed);
    }
    drop(input);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let success = loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("visual export cancelled".into());
        }
        if let Some(status) = encoder
            .0
            .lock()
            .map_err(|_| "encoder lock failed")?
            .as_mut()
            .ok_or("encoder missing")?
            .try_wait()
            .map_err(|e| e.to_string())?
        {
            break status.success();
        }
        if std::time::Instant::now() >= deadline {
            return Err("encoder finalization timed out".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    if !success {
        return Err(std::fs::read_to_string(scratch.0.join("encoder.log"))
            .unwrap_or_else(|_| "video encoder failed".into()));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("visual export cancelled".into());
    }
    // Atomic, no-clobber publication. The existing destination is never removed.
    std::fs::hard_link(scratch.0.join("movie.mp4"), path).map_err(|e| e.to_string())?;
    Ok(format!(
        "VIDEO COMPLETE · {} · {frames} frames · {}",
        path.display(),
        gpu.adapter
    ))
}
pub fn frame_sample(index: u64, fps: u32) -> u64 {
    index.saturating_mul(48000) / fps.max(1) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visual_frame_clock_is_absolute_not_accumulated() {
        for fps in [24, 25, 30, 60] {
            assert_eq!(frame_sample(3600 * fps as u64, fps), 172800000);
        }
        let mut song = Song::default();
        song.set_base_bpm(120.0);
        song.set_tempo_mark(192, 60.0);
        let time = TempoTable::build(&song, 48000.0, 120.0);
        assert_eq!(time.beat_at_sample(frame_sample(90, 30)), 5.0);
    }
    #[test]
    fn visual_export_refuses_overwrite_and_bad_dimensions() {
        assert!(validate(Path::new("/tmp/a.avi"), [1280, 720], 30).is_err());
        assert!(validate(Path::new("/tmp/new-visual.mp4"), [1279, 720], 30).is_err());
        assert!(validate(Path::new("/tmp/new-visual.mp4"), [1280, 720], 29).is_err());
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("visual-protect-{}-{nonce}.mp4", std::process::id()));
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap()
            .write_all(b"existing user file")
            .unwrap();
        assert!(validate(&path, [1280, 720], 30).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"existing user file");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn visual_cancel_reaps_worker_without_publishing_a_partial_file() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("visual-cancel-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("cancel.mp4");
        let mut score = Score::default();
        for cmd in ["clip a 192000", "layer a b rings", "place a 0 192000 once"] {
            score = super::super::command::edit(&score, cmd).unwrap();
        }
        let job = Job::start(Song::default(), score, path.clone(), [320, 180], 30).unwrap();
        job.request_cancel();
        drop(job); // Join is complete; no detached worker or scratch remains.
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir(dir).unwrap();
    }
}
