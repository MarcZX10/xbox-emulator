import json, os, math
from xml.sax.saxutils import escape

here = os.path.dirname(os.path.abspath(__file__))
root = os.path.normpath(os.path.join(here, "..", ".."))
dat = os.path.join(here, "data")
img = os.path.join(root, "docs", "img")
cfg = json.load(open(os.path.join(here, "progress.json"), encoding="utf-8"))
GREEN, AMBER, GREY, BG = "#2ea043", "#d29922", "#6e7681", "#0d1117"
COL = {"done": GREEN, "partial": AMBER, "none": GREY}
XCOL = {"implemented": "#2ea043", "forwarded": "#388bfd", "hooked": "#a371f7", "pc": "#2f8f83",
        "title": "#56b6c2", "stub": "#d29922", "unverified": "#3d444d", "missing": "#6e7681"}
XORDER = ["implemented", "forwarded", "hooked", "pc", "title", "stub", "unverified", "missing"]
COVERED = {"implemented", "forwarded", "hooked", "pc", "title"}
LABEL = {"implemented": "implemented in emulator", "forwarded": "forwarded by emulator to PC",
         "hooked": "PC system DLL, hooked by emulator", "pc": "PC system DLL (name match)",
         "title": "shipped with the title", "stub": "emulator stub", "unverified": "unverified",
         "missing": "missing"}
TOMBOX = "https://github.com/drycheetah84/TomBox/blob/master/"


def jload(name):
    return json.load(open(os.path.join(dat, name), encoding="utf-8"))


def ranges(s):
    out = set()
    for part in filter(None, s.split(",")):
        a, _, b = part.partition("-")
        out.update(range(int(a), int(b or a) + 1))
    return out


def imports():
    exe = jload("fortnite_exe.json")
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
    used = {}
    for line in open(os.path.join(dat, "mnemonics.txt"), encoding="utf-8"):
        n, u, ok = line.split()
        used[n] = (int(u), int(ok))
    groups, seen = {}, set()
    for g, names in jload("mnem_groups.json"):
        for n in names:
            seen.add(n)
            u, ok = used.get(n, (0, 0))
            groups.setdefault(g, []).append("done" if u and u == ok else "partial" if u else "none")
    for n, (u, ok) in used.items():
        if n not in seen:
            g = "EXP" if n == "exp" else "DS" if n.startswith("ds_") else "OTHER"
            groups.setdefault(g, []).append("done" if u == ok else "partial")
    return groups


def squarify(items, x, y, w, h):
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
                out.append((k, x, cy, cw, a / cw))
                cy += a / cw
            x += cw
            w -= cw
        else:
            ch, cx = s / w, x
            for k, a in row:
                out.append((k, cx, y, a / ch, ch))
                cx += a / ch
            y += ch
            h -= ch
    return out


def text(x, y, s, size=12, fill="#ffffff"):
    return (f'<text x="{x:.2f}" y="{y:.2f}" font-family="sans-serif" font-size="{size}" fill="{fill}" stroke="{BG}" '
            f'stroke-width="3" paint-order="stroke">{escape(s)}</text>')


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
        o.append(f'<g><title>{escape(key)}: {st.count("done")}/{cnt} ({100 * st.count("done") // cnt}%)</title>')
        for i, s in enumerate(st):
            o.append(f'<rect x="{ix + (i % cols) * sw:.2f}" y="{iy + (i // cols) * sh:.2f}" width="{sw:.2f}" height="{sh:.2f}" '
                     f'fill="{COL[s]}" stroke="{BG}" stroke-width="0.5"/>')
        o.append(f'<rect x="{cx:.2f}" y="{cy:.2f}" width="{cw:.2f}" height="{ch:.2f}" fill="none" stroke="{BG}" stroke-width="2"/>')
        label = key[: max(0, int((cw - 6) / 7))]
        if label and ch >= 18:
            o.append(text(cx + 5, cy + 16, label))
        o.append("</g>")
    return y + height + 10, done, n


def lighten(c, t=0.45):
    r, g, b = (int(c[i:i + 2], 16) for i in (1, 3, 5))
    return "#%02x%02x%02x" % tuple(int(v + (255 - v) * t) for v in (r, g, b))


def title_import_index():
    ti = jload("title_imports.json")
    direct, by_func = {}, {}
    for title, t in ti.items():
        for mods in t["binaries"].values():
            for m in mods:
                low = m["module"].lower()
                for f in m["functions"]:
                    if low.startswith(("api-ms-", "ext-ms-")):
                        by_func.setdefault(f, set()).add(title)
                    else:
                        direct.setdefault((low, f), set()).add(title)
    return ti, direct, by_func


HOST_PRIORITY = ["kernelbase.dll", "ucrtbase.dll", "ntdll.dll", "combase.dll", "msvcp_win.dll", "rpcrt4.dll",
                 "sechost.dll", "minuserx.dll", "oleaut32.dll"]


def console():
    con = jload("console_exports.json")
    ti, direct, by_func = title_import_index()
    emu = {k.lower(): v for k, v in cfg["emulator"].items()}
    hooked = {k: set(v) for k, v in cfg["hooked"].items() if k != "file"}
    host = {}
    for lib in sorted(con, key=lambda l: (HOST_PRIORITY.index(l.lower()) if l.lower() in HOST_PRIORITY else 99, l.lower())):
        for n, has, fwd, pres in con[lib]["exports"]:
            if fwd is None and n in by_func and n not in host:
                host[n] = lib.lower()
    libs = {}
    for lib, v in con.items():
        low = lib.lower()
        rows = []
        for n, has, fwd, pres in v["exports"]:
            if low in emu:
                st = emu[low]["functions"].get(n, "missing")
            elif has == 1:
                st = "hooked" if n in hooked.get(low, ()) else "pc"
            elif has == 2:
                st = "unverified"
            else:
                st = "missing"
            users = set(direct.get((low, n), set()))
            if host.get(n) == low:
                users |= by_func.get(n, set())
            rows.append((n, st, frozenset(users), tuple(pres)))
        libs[lib] = {"rows": rows, "pc_dll": v["pc_dll"], "dir": v["dir"], "versions": v["versions"]}
    for g, v in cfg["gdk_linked"].items():
        libs[g] = {"rows": [(n, "implemented", frozenset(["Fortnite"]), ("Fortnite",)) for n in v["functions"]],
                   "pc_dll": False, "dir": "Fortnite EXE (statically linked)", "versions": {}}
    return libs


def console_section(o, y, libs, height):
    allrows = [r for v in libs.values() for r in v["rows"]]
    n = len(allrows)
    cov = sum(1 for r in allrows if r[1] in COVERED)
    xo = [r for v in libs.values() if not v["pc_dll"] for r in v["rows"]]
    xcov = sum(1 for r in xo if r[1] in COVERED)
    o.append(text(4, y + 21, f"Xbox Series system library exports: {100 * cov / n:.2f}% ({cov}/{n}); "
                             f"Xbox-only libraries {100 * xcov / len(xo):.2f}% ({xcov}/{len(xo)})", 16))
    y += 30
    lx = 4
    for s in XORDER:
        o.append(f'<rect x="{lx}" y="{y + 2}" width="10" height="10" fill="{XCOL[s]}"/>')
        o.append(text(lx + 14, y + 11, s, 11))
        lx += 14 + 7 * len(s) + 14
    o.append(f'<rect x="{lx}" y="{y + 2}" width="10" height="10" fill="{lighten(XCOL["pc"])}"/>')
    o.append(text(lx + 14, y + 11, "lighter = imported by an analysed title", 11))
    y += 18
    order = sorted(libs.items(), key=lambda kv: -len(kv[1]["rows"]))
    for key, cx, cy, cw, ch in squarify([(k, len(v["rows"])) for k, v in order], 0, y, 1000, height):
        rows = sorted(libs[key]["rows"], key=lambda r: (XORDER.index(r[1]), not r[2]))
        cnt = len(rows)
        ix, iy, iw, ih = cx + 1, cy + 1, cw - 2, ch - 2
        cols = max(1, min(cnt, math.ceil(math.sqrt(cnt * iw / ih))))
        nrows = math.ceil(cnt / cols)
        sw, sh = iw / cols, ih / nrows
        c = sum(1 for r in rows if r[1] in COVERED)
        per = {}
        for r in rows:
            for t in r[2]:
                per[t] = per.get(t, 0) + 1
        used = ", ".join(f"{t} imports {k}" for t, k in sorted(per.items())) or "not imported by an analysed title"
        o.append(f'<g><title>{escape(key)}: {c}/{cnt} covered; {escape(used)}</title>')
        i = 0
        while i < cnt:
            row, col = divmod(i, cols)
            fill = lighten(XCOL[rows[i][1]]) if rows[i][2] else XCOL[rows[i][1]]
            j = i + 1
            while j < cnt and j // cols == row and rows[j][1] == rows[i][1] and bool(rows[j][2]) == bool(rows[i][2]):
                j += 1
            o.append(f'<rect x="{ix + col * sw:.2f}" y="{iy + row * sh:.2f}" width="{(j - i) * sw:.2f}" height="{sh:.2f}" fill="{fill}"/>')
            i = j
        if sw >= 3 and sh >= 3:
            for k in range(1, nrows):
                o.append(f'<line x1="{ix:.2f}" y1="{iy + k * sh:.2f}" x2="{ix + iw:.2f}" y2="{iy + k * sh:.2f}" stroke="{BG}" stroke-width="0.5"/>')
        o.append(f'<rect x="{cx:.2f}" y="{cy:.2f}" width="{cw:.2f}" height="{ch:.2f}" fill="none" stroke="{BG}" stroke-width="2"/>')
        label = key.rsplit(".", 1)[0] if key.lower().endswith(".dll") else key
        label = label[: max(0, int((cw - 6) / 7))]
        if label and ch >= 18:
            o.append(text(cx + 5, cy + 16, label))
        o.append("</g>")
    return y + height + 10, cov, n, xcov, len(xo)


def badge(name, label, value, colour):
    lw, vw = 7 * len(label) + 12, 7 * len(value) + 12
    svg = (f'<svg xmlns="http://www.w3.org/2000/svg" width="{lw + vw}" height="20" role="img" aria-label="{escape(label)}: {escape(value)}">'
           f'<rect width="{lw}" height="20" fill="#555"/><rect x="{lw}" width="{vw}" height="20" fill="{colour}"/>'
           f'<g fill="#fff" font-family="Verdana,DejaVu Sans,sans-serif" font-size="11" text-anchor="middle">'
           f'<text x="{lw / 2}" y="14">{escape(label)}</text><text x="{lw + vw / 2}" y="14">{escape(value)}</text></g></svg>')
    open(os.path.join(img, f"badge-{name}.svg"), "w", encoding="utf-8").write(svg)


def pct_colour(p):
    return "#4c1" if p >= 90 else "#97ca00" if p >= 70 else "#dfb317" if p >= 40 else "#fe7d37"


def metric(d, t):
    return {"done": d, "total": t, "pct": round(100 * d / t, 2) if t else None}




def tcfg(title):
    return next((t for t in cfg["titles_analysed"] if t["name"] == title), {})


def classify_title_import(title, shipped, binary, mod, f, pcflag):
    tc = tcfg(title)
    low = mod.lower()
    emu = {k.lower(): v for k, v in cfg["emulator"].items()}
    hooked = {k: set(v) for k, v in cfg["hooked"].items() if k != "file"}
    if binary.lower() in {b.lower() for b in tc.get("unverified_binaries", [])}:
        return "unverified", f"{binary} is delay-loaded by the EXE and never appears in run logs"
    if low in emu:
        st = emu[low]["functions"].get(f, "missing")
        note = cfg["notes"].get(f, "") if st != "missing" else f"not exported by the emulator's {mod}"
        return st, f"[{emu[low]['file']}]({TOMBOX}{emu[low]['file']})" + (f" ({note})" if note else "")
    if low in {m.lower() for m in tc.get("unverified_modules", [])}:
        return "unverified", "delay-loaded; no load seen in run logs"
    if low in shipped:
        return "title", f"shipped with the title ({mod})"
    if f in hooked.get(low, ()):
        return "hooked", f"PC System32\\{mod}; [{cfg['hooked']['file']}]({TOMBOX}{cfg['hooked']['file']})"
    if pcflag == 2:
        o = jload("pc_ordinals.json").get(mod, {}).get(f)
        if o:
            return "pc", f"PC System32\\{mod} ordinal {f} = {o} on PC (same meaning on console not verified)"
        return "unverified", f"ordinal import {f} exists on PC {mod}; meaning not checked"
    if pcflag == 1:
        return "pc", f"PC System32\\{mod}" + ("; resolved by the OS loader in our runs" if tc.get("run_evidence") else "; export name match")
    return "missing", f"no emulator export, no PC export named {f} in {mod}"


def title_rows(title, t):
    shipped = {b.rsplit("/", 1)[-1].lower() for b in t["binaries"]}
    out = []
    for binary in sorted(t["binaries"], key=lambda b: (not b.lower().endswith(".exe"), "/" in b, b.lower())):
        for m in t["binaries"][binary]:
            rows = [(f, *classify_title_import(title, shipped, binary, m["module"], f, pf)) for f, pf in zip(m["functions"], m["pc"])]
            out.append((binary, m, rows))
    return out


def library_map(summary, libs):
    ti = jload("title_imports.json")
    L = []
    w = L.append
    w("# Library map")
    w("")
    w(f"Generated by `tools/progress/gen.py` from `tools/progress/data/` (refresh with `tools/progress/sync.py`). Upstream: TomBox `{cfg['upstream']['commit']}`; emulator source paths link to upstream. Titles analysed: "
      + ", ".join(f"{k} (console OS {v['os_build']})" for k, v in ti.items()) + ".")
    w("")
    w("## Statuses")
    w("")
    w("| Status | Meaning |")
    w("|---|---|")
    w("| implemented | the emulator provides its own implementation |")
    w("| forwarded | an emulator DLL exports it and forwards to a PC system component (D3D12, Media Foundation, DirectStorage) |")
    w("| hooked | resolved from a PC system DLL; the emulator patches the entry point at run time (network policy) |")
    w("| pc | a PC System32 DLL of the same name exports the same name; resolving is not the same as behaving like the console |")
    w("| title | provided by a DLL that ships with the title |")
    w("| stub | the emulator exports it but returns a fixed result (E_NOTIMPL / S_OK / inert object) |")
    w("| unverified | not observed loading, or an ordinal export whose console meaning was not checked |")
    w("| missing | no emulator export and no PC export of that name |")
    w("")
    w("## Headline numbers")
    w("")
    w("Upstream's metric set (same definitions as upstream `tools/progress/gen.py`, Fortnite EXE for system functions), then our additions:")
    w("")
    w("| Metric | Done | Total | % |")
    w("|---|---|---|---|")
    for k, v in list(summary["headline"].items()) + list(summary["extra"].items()):
        if isinstance(v, dict) and "total" in v:
            w(f"| {k} | {v['done'] if v['done'] is not None else 'not published'} | {v['total'] if v['total'] is not None else '-'} | {v['pct'] if v['pct'] is not None else '-'} |")
    w("")
    w("Shader compile: all 139 shaders of the lobby frame compile with DXC (TomBox `docs/DEVELOPMENT_LOG.md:894`); 25/25 fixtures compile (`:672`). The full-library compile sweep (`:706`) has no published final count, so no compile badge is shown.")
    w("")
    w("Direct3D 12.x: 160 / 128 are the table sizes the wrappers allocate (`d3d12_xs/src/device_wrap.rs`, `cmdlist_wrap.rs`), not documented interface sizes. Device: done = forwarded PC slots 0-43 plus intercepted Xbox slots 44, 48, 55, 60-62, 73, 78-81; partial = logging S_OK stubs 45-90. CommandList: done = 0-26, 32-52 (PC methods shifted by 4), 59, 67, 68, 83; 27-31 are Xbox-inserted stubs and 53-127 log-and-ignore probes, counted as not done.")
    w("")
    counts = {}
    for title, t in ti.items():
        tc = tcfg(title)
        counts[title] = {}
        w(f"## {title} imports" + (f" ({tc['build']})" if tc.get("build") else ""))
        w("")
        if tc.get("summary"):
            w(tc["summary"])
            w("")
        if tc.get("run_evidence"):
            w("Every static import of the EXE and its statically loaded DLLs resolved at load: the process booted and presented frames in the 2026-11-21 runs. Delay imports resolve only when first called.")
        else:
            w("Static analysis only: this title has not been run. `pc` = a PC System32 DLL exports that name.")
        w("")
        cur = None
        for binary, m, rows in title_rows(title, t):
            if binary != cur:
                w(f"### {title}: {binary}")
                w("")
                cur = binary
            c = {}
            for _, s, _ in rows:
                c[s] = c.get(s, 0) + 1
                counts[title][s] = counts[title].get(s, 0) + 1
            w(f"<details><summary><b>{escape(m['module'])}</b>{' (delay-loaded)' if m['kind'] == 'delay' else ''}: {len(rows)} functions; "
              + ", ".join(f"{s} {c[s]}" for s in XORDER if s in c) + "</summary>")
            w("")
            w("| Function | Status | Implemented in / evidence |")
            w("|---|---|---|")
            for f, s, e in rows:
                w(f"| `{f}` | {s} | {e} |")
            w("")
            w("</details>")
            w("")
        w(f"{title} import totals ({len(t['binaries'])} binaries): " + ", ".join(f"{s} {counts[title][s]}" for s in XORDER if s in counts[title]) + ".")
        w("")
        users = {}
        for o, ot in ti.items():
            if o != title:
                for mods in ot["binaries"].values():
                    for m in mods:
                        for f in m["functions"]:
                            users.setdefault((m["module"].lower(), f), set()).add(o)
        gaps = [(b, m["module"], f, s) for b, m, rows in title_rows(title, t) for f, s, _ in rows if s not in COVERED]
        if gaps:
            w(f"<details><summary><b>{title}: imports not covered</b> ({len(gaps)}; status not in implemented / forwarded / hooked / pc / title)</summary>")
            w("")
            w("| Binary | Module | Function | Status | Also imported by |")
            w("|---|---|---|---|---|")
            for b, mod, f, s in sorted(gaps, key=lambda g: (XORDER.index(g[3]), g[1].lower(), g[2].lower())):
                w(f"| {b} | {mod} | `{f}` | {s} | {', '.join(sorted(users.get((mod.lower(), f), ()))) or 'only ' + title} |")
            w("")
            w("</details>")
            w("")
        if len(ti) > 1:
            mine = {lib.lower() for lib, v in libs.items() if title in v["versions"]}
            parts = []
            for o in ti:
                if o == title:
                    continue
                theirs = {lib.lower() for lib, v in libs.items() if o in v["versions"]}
                extra = sum(1 for lib, v in libs.items() if title in v["versions"] and o in v["versions"]
                            for r in v["rows"] if title in r[3] and o not in r[3])
                same = ti[o]["os_build"] == t["os_build"]
                parts.append(f"vs {o} (OS {ti[o]['os_build']}{', same build' if same else ''}): {len(mine - theirs)} DLLs only here, "
                             f"{len(theirs - mine)} DLLs only there, {extra} exports only here in shared DLLs")
            w(f"{title} console OS {t['os_build']}, {len(mine)} export-bearing DLLs; " + "; ".join(parts) + ".")
            w("")
    tnames = list(ti)
    w("## Xbox Series system libraries (EmbeddedXvd, union of all analysed titles)")
    w("")
    w("Every export of every DLL in each title's EmbeddedXvd `Windows\\System32` and `System32\\forwarders` (union, so the newest console OS defines the surface), plus the GDK wrappers the emulator redirects inside the Fortnite EXE (statically linked, not exports; build-specific). `pc` means a PC DLL of the same name exports the same name. The emulator's `mfplat.dll` proxy shadows the PC one for the title, so console mfplat exports it does not forward are counted missing. xgameruntime.dll exports only its dispatcher entry points; the GDK services behind it (XGameSave, XGameUi, XNetworking, ...) are reached via `QueryApiImpl` and are not enumerated here. Import attribution for API-set imports picks one host DLL (KernelBase, ucrtbase, ntdll, combase, ... first).")
    w("")
    w("| Library | Folder | PC DLL | Exports | " + " | ".join(XORDER) + " | " + " | ".join(f"{t} imports" for t in tnames) + " | Versions |")
    w("|---|---|---|---|" + "---|" * len(XORDER) + "---|" * len(tnames) + "---|")
    for lib, v in sorted(libs.items(), key=lambda kv: kv[0].lower()):
        c = {s: 0 for s in XORDER}
        for r in v["rows"]:
            c[r[1]] += 1
        vers = "; ".join(f"{k} {x}" for k, x in v["versions"].items())
        w(f"| {lib} | {v['dir']} | {'yes' if v['pc_dll'] else 'no'} | {len(v['rows'])} | " + " | ".join(str(c[s]) for s in XORDER)
          + " | " + " | ".join(str(sum(1 for r in v["rows"] if t in r[2])) for t in tnames) + f" | {vers} |")
    w("")
    if len(tnames) > 1:
        w("### Exports not present in every title's console OS")
        w("")
        w("Newer console OS builds add exports; each is its own item in the map and stays `missing` (or `pc`) until implemented.")
        w("")
        for lib, v in sorted(libs.items(), key=lambda kv: kv[0].lower()):
            part = [r for r in v["rows"] if v["versions"] and set(r[3]) != set(v["versions"])]
            if not part:
                continue
            only = {}
            for r in part:
                only.setdefault(", ".join(r[3]), []).append(r)
            w(f"<details><summary><b>{escape(lib)}</b>: " + "; ".join(f"{len(rs)} only in {k}" for k, rs in only.items()) + "</summary>")
            w("")
            w("| Function | Present in | Status |")
            w("|---|---|---|")
            for k, rs in only.items():
                for r in sorted(rs, key=lambda r: r[0].lower()):
                    w(f"| `{r[0]}` | {k} | {r[1]} |")
            w("")
            w("</details>")
            w("")
    w("### Xbox-only and emulator-provided libraries, per function")
    w("")
    emu = {k.lower() for k in cfg["emulator"]}
    for lib, v in sorted(libs.items(), key=lambda kv: kv[0].lower()):
        if v["pc_dll"] and lib.lower() not in emu:
            continue
        w(f"<details><summary><b>{escape(lib)}</b>: {len(v['rows'])} exports, {sum(1 for r in v['rows'] if r[1] in COVERED)} covered</summary>")
        w("")
        w("| Function | Status | Imported by | Present in |")
        w("|---|---|---|---|")
        for r in sorted(v["rows"], key=lambda r: (XORDER.index(r[1]), r[0].lower())):
            w(f"| `{r[0]}` | {r[1]} | {', '.join(sorted(r[2]))} | {', '.join(r[3])} |")
        w("")
        w("</details>")
        w("")
    w("## Verified on this machine (Fortnite)")
    w("")
    w("| Milestone | Evidence |")
    w("|---|---|")
    for m in cfg["verified_milestones"]:
        w(f"| {m['name']} | `{m['evidence']}` |")
    for m in cfg["open_milestones"]:
        w(f"| {m} | not reached |")
    w("")
    w("## Unverified")
    w("")
    w("- Fortnite: GameChat2.dll (14 delay imports) and everything it imports: never seen loading. CONCRT140.dll (MSVCP140 delay import): not observed.")
    w("- XAudio2_9 / GameInput ordinal imports: the PC ordinal names are listed; that the console DLL uses the same ordinals was not checked. XAudio2 starts streaming in the Fortnite log, but there is no audio.")
    w("- `pc` status is an export-name match only; behaviour differences between console and PC system DLLs are not measured.")
    w("- Console exports that exist only by ordinal on the PC DLL are `unverified`.")
    w("- Titles without run evidence are classified statically only.")
    w("- Milestones 14/19 and the RDNA2 instruction table are upstream's numbers (`progress.json`, `data/mnemonics.txt`); not re-measured here.")
    w("")
    open(os.path.join(root, "docs", "LIBRARY_MAP.md"), "w", encoding="utf-8").write("\n".join(L))
    return counts


os.makedirs(img, exist_ok=True)
o, y = [], 0
y, a_done, a_n = section(o, y, "Imported system functions*", imports(), 300)
y, b_done, b_n = section(o, y, "Direct3D 12.x interface methods", d3d12x(), 170)
y, c_done, c_n = section(o, y, "RDNA2 shader instructions translated", instructions(), 260)
stages = {k: [v] for k, v in cfg["stages"].items()}
y, d_done, d_n = section(o, y, "Shader stages translated", stages, 60)
libs = console()
y, e_done, e_n, x_done, x_n = console_section(o, y, libs, 900)
svg = (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1000 {y:.0f}" width="1000" height="{y:.0f}">\n'
       f'<rect x="0" y="0" width="1000" height="{y:.0f}" fill="{BG}"/>\n' + "\n".join(o) + "\n</svg>\n")
open(os.path.join(img, "progress.svg"), "w", encoding="utf-8").write(svg)
ti_all = jload("title_imports.json")
examined = sorted(set(cfg["titles_upstream"]) | set(ti_all))
m, s = cfg["milestones"], cfg["shaders"]
badge("imports", "system functions*", f"{100 * a_done / a_n:.2f}%", pct_colour(100 * a_done / a_n))
badge("d3d12x", "Direct3D 12.x methods", f"{100 * b_done / b_n:.2f}%", pct_colour(100 * b_done / b_n))
badge("instructions", "RDNA2 instructions", f"{100 * c_done / c_n:.2f}%", pct_colour(100 * c_done / c_n))
badge("shaders", "shaders translated", f'{s["translated"]:,}/{s["total"]:,}', "#4c1")
badge("stages", "shader stages", f"{d_done}/{d_n}", pct_colour(100 * d_done / d_n))
badge("titles", "titles examined", str(len(examined)), "#007ec6")
badge("milestones", "milestones", f'{m["done"]}/{m["total"]}', pct_colour(100 * m["done"] / m["total"]))
badge("console", "Xbox system exports", f"{100 * e_done / e_n:.2f}%", pct_colour(100 * e_done / e_n))
badge("xbox-only", "Xbox-only exports", f"{100 * x_done / x_n:.2f}%", pct_colour(100 * x_done / x_n))
vm = len(cfg["verified_milestones"])
summary = {
    "updated": cfg["updated"],
    "headline": {
        "system_functions": metric(a_done, a_n),
        "d3d12x_methods": metric(b_done, b_n),
        "rdna2_instructions": metric(c_done, c_n),
        "shaders_translated": metric(s["translated"], s["total"]),
        "shader_stages": metric(d_done, d_n),
        "milestones": metric(m["done"], m["total"]),
        "titles_examined": {"done": len(examined), "total": None, "pct": None, "titles": examined},
    },
    "extra": {
        "console_exports": metric(e_done, e_n),
        "xbox_only_exports": metric(x_done, x_n),
        "shader_compile_lobby_frame": metric(cfg["shader_compile"]["lobby_frame"]["done"], cfg["shader_compile"]["lobby_frame"]["total"]),
        "shader_compile_fixtures": metric(cfg["shader_compile"]["fixtures"]["done"], cfg["shader_compile"]["fixtures"]["total"]),
        "shader_compile_full_sweep": {"done": None, "total": cfg["shader_compile"]["full_sweep"]["total"], "pct": None},
        "milestones_verified_this_machine": metric(vm, vm + len(cfg["open_milestones"])),
    },
}
for title, t in ti_all.items():
    rows = [r for _, _, rs in title_rows(title, t) for r in rs]
    summary["extra"][f"title_imports_{title}"] = metric(sum(1 for r in rows if r[1] in COVERED), len(rows))
tcounts = library_map(summary, libs)
summary["title_import_status"] = tcounts
summary["console_os_builds"] = {k: v["os_build"] for k, v in ti_all.items()}
json.dump(summary, open(os.path.join(here, "summary.json"), "w", encoding="utf-8"), indent=1)
print(f"imports {a_done}/{a_n}  d3d12x {b_done}/{b_n}  instructions {c_done}/{c_n}  stages {d_done}/{d_n}  "
      f"console {e_done}/{e_n}  xbox-only {x_done}/{x_n}  titles {examined}  height {y:.0f}  bytes {len(svg)}")
print(json.dumps({k: v for k, v in summary["extra"].items() if k.startswith("title_imports")}))
