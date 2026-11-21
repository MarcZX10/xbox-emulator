import json, os, sys

here = os.path.dirname(os.path.abspath(__file__))
root = os.path.normpath(os.path.join(here, "..", ".."))
data = os.path.normpath(os.path.join(root, "..", "tools", "progress", "data"))
out = os.path.join(root, "crates", "compat", "compat-common", "data", "console_apiset_hosts.txt")
if len(sys.argv) > 1:
    data = sys.argv[1]

HOST_PRIORITY = ["kernelbase.dll", "ucrtbase.dll", "ntdll.dll", "combase.dll", "msvcp_win.dll", "rpcrt4.dll",
                 "sechost.dll", "minuserx.dll", "oleaut32.dll"]

con = json.load(open(os.path.join(data, "console_exports.json"), encoding="utf-8"))
ti = json.load(open(os.path.join(data, "title_imports.json"), encoding="utf-8"))
order = sorted(con, key=lambda l: (HOST_PRIORITY.index(l.lower()) if l.lower() in HOST_PRIORITY else 99, l.lower()))


def build_of(t):
    b = (ti[t].get("os_build") or "").split(".")
    return int(b[2]) if len(b) > 2 and b[2].isdigit() else 0


rows = set()
for title, t in ti.items():
    build = build_of(title)
    wanted = {}
    for mods in t["binaries"].values():
        for m in mods:
            low = m["module"].lower()
            if low.startswith(("api-ms-", "ext-ms-")):
                for f in m["functions"]:
                    wanted.setdefault(f, set()).add(low)
    host = {}
    for lib in order:
        for n, has, fwd, pres in con[lib]["exports"]:
            if fwd is None and n in wanted and n not in host and title in pres:
                host[n] = lib.lower()
    for f, sets in wanted.items():
        if f in host:
            for s in sets:
                rows.add((build, s, f, host[f]))

os.makedirs(os.path.dirname(out), exist_ok=True)
with open(out, "w", encoding="utf-8", newline="\n") as fh:
    for r in sorted(rows):
        fh.write(f"{r[0]} {r[1]} {r[2]} {r[3]}\n")
print(len(rows), "rows ->", out)
