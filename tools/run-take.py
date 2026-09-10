import os
import pathlib
import subprocess
import sys
import time
import argparse

root = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description="Run an unbroken native TAKE; optional prepared project and bounded audition budget.")
parser.add_argument("script", nargs="?", type=pathlib.Path, default=root / "notes/take-idm-low-bass.drive")
parser.add_argument("--project", type=pathlib.Path)
parser.add_argument("--timeout", type=float, default=90)
args = parser.parse_args()
if not 1 <= args.timeout <= 3600:
    parser.error("--timeout must be 1..3600 seconds")
script = args.script.resolve()
if not script.is_file():
    parser.error("TAKE script does not exist")
if args.project and not args.project.is_file():
    parser.error("project does not exist")
run_id = time.time_ns()
fifo = pathlib.Path(f'/tmp/daw-take-{run_id}.fifo')
log_path = root / f'notes/{script.stem}-{run_id}.trace'
env = dict(os.environ, DAW_DRIVE=str(fifo))
with log_path.open('w') as log:
    command = [str(root / 'target/debug/stage')]
    if args.project:
        command.append(str(args.project.resolve()))
    app = subprocess.Popen(command, cwd=root, env=env,
                           stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    print(f'Take app PID {app.pid}; trace {log_path}', flush=True)
    deadline = time.monotonic() + 30
    while not fifo.exists():
        if app.poll() is not None or time.monotonic() > deadline:
            print('Launch failed', flush=True)
            sys.exit(1)
        time.sleep(0.05)
    started = time.monotonic()
    with fifo.open('w') as drive:
        drive.write(script.read_text())
    with log_path.open() as trace:
        while time.monotonic() - started < args.timeout:
            line = trace.readline()
            if not line:
                if app.poll() is not None:
                    print('App exited', flush=True)
                    sys.exit(1)
                time.sleep(0.01)
                continue
            if 'drive ' in line or 'stage:' in line:
                print(line.rstrip(), flush=True)
            if any(word in line for word in ['REFUSED', 'GAVE UP', 'panicked at', 'drive: no such']):
                app.terminate()
                print(f'TAKE FAILED after {time.monotonic() - started:.2f}s; app closed', flush=True)
                sys.exit(2)
            if 'TAKE COMPLETE' in line:
                print(f'TAKE FINISHED after {time.monotonic() - started:.2f}s', flush=True)
                sys.exit(0)
    app.terminate()
    print(f'TAKE LOCKED: {args.timeout:g}s without completion; app closed', flush=True)
    sys.exit(3)
