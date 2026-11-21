import argparse
import os
import sys

BOM = b"\xef\xbb\xbf"
IDENT = set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_")


def is_ident_char(c):
    return c in IDENT or (ord(c) > 127 and c.isidentifier())


def comment_mask(text):
    n = len(text)
    mask = bytearray(n)
    i = 0
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            j = text.find("\n", i)
            j = n if j < 0 else j
            if j > i and text[j - 1] == "\r":
                j -= 1
            for k in range(i, j):
                mask[k] = 1
            i = j
            continue
        if c == "/" and i + 1 < n and text[i + 1] == "*":
            depth = 1
            j = i + 2
            while j < n and depth:
                if text.startswith("/*", j):
                    depth += 1
                    j += 2
                elif text.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            if depth:
                raise ValueError("unterminated block comment at offset %d" % i)
            for k in range(i, j):
                mask[k] = 1
            i = j
            continue
        prev_ident = i > 0 and is_ident_char(text[i - 1])
        if not prev_ident and c in "rbc":
            j = i + 1
            if c in "bc" and j < n and text[j] == "r":
                j += 1
            raw = j > i + 1 or c == "r"
            if raw:
                h = j
                while h < n and text[h] == "#":
                    h += 1
                if h < n and text[h] == '"':
                    close = '"' + "#" * (h - j)
                    e = text.find(close, h + 1)
                    if e < 0:
                        raise ValueError("unterminated raw string at offset %d" % i)
                    i = e + len(close)
                    continue
            elif j < n and text[j] in "\"'":
                i = j
                c = text[i]
                prev_ident = False
            else:
                i += 1
                while i < n and is_ident_char(text[i]):
                    i += 1
                continue
        if c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            if j >= n:
                raise ValueError("unterminated string at offset %d" % i)
            i = j + 1
            continue
        if c == "'":
            if i + 1 < n and text[i + 1] == "\\":
                j = i + 1
                while j < n and text[j] != "'":
                    j += 2 if text[j] == "\\" else 1
                i = j + 1
                continue
            if i + 2 < n and text[i + 2] == "'" and text[i + 1] not in "\r\n":
                i += 3
                continue
            i += 1
            continue
        if is_ident_char(c):
            i += 1
            while i < n and is_ident_char(text[i]):
                i += 1
            continue
        i += 1
    return mask


def strip_text(text):
    mask = comment_mask(text)
    out = []
    removed = 0
    pos = 0
    n = len(text)
    while pos < n:
        e = text.find("\n", pos)
        end = n if e < 0 else e + 1
        line = text[pos:end]
        body_end = end - (1 if line.endswith("\n") else 0)
        if body_end > pos and text[body_end - 1] == "\r":
            body_end -= 1
        has_comment = False
        has_code = False
        for k in range(pos, body_end):
            if mask[k]:
                if not text[k].isspace():
                    has_comment = True
            elif not text[k].isspace():
                has_code = True
                break
        if has_comment and not has_code:
            removed += 1
        else:
            out.append(line)
        pos = end
    return "".join(out), removed


def strip_bytes(data):
    bom = data.startswith(BOM)
    body = data[len(BOM):] if bom else data
    text = body.decode("utf-8")
    new, removed = strip_text(text)
    return (BOM if bom else b"") + new.encode("utf-8"), removed


def iter_rs(root):
    for dp, dn, fn in os.walk(root):
        dn[:] = [d for d in dn if d not in ("target", ".git") and not d.startswith("target-")]
        for f in sorted(fn):
            if f.endswith(".rs"):
                yield os.path.join(dp, f)


def main(argv=None):
    ap = argparse.ArgumentParser(description="Remove comment-only lines from Rust sources.")
    ap.add_argument("paths", nargs="*", default=[os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "crates")])
    g = ap.add_mutually_exclusive_group()
    g.add_argument("--dry-run", action="store_true", default=True)
    g.add_argument("--apply", action="store_true")
    a = ap.parse_args(argv)
    files = []
    for p in a.paths:
        files.extend([p] if os.path.isfile(p) else iter_rs(p))
    total = changed = errors = 0
    for f in files:
        with open(f, "rb") as h:
            data = h.read()
        try:
            new, removed = strip_bytes(data)
        except (ValueError, UnicodeDecodeError) as e:
            errors += 1
            print("SKIP %s: %s" % (f, e))
            continue
        if not removed:
            continue
        changed += 1
        total += removed
        print("%6d  %s" % (removed, os.path.normpath(f)))
        if a.apply:
            with open(f, "wb") as h:
                h.write(new)
    mode = "removed" if a.apply else "would remove"
    print("%s %d comment-only lines in %d of %d files (%d skipped)" % (mode, total, changed, len(files), errors))
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
