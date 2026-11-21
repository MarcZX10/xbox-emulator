# Development log

Reverse chronological. Each entry: what was done, evidence, blockers, next action.

---

## 2026-11-21 — TomBox staged, Fortnite dump downloading

**Done**
- Cloned `drycheetah84/TomBox` → `D:\TomBox`.
- Baseline `cargo build --release` passed in 29 s.
- Built the six static-CRT compat DLLs (`d3d12_xs`, `xmem`, `xcrdapi`, `acphal`,
  `dxcompiler_xs`, `mfplat`) with `RUSTFLAGS=-C target-feature=+crt-static`.
  Sizes: d3d12_xs 1.85 MB, acphal 705 KB, xcrdapi 724 KB, mfplat 698 KB,
  dxcompiler_xs 689 KB, xmem 702 KB.
- Built `mdbg.exe` tracer (229 KB) from `tools/dev/mdbg.rs`.
- Wrote `D:\TomBox\run_fortnite.ps1` — extract + stage DLLs + `subst G:` + run
  under `mdbg.exe` with log summary.
- Discovered first attempt at the Fortnite dump was a 27 GB `.xvc` (encrypted
  retail container). Verified — first 64 bytes are high-entropy with no magic,
  consistent with AES ciphertext. Deleted. Downloading the archive.org
  decrypted ZIP (27.4 GB) instead. URL verified live (HTTP 200).

**Evidence**
- `.xvc` bytes 0..64 are random; a valid ZIP would start with `50 4b 03 04`.
- Microsoft Gaming Services not installed on this machine. Required for
  `xgameruntime.dll`.

**Blockers**
- Dump download in progress.
- Gaming Services install needed before first run.

**Next**
1. `run_fortnite.ps1 -Extract` once ZIP arrives.
2. `run_fortnite.ps1 -Run` — capture baseline log.
3. Decide on first contribution: keyboard input is the smallest high-value PR
   (upstream README lists input as "partial (mouse)").
