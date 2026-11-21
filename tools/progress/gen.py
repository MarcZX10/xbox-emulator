"""Render progress.svg and badge-*.svg from data.json.

One square per item: green = done, amber = partial, grey = not done.
Adapted from drycheetah84/TomBox tools/progress/gen.py (same visual language).

usage: py tools/progress/gen.py
"""
import json, os, math
here = os.path.dirname(os.path.abspath(__file__))
root = os.path.join(here, "..", "..")
cfg = json.load(open(os.path.join(here, "data.json"), encoding="utf-8"))
GREEN, AMBER, GREY, BG = "#2ea043", "#d29922", "#6e7681", "#0d1117"
COL = {"done": GREEN, "partial": AMBER, "none": GREY}

def squarify(items, x, y, w, h):
    out, items = [], list(items)
    total = sum(a for _, a in items)
    if total == 0 or w <= 0 or h <= 0:
        return out
    scale = w * h / total
    items = [(k, a * scale) for k, a in items]
    def worst(row, side):
        s = sum(a for _, a in row)
        return max(max(side * side * a / (s * s), s * s / (side * side * a)) for _, a in row)
    while items:
        side = min(w, h)
        row = [items.pop(0)]
        while items and worst(row + [items[0]], side) <= worst(row, side):
            row.append(items.pop(0))
        s = sum(a for _, a in row)
        if w >= h:
            cw, cy = s / h, y
            for k, a in row:
                out.append((k, x, cy, cw, a / cw)); cy += a / cw
            x += cw; w -= cw
        else:
            ch, cx = s / w, x
            for k, a in row:
                out.append((k, cx, y, a / ch, ch)); cx += a / ch
            y += ch; h -= ch
    return out

def text(x, y, s, size=12):
    return (f'<text x="{x:.2f}" y="{y:.2f}" font-family="sans-serif" font-size="{size}" '
            f'fill="#ffffff" stroke="{BG}" stroke-width="3" paint-order="stroke">{s}</text>')

def section(o, y, title, groups, height):
    flat = [s for items in groups.values() for (_, s) in items]
    n = len(flat)
    done = flat.count("done")
    partial = flat.count("partial")
    o.append(text(4, y + 21, f"{title}: {done}/{n} done, {partial} partial", 16))
    y += 30
    order = sorted(groups.items(), key=lambda kv: -len(kv[1]))
    for key, cx, cy, cw, ch in squarify([(k, len(v)) for k, v in order], 0, y, 1000, height):
        items = groups[key]
        st = sorted([s for (_, s) in items], key=["done","partial","none"].index)
        cnt = len(st)
        ix, iy, iw, ih = cx + 1, cy + 1, cw - 2, ch - 2
        cols = max(1, min(cnt, math.ceil(math.sqrt(cnt * iw / ih))))
        rows = math.ceil(cnt / cols)
        sw, sh = iw / cols, ih / rows
        o.append(f'<g><title>{key}: {st.count("done")}/{cnt}</title>')
        for i, s in enumerate(st):
            o.append(f'<rect x="{ix + (i % cols) * sw:.2f}" y="{iy + (i // cols) * sh:.2f}" '
                     f'width="{sw:.2f}" height="{sh:.2f}" fill="{COL[s]}" stroke="{BG}" stroke-width="0.5"/>')
        o.append(f'<rect x="{cx:.2f}" y="{cy:.2f}" width="{cw:.2f}" height="{ch:.2f}" '
                 f'fill="none" stroke="{BG}" stroke-width="2"/>')
        label = key[: max(0, int((cw - 6) / 7))]
        if label and ch >= 18:
            o.append(text(cx + 5, cy + 16, label))
        o.append("</g>")
    return y + height + 10

def badge(name, label, value, colour):
    lw, vw = 7 * len(label) + 12, 7 * len(value) + 12
    svg = (f'<svg xmlns="http://www.w3.org/2000/svg" width="{lw + vw}" height="20" role="img" '
           f'aria-label="{label}: {value}"><rect width="{lw}" height="20" fill="#555"/>'
           f'<rect x="{lw}" width="{vw}" height="20" fill="{colour}"/>'
           f'<g fill="#fff" font-family="Verdana,DejaVu Sans,sans-serif" font-size="11" text-anchor="middle">'
           f'<text x="{lw / 2}" y="14">{label}</text><text x="{lw + vw / 2}" y="14">{value}</text></g></svg>')
    out = os.path.join(root, "docs", "img", f"badge-{name}.svg")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    open(out, "w", encoding="utf-8").write(svg)

def pct_colour(p):
    return "#4c1" if p >= 90 else "#97ca00" if p >= 70 else "#dfb317" if p >= 40 else "#fe7d37"

o, y = [], 0
tot_done = tot_n = 0
for sec in cfg["sections"]:
    flat = [s for items in sec["groups"].values() for (_, s) in items]
    tot_done += flat.count("done")
    tot_n += len(flat)
    y = section(o, y, sec["title"], sec["groups"], sec["height"])

svg = (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1000 {y:.0f}" width="1000" height="{y:.0f}">\n'
       f'<rect x="0" y="0" width="1000" height="{y:.0f}" fill="{BG}"/>\n' + "\n".join(o) + "\n</svg>\n")
out_svg = os.path.join(root, "docs", "img", "progress.svg")
os.makedirs(os.path.dirname(out_svg), exist_ok=True)
open(out_svg, "w", encoding="utf-8").write(svg)

m = cfg["milestones"]
pct = 100 * tot_done / tot_n if tot_n else 0
badge("overall", "overall", f"{tot_done}/{tot_n} ({pct:.0f}%)", pct_colour(pct))
badge("milestones", "milestones", f'{m["done"]}/{m["total"]}', pct_colour(100 * m["done"] / m["total"]))
badge("titles", "titles examined", str(cfg["titles"]), "#007ec6")
badge("updated", "updated", cfg["updated"], "#555")

print(f"overall {tot_done}/{tot_n} ({pct:.1f}%)  svg height {y:.0f}")
