#!/usr/bin/env python3
"""trace — turn a UI mockup into numbers, and numbers into a panel spec.

Written after tracing `session-macos-square-concept.png` by hand and doing
every one of these steps at least three times in ad-hoc shell. The point is
that the next mockup costs a command instead of an afternoon.

The method it encodes, in order:

  1. normalise  strip the desktop and window chrome, resample to the app's
                own raster, so the drawing and a `shot` render can be
                compared pixel for pixel
  2. bands      scan a column or row for runs of near-constant colour —
                every boundary in the layout, as a number
  3. probe      one element's full anatomy: bounds, fill gradient, border
  4. cells      a row of repeated cells: bounds, margins, gaps, and whether
                the widths follow a rule or were placed by hand
  5. palette    dominant colours and named probes, converted to the OkLCh
                the theme file speaks
  6. diff       app render against reference: RMSE, difference map, blend
  7. emit       the measurements as a PANEL SPEC — `@tune` constants in the
                house style, palette role lines, and the per-panel uniforms
                a shader wants: rect, corner radius, gradient stops, border
                width and colour, shadow.

WHY A PANEL SPEC AND NOT egui CALLS. egui's shape API has no rounded rect
with a gradient and an inner border, no blur, no signed-distance anything;
a linear gradient already costs a hand-built four-vertex mesh, and glass,
soft shadows and true hairlines are not expressible at all. These mockups
are full of exactly those. `src/shell/screen.rs` already establishes the
pattern to use instead: UI code registers the exact rectangles it wants,
and a wgpu pass draws them with a real shader. A panel pass is that same
shape, running BEFORE egui so text lands on top rather than after it.

So this tool stops at MEASUREMENTS plus uniforms. What consumes them is a
shader, and egui keeps the text.

DEPENDENCIES: ImageMagick only. Deliberately not Pillow or numpy — neither
is installed on this machine, and a tracing tool that needs a pip install
before it can look at a picture is a tracing tool nobody runs.

Pixels come back through `magick ... txt:-`, which is slow per pixel and
fine per strip. Every command here reads strips, never whole images.

    tools/trace.py normalise shot.png ref.png --size 1280x800
    tools/trace.py bands ref.png --col 6
    tools/trace.py probe ref.png --rect 176,45,136,43
    tools/trace.py cells ref.png --band 45..87 --expect 8
    tools/trace.py palette ref.png --probe 760,450=ground --probe 229,200=panel
    tools/trace.py diff app.png ref.png --out /tmp/trace
    tools/trace.py emit measured.json --module deck
"""

import argparse
import json
import math
import re
import subprocess
import sys

# ---------------------------------------------------------------- pixels


def _magick(args):
    out = subprocess.run(["magick", *args], capture_output=True, text=True)
    if out.returncode != 0:
        sys.exit(f"magick failed: {out.stderr.strip()}")
    return out.stdout


def size(img):
    w, h = _magick([img, "-format", "%w %h", "info:"]).split()
    return int(w), int(h)


def region(img, x, y, w, h):
    """{(x, y): (r, g, b)} for a rectangle, in IMAGE coordinates."""
    txt = _magick([img, "-crop", f"{w}x{h}+{x}+{y}", "+repage", "txt:-"])
    px = {}
    for line in txt.splitlines()[1:]:
        m = re.match(r"(\d+),(\d+): \((\d+),(\d+),(\d+)", line)
        if m:
            px[(x + int(m.group(1)), y + int(m.group(2)))] = (
                int(m.group(3)),
                int(m.group(4)),
                int(m.group(5)),
            )
    return px


def column(img, x, y0=0, y1=None):
    _, h = size(img)
    y1 = h if y1 is None else y1
    px = region(img, x, y0, 1, y1 - y0)
    return [px.get((x, y)) for y in range(y0, y1)]


def strip_row(img, y, x0=0, x1=None):
    w, _ = size(img)
    x1 = w if x1 is None else x1
    px = region(img, x0, y, x1 - x0, 1)
    return [px.get((x, y)) for x in range(x0, x1)]


def near(a, b, tol):
    return a is not None and b is not None and sum(abs(i - j) for i, j in zip(a, b)) <= tol


def mean(colours):
    colours = [c for c in colours if c]
    if not colours:
        return None
    n = len(colours)
    return tuple(sum(c[i] for c in colours) // n for i in range(3))


def hexs(c):
    return "#%02x%02x%02x" % c if c else "--"


# ------------------------------------------------------------- colour maths


def _linear(v):
    v /= 255
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def oklch(c):
    """sRGB 0..255 -> (L, C, h in radians), the theme file's own units."""
    r, g, b = (_linear(v) for v in c)
    l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b
    m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b
    s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b
    l_, m_, s_ = l ** (1 / 3), m ** (1 / 3), s ** (1 / 3)
    L = 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_
    a = 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_
    bb = 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_
    return L, math.hypot(a, bb), math.atan2(bb, a)


def luminance(c):
    r, g, b = (_linear(v) for v in c)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a, b):
    la, lb = luminance(a), luminance(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


# ------------------------------------------------------------------ bands


def runs(seq, tol, minlen, offset=0):
    """Contiguous runs of near-constant colour: (start, end, colour)."""
    out, start = [], 0
    for i in range(1, len(seq)):
        if not near(seq[i - 1], seq[i], tol):
            if i - start >= minlen:
                out.append((start + offset, i - 1 + offset, mean(seq[start:i])))
            start = i
    if len(seq) - start >= minlen:
        out.append((start + offset, len(seq) - 1 + offset, mean(seq[start:])))
    return out


def cmd_bands(a):
    if (a.col is None) == (a.row is None):
        sys.exit("give exactly one of --col or --row")
    seq = column(a.image, a.col) if a.col is not None else strip_row(a.image, a.row)
    axis = "y" if a.col is not None else "x"
    print(f"{a.image}  {'column x=' + str(a.col) if a.col is not None else 'row y=' + str(a.row)}")
    for s, e, c in runs(seq, a.tol, a.min):
        print(f"  {axis} {s:4d}-{e:4d}  len {e - s + 1:4d}  {hexs(c)}  {c}")


# ------------------------------------------------------------------ probe


def anatomy(img, x, y, w, h):
    """One element: border colour, and the fill at its head and foot.

    A raised cell in these mockups is not one flat colour — it carries a
    slight vertical gradient and a lighter line around it. This reports
    all three, and whether the gradient is real or the element is flat.
    """
    # Five columns spread across the element, median per row. One column
    # is not enough: the middle of a cell is exactly where its label is,
    # and the first version of this reported the text's colour as the
    # fill. A median rejects a glyph crossing one or two of the five.
    picks = [x + int(w * f) for f in (0.14, 0.30, 0.50, 0.70, 0.86)]
    samples = [column(img, px, y, y + h) for px in picks]
    col = []
    for i in range(h):
        row_ = sorted((c for c in (s[i] for s in samples) if c), key=luminance)
        col.append(row_[len(row_) // 2] if row_ else None)
    # A border is a LIGHTER line than the fill it encloses, and it is one
    # or two pixels thick with an antialiased pixel either side. So take
    # the brightest of the first and last few rows rather than the very
    # edge, which is usually half border and half whatever is behind it.
    def brightest(cs):
        cs = [c for c in cs if c]
        return max(cs, key=luminance) if cs else None
    top_border = brightest(col[:3])
    bottom_border = brightest(col[-3:])
    inner = [c for c in col[2 : h - 2] if c]
    if len(inner) < 6:
        return dict(border=None, top=None, foot=None, gradient=False)
    head = mean(inner[: max(2, len(inner) // 4)])
    foot = mean(inner[-max(2, len(inner) // 4) :])
    delta = sum(abs(i - j) for i, j in zip(head, foot))
    left = strip_row(img, y + h // 2, x, x + w)
    sides = [c for c in (left[0], left[1], left[-2], left[-1]) if c]
    edge = [c for c in (top_border, bottom_border) if c]
    if sides:
        edge.append(max(sides, key=luminance))
    return dict(
        border=mean(edge) if edge else None,
        top=head,
        foot=foot,
        gradient=delta >= 4,
        delta=delta,
    )


def cmd_probe(a):
    x, y, w, h = (int(v) for v in a.rect.split(","))
    r = anatomy(a.image, x, y, w, h)
    print(f"{a.image}  rect {x},{y} {w}x{h}")
    print(f"  border   {hexs(r['border'])}")
    print(f"  fill top {hexs(r['top'])}")
    print(f"  fill foot{hexs(r['foot'])}")
    print(f"  gradient {'yes' if r['gradient'] else 'no'}  (delta {r.get('delta', 0)})")
    if r["top"] and r["border"]:
        print(f"  border/fill contrast {contrast(r['border'], r['top']):.2f}:1")


# ------------------------------------------------------------------ cells


def cmd_cells(a):
    y0, y1 = (int(v) for v in a.band.split(".."))
    mid = (y0 + y1) // 2
    seq = strip_row(a.image, mid)
    # Ground is sampled at the two ENDS of the band, which is right
    # whenever the crop includes the row's margins.
    #
    # It is tempting to use the most common tone instead, to survive a
    # crop that lands inside the window. Do not: in a row of keys the
    # cells cover most of the width, so the most common tone IS the cell
    # fill, and the detector then finds every letter of every label as a
    # separate cell. Tried, measured, reverted. When the crop is wrong,
    # fix the crop or pass --ground.
    if a.ground:
        g = a.ground.lstrip("#")
        ground = tuple(int(g[i:i + 2], 16) for i in (0, 2, 4))
    else:
        ground = mean(seq[:4] + seq[-4:])
    inside = [x for x, c in enumerate(seq) if c and not near(c, ground, a.tol)]
    if not inside:
        sys.exit("no cells found: is the band right, and is the row ground at the edges?")
    groups, s, p = [], inside[0], inside[0]
    for x in inside[1:]:
        if x - p > a.gap:
            groups.append((s, p))
            s = x
        p = x
    groups.append((s, p))
    w, _ = size(a.image)
    print(f"{a.image}  band y {y0}..{y1}  ground {hexs(ground)}  found {len(groups)} cells")
    if a.expect and not (a.expect * 0.5 <= len(groups) <= a.expect * 2):
        print(f"\n  SUSPECT: expected about {a.expect}. One cell spanning everything\n"
              f"  means the ground sample landed INSIDE a cell — the crop is tight.\n"
              f"  Many narrow ones mean it landed inside the cell FILL, so every\n"
              f"  glyph reads as a cell. Fix the crop, or pass --ground <hex>.\n")
    print(f"  {'#':>3} {'x0':>5} {'x1':>5} {'width':>6} {'gap':>4}")
    widths, gaps, prev = [], [], None
    for i, (x0, x1) in enumerate(groups):
        g = "" if prev is None else x0 - prev - 1
        print(f"  {i + 1:>3} {x0:5d} {x1:5d} {x1 - x0 + 1:6d} {str(g):>4}")
        widths.append(x1 - x0 + 1)
        if prev is not None:
            gaps.append(x0 - prev - 1)
        prev = x1
    lead, trail = groups[0][0], w - groups[-1][1] - 1
    print(f"\n  margins  {lead} lead, {trail} trail")
    if gaps:
        print(f"  gaps     {min(gaps)}..{max(gaps)}, mean {sum(gaps) / len(gaps):.1f}")
    print(f"  widths   {min(widths)}..{max(widths)}, mean {sum(widths) / len(widths):.1f}")
    n = a.expect or len(groups)
    if gaps:
        even = (w - lead - trail - (n - 1) * round(sum(gaps) / len(gaps))) / n
        spread = max(widths) - min(widths)
        print(f"  even     {even:.1f} each, if they shared what margins and gaps leave")
        # The judgement this tool exists to make explicit rather than by eye.
        if spread > 0.12 * (sum(widths) / len(widths)):
            print(
                f"\n  VERDICT: hand-placed. Widths vary by {spread}px, "
                f"{100 * spread / (sum(widths) / len(widths)):.0f}% of the mean.\n"
                f"  Take the even rule ({even:.0f}px), not the drawing's jitter — it will\n"
                f"  adapt to other window widths and lands within a pixel of the mean."
            )
        else:
            print("\n  VERDICT: a real grid. The widths agree; copy them.")
    print("\n  anatomy of cell 1:")
    x0, x1 = groups[0]
    r = anatomy(a.image, x0, y0, x1 - x0 + 1, y1 - y0 + 1)
    print(f"    border {hexs(r['border'])}  fill {hexs(r['top'])} -> {hexs(r['foot'])}"
          f"  {'gradient' if r['gradient'] else 'flat'}")


# ---------------------------------------------------------------- normalise


def cmd_normalise(a):
    """Find the app content inside a screenshot and resample it.

    A mockup often arrives with a desktop behind it and window chrome
    around it. Content is assumed to be the LEAST saturated region — real
    UI is near-neutral or narrow-hue, wallpaper is not — which is what
    separated the macOS concept from its blue gradient.
    """
    w, h = size(a.image)
    def sat(c):
        return max(c) - min(c) if c else 999
    # The UNION across several scanlines, not one through the middle.
    # A single line fails whenever the UI itself is saturated somewhere
    # along it — the first version of this missed the whole right-hand
    # desk, because the row it chose crossed the green meters and read
    # them as wallpaper. Any one line under-reports; no line reports
    # content outside the window, so the union is safe in the direction
    # that matters.
    xs, ys = [], []
    for f in (0.2, 0.35, 0.5, 0.65, 0.8):
        for x, c in enumerate(strip_row(a.image, int(h * f))):
            if sat(c) < a.chroma:
                xs.append(x)
        for y, c in enumerate(column(a.image, int(w * f))):
            if sat(c) < a.chroma:
                ys.append(y)
    if not xs or not ys:
        sys.exit("found no low-chroma content; raise --chroma or crop by hand")
    x0, x1, y0, y1 = min(xs), max(xs), min(ys), max(ys)
    cw, ch = x1 - x0 + 1, y1 - y0 + 1
    tw, th = (int(v) for v in a.size.split("x"))
    print(f"content {x0},{y0} {cw}x{ch}  aspect {cw / ch:.4f}  target {tw / th:.4f}")
    if abs(cw / ch - tw / th) > 0.05:
        print("  WARNING: aspect differs by more than 5%; the resample will distort.")
    _magick([a.image, "-crop", f"{cw}x{ch}+{x0}+{y0}", "+repage",
             "-resize", f"{tw}x{th}!", a.out])
    print(f"wrote {a.out}")


# ----------------------------------------------------------------- palette


def cmd_palette(a):
    print(f"{a.image}  dominant colours ({a.colors} buckets)")
    txt = _magick([a.image, "-colors", str(a.colors), "-format", "%c", "histogram:info:-"])
    rows = []
    for line in txt.splitlines():
        m = re.search(r"(\d+):\s+\(\s*([\d.]+),\s*([\d.]+),\s*([\d.]+)", line)
        if m:
            rows.append((int(m.group(1)),
                         tuple(int(float(m.group(i))) for i in (2, 3, 4))))
    for n, c in sorted(rows, reverse=True):
        L, C, hh = oklch(c)
        print(f"  {hexs(c)}  {n:>9}px   L {L:.3f}  C {C:.3f}  h {hh:6.3f}")
    if not a.probe:
        return
    print("\nnamed probes, as theme-file lines:")
    lines = []
    for spec in a.probe:
        pos, _, name = spec.partition("=")
        x, y = (int(v) for v in pos.split(","))
        c = region(a.image, x, y, 1, 1).get((x, y))
        L, C, hh = oklch(c)
        lines.append(f"{name:8s} {L:.3f} {C:.3f} {hh:6.3f}   # {hexs(c)}")
        print("  " + lines[-1])
    if a.out:
        with open(a.out, "w") as f:
            f.write("# name  L  C  h(radians), OkLCh.\n")
            f.write(f"# traced from {a.image} by tools/trace.py\n")
            f.write("\n".join(lines) + "\n")
        print(f"\nwrote {a.out}")


# -------------------------------------------------------------------- diff


def cmd_diff(a):
    import os
    os.makedirs(a.out, exist_ok=True)
    r = subprocess.run(["magick", "compare", "-metric", "RMSE", a.app, a.ref, "null:"],
                       capture_output=True, text=True)
    print(f"RMSE {r.stderr.strip()}   (0 = identical)")
    _magick([a.ref, a.app, "-compose", "difference", "-composite",
             "-colorspace", "Gray", "-auto-level", f"{a.out}/diff.png"])
    _magick([a.ref, a.app, "-compose", "blend", "-define", "compose:args=50",
             "-composite", f"{a.out}/blend.png"])
    print(f"wrote {a.out}/diff.png (bright = disagreement)")
    print(f"wrote {a.out}/blend.png (ghosting = misalignment)")
    print("\nband comparison, column x=%d:" % a.col)
    for name, img in (("app", a.app), ("ref", a.ref)):
        print(f"  -- {name} --")
        for s, e, c in runs(column(img, a.col), 6, 6)[: a.rows]:
            print(f"     y {s:4d}-{e:4d}  len {e - s + 1:4d}  {hexs(c)}")
    print("\nNOTE: RMSE is a good scoreboard for gross alignment and a bad one\n"
          "once that is fixed — most of a frame is empty ground, so the metric\n"
          "goes flat while per-element boundaries are still wrong. Trust the\n"
          "band table past that point.")


# -------------------------------------------------------------------- emit


HOUSE = '''/// {doc}
/// @tune {lo}..{hi} px
const {name}: f32 = {value};'''


def cmd_emit(a):
    spec = json.load(open(a.spec))
    print(f"// Traced from {spec.get('source', '?')} by tools/trace.py.")
    print(f"// Measured at {spec.get('raster', '?')}; every number below is a")
    print("// measurement, not a preference.\n")
    for k in spec.get("constants", []):  # layout, still ordinary Rust
        v = float(k["value"])
        lo = k.get("min", 0)
        hi = k.get("max", int(v * 2 + 8))
        print(HOUSE.format(doc=k.get("doc", "Traced."), lo=lo, hi=hi,
                           name=k["name"], value=f"{v:g}" if v % 1 else f"{v:.1f}"))
    for pan in spec.get("panels", []):
        r = pan["rect"]
        print(f"\n// panel {pan.get('name', '?')} — uniforms for the panel pass")
        print(f"Panel {{")
        print(f"    rect: [{r[0]:.1f}, {r[1]:.1f}, {r[2]:.1f}, {r[3]:.1f}],")
        print(f"    radius: {pan.get('radius', 0):.1f},")
        for k in ("top", "foot", "border"):
            if pan.get(k):
                c = tuple(int(pan[k][i:i + 2], 16) for i in (1, 3, 5))
                print(f"    {k}: [{c[0] / 255:.4f}, {c[1] / 255:.4f}, "
                      f"{c[2] / 255:.4f}, 1.0],   // {pan[k]}")
        print(f"    border_px: {pan.get('border_px', 1.0):.1f},")
        print(f"    shadow: {pan.get('shadow', 0.0):.2f},")
        print("}")
    if spec.get("roles"):
        print("\n// theme-file lines:")
        for name, hx in spec["roles"].items():
            c = tuple(int(hx[i:i + 2], 16) for i in (1, 3, 5))
            L, C, hh = oklch(c)
            print(f"// {name:8s} {L:.3f} {C:.3f} {hh:6.3f}   # {hx}")


# -------------------------------------------------------------------- main


def main():
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)

    n = sub.add_parser("normalise", help="strip chrome, resample to the app's raster")
    n.add_argument("image"); n.add_argument("out")
    n.add_argument("--size", default="1280x800")
    n.add_argument("--chroma", type=int, default=35,
                   help="max max-min channel spread counted as UI, not wallpaper")
    n.set_defaults(fn=cmd_normalise)

    b = sub.add_parser("bands", help="runs of near-constant colour along a line")
    b.add_argument("image"); b.add_argument("--col", type=int); b.add_argument("--row", type=int)
    b.add_argument("--tol", type=int, default=6); b.add_argument("--min", type=int, default=4)
    b.set_defaults(fn=cmd_bands)

    pr = sub.add_parser("probe", help="one element: border, fill gradient")
    pr.add_argument("image"); pr.add_argument("--rect", required=True, metavar="x,y,w,h")
    pr.set_defaults(fn=cmd_probe)

    c = sub.add_parser("cells", help="a row of repeated cells: bounds, gaps, rule or jitter")
    c.add_argument("image"); c.add_argument("--band", required=True, metavar="y0..y1")
    c.add_argument("--expect", type=int); c.add_argument("--tol", type=int, default=28)
    c.add_argument("--gap", type=int, default=2)
    c.add_argument("--ground", help="row ground as #rrggbb, when the crop has no margin")
    c.set_defaults(fn=cmd_cells)

    pa = sub.add_parser("palette", help="dominant colours and named probes, in OkLCh")
    pa.add_argument("image"); pa.add_argument("--colors", type=int, default=14)
    pa.add_argument("--probe", action="append", metavar="x,y=name")
    pa.add_argument("--out")
    pa.set_defaults(fn=cmd_palette)

    d = sub.add_parser("diff", help="app render against reference")
    d.add_argument("app"); d.add_argument("ref"); d.add_argument("--out", default="/tmp/trace")
    d.add_argument("--col", type=int, default=6); d.add_argument("--rows", type=int, default=8)
    d.set_defaults(fn=cmd_diff)

    e = sub.add_parser("emit", help="measurements as Rust, in the house style")
    e.add_argument("spec"); e.add_argument("--module")
    e.set_defaults(fn=cmd_emit)

    a = p.parse_args()
    a.fn(a)


if __name__ == "__main__":
    main()
