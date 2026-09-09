//! Printed bakes and versioned, editable recipes. Unique create prevents
//! concurrent kiln windows from overwriting one another's material.
use super::{Patch, membrane::Render};
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Recipe {
    pub version: u32,
    pub engine: String,
    pub note: u8,
    pub velocity: u8,
    pub rate: u32,
    pub patch: Patch,
}
pub fn directory() -> PathBuf {
    crate::corpus::dir().join("daw/kiln/membrane")
}
pub fn name(p: &Patch) -> String {
    let mut result = "membrane".to_owned();
    for (m, table) in p.macros.iter().zip(super::MACROS) {
        if (*m - 0.5).abs() > 0.005 {
            result.push_str(&format!("-{}-{m:.2}", table.name.to_lowercase()));
        }
    }
    result
}
pub fn print(
    render: &Render,
    p: &Patch,
    note: u8,
    velocity: u8,
    dir: &Path,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let base = name(p);
    for number in 1..=100_000 {
        let path = dir.join(format!("{base}-{number:03}.wav"));
        let file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        };
        let side = path.with_extension("sound.ron");
        let result = (|| -> Result<(), String> {
            let mut writer = hound::WavWriter::new(
                file,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: render.rate,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                },
            )
            .map_err(|e| e.to_string())?;
            for &sample in render.samples.iter() {
                writer.write_sample(sample).map_err(|e| e.to_string())?;
            }
            writer.finalize().map_err(|e| e.to_string())?;
            let recipe = Recipe {
                version: 1,
                engine: "membrane".into(),
                note,
                velocity,
                rate: render.rate,
                patch: p.clone(),
            };
            let text = ron::ser::to_string_pretty(&recipe, ron::ser::PrettyConfig::default())
                .map_err(|e| e.to_string())?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&side)
                .map_err(|e| e.to_string())?;
            file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = std::fs::remove_file(&path);
            return Err(error);
        }
        return Ok(path);
    }
    Err("kiln file numbering exhausted".into())
}
pub fn read(path: &Path) -> Result<Recipe, String> {
    let side = path.with_extension("sound.ron");
    let text = std::fs::read_to_string(side).map_err(|e| e.to_string())?;
    let recipe: Recipe = ron::from_str(&text).map_err(|e| e.to_string())?;
    if recipe.version != 1 || recipe.engine != "membrane" {
        return Err("unsupported kiln recipe".into());
    }
    if recipe.patch.sliders.len() != super::params::MEMBRANE_SLIDERS.len()
        || recipe
            .patch
            .sliders
            .iter()
            .chain(recipe.patch.macros.iter())
            .any(|v| !v.is_finite())
    {
        return Err("invalid kiln patch".into());
    }
    Ok(recipe)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unique_wav_and_recipe_roundtrip() {
        let dir = std::env::temp_dir().join(format!("daw-kiln-recipe-{}", std::process::id()));
        let p = Patch::default();
        let r = Render {
            samples: std::sync::Arc::new(vec![0., 0.2, -0.1]),
            animation: Default::default(),
            rate: 48_000,
            millis: 0.,
            wire_energy: 0.,
            shell_energy: 0.,
            max_tension: 0.,
        };
        let a = print(&r, &p, 60, 100, &dir).unwrap();
        let b = print(&r, &p, 60, 100, &dir).unwrap();
        assert_ne!(a, b);
        assert_eq!(read(&a).unwrap().patch, p);
        let mut reader = hound::WavReader::open(&a).unwrap();
        assert_eq!(
            reader
                .samples::<f32>()
                .map(Result::unwrap)
                .collect::<Vec<_>>(),
            *r.samples
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
