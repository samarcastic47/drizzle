#!/usr/bin/env python3
"""Render docs/supply.svg: $DRIZZLE supply over 60 days of real weather.

Stdlib only (no matplotlib). Runs sim.run() for four scenarios and draws a
hand-written SVG line chart.

Usage: python3 tools/supply_chart.py
"""
import math
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)

import sim  # noqa: E402

RAIN = os.path.join(ROOT, "rain_60d.json")
OUT = os.path.join(ROOT, "docs", "supply.svg")

EMISSION = 100.0
MIXED = {"forecaster": 0.4, "herd": 0.3, "loyal": 0.2, "idle": 0.1}
MIXED_NO_IDLE = {"forecaster": 0.4, "herd": 0.3, "loyal": 0.3}

# (label, kwargs, css color, stroke width, dash array)
SERIES = [
    ("evap 0.10% (chosen), mixed players",
     dict(emission=EMISSION, evap=0.001, mix=MIXED), "#1f6feb", 2.6, None),
    ("evap 0.05%, mixed",
     dict(emission=EMISSION, evap=0.0005, mix=MIXED), "#8b949e", 1.2, "6 4"),
    ("evap 0.25%, mixed",
     dict(emission=EMISSION, evap=0.0025, mix=MIXED), "#d29922", 1.2, "6 4"),
    ("0.10% + 200/day rain-danced into Dubai",
     dict(emission=EMISSION, evap=0.001, mix=MIXED_NO_IDLE, dances=200),
     "#a371f7", 1.3, "2 3"),
]

W, H = 800, 420
X0, X1 = 80, 772          # plot left/right
Y0, Y1 = 62, 362          # plot top/bottom
DAYS = 60
START_SUPPLY = 100000.0

BG = "#ffffff"
INK = "#1f2328"
MUTED = "#57606a"
GRID = "#eaeef2"
AXIS = "#d0d7de"
FONT = "system-ui, sans-serif"


def fmt_k(v):
    """125000 -> '125k', 1500000 -> '1.5M'."""
    a = abs(v)
    if a >= 1e6:
        return f"{v / 1e6:g}M"
    if a >= 1e3:
        return f"{v / 1e3:g}k"
    return f"{v:g}"


def nice_ticks(lo, hi, target=5):
    """Multiples of a 1/2/2.5/5 step inside [lo, hi], count closest to target."""
    best = None
    for exp in range(-2, 12):
        for m in (1.0, 2.0, 2.5, 5.0):
            step = m * 10.0 ** exp
            start = math.ceil(lo / step - 1e-9) * step
            ticks = []
            v = start
            while v <= hi + 1e-9 and len(ticks) < 200:
                ticks.append(v)
                v += step
            if not ticks:
                continue
            key = (abs(len(ticks) - target), step)
            if best is None or key < best[0]:
                best = (key, ticks)
    return [round(t, 6) for t in best[1]] if best else [lo, hi]


def build():
    cities, hours = sim.load(RAIN)
    rows = []
    for label, kwargs, color, width, dash in SERIES:
        curve = sim.run(cities, hours, **kwargs)["curve_daily"]
        values = [START_SUPPLY] + [float(v) for v in curve]
        rows.append(dict(label=label, color=color, width=width,
                         dash=dash, values=values))

    lo = min(min(r["values"]) for r in rows)
    hi = max(max(r["values"]) for r in rows)
    ymin = math.floor(lo / 10000.0) * 10000.0
    ymax = math.ceil(hi / 10000.0) * 10000.0
    if ymax == ymin:
        ymax = ymin + 10000.0

    def px(day):
        return X0 + (day / DAYS) * (X1 - X0)

    def py(v):
        return Y1 - (v - ymin) / (ymax - ymin) * (Y1 - Y0)

    out = []
    add = out.append
    add(f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" '
        f'width="{W}" height="{H}" font-family="{FONT}">')
    add(f'<title>$DRIZZLE supply over 60 days of real weather</title>')
    add(f'<rect x="0" y="0" width="{W}" height="{H}" fill="{BG}"/>')

    # title
    add(f'<text x="{W / 2:.0f}" y="28" text-anchor="middle" font-size="15" '
        f'fill="{INK}">$DRIZZLE supply over 60 days of real weather '
        f'(12 cities, Open-Meteo, Sep 12 \u2013 Nov 10 2026)</text>')

    # y gridlines + labels
    ticks = nice_ticks(ymin, ymax)
    for t in ticks:
        y = py(t)
        add(f'<line x1="{X0}" y1="{y:.1f}" x2="{X1}" y2="{y:.1f}" '
            f'stroke="{GRID}" stroke-width="1"/>')
        add(f'<text x="{X0 - 8}" y="{y + 3.5:.1f}" text-anchor="end" '
            f'font-size="11" fill="{MUTED}">{fmt_k(t)}</text>')

    # axes
    add(f'<line x1="{X0}" y1="{Y0}" x2="{X0}" y2="{Y1}" '
        f'stroke="{AXIS}" stroke-width="1"/>')
    add(f'<line x1="{X0}" y1="{Y1}" x2="{X1}" y2="{Y1}" '
        f'stroke="{AXIS}" stroke-width="1"/>')

    # x ticks every 10 days
    for day in range(10, DAYS + 1, 10):
        x = px(day)
        add(f'<line x1="{x:.1f}" y1="{Y1}" x2="{x:.1f}" y2="{Y1 + 5}" '
            f'stroke="{AXIS}" stroke-width="1"/>')
        add(f'<text x="{x:.1f}" y="{Y1 + 19}" text-anchor="middle" '
            f'font-size="11" fill="{MUTED}">day {day}</text>')

    # initial-supply reference line
    yref = py(START_SUPPLY)
    add(f'<line x1="{X0}" y1="{yref:.1f}" x2="{X1}" y2="{yref:.1f}" '
        f'stroke="{MUTED}" stroke-width="1.2" stroke-dasharray="4 3" '
        f'opacity="0.75"/>')
    add(f'<text x="{X1 - 6}" y="{yref - 5:.1f}" text-anchor="end" '
        f'font-size="10" fill="{MUTED}">initial 100k</text>')

    # series
    for r in rows:
        pts = " ".join(f"{px(i):.1f},{py(v):.1f}"
                       for i, v in enumerate(r["values"]))
        dash = f' stroke-dasharray="{r["dash"]}"' if r["dash"] else ""
        add(f'<polyline points="{pts}" fill="none" stroke="{r["color"]}" '
            f'stroke-width="{r["width"]}" stroke-linejoin="round" '
            f'stroke-linecap="round"{dash}/>')

    # legend, top-left inside the plot
    lx, ly = X0 + 10, Y0 + 10
    lw, lh = 262, 4 * 17 + 14
    add(f'<rect x="{lx}" y="{ly}" width="{lw}" height="{lh}" rx="4" '
        f'fill="{BG}" stroke="{AXIS}" stroke-width="1" opacity="0.96"/>')
    for i, r in enumerate(rows):
        ty = ly + 18 + i * 17
        dash = f' stroke-dasharray="{r["dash"]}"' if r["dash"] else ""
        add(f'<line x1="{lx + 10}" y1="{ty - 3.5:.1f}" x2="{lx + 30}" '
            f'y2="{ty - 3.5:.1f}" stroke="{r["color"]}" '
            f'stroke-width="{r["width"]}" stroke-linecap="round"{dash}/>')
        add(f'<text x="{lx + 38}" y="{ty}" font-size="11" '
            f'fill="{INK}">{r["label"]}</text>')

    add('</svg>')

    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as fh:
        fh.write("\n".join(out) + "\n")

    for r in rows:
        print(f'{r["label"]:52} final supply {r["values"][-1]:>10,.0f}')
    return rows


if __name__ == "__main__":
    build()
