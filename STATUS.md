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
| 10-14 | client asks for `eg1` JWT tokens (now issued); WinHTTP read tracing showed **no response was ever consumed** | root cause: the emulator's WinHttpOpen hook cleared WINHTTP_FLAG_ASYNC together with SECURE_DEFAULTS, so no completion callbacks fired |
| 15 | **login completes** (`HandleLoginRequestComplete Result=2`), hotfix `UpdateSuccess`, Frontend map loaded, **UI state `Startup.SubgameSelect`**, then **Battle Royale lobby `UI.State.Athena.Frontend`** (09:23:16); still running and logging 5 min 16 s later, 0 crashes | running |
| 16-17 | **online services emulated inside the emulator** (no separate server process): login `Result=2`, hotfix `UpdateSuccess`, lobby `UI.State.Athena.Frontend` 09:48:24; account name `XboxEmulator`, no third-party branding; virtual Xbox pad attaches to GameInput | run 16: crash at EXE+0xbd9441 with the virtual pad on (being fixed; run 17 ran with it off); yellow band still present after the fp16 export fix — 123 render targets captured for diagnosis |

## Next

2. Lobby rendering: pale-yellow fill over the top third, black background regions, character material, bloom.
3. Own library/progress map (replacing upstream's hot-linked images).
4. Virtual gamepad control channel (no real mouse).
3. Input (keyboard), then controller.
