//! Lab job lifecycle, editable recipes, and the single undoable sampler send.
//! Stage data stays independent of the graphics toolkit and audio callback.
use super::lab::{Instrument, Kiln};
use super::{FocusScope, RefusalReason, Stage, Step};
use crate::kiln::{
    job::{Action, Job, Request},
    membrane::Render,
};
use crate::sequencing::{Note, TrackId, TrackKind};
use std::sync::Arc;
use std::time::{Duration, Instant};

impl Stage {
    fn kiln_rate(&self) -> u32 {
        self.vitals.stream().map_or(48_000, |s| s.sample_rate)
    }
    pub(super) fn kiln_request(&mut self, action: Action) -> Result<(), RefusalReason> {
        let rate = self.kiln_rate();
        let target = self.addressed_track().map(|i| self.song.tracks[i].id);
        let k = self.lab.focused_kiln_mut().ok_or(RefusalReason::Empty)?;
        let key = k.patch().key(60, 100, rate);
        if action == Action::Replace && k.last_sent.is_none() {
            self.notice = Some("send a sound before replacing it".into());
            return Err(RefusalReason::Unavailable);
        }
        if action == Action::Hear
            && k.key == Some(key)
            && k.render.is_some()
            && k.submitted.is_none()
        {
            k.hear = true;
            k.played = Some(Instant::now());
            k.scrub = None;
            return Ok(());
        }
        let target = if action == Action::Replace {
            k.last_sent
        } else {
            target
        };
        if matches!(action, Action::Send | Action::Replace)
            && k.key == Some(key)
            && k.printed.is_some()
            && k.submitted.is_none()
        {
            let path = k.printed.clone().ok_or(RefusalReason::Empty)?;
            let id = self.send_kiln_material(path, target, action == Action::Replace)?;
            if let Some(k) = self.lab.focused_kiln_mut() {
                k.last_sent = Some(id);
            }
            return Ok(());
        }
        if k.job.is_none() {
            k.job = Some(Arc::new(
                Job::new().map_err(|_| RefusalReason::Unavailable)?,
            ));
        }
        k.key = Some(key);
        k.changed = None;
        k.submitted = Some(action);
        k.status = if matches!(action, Action::Print | Action::Send | Action::Replace) {
            "baking"
        } else {
            "previewing"
        }
        .into();
        if let Some(job) = &k.job {
            job.submit(Request {
                patch: k.patch(),
                rate,
                action,
                target,
            });
        }
        Ok(())
    }
    pub(super) fn poll_kiln(&mut self) -> bool {
        let rate = self.kiln_rate();
        let now = Instant::now();
        let mut busy = false;
        let mut landed = Vec::new();
        for window in &mut self.lab.windows {
            let Instrument::Kiln(k) = &mut window.instrument else {
                continue;
            };
            if !self.lab.open && k.job.is_none() {
                continue;
            }
            let key = k.patch().key(60, 100, rate);
            if k.key.is_some_and(|old| old != key) {
                if let Some(job) = &k.job {
                    job.cancel();
                }
                k.key = None;
                k.submitted = None;
                k.printed = None;
                k.changed = Some(now);
            }
            if k.key.is_none() && k.changed.is_none() {
                k.changed = Some(now);
            }
            if k.changed
                .is_some_and(|t| now.duration_since(t) >= Duration::from_millis(60))
            {
                if k.job.is_none() {
                    match Job::new() {
                        Ok(job) => k.job = Some(Arc::new(job)),
                        Err(e) => {
                            k.status = e;
                            k.changed = None;
                            continue;
                        }
                    }
                }
                k.key = Some(key);
                k.changed = None;
                k.submitted = Some(Action::Preview);
                k.status = "previewing".into();
                if let Some(job) = &k.job {
                    job.submit(Request {
                        patch: k.patch(),
                        rate,
                        action: Action::Preview,
                        target: None,
                    });
                }
            }
            if let Some(done) = k.job.as_ref().and_then(|j| j.take()) {
                if done.key != key {
                    continue;
                }
                k.submitted = None;
                match done.result {
                    Ok((render, path)) => {
                        k.status = format!(
                            "{} · {:.1} ms",
                            if path.is_some() { "baked" } else { "preview" },
                            render.millis
                        );
                        k.render = Some(Arc::new(render));
                        k.printed = path.clone();
                        k.played = Some(now);
                        k.scrub = None;
                        k.hear = done.request.action == Action::Hear;
                        if let Some(path) = path {
                            landed.push((window.id, path, done.request));
                        }
                    }
                    Err(e) => {
                        k.status = format!("failed · {e}");
                        self.notice = Some(k.status.clone());
                    }
                }
            }
            busy |= k.changed.is_some()
                || k.submitted.is_some()
                || (self.lab.open
                    && k.played
                        .is_some_and(|t| now.duration_since(t).as_secs_f32() < 3.0));
        }
        for (window, path, request) in landed {
            if matches!(request.action, Action::Send | Action::Replace) {
                match self.send_kiln_material(
                    path,
                    request.target,
                    request.action == Action::Replace,
                ) {
                    Ok(id) => {
                        if let Some(window) = self.lab.window_mut(window) {
                            let Instrument::Kiln(k) = &mut window.instrument else {
                                continue;
                            };
                            k.last_sent = Some(id);
                        }
                    }
                    Err(_) => {
                        self.notice = Some("bake saved · destination track no longer exists".into())
                    }
                }
            } else {
                self.notice = Some(format!(
                    "printed · {}",
                    path.file_stem().unwrap_or_default().to_string_lossy()
                ));
            }
            if let Some(browser) = &mut self.browser {
                browser.set_children(
                    super::Shelf::Samples,
                    super::sample_nodes(&self.library_snapshot.assets),
                    super::BrowserStatus::Ready,
                );
            }
        }
        busy
    }
    /// Deterministic render for the headless lab poses; never called by live UI.
    #[doc(hidden)]
    pub fn pose_kiln(&mut self, t: f32) {
        for window in &mut self.lab.windows {
            let Instrument::Kiln(k) = &mut window.instrument else {
                continue;
            };
            let patch = k.patch();
            if let Some(render) = crate::kiln::membrane::Membrane::default().render(
                &patch,
                60,
                100,
                48_000,
                false,
                &mut |_| true,
            ) {
                k.status = format!("preview · {:.1} ms", render.millis);
                k.render = Some(Arc::new(render));
                k.key = Some(patch.key(60, 100, 48_000));
                k.scrub = Some(t);
                k.changed = None;
            }
        }
    }
    pub fn take_kiln_audio(&mut self) -> Option<Render> {
        for window in &mut self.lab.windows {
            let Instrument::Kiln(k) = &mut window.instrument else {
                continue;
            };
            if k.hear {
                k.hear = false;
                return k.render.as_deref().cloned();
            }
        }
        None
    }
    pub(super) fn open_kiln_recipe(&mut self, path: &std::path::Path) -> Result<(), RefusalReason> {
        let recipe = crate::kiln::files::read(path).map_err(|_| RefusalReason::Unavailable)?;
        let mut k = Kiln::default();
        k.set_patch(recipe.patch);
        self.lab.open_window(Instrument::Kiln(k));
        self.lab.open = true;
        self.lab.inside = true;
        self.browser = None;
        self.notice = Some("kiln · recipe reopened".into());
        Ok(())
    }
    fn send_kiln_material(
        &mut self,
        path: std::path::PathBuf,
        target: Option<TrackId>,
        replace: bool,
    ) -> Result<TrackId, RefusalReason> {
        let target = target.and_then(|id| self.song.tracks.iter().position(|t| t.id == id));
        if replace && target.is_none() {
            return Err(RefusalReason::Unavailable);
        }
        self.settle();
        self.fit_playing();
        let index = if replace {
            target.ok_or(RefusalReason::Empty)?
        } else {
            self.song.add_track(TrackKind::Instrument);
            let index = target.map_or(self.song.tracks.len() - 1, |i| i + 1);
            let track = self.song.tracks.pop().ok_or(RefusalReason::Empty)?;
            self.song.tracks.insert(index, track);
            self.playing.insert(index, None);
            index
        };
        let device = if replace {
            self.song.tracks[index]
                .machine
                .as_ref()
                .filter(|d| d.kind == crate::devices::DeviceKind::Sampler)
                .map(|d| d.id)
                .ok_or(RefusalReason::Unavailable)?
        } else {
            self.song.set_lane(index, crate::lane::Lane::Drum);
            self.song
                .add_device(index, crate::devices::DeviceKind::Sampler)
                .ok_or(RefusalReason::Unavailable)?
        };
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if let Some(d) = self.song.device_mut(device) {
            d.sample = Some(path);
            d.slices = vec![0.0];
            if !replace {
                d.set(crate::params::sampler::MODE, 2.0);
                d.set(crate::params::sampler::ROOT, 60.0);
                // Start with the preamp and drive bypassed; the sampler's
                // remaining controls retain their normal defaults.
                d.set(crate::params::sampler::PREAMP, 0.0);
                d.set(crate::params::sampler::DRIVE, 0.0);
            }
        }
        self.song.tracks[index].name = name.clone();
        let id = self.song.tracks[index].id;
        if self.song.session.scenes.is_empty() {
            self.song.session.scenes.push(Default::default());
        }
        if let Some(pattern) = self.song.fill_slot(index, 0) {
            if let Some(p) = self.song.pattern_mut(pattern) {
                p.set_primary(0, Note::new(60, crate::sequencing::PATTERN_STEP_TICKS, 100));
            }
        }
        self.fit_playing();
        self.fit_session();
        self.arrangement.track = index;
        if let FocusScope::Lattice(lattice) = self.focus.root_mut() {
            lattice.focus_col(index);
            for _ in 0..lattice.rows() {
                lattice.step(Step::Up);
            }
        }
        self.inside = None;
        self.touched();
        self.settle();
        self.notice = Some(format!("sent · {name} · tr {:02}", index + 1));
        Ok(id)
    }
    pub(super) fn kiln_orbit(&mut self, step: Step) -> Result<(), RefusalReason> {
        let k = self.lab.focused_kiln_mut().ok_or(RefusalReason::Empty)?;
        match step {
            Step::Left => k.camera[0] -= 0.12,
            Step::Right => k.camera[0] += 0.12,
            Step::Up => k.camera[1] = (k.camera[1] + 0.1).min(1.45),
            Step::Down => k.camera[1] = (k.camera[1] - 0.1).max(-1.3),
        }
        Ok(())
    }
    pub(super) fn kiln_zoom(&mut self, closer: bool) -> Result<(), RefusalReason> {
        let k = self.lab.focused_kiln_mut().ok_or(RefusalReason::Empty)?;
        k.camera[2] = (k.camera[2] * if closer { 0.9 } else { 1.1 }).clamp(2.4, 9.0);
        Ok(())
    }
    pub(super) fn kiln_scrub(&mut self, forward: bool) -> Result<(), RefusalReason> {
        let k = self.lab.focused_kiln_mut().ok_or(RefusalReason::Empty)?;
        let length = k.render.as_ref().map_or(2.0, |r| r.animation.duration);
        k.scrub = Some(
            (k.scrub.unwrap_or(0.0) + if forward { 0.005 } else { -0.005 }).clamp(0.0, length),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::stage::{ApplyOutcome, StageIntent};
    #[test]
    fn send_is_one_undoable_track_and_replace_keeps_its_identity() {
        let mut stage = Stage::default();
        let before = stage.song.clone();
        let anchor = stage.song.tracks.first().map(|t| t.id);
        let id = stage
            .send_kiln_material("/tmp/membrane-send-a.wav".into(), anchor, false)
            .unwrap();
        assert_eq!(stage.song.tracks.len(), before.tracks.len() + 1);
        let index = stage.song.tracks.iter().position(|t| t.id == id).unwrap();
        assert_eq!(index, 1);
        assert_eq!(stage.song.tracks[index].lane, crate::lane::Lane::Drum);
        assert!(stage.song.slot_clip(index, 0).is_some());
        let machine = stage.song.tracks[index].machine.as_ref().unwrap().id;
        let replacement = stage
            .send_kiln_material("/tmp/membrane-send-b.wav".into(), Some(id), true)
            .unwrap();
        assert_eq!(replacement, id);
        assert_eq!(
            stage.song.tracks[index].machine.as_ref().unwrap().id,
            machine
        );
        assert_eq!(stage.song.tracks.len(), before.tracks.len() + 1);
        assert_eq!(stage.apply(StageIntent::Undo), ApplyOutcome::Changed);
        assert_eq!(
            stage.song.tracks[index]
                .machine
                .as_ref()
                .unwrap()
                .sample
                .as_deref(),
            Some(std::path::Path::new("/tmp/membrane-send-a.wav"))
        );
        let _ = stage.apply(StageIntent::Undo);
        assert_eq!(stage.song, before);
    }
    #[test]
    fn missing_replace_target_cannot_create_a_track() {
        let mut stage = Stage::default();
        let before = stage.song.clone();
        assert!(
            stage
                .send_kiln_material("/tmp/kiln.wav".into(), Some(TrackId(u64::MAX)), true)
                .is_err()
        );
        assert_eq!(stage.song, before);
    }
    #[test]
    fn editing_cancels_the_old_generation_before_debounce() {
        let mut stage = Stage::default();
        let _ = stage.apply(StageIntent::Lab);
        stage.kiln_request(Action::Print).unwrap();
        let _ = stage.apply(StageIntent::Step(Step::Up));
        let k = stage.lab.focused_kiln_mut().unwrap();
        assert!(k.key.is_none());
        assert!(k.changed.is_some());
        assert!(k.submitted.is_none());
        assert!(k.printed.is_none());
    }
}
