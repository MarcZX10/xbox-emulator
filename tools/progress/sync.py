import argparse, json, os, re, shutil

here = os.path.dirname(os.path.abspath(__file__))
data = os.path.join(here, "data")
cfg = json.load(open(os.path.join(here, "progress.json"), encoding="utf-8"))

ap = argparse.ArgumentParser(description="Refresh tools/progress/data from the TomBox tree, the title dumps (read-only) and this PC's System32. Needs pefile.")
ap.add_argument("--tombox", default=r"D:\TomBox")
ap.add_argument("--pc", default=os.path.join(os.environ.get("SystemRoot", r"C:\Windows"), "System32"))
a = ap.parse_args()

import pefile


def load(path, dirs):
    pe = pefile.PE(path, fast_load=True)
    pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY[d] for d in dirs])
    return pe


def imports(path):
    pe = load(path, ["IMAGE_DIRECTORY_ENTRY_IMPORT", "IMAGE_DIRECTORY_ENTRY_DELAY_IMPORT"])
    mods = []
    for kind, attr in (("import", "DIRECTORY_ENTRY_IMPORT"), ("delay", "DIRECTORY_ENTRY_DELAY_IMPORT")):
        for e in getattr(pe, attr, []):
            mods.append({"module": e.dll.decode(), "kind": kind,
                         "functions": [i.name.decode() if i.name else f"#{i.ordinal}" for i in e.imports]})
    pe.close()
    return mods


def exports(path):
    pe = load(path, ["IMAGE_DIRECTORY_ENTRY_EXPORT"])
    out = []
    if hasattr(pe, "DIRECTORY_ENTRY_EXPORT"):
        for s in pe.DIRECTORY_ENTRY_EXPORT.symbols:
            out.append((s.name.decode(errors="replace") if s.name else None, s.ordinal,
                        s.forwarder.decode(errors="replace") if s.forwarder else None))
    pe.close()
    return out


def version(path):
    try:
        pe = load(path, ["IMAGE_DIRECTORY_ENTRY_RESOURCE"])
        v = pe.VS_FIXEDFILEINFO[0]
        pe.close()
        return "%d.%d.%d.%d" % (v.FileVersionMS >> 16, v.FileVersionMS & 0xFFFF, v.FileVersionLS >> 16, v.FileVersionLS & 0xFFFF)
    except Exception:
        return None


pc_cache = {}


def pc_exports(name):
    key = name.lower()
    if key not in pc_cache:
        p = os.path.join(a.pc, name)
        pc_cache[key] = None
        if os.path.isfile(p):
            try:
                ex = exports(p)
                pc_cache[key] = ({e[0] for e in ex if e[0]}, {f"#{e[1]}": e[0] for e in ex})
            except Exception:
                pass
    return pc_cache[key]


API_HOSTS = ["kernelbase.dll", "kernel32.dll", "ntdll.dll", "ucrtbase.dll", "combase.dll", "user32.dll", "advapi32.dll",
             "sechost.dll", "ole32.dll", "oleaut32.dll", "msvcp_win.dll", "shcore.dll", "rpcrt4.dll", "bcrypt.dll",
             "twinapi.appcore.dll", "wer.dll", "powrprof.dll", "cfgmgr32.dll", "gdi32.dll", "shlwapi.dll", "crypt32.dll"]
api_union = None


def api_set(module):
    global api_union
    p = os.path.join(a.pc, "downlevel", module)
    if os.path.isfile(p):
        ex = exports(p)
        return ({e[0] for e in ex if e[0]}, {f"#{e[1]}": e[0] for e in ex})
    if api_union is None:
        names = set()
        for h in API_HOSTS:
            pc = pc_exports(h)
            if pc:
                names |= pc[0]
        api_union = (names, {})
    return api_union


def pc_has(module, f):
    pc = api_set(module) if module.lower().startswith(("api-ms-", "ext-ms-")) else pc_exports(module)
    if pc is None:
        return 0
    if f.startswith("#"):
        return 2 if f in pc[1] else 0
    return 1 if f in pc[0] else 0


os.makedirs(data, exist_ok=True)
shutil.copyfile(os.path.join(a.tombox, "analysis", "fortnite_exe.json"), os.path.join(data, "fortnite_exe.json"))
shutil.copyfile(os.path.join(a.tombox, "tools", "progress", "mnemonics.txt"), os.path.join(data, "mnemonics.txt"))

src = open(os.path.join(a.tombox, "crates", "tombox-xshader", "src", "rdna2", "mnem.rs"), encoding="utf-8").read()
groups, cur, seen = [], None, set()
for line in src.split("\n"):
    m = re.match(r"pub fn (\w+)\(", line)
    if m:
        cur = {"lookup": "VINTRP"}.get(m.group(1), m.group(1).upper())
    e = re.search(r"Enc::(\w+) =>", line)
    if e:
        cur = {"Vinterp": "VINTRP"}.get(e.group(1), e.group(1).upper())
    if line.lstrip().startswith("//"):
        continue
    for n in re.findall(r'"([a-z][a-z0-9_]*)"', line):
        if n in seen:
            continue
        seen.add(n)
        if not groups or groups[-1][0] != cur:
            groups.append([cur, []])
        groups[-1][1].append(n)
json.dump(groups, open(os.path.join(data, "mnem_groups.json"), "w", encoding="utf-8"), indent=0)

TWINS = {"minuserx.dll": ["user32.dll"], "kernel32legacy.dll": ["kernel32.dll", "kernelbase.dll"],
         "advapi32legacy.dll": ["advapi32.dll"], "minappmodelx.dll": ["twinapi.appcore.dll", "kernel.appcore.dll"]}


def twin_exports(key):
    names, found = set(), []
    for t in TWINS.get(key, ()):
        pc = pc_exports(t)
        if pc:
            names |= pc[0]
            found.append(t)
    return names, found


titles, console, skipped = {}, {}, []
for t in cfg["titles_analysed"]:
    bdir = os.path.join(a.tombox, *t["binaries"].split("/"))
    exe = os.path.join(bdir, t["exe"])
    if not os.path.isfile(exe) or ("exe_size" in t and os.path.getsize(exe) != t["exe_size"]):
        skipped.append(t["name"])
        continue
    bins = {}
    for sub in t.get("binary_dirs", [""]):
        d = os.path.join(bdir, *filter(None, sub.split("/")))
        if not os.path.isdir(d):
            continue
        for name in sorted(os.listdir(d), key=str.lower):
            p = os.path.join(d, name)
            if os.path.isfile(p) and name.lower().endswith((".exe", ".dll")):
                mods = imports(p)
                for m in mods:
                    m["pc"] = [pc_has(m["module"], f) for f in m["functions"]]
                bins[f"{sub}/{name}" if sub else name] = mods
    xvd = os.path.join(a.tombox, *t["xvd"].split("/"))
    os_build = version(os.path.join(xvd, "System32", "ntdll.dll"))
    titles[t["name"]] = {"exe": t["exe"], "binaries": bins, "os_build": os_build}
    for sub in ("System32", "System32/forwarders"):
        d = os.path.join(xvd, *sub.split("/"))
        if not os.path.isdir(d):
            continue
        for name in sorted(os.listdir(d), key=str.lower):
            p = os.path.join(d, name)
            if not os.path.isfile(p) or not name.lower().endswith(".dll"):
                continue
            ex = exports(p)
            if not ex:
                continue
            key = name.lower()
            pc = pc_exports(name)
            lib = console.setdefault(key, {"name": name, "dir": sub, "pc_dll": pc is not None, "exports": {}, "versions": {}})
            lib["versions"][t["name"]] = version(p)
            for n, o, fwd in ex:
                k = n or f"#{o}"
                row = lib["exports"].setdefault(k, [0 if pc is None else (1 if n in pc[0] else 0) if n else (2 if k in pc[1] else 0), fwd, []])
                row[2].append(t["name"])

out = {}
for key, lib in console.items():
    out[lib["name"]] = {"dir": lib["dir"], "pc_dll": lib["pc_dll"], "versions": lib["versions"],
                        "exports": [[k, v[0], v[1], v[2]] for k, v in lib["exports"].items()]}
    if not lib["pc_dll"] and key in TWINS:
        tn, found = twin_exports(key)
        out[lib["name"]]["pc_twin"] = found
        out[lib["name"]]["twin_has"] = sorted(k for k in lib["exports"] if k in tn)
json.dump(out, open(os.path.join(data, "console_exports.json"), "w", encoding="utf-8"), separators=(",", ":"))
json.dump(titles, open(os.path.join(data, "title_imports.json"), "w", encoding="utf-8"), indent=1)

emu, cdir = {}, os.path.join(a.tombox, "crates", "compat")
for crate in sorted(os.listdir(cdir), key=str.lower):
    table = os.path.join(cdir, crate, "exports.txt")
    if not os.path.isfile(table):
        continue
    lib = re.search(r'\[lib\][^\[]*?name\s*=\s*"([^"]+)"', open(os.path.join(cdir, crate, "Cargo.toml"), encoding="utf-8").read())
    dll = (lib.group(1) if lib else crate) + ".dll"
    dll = next((n for n in out if n.lower() == dll.lower()), dll)
    funcs, notes = {}, {}
    for line in open(table, encoding="utf-8"):
        body, _, note = line.strip().partition(" | ")
        f = body.split()
        if len(f) < 3:
            continue
        funcs[f[1]] = f[2]
        if note:
            notes[f[1]] = note.strip()
    emu[dll] = {"file": f"crates/compat/{crate}/src/lib.rs", "functions": funcs, "notes": notes}
json.dump(emu, open(os.path.join(data, "emulator_exports.json"), "w", encoding="utf-8"), indent=1)

EVIDENCE = ["chatprobe.json", "media.json", "gameinput_ext.json", "oshost.json"]
edir, odir = os.path.join(a.tombox, "analysis", "progress_evidence"), os.path.join(data, "evidence")
os.makedirs(odir, exist_ok=True)
copied = []
for name in EVIDENCE:
    p = os.path.join(edir, name)
    if os.path.isfile(p):
        json.load(open(p, encoding="utf-8"))
        shutil.copyfile(p, os.path.join(odir, name))
        copied.append(name)
    elif os.path.isfile(os.path.join(odir, name)):
        os.remove(os.path.join(odir, name))

ords = {}
for dll in ("XAudio2_9.dll", "GameInput.dll"):
    p = os.path.join(a.pc, dll)
    if os.path.isfile(p):
        ords[dll] = {f"#{o}": n for n, o, f in exports(p) if n}
json.dump(ords, open(os.path.join(data, "pc_ordinals.json"), "w", encoding="utf-8"), indent=1)

print("titles", list(titles), "skipped", skipped, "console libraries", len(out),
      "console exports", sum(len(v["exports"]) for v in out.values()),
      "os builds", {k: v["os_build"] for k, v in titles.items()},
      "emulator export tables", {k: len(v["functions"]) for k, v in emu.items()},
      "twins", {k: (v.get("pc_twin"), len(v.get("twin_has", []))) for k, v in out.items() if "pc_twin" in v},
      "evidence", copied)
