import os, re, shutil, sys

SRC = r"D:\xbox-pc-port\emulator"
DST = r"D:\xbox-pc-port"
MANAGED = ["crates", os.path.join("tools", "dev"), os.path.join("tools", "qa"), os.path.join("tools", "verify"),
           os.path.join("tools", "snap_window.ps1")]

TOP_FILES = {"Cargo.toml", "Cargo.lock", "stage_compat.ps1", "run_fortnite.ps1", "run_xdefiant.ps1", "run_xone.ps1"}
TOP_DIRS = {"crates", "tools"}
SKIP_DIRS = {"target", "batch_runs", "evidence", "node_modules", ".git", "__pycache__", "out", "runs", "backend_data"}
SKIP_EXT = {".md", ".log", ".exe", ".dll", ".pdb", ".lib", ".obj", ".o", ".a", ".bin", ".dxil", ".cso", ".zip",
            ".7z", ".rar", ".xvd", ".xvc", ".msixvc", ".png", ".jpg", ".jpeg", ".gif", ".mp4", ".webm", ".wav",
            ".xma", ".etl", ".dmp", ".pak", ".ucas", ".utoc", ".sys", ".so", ".dylib", ".rlib", ".pyc", ".ilk", ".exp"}
MAX_BYTES = 2 * 1024 * 1024
BAD = re.compile(r"(?i)brady|desktop-rlep7qm|bradymessinioz|jb workspace")


def wanted(rel):
    parts = rel.replace("\\", "/").split("/")
    if len(parts) == 1:
        return parts[0] in TOP_FILES
    if parts[0] not in TOP_DIRS:
        return False
    if parts[0] == "tools" and len(parts) > 2 and parts[1] == "progress":
        return False
    for p in parts[:-1]:
        if p in SKIP_DIRS or p.startswith("target"):
            return False
    name = parts[-1]
    ext = os.path.splitext(name)[1].lower()
    if ext in SKIP_EXT or name.lower().endswith((".txt.bak", ".orig")):
        return False
    return True


def main():
    for m in MANAGED + sorted(TOP_FILES):
        p = os.path.join(DST, m)
        if os.path.isdir(p):
            shutil.rmtree(p)
        elif os.path.isfile(p):
            os.remove(p)
    copied, blocked = 0, []
    for root, dirs, files in os.walk(SRC):
        rel_root = os.path.relpath(root, SRC)
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS and not d.startswith(("target", "run"))]
        for f in files:
            rel = f if rel_root == "." else os.path.join(rel_root, f)
            if not wanted(rel):
                continue
            path = os.path.join(root, f)
            if os.path.getsize(path) > MAX_BYTES:
                continue
            with open(path, "rb") as h:
                data = h.read()
            if b"\0" in data[:8192]:
                continue
            text = data.decode("utf-8", "replace")
            if BAD.search(text):
                blocked.append(rel)
                continue
            out = os.path.join(DST, rel)
            os.makedirs(os.path.dirname(out), exist_ok=True)
            with open(out, "wb") as h:
                h.write(data)
            copied += 1
    print("copied", copied)
    for b in blocked:
        print("blocked (personal info):", b)


if __name__ == "__main__":
    main()
