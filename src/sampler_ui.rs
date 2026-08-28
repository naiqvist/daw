//! The sampler's own surfaces: loading a file into one, moving its
//! slices, and the full-size overlay a card opens into.
//!
//! Apart from the rack because a sampler is the one device whose editing
//! needs more room than a card has — and because its faces are a CACHE of
//! what a file looks like, derived from the document, never saved with
//! it, and rebuilt whenever the file behind it changes.

use super::*;

impl App {
    /// Walk every track's meter toward this block's peak.
    ///
    /// The engine reports one peak per block; the frame rate and the block
    /// rate are unrelated, so the ballistics are advanced by the FRAME's
    /// elapsed time — a meter is a thing the eye reads, and it should fall
    /// at the same speed however the buffer size is set.
    ///
    /// With the engine off there is nothing to report, and the meters are
    /// walked toward silence rather than frozen at their last reading.
    /// Rebuild the sampler pictures the rack is about to draw.
    ///
    /// A picture is rebuilt only when its PATH changes, because building
    /// one decodes a file and scans every sample of it. Everything else a
    /// sampler's card shows — the region, the loop, the markers — comes
    /// from parameters and is free.
    ///
    /// Faces for devices that no longer exist are dropped here rather
    /// than on delete: a device can leave through half a dozen doors and
    /// only one of them would be remembered.
    /// Point a sampler at a file.
    ///
    /// A RECOMPILE, not a letter: letters carry an `f32` and a sample is
    /// megabytes, so the new material arrives the way every other
    /// compiled allocation does — through a schedule swap.
    ///
    /// The slice table is cleared with it. Markers cut from one file mean
    /// nothing in another, and carrying them over would leave a chopped
    /// break sliced at the old file's transients — which sounds like a
    /// bug and reads like one.
    pub(crate) fn load_sample_into(&mut self, track: usize, instance: u64, path: PathBuf) {
        let Some(track) = self.arrangement.tracks.get_mut(track) else {
            return;
        };
        if !track
            .chain
            .iter()
            .any(|d| d.id == instance && d.kind() == DeviceKind::Sampler)
        {
            return;
        }
        let source = track.sampler_sources.entry(instance).or_default();
        if source.path == path {
            return;
        }
        source.path = path;
        source.slices.clear();
        self.arrangement.force_recompile = true;
    }

    /// Move one slice marker, keeping the table sorted.
    ///
    /// Sorted HERE, green side, for the reason a clip's envelope is: the
    /// callback walks the table assuming it rises, and sorting it there
    /// would be both an allocation and an unbounded path.
    pub(crate) fn move_slice(&mut self, track: usize, instance: u64, index: usize, frame: u64) {
        let Some(track) = self.arrangement.tracks.get_mut(track) else {
            return;
        };
        let Some(source) = track.sampler_sources.get_mut(&instance) else {
            return;
        };
        let Some(slot) = source.slices.get_mut(index) else {
            return;
        };
        *slot = frame;
        source.slices.sort_unstable();
        source.slices.dedup();
        // LATCH the table. The grid is re-derived from the count on every
        // frame it is in force, so without this the drag would be undone
        // before it was drawn — the marker would spring back and the
        // gesture would look broken when it was in fact working.
        if let Some(device) = track.chain.iter_mut().find(|d| d.id == instance)
            && let DeviceState::Sampler(params) = &mut device.state
            && params.slice_source.round() == daw::params::sampler::SLICE_GRID
        {
            params.slice_source = daw::params::sampler::SLICE_CUSTOM;
        }
        self.arrangement.force_recompile = true;
    }

    /// Cut the file up again, from the knobs.
    ///
    /// A grid is arithmetic and instant. Detection is a scan of the whole
    /// file, which is why it happens HERE — on an explicit ask — rather
    /// than every time the slice count moves.
    pub(crate) fn rebuild_slices(&mut self, track: usize, instance: u64) {
        let rate = self
            .engine
            .as_ref()
            .map(|engine| engine.info().sample_rate)
            .unwrap_or(48_000);
        let Some(lane) = self.arrangement.tracks.get(track) else {
            return;
        };
        let Some(device) = lane.chain.iter().find(|d| d.id == instance) else {
            return;
        };
        let DeviceState::Sampler(params) = device.state else {
            return;
        };
        let Some(path) = lane
            .sampler_sources
            .get(&instance)
            .map(|s| s.path.clone())
            .filter(|p| !p.as_os_str().is_empty())
        else {
            return;
        };
        let Ok(material) = daw::audio::material::load_cached(&path, rate) else {
            return;
        };
        let slices = match params.slice_source.round() {
            s if s == daw::params::sampler::SLICE_TRANSIENTS => {
                daw::slice::transients(&material, 0.5)
            }
            // A table the user has edited is not re-cut on a whim. Asking
            // for a grid again is a matter of turning the cell back to
            // `grid`, which is one click and is undoable.
            s if s == daw::params::sampler::SLICE_CUSTOM => return,
            _ => daw::slice::grid(material.frames, params.slices.round().max(1.0) as usize),
        };
        if let Some(source) = self
            .arrangement
            .tracks
            .get_mut(track)
            .and_then(|t| t.sampler_sources.get_mut(&instance))
        {
            source.slices = slices;
        }
        self.arrangement.force_recompile = true;
    }

    /// The sampler's display at full size, over everything else.
    ///
    /// An OVERLAY rather than a fourth bottom-panel face: what makes this
    /// worth having is room, and a region that shares the window with the
    /// arrangement has exactly as little of it as the rack did.
    ///
    /// Everything it returns goes down the same roads a card's edits do —
    /// this is the same plot given space, not a second editor with a
    /// second set of gestures to learn.
    pub(crate) fn expanded_sampler_overlay(&mut self, ctx: &egui::Context) {
        let Some((track, instance)) = self.expanded_sampler else {
            return;
        };
        // The device may have been deleted, bypassed away, or the track
        // removed while this was open. Close rather than draw a ghost.
        let Some(DeviceState::Sampler(params)) = self
            .arrangement
            .tracks
            .get(track)
            .and_then(|t| t.chain.iter().find(|d| d.id == instance))
            .map(|d| d.state)
        else {
            self.expanded_sampler = None;
            return;
        };
        let (page, zoom, scroll) = self
            .arrangement
            .tracks
            .get(track)
            .and_then(|t| t.chain.iter().find(|d| d.id == instance))
            .map_or((0, 1.0, 0.0), |d| (d.page, d.view_zoom, d.view_scroll));

        let theme = self.theme.clone();
        let face = self
            .sampler_faces
            .get(&instance)
            .cloned()
            .unwrap_or_default();
        let slices = self
            .sampler_slices
            .get(&instance)
            .cloned()
            .unwrap_or_default();
        let mut knobs = device::SamplerUi::from_engine(|id| params.get(id).unwrap_or_default());

        let screen = ctx.content_rect();
        let frame = egui::Frame::new()
            .fill(theme.surface_sunken)
            .inner_margin(egui::Margin::same(theme.sp(space::SM) as i8));
        let mut out = None;
        egui::Area::new(egui::Id::new("sampler_expanded"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.set_width(screen.width());
                ui.set_height(screen.height());
                frame.show(ui, |ui| {
                    ui.set_height(ui.available_height());
                    let view = device::SamplerView {
                        name: &face.name,
                        wave: &face.wave,
                        frames: face.frames,
                        slices: &slices,
                        voices: &[],
                        truncated: face.truncated,
                        original_rate: face.original_rate,
                    };
                    out = Some(device::sampler_expanded(
                        ui, &theme, &mut knobs, page, zoom, scroll, &view,
                    ));
                });
            });

        let Some(out) = out else { return };
        if out.collapse || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.expanded_sampler = None;
        }
        if let Some(device) = self
            .arrangement
            .tracks
            .get_mut(track)
            .and_then(|t| t.chain.iter_mut().find(|d| d.id == instance))
        {
            device.page = out.page;
            device.view_zoom = out.zoom;
            device.view_scroll = out.scroll;
        }
        if !out.edits.is_empty() {
            self.apply_device_edits(ChainOwner::Track(track), instance, &out.edits);
        }
        if let Some((index, frame)) = out.slice_moved {
            self.move_slice(track, instance, index, frame);
        }
        if out.reslice {
            self.rebuild_slices(track, instance);
        }
    }

    pub(crate) fn refresh_sampler_faces(&mut self) {
        let rate = self
            .engine
            .as_ref()
            .map(|engine| engine.info().sample_rate)
            .unwrap_or(48_000);
        // Collected first: the loop below decodes, and it cannot hold a
        // borrow of the arrangement while it does.
        let wanted: Vec<(usize, u64, PathBuf)> = self
            .arrangement
            .tracks
            .iter()
            .enumerate()
            .flat_map(|(t, track)| {
                track
                    .chain
                    .iter()
                    .filter(|device| device.kind() == DeviceKind::Sampler)
                    .map(move |device| {
                        (
                            t,
                            device.id,
                            track
                                .sampler_sources
                                .get(&device.id)
                                .map(|s| s.path.clone())
                                .unwrap_or_default(),
                        )
                    })
            })
            .collect();

        let live: HashSet<u64> = wanted.iter().map(|(_, id, _)| *id).collect();
        self.sampler_faces.retain(|id, _| live.contains(id));

        // --- 1. the pictures, rebuilt only when a PATH changes ---------
        for (_, id, path) in &wanted {
            if self
                .sampler_faces
                .get(id)
                .is_some_and(|face| face.path == *path)
            {
                continue;
            }
            if path.as_os_str().is_empty() {
                self.sampler_faces.insert(*id, SamplerFace::default());
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let face = match daw::audio::material::load_cached(path, rate) {
                Ok(material) => SamplerFace {
                    name,
                    wave: sampler_wave(&material),
                    frames: material.frames,
                    truncated: material.truncated,
                    original_rate: material.original_rate,
                    path: path.clone(),
                },
                // An unreadable file gets a face with its NAME and no
                // picture, rather than no face at all: "this file will
                // not open" is a thing the card should be able to say.
                Err(_) => SamplerFace {
                    name,
                    path: path.clone(),
                    ..SamplerFace::default()
                },
            };
            self.sampler_faces.insert(*id, face);
        }

        // --- 2. the GRID, authored HERE and not left to the engine -----
        //
        // `Node::Sampler`'s compile falls back to a grid when the table is
        // empty, which keeps an old project playing — but a fallback the
        // document never sees is a fallback the CARD cannot draw, and that
        // is exactly why the markers were invisible. So the document owns
        // the table: while the source is `grid` it is re-authored from the
        // count, which is a handful of integer divisions and makes the
        // knob RE-CUT the file as you turn it.
        //
        // Detected onsets are deliberately NOT re-authored. They cost a
        // scan of the whole file, and they are ordinary editable data once
        // they exist — walking over them here would throw away every
        // marker the user had dragged.
        //
        // After the pictures, because the grid is computed from the frame
        // count a picture just established.
        for (t, id, _) in &wanted {
            let Some(device) = self
                .arrangement
                .tracks
                .get(*t)
                .and_then(|track| track.chain.iter().find(|d| d.id == *id))
            else {
                continue;
            };
            let DeviceState::Sampler(params) = device.state else {
                continue;
            };
            if params.slice_source.round() != daw::params::sampler::SLICE_GRID {
                continue;
            }
            let frames = self.sampler_faces.get(id).map_or(0, |face| face.frames);
            if frames == 0 {
                continue;
            }
            let grid = daw::slice::grid(frames, params.slices.round().max(1.0) as usize);
            if let Some(source) = self
                .arrangement
                .tracks
                .get_mut(*t)
                .and_then(|track| track.sampler_sources.get_mut(id))
                && source.slices != grid
            {
                source.slices = grid;
                self.arrangement.force_recompile = true;
            }
        }

        // --- 3. the flat copy the rack draws from ---------------------
        //
        // Last, so it carries the grid authored above rather than the
        // table as it was before this frame started.
        self.sampler_slices.clear();
        for (t, id, _) in &wanted {
            let slices = self
                .arrangement
                .tracks
                .get(*t)
                .and_then(|track| track.sampler_sources.get(id))
                .map(|source| source.slices.clone())
                .unwrap_or_default();
            self.sampler_slices.insert(*id, slices);
        }
    }
}
