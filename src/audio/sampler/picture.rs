use super::SamplerParams;
use crate::pages::{Hero, HeroMark, HeroSeries};
use crate::params::sampler as p;

pub fn hero(params: &SamplerParams, page: &str, selected: Option<u32>) -> Option<Hero> {
    let declared = p::KEYS
        .iter()
        .flatten()
        .flat_map(|k| k.subpages)
        .any(|s| s.title == page);
    if !declared {
        return None;
    }
    let mut out = Hero {
        waveform: None,
        title: page.to_uppercase(),
        series: Vec::new(),
        marks: Vec::new(),
        x_labels: ["0".into(), "1".into()],
        y_labels: ["0".into(), "1".into()],
        diagonal: false,
    };
    let points: Vec<(f32, f32)> = match page {
        "Amp" | "Envelope" => {
            let (a, d, s, r) = if page == "Amp" {
                (
                    params.amp_attack_ms,
                    params.amp_decay_ms,
                    params.amp_sustain,
                    params.amp_release_ms,
                )
            } else {
                (
                    params.mod_attack_ms,
                    params.mod_decay_ms,
                    params.mod_sustain,
                    params.mod_release_ms,
                )
            };
            let total = (a + d + r + 500.0).max(1.0);
            out.x_labels[1] = format!("{:.2} s", total / 1000.0);
            vec![
                (0.0, 0.0),
                (a / total, 1.0),
                ((a + d) / total, s),
                ((a + d + 500.0) / total, s),
                (1.0, 0.0),
            ]
        }
        "Colour" => {
            out.diagonal = true;
            out.x_labels = ["−1".into(), "+1".into()];
            out.y_labels = out.x_labels.clone();
            let mut shaper = crate::dsp::shaper::Waveshaper::new();
            shaper.configure(
                crate::dsp::shaper::Mode::SoftClip,
                1.0 + params.drive * 7.0,
                params.drive * 0.08,
                1.0,
            );
            (0..128)
                .map(|i| {
                    let x = i as f32 / 127.0;
                    let v = if params.drive == 0.0 {
                        x * 2.0 - 1.0
                    } else {
                        shaper.shape(x * 2.0 - 1.0)
                    };
                    (x, (v * 0.5 + 0.5).clamp(0.0, 1.0))
                })
                .collect()
        }
        "Filter" => {
            out.x_labels = ["20 Hz".into(), "20 kHz".into()];
            out.y_labels = ["−48 dB".into(), "+24 dB".into()];
            out.marks.push(HeroMark {
                x: (params.cutoff_hz / 20.0).log10() / 3.0,
                label: format!("{:.0} Hz", params.cutoff_hz),
                lit: true,
            });
            (0..128)
                .map(|i| {
                    let x = i as f32 / 127.0;
                    let f = 20.0 * 1000f32.powf(x) / params.cutoff_hz.max(1.0);
                    let den = ((1.0 - f * f).powi(2) + (f / params.res).powi(2))
                        .sqrt()
                        .max(1e-8);
                    let mag = match params.filter_mode.round() as u32 {
                        1 => f * f / den,
                        2 => f / params.res / den,
                        3 => (1.0 - f * f).abs() / den,
                        _ => 1.0 / den,
                    };
                    let db = 20.0 * mag.max(1e-9).log10() * (1.0 + params.filter_slope);
                    (x, ((db + 48.0) / 72.0).clamp(0.0, 1.0))
                })
                .collect()
        }
        "Comb" => {
            out.x_labels = ["0 Hz".into(), "8 × note".into()];
            out.y_labels = ["−24 dB".into(), "+24 dB".into()];
            (0..256)
                .map(|i| {
                    let x = i as f32 / 255.0;
                    let phase = core::f32::consts::TAU * x * 8.0 / params.comb_focus;
                    let gain = (1.0 + params.comb_feed.powi(2)
                        - 2.0 * params.comb_feed * phase.cos())
                    .sqrt()
                    .max(1e-6)
                    .recip();
                    (
                        x,
                        ((20.0 * gain.log10() * params.comb_mix + 24.0) / 48.0).clamp(0.0, 1.0),
                    )
                })
                .collect()
        }
        "Routes" => {
            out.y_labels = ["−depth".into(), "+depth".into()];
            vec![
                (0.0, (params.env_pitch / 48.0 + 1.0) * 0.5),
                (0.5, (params.env_position + 1.0) * 0.5),
                (1.0, (params.env_size + 1.0) * 0.5),
            ]
        }
        "Voice" => (0..params.voice_count.round() as usize)
            .map(|i| {
                (
                    i as f32 / 15.0,
                    0.5 + params.spread * (i as f32 / 15.0 - 0.5),
                )
            })
            .collect(),
        _ => vec![(0.0, 0.5), (1.0, 0.5)],
    };
    out.series.push(HeroSeries {
        name: "response",
        points,
        lit: selected.is_some(),
    });
    Some(out)
}
