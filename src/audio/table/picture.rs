static TABLES: std::sync::OnceLock<Vec<Vec<f32>>> = std::sync::OnceLock::new();
use super::bank::{Patch, chord, glass_ratio, material_ratios};
use crate::pages::{Hero, HeroMark, HeroSeries, KeyTable};
pub fn hero<P: Patch>(p: P, keys: KeyTable, page: &str, selected: Option<u32>) -> Option<Hero> {
    let sub = keys
        .iter()
        .flatten()
        .flat_map(|k| k.subpages)
        .find(|s| s.title == page)?;
    let selected = selected.filter(|id| sub.slots.contains(&Some(*id)));
    let v = |id| p.get(id);
    let mut h = Hero {
        waveform: None,
        title: page.to_owned(),
        series: Vec::new(),
        marks: Vec::new(),
        x_labels: ["0".into(), "1".into()],
        y_labels: ["0".into(), "1".into()],
        diagonal: false,
    };
    let curve = |name: &'static str, f: &dyn Fn(f32) -> f32| HeroSeries {
        name,
        points: (0..=192)
            .map(|i| {
                let x = i as f32 / 192.;
                (x, f(x).clamp(0., 1.))
            })
            .collect(),
        lit: true,
    };
    match page {
        "Amp" => {
            let total = (v(0) + v(1) + v(3)).max(1.) + 500.;
            h.x_labels[1] = format!("{:0.1} s", total * 0.001);
            h.title = format!(
                "GATE · {:0.0} ms / {:0.0} ms / {:0.0}% / {:0.0} ms",
                v(0),
                v(1),
                v(2) * 100.,
                v(3)
            );
            h.series.push(curve("envelope", &|x| {
                let ms = x * total;
                if ms < v(0) {
                    ms / v(0).max(0.01)
                } else if ms < v(0) + v(1) + 500. {
                    v(2) + (1. - v(2)) * (-6.907755 * (ms - v(0)) / v(1).max(1.)).exp()
                } else {
                    v(2) * (-6.907755 * (ms - v(0) - v(1) - 500.) / v(3).max(1.)).exp()
                }
            }));
            h.marks.push(HeroMark {
                x: (v(0) + v(1) + 500.) / total,
                label: "OFF".into(),
                lit: selected == Some(3),
            });
        }
        "Filter" | "Band" => {
            h.x_labels = ["30 Hz".into(), "20 kHz".into()];
            h.y_labels = ["−60 dB".into(), "+18 dB".into()];
            let hz = if P::KIND == 2 { v(8).min(v(20)) } else { v(8) };
            h.title = format!("LOW PASS · {:0.0} Hz · Q {:0.2}", hz, v(9));
            h.series.push(curve("response", &|x| {
                let f = 30. * (20000_f32 / 30.).powf(x);
                let w = f / hz.max(1.);
                let mag = 1. / ((1. - w * w).powi(2) + (w / v(9)).powi(2)).sqrt().max(1e-6);
                (20. * mag.log10() + 60.) / 78.
            }));
            h.marks.push(HeroMark {
                x: (hz / 30.).log2() / (20000_f32 / 30.).log2(),
                label: format!("{hz:0.0}"),
                lit: true,
            });
        }
        "Wave" => {
            h.x_labels = ["0°".into(), "360°".into()];
            h.y_labels = ["−1".into(), "+1".into()];
            h.title = format!(
                "MORPH {:0.2} · SCAN {:+0.2} · ROUGH {:0.0}%",
                v(13),
                v(14),
                v(20) * 100.
            );
            let tables = TABLES.get_or_init(|| {
                crate::dsp::osc::Waveform::ALL
                    .iter()
                    .map(|w| {
                        let mut table = vec![0.; crate::dsp::osc::table_len(*w)];
                        crate::dsp::osc::build_tables(*w, &mut table);
                        table
                    })
                    .collect()
            });
            h.series.push(curve("wave", &|x| {
                let morph = v(13);
                let metal = ((morph - 3.) / 3.).clamp(0., 1.);
                let noise = ((morph - 5.2) / 1.8).clamp(0., 1.);
                let phase = x + v(18);
                let pm = (core::f32::consts::TAU * phase * (1. + metal * 0.41421356)).sin()
                    * metal
                    * v(20)
                    * 0.18;
                let read = |w: usize, p, h| {
                    super::bank::mip_cycle(
                        tables.get(w).map(Vec::as_slice).unwrap_or(&[]),
                        p,
                        h,
                        48000.,
                    )
                };
                let hz = 261.62555 * 2_f32.powf(v(6) / 12.);
                let mut wave = super::bank::wave_pair(morph, phase + pm, hz, read);
                if (morph - 3.).abs() < 1. {
                    let pulse = read(2, phase + pm, hz) - read(2, phase + pm + v(21), hz);
                    let amount = (1. - (morph - 3.).abs()) * (2. * (v(21) - 0.5).abs());
                    wave = wave * (1. - amount) + pulse * amount;
                }
                // Noise has no repeating cycle. Its wash is represented by a fixed
                // deterministic realization, explicitly labelled in the caption.
                let seed = (x * 192.) as u32;
                let random = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let n = (random.rotate_left(13) as f64 / 2147483648. - 1.) as f32;
                let sample = wave * (1. - noise).sqrt() + n * noise.sqrt() * 0.65;
                0.5 + sample * 0.4
            }));
            if v(13) > 5.2 {
                h.title.push_str(" · AIR REALIZATION");
            }
        }
        "Voicing" => {
            h.x_labels = ["ROOT".into(), "+4 OCT".into()];
            h.y_labels = ["LEFT".into(), "RIGHT".into()];
            let notes = chord(v(22).round() as usize);
            for (i, n) in notes.iter().enumerate() {
                let interval = *n as f32
                    + if i < (v(23) as usize) { 12. } else { 0. }
                    + if i % 2 == 1 { v(24) * 12. } else { 0. };
                let x = (interval / 48.).clamp(0., 1.);
                h.series.push(HeroSeries {
                    name: "voice",
                    points: vec![(x, 0.), (x, 0.7 + v(17) * 0.003)],
                    lit: true,
                });
                h.marks.push(HeroMark {
                    x,
                    label: format!("{interval:+0.0}"),
                    lit: true,
                });
            }
            h.title = format!(
                "{} TONES · DETUNE {:0.0} ct · DRIFT {:0.0} ct",
                notes.len(),
                v(17),
                v(25)
            );
        }
        "Body" | "Modes" | "Contact" => {
            h.x_labels = ["FUNDAMENTAL".into(), "×20".into()];
            h.title = format!(
                "MATERIAL {:0.2} · INHARM {:0.2} · DAMP {:0.2}",
                v(13),
                v(14),
                v(15)
            );
            let ratios = material_ratios(v(13), v(14));
            for (i, ratio) in ratios.iter().enumerate() {
                let x = ratio.log2() / 20_f32.log2();
                let weight = v(21 + i as u32)
                    * (0.2 + (core::f32::consts::PI * (i + 1) as f32 * v(18)).sin().abs())
                    / (i + 1) as f32;
                h.series.push(HeroSeries {
                    name: "mode",
                    points: vec![(x, 0.), (x, (weight * 0.7).min(1.))],
                    lit: selected
                        .map(|s| s < 21 || s > 26 || s == 21 + i as u32)
                        .unwrap_or(true),
                });
                h.marks.push(HeroMark {
                    x,
                    label: format!("{ratio:0.2}×"),
                    lit: false,
                });
            }
            if page == "Contact" {
                h.series.clear();
                h.marks.clear();
                h.series
                    .push(curve("contact", &|x| (1. - x).powf(1. + v(17) * 6.)));
                h.x_labels[0] = "0 ms".into();
                h.x_labels[1] = format!("{:0.1} ms", v(28));
            }
        }
        "Prism" => {
            h.x_labels = ["30 Hz".into(), "16 kHz".into()];
            h.title = format!(
                "SIEVE {:0.0}% · SHIFT {:+0.0} Hz · FREEZE {:0.0}%",
                v(14) * 100.,
                v(16),
                v(19) * 100.
            );
            h.series.push(curve("sieve", &|x| {
                let f = 30. * (16000_f32 / 30.).powf(x);
                let moved = f - v(16);
                let note = 261.62555 * 2_f32.powf(v(6) / 12.);
                let d = ((moved / note - moved.div_euclid(note) - 0.5).abs() * 2.).clamp(0., 1.);
                let mask = if d > 1. - v(15) { 1. } else { 1. - v(14) };
                mask * (0.4 + (x - 0.5) * v(17) / 48.).clamp(0.05, 1.)
            }));
        }
        "Motion" => {
            h.x_labels = ["0 s".into(), "4 s".into()];
            h.series.push(curve("sieve", &|x| {
                (v(14) + v(21) * (-6.907755 * x * 4000. / v(22)).exp()).clamp(0., 1.)
            }));
            h.series.push(curve("shift", &|x| {
                0.5 + (v(16) + v(23) * (core::f32::consts::TAU * x * 4. * v(24)).sin()) / 8000.
            }));
        }
        "Pitch" => {
            let hz = 261.62555 * 2_f32.powf(v(6) / 12.);
            let ratio = glass_ratio(v(14), v(15), v(16), v(22));
            h.title = format!("C4 REFERENCE · {:0.1} Hz · TUNE {:+0.0} st", hz, v(6));
            h.x_labels = ["0 ms".into(), "5 ms".into()];
            h.y_labels = ["−1".into(), "+1".into()];
            h.series.push(curve("carrier", &|x| {
                let phase = core::f32::consts::TAU * hz * x * 0.005;
                0.5 + 0.45 * (phase + (phase * ratio).sin() * v(17)).sin()
            }));
        }
        "Ops" | "Gesture" => {
            let ratio = glass_ratio(v(14), v(15), v(16), v(22));
            h.title = format!(
                "B {:0.3}× → A · INDEX {:0.2} · FEEDBACK {:0.0}%",
                ratio,
                v(17),
                v(20) * 100.
            );
            if matches!(selected, Some(17) | Some(18) | Some(19) | Some(21)) {
                h.x_labels = ["0 s".into(), format!("{:0.1} s", v(18) * 0.003)];
                h.series.push(curve("index", &|x| {
                    (v(19) + (1. - v(19)) * (-6.907755 * x * 3.).exp()) * v(17) / 12.
                }));
            } else {
                h.x_labels = ["0°".into(), "360°".into()];
                h.series.push(curve("carrier", &|x| {
                    0.5 + 0.45
                        * (core::f32::consts::TAU * x
                            + (core::f32::consts::TAU * x * ratio).sin() * v(17))
                        .sin()
                }));
            }
        }
        "Ripple" => {
            h.x_labels = ["0×".into(), "8× NOTE".into()];
            h.series.push(curve("comb", &|x| {
                let phase = core::f32::consts::TAU * x * 8. / v(29);
                let mag = 1.
                    / (1. + v(30) * v(30) - 2. * v(30) * phase.cos())
                        .sqrt()
                        .max(0.05);
                (0.5 + mag.log2() / 8.).clamp(0., 1.)
            }));
        }
        "Fold" => {
            h.diagonal = true;
            h.x_labels = ["−1".into(), "+1".into()];
            let mut shape = crate::dsp::shaper::Waveshaper::new();
            shape.configure(crate::dsp::shaper::Mode::Fold, 1. + v(33), v(35), v(36));
            h.series
                .push(curve("transfer", &|x| 0.5 + 0.5 * shape.shape(x * 2. - 1.)));
        }
        "Ensemble" => {
            h.x_labels = ["0 s".into(), "2 s".into()];
            h.y_labels = ["3 ms".into(), "27 ms".into()];
            for i in 0..3 {
                h.series.push(curve("delay", &|x| {
                    let phase = x * 2. * v(38) + i as f32 / 3.;
                    0.5 + v(37) / 24.
                        * (0.7 * (core::f32::consts::TAU * phase).sin()
                            + 0.3 * (core::f32::consts::TAU * phase * 10.).sin())
                }));
            }
        }
        "Slap" | "Shimmer" => {
            let (time, feed, pitch) = if P::KIND == 0 {
                (v(41), v(42), 0.)
            } else {
                (v(30), v(31), v(29))
            };
            h.x_labels = ["0 s".into(), format!("{:0.1} s", time * 0.008)];
            h.title = format!(
                "{time:0.0} ms · FEED {:0.0}% · {pitch:+0.0} st / REPEAT",
                feed * 100.
            );
            for i in 1..8 {
                let x = i as f32 / 8.;
                h.series.push(HeroSeries {
                    name: "repeat",
                    points: vec![(x, 0.), (x, feed.abs().powi(i - 1).clamp(0., 1.))],
                    lit: true,
                });
            }
        }
        "Disperse" => {
            h.x_labels = ["30 Hz".into(), "16 kHz".into()];
            h.title = format!(
                "{:0.0} STAGES · FOCUS {:0.2}× · WIDTH {:0.1}",
                v(29) * 8.,
                v(30),
                v(31)
            );
            h.series.push(curve("phase", &|x| {
                0.5 + (x * 24. - v(30).log2() * 3.).atan() * v(29) / core::f32::consts::PI
            }));
        }
        "Bloom" | "Halo" => {
            let decay = if P::KIND == 2 { v(33) } else { v(34) };
            h.x_labels = ["0 s".into(), "12 s".into()];
            h.series.push(curve("decay", &|x| {
                (-6.907755 * x * 12. / decay.max(0.05)).exp()
            }));
            h.title = format!("TAIL · {:0.1} s · DAMP {:0.0} Hz", decay, v(35));
        }
        "Smear" => {
            h.x_labels = ["LOW".into(), "HIGH".into()];
            h.y_labels = ["0 ms".into(), "200 ms".into()];
            h.series
                .push(curve("delay", &|x| (v(29) + (v(30) - v(29)) * x) / 200.));
        }
        "Tilt" => {
            h.x_labels = ["30 Hz".into(), "20 kHz".into()];
            h.y_labels = ["−18 dB".into(), "+18 dB".into()];
            h.series
                .push(curve("tilt", &|x| 0.5 + (x - 0.5) * v(33) / 36.));
        }
        _ => return None,
    }
    Some(h)
}
