#!/usr/bin/env python3
"""Honest input accounting for TAKE recipes; text events are not single keys."""
import argparse
import json
from pathlib import Path


def measure(text):
    lines = [line.strip() for line in text.splitlines()
             if line.strip() and not line.lstrip().startswith('#')]
    keys = [line[4:] for line in lines if line.startswith('key ')]
    entries = [line[5:] for line in lines if line.startswith('text ')]
    characters = sum(map(len, entries))
    return {
        'drive_commands': len(lines),
        'key_chords': len(keys),
        'text_events': len(entries),
        'text_characters': characters,
        'chords_plus_characters': len(keys) + characters,
        'state_waits': sum(line.startswith(('until ', 'gone ')) for line in lines),
        'fixed_frame_waits': sum(line.startswith('wait ') for line in lines),
        'palette_opens': sum(key.lower() == 'ctrl+shift+p' for key in keys),
        'arrow_chords': sum('arrow' in key.lower() for key in keys),
        'parameter_page_keys': sum(key.upper() in ('F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8') for key in keys),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('scripts', nargs='+', type=Path)
    args = parser.parse_args()
    results = [{'path': str(path), **measure(path.read_text())} for path in args.scripts]
    report = {'scripts': results,
              'caveat': 'Chords-plus-characters is a rough input-effort proxy, not physical keystrokes or cognitive effort. Audition audio time is not UI overhead.'}
    if len(results) == 2:
        report['reduction_percent'] = {
            key: (100 * (1 - results[1][key] / results[0][key]) if results[0][key] else None)
            for key in results[0] if key != 'path'
        }
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
