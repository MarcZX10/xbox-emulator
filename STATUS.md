# Status — 2026-11-21

Target: Fortnite XSX `2.0.1461.7919` (the build TomBox is pinned to).

## Toolchain

| Piece | State |
|---|---|
| Repo cloned to `D:\TomBox` | done |
| Baseline `cargo build --release` | passes (29 s warm) |
| Static-CRT compat DLLs (`d3d12_xs`, `xmem`, `xcrdapi`, `acphal`, `dxcompiler_xs`, `mfplat`) | built, 6/6 |
| `mdbg.exe` tracer | built (229 KB) |
| Launch wrapper `D:\TomBox\run_fortnite.ps1` | written |

## Dump

| Piece | State |
|---|---|
| Decrypted archive.org ZIP (27.4 GB) | downloading |
| Extract to `input\extracted\Mount` | pending |
| Real `xg_xs.dll` staged into game dir | pending |
| Our six compat DLLs staged into game dir | pending |
| `subst G: input\extracted\Mount` | pending |

## Host environment

| Piece | State |
|---|---|
| Rust 1.100 (MSVC) | installed |
| Windows SDK + DXC | present |
| Microsoft Gaming Services (`xgameruntime.dll`) | **not installed** — needed |

## Upstream state (reported, not reproduced locally)

Fortnite boots, logs in to a private server, reaches the lobby. Not playable:
3D character material wrong, bloom artifacts along the top of the screen, no
audio, mouse-only input, no controller, no gameplay.

## Next

1. Install Microsoft Gaming Services.
2. `run_fortnite.ps1 -Extract` when ZIP finishes.
3. `run_fortnite.ps1 -Run` — capture baseline log as the reference for every
   change going forward.
4. First real contribution candidate: keyboard input.
