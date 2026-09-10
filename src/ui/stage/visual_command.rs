//! Optional native visual score editor. No calls into the live audio graph.
use super::Stage;
use crate::visuals::{self, Compiled, Frame, Score, command, gpu};
use std::sync::atomic::Ordering;

pub(super) const COMMANDS: &[crate::ui::palette::TypedCommand] = &[
    crate::ui::palette::TypedCommand {
        name: "visual",
        usage: "visual on/off | inspect/params | clip/layer/set/key/ramp/place | lfo/pulse/random | lock/unlock | load/save <path> | export <width> <height> <fps> <file.mp4>",
    },
];

pub(super) struct Preview {
    pub(super) compiled: Compiled,
    pub(super) gpu: gpu::Lease,
    frame: Frame,
    cache: Option<(u64, u32)>,
    evaluations: u64,
}
impl Preview {
    fn new(score: &Score) -> Result<Self, String> {
        Ok(Self {
            compiled: Compiled::new(score)?,
            gpu: gpu::lease(),
            frame: bytemuck::Zeroable::zeroed(),
            cache: None,
            evaluations: 0,
        })
    }
    pub(super) fn frame(&mut self, tick: f64, aspect: f32) -> Frame {
        let key = (tick.to_bits(), aspect.to_bits());
        if self.cache != Some(key) {
            self.frame = self.compiled.frame(tick, aspect);
            self.cache = Some(key);
            self.evaluations += 1;
        }
        self.frame
    }
}

impl Stage {
    pub(super) fn visual_off(&mut self) {
        self.visual_preview = None;
        if let Some(job) = &self.visual_export {
            job.request_cancel();
        }
    }

    pub(super) fn apply_visual_command(&mut self, input: &str) -> bool {
        let rest = input.trim().strip_prefix("visual").unwrap_or(input).trim();
        let w: Vec<_> = rest.split_whitespace().collect();
        let result = (|| -> Result<String, String> {
            // These must still work with a malformed or future-version payload.
            if w == ["off"] {
                self.visual_off();
                return Ok(if self.visual_export.is_some() {
                    "VISUAL STOPPING"
                } else {
                    "VISUAL OFF · no preview / worker"
                }
                .into());
            }
            if w == ["cancel"] {
                if let Some(job) = &self.visual_export {
                    job.request_cancel();
                }
                return Ok("VIDEO CANCELLING".into());
            }
            if w == ["params"] {
                return Ok(visuals::PARAMS
                    .iter()
                    .map(|p| format!("{}={}..{}", p.0, p.1, p.2))
                    .collect::<Vec<_>>()
                    .join(" · "));
            }
            let score = command::decode(self.song.visuals.as_deref())?;
            match w.as_slice() {
                ["on"] => {
                    if self
                        .visual_export
                        .as_ref()
                        .is_some_and(|job| job.cancel.load(Ordering::Relaxed))
                    {
                        return Err("wait for VISUAL OFF before restarting".into());
                    }
                    self.visual_preview = Some(Preview::new(&score)?);
                    Ok("VISUAL ON · transport synced".into())
                }
                ["inspect"] => Ok(format!(
                    "VISUAL {} · {} · {} clips / {} placements · evaluations={} · worker={}",
                    if self.visual_preview.is_some() {
                        "ON"
                    } else if self.visual_export.is_some() {
                        "EXPORT"
                    } else {
                        "OFF"
                    },
                    if score.locked { "LOCKED" } else { "editable" },
                    score.clips.len(),
                    score.arrangement.len(),
                    self.visual_preview.as_ref().map_or(0, |p| p.evaluations),
                    self.visual_export.is_some()
                )),
                ["save", path @ ..] if !path.is_empty() => {
                    use std::io::Write;
                    let text = command::encode(&score)?;
                    let path = path.join(" ");
                    let mut file = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .map_err(|e| format!("{path}: {e}"))?;
                    file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
                    Ok(format!("visual score saved · {path}"))
                }
                ["export", width, height, fps, path @ ..] if !path.is_empty() => {
                    if self.visual_export.is_some() {
                        return Err("video worker already active; cancel or wait".into());
                    }
                    let width = width.parse().map_err(|_| "width needs integer pixels")?;
                    let height = height.parse().map_err(|_| "height needs integer pixels")?;
                    let fps = fps.parse().map_err(|_| "fps needs integer")?;
                    self.visual_export = Some(visuals::export::Job::start(
                        self.song.clone(),
                        score,
                        path.join(" ").into(),
                        [width, height],
                        fps,
                    )?);
                    Ok("VIDEO RENDERING · immutable audio + visual snapshot".into())
                }
                _ => {
                    let next = command::edit(&score, rest)?;
                    if next != score || self.song.visuals.is_none() {
                        let encoded = command::encode(&next)?;
                        if let Some(preview) = &mut self.visual_preview {
                            preview.compiled = Compiled::new(&next)?;
                            preview.cache = None;
                        }
                        self.song.visuals = Some(encoded);
                        self.settle(); // History + persistence; NOT an audio revision.
                    }
                    Ok(format!(
                        "visual · {} · {} clips / {} placements",
                        w.first().copied().unwrap_or(""),
                        next.clips.len(),
                        next.arrangement.len()
                    ))
                }
            }
        })();
        match result {
            Ok(message) => {
                self.notice = Some(message);
                true
            }
            Err(error) => {
                self.notice = Some(format!("REFUSED visual · {error}"));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visual_native_edit_undo_and_off_never_recompile_audio() {
        let mut s = Stage::new();
        let rev = s.revision();
        for cmd in [
            "visual clip a 192",
            "visual layer a b rings",
            "visual place a 0 192 once",
            "visual key a b hue 48 0.1",
            "visual lock",
        ] {
            assert!(s.apply_timeline_command(cmd), "{:?}", s.notice);
            assert_eq!(s.revision(), rev);
        }
        assert!(s.apply_timeline_command("visual on"));
        let p = s.visual_preview.as_mut().unwrap();
        let weak = std::sync::Arc::downgrade(&p.gpu);
        let frame = p.frame(48.5, 16.0 / 9.0);
        p.frame(48.5, 16.0 / 9.0);
        assert_eq!(p.evaluations, 1);
        assert_eq!(frame.info[1], 1.0);
        assert!(!s.apply_timeline_command("visual clear"));
        assert!(s.apply_timeline_command("visual off"));
        assert!(weak.upgrade().is_none());
        assert!(s.visual_preview.is_none());
        assert!(s.visual_export.is_none());
        let mut previous = s.song.clone();
        assert!(s.history.undo(&mut previous));
        s.adopt_song(previous);
        assert_eq!(s.revision(), rev);
        assert!(!command::decode(s.song.visuals.as_deref()).unwrap().locked);
    }
    #[test]
    fn visual_off_works_even_with_unreadable_payload() {
        let mut s = Stage::new();
        s.song.visuals = Some("future payload".into());
        assert!(s.apply_timeline_command("visual off"));
        assert!(!s.apply_timeline_command("visual on"));
        assert_eq!(s.song.visuals.as_deref(), Some("future payload"));
    }
}
