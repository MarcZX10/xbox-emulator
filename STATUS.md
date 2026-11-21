# Status — 2026-11-21

Current title: Fortnite XSX `2.0.1461.7919`.

## Local runtime

| Piece | State |
|---|---|
| TomBox built (`D:\TomBox`, master `7e8064f`) | done |
| Static-CRT compat DLLs (6) | done |
| `mdbg.exe` tracer | done |
| Dump extracted (`input\extracted`, pristine, never modified) | done |
| Run overlay `D:\TomBox\run\Mount` (junctions + hardlinks to the dump, TomBox DLLs only in the overlay) mounted as `G:` | done |
| PC DirectStorage 1.3.0 (`dstorage.dll`, `dstoragecore.dll`, Microsoft-signed, NuGet) staged in overlay | done |
| `mfplat_sys.dll` (copy of system `mfplat.dll`, target of TomBox's mfplat forwarder) staged in overlay | done |

## Boot progress

| Run | Reached | Ended by |
|---|---|---|
| 1 | GDK runtime services, xmem allocator (60 log lines, 4.4 s) | access violation: DirectStorage factory missing, null source buffer |
| 2 | DirectStorage working, engine init (14,440 log lines, 11.5 s) | fatal error: Media Foundation init failed (`mfplat_sys.dll` missing) |
| 3 | **2,400+ frames presented at 3840x2160** on this machine; online calls fail as expected (network guard, offline) | mdbg's default 70 s run limit |
| 4 | running, no time limit; title window clamped to the primary monitor | — |

## Next

1. Confirm the window stays on one monitor and what screen the game reaches offline.
2. Lobby.
3. Input (keyboard), then controller.
