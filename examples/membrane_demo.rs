//! Render the membrane's reference sounds and report measured render cost.
//! RUSTC_WRAPPER="" cargo run --example membrane_demo -- /path/to/output
use daw::kiln::{Patch, files, indices::*, membrane::Membrane};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("membrane-demo"));
    let engine = Membrane::default();
    let base = Patch::default();
    for i in 0..3 {
        let r = engine
            .render(&base, 60, 100, 48_000, false, &mut |_| true)
            .ok_or("preview failed")?;
        println!(
            "preview {i}: {:.2} ms, peak {:.4}, tension +{:.2} N/m",
            r.millis,
            r.samples.iter().fold(0f32, |p, s| p.max(s.abs())),
            r.max_tension
        );
    }
    for label in ["snare", "tom", "bass-drum", "wires-on", "wires-off"] {
        let mut p = base.clone();
        match label {
            "tom" => {
                p.turn_macro(0, 0.62);
                p.turn_macro(6, 0.0);
            }
            "bass-drum" => {
                p.turn_macro(0, 1.0);
                p.turn_macro(6, 0.0);
                p.turn_macro(5, 0.2);
                p.turn_macro(3, 0.15);
            }
            "wires-on" => {
                p.set(WIRES_REST_GAP, 0.0);
            }
            "wires-off" => {
                p.set(WIRES_COUNT, 0.0);
            }
            _ => {}
        }
        let r = engine
            .render(&p, 60, 100, 48_000, true, &mut |_| true)
            .ok_or_else(|| format!("{label} failed"))?;
        let out = files::print(&r, &p, 60, 100, &dir.join(label))?;
        println!(
            "{label}: {:.2} ms, wire energy {:.6}, shell energy {:.6}, tension +{:.2} N/m · {}",
            r.millis,
            r.wire_energy,
            r.shell_energy,
            r.max_tension,
            out.display()
        );
    }
    Ok(())
}
