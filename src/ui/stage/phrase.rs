//! Musical phrase operations, available through the ordinary `:` palette.
//! Green-side edits of ordinary notes and locks; no driver-only shortcuts,
//! new playback state, or hidden presets. Each sentence is one undo step.

use super::Stage;
use crate::sequencing::{Note, PATTERN_STEP_TICKS, Pattern};

const LIMIT: usize = 1024;

pub(super) const COMMANDS: &[crate::ui::palette::TypedCommand] = &[
    crate::ui::palette::TypedCommand {
        name: "length",
        usage: "length <ticks> · resize clip and its full-length placements; refuses cropping or collisions",
    },
    crate::ui::palette::TypedCommand {
        name: "voice",
        usage: "voice <intervals>|<next intervals> · cycle stacks at selected onsets from their lowest notes",
    },
    crate::ui::palette::TypedCommand {
        name: "rhythm",
        usage: "rhythm <division> <offsets> [at N] [every N] [gate N] [vel values] [pitch semitones] · offsets start at 0",
    },
    crate::ui::palette::TypedCommand {
        name: "group",
        usage: "group <division> <lengths> [at N] [every N] [vel values] [pitch semitones]",
    },
    crate::ui::palette::TypedCommand {
        name: "ratchet",
        usage: "ratchet <division> <start:end> [spacing A:B] [gate N] [vel A:B] [pitch A:B] · end excluded",
    },
    crate::ui::palette::TypedCommand {
        name: "shape",
        usage: "shape vel|pitch|gate <value,cycle,or A:B> · selection, else whole clip; gate in ticks",
    },
    crate::ui::palette::TypedCommand {
        name: "legato",
        usage: "legato [overlap in ticks] · length to next onset, capped at clip end",
    },
    crate::ui::palette::TypedCommand {
        name: "sweep",
        usage: "sweep <parameter> <start:end> [curve] [at tick:tick] · end excluded; underscores in names; units allowed",
    },
    crate::ui::palette::TypedCommand {
        name: "lock",
        usage: "lock <parameter> <value or choice> [at tick,tick,...] · selected time, else clip; 12-tick cells; one undo",
    },
];

#[derive(Clone)]
enum Contour {
    Cycle(Vec<f64>),
    Ramp(f64, f64),
}

fn number(s: &str) -> Result<f64, String> {
    s.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("invalid number: {s}"))
}

fn integer(s: &str) -> Result<usize, String> {
    s.parse::<usize>()
        .ok()
        .filter(|n| *n <= LIMIT * 192)
        .ok_or_else(|| format!("invalid count: {s}"))
}

fn list(s: &str) -> Result<Vec<usize>, String> {
    let values: Vec<_> = s.split(',').map(integer).collect::<Result<_, _>>()?;
    if values.len() > LIMIT {
        return Err("too many positions".into());
    }
    Ok(values)
}

impl Contour {
    fn parse(s: &str, low: f64, high: f64) -> Result<Self, String> {
        let valid = |s| {
            let n = number(s)?;
            if !(low..=high).contains(&n) {
                return Err(format!("value must be {low}..{high}"));
            }
            Ok(n)
        };
        if let Some((a, b)) = s.split_once(':') {
            Ok(Self::Ramp(valid(a)?, valid(b)?))
        } else {
            let values: Vec<_> = s.split(',').map(valid).collect::<Result<_, String>>()?;
            if values.len() > LIMIT {
                return Err("contour too long".into());
            }
            Ok(Self::Cycle(values))
        }
    }

    fn value(&self, i: usize, count: usize) -> f64 {
        match self {
            Self::Cycle(values) => values[i % values.len()],
            Self::Ramp(a, b) => a + (b - a) * i as f64 / count.saturating_sub(1).max(1) as f64,
        }
    }
}

struct Options {
    at: usize,
    every: Option<usize>,
    gate: Option<usize>,
    velocity: Option<Contour>,
    pitch: Contour,
    spacing: (f64, f64),
}

impl Options {
    fn parse(words: &[&str], ratchet: bool) -> Result<Self, String> {
        if words.len() % 2 != 0 {
            return Err("an option needs a value".into());
        }
        let mut result = Self {
            at: 0,
            every: None,
            gate: None,
            velocity: None,
            pitch: Contour::Cycle(vec![0.0]),
            spacing: (1.0, 1.0),
        };
        let mut seen = std::collections::HashSet::new();
        for pair in words.chunks_exact(2) {
            if !seen.insert(pair[0]) {
                return Err(format!("duplicate option: {}", pair[0]));
            }
            match pair[0] {
                "at" if !ratchet => result.at = integer(pair[1])?,
                "every" if !ratchet => result.every = Some(integer(pair[1])?),
                "gate" => result.gate = Some(integer(pair[1])?),
                "vel" => result.velocity = Some(Contour::parse(pair[1], 1.0, 127.0)?),
                "pitch" => result.pitch = Contour::parse(pair[1], -60.0, 67.0)?,
                "spacing" if ratchet => {
                    let c = Contour::parse(pair[1], 1.0, LIMIT as f64)?;
                    result.spacing = (c.value(0, 2), c.value(1, 2));
                }
                _ => return Err(format!("unknown option: {}", pair[0])),
            }
        }
        if result.every == Some(0) || result.gate == Some(0) {
            return Err("period and gate must be positive".into());
        }
        Ok(result)
    }
}

fn note_positions(pattern: &Pattern, selection: Option<&[usize]>) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    for step in 0..pattern.step_count() {
        if selection.is_some_and(|selected| !selected.contains(&step)) {
            continue;
        }
        let trig = pattern.trig(step);
        if !trig.enabled {
            continue;
        }
        for (index, note) in trig.notes.iter().enumerate() {
            let tick = (step * PATTERN_STEP_TICKS).saturating_add_signed(note.micro_ticks as isize);
            if tick < pattern.length_ticks {
                out.push((tick, step, index));
            }
        }
    }
    out.sort_by_key(|n| n.0);
    out
}

fn generate(pattern: &mut Pattern, words: &[&str]) -> Result<usize, String> {
    if words.len() < 3 {
        return Err("give a division and rhythm".into());
    }
    let division = integer(words[1])?;
    if division == 0 || division > 192 || 192 % division != 0 {
        return Err("division must divide 192 (e.g. 8, 16, 32, 64, 12, 24, 48)".into());
    }
    let unit = 192 / division;
    let ratchet = words[0] == "ratchet";
    let options = Options::parse(&words[3..], ratchet)?;
    let mut placements = Vec::new();
    if ratchet {
        let (a, b) = words[2].split_once(':').ok_or("ratchet needs start:end")?;
        let (start, end) = (integer(a)?, integer(b)?);
        if start >= end || end * unit > pattern.length_ticks {
            return Err("ratchet range is outside the clip".into());
        }
        let mut at = start;
        while at < end {
            if placements.len() == LIMIT {
                return Err("too many generated notes".into());
            }
            let progress = (at - start) as f64 / (end - start) as f64;
            let spacing = (options.spacing.0 + (options.spacing.1 - options.spacing.0) * progress)
                .round()
                .max(1.0) as usize;
            placements.push((
                at * unit,
                options.gate.unwrap_or(spacing).min(end - at) * unit,
            ));
            at += spacing;
        }
    } else {
        let values = list(words[2])?;
        let group = words[0] == "group";
        let mut motif = Vec::new();
        let mut cursor = 0;
        for value in values {
            if group && value == 0 {
                return Err("group lengths must be positive".into());
            }
            motif.push((
                if group { cursor } else { value },
                options.gate.unwrap_or(if group { value } else { 1 }),
            ));
            cursor = cursor.checked_add(value).ok_or("rhythm too long")?;
        }
        if let Some(period) = options.every {
            if motif.iter().any(|(at, _)| *at >= period) || (group && cursor > period) {
                return Err("rhythm does not fit its repeat period".into());
            }
        }
        let mut base = options.at;
        loop {
            for &(offset, length) in &motif {
                let at = (base + offset) * unit;
                if at >= pattern.length_ticks {
                    if options.every.is_none() {
                        return Err("rhythm is outside the clip".into());
                    }
                    continue;
                }
                placements.push((at, length * unit));
                if placements.len() > LIMIT {
                    return Err("too many generated notes".into());
                }
            }
            let Some(period) = options.every else {
                break;
            };
            base += period;
            if base * unit >= pattern.length_ticks {
                break;
            }
        }
    }
    if placements.is_empty() {
        return Err("rhythm places no notes".into());
    }
    placements.sort_by_key(|n| n.0);
    if placements.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err("duplicate onset in rhythm".into());
    }
    let count = placements.len();
    // Preserve an existing note/chord as the ratchet's source. New bursts use
    // the ordinary middle-C entry default, so they work on any instrument.
    let source: Vec<Note> = if ratchet {
        let at = placements[0].0;
        pattern
            .trig(at / PATTERN_STEP_TICKS)
            .notes_at((at % PATTERN_STEP_TICKS) as i16)
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    if !source.is_empty() {
        let source_step = placements[0].0 / PATTERN_STEP_TICKS;
        let source_trig = pattern.trig(source_step).clone();
        let addressed: std::collections::BTreeSet<_> = placements.iter().map(|n| n.0).collect();
        let steps: std::collections::BTreeSet<_> =
            addressed.iter().map(|t| t / PATTERN_STEP_TICKS).collect();
        for &step in &steps {
            let target = pattern.trig(step);
            let outside = target.notes.iter().any(|note| {
                let tick =
                    (step * PATTERN_STEP_TICKS).saturating_add_signed(note.micro_ticks as isize);
                !addressed.contains(&tick)
            });
            if outside
                && (target.rules() != source_trig.rules()
                    || source_trig.retrig.is_some()
                    || target.sound != source_trig.sound
                    || target.probability != source_trig.probability)
            {
                return Err("ratchet would change another note's shared step rules; use a separate step or clip".into());
            }
        }
        for step in steps {
            let target = pattern.trig_mut(step);
            target.locks = source_trig.locks.clone();
            target.sound = source_trig.sound.clone();
            target.cond = source_trig.cond;
            target.probability = source_trig.probability;
            // The repetitions are now explicit notes, never double-retriggered.
            target.retrig = None;
        }
    }
    for (i, &(tick, length)) in placements.iter().enumerate() {
        let trig = pattern.trig_mut(tick / PATTERN_STEP_TICKS);
        let micro = (tick % PATTERN_STEP_TICKS) as i16;
        trig.notes.retain(|n| n.micro_ticks != micro);
        let templates = if source.is_empty() {
            vec![Note::new(60, length, 100)]
        } else {
            source.clone()
        };
        for mut note in templates {
            note.micro_ticks = micro;
            note.length_ticks = length.max(1);
            if let Some(velocity) = &options.velocity {
                note.velocity = velocity.value(i, count).round() as u8;
            }
            note.pitch = note
                .pitch
                .shifted_semitones(options.pitch.value(i, count).round() as isize);
            trig.add_tone_at(note);
        }
    }
    Ok(count)
}

fn transform(
    pattern: &mut Pattern,
    words: &[&str],
    selection: Option<&[usize]>,
    key: &crate::pitch::Key,
) -> Result<usize, String> {
    let positions = note_positions(pattern, selection);
    if positions.is_empty() {
        return Err("select notes or fill the clip first".into());
    }
    let count = positions.len();
    match words {
        ["voice", intervals] => {
            let stacks: Vec<Vec<isize>> = intervals
                .split('|')
                .map(|stack| {
                    let intervals: Vec<isize> = stack
                        .split(',')
                        .map(|value| {
                            value
                                .parse::<isize>()
                                .ok()
                                .filter(|n| (-60..=67).contains(n))
                                .ok_or("voice intervals must be integer semitones in -60..67")
                        })
                        .collect::<Result<_, _>>()?;
                    if intervals.is_empty() || intervals.len() > 16 {
                        return Err("voice needs 1..16 intervals per stack");
                    }
                    let mut unique = intervals.clone();
                    unique.sort_unstable();
                    unique.dedup();
                    if unique.len() != intervals.len() {
                        return Err("duplicate voicing interval");
                    }
                    Ok(intervals)
                })
                .collect::<Result<_, _>>()?;
            if stacks.len() > 64 {
                return Err("at most 64 voicing stacks".into());
            }
            let mut onsets: Vec<_> = positions.iter().map(|n| n.0).collect();
            onsets.dedup();
            for (ordinal, tick) in onsets.into_iter().enumerate() {
                let (step, micro) = (
                    tick / PATTERN_STEP_TICKS,
                    (tick % PATTERN_STEP_TICKS) as i16,
                );
                let trig = pattern.trig_mut(step);
                let source = trig
                    .notes_at(micro)
                    .min_by(|a, b| a.pitch.resolve(key).total_cmp(&b.pitch.resolve(key)))
                    .cloned()
                    .ok_or("voicing source is missing")?;
                trig.notes.retain(|note| note.micro_ticks != micro);
                for interval in &stacks[ordinal % stacks.len()] {
                    let mut note = source.clone();
                    note.pitch = note.pitch.shifted_semitones(*interval);
                    trig.add_tone_at(note);
                }
            }
        }
        ["legato"] | ["legato", _] => {
            let overlap = words.get(1).map(|s| integer(s)).transpose()?.unwrap_or(0);
            if overlap > 192 {
                return Err("overlap must be 0..192 ticks".into());
            }
            for &(tick, step, index) in &positions {
                let next = positions
                    .iter()
                    .find(|n| n.0 > tick)
                    .map_or(pattern.length_ticks, |n| n.0);
                pattern.trig_mut(step).notes[index].length_ticks =
                    next.saturating_add(overlap).min(pattern.length_ticks) - tick;
            }
        }
        ["shape", field @ ("vel" | "pitch" | "gate"), values] => {
            let (min, max) = match *field {
                "vel" => (1.0, 127.0),
                "pitch" => (-60.0, 67.0),
                _ => (1.0, pattern.length_ticks as f64),
            };
            let contour = Contour::parse(values, min, max)?;
            // A chord shares one contour position, rather than its tones
            // advancing the cycle independently.
            let mut onsets: Vec<_> = positions.iter().map(|n| n.0).collect();
            onsets.dedup();
            for &(tick, step, index) in &positions {
                let i = onsets.binary_search(&tick).map_err(|_| "missing onset")?;
                let value = contour.value(i, onsets.len()).round();
                let remaining = pattern.length_ticks - tick;
                let note = &mut pattern.trig_mut(step).notes[index];
                match *field {
                    "vel" => note.velocity = value as u8,
                    "pitch" => note.pitch = note.pitch.shifted_semitones(value as isize),
                    _ => note.length_ticks = (value as usize).min(remaining),
                }
            }
        }
        _ => {
            return Err(
                "use shape vel|pitch|gate <cycle or start:end>, or legato [overlap ticks]".into(),
            );
        }
    }
    Ok(count)
}

impl Stage {
    pub(super) fn apply_phrase_command(&mut self, input: &str) -> bool {
        let result = self.phrase_edit(input);
        match result {
            Ok(count) => {
                self.notice = Some(format!("phrase · {count} onsets edited"));
                true
            }
            Err(error) => {
                self.notice = Some(format!("REFUSED phrase · {error}"));
                false
            }
        }
    }

    fn phrase_edit(&mut self, input: &str) -> Result<usize, String> {
        if input.len() > 4096 {
            return Err("sentence too long".into());
        }
        let opened = self.inside.ok_or("open a clip first")?;
        if input.split_whitespace().next() == Some("length") {
            return self.set_phrase_length(input);
        }
        let before = self.song.pattern(opened.pattern).ok_or("clip is missing")?;
        let mut edited = before.clone();
        let mut enable_section = None;
        let selected = self
            .sequencer
            .standing_selection_steps(opened.pattern.0, PATTERN_STEP_TICKS);
        let words: Vec<_> = input.split_whitespace().collect();
        let count = match words.first().copied() {
            Some("rhythm" | "group" | "ratchet") => generate(&mut edited, &words)?,
            Some("shape" | "legato" | "voice") => {
                transform(&mut edited, &words, selected.as_deref(), &self.song.key)?
            }
            Some("sweep" | "lock") => {
                let sweep = words[0] == "sweep";
                if words.len() < 3 {
                    return Err("sweep <parameter> <start:end> [curve] [at tick:tick], or lock <parameter> <value> [at ticks]".into());
                }
                let address = if words[1].contains('.') {
                    words[1].to_owned()
                } else {
                    format!("machine.{}", words[1])
                };
                let target = super::parameter_command::resolve(&self.song, opened.track, &address)?;
                if target.track != opened.track {
                    return Err("locks must address this clip's track".into());
                }
                if sweep && !target.choices.is_empty() {
                    return Err("sweeps require a continuous parameter".into());
                }
                let (id, param) = super::parameter_command::device_param(&self.song, &target.id)
                    .ok_or("sweep needs an instrument or effect target")?;
                let device = self.song.device(id).ok_or("device missing")?;
                if device.kind == crate::devices::DeviceKind::Scomp
                    && crate::params::scomp::baked(param)
                {
                    return Err("this parameter requires baking, not live locks".into());
                }
                let lock_device = if self.song.tracks[opened.track]
                    .machine
                    .as_ref()
                    .is_some_and(|m| m.id == id)
                {
                    None
                } else {
                    Some(id)
                };
                let (a, b) = if sweep {
                    words[2].split_once(':').ok_or("sweep needs start:end")?
                } else {
                    (words[2], words[2])
                };
                let low = f64::from(super::parameter_command::evaluate(&target, a, "=")?);
                let high = f64::from(super::parameter_command::evaluate(&target, b, "=")?);
                if matches!(device.kind, crate::devices::DeviceKind::Console(kind) if !kind.always_in())
                    && device.bypassed
                {
                    enable_section = Some(id);
                }
                let mut cursor = 3;
                let curve = if sweep && words.get(cursor).is_some_and(|s| *s != "at") {
                    let value = number(words[cursor])?;
                    cursor += 1;
                    value
                } else {
                    1.0
                };
                if !(0.1..=10.0).contains(&curve) {
                    return Err("curve must be 0.1..10".into());
                }
                let steps = if cursor == words.len() {
                    selected.unwrap_or_else(|| {
                        (0..edited.length_ticks.div_ceil(PATTERN_STEP_TICKS)).collect()
                    })
                } else {
                    if words.get(cursor) != Some(&"at") || words.len() != cursor + 2 {
                        return Err("expected at <ticks>, no trailing words".into());
                    }
                    lock_steps(words[cursor + 1], edited.length_ticks, sweep)?
                };
                if sweep && steps.len() < 2 {
                    return Err("select at least two time cells".into());
                }
                let first = *steps.first().ok_or("empty selection")?;
                let last = *steps.last().ok_or("empty selection")?;
                for step in &steps {
                    let x = (*step - first) as f64 / (last - first).max(1) as f64;
                    let value = low + (high - low) * x.powf(curve);
                    let trig = edited.trig_mut(*step);
                    trig.set_lock_on(lock_device, param, value as f32);
                    trig.set_slide_on(lock_device, param, sweep && *step != last);
                }
                steps.len()
            }
            _ => return Err("unknown phrase operation".into()),
        };
        if &edited != before || enable_section.is_some() {
            *self
                .song
                .pattern_mut(opened.pattern)
                .ok_or("clip is missing")? = edited;
            if let Some(id) = enable_section
                && let Some(device) = self.song.device_mut(id)
            {
                device.bypassed = false;
            }
            self.touched();
            self.settle();
        }
        Ok(count)
    }

    fn set_phrase_length(&mut self, input: &str) -> Result<usize, String> {
        let words: Vec<_> = input.split_whitespace().collect();
        if words.len() != 2 {
            return Err("length needs one tick count".into());
        }
        let ticks = integer(words[1])?;
        if !(1..=49_152).contains(&ticks) {
            return Err("length must be 1..49152 ticks".into());
        }
        let opened = self.inside.ok_or("open a clip first")?;
        let pattern = self.song.pattern(opened.pattern).ok_or("clip missing")?;
        let old = pattern.length_ticks;
        if old == ticks {
            return Ok(0);
        }
        for step in 0..pattern.step_count() {
            let trig = pattern.trig(step);
            if step * PATTERN_STEP_TICKS >= ticks && !trig.locks.is_empty() {
                return Err("length would hide parameter locks".into());
            }
            for note in &trig.notes {
                let start =
                    (step * PATTERN_STEP_TICKS).saturating_add_signed(note.micro_ticks as isize);
                if start.saturating_add(note.length_ticks) > ticks {
                    return Err(
                        "length would crop notes; shorten or move them explicitly first".into(),
                    );
                }
            }
        }
        let placements: Vec<_> = self
            .song
            .tracks
            .iter()
            .flat_map(|t| &t.blocks)
            .filter(|b| b.pattern_id == opened.pattern)
            .cloned()
            .collect();
        if placements.iter().any(|b| b.length_ticks != old) {
            return Err("clip has custom-length placements; resize those explicitly first".into());
        }
        let mut candidate = self.song.clone();
        for block in &placements {
            if !candidate.resize_pattern_block(block.id, block.start_tick, ticks) {
                return Err("resized placement would overlap another block".into());
            }
        }
        candidate
            .pattern_mut(opened.pattern)
            .ok_or("clip missing")?
            .extend_timeline(ticks)?;
        self.song = candidate;
        self.touched();
        self.settle();
        Ok(placements.len())
    }
}

/// Explicit time addresses are ticks, aligned to the stored lock grid.
/// They override the standing selection. Refuse, never silently round.
fn lock_steps(text: &str, length: usize, sweep: bool) -> Result<Vec<usize>, String> {
    let mut ticks = if sweep {
        let (a, b) = text
            .split_once(':')
            .ok_or("sweep window needs start:end ticks")?;
        let a = integer(a)?;
        let b = integer(b)?;
        if a >= b || b > length || a % PATTERN_STEP_TICKS != 0 || b % PATTERN_STEP_TICKS != 0 {
            return Err("window must fit clip and align to 12-tick cells; end excluded".into());
        }
        (a..b).step_by(PATTERN_STEP_TICKS).collect::<Vec<_>>()
    } else {
        list(text)?
    };
    if ticks.is_empty()
        || ticks.len() > 4_096
        || ticks
            .iter()
            .any(|t| *t >= length || *t % PATTERN_STEP_TICKS != 0)
    {
        return Err("lock ticks must fit clip and align to 12-tick cells".into());
    }
    ticks.sort_unstable();
    ticks.dedup();
    Ok(ticks.into_iter().map(|t| t / PATTERN_STEP_TICKS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequencing::PatternId;

    #[test]
    fn explicit_length_updates_placements_atomically_and_refuses_cropping() {
        let mut stage = Stage::new();
        let id = stage.song.tracks[0].blocks[0].pattern_id;
        *stage.song.pattern_mut(id).unwrap() = Pattern::empty(id, "length test".into());
        stage.inside = Some(super::super::Opened {
            pattern: id,
            track: 0,
        });
        stage.settle();
        let before = stage.song.clone();
        assert!(stage.apply_phrase_command("length 672"));
        assert_eq!(stage.song.pattern(id).unwrap().length_ticks, 672);
        assert_eq!(stage.song.tracks[0].blocks[0].length_ticks, 672);
        let _ = stage.apply(super::super::StageIntent::Undo);
        assert_eq!(stage.song, before);
        assert!(stage.apply_phrase_command("rhythm 4 3 gate 1"));
        let before = stage.song.clone();
        assert!(!stage.apply_phrase_command("length 100"));
        assert_eq!(stage.song, before);
    }

    #[test]
    fn voicing_cycles_change_extensions_without_changing_rhythm() {
        let mut p = pattern();
        run(
            &mut p,
            "rhythm 4 0,4,8,12 gate 4 pitch -10,-17,-12,-19 vel 74",
        )
        .unwrap();
        transform(
            &mut p,
            &[
                "voice",
                "0,7,10,15,26|0,7,10,16,21|0,7,11,16,26|0,7,11,16,26",
            ],
            None,
        )
        .unwrap();
        for (at, expected) in [
            (0, vec![50, 57, 60, 65, 76]),
            (16, vec![43, 50, 53, 59, 64]),
            (32, vec![48, 55, 59, 64, 74]),
            (48, vec![41, 48, 52, 57, 67]),
        ] {
            assert_eq!(
                p.trig(at)
                    .notes
                    .iter()
                    .map(|n| crate::pitch::nearest_midi(
                        n.pitch.resolve(&crate::pitch::default_key())
                    ))
                    .collect::<Vec<_>>(),
                expected
            );
            assert!(
                p.trig(at)
                    .notes
                    .iter()
                    .all(|n| n.length_ticks == 192 && n.velocity == 74)
            );
        }
        let before = p.clone();
        assert!(transform(&mut p, &["voice", "0,7|0,0"], None).is_err());
        assert_eq!(p, before);
    }

    #[test]
    fn effect_sweep_is_instance_targeted_unit_aware_and_atomic() {
        let mut stage = Stage::new();
        let id = stage.song.tracks[0].blocks[0].pattern_id;
        stage.inside = Some(super::super::Opened {
            pattern: id,
            track: 0,
        });
        stage
            .song
            .add_device(0, crate::devices::DeviceKind::Table)
            .unwrap();
        stage.settle();
        let original = stage.song.clone();
        assert!(!stage.apply_phrase_command("sweep room.mix 0:101%"));
        assert_eq!(stage.song, original);
        assert!(stage.apply_phrase_command("sweep room.mix 0%:35% 2"));
        let room = stage
            .song
            .section(0, crate::console::SectionKind::Room)
            .unwrap();
        assert!(!room.bypassed);
        let room_id = room.id;
        let mix = crate::params::console::room::MIX;
        let p = stage.song.pattern(id).unwrap();
        assert_eq!(p.trig(0).lock_on(Some(room_id), mix), Some(0.0));
        assert_eq!(p.trig(63).lock_on(Some(room_id), mix), Some(35.0));
        assert_eq!(p.trig(63).lock(mix), None);
        let _ = stage.apply(super::super::StageIntent::Undo);
        assert_eq!(stage.song, original);
        assert!(stage.apply_phrase_command("sweep cutoff 8kHz:800Hz"));
        assert_eq!(
            stage
                .song
                .pattern(id)
                .unwrap()
                .trig(0)
                .lock(crate::params::table::CUTOFF),
            Some(8000.0)
        );
    }

    #[test]
    fn local_expression_windows_and_discrete_note_locks_are_atomic_and_undoable() {
        let mut stage = Stage::new();
        let id = stage.song.tracks[0].blocks[0].pattern_id;
        stage.inside = Some(super::super::Opened {
            pattern: id,
            track: 0,
        });
        stage
            .song
            .add_device(0, crate::devices::DeviceKind::Acid)
            .unwrap();
        stage.song.furnish();
        stage.settle();
        let before = stage.song.clone();
        assert!(stage.apply_phrase_command("lock ornament murki at 48,144"));
        let p = stage.song.pattern(id).unwrap();
        assert_eq!(p.trig(4).lock(crate::params::acid::ORNAMENT), Some(6.0));
        assert_eq!(p.trig(12).lock(crate::params::acid::ORNAMENT), Some(6.0));
        assert_eq!(p.trig(5).lock(crate::params::acid::ORNAMENT), None);
        let _ = stage.apply(super::super::StageIntent::Undo);
        assert_eq!(stage.song, before);
        assert!(stage.apply_phrase_command("sweep vibrato_intensity 0ct:24ct at 96:144"));
        let p = stage.song.pattern(id).unwrap();
        assert_eq!(
            p.trig(8).lock(crate::params::acid::VIBRATO_INTENSITY),
            Some(0.0)
        );
        assert_eq!(
            p.trig(11).lock(crate::params::acid::VIBRATO_INTENSITY),
            Some(24.0)
        );
        assert_eq!(
            p.trig(12).lock(crate::params::acid::VIBRATO_INTENSITY),
            None
        );
        let after = stage.song.clone();
        for bad in [
            "lock ornament nope at 48",
            "lock ornament murki at 49",
            "lock ornament murki at 48,99999",
            "lock ornament murki at 48 trailing",
            "sweep vibrato_intensity 0:101 at 96:144",
            "sweep ornament 0:6",
            "sweep vibrato_intensity 0:24 at 96:108",
            "sweep vibrato_intensity 0:24 at 96:145",
        ] {
            assert!(!stage.apply_phrase_command(bad), "{bad}");
            assert_eq!(stage.song, after, "{bad} partially edited the score");
        }
    }

    fn pattern() -> Pattern {
        Pattern::empty(PatternId(1), "test".into())
    }
    fn run(p: &mut Pattern, command: &str) -> Result<usize, String> {
        generate(p, &command.split_whitespace().collect::<Vec<_>>())
    }
    fn transform(
        p: &mut Pattern,
        words: &[&str],
        selected: Option<&[usize]>,
    ) -> Result<usize, String> {
        super::transform(p, words, selected, &crate::pitch::default_key())
    }

    #[test]
    fn slow_rate_steps_and_rewind_are_audition_safe() {
        let def = crate::params::def(
            crate::params::table::TABLE,
            crate::params::table::MOTION_RATE,
        );
        let label = &crate::params::table::LABELS[crate::params::table::MOTION_RATE as usize];
        assert_eq!(super::super::chain::step_of(def, label, false), 0.01);
        assert_eq!(super::super::chain::step_of(def, label, true), 0.1);
        let mut stage = Stage::new();
        assert_eq!(
            stage.apply(super::super::StageIntent::Rewind),
            super::super::ApplyOutcome::Changed
        );
        assert!(stage.refusal.is_none());
    }

    #[test]
    fn explicit_voicing_preserves_gates_velocity_and_locks() {
        let mut p = pattern();
        run(&mut p, "rhythm 4 0 gate 12 pitch -12 vel 82").unwrap();
        p.trig_mut(0).set_lock(4, 1800.0);
        transform(&mut p, &["voice", "0,7,11,16,19"], None).unwrap();
        let notes = &p.trig(0).notes;
        assert_eq!(notes.len(), 5);
        assert!(
            notes
                .iter()
                .all(|n| n.velocity == 82 && n.length_ticks == 576)
        );
        assert_eq!(p.trig(0).lock(4), Some(1800.0));
        assert_eq!(
            notes
                .iter()
                .map(|n| crate::pitch::nearest_midi(n.pitch.resolve(&crate::pitch::default_key())))
                .collect::<Vec<_>>(),
            vec![48, 55, 59, 64, 67]
        );
        let before = p.clone();
        assert!(transform(&mut p, &["voice", "0,0"], None).is_err());
        assert_eq!(p, before);
    }

    #[test]
    fn groups_and_legato_work_in_unrelated_meters_and_keep_chords_together() {
        let mut p = pattern();
        p.extend_timeline(7 * 48).unwrap();
        run(&mut p, "group 4 2,2,3 pitch 0,3,7").unwrap();
        p.trig_mut(0).add_tone_at(Note::new(67, 48, 100));
        transform(&mut p, &["legato", "3"], None).unwrap();
        let notes = note_positions(&p, None);
        assert_eq!(
            notes.iter().map(|n| n.0).collect::<Vec<_>>(),
            vec![0, 0, 96, 192]
        );
        assert_eq!(p.trig(0).notes[0].length_ticks, 99);
        assert_eq!(p.trig(0).notes[1].length_ticks, 99);
        assert_eq!(p.trig(16).notes[0].length_ticks, 144);
    }

    #[test]
    fn ratchet_accelerates_on_grid_and_expands_to_editable_chords() {
        let mut p = pattern();
        p.set_primary(0, Note::new(64, 48, 92));
        p.trig_mut(0).add_tone_at(Note::new(71, 48, 92));
        p.trig_mut(0).set_lock(7, 0.25);
        run(
            &mut p,
            "ratchet 64 0:32 spacing 4:1 gate 1 vel 40:110 pitch 0:12",
        )
        .unwrap();
        let notes = note_positions(&p, None);
        let ticks: Vec<_> = notes
            .iter()
            .map(|n| n.0)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        assert!(ticks.len() > 8);
        assert_eq!(ticks[1] - ticks[0], 12);
        assert_eq!(ticks[ticks.len() - 1] - ticks[ticks.len() - 2], 3);
        assert!(ticks.iter().all(|t| t % 3 == 0));
        assert_eq!(notes.len(), ticks.len() * 2);
        assert!(
            ticks
                .iter()
                .all(|tick| p.trig(tick / PATTERN_STEP_TICKS).lock(7) == Some(0.25))
        );
        assert_eq!(p.trig(0).notes[0].velocity, 40);
        assert!(
            notes
                .iter()
                .all(|n| p.trig(n.1).notes[n.2].length_ticks == 3)
        );
        let last = notes.last().unwrap();
        assert_eq!(p.trig(last.1).notes[last.2].velocity, 110);
    }

    #[test]
    fn contours_and_selection_leave_backbeats_and_locks_intact() {
        let mut p = pattern();
        run(&mut p, "rhythm 16 0,4,8,12 every 16 gate 2 vel 80,116").unwrap();
        p.trig_mut(4).set_lock(7, 0.25);
        transform(&mut p, &["shape", "vel", "40:90"], Some(&[0, 8])).unwrap();
        assert_eq!(p.trig(0).notes[0].velocity, 40);
        assert_eq!(p.trig(8).notes[0].velocity, 90);
        assert_eq!(p.trig(4).notes[0].velocity, 116);
        assert_eq!(p.trig(4).lock(7), Some(0.25));
    }

    #[test]
    fn invalid_sentences_are_atomic_and_edits_undo_in_one_step() {
        let mut stage = Stage::new();
        let id = stage.song.tracks[0].blocks[0].pattern_id;
        stage.inside = Some(super::super::Opened {
            pattern: id,
            track: 0,
        });
        let before = stage.song.clone();
        for sentence in [
            "rhythm 0 1",
            "rhythm 64 0,0",
            "group 4 3,0,2",
            "rhythm 16 0 every 0",
            "ratchet 64 0:9999999",
            "rhythm 16 0 vel NaN",
            "rhythm 16 0 at 9999999",
            "rhythm 16 0 wat 1",
        ] {
            assert!(!stage.apply_phrase_command(sentence), "{sentence}");
            assert_eq!(stage.song, before, "{sentence}");
        }
        assert!(stage.apply_phrase_command("group 4 3,3,2 every 8 pitch 0,-2,-5"));
        let edited = stage.song.clone();
        let text = ron::to_string(&edited).unwrap();
        assert_eq!(
            ron::from_str::<crate::sequencing::Song>(&text).unwrap(),
            edited
        );
        let _ = stage.apply(super::super::StageIntent::Undo);
        assert_eq!(stage.song, before);
        let _ = stage.apply(super::super::StageIntent::Redo);
        assert_eq!(stage.song, edited);
    }

    #[test]
    fn sweeps_follow_time_and_reverse_without_rewriting_notes() {
        let mut stage = Stage::new();
        let id = stage.song.tracks[0].blocks[0].pattern_id;
        stage.inside = Some(super::super::Opened {
            pattern: id,
            track: 0,
        });
        stage
            .song
            .add_device(0, crate::devices::DeviceKind::Sampler)
            .unwrap();
        assert!(stage.apply_phrase_command("ratchet 64 0:64 spacing 4:1 vel 50:100"));
        let notes = note_positions(stage.song.pattern(id).unwrap(), None);
        assert!(stage.apply_phrase_command("sweep cutoff 500:5000 2"));
        let p = stage.song.pattern(id).unwrap();
        let cutoff = crate::params::sampler::CUTOFF;
        assert_eq!(p.trig(0).lock(cutoff), Some(500.0));
        assert_eq!(p.trig(63).lock(cutoff), Some(5000.0));
        assert!(
            p.trig(0)
                .locks
                .iter()
                .any(|lock| lock.param == cutoff && lock.slide)
        );
        assert!(p.trig(32).lock(cutoff).unwrap() < 3000.0);
        assert_eq!(note_positions(p, None), notes);
        assert!(stage.apply_phrase_command("sweep cutoff 5000:500"));
        assert_eq!(
            stage.song.pattern(id).unwrap().trig(0).lock(cutoff),
            Some(5000.0)
        );
        let before = stage.song.clone();
        for command in [
            "sweep missing 0:1",
            "sweep cutoff 0:999999",
            "sweep cutoff 500:5000 NaN",
            "legato 999999",
        ] {
            assert!(!stage.apply_phrase_command(command));
            assert_eq!(stage.song, before);
        }
    }
}
