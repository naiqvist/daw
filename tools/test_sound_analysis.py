"""Regression checks for the read-only mono/stereo sound analysis tool."""
import json
import math
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
import wave


class SoundAnalysis(unittest.TestCase):
    def test_mono_and_stereo_report_equal_level_but_not_fake_stereo_width(self):
        with tempfile.TemporaryDirectory(prefix="daw-sound-analysis-") as directory:
            reports = []
            for channels in (1, 2):
                path = Path(directory) / f"sine-{channels}.wav"
                with wave.open(str(path), "wb") as wav:
                    wav.setparams((channels, 2, 8000, 0, "NONE", "not compressed"))
                    samples = [round(8192 * math.sin(2 * math.pi * 250 * i / 8000))
                               for i in range(8000)]
                    wav.writeframes(b"".join(struct.pack("<h", sample) * channels
                                             for sample in samples))
                report = json.loads(subprocess.check_output([
                    sys.executable, str(Path(__file__).with_name("analyze-sound.py")), str(path)
                ], text=True))
                self.assertEqual(report["channels"], channels)
                self.assertAlmostEqual(report["sample_peak_dbfs"], -12.04, places=2)
                self.assertAlmostEqual(report["rms_dbfs"], -15.05, places=2)
                self.assertEqual(report["samples_at_or_above_full_scale"], 0)
                self.assertEqual(report["duration_s"], 1.0)
                reports.append(report)
            self.assertIsNone(reports[0]["stereo_correlation"])
            self.assertIsNone(reports[0]["side_to_mid_db"])
            self.assertEqual(reports[1]["stereo_correlation"], 1.0)
            self.assertEqual(reports[0]["envelope_half_second"], reports[1]["envelope_half_second"])
            self.assertEqual(reports[0]["last_half_second_rms_dbfs"], reports[1]["last_half_second_rms_dbfs"])


if __name__ == "__main__":
    unittest.main()
