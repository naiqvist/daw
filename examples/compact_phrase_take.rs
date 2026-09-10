//! Auditable re-authoring harness for a native phrase recipe, not an audio importer.
//! All shortened statements execute through Stage's ordinary palette commands.
//! Refuse to emit unless the entire resulting Song equals the original recipe.
//! cargo run --example compact_phrase_take -- EMPTY.stage.ron COMMANDS.txt FRESH_DIR
use daw::{
    sequencing::{Note, Pattern, PatternId},
    ui::stage::Stage,
};
use std::{collections::BTreeMap, fs::OpenOptions, io::Write, path::Path};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn write_new(path: &Path, text: &str) -> Result<()> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(text.as_bytes())?;
    Ok(())
}
fn apply(s: &mut Stage, c: &str) {
    assert!(s.apply_timeline_command(c), "{c}\n{}", s.status_line());
}
fn active(c: &str, current: Option<PatternId>) -> Option<PatternId> {
    if let Some(id) = c.strip_prefix("go clip #") {
        id.parse().ok().map(PatternId)
    } else if c.starts_with("go ") {
        None
    } else {
        current
    }
}
fn events(p: &Pattern) -> Option<BTreeMap<usize, Note>> {
    let mut out = BTreeMap::new();
    for step in 0..p.step_count() {
        for note in &p.trig(step).notes {
            if note.micro_ticks < 0 || note.muted {
                return None;
            }
            if out
                .insert(step * 12 + note.micro_ticks as usize, note.clone())
                .is_some()
            {
                return None;
            }
        }
    }
    Some(out)
}
fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}
fn cycle<T: PartialEq + ToString>(values: &[T]) -> String {
    let n = (1..=values.len())
        .find(|p| values.iter().enumerate().all(|(i, v)| *v == values[i % p]))
        .unwrap();
    values[..n]
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
fn notes_recipe(before: &Pattern, after: &Pattern) -> Option<String> {
    let original = events(before)?;
    let final_events = events(after)?;
    let changed: Vec<_> = final_events
        .iter()
        .filter(|(t, n)| original.get(t) != Some(*n))
        .collect();
    if changed.is_empty() {
        return None;
    }
    let mut unit = 192;
    for (tick, note) in &changed {
        unit = gcd(gcd(unit, **tick), note.length_ticks);
    }
    let ticks: Vec<_> = changed.iter().map(|(t, _)| **t / unit).collect();
    let gates: Vec<_> = changed.iter().map(|(_, n)| n.length_ticks / unit).collect();
    let velocities: Vec<_> = changed.iter().map(|(_, n)| n.velocity).collect();
    let mut pitches = Vec::new();
    for (_, note) in &changed {
        let midi = daw::pitch::nearest_midi(note.pitch.resolve(&daw::pitch::default_key()));
        let offset = midi as i32 - 60;
        // Do not quantize non-12TET/degree-anchored material to save commands.
        if Note::new(60, 1, 100)
            .pitch
            .shifted_semitones(offset as isize)
            != note.pitch
        {
            return None;
        }
        pitches.push(offset);
    }
    let command = format!(
        "rhythm {} {} gate {} pitch {} vel {}",
        192 / unit,
        ticks
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(","),
        cycle(&gates),
        cycle(&pitches),
        cycle(&velocities)
    );
    (command.len() < 4096 && changed.len() <= 1024).then_some(command)
}

#[derive(Clone, Debug, PartialEq)]
struct Sweep {
    target: String,
    values: String,
    start: usize,
    end: usize,
}
fn sweep(c: &str) -> Option<Sweep> {
    let w: Vec<_> = c.split_whitespace().collect();
    if w.len() != 5 || w[0] != "sweep" || w[3] != "at" {
        return None;
    }
    let (a, b) = w[4].split_once(':')?;
    Some(Sweep {
        target: w[1].into(),
        values: w[2].into(),
        start: a.parse().ok()?,
        end: b.parse().ok()?,
    })
}
fn repeats(input: Vec<String>, template: &Stage) -> Vec<String> {
    let mut result = Vec::new();
    let mut at = 0;
    let mut opened = None;
    let mut lengths: BTreeMap<_, _> = template
        .song()
        .patterns
        .iter()
        .map(|p| (p.id.0, p.length_ticks))
        .collect();
    while at < input.len() {
        opened = active(&input[at], opened);
        if let Some(n) = input[at]
            .strip_prefix("length ")
            .and_then(|s| s.parse().ok())
        {
            if let Some(id) = opened {
                lengths.insert(id.0, n);
            }
        }
        if at + 3 < input.len()
            && let (Some(a), Some(b), Some(c), Some(d), Some(id)) = (
                sweep(&input[at]),
                sweep(&input[at + 1]),
                sweep(&input[at + 2]),
                sweep(&input[at + 3]),
                opened,
            )
        {
            let period = c.start.saturating_sub(a.start);
            let shifted = |x: &Sweep, y: &Sweep, delta| {
                x.target == y.target
                    && x.values == y.values
                    && x.start + delta == y.start
                    && x.end + delta == y.end
            };
            if period > 0
                && a.end <= b.start
                && b.end <= a.start + period
                && shifted(&a, &c, period)
                && shifted(&b, &d, period)
            {
                let mut count = 2;
                while at + count * 2 + 1 < input.len() {
                    let (Some(x), Some(y)) = (
                        sweep(&input[at + count * 2]),
                        sweep(&input[at + count * 2 + 1]),
                    ) else {
                        break;
                    };
                    if !shifted(&a, &x, count * period) || !shifted(&b, &y, count * period) {
                        break;
                    }
                    count += 1;
                }
                let length = lengths[&id.0];
                if a.start + count * period >= length
                    && b.start + count * period >= length
                    && b.end + (count - 1) * period <= length
                {
                    result.push(format!("{} every {period}", input[at]));
                    result.push(format!("{} every {period}", input[at + 1]));
                    at += count * 2;
                    continue;
                }
            }
        }
        result.push(input[at].clone());
        at += 1;
    }
    result
}
fn bundles(input: Vec<String>) -> Vec<String> {
    let mut result = Vec::new();
    let mut at = 0;
    while at < input.len() {
        let words: Vec<_> = input[at].split_whitespace().collect();
        if words.len() == 5 && matches!(words[0], "lock" | "sweep") && words[3] == "at" {
            let mut assignments = vec![format!("{}={}", words[1], words[2])];
            let mut end = at + 1;
            while end < input.len() {
                let other: Vec<_> = input[end].split_whitespace().collect();
                if other.len() != 5
                    || other[0] != words[0]
                    || other[3] != words[3]
                    || other[4] != words[4]
                    || assignments
                        .iter()
                        .any(|a| a.starts_with(&format!("{}=", other[1])))
                {
                    break;
                }
                assignments.push(format!("{}={}", other[1], other[2]));
                end += 1;
            }
            if assignments.len() > 1 {
                result.push(format!(
                    "{} {} at {}",
                    words[0],
                    assignments.join(";"),
                    words[4]
                ));
                at = end;
                continue;
            }
        }
        result.push(input[at].clone());
        at += 1;
    }
    result
}
fn ratchets(input: Vec<String>) -> Vec<String> {
    let mut result = Vec::new();
    let mut at = 0;
    while at < input.len() {
        let w: Vec<_> = input[at].split_whitespace().collect();
        if w.first() == Some(&"ratchet")
            && at + 1 < input.len()
            && let Some(s) = sweep(&input[at + 1])
        {
            let division: usize = w[1].parse().unwrap();
            let (from, to) = w[2].split_once(':').unwrap();
            if from.parse::<usize>().unwrap() * 192 / division == s.start
                && to.parse::<usize>().unwrap() * 192 / division == s.end
            {
                result.push(format!("{} sweep {}={}", input[at], s.target, s.values));
                at += 2;
                continue;
            }
        }
        result.push(input[at].clone());
        at += 1;
    }
    result
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("use EMPTY.stage.ron COMMANDS.txt FRESH_DIR".into());
    }
    let commands: Vec<String> = std::fs::read_to_string(&args[2])?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_owned)
        .collect();
    let mut original = Stage::new();
    original.open(&args[1])?;
    let mut template = Stage::new();
    template.open(&args[1])?;
    let mut compact = Vec::new();
    let mut cursor = 0;
    let mut opened = None;
    while cursor < commands.len() {
        let c = &commands[cursor];
        opened = active(c, opened);
        if (c.starts_with("rhythm ") || c.starts_with("group "))
            && let Some(id) = opened
        {
            let before = original.song().pattern(id).unwrap().clone();
            let start = cursor;
            while cursor < commands.len()
                && (commands[cursor].starts_with("rhythm ")
                    || commands[cursor].starts_with("group "))
            {
                apply(&mut original, &commands[cursor]);
                cursor += 1;
            }
            if let Some(recipe) = notes_recipe(&before, original.song().pattern(id).unwrap()) {
                compact.push(recipe);
            } else {
                compact.extend_from_slice(&commands[start..cursor]);
            }
        } else {
            apply(&mut original, c);
            compact.push(c.clone());
            cursor += 1;
        }
    }
    let compact = ratchets(bundles(repeats(compact, &template)));
    let mut checked = Stage::new();
    checked.open(&args[1])?;
    for (i, c) in compact.iter().enumerate() {
        assert!(c.len() < 4096);
        apply(&mut checked, c);
        if i % 100 == 0 {
            println!("checked {i}/{}", compact.len());
        }
    }
    if original.song() != checked.song() {
        for a in &original.song().patterns {
            if checked.song().pattern(a.id) != Some(a) {
                eprintln!("MISMATCH pattern {} {}", a.id.0, a.name);
            }
        }
        return Err("compact recipe changed the Song; nothing emitted".into());
    }
    let dir = Path::new(&args[3]);
    std::fs::create_dir(dir)?;
    template.save_as(dir.join("Replay.stage.ron"))?;
    checked.save_as(dir.join("preflight.stage.ron"))?;
    write_new(&dir.join("commands.txt"), &compact.join("\n"))?;
    let mut drive = String::from(
        "# Compact native phrase TAKE; same Song as original, no hidden note import.\npace 1\ntimeout 900\nuntil library ready\n",
    );
    let mut palette = |c: &str| drive.push_str(&format!("key ctrl+shift+p\ntext {c}\nkey Enter\n"));
    for c in &compact {
        palette(c);
    }
    palette("go bar 1");
    drop(palette);
    drive.push_str("key ctrl+s\nkey Home\nkey Space\nkey ctrl+shift+r\nuntil play bar 003\n");
    for (bar, end) in [(17, 21), (97, 101)] {
        drive.push_str(&format!(
            "key ctrl+shift+p\ntext seek bar {bar}\nkey Enter\nuntil play bar {end:03}\n"
        ));
    }
    drive.push_str("key ArrowDown\nkey ArrowRight\nkey Enter\n");
    drive.push_str(&format!(
        "shot {}\n",
        dir.join("tracker-peak.png").display()
    ));
    drive.push_str("key Escape\nkey ctrl+shift+d\n");
    drive.push_str(&format!("shot {}\n", dir.join("health.png").display()));
    drive.push_str("key Escape\nkey ctrl+shift+p\ntext seek bar 65\nkey Enter\nuntil play bar 069\nkey Space\nkey Home\nkey ctrl+shift+e\nkey ArrowDown\nkey Enter\nkey ctrl+a\n");
    drive.push_str(&format!("text {}\n", dir.join("audio-v2.wav").display()));
    drive.push_str("key Enter\nkey ArrowDown\nkey ArrowDown\nkey ArrowDown\nkey ArrowDown\nkey Enter\nuntil EXPORT COMPLETE\nkey Escape\nkey Home\nkey Space\nkey ctrl+shift+r\nuntil play bar 002\n");
    drive.push_str(&format!("shot {}\n", dir.join("tracker.png").display()));
    drive.push_str("echo TAKE COMPLETE\n");
    write_new(&dir.join("compact.drive"), &drive)?;
    let before: usize = commands.iter().map(String::len).sum();
    let after: usize = compact.iter().map(String::len).sum();
    println!(
        "EXACT SONG EQUALITY: {} -> {} native palette commands ({:.1}% reduction); {} -> {} command characters ({:.1}% reduction)",
        commands.len(),
        compact.len(),
        100. * (1. - compact.len() as f64 / commands.len() as f64),
        before,
        after,
        100. * (1. - after as f64 / before as f64)
    );
    Ok(())
}
