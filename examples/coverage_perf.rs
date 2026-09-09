//! Bounded realtime workload probes. Run after builds, without other CPU jobs.
//! These are local wall-time observations, not universal callback guarantees.
use daw::audio::graph::Ramp;
use std::time::Instant;
const SR: f32 = 48000.;
const BLOCK: usize = 256;
const BLOCKS: usize = 1000;

macro_rules! bench {
    ($module:ident,$params:ident,$voices:ident) => {{
        use daw::audio::$module::{$params, $voices};
        let mut p = $params::default();
        if let Some(def) = daw::params::$module::TABLE
            .iter()
            .find(|d| d.name == "voices")
        {
            p.set(def.id, 16.);
        }
        let mut bank = $voices::new();
        bank.prepare(SR, BLOCK, p);
        let mut samples = [0.; BLOCK];
        for dense in [false, true] {
            bank.reset();
            for i in 0..16 {
                bank.note_on(36 + i, 100, i as u64);
            }
            let mut durations = [0u128; BLOCKS];
            let start = Instant::now();
            let mut peak = 0.0f32;
            for (i, duration) in durations.iter_mut().enumerate() {
                let t = Instant::now();
                assert_no_alloc::assert_no_alloc(|| {
                    if dense && i % 4 == 0 {
                        // Four attacks every ~21ms: a deliberately dense retrigger workload.
                        for j in 0..4 {
                            bank.note_on(36 + ((i + j) % 24) as u8, 100, (i * BLOCK + j) as u64);
                        }
                    }
                    bank.render(&mut samples, 0, &mut Ramp::across(1., 1., BLOCK));
                });
                *duration = t.elapsed().as_nanos();
                assert!(samples.iter().all(|x| x.is_finite()));
                peak = samples.iter().map(|s| s.abs()).fold(peak, f32::max);
            }
            let elapsed = start.elapsed().as_secs_f64();
            durations.sort_unstable();
            println!(
                "{},{},{:.4},{:.2},{:.2},{:.2},{:.4}",
                stringify!($module),
                if dense { "retrigger" } else { "16 voices" },
                elapsed,
                durations[BLOCKS / 2] as f64 / 1000.,
                durations[BLOCKS * 99 / 100] as f64 / 1000.,
                durations[BLOCKS - 1] as f64 / 1000.,
                peak
            );
        }
    }};
}
fn main() {
    println!("machine,workload,wall_seconds,p50_us,p99_us,max_us,peak");
    bench!(table, TableParams, TableVoices);
    bench!(ring, RingParams, RingVoices);
    bench!(prism_voice, PrismVoiceParams, PrismVoiceVoices);
    bench!(mass, MassParams, MassVoices);
    bench!(pluck, PluckParams, PluckVoices);
    bench!(vox, VoxParams, VoxVoices);
    bench!(pipe, PipeParams, PipeVoices);
    bench!(glass, GlassParams, GlassVoices);
}
