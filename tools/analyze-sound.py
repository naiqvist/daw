"""Read-only sound-design measurements; ffmpeg/ffprobe plus Python stdlib.

Usage: python tools/analyze-sound.py render.wav
No normalization or audio rewriting. JSON is printed for a take receipt.
"""
import array
import cmath
import json
import math
import statistics
import subprocess
import sys

path = sys.argv[1]
probe = json.loads(subprocess.check_output([
    "ffprobe", "-v", "error", "-show_streams", "-show_format", "-of", "json", path
]))
stream = next(s for s in probe["streams"] if s["codec_type"] == "audio")
rate = int(stream["sample_rate"])
channels = int(stream["channels"])
if channels not in (1, 2):
    raise SystemExit("Sound analysis currently accepts mono or stereo audio.")
raw = subprocess.check_output([
    "ffmpeg", "-v", "error", "-i", path, "-f", "f32le", "-acodec", "pcm_f32le", "-"
])
samples = array.array("f", raw)
if sys.byteorder != "little":
    samples.byteswap()
if not all(math.isfinite(x) for x in samples):
    raise SystemExit("FAIL: non-finite samples")
left, right = (samples[::2], samples[1::2]) if channels == 2 else (samples, samples)
frames = len(left)
if frames == 0:
    raise SystemExit("FAIL: empty audio")
db = lambda x: 20 * math.log10(max(x, 1e-15))
energy_l = sum(x*x for x in left)
energy_r = sum(x*x for x in right)
cross = sum(l*r for l, r in zip(left, right))
mid_energy = (energy_l + energy_r + 2*cross) / 4
side_energy = max(0, (energy_l + energy_r - 2*cross) / 4)
envelope = []
block = rate // 2
for start in range(0, frames, block):
    stop = min(start+block, frames)
    energy = sum(x*x for x in samples[start*channels:stop*channels]) / ((stop-start)*channels)
    envelope.append({"time_s": round(start/rate, 3), "rms_dbfs": round(db(math.sqrt(energy)), 2)})

def fft(values):
    n = len(values)
    if n == 1:
        return values
    even, odd = fft(values[::2]), fft(values[1::2])
    rot = [cmath.exp(-2j*math.pi*k/n)*odd[k] for k in range(n//2)]
    return [even[k]+rot[k] for k in range(n//2)] + [even[k]-rot[k] for k in range(n//2)]

spectra = []
nfft = 8192
for second in range(2, min(11, frames//rate)):
    start = second*rate
    if start+nfft > frames:
        break
    window = [(left[start+i]+right[start+i])*0.5*(0.5-0.5*math.cos(2*math.pi*i/(nfft-1))) for i in range(nfft)]
    power = [abs(z)**2 for z in fft(window)[:nfft//2]]
    total = sum(power)
    centroid = sum(k*rate/nfft*p for k,p in enumerate(power))/max(total,1e-30)
    spectra.append({"time_s": second, "power_centroid_hz": round(centroid,1),
                    "energy_above_5khz_pct": round(100*sum(power[math.ceil(5000*nfft/rate):])/max(total,1e-30),3)})

report = {
    "file": path, "sample_rate": rate, "channels": channels,
    "bits_per_sample": stream.get("bits_per_sample"), "duration_s": frames/rate,
    "sample_peak_dbfs": round(db(max(abs(x) for x in samples)),2),
    "rms_dbfs": round(db(math.sqrt((energy_l+energy_r)/(2*frames))),2),
    "samples_at_or_above_full_scale": sum(abs(x)>=1 for x in samples),
    "stereo_correlation": round(cross/max(math.sqrt(energy_l*energy_r),1e-30),4) if channels == 2 else None,
    "side_to_mid_db": round(10*math.log10(max(side_energy,1e-30)/max(mid_energy,1e-30)),2) if channels == 2 else None,
    "last_half_second_rms_dbfs": round(db(math.sqrt(sum(x*x for x in samples[-(rate//2)*channels:])/min((rate//2)*channels,len(samples)))),2),
    "envelope_half_second": envelope,
    "spectrum_hann_8192_mono_power_weighted": spectra,
    "interpretation_limits": "Spectral movement includes oscillator/ensemble beating, not only filtering. Whole-file RMS includes the tail. Measurements do not establish aesthetic acceptance."
}
print(json.dumps(report, indent=2))
