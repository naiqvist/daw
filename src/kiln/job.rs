//! One persistent worker per kiln window, one replaceable queued request.
//! Cancellation is generation stamped: stale completions never reach the UI.
use super::{
    Patch, files,
    membrane::{Membrane, Render},
};
use crate::sequencing::TrackId;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Preview,
    Hear,
    Print,
    Send,
    Replace,
}
#[derive(Clone, Debug)]
pub struct Request {
    pub patch: Patch,
    pub rate: u32,
    pub action: Action,
    pub target: Option<TrackId>,
}
#[derive(Clone, Debug)]
pub struct Completed {
    pub key: u64,
    pub request: Request,
    pub result: Result<(Render, Option<std::path::PathBuf>), String>,
}
#[derive(Default)]
struct Shared {
    pending: Mutex<Option<(u64, Request)>>,
    wake: Condvar,
    generation: AtomicU64,
    stop: AtomicBool,
    progress: Mutex<f32>,
    done: Mutex<Option<Completed>>,
}
pub struct Job {
    shared: Arc<Shared>,
}
impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KilnJob").finish_non_exhaustive()
    }
}
impl PartialEq for Job {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }
}
impl Job {
    pub fn new() -> Result<Self, String> {
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("kiln-membrane".into())
            .spawn(move || {
                let engine = Membrane::default();
                loop {
                    let mut pending = worker.pending.lock().unwrap_or_else(|e| e.into_inner());
                    while pending.is_none() && !worker.stop.load(Ordering::Relaxed) {
                        pending = worker.wake.wait(pending).unwrap_or_else(|e| e.into_inner());
                    }
                    if worker.stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let Some((generation, request)) = pending.take() else {
                        continue;
                    };
                    drop(pending);
                    let valid = || {
                        !worker.stop.load(Ordering::Relaxed)
                            && worker.generation.load(Ordering::Acquire) == generation
                    };
                    let key = request.patch.key(60, 100, request.rate);
                    let full = matches!(
                        request.action,
                        Action::Print | Action::Send | Action::Replace
                    );
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let render =
                            engine.render(&request.patch, 60, 100, request.rate, full, &mut |f| {
                                if let Ok(mut p) = worker.progress.lock() {
                                    *p = f;
                                }
                                valid()
                            });
                        let Some(render) = render else {
                            return Err("render cancelled or model left its finite range".into());
                        };
                        if !valid() {
                            return Err("render cancelled".into());
                        }
                        let path = if full {
                            Some(files::print(
                                &render,
                                &request.patch,
                                60,
                                100,
                                &files::directory(),
                            )?)
                        } else {
                            None
                        };
                        Ok((render, path))
                    }))
                    .unwrap_or_else(|_| Err("kiln worker failed while rendering".into()));
                    if let Ok(mut done) = worker.done.lock() {
                        if valid() {
                            *done = Some(Completed {
                                key,
                                request,
                                result,
                            });
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { shared })
    }
    pub fn submit(&self, request: Request) {
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let generation = self.shared.generation.fetch_add(1, Ordering::AcqRel) + 1;
        *pending = Some((generation, request));
        if let Ok(mut done) = self.shared.done.lock() {
            *done = None;
        }
        if let Ok(mut progress) = self.shared.progress.lock() {
            *progress = 0.;
        }
        self.shared.wake.notify_one();
    }
    pub fn cancel(&self) {
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut p) = self.shared.pending.lock() {
            *p = None;
        }
        if let Ok(mut d) = self.shared.done.lock() {
            *d = None;
        }
    }
    pub fn progress(&self) -> f32 {
        self.shared.progress.lock().map_or(0., |p| *p)
    }
    pub fn take(&self) -> Option<Completed> {
        self.shared.done.lock().ok()?.take()
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.cancel();
        self.shared.wake.notify_one();
    }
}
