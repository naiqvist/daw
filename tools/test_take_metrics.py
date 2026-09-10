import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('take_metrics', Path(__file__).with_name('take-metrics.py'))
metrics = importlib.util.module_from_spec(spec)
spec.loader.exec_module(metrics)


class TakeMetricsTests(unittest.TestCase):
    def test_text_is_not_one_keystroke(self):
        result = metrics.measure('# comment\nkey ctrl+shift+p\ntext param cutoff=1800\nkey Enter\nuntil ready\n')
        self.assertEqual(result['drive_commands'], 4)
        self.assertEqual(result['text_events'], 1)
        self.assertEqual(result['text_characters'], len('param cutoff=1800'))
        self.assertEqual(result['chords_plus_characters'], 2 + len('param cutoff=1800'))
        self.assertEqual(result['state_waits'], 1)
        self.assertEqual(result['fixed_frame_waits'], 0)

    def test_blank_lines_and_comments_are_not_actions(self):
        self.assertEqual(metrics.measure('\n # comment\n')['drive_commands'], 0)


if __name__ == '__main__':
    unittest.main()
