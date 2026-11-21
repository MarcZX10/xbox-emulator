import glob
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = r"D:\xbox-pc-port\tools\progress"


def norm(s):
    return s.replace("\\", "/").lower()


def evidence_rows(title):
    rows = {}
    for f in sorted(glob.glob(os.path.join(HERE, "evidence", "*_load.json"))):
        d = json.load(open(f, encoding="utf-8"))
        if d.get("title") != title or not d.get("result"):
            continue
        for r in d["result"].get("rows", []):
            key = (norm(r["binary"]), norm(r["module"]), r["function"], r["kind"])
            rows.setdefault(key, []).append((os.path.basename(f), r))
    return rows


def main():
    progress = json.load(open(os.path.join(DATA, "progress.json"), encoding="utf-8"))
    imports = json.load(open(os.path.join(DATA, "data", "title_imports.json"), encoding="utf-8"))
    report = {}
    ok_all = True
    for t in progress["titles_analysed"]:
        title = t.get("title") or t.get("name")
        ub = {norm(b) for b in t.get("unverified_binaries", [])}
        um = {norm(m) for m in t.get("unverified_modules", [])}
        if not ub and not um:
            continue
        ev = evidence_rows(title)
        want = []
        for binary, entries in imports[title]["binaries"].items():
            for e in entries:
                if norm(binary) in ub or norm(e["module"]) in um:
                    for fn in e["functions"]:
                        want.append((norm(binary), norm(e["module"]), fn, e["kind"]))
        missing = [w for w in want if w not in ev]
        unresolved = [w for w in want if w in ev and not any(r["resolved"] for _, r in ev[w])]
        ok = not missing and not unresolved
        ok_all &= ok
        report[title] = {
            "unverified_binaries": sorted(ub),
            "unverified_modules": sorted(um),
            "rows": len(want),
            "covered": len(want) - len(missing),
            "resolved": len(want) - len(missing) - len(unresolved),
            "missing": [list(w) for w in missing],
            "unresolved": [list(w) for w in unresolved],
            "ok": ok,
        }
    out = os.path.join(HERE, "evidence", "coverage.json")
    json.dump(report, open(out, "w", encoding="utf-8"), indent=2)
    for t, r in report.items():
        print(f"{t}: {r['resolved']}/{r['rows']} resolved, {len(r['missing'])} missing, ok={r['ok']}")
    return 0 if ok_all else 1


if __name__ == "__main__":
    sys.exit(main())
