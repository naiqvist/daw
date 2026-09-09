#!/usr/bin/env python3
"""Analyze examples/coverage.rs output using Python's standard library only.

Writes matrix.csv/json, continuity.json, agency.json, hulls.json, report.md,
and coverage.html. `--accept` is the deliberately strict, standalone
coverage_matrix_meets_the_plan check. A smoke run produces useful reports
but cannot certify the plan. Hull interiors NEVER create reached cells.
"""
from __future__ import annotations

import argparse
import csv
import html
import json
import math
import shutil
from collections import defaultdict
from pathlib import Path
import sys

Y_EDGES = (30, 60, 120, 240, 480, 960, 1920, 3840, 7680, 16000)
CLASSES = ("SUSTAINED", "DECAYING", "TRANSIENT")
SILENCE = 1e-7


def apply_metric(row: dict, prefix: str, metric: str) -> bool:
    """Derive a declared analysis axis without rewriting raw input CSVs."""
    if metric == "legacy":
        return False
    stem = f"{prefix}_" if prefix else ""
    recovered = f"{stem}mean_semitones" not in row
    if recovered:
        legacy = float(row[f"{stem}inharmonicity"])
        if legacy >= 1:
            raise ValueError("cannot recover raw semitone deviation from a clipped legacy value; rerender or use --metric legacy")
        deviation = legacy * 6
    else:
        deviation = float(row[f"{stem}mean_semitones"])
    normalized = deviation / (deviation + .25)
    row[f"{stem}legacy_x"] = row[f"{stem}x"]
    row[f"{stem}x"] = str(.6 * normalized + .4 * float(row[f"{stem}flatness"]))
    row[f"{stem}inharmonicity"] = str(normalized)
    return recovered


def exception(x: int, y: int, time: str) -> str | None:
    """The complete, explicit §7 list, applied to the declared grid."""
    if time == "TRANSIENT" and x < 2:
        return "§7: harmonic transient is a click; not counted"
    if x == 0 and y == 8:
        return "§7: pure high sine is a test tone; top bin is 7680–16000 Hz"
    if time == "TRANSIENT" and x == 9 and y == 0:
        return "§7: sub-bass noise transient not required"
    if time == "SUSTAINED" and 4 <= x <= 6 and y == 0:
        return "§7: inharmonic sub sustain deliberately excluded"
    return None


def read_csv(path: Path) -> list[dict]:
    with path.open(newline="") as stream:
        return list(csv.DictReader(stream))


def merge_runs(destination: Path, sources: list[Path]) -> None:
    """Join disjoint-machine runs without fabricating a shared measurement."""
    if any(destination.resolve() == source.resolve() for source in sources):
        raise ValueError("merge output must be separate from every input directory")
    manifests = [{r["key"]: r["value"] for r in read_csv(source / "run.csv")} for source in sources]
    comparable = [{k:v for k,v in manifest.items() if k not in ("started_unix", "executable_modified_unix")} for manifest in manifests]
    if not comparable or any(manifest != comparable[0] for manifest in comparable[1:]):
        raise ValueError("merge requires identical executable checksum, sampling, seed and measurement settings")
    names = [m["machine"] for source in sources for m in read_csv(source / "machines.csv")]
    if len(names) != len(set(names)):
        raise ValueError("merge requires disjoint machine sets; repeated machines must be rerun together")
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(sources[0] / "run.csv", destination / "run.csv")
    write_json(destination / "merge_sources.json", [{"directory": str(source), "run": manifest}
                                                    for source, manifest in zip(sources, manifests)])
    for name in ("renders.csv", "vectors.csv", "machines.csv", "timings.csv"):
        header = None
        with (destination / name).open("w", newline="") as out:
            writer = csv.writer(out)
            for source in sources:
                with (source / name).open(newline="") as stream:
                    reader = csv.reader(stream)
                    current = next(reader)
                    if header is None:
                        header = current
                        writer.writerow(header)
                    elif current != header:
                        raise ValueError(f"inconsistent {name} schema")
                    writer.writerows(reader)
    for source in sources:
        for machine in read_csv(source / "machines.csv"):
            name = f"locks-{machine['machine']}.csv"
            shutil.copyfile(source / name, destination / name)


def subset_run(destination: Path, source: Path, names: list[str]) -> None:
    """Retain actual rows and parent IDs from a completed machine subset."""
    if destination.resolve() == source.resolve():
        raise ValueError("subset output must be separate from its source")
    if not names or len(names) != len(set(names)):
        raise ValueError("subset requires distinct nonempty machine names")
    available = {row["machine"] for row in read_csv(source / "machines.csv")}
    if set(names) - available:
        raise ValueError(f"subset machines not in source: {sorted(set(names) - available)}")
    for name in names:
        if not (source / f"locks-{name}.csv").is_file():
            raise ValueError(f"subset machine has not completed lock measurements: {name}")
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source / "run.csv", destination / "run.csv")
    for name in ("renders.csv", "vectors.csv", "machines.csv", "timings.csv"):
        with (source / name).open(newline="") as incoming, (destination / name).open("w", newline="") as outgoing:
            reader = csv.DictReader(incoming)
            writer = csv.DictWriter(outgoing, fieldnames=reader.fieldnames)
            writer.writeheader()
            writer.writerows(row for row in reader if row["machine"] in names)
    for name in names:
        shutil.copyfile(source / f"locks-{name}.csv", destination / f"locks-{name}.csv")
    if (source / "calibration.csv").is_file():
        shutil.copyfile(source / "calibration.csv", destination / "calibration.csv")
    write_json(destination / "source_subset.json", {
        "kind": "measured_machine_subset", "source_directory": str(source.resolve()),
        "machines": names, "run_manifest_preserved": True,
        "vector_ids_and_parent_witnesses_preserved": True,
    })


def cell_id(cell: tuple[int, int, str]) -> str:
    x, y, time = cell
    return f"{time}:{x}:{y}"


def xy(x: float, centroid: float, rms: float) -> tuple[int, int] | None:
    if not all(map(math.isfinite, (x, centroid, rms))) or rms < SILENCE:
        return None
    if not 0 <= x <= 1 or not Y_EDGES[0] <= centroid <= Y_EDGES[-1]:
        return None
    xb = min(9, int(x * 10))
    yb = next((i for i in range(9) if centroid < Y_EDGES[i + 1]), 8)
    return xb, yb


def position(row: dict, prefix: str | None = None) -> tuple[int, int] | None:
    if row.get("valid", "true") != "true":
        return None
    prefix = prefix or ("held" if row["time_class"] == "SUSTAINED" else "attack")
    return xy(*(float(row[f"{prefix}_{field}"]) for field in ("x", "centroid", "rms")))


def get_cell(row: dict) -> tuple[int, int, str] | None:
    if row["time_class"] not in CLASSES:
        return None
    pos = position(row)
    return (*pos, row["time_class"]) if pos else None


def adjacent(a: tuple[int, int] | None, b: tuple[int, int] | None) -> bool:
    # T is a separate axis, without a declared neighborhood metric. A fine
    # envelope turn may therefore change T after the macro landed in X/Y.
    return a is not None and b is not None and max(abs(a[0] - b[0]), abs(a[1] - b[1])) <= 1


def key(row: dict, vector: str | None = None) -> tuple[str, str, str, str]:
    return row["machine"], vector or row["vector"], row["note"], row["velocity"]


def witness(row: dict) -> dict:
    return {"vector": int(row["vector"]), "source": row["source"],
            "note": int(row["note"]), "velocity": int(row["velocity"]),
            "turns": int(row["turns"]), "cost5": int(row["cost5"]),
            "distance": float(row["distance"]),
            "macro_parent": int(row["parent"]) if row["parent"] else None}


def convex_hull(points: list[tuple[float, float]]) -> list[tuple[float, float]]:
    """Monotonic-chain hull in X/log2(Hz), solely for visualization."""
    points = sorted(set(points))
    if len(points) <= 1:
        return points
    def cross(o, a, b):
        return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    lower = []
    for p in points:
        while len(lower) >= 2 and cross(lower[-2], lower[-1], p) <= 0:
            lower.pop()
        lower.append(p)
    upper = []
    for p in reversed(points):
        while len(upper) >= 2 and cross(upper[-2], upper[-1], p) <= 0:
            upper.pop()
        upper.append(p)
    return lower[:-1] + upper[:-1]


def continuity_report(rows: list[dict], machines: dict, steps: int, source: str = "walk") -> dict:
    groups = defaultdict(list)
    for row in rows:
        if row["source"] == source:
            groups[row["machine"], row["y_setting"], row["note"], row["velocity"]].append(row)
    report = {}
    for machine in machines:
        sweeps = []
        for group, points in groups.items():
            if group[0] != machine:
                continue
            points.sort(key=lambda p: int(p["walk_step"]))
            failures = []
            if [int(p["walk_step"]) for p in points] != list(range(steps)):
                failures.append({"reason": "missing or duplicate step"})
            previous = None
            path = []
            for point in points:
                pos = position(point)
                prefix = "held" if point["time_class"] == "SUSTAINED" else "attack"
                x = float(point[f"{prefix}_x"])
                centroid = float(point[f"{prefix}_centroid"])
                step = int(point["walk_step"])
                path.append({"step": step, "x": x, "centroid": centroid, "cell": pos,
                             "time_class": point["time_class"], "vector": int(point["vector"])})
                if pos is None:
                    failures.append({"step": step, "reason": "silent, invalid, or outside measured Y range"})
                elif previous is not None and abs(pos[0] - previous[0]) > 1:
                    failures.append({"step": step, "reason": "X jumps more than one bin",
                                     "from": previous[0], "to": pos[0]})
                previous = pos
            sweeps.append({"y_setting": int(group[1]), "note": int(group[2]),
                           "velocity": int(group[3]), "pass": not failures,
                           "failures": failures, "path": path})
        report[machine] = {"required": source == "walk" and machines[machine]["band_required"] == "true",
                           "complete": len(sweeps) == 18 and steps >= 20,
                           "pass": len(sweeps) == 18 and all(s["pass"] for s in sweeps),
                           "sweeps": sweeps}
    return report


def agency_report(rows: list[dict], machines: dict, directory: Path, metric: str) -> dict:
    starters = {(r["machine"], r["note"], r["velocity"]): r for r in rows if r["source"] == "starter"}
    report = {}
    for machine in machines:
        moves = []
        for row in rows:
            if row["machine"] != machine or row["source"] != "macro":
                continue
            start = starters.get((machine, row["note"], row["velocity"]))
            a, b = position(start) if start else None, position(row)
            if a and b and a != b:
                moves.append(witness(row))
        locks = []
        lockpath = directory / f"locks-{machine}.csv"
        if lockpath.exists():
            groups = defaultdict(dict)
            for row in read_csv(lockpath):
                apply_metric(row, "", metric)
                group = (row["walker"], row.get("trial", "legacy"), row.get("note", "48"), row.get("velocity", "100"))
                groups[group][row["hit"]] = row
            for (walker, trial, note, velocity), hits in groups.items():
                positions = {hit: xy(*(float(row[f]) for f in ("x", "centroid", "rms")))
                             for hit, row in hits.items()}
                first, locked, restored = (positions.get(h) for h in ("base", "locked", "restored"))
                locks.append({"walker": int(walker), "trial": trial, "note": int(note), "velocity": int(velocity), "positions": positions,
                              "moves_cell": first is not None and locked is not None and first != locked,
                              "returns_cell": first is not None and first == restored})
        report[machine] = {"macro_moves_cell": bool(moves), "macro_witness": moves[0] if moves else None,
                           "locks": locks,
                           "lock_pass": any(p["moves_cell"] and p["returns_cell"] for p in locks),
                           "picture_test": "must be checked by the instrument's Rust hero tests; not inferred here"}
    return report


def analyze(directory: Path, metric: str = "25cent") -> dict:
    rows = read_csv(directory / "renders.csv")
    recovered_windows = 0
    for row in rows:
        for prefix in ("attack", "held", "post_release"):
            if f"{prefix}_x" in row:
                recovered_windows += apply_metric(row, prefix, metric)
    machine_rows = read_csv(directory / "machines.csv")
    machines = {m["machine"]: m for m in machine_rows}
    run = {r["key"]: r["value"] for r in read_csv(directory / "run.csv")}
    indexed = {key(r): r for r in rows}
    lock_trajectories = sum(len(read_csv(directory / f"locks-{machine}.csv")) for machine in machines
                            if (directory / f"locks-{machine}.csv").exists())
    integrity = []
    if len(indexed) != len(rows):
        integrity.append("duplicate render key")
    for machine in machines:
        condition_sets = defaultdict(set)
        source_vectors = defaultdict(set)
        for row in rows:
            if row["machine"] == machine:
                condition_sets[row["vector"]].add((int(row["note"]), int(row["velocity"])))
                source_vectors[row["source"]].add(row["vector"])
        expected = {(n, v) for n in (36, 48, 60) for v in (64, 127)}
        if not condition_sets or any(conditions != expected for conditions in condition_sets.values()):
            integrity.append(f"{machine}: incomplete note/velocity conditions")
        expected_sources = {"lhs": int(run["lhs_vectors"]), "probe": int(run["fine_probes"]),
                            "starter": 1, "macro": 15, "walk": 3 * int(run["walk_steps"])}
        if machine == "glass":
            expected_sources["ratio_walk"] = 3 * int(run["walk_steps"])
        for source, count in expected_sources.items():
            if len(source_vectors[source]) != count:
                integrity.append(f"{machine}: expected {count} {source} vectors, found {len(source_vectors[source])}")
        if "lock_trajectories_per_machine" in run:
            path = directory / f"locks-{machine}.csv"
            measured_locks = len(read_csv(path)) if path.exists() else 0
            expected_locks = int(run["lock_trajectories_per_machine"])
            if measured_locks != expected_locks:
                integrity.append(f"{machine}: expected {expected_locks} lock trajectories, found {measured_locks}")
    seen = defaultdict(lambda: defaultdict(list))
    clouds = defaultdict(list)
    invalid = 0
    outside = 0
    for row in rows:
        if row["valid"] != "true":
            invalid += 1
            continue
        cell = get_cell(row)
        if cell is None:
            outside += 1
            continue
        seen[cell][row["machine"]].append(row)
        prefix = "held" if row["time_class"] == "SUSTAINED" else "attack"
        clouds[row["machine"], cell[2]].append((float(row[f"{prefix}_x"]), math.log2(float(row[f"{prefix}_centroid"]))))
    matrix = []
    for time in CLASSES:
        for y in range(9):
            for x in range(10):
                cell = (x, y, time)
                entry = {"id": cell_id(cell), "x_bin": x, "y_bin": y, "time_class": time,
                         "x_range": [x / 10, (x + 1) / 10], "y_range_hz": list(Y_EDGES[y:y + 2]),
                         "exception": exception(*cell), "machines": {}, "pass": False}
                for machine, meta in machines.items():
                    measured = seen[cell].get(machine, [])
                    verified = []
                    for row in measured:
                        turns = int(row["turns"])
                        if row["source"] == "starter" and turns == 0:
                            verified.append(row)
                        elif row["source"] == "macro" and turns <= 1:
                            verified.append(row)
                        elif row["source"] == "probe" and turns <= 3:
                            parent = indexed.get(key(row, row["parent"]))
                            if parent and parent["source"] == "macro" and adjacent(position(parent), position(row)):
                                verified.append(row)
                    nearest = min(measured, key=lambda r: float(r["distance"]), default=None)
                    cheapest = min(verified, key=lambda r: (int(r["turns"]), float(r["distance"])), default=None)
                    data = {"reached": bool(measured), "counted": meta["counts"] == "true",
                            "min_cost": int(cheapest["turns"]) if cheapest else None,
                            "witness": witness(cheapest) if cheapest else None,
                            "nearest_observed": witness(nearest) if nearest else None}
                    entry["machines"][machine] = data
                    if data["counted"] and data["min_cost"] is not None and data["min_cost"] <= 3:
                        entry["pass"] = True
                matrix.append(entry)
    continuity = continuity_report(rows, machines, int(run["walk_steps"]))
    ratio_continuity = continuity_report(rows, {m:v for m,v in machines.items() if m=="glass"}, int(run["walk_steps"]), "ratio_walk")
    agency = agency_report(rows, machines, directory, metric)
    hulls = {machine: {time: {"points": [{"x": x, "centroid_hz": 2 ** logy} for x, logy in convex_hull(clouds[machine, time])],
                                     "observations": len(clouds[machine, time])}
                       for time in CLASSES} for machine in machines}
    result = {"schema": 1, "run": run,
              "analysis": {"metric": metric, "mapping": "clamp(d/6,0,1)" if metric == "legacy" else "d/(d+0.25 semitones)",
                           "legacy_windows_recovered_without_clipping": recovered_windows,
                           "raw_directory": str(directory.resolve())},
              "machines": machines, "matrix": matrix,
              "continuity": continuity, "ratio_continuity": ratio_continuity, "agency": agency, "hulls": hulls,
              "diagnostics": {"renders": len(rows), "parameter_note_velocity_cases": len(rows),
                              "held_released_trajectories": len(rows) * 2,
                              "lock_trajectories": lock_trajectories, "invalid_audio": invalid,
                              "silent_or_outside_grid": outside, "integrity_errors": integrity}}
    return result


def coverage_matrix_meets_the_plan(result: dict) -> list[str]:
    """Standalone acceptance, intentionally absent from the fast Rust suite."""
    failures = list(result["diagnostics"]["integrity_errors"])
    if len(result["run"].get("executable_sha256", "")) != 64:
        failures.append("missing executable SHA-256 provenance")
    if int(result["run"]["lhs_vectors"]) < 3000:
        failures.append("incomplete exploration: fewer than 3000 LHS vectors per machine")
    if int(result["run"]["walk_steps"]) < 20:
        failures.append("incomplete continuity: fewer than20 X steps")
    if int(result["run"]["fine_probes"]) < 1:
        failures.append("starter-centered fine probes were not run")
    if result["diagnostics"]["invalid_audio"]:
        failures.append(f"{result['diagnostics']['invalid_audio']} renders contain non-finite audio")
    for cell in result["matrix"]:
        if cell["exception"] or cell["pass"]:
            continue
        reached = any(m["reached"] and m["counted"] for m in cell["machines"].values())
        failures.append(f"{cell['id']}: {'no verified cost <=3 witness' if reached else 'unreached'}")
    for machine, report in result["continuity"].items():
        if report["required"] and (not report["pass"] or not report["complete"]):
            failures.append(f"{machine}: required continuous band not demonstrated")
        if report["required"] and not result["agency"][machine]["macro_moves_cell"]:
            failures.append(f"{machine}: no one-turn macro crosses a cell")
        if report["required"] and not result["agency"][machine]["lock_pass"]:
            failures.append(f"{machine}: locked two-hit region change and restoration not demonstrated")
    return failures


def write_json(path: Path, value) -> None:
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")


def write_outputs(directory: Path, result: dict) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    failures = coverage_matrix_meets_the_plan(result)
    matrix = result["matrix"]
    for name in ("matrix", "continuity", "ratio_continuity", "agency", "hulls"):
        write_json(directory / f"{name}.json", {"schema": 1, "analysis": result["analysis"], name: result[name]})
    write_json(directory / "acceptance.json", {"pass": not failures, "failures": failures,
                                             "diagnostics": result["diagnostics"], "run": result["run"], "analysis": result["analysis"]})
    write_json(directory / "exceptions.json", [{"cell": c["id"], "reason": c["exception"]}
                                               for c in matrix if c["exception"]])
    with (directory / "matrix.csv").open("w", newline="") as stream:
        writer = csv.writer(stream)
        writer.writerow(("cell", "x_bin", "y_bin", "time_class", "exception", "machine", "counted", "reached", "min_cost", "vector", "note", "velocity"))
        for cell in matrix:
            for machine, value in cell["machines"].items():
                witness = value["witness"] or {}
                writer.writerow((cell["id"], cell["x_bin"], cell["y_bin"], cell["time_class"], cell["exception"] or "", machine,
                                 value["counted"], value["reached"], value["min_cost"], witness.get("vector", ""), witness.get("note", ""), witness.get("velocity", "")))
    required = [c for c in matrix if not c["exception"]]
    reached = sum(any(m["reached"] and m["counted"] for m in c["machines"].values()) for c in required)
    cheap = sum(c["pass"] for c in required)
    lines = ["# Measured synthesis coverage", "", f"**{'PASS' if not failures else 'NOT ACCEPTED'}**. {result['diagnostics']['renders']} parameter/note/velocity cases; "
             f"{reached}/{len(required)} required cells reached; {cheap}/{len(required)} have a verified cost≤3 path.", "",
             f"{result['diagnostics']['held_released_trajectories']} independent held/released audio trajectories and "
             f"{result['diagnostics']['lock_trajectories']} additional lock-test trajectories were measured.", "",
             f"Analysis metric: **{result['analysis']['metric']}**, inharmonicity = `{result['analysis']['mapping']}`. "
             f"Raw measurements: `{result['analysis']['raw_directory']}`.", "",
             "A reached cell is an observation. A cost witness is a measured starter → one macro → up to two cell edits. "
             "Random-nearest cost estimates and convex-hull interiors do not establish coverage.", "",
             "| Machine | Reached required cells | Cost≤3 cells | X sweeps passing | Macro | Lock | Starter |",
             "|---|---:|---:|---:|---|---|---|"]
    for machine, meta in result["machines"].items():
        cells = [c["machines"][machine] for c in required]
        sweeps = result["continuity"][machine]["sweeps"]
        agency = result["agency"][machine]
        lines.append(f"| {machine}{' (reference only)' if meta['counts'] != 'true' else ''} | {sum(c['reached'] for c in cells)} | "
                     f"{sum(c['min_cost'] is not None for c in cells)} | {sum(s['pass'] for s in sweeps)}/{len(sweeps)} | "
                     f"{agency['macro_moves_cell']} | {agency['lock_pass']} | {meta['starter']} |")
    lines += ["", "The full acceptance failure list is in acceptance.json. Each matrix cost includes its exact vector, note, velocity, "
              "and macro parent. vectors.csv contains every parameter value. Hulls use X/log₂ Hz geometry and are display outlines only.", "",
              "Measurement corrections and limitations are documented in notes/20260909-coverage-harness.md. "
              "Picture behavior and the one-hour keyboard session require separate validation; this report does not certify them."]
    (directory / "report.md").write_text("\n".join(lines) + "\n")
    panels = []
    for time in CLASSES:
        boxes = []
        for cell in (c for c in matrix if c["time_class"] == time):
            observed = [m for m, v in cell["machines"].items() if v["counted"] and v["reached"]]
            fill = "#263a37" if cell["exception"] else "#76e8bd" if cell["pass"] else "#bfac60" if observed else "#132321"
            title = html.escape(f"{cell['id']}: {cell['exception'] or ', '.join(observed) or 'unreached'}; "
                                + ", ".join(f"{m} cost {v['min_cost']}" for m, v in cell["machines"].items() if v["min_cost"] is not None))
            x, y = 64 + cell["x_bin"] * 29, 15 + (8 - cell["y_bin"]) * 29
            boxes.append(f'<rect x="{x}" y="{y}" width="26" height="26" fill="{fill}"><title>{title}</title></rect>')
        for i, edge in enumerate(Y_EDGES[:-1]):
            boxes.append(f'<text x="56" y="{32 + (8 - i) * 29}" text-anchor="end">{edge}</text>')
        boxes.append('<text x="64" y="299">tonal 0</text><text x="351" y="299" text-anchor="end">1 noise</text>')
        panels.append(f'<section><h2>{time.lower()}</h2><svg viewBox="0 0 365 310" role="img" aria-label="{time} timbre coverage">{"".join(boxes)}</svg></section>')
    page = f'''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Measured timbre coverage</title>
<style>body{{background:#071412;color:#cae9de;font:15px system-ui;margin:40px}}main{{display:flex;flex-wrap:wrap;gap:24px}}section{{width:365px}}h1{{font-weight:500}}h2{{font-size:16px}}text{{fill:#a5bcb5;font:11px monospace}}p{{max-width:1000px;line-height:1.6}}a{{color:#76e8bd}}</style>
<h1>Measured timbre coverage · {"pass" if not failures else "not accepted"}</h1><p>{reached}/{len(required)} required cells reached; {cheap} with verified cost≤3. Green: cheap path. Ochre: observed, cost unproven. Dark: missing. Muted: explicit exception. Hover a cell for measured instruments.</p><main>{"".join(panels)}</main>
<p>48 kHz, independent held and released trajectories. X =0.6 inharmonicity +0.4 averaged spectral flatness. The upper Y bin spans 7680–16000 Hz. A hull is never evidence that its interior is reachable. <a href="report.md">Report</a> · <a href="matrix.json">Matrix</a> · <a href="acceptance.json">Acceptance failures</a></p></html>'''
    (directory / "coverage.html").write_text(page)


def self_test() -> None:
    import tempfile
    assert xy(1, 16000, 1) == (9, 8)
    assert xy(0, 30, 1) == (0, 0)
    assert xy(0, 29.9, 1) is None
    assert xy(0, 300, 0) is None
    assert len([1 for t in CLASSES for y in range(9) for x in range(10) if exception(x, y, t)]) == 24
    assert convex_hull([(0, 0), (1, 0), (0, 1), (.2, .2)]) == [(0, 0), (1, 0), (0, 1)]
    assert adjacent((1, 2), (2, 3)) and not adjacent((1, 2), (3, 2))
    # The acceptance function must refuse both a missing and an observed
    # but expensive cell, and must never bless a short run.
    fake = {"run": {"lhs_vectors": "1", "walk_steps": "2", "fine_probes": "0"},
            "diagnostics": {"integrity_errors": [], "invalid_audio": 0},
            "matrix": [{"id": "a", "exception": None, "pass": False, "machines": {}},
                       {"id": "b", "exception": None, "pass": False, "machines": {"x": {"reached": True, "counted": True}}}],
            "continuity": {}}
    failures = coverage_matrix_meets_the_plan(fake)
    assert any("a: unreached" in f for f in failures)
    assert any("b: no verified" in f for f in failures)
    assert len(failures) == 6
    raw = {"mean_semitones": ".25", "inharmonicity": str(.25 / 6), "x": ".025", "flatness": "0"}
    assert not apply_metric(raw, "", "25cent")
    assert abs(float(raw["x"]) - .3) < 1e-12
    clipped = {"inharmonicity": "1", "x": ".6", "flatness": "0"}
    try:
        apply_metric(clipped, "", "25cent")
        raise AssertionError("clipped legacy deviation was reconstructed")
    except ValueError:
        pass
    # End-to-end CSV path: a three-turn probe whose measured macro landed
    # nearby is evidence; a close LHS vector and a distant macro are not.
    with tempfile.TemporaryDirectory(prefix="daw-coverage-test-") as temp:
        directory = Path(temp)
        def table(name, records):
            with (directory / name).open("w", newline="") as stream:
                writer = csv.DictWriter(stream, fieldnames=list(records[0]))
                writer.writeheader()
                writer.writerows(records)
        table("machines.csv", [{"machine": "test", "counts": "true", "band_required": "false",
                                "x": "0", "y": "1", "t": "2", "starter": "defaults"}])
        table("run.csv", [{"key": k, "value": v} for k, v in
                          {"lhs_vectors": "1", "fine_probes": "2", "walk_steps": "2"}.items()])
        rows = []
        for vector, source, parent, turns, x, y in [
                (0, "starter", "", 0, .05, 300), (1, "macro", "", 1, .35, 300),
                (2, "probe", "1", 3, .45, 700), (3, "lhs", "", 3, .95, 300),
                (4, "probe", "1", 3, .85, 4000)]:
            for note in (36, 48, 60):
                for velocity in (64, 127):
                    row = {"machine": "test", "vector": str(vector), "source": source,
                           "parent": parent, "turns": str(turns), "cost5": str(turns),
                           "distance": "0.1", "note": str(note), "velocity": str(velocity),
                           "valid": "true", "time_class": "DECAYING", "y_setting": "", "walk_step": ""}
                    for prefix in ("held", "attack"):
                        row.update({f"{prefix}_x": str(x), f"{prefix}_centroid": str(y), f"{prefix}_rms": "1"})
                    rows.append(row)
        table("renders.csv", rows)
        measured = analyze(directory, "legacy")
        cells = {c["id"]: c for c in measured["matrix"]}
        assert cells["DECAYING:4:4"]["machines"]["test"]["min_cost"] == 3
        assert cells["DECAYING:9:3"]["machines"]["test"]["reached"]
        assert cells["DECAYING:9:3"]["machines"]["test"]["min_cost"] is None
        assert cells["DECAYING:8:7"]["machines"]["test"]["min_cost"] is None
        write_outputs(directory, measured)
        assert (directory / "coverage.html").is_file()
        assert not json.loads((directory / "acceptance.json").read_text())["pass"]
        try:
            merge_runs(directory / "merged", [directory, directory])
            raise AssertionError("duplicate machine merge was accepted")
        except ValueError:
            pass
    with tempfile.TemporaryDirectory() as temporary:
        source = Path(temporary) / "source"
        destination = Path(temporary) / "subset"
        source.mkdir()
        (source / "run.csv").write_text("key,value\nexecutable_sha256,unchanged\nlhs_vectors,3000\n")
        for name in ("renders.csv", "vectors.csv", "machines.csv", "timings.csv"):
            (source / name).write_text("machine,vector,parent\nfirst,101,100\nsecond,201,200\n")
        (source / "locks-first.csv").write_text("machine,hit\nfirst,restore\n")
        subset_run(destination, source, ["first"])
        assert (destination / "run.csv").read_bytes() == (source / "run.csv").read_bytes()
        assert read_csv(destination / "renders.csv") == [{"machine": "first", "vector": "101", "parent": "100"}]
        assert json.loads((destination / "source_subset.json").read_text())["machines"] == ["first"]
        try:
            subset_run(destination, source, ["missing"])
            raise AssertionError("missing machine subset accepted")
        except ValueError:
            pass
    print("coverage analyzer self-tests passed")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path, nargs="?")
    parser.add_argument("--accept", action="store_true", help="exit1 unless the complete plan passes")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--merge", nargs="+", type=Path, help="merge disjoint-machine raw runs into directory before analyzing")
    parser.add_argument("--subset-from", type=Path, help="copy measured rows from this run into directory before analyzing")
    parser.add_argument("--subset-machines", help="comma-separated machines to retain with --subset-from")
    parser.add_argument("--metric", choices=("legacy", "25cent"), default="25cent", help="explicit inharmonicity normalization; raw CSV remains unchanged")
    parser.add_argument("--analysis-out", type=Path, help="write analysis separately, preserving another metric's report")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        if args.directory is None:
            return 0
    if args.directory is None:
        parser.error("directory is required unless using --self-test")
    if args.subset_from or args.subset_machines:
        if not args.subset_from or not args.subset_machines or args.merge:
            parser.error("--subset-from and --subset-machines require each other and exclude --merge")
        subset_run(args.directory, args.subset_from, args.subset_machines.split(","))
    if args.merge:
        merge_runs(args.directory, args.merge)
    result = analyze(args.directory, args.metric)
    destination = args.analysis_out or args.directory
    write_outputs(destination, result)
    failures = coverage_matrix_meets_the_plan(result)
    print((destination / "report.md").read_text())
    if args.accept and failures:
        print(f"coverage_matrix_meets_the_plan FAILED: {len(failures)} reasons; see acceptance.json", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
