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


def inner_of(col, h):
    return [c for c in col[2 : h - 2] if c]


def anatomy(img, x, y, w, h, ground=None):
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
    # Each side measured on its own, then the STRONGEST kept — not the
    # average of four. Averaging is what made every recreated border come
    # out dimmer than the original: a bright top edge got blended with
    # three sides that were half background, and the result was a colour
    # present nowhere in the drawing.
    mid_row = strip_row(img, y + h // 2, x, x + w)
    fill = mean(inner_of(col, h)) if col else None

    # Each side offers its outermost TWO pixels, and the side keeps the
    # one CLOSER to the fill. A bounding box can be a pixel wide of the
    # object, and then the outermost pixel is whatever lies beyond it —
    # for a key cell that is the dark gap between cells, which is further
    # from the fill than the real border and would win a naive
    # most-different test. Probing one rect off by a single pixel flipped
    # the answer from #3f5979 to #010a1a, so the pair is taken and the
    # outlier discarded.
    def side(pair):
        """The outermost pixel, unless it is really the page behind."""
        cand = [c for c in pair if c]
        if not cand:
            return None
        if ground and fill and len(cand) > 1:
            d_ground = sum(abs(i - j) for i, j in zip(cand[0], ground))
            d_fill = sum(abs(i - j) for i, j in zip(cand[0], fill))
            if d_ground < d_fill:
                return cand[1]
        return cand[0]

    # Each pair is ordered OUTERMOST FIRST, which for the bottom and right
    # sides means reversing the slice. Getting that wrong silently read the
    # inner pixel on two of the four sides: KCalc's buttons carry their
    # border on the right edge, so the border came back as the fill and
    # every recreated button lost its edge.
    edges = [c for c in (
        side(col[:2]),
        side(col[-2:][::-1]),
        side(mid_row[:2] if mid_row else []),
        side(mid_row[-2:][::-1] if mid_row else []),
    ) if c]
    border = None
    if edges and fill:
        border = max(edges, key=lambda c: sum(abs(i - j) for i, j in zip(c, fill)))
    elif edges:
        border = edges[0]
    # Thickness: walk in from the top until the fill is reached.
    width_px = 0
    if fill:
        for c in col[:6]:
            if c and near(c, fill, 24):
                break
            width_px += 1
    return dict(
        border=border,
        border_px=max(1, min(width_px, 4)),
        top=head,
        foot=foot,
        gradient=delta >= 4,
        delta=delta,
    )


def cmd_probe(a):
    x, y, w, h = (int(v) for v in a.rect.split(","))
    # The page ground, so an overshooting rect can be told from a border.
    corner = region(a.image, 0, 0, 3, 3).get((1, 1))
    r = anatomy(a.image, x, y, w, h, corner)
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

    au = sub.add_parser("auto", help="the whole method: segment, measure, group, emit")
    au.add_argument("image"); au.add_argument("--out", default="spec.json")
    au.add_argument("--normalise", action="store_true",
                    help="strip chrome and resample first")
    au.add_argument("--size", default="1280x800")
    au.add_argument("--fuzz", type=int, default=None,
                    help="percent; omit to sweep and choose (recommended)")
    au.add_argument("--area", type=int, default=400, help="smallest object counted")
    au.add_argument("--min-side", type=int, default=10, dest="min_side")
    au.add_argument("--lit", type=int, default=60,
                    help="colour distance at which a row member counts as lit")
    au.add_argument("--no-text", action="store_true", dest="no_text",
                    help="skip OCR; surfaces only")
    au.add_argument("--limit", type=int, default=12, help="how many single panels to report")
    au.set_defaults(fn=cmd_auto)

    rn = sub.add_parser("render", help="rebuild a picture from a spec, to score the measurement")
    rn.add_argument("spec"); rn.add_argument("out")
    rn.add_argument("--font", default="sans-serif",
                    help="a family fontconfig knows, or a path to a .ttf")
    rn.set_defaults(fn=cmd_render)

    a = p.parse_args()
    a.fn(a)




# -------------------------------------------------------------------- auto
#
# The whole method in one command. Everything above is a hand tool; this is
# the machine. Given a mockup it finds the panels itself, measures each one,
# groups the repeated rows, and writes a spec.
#
# Segmentation is ImageMagick's connected-component labelling rather than
# anything hand-rolled: with a fuzz it merges each panel's gradient into one
# object and hands back the bounding box, area and mean colour. That single
# call replaces the column-scanning the earlier commands do by hand, and it
# found all eight key cells at the positions measured by hand, including
# telling the lit one apart by its mean colour alone.


def components(img, fuzz, area):
    """[(id, x, y, w, h, area, mean_rgb)], largest first."""
    txt = _magick([
        img, "-fuzz", f"{fuzz}%",
        "-define", "connected-components:verbose=true",
        "-define", f"connected-components:area-threshold={area}",
        "-define", "connected-components:mean-color=true",
        "-connected-components", "8", "null:",
    ])
    out = []
    for line in txt.splitlines()[1:]:
        m = re.match(
            r"\s*(\d+):\s+(\d+)x(\d+)\+(\d+)\+(\d+)\s+[\d.,]+\s+(\d+)\s+srgb\("
            r"([\d.]+)%,([\d.]+)%,([\d.]+)%\)", line)
        if not m:
            continue
        g = m.groups()
        out.append(dict(
            id=int(g[0]), w=int(g[1]), h=int(g[2]), x=int(g[3]), y=int(g[4]),
            area=int(g[5]),
            colour=tuple(round(float(v) * 255 / 100) for v in g[6:9]),
        ))
    return sorted(out, key=lambda o: -o["area"])


def choose_fuzz(img, area, min_side, verbose=True):
    """Pick the fuzz that segments this image, by sweeping and reading the shape.

    There is no good default. Fuzz has to bridge a panel's own gradient
    without bridging the step between a panel and its ground, and how far
    apart those two are is a property of the DESIGN. The default of 6%
    suited the macOS concept and silently swallowed the KCalc display,
    which sits only thirteen units per channel off its window ground — so
    6% merged them and a whole panel vanished from the spec with no
    complaint.

    Object count against fuzz has a characteristic shape: a PLATEAU where
    the segmentation is right, a spike above it where gradients start
    splitting into bands, and a collapse beyond that as everything merges
    into one. Take the highest fuzz still on the first plateau — the most
    forgiving setting that has not yet begun merging things that differ.
    """
    counts = []
    for f in (1, 2, 3, 4, 5, 6, 8, 10):
        objs = components(img, f, area)
        counts.append((f, len([o for o in objs if o["w"] >= min_side and o["h"] >= min_side])))
    best, run = counts[0], []
    for i, (f, n) in enumerate(counts):
        if run and abs(n - run[-1][1]) > 0.25 * max(n, run[-1][1]):
            break
        run.append((f, n))
    if run:
        best = run[-1]
    if verbose:
        shape = "  ".join(f"{f}%:{n}" for f, n in counts)
        print(f"fuzz sweep   {shape}")
        print(f"chose {best[0]}% — highest still on the first plateau\n")
    return best[0]


def corner_radius(img, o, fill, tol=30):
    """How far in from the left the fill starts on the panel's first row.

    A square corner gives 0; a rounded one gives roughly its radius. Read
    two rows down from the top edge so the border itself is not counted.
    """
    row_ = strip_row(img, o["y"] + 2, o["x"], o["x"] + min(o["w"], 24))
    for i, c in enumerate(row_):
        if c and near(c, fill, tol):
            return i
    return 0


def cmd_auto(a):
    img = a.image
    if a.normalise:
        norm = (a.out or "spec") + ".normalised.png"
        cmd_normalise(argparse.Namespace(image=img, out=norm, size=a.size, chroma=35))
        img = norm
    w, h = size(img)
    fuzz = choose_fuzz(img, a.area, a.min_side) if a.fuzz is None else a.fuzz
    objs = components(img, fuzz, a.area)
    if not objs:
        sys.exit("no components found; try a larger --fuzz or smaller --area")
    bg = objs[0]
    print(f"{img}  {w}x{h}   {len(objs)} objects, ground {hexs(bg['colour'])}\n")

    # A panel is a component that is not the ground, not a hairline, and not
    # a glyph. Glyphs are what is left once those go, and they belong to
    # egui, so they are counted and not measured.
    panels = [o for o in objs[1:]
              if o["w"] >= a.min_side and o["h"] >= a.min_side
              and o["w"] * o["h"] >= a.area]

    # Drop nested near-duplicates. A fuzz low enough to keep a panel apart
    # from its ground is often low enough to return that panel TWICE — once
    # for its border ring and once for the fill inside it. Drawn back, the
    # pair is a button with a doubled edge, and it scored WORSE than the
    # run that had missed a whole panel. Keep the larger of any two that
    # overlap by most of the smaller's area.
    def overlap(p, q):
        w = min(p["x"] + p["w"], q["x"] + q["w"]) - max(p["x"], q["x"])
        h = min(p["y"] + p["h"], q["y"] + q["h"]) - max(p["y"], q["y"])
        return max(0, w) * max(0, h)
    kept = []
    for o in sorted(panels, key=lambda o: -o["w"] * o["h"]):
        small = o["w"] * o["h"]
        if any(overlap(o, k) > 0.7 * small for k in kept):
            continue
        kept.append(o)
    dropped = len(panels) - len(kept)
    panels = kept
    if dropped:
        print(f"({dropped} nested duplicates dropped)\n")
    glyphs = len(objs) - 1 - len(panels) - dropped

    # Repeated rows: panels sharing a top edge and a height are one row, and
    # a row is where the interesting question lives — rule, or hand-placed.
    rows, singles, panel_objs = {}, [], []
    for o in panels:
        rows.setdefault((o["y"], o["h"]), []).append(o)
    spec = dict(source=img, raster=f"{w}x{h}", constants=[], panels=[], roles={})

    for (y, ph), group in sorted(rows.items()):
        group.sort(key=lambda o: o["x"])
        if len(group) < 2:
            singles.extend(group)
            continue
        widths = [o["w"] for o in group]
        # Members of wildly different sizes are not a row that happens to
        # be uneven; they are separate things that share a top edge. The
        # footer's segments did exactly this and came back as a "row" with
        # a 213% spread, which is a true number and a useless one.
        if max(widths) > 3 * min(widths):
            singles.extend(group)
            continue
        gaps = [group[i + 1]["x"] - (group[i]["x"] + group[i]["w"])
                for i in range(len(group) - 1)]
        lead = group[0]["x"]
        trail = w - (group[-1]["x"] + group[-1]["w"])
        n = len(group)
        gap = round(sum(gaps) / len(gaps)) if gaps else 0
        even = (w - lead - trail - (n - 1) * gap) / n
        spread = max(widths) - min(widths)
        avg = sum(widths) / n
        print(f"ROW of {n} at y {y}, height {ph}")
        print(f"  margins {lead} / {trail}   gaps {min(gaps)}..{max(gaps)} (mean {gap})")
        print(f"  widths  {min(widths)}..{max(widths)} (mean {avg:.1f})   even rule {even:.1f}")
        if spread > 0.12 * avg:
            print(f"  VERDICT hand-placed ({100 * spread / avg:.0f}% spread) — take the even rule")
        else:
            print("  VERDICT a real grid — copy the widths")
        # Measure a TYPICAL member, not the first. The first key cell is
        # the lit one, and the first pass reported its blue as the row's
        # material — a state described as a style.
        base = mean([o["colour"] for o in group])
        first = min(group, key=lambda o: sum(abs(i - j) for i, j in zip(o["colour"], base)))
        r = anatomy(img, first["x"], y, first["w"], ph, bg["colour"])
        rad = corner_radius(img, first, r["top"]) if r["top"] else 0
        odd = [o for o in group if not near(o["colour"], base, a.lit)]
        print(f"  cell    {hexs(r['top'])} -> {hexs(r['foot'])}"
              f"  {'gradient' if r['gradient'] else 'flat'}"
              f"  border {hexs(r['border'])}  radius ~{rad}")
        # A member far from the row's mean colour is the LIT one.
        for o in odd:
            print(f"  lit     index {group.index(o)}, {hexs(o['colour'])} — a state, not a style")
        spec["constants"] += [
            dict(name=f"ROW{y}_MARGIN", value=lead, doc=f"Row at y {y}: clear space at each end.", min=0, max=64),
            dict(name=f"ROW{y}_GAP", value=gap, doc=f"Row at y {y}: between two cells.", min=0, max=32),
            dict(name=f"ROW{y}_H", value=ph, doc=f"Row at y {y}: one cell's height.", min=0, max=96),
        ]
        for o in group:
            spec["panels"].append(dict(
                name=f"row y{y} cell {group.index(o)}",
                rect=[o["x"], o["y"], o["w"], o["h"]],
                radius=rad,
                # A lit member keeps its own colour; the rest share the
                # row's material. Drawing them all from the representative
                # would erase exactly the state the row is reporting.
                top=hexs(o["colour"]) if o in odd else hexs(r["top"]),
                foot=hexs(o["colour"]) if o in odd else hexs(r["foot"]),
                border=hexs(r["border"]), border_px=r.get("border_px", 1), shadow=0.0))
            panel_objs.append((spec["panels"][-1], o))
            spec["panels"][-1]["_fill"] = r["top"]
        print()

    for o in sorted(singles, key=lambda o: -o["area"])[: a.limit]:
        r = anatomy(img, o["x"], o["y"], o["w"], o["h"], bg["colour"])
        rad = corner_radius(img, o, r["top"]) if r["top"] else 0
        print(f"PANEL {o['w']}x{o['h']} at {o['x']},{o['y']}   {hexs(o['colour'])}")
        print(f"  {hexs(r['top'])} -> {hexs(r['foot'])}"
              f"  {'gradient' if r['gradient'] else 'flat'}"
              f"  border {hexs(r['border'])}  radius ~{rad}")
        spec["panels"].append(dict(
            name=f"panel at {o['x']},{o['y']}", rect=[o["x"], o["y"], o["w"], o["h"]],
            radius=rad, top=hexs(r["top"]), foot=hexs(r["foot"]),
            border=hexs(r["border"]), border_px=r.get("border_px", 1), shadow=0.0))

    spec["texts"] = []
    if not a.no_text:
        print("\nreading text...")
        labelled = 0
        for pan, o in panel_objs:
            got = panel_text(img, o, pan.get("_fill"))
            if not got:
                continue
            txt, ink = got
            pan["label"] = txt
            spec["texts"].append(dict(
                text=txt, rect=[o["x"], o["y"], o["w"], o["h"]],
                colour=hexs(ink), size=round(o["h"] * 0.52), align="center"))
            labelled += 1
        # Anything not inside a panel: menu bars, captions, status words.
        loose = 0
        words = sparse_text(img, bg["colour"])
        # Size comes from the LINE, not the word. A word's own box height
        # includes whatever descenders it happens to contain, so "Settings"
        # measured taller than "File" and rendered visibly bigger on the
        # same menu bar. Words sharing a baseline share a size: the median
        # of the line, which no single g or y can move.
        lines = {}
        for wrd in words:
            lines.setdefault(round(wrd["y"] / 6), []).append(wrd)
        for group_ in lines.values():
            hs = sorted(x["h"] for x in group_)
            med = hs[len(hs) // 2]
            for x in group_:
                x["line_h"] = med
        for wrd in words:
            inside = any(o["x"] <= wrd["x"] and o["y"] <= wrd["y"]
                         and wrd["x"] + wrd["w"] <= o["x"] + o["w"]
                         and wrd["y"] + wrd["h"] <= o["y"] + o["h"]
                         for _, o in panel_objs)
            if inside:
                continue
            ink = ink_of(img, wrd["x"], wrd["y"], wrd["w"], wrd["h"], bg["colour"])
            spec["texts"].append(dict(
                text=wrd["text"], rect=[wrd["x"], wrd["y"], wrd["w"], wrd["h"]],
                colour=hexs(ink), size=round(wrd.get("line_h", wrd["h"]) * 1.05),
                align="left"))
            loose += 1
        print(f"  {labelled} panel labels, {loose} loose words")
        # Whatever had no readable word is a SHAPE. Extract it.
        spec["icons"] = []
        icondir = (a.out or "spec") + ".icons"
        # EVERY panel's ink, whether or not OCR read a word in it. The
        # string and the mask answer different questions: the app will draw
        # a real glyph and wants the string, while a recreation has to be
        # judged against the original and wants the shape. Relying on the
        # string alone left KCalc's divide sign as a full stop and its
        # multiply as an X, which is what OCR made of them.
        for i, (pan, o) in enumerate(panel_objs):
            got = extract_icon(img, o, pan.get("_fill"), icondir, i)
            if got:
                spec["icons"].append(got)
        for i, o in enumerate(objs[1:]):
            if o["w"] > 64 or o["h"] > 64 or o["w"] < 5 or o["h"] < 5:
                continue
            if any(abs(o["x"] - t["rect"][0]) < 4 and abs(o["y"] - t["rect"][1]) < 4
                   for t in spec["texts"]):
                continue
            if any(abs(o["x"] - g["rect"][0]) < 4 and abs(o["y"] - g["rect"][1]) < 4
                   for g in spec["icons"]):
                continue
            got = extract_icon(img, o, bg["colour"], icondir, 900 + i)
            if got:
                spec["icons"].append(got)
        masks = sum(1 for g in spec["icons"] if g["kind"] == "mask")
        print(f"  {len(spec['icons'])} icons ({masks} as masks, "
              f"{len(spec['icons']) - masks} kept as pixels)")
        sample = ", ".join(repr(t["text"]) for t in spec["texts"][:14])
        print(f"  {sample}")

    print(f"\n{glyphs} objects were too small to be surfaces.")
    spec["roles"]["ground"] = hexs(bg["colour"])
    if a.out:
        json.dump(spec, open(a.out, "w"), indent=2)
        print(f"\nwrote {a.out}   ->  tools/trace.py emit {a.out}")



# ------------------------------------------------------------------ render
#
# The loop closed. A spec that cannot be drawn back into something like the
# original is a spec that quietly lost something, and until you try you do
# not know what. This rebuilds the picture from the JSON alone — no access
# to the source image — so `diff` between the two is an honest score for the
# MEASUREMENT rather than for anybody's eye.
#
# ImageMagick does the drawing here because it is already the dependency and
# this is a check, not the product. The real consumer is a wgpu panel pass;
# this and that read the same spec.


def resolve_font(name):
    """A font ImageMagick will actually open.

    Its own font names are build-dependent — this machine knows Adwaita-*
    and not DejaVu-Sans — so ask fontconfig for a FILE instead. A path
    always works, and `--font` still accepts one directly.
    """
    if "/" in name:
        return name
    out = subprocess.run(["fc-match", "-f", "%{file}", name],
                         capture_output=True, text=True).stdout.strip()
    return out or name


def cmd_render(a):
    spec = json.load(open(a.spec))
    a.font = resolve_font(a.font)
    w, h = (int(v) for v in spec["raster"].split("x"))
    ground = spec.get("roles", {}).get("ground", "#000000")
    args = ["-size", f"{w}x{h}", f"xc:{ground}"]
    tiles = []
    import tempfile, os
    tmp = tempfile.mkdtemp(prefix="trace-render-")
    for i, pan in enumerate(spec.get("panels", [])):
        x, y, pw, ph = (int(round(v)) for v in pan["rect"])
        if pw < 1 or ph < 1:
            continue
        tile = os.path.join(tmp, f"{i}.png")
        top, foot = pan.get("top", ground), pan.get("foot", ground)
        # A vertical gradient, or a flat fill when the two ends agree.
        if top == foot:
            _magick(["-size", f"{pw}x{ph}", f"xc:{top}", tile])
        else:
            _magick(["-size", f"{pw}x{ph}", f"gradient:{top}-{foot}", tile])
        r = int(pan.get("radius", 0))
        border = pan.get("border")
        draw = []
        if border and pan.get("border_px", 0) >= 1:
            draw = ["-fill", "none", "-stroke", border, "-strokewidth", "1",
                    "-draw", f"roundrectangle 0.5,0.5 {pw - 1.5},{ph - 1.5} {r},{r}"]
        if draw:
            _magick([tile, *draw, tile])
        tiles.append((tile, x, y))
    _magick([*args, a.out])
    for tile, x, y in tiles:
        _magick([a.out, tile, "-geometry", f"+{x}+{y}", "-composite", a.out])
    icons_ = spec.get("icons", [])
    texts = [t for t in spec.get("texts", [])
             if not any(abs(t["rect"][0] - g["rect"][0]) < 4
                        and abs(t["rect"][1] - g["rect"][1]) < 4 for g in icons_)]
    for t in texts:
        x, y, tw, th = (int(round(v)) for v in t["rect"])
        pt = max(6, int(t.get("size", th)))
        col = t.get("colour") or "#ffffff"
        if t.get("align") == "center":
            _magick([a.out, "-font", a.font, "-pointsize", str(pt), "-fill", col,
                     "-gravity", "center",
                     "-annotate", f"{x + tw // 2 - w // 2:+d}{y + th // 2 - h // 2:+d}",
                     t["text"], a.out])
        else:
            _magick([a.out, "-font", a.font, "-pointsize", str(pt), "-fill", col,
                     "-gravity", "NorthWest", "-annotate", f"+{x}+{y}",
                     t["text"], a.out])
    icons = spec.get("icons", [])
    for g in icons:
        x, y = int(g["rect"][0]), int(g["rect"][1])
        if g["kind"] == "mask":
            # The mask tints: a flat colour, shown through the ink.
            tint = "/tmp/.trace-icon-tint.png"
            _magick(["-size", f"{int(g['rect'][2])}x{int(g['rect'][3])}",
                     f"xc:{g.get('colour', '#ffffff')}", g["file"],
                     "-alpha", "off", "-compose", "CopyOpacity", "-composite", tint])
            _magick([a.out, tint, "-geometry", f"+{x}+{y}", "-composite", a.out])
        else:
            _magick([a.out, g["file"], "-geometry", f"+{x}+{y}", "-composite", a.out])
    print(f"wrote {a.out}  ({len(tiles)} panels, {len(texts)} texts, "
          f"{len(icons)} icons from {a.spec})")
    print("The STRING may be wrong where OCR guessed; its box, size, colour and")
    print("alignment are measured, and those are what a layout is made of.")



# -------------------------------------------------------------------- text
#
# Text is layout. Leaving it out made the first KCalc recreation look
# hollow — the panels were right and the thing still read as a wireframe,
# because half of what a UI communicates is where its words sit.
#
# Two passes, because one does not work. Tesseract in sparse mode finds
# words — KCalc's File, Edit, Settings, Help, NORM — and misses every single
# digit, since a lone glyph gives a page segmenter nothing to segment. But
# by this point the panels are already known, so each button can be cropped
# and read on its own in single-character mode, which gets them all.
#
# The characters do not have to be right for the trace to be useful. Their
# BOX, SIZE, COLOUR and ALIGNMENT are the measurement; the string is a
# convenience, and a wrong one is visible immediately.


def _ocr(img_args, psm, whitelist=None):
    tmp = "/tmp/.trace-ocr.png"
    _magick([*img_args, tmp])
    cmd = ["tesseract", tmp, "-", "--psm", str(psm)]
    if whitelist:
        cmd += ["-c", f"tessedit_char_whitelist={whitelist}"]
    out = subprocess.run(cmd, capture_output=True, text=True)
    return out.stdout.strip()


def ink_of(img, x, y, w, h, fill):
    """The text's colour: the pixel in the box furthest from the fill."""
    px = [c for c in region(img, x, y, w, h).values() if c]
    if not px or not fill:
        return None
    return max(px, key=lambda c: sum(abs(i - j) for i, j in zip(c, fill)))


def panel_text(img, o, fill):
    """One panel's label, read on its own. Returns (text, ink) or None."""
    dark = luminance(fill) < 0.3 if fill else True
    args = [img, "-crop", f"{o['w'] - 4}x{o['h'] - 4}+{o['x'] + 2}+{o['y'] + 2}",
            "+repage", "-resize", "500%"]
    if dark:
        args.append("-negate")
    args += ["-colorspace", "Gray", "-threshold", "55%"]
    best = ""
    for psm in (10, 8):
        t = _ocr(args, psm)
        t = "".join(ch for ch in t if ch.isprintable()).strip()
        if len(t) > len(best):
            best = t
        if best and psm == 10 and len(best) == 1:
            break
    if not best or len(best) > 12:
        return None
    return best, ink_of(img, o["x"] + 2, o["y"] + 2, o["w"] - 4, o["h"] - 4, fill)


def sparse_text(img, ground, minconf=45):
    """Words anywhere on the image, with their boxes, at native scale."""
    dark = luminance(ground) < 0.3 if ground else True
    args = [img, "-resize", "300%"]
    if dark:
        args.append("-negate")
    args += ["-colorspace", "Gray"]
    tmp = "/tmp/.trace-ocr-sparse.png"
    _magick([*args, tmp])
    out = subprocess.run(["tesseract", tmp, "-", "--psm", "11", "tsv"],
                         capture_output=True, text=True).stdout
    found = []
    for line in out.splitlines()[1:]:
        f = line.split("\t")
        if len(f) < 12:
            continue
        try:
            conf = float(f[10])
        except ValueError:
            continue
        word = f[11].strip()
        if conf < minconf or not word:
            continue
        # back out of the 300% used to give tesseract something to chew on
        x, y, w, h = (int(int(v) / 3) for v in f[6:10])
        found.append(dict(text=word, x=x, y=y, w=w, h=h, conf=round(conf)))
    return found


# -------------------------------------------------------------------- icons
#
# What is left once surfaces and words are accounted for. A divide sign, a
# window's close cross, an app badge: shapes with no character to read, and
# OCR either returns nothing or returns a lie — KCalc's divide came back as
# a full stop, its multiply as an X.
#
# These are extracted as MASKS, not screenshots. The mask is the ink's
# coverage and the colour is carried beside it, which keeps the icon
# recolourable by theme and makes it a real trace rather than a pasted
# fragment. An icon with several colours in it cannot be a mask, so that
# one is kept as pixels and said to be.


def extract_icon(img, o, fill, outdir, idx, tol=42, inset=3):
    """One object's ink, as a mask plus a colour, or as pixels if it is
    genuinely multicoloured. Returns a spec entry, or None if the region
    holds no ink worth keeping."""
    import os
    os.makedirs(outdir, exist_ok=True)
    # Inset past the border ring. Cropping the whole object counted the
    # border as a second ink, so the one-colour test failed and twenty of
    # twenty-two glyphs fell back to being kept as pixels — a paste rather
    # than a trace.
    inset = min(inset, o["w"] // 4, o["h"] // 4)
    o = dict(o, x=o["x"] + inset, y=o["y"] + inset,
             w=max(1, o["w"] - 2 * inset), h=max(1, o["h"] - 2 * inset))
    px = region(img, o["x"], o["y"], o["w"], o["h"])
    ink = [c for c in px.values() if c and fill
           and sum(abs(i - j) for i, j in zip(c, fill)) > tol]
    if len(ink) < 6:
        return None
    # Is the ink one colour wearing antialiasing, or several?
    strong = sorted(ink, key=lambda c: -sum(abs(i - j) for i, j in zip(c, fill)))
    head = strong[: max(4, len(strong) // 5)]
    base = mean(head)
    spread = max(sum(abs(i - j) for i, j in zip(c, base)) for c in head)
    stem = os.path.join(outdir, f"icon{idx}")
    crop = ["-crop", f"{o['w']}x{o['h']}+{o['x']}+{o['y']}", "+repage"]
    if spread <= 90:
        # One ink: threshold against the fill into an alpha mask.
        out = stem + ".mask.png"
        _magick([img, *crop, "-colorspace", "Gray",
                 "-negate" if luminance(fill) > luminance(base) else "-auto-level",
                 "-auto-level", "-alpha", "off", out])
        return dict(kind="mask", file=out, rect=[o["x"], o["y"], o["w"], o["h"]],
                    colour=hexs(base))
    out = stem + ".png"
    _magick([img, *crop, out])
    return dict(kind="pixels", file=out, rect=[o["x"], o["y"], o["w"], o["h"]])


# The entrypoint stays at the very foot of this file. Three times now a new
# command has been appended below it and every one failed with a NameError
# at call time, because `main()` had already run before the functions it
# dispatches to existed. Append above this line.
if __name__ == "__main__":
    main()
