//! Family 10 of the kernel roadmap: FFT and spectral streaming primitives.
//!
//! [`RealFft`] performs an unnormalised forward real FFT and a normalised
//! inverse. [`FrameCutter`] turns arbitrary audio blocks into overlapping
//! frames, and [`OverlapAdd`] turns processed frames back into a stream.
//! [`Window`] supplies the usual analysis/synthesis windows, while
//! [`magnitude_phase`] and [`polar_to_cartesian`] bridge complex spectra and
//! display/effect-friendly polar values. All bulk storage is caller-owned.

/// A periodic window suitable for repeating FFT frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    Rectangular,
    Hann,
    SqrtHann,
    Hamming,
    Blackman,
}

/// Green zone: fill a caller-owned periodic window.
///
/// A one-sample window is unity. Periodic rather than symmetric definitions
/// are used because adjacent frames share endpoints; in particular, two
/// 50%-overlapped square-root Hann windows reconstruct at unity gain.
pub fn fill_window(kind: Window, window: &mut [f32]) {
    if window.len() == 1 {
        if let Some(value) = window.first_mut() {
            *value = 1.0;
        }
        return;
    }
    let size = window.len() as f32;
    for (index, value) in window.iter_mut().enumerate() {
        let phase = core::f32::consts::TAU * index as f32 / size;
        *value = match kind {
            Window::Rectangular => 1.0,
            Window::Hann => 0.5 - 0.5 * phase.cos(),
            Window::SqrtHann => (0.5 - 0.5 * phase.cos()).max(0.0).sqrt(),
            Window::Hamming => 0.54 - 0.46 * phase.cos(),
            Window::Blackman => 0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos(),
        };
    }
}

/// Stateless window multiply, truncating to the shortest slice.
///
/// State: none. Per-sample cost: 1 multiply. Denormal-safe: relies on engine
/// FTZ. In-place safe: no; use [`apply_window_in_place`]. Latency: 0 samples.
pub fn apply_window(input: &[f32], window: &[f32], output: &mut [f32]) {
    for ((sample, weight), out) in input.iter().zip(window.iter()).zip(output.iter_mut()) {
        *out = *sample * *weight;
    }
}

/// Apply a window in place, truncating to the shorter slice.
///
/// State: none. Per-sample cost: 1 multiply. Denormal-safe: relies on engine
/// FTZ. In-place safe: yes. Latency: 0 samples.
pub fn apply_window_in_place(io: &mut [f32], window: &[f32]) {
    for (sample, weight) in io.iter_mut().zip(window.iter()) {
        *sample *= *weight;
    }
}

/// Green zone: scale a synthesis window for unity overlap-add gain.
///
/// `phase_sums` is caller scratch of at least `hop` floats. The function
/// returns false without changing `synthesis` when sizes are inconsistent or
/// any overlap phase has zero/non-finite gain.
pub fn normalize_overlap(
    analysis: &[f32],
    synthesis: &mut [f32],
    hop: usize,
    phase_sums: &mut [f32],
) -> bool {
    if analysis.is_empty()
        || analysis.len() != synthesis.len()
        || hop == 0
        || hop > analysis.len()
        || phase_sums.len() < hop
    {
        return false;
    }
    for sum in phase_sums.iter_mut().take(hop) {
        *sum = 0.0;
    }
    for (index, (a, s)) in analysis.iter().zip(synthesis.iter()).enumerate() {
        if let Some(sum) = phase_sums.get_mut(index % hop) {
            *sum += *a * *s;
        }
    }
    if phase_sums
        .iter()
        .take(hop)
        .any(|sum| !sum.is_finite() || *sum == 0.0)
    {
        return false;
    }
    for (index, sample) in synthesis.iter_mut().enumerate() {
        if let Some(sum) = phase_sums.get(index % hop) {
            *sample /= *sum;
        }
    }
    true
}

/// Convert complex bins to magnitude and phase, truncating to the shortest
/// slice. Phase is in radians from `-pi..=pi`.
///
/// State: none. Per-bin cost: one hypot + one atan2. Denormal-safe: relies on
/// engine FTZ. In-place safe: outputs are distinct. Latency: 0 samples.
pub fn magnitude_phase(real: &[f32], imag: &[f32], magnitude: &mut [f32], phase: &mut [f32]) {
    for (((re, im), mag), angle) in real
        .iter()
        .zip(imag.iter())
        .zip(magnitude.iter_mut())
        .zip(phase.iter_mut())
    {
        *mag = re.hypot(*im);
        *angle = im.atan2(*re);
    }
}

/// Convert magnitude/phase bins back to Cartesian form, truncating to the
/// shortest slice.
///
/// State: none. Per-bin cost: one sin/cos pair + 2 multiplies.
/// Denormal-safe: relies on engine FTZ. In-place safe: outputs are distinct.
/// Latency: 0 samples.
pub fn polar_to_cartesian(magnitude: &[f32], phase: &[f32], real: &mut [f32], imag: &mut [f32]) {
    for (((mag, angle), re), im) in magnitude
        .iter()
        .zip(phase.iter())
        .zip(real.iter_mut())
        .zip(imag.iter_mut())
    {
        let (sin, cos) = angle.sin_cos();
        *re = *mag * cos;
        *im = *mag * sin;
    }
}

/// Streaming overlap-frame analysis over caller-owned ring storage.
///
/// Frames are written oldest-to-newest into consecutive `size()`-sample
/// chunks. The first frame is left-padded with `size - hop` zeros, making the
/// paired streaming latency exactly [`latency`](Self::latency). If `frames`
/// cannot hold the next complete frame, input consumption stops before that
/// frame would be due; the returned `(consumed_samples, produced_frames)`
/// tells the caller where to resume.
///
/// State: 32 bytes plus `storage_len()` caller storage. Per-sample cost: one
/// write, plus one N-sample copy every hop. Denormal-safe: copies input as-is.
/// In-place safe: input, frames, and storage must be distinct. Latency:
/// `size - hop` samples.
#[derive(Debug, Clone, Copy)]
pub struct FrameCutter {
    size: usize,
    hop: usize,
    write: usize,
    until_frame: usize,
}

impl Default for FrameCutter {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameCutter {
    pub const fn new() -> Self {
        Self {
            size: 0,
            hop: 0,
            write: 0,
            until_frame: 0,
        }
    }

    /// Green zone: configure a power-of-two FFT size and a hop in `1..=size`.
    pub fn prepare(&mut self, size: usize, hop: usize) -> bool {
        if !valid_size(size) || hop == 0 || hop > size {
            self.size = 0;
            self.hop = 0;
            self.write = 0;
            self.until_frame = 0;
            return false;
        }
        self.size = size;
        self.hop = hop;
        self.reset();
        true
    }

    /// Reset stream position. The caller must also clear the storage slice.
    pub fn reset(&mut self) {
        self.write = self.size.saturating_sub(self.hop);
        self.until_frame = self.hop;
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn hop(&self) -> usize {
        self.hop
    }

    pub fn storage_len(&self) -> usize {
        self.size
    }

    pub fn latency(&self) -> usize {
        self.size.saturating_sub(self.hop)
    }

    /// Red zone: consume arbitrary-length audio and emit complete frames.
    pub fn process(
        &mut self,
        input: &[f32],
        frames: &mut [f32],
        storage: &mut [f32],
    ) -> (usize, usize) {
        if self.size == 0 || storage.len() != self.size {
            return (0, 0);
        }
        let capacity = frames.len() / self.size;
        let mut frame_chunks = frames.chunks_exact_mut(self.size);
        let mut consumed = 0usize;
        let mut produced = 0usize;

        for sample in input.iter() {
            if self.until_frame == 1 && produced >= capacity {
                break;
            }
            if let Some(slot) = storage.get_mut(self.write) {
                *slot = *sample;
            } else {
                break;
            }
            self.write += 1;
            if self.write == self.size {
                self.write = 0;
            }
            self.until_frame -= 1;
            consumed += 1;

            if self.until_frame == 0
                && let Some(frame) = frame_chunks.next()
            {
                for (out, history) in frame
                    .iter_mut()
                    .zip(storage.iter().cycle().skip(self.write).take(self.size))
                {
                    *out = *history;
                }
                produced += 1;
                self.until_frame = self.hop;
            }
        }
        (consumed, produced)
    }
}

/// Streaming weighted overlap-add synthesis over caller-owned accumulation
/// storage.
///
/// Each complete `size()`-sample input frame produces exactly `hop()` output
/// samples. Frames are expected to have their synthesis window already
/// applied. Use [`normalize_overlap`] when the analysis/synthesis window pair
/// is not already constant-overlap-add.
///
/// State: 24 bytes plus `storage_len()` caller storage. Per output frame:
/// N additions + hop reads/clears. Denormal-safe: relies on engine FTZ.
/// In-place safe: frames, output, and storage must be distinct. Latency: 0
/// additional samples; [`FrameCutter`] reports the paired stream latency.
#[derive(Debug, Clone, Copy)]
pub struct OverlapAdd {
    size: usize,
    hop: usize,
    head: usize,
}

impl Default for OverlapAdd {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlapAdd {
    pub const fn new() -> Self {
        Self {
            size: 0,
            hop: 0,
            head: 0,
        }
    }

    /// Green zone: configure the same size/hop pair as the analysis cutter.
    pub fn prepare(&mut self, size: usize, hop: usize) -> bool {
        if !valid_size(size) || hop == 0 || hop > size {
            self.size = 0;
            self.hop = 0;
            self.head = 0;
            return false;
        }
        self.size = size;
        self.hop = hop;
        self.reset();
        true
    }

    /// Reset stream position. The caller must also clear the storage slice.
    pub fn reset(&mut self) {
        self.head = 0;
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn hop(&self) -> usize {
        self.hop
    }

    pub fn storage_len(&self) -> usize {
        self.size
    }

    pub fn latency(&self) -> usize {
        0
    }

    /// Red zone: add complete frames and emit hop-sized stream chunks.
    /// Returns `(consumed_frames, written_samples)`.
    pub fn process(
        &mut self,
        frames: &[f32],
        output: &mut [f32],
        storage: &mut [f32],
    ) -> (usize, usize) {
        if self.size == 0 || storage.len() != self.size {
            return (0, 0);
        }
        let count = (frames.len() / self.size).min(output.len() / self.hop);
        let mut frame_chunks = frames.chunks_exact(self.size);
        let mut output_chunks = output.chunks_exact_mut(self.hop);
        let mut completed = 0usize;

        for _ in 0..count {
            let Some(frame) = frame_chunks.next() else {
                break;
            };
            let Some(out) = output_chunks.next() else {
                break;
            };
            for (offset, sample) in frame.iter().enumerate() {
                let mut index = self.head + offset;
                if index >= self.size {
                    index -= self.size;
                }
                if let Some(sum) = storage.get_mut(index) {
                    *sum += *sample;
                }
            }
            for sample in out.iter_mut() {
                if let Some(sum) = storage.get_mut(self.head) {
                    *sample = *sum;
                    *sum = 0.0;
                }
                self.head += 1;
                if self.head == self.size {
                    self.head = 0;
                }
            }
            completed += 1;
        }
        (completed, completed * self.hop)
    }
}

/// Radix-2 real FFT with caller-owned interleaved complex scratch.
///
/// Forward output contains `N / 2 + 1` bins from DC through Nyquist. The
/// inverse reconstructs the conjugate half and divides by N, so a
/// forward/inverse round trip returns the input within floating-point error.
/// Buffer mismatches fail silent rather than partially transforming data.
///
/// State: 8 bytes. Cost: O(N log N), roughly 5·N·log2(N) arithmetic ops.
/// Denormal-safe: relies on engine FTZ; finite input remains finite for
/// practical transform sizes.
/// In-place safe: n/a — input/output are distinct and scratch is separate.
/// Latency: 0 samples (framing/overlap latency belongs to the caller).
#[derive(Debug, Clone, Copy)]
pub struct RealFft {
    size: usize,
}

impl Default for RealFft {
    fn default() -> Self {
        Self::new()
    }
}

impl RealFft {
    pub const fn new() -> Self {
        Self { size: 0 }
    }

    /// Green zone: select a power-of-two transform size. Returns false and
    /// leaves the kernel unprepared for zero, non-power-of-two, or sizes whose
    /// scratch length would overflow.
    pub fn prepare(&mut self, size: usize) -> bool {
        self.size = if Self::scratch_len(size) > 0 { size } else { 0 };
        self.size != 0
    }

    /// Zero state, keep the prepared size — the contract every other kernel
    /// honours. The transform is stateless (`forward`/`inverse` take `&self`),
    /// so there is no audio history to clear and this is a deliberate no-op:
    /// a node can reset every kernel it owns uniformly and this one keeps
    /// transforming. Un-preparing is `prepare`'s job, not `reset`'s.
    pub fn reset(&mut self) {}

    pub fn size(&self) -> usize {
        self.size
    }

    /// Number of non-redundant complex bins for a valid transform size.
    pub fn bins(size: usize) -> usize {
        if valid_size(size) { size / 2 + 1 } else { 0 }
    }

    /// Floats required for caller-owned interleaved complex scratch.
    pub fn scratch_len(size: usize) -> usize {
        if valid_size(size) {
            size.checked_mul(2).unwrap_or(0)
        } else {
            0
        }
    }

    /// Red zone: unnormalised real-input FFT.
    ///
    /// `input` must contain exactly N samples; `real` and `imag` must each
    /// hold at least `bins(N)` values; scratch must hold `scratch_len(N)`.
    pub fn forward(&self, input: &[f32], real: &mut [f32], imag: &mut [f32], scratch: &mut [f32]) {
        let bins = Self::bins(self.size);
        let needed = Self::scratch_len(self.size);
        if input.len() != self.size
            || real.len() < bins
            || imag.len() < bins
            || scratch.len() < needed
            || bins == 0
        {
            clear_pair(real, imag);
            return;
        }

        let work = &mut scratch[..needed];
        for (complex, sample) in work.as_chunks_mut::<2>().0.iter_mut().zip(input.iter()) {
            complex[0] = *sample;
            complex[1] = 0.0;
        }
        complex_fft(work, self.size, false);

        for ((out_re, out_im), complex) in real
            .iter_mut()
            .zip(imag.iter_mut())
            .take(bins)
            .zip(work.as_chunks::<2>().0.iter())
        {
            *out_re = complex[0];
            *out_im = complex[1];
        }
    }

    /// Red zone: normalised inverse from the non-redundant real spectrum.
    pub fn inverse(&self, real: &[f32], imag: &[f32], output: &mut [f32], scratch: &mut [f32]) {
        let bins = Self::bins(self.size);
        let needed = Self::scratch_len(self.size);
        if output.len() != self.size
            || real.len() < bins
            || imag.len() < bins
            || scratch.len() < needed
            || bins == 0
        {
            for sample in output.iter_mut() {
                *sample = 0.0;
            }
            return;
        }

        let work = &mut scratch[..needed];
        for complex in work.as_chunks_mut::<2>().0.iter_mut() {
            complex[0] = 0.0;
            complex[1] = 0.0;
        }
        for (complex, (bin_re, bin_im)) in work
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .take(bins)
            .zip(real.iter().zip(imag.iter()))
        {
            complex[0] = *bin_re;
            complex[1] = *bin_im;
        }
        // A real signal's DC and Nyquist bins cannot have imaginary parts.
        work[1] = 0.0;
        if self.size > 1 {
            work[self.size + 1] = 0.0;
        }
        for bin in 1..(self.size / 2) {
            let source = bin * 2;
            let mirror = (self.size - bin) * 2;
            work[mirror] = work[source];
            work[mirror + 1] = -work[source + 1];
        }

        complex_fft(work, self.size, true);
        let scale = 1.0 / self.size as f32;
        for (sample, complex) in output.iter_mut().zip(work.as_chunks::<2>().0.iter()) {
            *sample = complex[0] * scale;
        }
    }
}

fn valid_size(size: usize) -> bool {
    size.is_power_of_two() && size.checked_mul(2).is_some()
}

fn clear_pair(real: &mut [f32], imag: &mut [f32]) {
    for sample in real.iter_mut().chain(imag.iter_mut()) {
        *sample = 0.0;
    }
}

/// Iterative decimation-in-time complex FFT over interleaved `[re, im]`.
fn complex_fft(work: &mut [f32], size: usize, inverse: bool) {
    if size <= 1 {
        return;
    }
    let shift = usize::BITS - size.trailing_zeros();
    for index in 0..size {
        let reversed = index.reverse_bits() >> shift;
        if reversed > index {
            work.swap(index * 2, reversed * 2);
            work.swap(index * 2 + 1, reversed * 2 + 1);
        }
    }

    let mut width = 2usize;
    while width <= size {
        let angle = if inverse { 1.0 } else { -1.0 } * core::f32::consts::TAU / width as f32;
        let step_re = angle.cos();
        let step_im = angle.sin();
        for start in (0..size).step_by(width) {
            let mut twiddle_re = 1.0f32;
            let mut twiddle_im = 0.0f32;
            for offset in 0..(width / 2) {
                let even = (start + offset) * 2;
                let odd = (start + offset + width / 2) * 2;
                let odd_re = work[odd] * twiddle_re - work[odd + 1] * twiddle_im;
                let odd_im = work[odd] * twiddle_im + work[odd + 1] * twiddle_re;
                let even_re = work[even];
                let even_im = work[even + 1];
                work[even] = even_re + odd_re;
                work[even + 1] = even_im + odd_im;
                work[odd] = even_re - odd_re;
                work[odd + 1] = even_im - odd_im;
                let next_re = twiddle_re * step_re - twiddle_im * step_im;
                twiddle_im = twiddle_re * step_im + twiddle_im * step_re;
                twiddle_re = next_re;
            }
        }
        if width == size {
            break;
        }
        width *= 2;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ------------------------------------------------ spectral utilities ---

    #[test]
    fn periodic_windows_have_their_reference_values() {
        let mut hann = [0.0; 4];
        fill_window(Window::Hann, &mut hann);
        for (got, want) in hann.iter().zip([0.0, 0.5, 1.0, 0.5]) {
            assert!((*got - want).abs() < 1e-6, "Hann: {got} vs {want}");
        }

        let mut root = [0.0; 4];
        fill_window(Window::SqrtHann, &mut root);
        assert!(root.iter().zip(hann).all(|(r, h)| (r * r - h).abs() < 1e-6));

        let mut hamming = [0.0; 4];
        fill_window(Window::Hamming, &mut hamming);
        for (got, want) in hamming.iter().zip([0.08, 0.54, 1.0, 0.54]) {
            assert!((*got - want).abs() < 1e-6, "Hamming: {got} vs {want}");
        }

        let mut one = [0.0];
        fill_window(Window::Blackman, &mut one);
        assert_eq!(one, [1.0]);
        fill_window(Window::Rectangular, &mut one);
        assert_eq!(one, [1.0]);
    }

    #[test]
    fn overlap_normalization_makes_every_phase_unity() {
        const N: usize = 16;
        const H: usize = 4;
        let mut analysis = [0.0; N];
        let mut synthesis = [0.0; N];
        fill_window(Window::Hann, &mut analysis);
        fill_window(Window::Hamming, &mut synthesis);
        let mut sums = [0.0; H];
        assert!(normalize_overlap(&analysis, &mut synthesis, H, &mut sums));
        for phase in 0..H {
            let sum: f32 = analysis
                .iter()
                .zip(synthesis.iter())
                .skip(phase)
                .step_by(H)
                .map(|(a, s)| a * s)
                .sum();
            assert!((sum - 1.0).abs() < 1e-6, "phase {phase}: {sum}");
        }

        let before = synthesis;
        assert!(!normalize_overlap(&analysis, &mut synthesis, 0, &mut sums));
        assert_eq!(synthesis, before, "invalid setup must not mutate");
    }

    #[test]
    fn magnitude_and_phase_round_trip_reference_bins() {
        let real = [3.0, -1.0, 0.0];
        let imag = [4.0, 0.0, -2.0];
        let mut magnitude = [0.0; 3];
        let mut phase = [0.0; 3];
        magnitude_phase(&real, &imag, &mut magnitude, &mut phase);
        assert_eq!(magnitude[0], 5.0);
        assert!((phase[0] - 4.0f32.atan2(3.0)).abs() < 1e-6);
        assert!((phase[1].abs() - core::f32::consts::PI).abs() < 1e-6);
        assert!((phase[2] + core::f32::consts::FRAC_PI_2).abs() < 1e-6);

        let mut round_re = [0.0; 3];
        let mut round_im = [0.0; 3];
        polar_to_cartesian(&magnitude, &phase, &mut round_re, &mut round_im);
        assert!(
            real.iter()
                .zip(round_re)
                .all(|(a, b)| (*a - b).abs() < 1e-6)
        );
        assert!(
            imag.iter()
                .zip(round_im)
                .all(|(a, b)| (*a - b).abs() < 1e-6)
        );
    }

    #[test]
    fn windowed_fft_overlap_add_reconstructs_with_reported_latency() {
        const N: usize = 64;
        const H: usize = 32;
        const SAMPLES: usize = 256;
        let input: [f32; SAMPLES] = core::array::from_fn(|i| {
            0.6 * (i as f32 * 0.11).sin() + 0.2 * (i as f32 * 0.037).cos()
        });

        let mut cutter = FrameCutter::new();
        assert!(cutter.prepare(N, H));
        assert_eq!(cutter.latency(), N - H);
        let mut history = [0.0; N];
        let mut frames = [0.0; SAMPLES / H * N];
        let (consumed, produced) = cutter.process(&input, &mut frames, &mut history);
        assert_eq!((consumed, produced), (SAMPLES, SAMPLES / H));

        let mut window = [0.0; N];
        fill_window(Window::SqrtHann, &mut window);
        let mut fft = RealFft::new();
        assert!(fft.prepare(N));
        let mut bins_re = [0.0; N / 2 + 1];
        let mut bins_im = [0.0; N / 2 + 1];
        let mut fft_scratch = [0.0; N * 2];
        let mut time = [0.0; N];
        for frame in frames.as_chunks_mut::<N>().0.iter_mut().take(produced) {
            for (sample, weight) in frame.iter_mut().zip(window) {
                *sample *= weight;
            }
            fft.forward(frame, &mut bins_re, &mut bins_im, &mut fft_scratch);
            fft.inverse(&bins_re, &bins_im, &mut time, &mut fft_scratch);
            for ((sample, reconstructed), weight) in frame.iter_mut().zip(time).zip(window) {
                *sample = reconstructed * weight;
            }
        }

        let mut ola = OverlapAdd::new();
        assert!(ola.prepare(N, H));
        assert_eq!(ola.latency(), 0);
        let mut accumulation = [0.0; N];
        let mut output = [0.0; SAMPLES];
        let result = ola.process(&frames, &mut output, &mut accumulation);
        assert_eq!(result, (produced, produced * H));
        assert!(output[..N - H].iter().all(|sample| sample.abs() < 1e-6));
        for (got, want) in output[N - H..].iter().zip(input.iter()) {
            assert!((*got - *want).abs() < 3e-5, "{got} vs {want}");
        }
    }

    #[test]
    fn streaming_is_split_block_bit_exact() {
        const N: usize = 64;
        const H: usize = 16;
        const SAMPLES: usize = 256;
        let input: [f32; SAMPLES] = core::array::from_fn(|i| (i as f32 * 0.13).sin());

        let cut = |split: bool| {
            let mut cutter = FrameCutter::new();
            cutter.prepare(N, H);
            let mut storage = [0.0; N];
            let mut frames = [0.0; SAMPLES / H * N];
            if split {
                let (used_a, made_a) = cutter.process(&input[..100], &mut frames, &mut storage);
                let offset = made_a * N;
                let (used_b, made_b) =
                    cutter.process(&input[100..], &mut frames[offset..], &mut storage);
                assert_eq!((used_a + used_b, made_a + made_b), (SAMPLES, SAMPLES / H));
            } else {
                assert_eq!(
                    cutter.process(&input, &mut frames, &mut storage),
                    (SAMPLES, SAMPLES / H)
                );
            }
            frames
        };
        let whole_frames = cut(false);
        let split_frames = cut(true);
        assert!(
            whole_frames
                .iter()
                .zip(split_frames)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );

        let synth = |split: bool| {
            let mut ola = OverlapAdd::new();
            ola.prepare(N, H);
            let mut storage = [0.0; N];
            let mut output = [0.0; SAMPLES];
            if split {
                let first_frames = 7;
                assert_eq!(
                    ola.process(
                        &whole_frames[..first_frames * N],
                        &mut output[..first_frames * H],
                        &mut storage,
                    ),
                    (first_frames, first_frames * H)
                );
                assert_eq!(
                    ola.process(
                        &whole_frames[first_frames * N..],
                        &mut output[first_frames * H..],
                        &mut storage,
                    ),
                    (SAMPLES / H - first_frames, SAMPLES - first_frames * H)
                );
            } else {
                assert_eq!(
                    ola.process(&whole_frames, &mut output, &mut storage),
                    (SAMPLES / H, SAMPLES)
                );
            }
            output
        };
        let whole_output = synth(false);
        let split_output = synth(true);
        assert!(
            whole_output
                .iter()
                .zip(split_output)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
    }

    #[test]
    fn spectral_streaming_process_paths_do_not_allocate() {
        const N: usize = 64;
        const H: usize = 32;
        let mut window = [0.0; N];
        fill_window(Window::SqrtHann, &mut window);
        let input = [0.25; N];
        let mut windowed = [0.0; N];
        let mut cutter = FrameCutter::new();
        cutter.prepare(N, H);
        let mut history = [0.0; N];
        let mut frames = [0.0; N * 2];
        let mut ola = OverlapAdd::new();
        ola.prepare(N, H);
        let mut accumulation = [0.0; N];
        let mut output = [0.0; H * 2];
        let real = [0.3; N / 2 + 1];
        let imag = [0.4; N / 2 + 1];
        let mut magnitude = [0.0; N / 2 + 1];
        let mut phase = [0.0; N / 2 + 1];
        let mut round_re = [0.0; N / 2 + 1];
        let mut round_im = [0.0; N / 2 + 1];

        assert_no_alloc::assert_no_alloc(|| {
            apply_window(&input, &window, &mut windowed);
            apply_window_in_place(&mut windowed, &window);
            let (_, made) = cutter.process(&input, &mut frames, &mut history);
            let _ = ola.process(&frames[..made * N], &mut output, &mut accumulation);
            magnitude_phase(&real, &imag, &mut magnitude, &mut phase);
            polar_to_cartesian(&magnitude, &phase, &mut round_re, &mut round_im);
        });
    }

    #[test]
    fn streaming_accepts_edges_and_bad_storage_fails_inert() {
        let mut cutter = FrameCutter::new();
        assert!(!cutter.prepare(0, 0));
        assert!(!cutter.prepare(7, 3));
        assert!(cutter.prepare(8, 4));
        let mut storage = [0.0; 8];
        let mut frames = [0.0; 16];
        assert_eq!(cutter.process(&[], &mut frames, &mut storage), (0, 0));
        assert_eq!(cutter.process(&[1.0], &mut frames, &mut storage), (1, 0));
        assert_eq!(cutter.process(&[2.0; 7], &mut frames, &mut storage), (7, 2));
        assert_eq!(cutter.process(&[3.0], &mut frames, &mut [0.0; 7]), (0, 0));

        let mut one = FrameCutter::new();
        assert!(one.prepare(1, 1));
        let mut one_storage = [0.0];
        let mut one_frame = [0.0];
        assert_eq!(
            one.process(&[0.75], &mut one_frame, &mut one_storage),
            (1, 1)
        );
        assert_eq!(one_frame, [0.75]);

        let mut ola = OverlapAdd::new();
        assert!(!ola.prepare(8, 0));
        assert!(ola.prepare(8, 4));
        let mut output = [9.0; 4];
        assert_eq!(
            ola.process(&frames[..8], &mut output, &mut [0.0; 7]),
            (0, 0)
        );
        assert_eq!(output, [9.0; 4]);
    }

    #[test]
    fn streaming_silence_and_denormals_stay_finite() {
        const N: usize = 32;
        const H: usize = 8;
        let mut cutter = FrameCutter::new();
        cutter.prepare(N, H);
        let mut history = [0.0; N];
        let mut frames = [0.0; N * 4];
        let silence = [0.0; H * 4];
        let (_, made) = cutter.process(&silence, &mut frames, &mut history);
        assert!(frames.iter().all(|sample| *sample == 0.0));

        if let Some(last) = frames.last_mut() {
            *last = f32::from_bits(1);
        }
        let mut ola = OverlapAdd::new();
        ola.prepare(N, H);
        let mut accumulation = [0.0; N];
        let mut output = [0.0; H * 4];
        let result = ola.process(&frames[..made * N], &mut output, &mut accumulation);
        assert_eq!(result, (made, made * H));
        assert!(
            output
                .iter()
                .chain(accumulation.iter())
                .all(|sample| sample.is_finite())
        );
    }

    #[test]
    fn impulse_and_sine_match_the_analytic_spectrum() {
        const N: usize = 64;
        let mut fft = RealFft::new();
        assert!(fft.prepare(N));
        let mut scratch = [0.0; N * 2];
        let mut real = [0.0; N / 2 + 1];
        let mut imag = [0.0; N / 2 + 1];
        let mut impulse = [0.0; N];
        impulse[0] = 1.0;
        fft.forward(&impulse, &mut real, &mut imag, &mut scratch);
        assert!(real.iter().all(|value| (*value - 1.0).abs() < 1e-6));
        assert!(imag.iter().all(|value| value.abs() < 1e-6));

        let input: [f32; N] =
            core::array::from_fn(|i| (core::f32::consts::TAU * 5.0 * i as f32 / N as f32).sin());
        fft.forward(&input, &mut real, &mut imag, &mut scratch);
        assert!((imag[5].abs() - N as f32 * 0.5).abs() < 1e-3);
        assert!(real[5].abs() < 1e-3);
    }

    #[test]
    fn forward_inverse_round_trip() {
        const N: usize = 256;
        let mut fft = RealFft::new();
        fft.prepare(N);
        let input: [f32; N] = core::array::from_fn(|i| {
            ((i as f32) * 0.137).sin() * 0.7 + ((i as f32) * 0.031).cos() * 0.2
        });
        let mut real = [0.0; N / 2 + 1];
        let mut imag = [0.0; N / 2 + 1];
        let mut scratch = [0.0; N * 2];
        fft.forward(&input, &mut real, &mut imag, &mut scratch);
        let mut output = [0.0; N];
        fft.inverse(&real, &imag, &mut output, &mut scratch);
        assert!(input.iter().zip(output).all(|(a, b)| (*a - b).abs() < 2e-5));
    }

    #[test]
    fn repeated_frames_are_bit_exact_and_stateless() {
        const N: usize = 256;
        let mut fft = RealFft::new();
        fft.prepare(N);
        let input: [f32; N] = core::array::from_fn(|i| ((i as f32) * 0.1).sin());
        let mut a_re = [0.0; N / 2 + 1];
        let mut a_im = [0.0; N / 2 + 1];
        let mut b_re = [0.0; N / 2 + 1];
        let mut b_im = [0.0; N / 2 + 1];
        let mut scratch = [0.0; N * 2];
        fft.forward(&input, &mut a_re, &mut a_im, &mut scratch);
        fft.forward(&input, &mut b_re, &mut b_im, &mut scratch);
        assert!(
            a_re.iter()
                .zip(b_re)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
        assert!(
            a_im.iter()
                .zip(b_im)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
    }

    /// A transport discontinuity resets every kernel a node owns. This one
    /// is stateless, so the reset must be invisible: same size, bit-exact
    /// same spectrum. It used to un-prepare itself here and emit silence
    /// for the rest of the render.
    #[test]
    fn reset_keeps_the_prepared_size_and_output() {
        const N: usize = 64;
        let mut fft = RealFft::new();
        assert!(fft.prepare(N));
        let input: [f32; N] = core::array::from_fn(|i| ((i as f32) * 0.3).sin());
        let mut before_re = [0.0; N / 2 + 1];
        let mut before_im = [0.0; N / 2 + 1];
        let mut after_re = [0.0; N / 2 + 1];
        let mut after_im = [0.0; N / 2 + 1];
        let mut scratch = [0.0; N * 2];
        fft.forward(&input, &mut before_re, &mut before_im, &mut scratch);

        fft.reset();

        assert_eq!(fft.size(), N, "reset must not un-prepare the transform");
        fft.forward(&input, &mut after_re, &mut after_im, &mut scratch);
        assert!(
            before_re
                .iter()
                .zip(after_re)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
        assert!(
            before_im
                .iter()
                .zip(after_im)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );

        // And the inverse still reconstructs rather than writing zeros.
        let mut output = [0.0; N];
        fft.inverse(&after_re, &after_im, &mut output, &mut scratch);
        assert!(output.iter().any(|s| s.abs() > 1e-3), "inverse went silent");
        assert!(input.iter().zip(output).all(|(a, b)| (*a - b).abs() < 2e-5));
    }

    #[test]
    fn transform_does_not_allocate() {
        const N: usize = 64;
        let mut fft = RealFft::new();
        fft.prepare(N);
        let input = [0.5; N];
        let mut output = [0.0; N];
        let mut real = [0.0; N / 2 + 1];
        let mut imag = [0.0; N / 2 + 1];
        let mut scratch = [0.0; N * 2];
        assert_no_alloc::assert_no_alloc(|| {
            fft.forward(&input, &mut real, &mut imag, &mut scratch);
            fft.inverse(&real, &imag, &mut output, &mut scratch);
        });
    }

    #[test]
    fn edge_sizes_and_bad_buffers_fail_silent() {
        let mut fft = RealFft::new();
        assert!(!fft.prepare(0));
        assert!(!fft.prepare(7));
        let mut real = [1.0; 5];
        let mut imag = [1.0; 5];
        fft.forward(&[1.0; 7], &mut real, &mut imag, &mut []);
        assert!(real.iter().chain(&imag).all(|value| *value == 0.0));

        assert!(fft.prepare(1));
        let mut scratch = [0.0; 2];
        fft.forward(&[0.25], &mut real[..1], &mut imag[..1], &mut scratch);
        assert_eq!(real[0], 0.25);
        let mut output = [0.0; 1];
        fft.inverse(&real[..1], &imag[..1], &mut output, &mut scratch);
        assert_eq!(output[0], 0.25);

        fft.prepare(8);
        real.fill(1.0);
        imag.fill(1.0);
        fft.forward(&[0.0; 7], &mut real, &mut imag, &mut [0.0; 16]);
        assert!(real.iter().chain(&imag).all(|value| *value == 0.0));
    }

    #[test]
    fn silence_and_denormal_input_stay_finite() {
        const N: usize = 128;
        let mut fft = RealFft::new();
        fft.prepare(N);
        let mut input = [0.0; N];
        input[N - 1] = f32::from_bits(1);
        let mut real = [0.0; N / 2 + 1];
        let mut imag = [0.0; N / 2 + 1];
        let mut scratch = [0.0; N * 2];
        fft.forward(&input, &mut real, &mut imag, &mut scratch);
        assert!(real.iter().chain(&imag).all(|value| value.is_finite()));
        input.fill(9.0);
        fft.forward(&[0.0; N], &mut real, &mut imag, &mut scratch);
        assert!(real.iter().chain(&imag).all(|value| *value == 0.0));
    }
}
