//! The creative constants, editable while the stage is running.
//!
//! Some constants are structural and changing one is a code change. A few
//! are CREATIVE — how much is cut off a corner, how much the glass blooms,
//! how tall a head is — and those get changed twenty times in an afternoon
//! by someone looking at the result. The cockpit's answer (`~/Work/tachikoma`,
//! `tune.rs`) is the one here: a constant marked `@tune` in its doc comment
//! appears in the inspector, moves live, and can be written back into the
//! source as its new default.
//!
//! ```text
//! /// How much is taken off the two keyed corners.
//! /// @tune 0..40 px
//! const CUT: f64 = 9.0;
//! ```
//!
//! and at every use, `crate::tune!(CUT)` instead of `CUT`.
//!
//! Three copies of a value, and which one wins: the constant in the source
//! is the default and ships; the override file, `~/Corpus/daw.tune`, holds
//! whatever is being tried, watched so an edit from anywhere lands on the
//! next frame; the registry is the parsed override, read under a lock at
//! each use. An override always wins while it exists.
//!
//! Only the view and the glass are scanned: `src/ui/stage/view` and
//! `src/shell`. The core has no creative constants, by construction.

use std::collections::BTreeMap;
use std::sync::RwLock;

/// One `@tune` constant as the inspector sees it.
#[derive(Clone, Debug)]
pub struct Knob {
    /// `file.NAME`, the key in the override file.
    pub key: String,
    pub file: std::path::PathBuf,
    pub line: usize,
    pub name: String,
    pub ty: String,
    pub default: f64,
    pub range: Option<(f64, f64)>,
    pub unit: Option<String>,
    pub doc: String,
}

static VALUES: RwLock<BTreeMap<String, f64>> = RwLock::new(BTreeMap::new());

/// A number the registry can hold: everything a `@tune` constant may be.
pub trait Tunable: Copy {
    fn to_f64(self) -> f64;
    fn from_f64(v: f64) -> Self;
}

macro_rules! tunable_as {
    ($($t:ty),*) => {$(
        impl Tunable for $t {
            fn to_f64(self) -> f64 { self as f64 }
            fn from_f64(v: f64) -> Self { v as $t }
        }
    )*};
}
tunable_as!(f32, f64, u8, u16, u32, u64, usize, i32, i64);

/// The value in force: the override if there is one, else the default.
pub fn get<T: Tunable>(module: &str, name: &str, default: T) -> T {
    let Ok(values) = VALUES.read() else {
        return default;
    };
    if values.is_empty() {
        return default;
    }
    match values.get(&key(module, name)) {
        Some(v) => T::from_f64(*v),
        None => default,
    }
}

/// `file.NAME`: the last segment of the module path, so the key matches
/// what `scan_source` derives from the file's stem.
pub fn key(module: &str, name: &str) -> String {
    let leaf = module.rsplit("::").next().unwrap_or(module);
    format!("{leaf}.{name}")
}

/// A `@tune` constant's live value.
#[macro_export]
macro_rules! tune {
    ($name:ident) => {
        $crate::tune::get(module_path!(), stringify!($name), $name)
    };
}

/// Where the overrides live: beside the theme, one console.
pub fn overrides_path() -> std::path::PathBuf {
    crate::corpus::dir().join("daw.tune")
}

/// The directories whose sources carry knobs.
const SCANNED: &[&str] = &["src/ui/stage/view", "src/shell"];

/// Every `@tune` constant in the scanned sources.
pub fn scan_source() -> Vec<Knob> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for dir in SCANNED {
        walk(&root.join(dir), &mut files);
    }
    let mut knobs = Vec::new();
    for file in files {
        let Some(stem) = file.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(source) = std::fs::read_to_string(&file) else {
            continue;
        };
        for d in tunable::scan(&source) {
            let Ok(default) = d
                .literal
                .trim_end_matches("f32")
                .trim_end_matches("f64")
                .parse::<f64>()
            else {
                continue;
            };
            knobs.push(Knob {
                key: format!("{stem}.{}", d.name),
                file: file.clone(),
                line: d.line,
                name: d.name.to_owned(),
                ty: d.ty.to_owned(),
                default,
                range: d.range,
                unit: d.unit.map(str::to_owned),
                doc: d.doc.join(" ").trim().to_owned(),
            });
        }
    }
    knobs.sort_by(|a, b| a.key.cmp(&b.key));
    knobs
}

fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// The override file, watched.
pub struct Overrides {
    path: std::path::PathBuf,
    settle: watch::Settle,
    pub count: usize,
}

impl Overrides {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self {
            path,
            settle: watch::Settle::new(3),
            count: 0,
        }
    }

    /// Once a frame. `true` when the file just changed.
    pub fn poll(&mut self) -> bool {
        if !self.settle.observe(watch::fingerprint(&self.path)) {
            return false;
        }
        let text = std::fs::read_to_string(&self.path).unwrap_or_default();
        let next = parse(&text);
        self.count = next.len();
        if let Ok(mut w) = VALUES.write() {
            *w = next;
        }
        true
    }
}

/// `key value` per line; `#` starts a comment.
pub fn parse(text: &str) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        if let (Some(k), Some(v)) = (parts.next(), parts.next())
            && let Ok(v) = v.parse::<f64>()
        {
            out.insert(k.to_owned(), v);
        }
    }
    out
}

pub fn to_text(values: &BTreeMap<String, f64>) -> String {
    let mut out = String::from(
        "# daw tunables. key value.\n\
         # Written by the inspector; safe to edit by hand.\n\
         # Delete a line to go back to the compiled-in default.\n",
    );
    for (k, v) in values {
        out.push_str(&format!("{k} {v}\n"));
    }
    out
}

/// Write one override to the file; the watcher brings it into force.
pub fn set(key: &str, value: f64) -> std::io::Result<()> {
    let path = overrides_path();
    let mut values = parse(&std::fs::read_to_string(&path).unwrap_or_default());
    values.insert(key.to_owned(), value);
    std::fs::write(&path, to_text(&values))
}

/// Put a value straight into the registry without writing: a drag in
/// progress, sixty times a second.
pub fn preview(key: &str, value: f64) {
    if let Ok(mut w) = VALUES.write() {
        w.insert(key.to_owned(), value);
    }
}

pub fn clear(key: &str) -> std::io::Result<()> {
    let path = overrides_path();
    let mut values = parse(&std::fs::read_to_string(&path).unwrap_or_default());
    values.remove(key);
    std::fs::write(&path, to_text(&values))
}

pub fn override_of(key: &str) -> Option<f64> {
    VALUES.read().ok()?.get(key).copied()
}

/// Write the value into the source as the constant's new default.
pub fn commit(knob: &Knob, value: f64) -> std::io::Result<()> {
    let source = std::fs::read_to_string(&knob.file)?;
    let literal = literal_for(&knob.ty, value);
    let edited = tunable::rewrite(&source, &knob.name, &literal).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "{} is no longer marked @tune in {}",
                knob.name,
                knob.file.display()
            ),
        )
    })?;
    std::fs::write(&knob.file, edited)
}

/// A literal of the constant's type: floats keep a point, integers round.
pub fn literal_for(ty: &str, value: f64) -> String {
    if ty.trim_start().starts_with('f') {
        let s = format!("{value}");
        if s.contains(['.', 'e', 'N', 'i']) {
            s
        } else {
            format!("{s}.0")
        }
    } else {
        format!("{}", value.round() as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_override_format_survives_being_edited_by_hand() {
        let text = "# a comment\nchassis.CUT 12.5  # trailing\n\nheads.HEAD_H 48\nbad line here\n";
        let values = parse(text);
        assert_eq!(values.get("chassis.CUT"), Some(&12.5));
        assert_eq!(values.get("heads.HEAD_H"), Some(&48.0));
        assert_eq!(values.len(), 2);
        assert_eq!(parse(&to_text(&values)), values);
    }

    #[test]
    fn a_key_is_the_files_stem_and_the_name() {
        assert_eq!(key("daw::ui::stage::view::chassis", "CUT"), "chassis.CUT");
    }

    #[test]
    fn a_literal_keeps_its_type() {
        assert_eq!(literal_for("f64", 12.0), "12.0");
        assert_eq!(literal_for("f32", 0.35), "0.35");
        assert_eq!(literal_for("usize", 3.7), "4");
    }

    /// The scan finds the knobs the view actually declares.
    #[test]
    fn the_view_declares_knobs() {
        let knobs = scan_source();
        assert!(knobs.iter().any(|k| k.key == "chassis.CUT"), "{knobs:?}");
        assert!(
            knobs.iter().all(|k| k.range.is_some()),
            "every knob states its range"
        );
    }
}
