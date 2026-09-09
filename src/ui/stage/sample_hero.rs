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
    pub(super) fn deck_hero_height(&self) -> HeroHeight {
        let Some((_, page)) = self.selected_page() else {
            return HeroHeight::Band;
        };
        let Some(id) = self.deck_sample_device() else {
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
        let Some(id) = self.deck_sample_device() else {
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
        let id = self
            .deck_sample_device()
            .ok_or(RefusalReason::Unavailable)?;
        let steps = self.addressed_step_numbers();
        if self.addressing_steps()
            && !steps.is_empty()
            && let Some(opened) = self.inside
        {
            let intents: Vec<_> = steps
                .iter()
                .flat_map(|step| {
                    edits.iter().filter_map(move |(param, value)| {
                        crate::params::clamp(p::TABLE, *param, *value).map(|value| {
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
        if let Some((param, value)) = edits.last() {
            let spec = DeviceKind::Sampler.spec();
            if let Some(label) = spec.labels.get(*param as usize) {
                self.touch = Some(Touch {
                    device: spec.prefix,
                    name: label.name,
                    value: format!("{value:.3}"),
                });
            }
        }
        Ok(())
    }

    pub(super) fn hero_tool(&mut self, verb: u8) -> Result<(), RefusalReason> {
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
    use crate::ui::stage::{SampleData, StageIntent};
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
