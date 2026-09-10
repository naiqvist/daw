//! The material hero's green-zone data and undoable tools.
use super::{RefusalReason, Stage, Touch};
use crate::devices::DeviceKind;
use crate::pages::{Hero, HeroHandle, HeroHeight, WaveHero};
use crate::params::sampler as p;
use crate::sequencing::{
    Device, DeviceId, Note, PATTERN_STEP_TICKS, PATTERN_STEPS, Pattern, PatternId,
};

impl Stage {
    pub(super) fn hero_set_handle(
        &mut self,
        param: u32,
        fraction: f32,
    ) -> Result<(), RefusalReason> {
        let id = self.deck_sample_device().ok_or(RefusalReason::Empty)?;
        let d = self.hero_patch(self.song.device(id).ok_or(RefusalReason::Empty)?);
        let (a, b) = sample_region(&d);
        let value = if param == p::LOOP_START {
            (f64::from(fraction) - a) / (b - a).max(1e-9)
        } else if param == p::LOOP_SIZE {
            let position = a + (b - a) * f64::from(d.value(p::LOOP_START));
            (f64::from(fraction) - position) / (b - a).max(1e-9)
        } else {
            let mut base = d.clone();
            base.set(p::START, 0.0);
            base.set(p::END, 1.0);
            let (lo, hi) = sample_region(&base);
            (f64::from(fraction) - lo) / (hi - lo).max(1e-9)
        };
        self.hero_edit(&[(param, value.clamp(0.0, 1.0) as f32)])
    }
    pub(super) fn finish_hero_gesture(&mut self) {
        self.settle();
    }
    pub(super) fn deck_sample_device(&self) -> Option<DeviceId> {
        let track = self.deck_track()?;
        let d = self.song.tracks.get(track)?.machine.as_ref()?;
        (d.kind == DeviceKind::Sampler).then_some(d.id)
    }
    /// The track's machine, whatever kind it is. The tall panel, its
    /// tools and their edits belong to the MACHINE — the sampler was
    /// only the first one to want them.
    pub(super) fn deck_hero_device(&self) -> Option<DeviceId> {
        let track = self.deck_track()?;
        Some(self.song.tracks.get(track)?.machine.as_ref()?.id)
    }
    pub(super) fn deck_hero_height(&self) -> HeroHeight {
        let Some((_, page)) = self.selected_page() else {
            return HeroHeight::Band;
        };
        let Some(id) = self.deck_hero_device() else {
            return HeroHeight::Band;
        };
        self.song.device(id).map_or(HeroHeight::Band, |d| {
            crate::pages::hero_height(d.kind, page.title)
        })
    }
    pub(super) fn deck_hero_tools(&self) -> &'static [crate::pages::HeroTool] {
        if !self.deck.open {
            return &[];
        }
        let Some((_, page)) = self.selected_page() else {
            return &[];
        };
        let Some(id) = self.deck_hero_device() else {
            return &[];
        };
        self.song
            .device(id)
            .map_or(&[], |d| crate::pages::hero_tools(d.kind, page.title))
    }
    fn sample_params(d: &Device) -> crate::audio::sampler::SamplerParams {
        let mut params = crate::audio::sampler::SamplerParams::default();
        for (id, value) in &d.overrides {
            params.set(*id, *value);
        }
        params
    }
    fn hero_patch(&self, device: &Device) -> Device {
        let mut patch = device.clone();
        if self.addressing_steps()
            && let Some(opened) = self.inside
            && let Some(step) = self.addressed_step_numbers().first()
            && let Some(pattern) = self.song.pattern(opened.pattern)
        {
            for def in p::TABLE {
                if let Some(value) = pattern.trig(*step).lock_on(None, def.id) {
                    patch.set(def.id, value);
                }
            }
        }
        patch
    }
    pub(super) fn sampler_hero(
        &self,
        d: &Device,
        page: &str,
        selected: Option<u32>,
    ) -> Option<Hero> {
        let resolved = self.hero_patch(d);
        let d = &resolved;
        let params = Self::sample_params(d);
        let mut hero = crate::audio::sampler::hero(&params, page, selected)?;
        if crate::pages::hero_height(d.kind, page) != HeroHeight::Tall {
            return Some(hero);
        }
        let Some(data) = self
            .sample_data
            .as_ref()
            .filter(|data| Some(&data.path) == d.sample.as_ref())
        else {
            hero.title = "B · LOAD A SAMPLE".into();
            return Some(hero);
        };
        if data.frames == 0 {
            hero.title = "SAMPLE COULD NOT BE READ".into();
            return Some(hero);
        }
        let (a, b) = sample_region(d);
        let frames = data.frames as f64;
        let (loop_a, loop_b) = params.loop_region(
            a * frames,
            b * frames,
            data.sample_rate as f32,
            params.root as u8,
            self.bpm() / 60.0 / f64::from(data.sample_rate),
            0.0,
            0.0,
        );
        let loop_span = (loop_a / frames, loop_b / frames);
        let target = if matches!(page, "Loop" | "Loop detail") {
            loop_span
        } else {
            (a, b)
        };
        let (from, to) = if self.deck.sample_zoom {
            let pad = (target.1 - target.0) * 0.15;
            ((target.0 - pad).max(0.0), (target.1 + pad).min(1.0))
        } else {
            (0.0, 1.0)
        };
        let mut handles = Vec::new();
        if matches!(page, "Loop" | "Loop detail") {
            handles.push(HeroHandle {
                param: p::LOOP_START,
                at: loop_span.0 as f32,
                value: params.loop_start,
            });
            handles.push(HeroHandle {
                param: p::LOOP_SIZE,
                at: loop_span.1 as f32,
                value: params.loop_size,
            });
        } else {
            handles.push(HeroHandle {
                param: p::START,
                at: a as f32,
                value: params.start,
            });
            handles.push(HeroHandle {
                param: p::END,
                at: b as f32,
                value: params.end,
            });
        }
        hero.title = format!(
            "{} · {:.2} s · {} CH",
            data.path.file_name().unwrap_or_default().to_string_lossy(),
            data.peaks.seconds(),
            data.channels
        );
        hero.series.clear();
        hero.marks.clear();
        hero.waveform = Some(WaveHero {
            columns: data.peaks.columns(Some(&data.samples), from, to, 1024),
            overview: data.peaks.columns(None, 0.0, 1.0, 384),
            view: (from as f32, to as f32),
            region: (a as f32, b as f32),
            loop_region: (params.loop_mode > 0.0)
                .then_some((loop_span.0 as f32, loop_span.1 as f32)),
            fade: if params.hard >= 0.5 {
                0.0
            } else {
                (params.loop_fade
                    + params.loop_xfade_ms * data.sample_rate as f32 * 0.001
                        / (loop_b - loop_a).max(1.0) as f32)
                    .min(0.5)
            },
            slices: d.slices.iter().map(|s| *s as f32).collect(),
            handles,
            tools: self
                .deck_hero_tools()
                .iter()
                .map(|t| format!("{} {}", t.key.name(), t.word))
                .collect(),
            playheads: self
                .readout(d.id)
                .filter(|r| r.bands[2] > 0.0)
                .map_or_else(Vec::new, |r| {
                    let mut heads = vec![(r.bands[0], false)];
                    if params.slip >= 0.5 {
                        heads.push((r.bands[1], true));
                    }
                    heads
                }),
            detail: if page == "Time" {
                format!(
                    "{} · TIME {:.0}% · PITCH {:+.0} st · {}",
                    p::PLAYBACK_NAMES
                        .get(params.playback as usize)
                        .copied()
                        .unwrap_or("REPITCH"),
                    params.time,
                    params.tune,
                    if params.speed == 0.0 {
                        "HOLD"
                    } else if params.speed < 0.0 {
                        "BACKWARD"
                    } else {
                        "FORWARD"
                    }
                )
            } else {
                format!(
                    "SLICE {:02} · LOOP {:.1} ms · FADE {:.0}%",
                    params.slice as usize,
                    (loop_b - loop_a) / f64::from(data.sample_rate) * 1000.0,
                    params.loop_fade * 100.0
                )
            },
        });
        Some(hero)
    }

    pub(super) fn hero_edit(&mut self, edits: &[(u32, f32)]) -> Result<(), RefusalReason> {
        let id = self.deck_hero_device().ok_or(RefusalReason::Unavailable)?;
        let spec = self
            .song
            .device(id)
            .ok_or(RefusalReason::Empty)?
            .kind
            .spec();
        let steps = self.addressed_step_numbers();
        if self.addressing_steps()
            && !steps.is_empty()
            && let Some(opened) = self.inside
        {
            let intents: Vec<_> = steps
                .iter()
                .flat_map(|step| {
                    edits.iter().filter_map(move |(param, value)| {
                        crate::params::clamp(spec.params, *param, *value).map(|value| {
                            crate::ui::sequencer::sequence::Intent::SetLock {
                                tick: step * PATTERN_STEP_TICKS,
                                device: None,
                                param: *param,
                                value,
                            }
                        })
                    })
                })
                .collect();
            self.apply_sequence(opened.pattern, &intents);
            if let Some(keys) = &mut self.steps {
                keys.used |= keys.held;
            }
        } else {
            let device = self.song.device_mut(id).ok_or(RefusalReason::Empty)?;
            for (param, value) in edits {
                device.set(*param, *value);
            }
            self.remixed();
        }
        if let Some((param, value)) = edits.last()
            && let Some(label) = spec.labels.get(*param as usize)
        {
            self.touch = Some(Touch {
                device: spec.prefix,
                name: label.name,
                value: format!("{value:.3}"),
            });
        }
        Ok(())
    }

    /// The tall panel's LIST, when the machine on this page browses a
    /// library. ROM's bank is the only one so far.
    pub(super) fn deck_hero_list(&self) -> Option<crate::pages::ListHero> {
        if !self.deck.open {
            return None;
        }
        let (track, page) = self.selected_page()?;
        let machine = self.song.tracks.get(track)?.machine.as_ref()?;
        if machine.kind != DeviceKind::Rom {
            return None;
        }
        let mut params = crate::audio::rom::RomParams::default();
        for def in crate::params::rom::TABLE {
            let value = self.deck_machine_value(track, def.id)?;
            params.set(def.id, value);
        }
        crate::audio::rom::pcm_list(&params, page.title)
    }

    /// Everything in this page's tall panel the cursor can stand on, in
    /// the order Tab walks them. Derived from the panel itself: a list
    /// offers its rows, a waveform offers its brackets.
    pub(super) fn deck_hero_targets(&self) -> Vec<crate::pages::HeroTarget> {
        use crate::pages::HeroTarget;
        if self.deck_hero_height() != HeroHeight::Tall {
            return Vec::new();
        }
        if let Some(list) = self.deck_hero_list() {
            return vec![HeroTarget::Rows { param: list.param }];
        }
        self.deck_hero()
            .and_then(|hero| hero.waveform)
            .map(|wave| {
                wave.handles
                    .iter()
                    .map(|handle| HeroTarget::Handle {
                        param: handle.param,
                        at: handle.at,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The target the keys are on, if they are in the panel at all.
    pub(super) fn deck_hero_focus(&self) -> Option<crate::pages::HeroTarget> {
        let at = self.deck.hero_focus?;
        self.deck_hero_targets().get(at).copied()
    }

    /// Tab: take the keys into the panel, then on to its next target,
    /// then back out to the cell strip. Shift walks the other way.
    pub(super) fn hero_focus(&mut self, back: bool) -> Result<(), RefusalReason> {
        let targets = self.deck_hero_targets();
        if targets.is_empty() {
            return Err(RefusalReason::Unavailable);
        }
        self.deck.hero_focus = match (self.deck.hero_focus, back) {
            (None, false) => Some(0),
            (None, true) => Some(targets.len() - 1),
            (Some(at), false) => (at + 1 < targets.len()).then_some(at + 1),
            (Some(at), true) => at.checked_sub(1),
        };
        Ok(())
    }

    /// Escape, and every gesture that changes what the panel is showing:
    /// the keys go back to the cells.
    pub(super) fn leave_hero_focus(&mut self) -> bool {
        self.deck.hero_focus.take().is_some()
    }

    /// The arrows, while the panel has the keys. They follow the
    /// TARGET's own axis: a list is walked up and down and its groups
    /// sideways; a bracket slides along the waveform it lives on, finely
    /// with the horizontal arrows and in bigger steps with the vertical.
    pub(super) fn hero_focus_arrow(
        &mut self,
        step: super::Step,
        coarse: bool,
    ) -> Result<(), RefusalReason> {
        use super::Step;
        let target = self.deck_hero_focus().ok_or(RefusalReason::Unavailable)?;
        match target {
            crate::pages::HeroTarget::Rows { param } => {
                let list = self.deck_hero_list().ok_or(RefusalReason::Unavailable)?;
                let last = list.rows.len().saturating_sub(1);
                let to = match step {
                    Step::Up if coarse => list.group_jump(false),
                    Step::Down if coarse => list.group_jump(true),
                    Step::Up => list.selected.saturating_sub(1),
                    Step::Down => (list.selected + 1).min(last),
                    Step::Left => list.group_jump(false),
                    Step::Right => list.group_jump(true),
                };
                if to == list.selected {
                    return Err(RefusalReason::Edge(step));
                }
                self.hero_edit(&[(param, to as f32)])?;
                self.settle();
                Ok(())
            }
            crate::pages::HeroTarget::Handle { param, at } => {
                let grain = match step {
                    Step::Up | Step::Down => 0.05,
                    Step::Left | Step::Right => 0.005,
                } * if coarse { 4.0 } else { 1.0 };
                let to = match step {
                    Step::Up | Step::Right => at + grain,
                    Step::Down | Step::Left => at - grain,
                };
                if (to.clamp(0.0, 1.0) - at).abs() < f32::EPSILON {
                    return Err(RefusalReason::Edge(step));
                }
                self.hero_set_handle(param, to.clamp(0.0, 1.0))?;
                self.settle();
                Ok(())
            }
        }
    }

    /// Pick a row of the list with the pointer: the same parameter edit
    /// the PCM cell makes, so it undoes once and locks on a held step.
    pub(super) fn hero_pick_row(&mut self, row: usize) -> Result<(), RefusalReason> {
        let list = self.deck_hero_list().ok_or(RefusalReason::Unavailable)?;
        if row >= list.rows.len() {
            return Err(RefusalReason::Unavailable);
        }
        if row == list.selected {
            return Ok(());
        }
        self.hero_edit(&[(list.param, row as f32)])?;
        self.settle();
        Ok(())
    }

    /// ROM's tools. The keys are the deck's; the meanings are ROM's.
    /// Everything but HEAR is a parameter edit, so it undoes once and
    /// lands as a lock when steps are held.
    fn rom_hero_tool(&mut self, verb: u8) -> Result<(), RefusalReason> {
        let (track, page) = self.selected_page().ok_or(RefusalReason::Unavailable)?;
        let page = page.title;
        if !crate::pages::hero_tools(DeviceKind::Rom, page)
            .iter()
            .any(|tool| tool.verb == verb)
        {
            return Err(RefusalReason::Unavailable);
        }
        use crate::params::rom as rp;
        let second = page == "Osc 2";
        let (mine, other, key) = if second {
            (rp::PCM2, rp::PCM1, rp::KEY2)
        } else {
            (rp::PCM1, rp::PCM2, rp::KEY1)
        };
        let at = self
            .deck_machine_value(track, mine)
            .unwrap_or(0.0)
            .round()
            .max(0.0) as usize;
        match verb {
            // The category jump: what the cut CAT cell was for, as a key.
            1 | 2 => {
                let list = self.deck_hero_list().ok_or(RefusalReason::Unavailable)?;
                let to = list.group_jump(verb == 2);
                self.hero_edit(&[(mine, to as f32)])?;
                self.settle();
                Ok(())
            }
            // Layer what is under the cursor onto the other oscillator.
            6 => {
                self.hero_edit(&[(other, at as f32)])?;
                self.settle();
                let name = crate::audio::rom::bank::MULTIS
                    .get(at)
                    .map_or("", |multi| multi.name);
                self.notice = Some(format!("{name} on osc {}", if second { 1 } else { 2 }));
                Ok(())
            }
            // Hear the PCM itself, from the cache, with nothing else in
            // the path: the one tool that is not an edit.
            7 => {
                let Some(path) = crate::audio::rom::audition_file(at, 48_000) else {
                    // The graph bakes the bank when a ROM track compiles,
                    // and the example bakes it ahead of a session. A key
                    // press must not render one.
                    self.notice =
                        Some("no baked PCM yet — play a note, or bake the bank".to_owned());
                    return Ok(());
                };
                self.audition = Some(super::Audition::of(&path, 0.0, 1.0));
                let name = crate::audio::rom::bank::MULTIS
                    .get(at)
                    .map_or("", |multi| multi.name);
                self.notice = Some(format!("hear {name}"));
                Ok(())
            }
            // Pin the sample to its own root, or let it track again.
            8 => {
                let tracking = self.deck_machine_value(track, key).unwrap_or(1.0) > 0.5;
                self.hero_edit(&[(key, if tracking { 0.0 } else { 1.0 })])?;
                self.settle();
                self.notice = Some(
                    if tracking {
                        "fixed pitch"
                    } else {
                        "key tracking"
                    }
                    .to_owned(),
                );
                Ok(())
            }
            _ => Err(RefusalReason::Unavailable),
        }
    }

    pub(super) fn hero_tool(&mut self, verb: u8) -> Result<(), RefusalReason> {
        // A machine with its own tools answers first: the sampler's
        // PRINT must not fire on someone else's key.
        if self
            .deck_hero_device()
            .and_then(|id| self.song.device(id))
            .map(|device| device.kind)
            == Some(DeviceKind::Rom)
        {
            return self.rom_hero_tool(verb);
        }
        if verb == 11 && !self.deck_hero_tools().is_empty() {
            return self.request_sampler_print();
        }
        if !self.deck_hero_tools().iter().any(|t| t.verb == verb) {
            return Err(RefusalReason::Unavailable);
        }
        let id = self
            .deck_sample_device()
            .ok_or(RefusalReason::Unavailable)?;
        let device = self.song.device(id).ok_or(RefusalReason::Empty)?.clone();
        if verb == 0 {
            self.deck.sample_zoom = !self.deck.sample_zoom;
            return Ok(());
        }
        if verb == 10 {
            let _ = self.apply(super::StageIntent::Browse);
            return Ok(());
        }
        if verb == 8 {
            self.deck.sample_profile = (self.deck.sample_profile + 1) % 4;
            let common = [
                (p::START, 0.0),
                (p::END, 1.0),
                (p::ENV_POSITION, 0.0),
                (p::ENV_SIZE, 0.0),
                (p::SCAN, 0.0),
                (p::SPEED, 100.0),
                (p::TIME, 100.0),
                (p::COMB_MIX, 0.0),
            ];
            self.hero_edit(&common)?;
            let edits: &[(u32, f32)] = match self.deck.sample_profile {
                1 => &[
                    (p::MODE, 0.0),
                    (p::LOOP_MODE, 1.0),
                    (p::LOOP_START, 0.4),
                    (p::LOOP_SIZE, 0.12),
                    (p::LOOP_FADE, 0.45),
                    (p::PLAYBACK, 2.0),
                    (p::SPEED, 0.0),
                    (p::AMP_A, 1200.0),
                    (p::AMP_S, 1.0),
                    (p::AMP_R, 5000.0),
                    (p::CUTOFF, 4000.0),
                    (p::CHOKE, 0.0),
                ],
                2 => &[
                    (p::MODE, 0.0),
                    (p::LOOP_MODE, 1.0),
                    (p::LOOP_START, 0.2),
                    (p::LOOP_SIZE, 0.06),
                    (p::LOOP_FADE, 0.4),
                    (p::PLAYBACK, 3.0),
                    (p::TIME, 800.0),
                    (p::SCAN, 0.1),
                    (p::TRAVEL, 0.6),
                    (p::AMP_A, 500.0),
                    (p::AMP_S, 1.0),
                    (p::AMP_R, 6000.0),
                    (p::CUTOFF, 2500.0),
                    (p::CHOKE, 0.0),
                ],
                3 => &[
                    (p::MODE, 0.0),
                    (p::LOOP_MODE, 1.0),
                    (p::LOOP_SIZE, 0.001),
                    (p::LOOP_FADE, 0.0),
                    (p::PLAYBACK, 0.0),
                    (p::AMP_A, 2.0),
                    (p::AMP_S, 1.0),
                    (p::AMP_R, 300.0),
                    (p::CUTOFF, 8000.0),
                    (p::CHOKE, 0.0),
                ],
                _ => &[
                    (p::MODE, 2.0),
                    (p::LOOP_MODE, 0.0),
                    (p::PLAYBACK, 0.0),
                    (p::AMP_A, 1.0),
                    (p::AMP_S, 1.0),
                    (p::AMP_R, 20.0),
                    (p::CUTOFF, 20000.0),
                    (p::CHOKE, 1.0),
                ],
            };
            self.hero_edit(edits)?;
            self.notice =
                Some(["CHOP", "SUSTAIN", "SCAN", "TONE"][self.deck.sample_profile as usize].into());
            return Ok(());
        }
        if verb == 9 {
            let seed = device.value(p::SEED) as u32;
            let next = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let position = (next % 1000) as f32 / 1000.0;
            self.hero_edit(&[
                (p::SEED, (next % 65536) as f32),
                (p::LOOP_START, position),
                (p::SCAN, position * 0.4 - 0.2),
                (p::LOOP_SIZE, 0.02 + position * 0.15),
            ])?;
            self.notice = Some("texture varied · pitch and rhythm kept".into());
            return Ok(());
        }
        let data = self
            .sample_data
            .as_ref()
            .filter(|data| Some(&data.path) == device.sample.as_ref())
            .ok_or(RefusalReason::Empty)?;
        if data.frames == 0 {
            return Err(RefusalReason::Empty);
        }
        let frames = data.frames as f64;
        let mut slices = device.slices.clone();
        let selected = (device.value(p::SLICE).round() as usize).saturating_sub(1);
        match verb {
            1 => {
                slices = crate::slice::grid(data.frames, device.value(p::SLICES) as usize)
                    .iter()
                    .map(|f| *f as f64 / frames)
                    .collect()
            }
            2 => {
                let gap = f64::from(device.value(p::MIN_GAP)) * f64::from(data.sample_rate) * 0.001;
                let mut previous = 0.0;
                slices = crate::slice::transients_of_spaced(
                    &data.planar(),
                    device.value(p::SENSE),
                    device.value(p::MIN_GAP),
                )
                .into_iter()
                .filter(|f| {
                    let keep = *f == 0 || *f as f64 - previous >= gap;
                    if keep {
                        previous = *f as f64;
                    }
                    keep
                })
                .map(|f| f as f64 / frames)
                .take(64)
                .collect();
            }
            3 => {
                if slices.is_empty() {
                    slices.push(0.0);
                }
                if slices.len() >= 64 {
                    return Err(RefusalReason::Unavailable);
                }
                let a = slices.get(selected).copied().unwrap_or(0.0);
                let b = slices.get(selected + 1).copied().unwrap_or(1.0);
                slices.push(data.snapped((a + b) * 0.5));
            }
            4 => {
                if selected + 1 >= slices.len() {
                    return Err(RefusalReason::Unavailable);
                }
                slices.remove(selected + 1);
            }
            5 | 6 => {
                let (a, b) = sample_region(&device);
                let source = &data.samples;
                let start = (a * frames) as usize;
                let end = (b * frames) as usize;
                if end.saturating_sub(start) < 4 {
                    return Err(RefusalReason::Unavailable);
                }
                let (lo, hi) = if verb == 6 {
                    let mut first = None;
                    let mut pair = None;
                    let minimum = (data.sample_rate / 2000).max(2) as usize;
                    for i in start.saturating_add(1)..end.min(start + data.sample_rate as usize) {
                        if source.get(i - 1).copied().unwrap_or(0.0) <= 0.0
                            && source.get(i).copied().unwrap_or(0.0) > 0.0
                        {
                            if let Some(f) = first {
                                if i - f >= minimum {
                                    pair = Some((f, i));
                                    break;
                                }
                            } else {
                                first = Some(i);
                            }
                        }
                    }
                    pair.ok_or(RefusalReason::Unavailable)?
                } else {
                    let size = ((end - start) / 10)
                        .max(2)
                        .min((data.sample_rate as usize / 4).max(2));
                    let mut best = (f64::MAX, start);
                    for n in 1..64 {
                        let at = start + (end - start - size) * n / 64;
                        let mut score = 0.0f64;
                        for j in 0..32 {
                            let x = source.get(at + j).copied().unwrap_or(0.0);
                            let y = source
                                .get(at + size - 32.min(size) + j)
                                .copied()
                                .unwrap_or(0.0);
                            score += f64::from((x - y).powi(2));
                        }
                        if score < best.0 {
                            best = (score, at);
                        }
                    }
                    (best.1, best.1 + size)
                };
                let size = (hi - lo) as f64 / ((b - a) * frames).max(1.0);
                let pos = (lo as f64 - a * frames) / ((b - a) * frames).max(1.0);
                let mut edits = vec![
                    (p::LOOP_MODE, 1.0),
                    (p::LOOP_START, pos as f32),
                    (p::LOOP_SIZE, size as f32),
                    (p::LOOP_FADE, if verb == 6 { 0.0 } else { 0.4 }),
                ];
                if verb == 6 {
                    let hz = f64::from(data.sample_rate) / (hi - lo) as f64;
                    let midi = 69.0 + 12.0 * (hz / 440.0).log2();
                    edits.push((p::ROOT, midi.round().clamp(0.0, 127.0) as f32));
                    edits.push((p::FINE, ((midi.round() - midi) * 100.0) as f32));
                }
                self.hero_edit(&edits)?;
                self.deck.sample_zoom = true;
                self.notice = Some(
                    if verb == 6 {
                        "cycle found"
                    } else {
                        "sustain loop found"
                    }
                    .into(),
                );
                return Ok(());
            }
            7 => {
                let track = self.deck_track().ok_or(RefusalReason::Empty)?;
                if slices.is_empty() {
                    return Err(RefusalReason::Empty);
                }
                let beats = if device.value(p::SOURCE_BEATS) > 0.0 {
                    f64::from(device.value(p::SOURCE_BEATS))
                } else {
                    data.peaks.seconds() * self.bpm() / 60.0
                };
                let ticks = (beats * crate::sequencing::TICKS_PER_BEAT as f64)
                    .round()
                    .max(1.0) as usize;
                if ticks > PATTERN_STEPS * PATTERN_STEP_TICKS {
                    self.notice = Some("trim to 16 beats before making a pattern".into());
                    return Err(RefusalReason::Unavailable);
                }
                let mut pattern = Pattern::empty(PatternId(0), "sliced break".into());
                pattern.length_ticks = ticks;
                for (i, at) in slices.iter().enumerate() {
                    let tick = (*at * ticks as f64).round() as usize;
                    let step = (tick / PATTERN_STEP_TICKS).min(PATTERN_STEPS - 1);
                    let next = slices.get(i + 1).copied().unwrap_or(1.0);
                    if pattern.trig(step).enabled {
                        return Err(RefusalReason::Unavailable);
                    }
                    let mut note = Note::new(
                        device.value(p::ROOT) as u8,
                        ((next - at) * ticks as f64).round().max(1.0) as usize,
                        110,
                    );
                    note.micro_ticks = (tick - step * PATTERN_STEP_TICKS) as i16;
                    pattern.set_primary(step, note);
                    pattern.trig_mut(step).set_lock(p::SLICE, (i + 1) as f32);
                }
                let start = self.song.tracks.get(track).map_or(0, |t| {
                    t.blocks
                        .iter()
                        .map(|b| b.start_tick + b.length_ticks)
                        .max()
                        .unwrap_or(0)
                });
                self.song
                    .adopt_pattern(pattern, track, start, ticks)
                    .ok_or(RefusalReason::Unavailable)?;
                self.hero_edit(&[(p::MODE, 2.0), (p::SOURCE_BEATS, beats as f32)])?;
                self.touched();
                self.notice = Some("slice pattern added to arrangement".into());
                return Ok(());
            }
            _ => return Err(RefusalReason::Unavailable),
        }
        let d = self.song.device_mut(id).ok_or(RefusalReason::Empty)?;
        d.set_slices(slices);
        let count = d.slices.len();
        d.set(p::MODE, 2.0);
        d.set(p::SLICE_SOURCE, 2.0);
        d.set(p::SLICES, count as f32);
        self.touched();
        self.remixed();
        self.notice = Some(format!("{count} slices"));
        Ok(())
    }
}

fn sample_region(d: &Device) -> (f64, f64) {
    let (a, b) = if d.value(p::MODE).round() == 2.0 && !d.slices.is_empty() {
        let i = (d.value(p::SLICE).round() as usize).saturating_sub(1);
        (
            d.slices.get(i).copied().unwrap_or(0.0),
            if d.value(p::SLICE_THRU) >= 0.5 {
                1.0
            } else {
                d.slices.get(i + 1).copied().unwrap_or(1.0)
            },
        )
    } else {
        (0.0, 1.0)
    };
    (
        a + (b - a) * f64::from(d.value(p::START)),
        a + (b - a) * f64::from(d.value(p::END)),
    )
}

impl Stage {
    fn request_sampler_print(&mut self) -> Result<(), RefusalReason> {
        let track = self.deck_track().ok_or(RefusalReason::Empty)?;
        let id = self.deck_sample_device().ok_or(RefusalReason::Empty)?;
        let d = self.song.device(id).ok_or(RefusalReason::Empty)?;
        if d.sample.is_none() {
            return Err(RefusalReason::Empty);
        }
        let mut source = self.song.clone();
        for (index, lane) in source.tracks.iter_mut().enumerate() {
            lane.solo = false;
            lane.muted = index != track;
            lane.blocks.clear();
        }
        let mut phrase = self
            .inside
            .and_then(|i| self.song.pattern(i.pattern))
            .cloned()
            .unwrap_or_else(|| {
                let mut phrase = Pattern::empty(PatternId(0), "printed phrase".into());
                phrase.length_ticks = 8 * crate::sequencing::TICKS_PER_BEAT;
                phrase.set_primary(
                    0,
                    Note::new(d.value(p::ROOT) as u8, phrase.length_ticks, 110),
                );
                phrase
            });
        phrase.id = PatternId(0);
        let ticks = phrase.length_ticks;
        source
            .adopt_pattern(phrase, track, 0, ticks)
            .ok_or(RefusalReason::Unavailable)?;
        let folder = self
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(std::path::Path::to_path_buf)
            .or_else(|| self.home.clone())
            .unwrap_or_else(std::env::temp_dir);
        let path = folder.join("renders").join(format!(
            "slice-{}-{}.wav",
            id.0,
            super::arrangement::stamp()
        ));
        self.request_export_to(
            0,
            ticks,
            path,
            crate::ui::prefs::ExportFormat::Float32,
            None,
            2,
        )?;
        if let Some(request) = &mut self.export_request {
            request.source = Some(Box::new(source));
        }
        if let Some(export) = &mut self.export {
            export.import_sample = true;
        }
        self.notice = Some("PRINT · rendering this phrase and its effects".into());
        Ok(())
    }

    pub(super) fn import_sampler_print(&mut self, path: std::path::PathBuf) {
        self.settle();
        let track = self.song.tracks.len();
        self.song
            .add_track(crate::sequencing::TrackKind::Instrument);
        self.song.tracks[track].name = "SLICE print".into();
        self.song.tracks[track].muted = true;
        self.fit_session();
        if let Some(id) = self.song.add_device(track, DeviceKind::Sampler)
            && let Some(d) = self.song.device_mut(id)
        {
            d.sample = Some(path);
            for (key, value) in [
                (p::DRIVE, 0.0),
                (p::PREAMP, 0.0),
                (p::BITS, 16.0),
                (p::RATE, 48000.0),
                (p::CUTOFF, 20000.0),
                (p::GAIN, 0.0),
                (p::AMP_A, 0.1),
                (p::AMP_S, 1.0),
            ] {
                d.set(key, value);
            }
        }
        self.touched();
        self.remixed();
        self.settle();
        self.notice = Some(format!("PRINT · sample ready on muted track {}", track + 1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::PageKey;
    use crate::ui::stage::{SampleData, StageIntent, Step};
    fn setup() -> Stage {
        let mut stage = Stage::new();
        let id = stage.song.add_device(0, DeviceKind::Sampler).unwrap();
        stage.song.device_mut(id).unwrap().sample = Some("tools.wav".into());
        let samples = (0..48000)
            .map(|i| (i as f32 * core::f32::consts::TAU / 120.0).sin() * 0.5)
            .collect();
        stage.set_sample(SampleData::from_planar(
            "tools.wav".into(),
            std::sync::Arc::new(samples),
            1,
            48000,
            48000,
        ));
        let _ = stage.apply(StageIntent::Page(PageKey::Src));
        stage
    }
    /// THE KEYBOARD REACHES EVERY TARGET THE POINTER DOES. Tab walks
    /// into the panel, through its controls, and back out to the cells;
    /// every target it stands on moves with the arrows.
    #[test]
    fn tab_walks_every_panel_target_and_the_arrows_move_it() {
        for (name, mut stage) in [("rom", rom_stage(0)), ("slice", setup())] {
            let targets = stage.deck_hero_targets();
            assert!(
                !targets.is_empty(),
                "{name} has a tall panel with no target"
            );
            assert!(stage.deck.hero_focus.is_none(), "{name} started inside");
            for (at, target) in targets.iter().enumerate() {
                assert_eq!(
                    stage.apply(StageIntent::HeroFocus { back: false }),
                    super::super::ApplyOutcome::Changed
                );
                assert_eq!(stage.deck_hero_focus(), Some(*target), "{name} target {at}");
                // Every target answers an arrow along its own axis.
                let before = stage.song.clone();
                let moved = [Step::Down, Step::Up, Step::Left, Step::Right]
                    .into_iter()
                    .any(|step| {
                        let _ = stage.hero_focus_arrow(step, false);
                        stage.song != before
                    });
                assert!(moved, "{name} target {at} does not answer the arrows");
            }
            // One more Tab hands the keys back to the cell strip.
            let _ = stage.apply(StageIntent::HeroFocus { back: false });
            assert!(stage.deck.hero_focus.is_none(), "{name} never let go");
        }
    }

    #[test]
    fn the_cursor_walks_the_list_by_row_and_its_groups_sideways() {
        use crate::params::rom as rp;
        let mut stage = rom_stage(0);
        let _ = stage.apply(StageIntent::HeroFocus { back: false });
        let row = |stage: &Stage| rom_value(stage, rp::PCM1);

        // Down and up are the list's own axis: one row at a time.
        let _ = stage.hero_focus_arrow(Step::Down, false);
        assert_eq!(row(&stage), 1.0);
        let _ = stage.hero_focus_arrow(Step::Down, false);
        assert_eq!(row(&stage), 2.0);
        let _ = stage.hero_focus_arrow(Step::Up, false);
        assert_eq!(row(&stage), 1.0);
        // Sideways is by group, and so is a coarse turn.
        let _ = stage.hero_focus_arrow(Step::Right, false);
        assert_eq!(
            row(&stage),
            next_group(1),
            "Right did not jump to the next group"
        );
        let _ = stage.hero_focus_arrow(Step::Up, true);
        assert_eq!(row(&stage), 0.0, "a coarse turn is a group");
        // The top of the list is an edge, not a wrap.
        assert!(stage.hero_focus_arrow(Step::Up, false).is_err());
        assert_eq!(row(&stage), 0.0);
    }

    /// Escape hands the keys back before it closes the window, and a
    /// digit takes them to the cell it selects.
    #[test]
    fn the_keys_come_back_to_the_strip_by_escape_or_by_a_digit() {
        let mut stage = rom_stage(0);
        let _ = stage.apply(StageIntent::HeroFocus { back: false });
        assert!(stage.deck_hero_focus().is_some());
        let _ = stage.apply(StageIntent::Escape);
        assert!(stage.deck_hero_focus().is_none(), "escape left the panel");
        assert!(stage.deck.open, "escape closed the window too early");
        let _ = stage.apply(StageIntent::Escape);
        assert!(!stage.deck.open, "a second escape closes the window");

        let mut stage = rom_stage(0);
        let _ = stage.apply(StageIntent::HeroFocus { back: false });
        let _ = stage.apply(StageIntent::Slot(3));
        assert!(stage.deck_hero_focus().is_none());
        assert_eq!(stage.deck.slot, 3);
    }

    /// With the keys on the strip, the arrows are the strip's: the
    /// panel takes nothing it was not given.
    #[test]
    fn the_cell_strip_keeps_the_arrows_until_the_panel_is_asked_for() {
        use crate::params::rom as rp;
        let mut stage = rom_stage(0);
        let _ = stage.apply(StageIntent::Slot(1));
        let before = rom_value(&stage, rp::TUNE1);
        let _ = stage.apply(StageIntent::Turn {
            up: true,
            coarse: false,
        });
        assert!(
            rom_value(&stage, rp::TUNE1) > before,
            "the cell's own turn was stolen"
        );
        assert_eq!(rom_value(&stage, rp::PCM1), 0.0, "the list moved instead");
        // A page with no tall panel cannot be entered at all.
        let mut band = rom_stage(2);
        assert!(band.hero_focus(false).is_err());
        assert!(band.deck_hero_focus().is_none());
    }

    /// ROM on the deck: the bank list, and tools whose keys are the
    /// sampler's while their meanings are ROM's.
    fn rom_stage(page_taps: usize) -> Stage {
        let mut stage = Stage::new();
        let _ = stage.song.add_device(0, DeviceKind::Rom).unwrap();
        let _ = stage.apply(StageIntent::Page(PageKey::Src));
        for _ in 0..page_taps {
            let _ = stage.apply(StageIntent::Page(PageKey::Src));
        }
        stage
    }

    /// A multisample by NAME, and the first row of the group after the
    /// one a row sits in: what the category tools are for, asked of the
    /// bank rather than written into the test as an index.
    fn multi(name: &str) -> f32 {
        crate::audio::rom::bank::MULTIS
            .iter()
            .position(|multi| multi.name == name)
            .unwrap_or(0) as f32
    }

    fn next_group(from: usize) -> f32 {
        let multis = crate::audio::rom::bank::MULTIS;
        let group = multis.get(from).map_or("", |multi| multi.category);
        let mut groups: Vec<&str> = Vec::new();
        for multi in multis {
            if !groups.contains(&multi.category) {
                groups.push(multi.category);
            }
        }
        let here = groups.iter().position(|name| *name == group).unwrap_or(0);
        let wanted = groups[(here + 1) % groups.len()];
        multis
            .iter()
            .position(|multi| multi.category == wanted)
            .unwrap_or(0) as f32
    }

    fn rom_value(stage: &Stage, param: u32) -> f32 {
        stage.song.tracks[0]
            .machine
            .as_ref()
            .map_or(0.0, |machine| machine.value(param))
    }

    #[test]
    fn roms_oscillator_pages_are_tall_and_carry_the_bank() {
        let stage = rom_stage(0);
        assert_eq!(stage.deck_hero_height(), HeroHeight::Tall);
        let list = stage.deck_hero_list().expect("the bank");
        assert_eq!(list.rows.len(), crate::audio::rom::bank::MULTIS.len());
        assert_eq!(list.selected, 0);
        assert_eq!(list.param, crate::params::rom::PCM1);
        // Both oscillators start on the first row, and the margin says so.
        assert_eq!(list.rows[0].tags, "12");
        let words: Vec<&str> = stage.deck_hero_tools().iter().map(|t| t.word).collect();
        assert_eq!(words, ["CAT <", "CAT >", "TO OSC 2", "HEAR", "FIXED"]);
        // The second page addresses the second oscillator.
        let second = rom_stage(1);
        assert_eq!(
            second.deck_hero_list().map(|list| list.param),
            Some(crate::params::rom::PCM2)
        );
        assert!(
            second
                .deck_hero_tools()
                .iter()
                .any(|tool| tool.word == "TO OSC 1")
        );
        // The Layer page is an ordinary band picture with no list.
        let layer = rom_stage(2);
        assert_eq!(layer.deck_hero_height(), HeroHeight::Band);
        assert!(layer.deck_hero_list().is_none());
        assert!(layer.deck_hero_tools().is_empty());
    }

    #[test]
    fn the_category_tools_jump_by_group_wrap_and_undo_once() {
        use crate::params::rom as rp;
        let mut stage = rom_stage(0);
        let before = stage.song.clone();
        let first = rom_value(&stage, rp::PCM1) as usize;
        let _ = stage.apply(StageIntent::HeroTool(2));
        assert_eq!(
            rom_value(&stage, rp::PCM1),
            next_group(first),
            "CAT > did not leave the group"
        );
        let _ = stage.apply(StageIntent::HeroTool(1));
        assert_eq!(
            rom_value(&stage, rp::PCM1),
            first as f32,
            "CAT < did not come back"
        );
        // From the first group, CAT < wraps to the last one.
        let _ = stage.apply(StageIntent::HeroTool(1));
        let landed = rom_value(&stage, rp::PCM1) as usize;
        let last = crate::audio::rom::bank::MULTIS
            .last()
            .map_or("", |multi| multi.category);
        assert_eq!(
            crate::audio::rom::bank::MULTIS[landed].category,
            last,
            "CAT < did not wrap to the last group"
        );
        for _ in 0..3 {
            let _ = stage.apply(StageIntent::Undo);
        }
        assert_eq!(stage.song, before, "each jump is one undo step");
    }

    #[test]
    fn the_layer_tool_sends_the_row_across_and_fixed_pins_the_pitch() {
        use crate::params::rom as rp;
        let mut stage = rom_stage(0);
        let id = stage.song.tracks[0].machine.as_ref().unwrap().id;
        let tine = multi("Tine");
        stage.song.device_mut(id).unwrap().set(rp::PCM1, tine);
        let _ = stage.apply(StageIntent::HeroTool(6));
        assert_eq!(rom_value(&stage, rp::PCM2), tine, "the row did not cross");
        assert_eq!(rom_value(&stage, rp::PCM1), tine, "the source moved");
        assert_eq!(stage.notice.as_deref(), Some("Tine on osc 2"));
        // The list now marks the row for both oscillators.
        let list = stage.deck_hero_list().expect("the bank");
        assert_eq!(list.rows[tine as usize].tags, "12");

        let _ = stage.apply(StageIntent::HeroTool(8));
        assert_eq!(rom_value(&stage, rp::KEY1), 0.0, "FIXED did not pin");
        let _ = stage.apply(StageIntent::HeroTool(8));
        assert_eq!(rom_value(&stage, rp::KEY1), 1.0, "FIXED did not release");
    }

    /// HEAR plays the PCM straight from the cache and never renders one:
    /// with a baked bank it points at the file, without one it says so.
    #[test]
    fn hear_reads_the_cache_and_writes_nothing() {
        let mut stage = rom_stage(0);
        let _ = stage.apply(StageIntent::HeroTool(7));
        match stage.audition.as_ref() {
            Some(audition) => assert!(
                audition.path.exists(),
                "the audition points at a file that is not there"
            ),
            None => assert!(
                stage
                    .notice
                    .as_deref()
                    .is_some_and(|notice| notice.contains("baked")),
                "no audition and no word about why"
            ),
        }
    }

    /// A row is the PCM cell: picking one is the same edit, and the
    /// sampler's own tools stay out of it.
    #[test]
    fn picking_a_row_moves_the_cell_and_undoes_once() {
        use crate::params::rom as rp;
        let mut stage = rom_stage(0);
        let before = stage.song.clone();
        let _ = stage.hero_pick_row(4);
        assert_eq!(rom_value(&stage, rp::PCM1), 4.0);
        let _ = stage.apply(StageIntent::Undo);
        assert_eq!(stage.song, before, "a pick is one undo step");
        // A row that is not there is refused, and the row already under
        // the cursor is not an edit at all.
        assert!(stage.hero_pick_row(99).is_err());
        assert!(stage.hero_pick_row(0).is_ok());
        assert_eq!(stage.song, before);
    }

    /// The sampler's PRINT lives on the same key as nothing of ROM's:
    /// pressing it on a ROM track must refuse, not print a sampler.
    #[test]
    fn a_sampler_tool_does_not_fire_on_a_rom_track() {
        let mut stage = rom_stage(0);
        let before = stage.song.clone();
        assert!(stage.hero_tool(11).is_err());
        assert!(stage.hero_tool(0).is_err());
        assert_eq!(stage.song, before);
    }

    #[test]
    fn authored_slices_profiles_and_new_parameters_survive_roundtrip() {
        let mut stage = setup();
        let _ = stage.apply(StageIntent::HeroTool(1));
        let d = stage.song.tracks[0].machine.as_ref().unwrap();
        assert_eq!(d.slices.len(), 16);
        let before = stage.song.clone();
        let _ = stage.apply(StageIntent::HeroTool(3));
        assert_eq!(
            stage.song.tracks[0].machine.as_ref().unwrap().slices.len(),
            17
        );
        let _ = stage.apply(StageIntent::Undo);
        assert_eq!(stage.song, before);
        let _ = stage.apply(StageIntent::HeroTool(8));
        let encoded = ron::to_string(&stage.song).unwrap();
        let decoded: crate::sequencing::Song = ron::from_str(&encoded).unwrap();
        assert_eq!(decoded, stage.song);
    }
    #[test]
    fn print_is_an_isolated_snapshot_and_import_is_one_undo_step() {
        let mut stage = setup();
        let before = stage.song.clone();
        let _ = stage.apply(StageIntent::HeroTool(11));
        let request = stage.take_export().unwrap();
        let source = request.source.unwrap();
        assert!(source.tracks[0].blocks.len() == 1);
        assert_eq!(stage.song, before);
        stage.export_taken();
        stage.export_finished(Ok(()));
        assert_eq!(stage.song.tracks.len(), before.tracks.len() + 1);
        assert!(stage.song.tracks.last().unwrap().muted);
        let _ = stage.apply(StageIntent::Undo);
        assert_eq!(stage.song, before);
    }
}
