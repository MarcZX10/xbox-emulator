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
| 4 | title window clamped to the primary monitor (40,40 1936x1119); splash screen | login fails offline: `Unable to join the Fortnite login queue` |
| 5-7 | local backend (LawinServer on 127.0.0.1:3551, all hosts redirected there, HTTPS -> plain HTTP in the emulator); waiting room + splash passed; oauth token 200 | stalls at "CHECKING FOR UPDATES..." (hotfix dispatcher sends no request after the token) |
| 8-9 | WinHTTP tracing in net_guard (request paths, async errors); datarouter POSTs reach the backend (204) | run 8: crash in callback wrapper (fixed, per-handle routing); run 9: SetOption override made Xbox Live stats hang (reverted) |
| 10 | running, backend logs request bodies/responses | — |

## Next

1. Get past "Checking for updates" (hotfix/cloudstorage) and the xbl login.
2. Lobby.
3. Input (keyboard), then controller.
