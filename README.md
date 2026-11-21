# Xbox Emulator

**Running the Xbox Series X|S library on PC. A continuation of [drycheetah84/TomBox](https://github.com/drycheetah84/TomBox).**

The runtime reimplements the console's platform layer (Xbox-flavoured Direct3D 12, precompiled RDNA2 shaders, unified-memory textures, system libraries) so dumped, already-decrypted Xbox Series X|S builds run as normal Windows processes. It is not tied to any single title. This repo tracks our continuation — new titles brought up, platform calls extended, shader stages unlocked.

Uses legally obtained, already-decrypted dumps. The runtime does no key extraction or package decryption.

## Status

[![system functions](https://raw.githubusercontent.com/drycheetah84/TomBox/master/docs/img/badge-imports.svg)](https://raw.githubusercontent.com/drycheetah84/TomBox/master/docs/img/progress.svg)
[![Direct3D 12.x methods](https://raw.githubusercontent.com/drycheetah84/TomBox/master/docs/img/badge-d3d12x.svg)](https://raw.githubusercontent.com/drycheetah84/TomBox/master/docs/img/progress.svg)
[![RDNA2 instructions](https://raw.githubusercontent.com/drycheetah84/TomBox/master/docs/img/badge-instructions.svg)](https://raw.githubusercontent.com/drycheetah84/TomBox/master/docs/img/progress.svg)
[![shaders translated](https://raw.githubusercontent.com/drycheetah84/TomBox/master/docs/img/badge-shaders.svg)](#tested-titles)
[![milestones](https://raw.githubusercontent.com/drycheetah84/TomBox/master/docs/img/badge-milestones.svg)](#milestones)
[![titles examined](https://raw.githubusercontent.com/drycheetah84/TomBox/master/docs/img/badge-titles.svg)](#tested-titles)

[![progress map](https://raw.githubusercontent.com/drycheetah84/TomBox/master/docs/img/progress.svg)](https://raw.githubusercontent.com/drycheetah84/TomBox/master/docs/img/progress.svg)

<sub>One square per item: green = done, amber = partial (a stub, a forwarder, or not verified to work), grey = not done. Four sections — imported system functions the test title uses, Direct3D 12.x interface methods, RDNA2 shader instructions the decoder knows, and the eight shader stages. Mesh and amplification are the only stages still grey, blocking any title that uses them. Map is served live from the upstream TomBox repo.</sub>

## Milestones

| Milestone | State |
|---|---|
| Console executable loads and runs natively | done |
| Graphics device, pipelines and presentation | done |
| First frame from the real game | done |
| Main menu / lobby reached | done |
| 3D scene correct | in progress |
| Input | partial (mouse) |
| Audio | not started |
| Gameplay | not started |
| A second title | shaders examined, not run |

## Tested titles

| Title | Build | Result |
|---|---|---|
| Fortnite (Chapter 2 Season 4) | 2.0.1461.7919 | Boots, logs in to a private server, reaches the lobby. 2D interface correct. 3D scene lit but the character material is wrong and bloom shows artifacts. No audio. Not playable. |
| Grand Theft Auto VI | 1.0.2.1 | **Not run** (dump has no executable). Shader package examined: 26,503 shaders, plain compressed Xbox shader containers. Of a 4,000-shader sample all disassemble cleanly; ~⅓ of vertex shaders translate. Blockers: mesh shaders, shaders without root signatures. |

## Our local state

- [STATUS.md](STATUS.md) — what's built, downloaded, staged on this machine
- [docs/DEVELOPMENT_LOG.md](docs/DEVELOPMENT_LOG.md) — chronological log
- [docs/BLOCKERS.md](docs/BLOCKERS.md) — standing walls

## Credit

Upstream work by [drycheetah84](https://github.com/drycheetah84/TomBox). This repo tracks our continuation — new titles brought up, platform functions extended, shader stages unlocked. The progress map and per-section badges above are served directly from upstream and reflect the actual state of the runtime as upstream publishes it.
