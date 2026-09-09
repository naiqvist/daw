//! Independent spectral measurements of printed WAVs. No render in this tool.
use daw::dsp::fft::{RealFft, Window, fill_window};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for arg in std::env::args().skip(1) {
        let mut wav = hound::WavReader::open(&arg)?;
        let rate = wav.spec().sample_rate as f64;
        let samples: Vec<f32> = wav.samples::<f32>().collect::<Result<_, _>>()?;
        let n = 2048;
        let mut fft = RealFft::new();
        fft.prepare(n);
        let mut window = vec![0.; n];
        fill_window(Window::Hann, &mut window);
        let mut real = vec![0.; RealFft::bins(n)];
        let mut imag = real.clone();
        let mut power = vec![0f64; real.len()];
        let mut scratch = vec![0.; RealFft::scratch_len(n)];
        let start = (rate * 0.03) as usize;
        let end = ((rate * 0.20) as usize).min(samples.len());
        for offset in (start..end.saturating_sub(n)).step_by(n / 4) {
            let input: Vec<_> = samples[offset..offset + n]
                .iter()
                .zip(&window)
                .map(|(x, w)| x * w)
                .collect();
            fft.forward(&input, &mut real, &mut imag, &mut scratch);
            for ((p, r), im) in power.iter_mut().zip(&real).zip(&imag) {
                *p += f64::from(r * r + im * im);
            }
        }
        let stats = |low: f64, high: f64| {
            let bins: Vec<_> = power
                .iter()
                .enumerate()
                .filter(|(i, _)| {
                    *i as f64 * rate / n as f64 >= low && *i as f64 * rate / n as f64 <= high
                })
                .collect();
            let total = bins.iter().map(|(_, p)| **p).sum::<f64>();
            let centroid = bins
                .iter()
                .map(|(i, p)| *i as f64 * rate / n as f64 * **p)
                .sum::<f64>()
                / total.max(1e-30);
            let flat = (bins.iter().map(|(_, p)| p.max(1e-30).ln()).sum::<f64>()
                / bins.len() as f64)
                .exp()
                / (total / bins.len() as f64).max(1e-30);
            (centroid, flat)
        };
        let all = stats(20., 20000.);
        let high = stats(2000., 16000.);
        let peak = power
            .iter()
            .enumerate()
            .filter(|(i, _)| (*i as f64 * rate / n as f64) < 500.)
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map_or(0., |(i, _)| i as f64 * rate / n as f64);
        let rms = |from: f64, to: f64| {
            let s = &samples[((from * rate) as usize).min(samples.len())
                ..((to * rate) as usize).min(samples.len())];
            (s.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / s.len().max(1) as f64).sqrt()
        };
        println!(
            "{arg}: low peak {peak:.1} Hz; centroid {:.1} Hz; flatness {:.4}; 2–16 kHz {:.4}; rms 30–60ms {:.4}, 150–180ms {:.4}",
            all.0,
            all.1,
            high.1,
            rms(0.03, 0.06),
            rms(0.15, 0.18)
        );
    }
    Ok(())
}
