# xbox-pc-port

Running Xbox Series X|S builds on PC via [drycheetah84/TomBox](https://github.com/drycheetah84/TomBox).

Uses legally obtained, already-decrypted dumps. The runtime does no key extraction or package decryption.

## Status

![milestones](https://img.shields.io/badge/milestones-4%2F9-orange)
![compat DLLs built](https://img.shields.io/badge/compat_DLLs-6%2F6-brightgreen)
![Fortnite](https://img.shields.io/badge/Fortnite-staged-blue)
![last update](https://img.shields.io/badge/updated-2026--11--21-lightgrey)

### Progress map

<sub>🟩 done · 🟨 partial · ⬜ not started</sub>

| | | |
|---|---|---|
| 🟩 TomBox cloned | 🟩 Baseline build | 🟩 Static-CRT compat DLLs |
| 🟩 `mdbg.exe` tracer | 🟩 Launch wrapper | ⬜ Dump extracted |
| ⬜ First boot verified | ⬜ 3D scene correct | 🟨 Input (mouse only) |
| ⬜ Keyboard input | ⬜ Controller | ⬜ Audio |
| ⬜ Gameplay | | |

### Milestones

| # | Milestone | State |
|---|---|---|
| 1 | TomBox toolchain built | done |
| 2 | Fortnite dump extracted | pending download |
| 3 | First boot under `mdbg` | not started |
| 4 | Reaches lobby | upstream reports yes, not reproduced locally |
| 5 | Keyboard input | not started |
| 6 | Controller input | not started |
| 7 | 3D scene correct | not started |
| 8 | Audio | not started |
| 9 | Playable gameplay | not started |

## Tested titles

| Title | Build | Result |
|---|---|---|
| Fortnite XSX | 2.0.1461.7919 | runtime staged, awaiting decrypted dump |

## Documents

- [STATUS.md](STATUS.md) — current snapshot
- [docs/DEVELOPMENT_LOG.md](docs/DEVELOPMENT_LOG.md) — chronological log
- [docs/BLOCKERS.md](docs/BLOCKERS.md) — standing walls
