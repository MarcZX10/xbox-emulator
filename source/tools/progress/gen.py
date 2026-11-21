"""Render the README status images.  usage: py tools/progress/gen.py

docs/img/progress.svg is a map with one square per item (green = done, amber = partial,
grey = not done), grouped the way the data is grouped:
  * functions the test title imports (analysis/fortnite_exe.json + progress.json overrides)
  * Direct3D 12.x interface methods (slot ranges in progress.json)
  * RDNA2 instructions: every mnemonic the decoder knows (rdna2/mnem.rs); done when the test
    title's shaders use it and every shader using it translates (tools/progress/mnemonics.txt,
    written by `xscan <pak> --mnemonics 1`)
Badges (docs/img/badge-*.svg) carry the headline numbers.
"""
import json, os, re, math
here = os.path.dirname(os.path.abspath(__file__))
root = os.path.join(here, "..", "..")
cfg = json.load(open(os.path.join(here, "progress.json"), encoding="utf-8"))
GREEN, AMBER, GREY, BG = "#2ea043", "#d29922", "#6e7681", "#0d1117"
COL = {"done": GREEN, "partial": AMBER, "none": GREY}

def ranges(s):
    out = set()
    for part in filter(None, s.split(",")):
        a, _, b = part.partition("-")
        out.update(range(int(a), int(b or a) + 1))
    return out

# ---- data ---------------------------------------------------------------------------
def imports():
    exe = json.load(open(os.path.join(root, "analysis", "fortnite_exe.json"), encoding="utf-8"))
    ov = cfg["import_overrides"]
    groups = {}
    mods = [(m["module"], m["functions"]) for m in exe["imports"]] + [(m, ["(delay-loaded)"]) for m in exe.get("delay_imports", [])]
    for mod, funcs in mods:
        low = mod.lower()
        if low.startswith("api-ms-win-crt") or low.startswith("vcruntime") or low.startswith("msvcp"):
            g = "C/C++ runtime"
        elif low.startswith("api-ms-") or low.startswith("ext-ms-"):
            g = "Windows API sets"
        else:
            g = mod.rsplit(".", 1)[0]
        for f in funcs:
            groups.setdefault(g, []).append(ov.get(f, ov.get(mod, "done")))
    return groups

def d3d12x():
    groups = {}
    for name in ("Device", "CommandList"):
        d = cfg["d3d12x"][name]
        done, part = ranges(d["done"]), ranges(d["partial"])
        groups[name] = ["done" if i in done else "partial" if i in part else "none" for i in range(d["total"])]
    return groups

def instructions():
    src = open(os.path.join(root, "crates", "tombox-xshader", "src", "rdna2", "mnem.rs"), encoding="utf-8").read()
    used = {}
    for line in open(os.path.join(here, "mnemonics.txt"), encoding="utf-8"):
        n, u, ok = line.split()
        used[n] = (int(u), int(ok))
    groups, cur, seen = {}, None, set()
    for line in src.split("\n"):
        m = re.match(r"pub fn (\w+)\(", line)
        if m:
            cur = {"lookup": "VINTRP"}.get(m.group(1), m.group(1).upper())
        for n in re.findall(r'=> "([a-z0-9_]+)"', line):
            if n in seen:
                continue
            seen.add(n)
            u, ok = used.get(n, (0, 0))
            groups.setdefault(cur, []).append("done" if u and u == ok else "partial" if u else "none")
    # mnemonics the decoder produces outside the tables (exports, DS ops, ...)
    for n, (u, ok) in used.items():
        if n not in seen:
            g = "EXP" if n == "exp" else "DS" if n.startswith("ds_") else "OTHER"
            groups.setdefault(g, []).append("done" if u == ok else "partial")
    return groups

# ---- drawing ------------------------------------------------------------------------
def squarify(items, x, y, w, h):
    """items: [(key, area)] sorted descending; returns [(key, x, y, w, h)]."""
    out, items = [], list(items)
    total = sum(a for _, a in items)
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
    return (f'<text x="{x:.2f}" y="{y:.2f}" font-family="sans-serif" font-size="{size}" fill="#ffffff" stroke="{BG}" '
            f'stroke-width="3" paint-order="stroke">{s}</text>')

def section(o, y, title, groups, height):
    n = sum(len(v) for v in groups.values())
    done = sum(v.count("done") for v in groups.values())
    o.append(text(4, y + 21, f"{title}: {100 * done / n:.2f}% ({done}/{n})", 16))
    y += 30
    order = sorted(groups.items(), key=lambda kv: -len(kv[1]))
    for key, cx, cy, cw, ch in squarify([(k, len(v)) for k, v in order], 0, y, 1000, height):
        st = sorted(groups[key], key=["done", "partial", "none"].index)
        cnt = len(st)
        ix, iy, iw, ih = cx + 1, cy + 1, cw - 2, ch - 2
        cols = max(1, min(cnt, math.ceil(math.sqrt(cnt * iw / ih))))
        rows = math.ceil(cnt / cols)
        sw, sh = iw / cols, ih / rows
        o.append(f'<g><title>{key}: {st.count("done")}/{cnt} ({100 * st.count("done") // cnt}%)</title>')
        for i, s in enumerate(st):
            o.append(f'<rect x="{ix + (i % cols) * sw:.2f}" y="{iy + (i // cols) * sh:.2f}" width="{sw:.2f}" height="{sh:.2f}" '
                     f'fill="{COL[s]}" stroke="{BG}" stroke-width="0.5"/>')
        o.append(f'<rect x="{cx:.2f}" y="{cy:.2f}" width="{cw:.2f}" height="{ch:.2f}" fill="none" stroke="{BG}" stroke-width="2"/>')
        label = key[: max(0, int((cw - 6) / 7))]
        if label and ch >= 18:
            o.append(text(cx + 5, cy + 16, label))
        o.append("</g>")
    return y + height + 10, done, n

def badge(name, label, value, colour):
    lw, vw = 7 * len(label) + 12, 7 * len(value) + 12
    svg = (f'<svg xmlns="http://www.w3.org/2000/svg" width="{lw + vw}" height="20" role="img" aria-label="{label}: {value}">'
           f'<rect width="{lw}" height="20" fill="#555"/><rect x="{lw}" width="{vw}" height="20" fill="{colour}"/>'
           f'<g fill="#fff" font-family="Verdana,DejaVu Sans,sans-serif" font-size="11" text-anchor="middle">'
           f'<text x="{lw / 2}" y="14">{label}</text><text x="{lw + vw / 2}" y="14">{value}</text></g></svg>')
    open(os.path.join(root, "docs", "img", f"badge-{name}.svg"), "w", encoding="utf-8").write(svg)

def pct_colour(p):
    return "#4c1" if p >= 90 else "#97ca00" if p >= 70 else "#dfb317" if p >= 40 else "#fe7d37"

o, y = [], 0
y, a_done, a_n = section(o, y, "Imported system functions*", imports(), 300)
y, b_done, b_n = section(o, y, "Direct3D 12.x interface methods", d3d12x(), 170)
y, c_done, c_n = section(o, y, "RDNA2 shader instructions translated", instructions(), 260)
stages = {k: [v] for k, v in cfg["stages"].items() if k != "comment"}
y, d_done, d_n = section(o, y, "Shader stages translated", stages, 60)
svg = (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1000 {y:.0f}" width="1000" height="{y:.0f}">\n'
       f'<rect x="0" y="0" width="1000" height="{y:.0f}" fill="{BG}"/>\n' + "\n".join(o) + "\n</svg>\n")
open(os.path.join(root, "docs", "img", "progress.svg"), "w", encoding="utf-8").write(svg)
m, s = cfg["milestones"], cfg["shaders"]
badge("imports", "system functions*", f"{100 * a_done / a_n:.2f}%", pct_colour(100 * a_done / a_n))
badge("d3d12x", "Direct3D 12.x methods", f"{100 * b_done / b_n:.2f}%", pct_colour(100 * b_done / b_n))
badge("instructions", "RDNA2 instructions", f"{100 * c_done / c_n:.2f}%", pct_colour(100 * c_done / c_n))
badge("shaders", "shaders translated", f'{s["translated"]:,}/{s["total"]:,}', "#4c1")
badge("titles", "titles examined", str(cfg.get("titles", 1)), "#007ec6")
badge("milestones", "milestones", f'{m["done"]}/{m["total"]}', pct_colour(100 * m["done"] / m["total"]))
print(f"imports {a_done}/{a_n}  d3d12x {b_done}/{b_n}  instructions {c_done}/{c_n}  height {y:.0f}")
