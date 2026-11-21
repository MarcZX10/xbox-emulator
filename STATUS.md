# Status — 2026-11-21

## TomBox (Fortnite XSX 2.0.1461.7919)

| Step | State |
|---|---|
| Repo cloned to `D:\TomBox` | done |
| Baseline `cargo build --release` | passes (29 s warm) |
| Static-CRT compat DLLs (`d3d12_xs`, `xmem`, `xcrdapi`, `acphal`, `dxcompiler_xs`, `mfplat`) | built |
| `mdbg.exe` tracer | built |
| Launch wrapper `D:\TomBox\run_fortnite.ps1` | written |
| Dump — decrypted archive.org ZIP (27.4 GB) | downloading |
| Extract + mount `G:` + run | pending zip |
| Microsoft Gaming Services (`xgameruntime.dll`) | not verified installed |

Upstream reports Fortnite boots, logs in, reaches lobby. Not playable: 3D
character material wrong, bloom artifacts, no audio, mouse-only input, no
controller.

## rage9 / GTA VI

| Step | State |
|---|---|
| Dump inventoried (`D:\GTA 6`, ~213 GB CyberLeek v1.0.2.1) | done |
| RPF9 container parsed | done (TOC AES-ECB, single global key) |
| Decryption key recovery | **blocked** — key is in withheld `game_scarlett_master.exe` |
| XVD / `gameos.xvd` | uses `StandardOdk` (hardware-fused via `clipsp.sys`) |
| Audio (AWC) extractor | done — 188 radio tracks, codec is length-prefixed Opus |
| Launcher UI + self-update + R2 distribution | done |
| Online harness (Steam Spacewar / appid 480) | proven |
| Native installer app (wry/tao WebView2) | built |

Every decryption lead exhausted: raw key scans, PBKDF2 derivations, cross-layer
with recovered CIKs, metadata, structural ECB analysis, driver hashes, oracle
validation against CodeWalker's reference code. Door stays shut without the exe.

## Next

1. Fortnite ZIP download finishes.
2. `run_fortnite.ps1 -Extract` then `-Run`.
3. Capture a baseline log of where it currently reaches, as the reference for any
   change we make.
4. First real contribution candidate: keyboard input (README lists input as
   "partial (mouse)").
