# Blockers

What the emulator is still missing, per title, and who is on it. Updated every time a run finds a gap. Game dumps are complete and untouched; every entry here is an emulator gap.

Last update: 2026-11-21 (run 33: backdrop diag, 45 FPS lobby).

---

## Fortnite (Xbox Series X|S)

### 🟨 Lobby 3D backdrop: now drawn (run 35), detail missing
- **Seen:** run 22-27. The scenery around the character never reaches the GPU. The character, floor and stage discs draw.
- **Cause found:** Fortnite culls far objects with GPU occlusion queries. It nests a dummy outer query around the per-object queries. Xbox allows that; PC D3D12 allows one active occlusion query per command list.
- **Done:** `query_x.rs` un-nests them. Run 27 confirmed the nesting: 245 un-nested queries.
- **Still missing:** every sampled query result is still 0, so everything still reads as occluded.
- **Run 33 diag:** the occlusion boxes (VS `f89dddcf70078d07`) reach clipping (CPrimitives 10-12 of 12), but PSInvocations is 0 and no samples pass. The viewport and scissor are full screen. Suspects: depth-bounds or depth/stencil state in the PSO translation, or sample mask 0. The box PSO is not registered in the diag, so its creation path is not hooked.

### 🟨 Lobby hologram character / slow login
- **Seen:** the character shows as a cyan hologram, which means default profile/loadout data. Login→lobby takes about 29 s after the splash screen (QueryProfile 8.6 s).
- **Missing:** a full athena profile loadout in the in-process services, and connection reuse.

### 🟨 Performance
- **Run 33 lobby:** 45.2 FPS average (the console runs it at 60), with 1-2.5 s hitches. Frontend LoadMap takes 24.5 s.
- **Missing:** console frame pacing (SetFrameIntervalX / ScheduleFrameEventX), title core affinity, and hitch sources.
- **Translated shader cost (offline, 336 run35 shaders):**
  - **Cause:** every lane-masked VALU write became `if (exec_lo & 1u) v = ...;`. That produced 47,923 branches and 40,658 phi nodes in DXIL. The worst passes were the deferred-lighting PS `33a16d2f`, `a06ce28f` and `f3bc163c`, and the CS `4e9db7ab` and `eb81f3b1`, at 6-7x the ISA instruction count.
  - **Fix:** pure ALU writes now use `select()`. This gives the same result for active and inactive lanes. Wave, quad, derivative, sample and memory ops keep the branch.
  - **Result:** branches 47,923 to 6,668, phi 40,658 to 20,302, DXIL 460,604 to 400,850 instructions, no new compile failures. The top pixel shader went from 7.0x to 5.0x the ISA, with 3,908 branches down to 224.
  - **Still to do:** GPU ms per pass in a live run (`TOMBOX_GPUPROF=1`, A/B with `TOMBOX_XLAT_GUARD_BRANCH=1`).
  - **Not used:** no 16-bit packed math (`v_pk_*`) appears in these shaders, so there is no 16-bit gap to recover. SampleGrad appears in only 1 shader (in-loop implicit samples).
- **GPU hitches are waits, not shader cost:**
  - **Run 40:** 145 HITCHHUNTER GPU hitches, 70-1,250 ms each, about 2 per second through the lobby. Many are on frames of only 30-50 draws.
  - **Run 35 was worse:** up to 10.5 s on frames of 39-52 draws.
  - **Likely cause:** that much GPU time on so few draws means the timed interval includes idle or wait time (cross-queue fence waits, late submits) or a timestamp problem.
  - **Next:** run with `TOMBOX_GPUPROF=1`. It reports summed per-draw GPU ms next to the wall span; a large gap between them confirms waits.
- **Anaconda (XSX_Anaconda profile) black scene, offline checks:**
  - 62 shaders appear only in Anaconda runs (37-39). All translate and compile.
  - No PSO failures (pso_fail=0, untranslatable=0).
  - None of the new compute shaders uses LDS or 16-bit packed math.
  - So it is not a translation failure. It needs an RT capture.
  - Occlusion results: about 97% are zero in runs 39 and 40, against 17% in run 35. The early proxy-box diagnostics are identical, so this may reflect the scene rather than a query bug.
- **After that:** the older build drew the backdrop black (PS `31dd3b33133e9ca3`). Its material constant buffers (cb1/cb2) are bound as root CBVs, and the trace does not capture those yet.

### 🟨 Flashing rectangle (bloom inf) - FIXED
- **Seen:** a top-left cream block, about 46% wide and 31% tall.
- **Cause:** infinite values in the R11G11B10F bloom chain.
- **Done:** run 27 clamps overflowing UAV stores toward zero, as AMD hardware does. Very bright stage-light texels no longer reach large inf counts.
- **Root cause (fixed in target-shade6, verified in run 35):**
  - The TAA-like compute shaders 1e8bbd9b and 251ed6ba share LDS with no `s_barrier`, which is only safe under wave64 lockstep. Per-lane HLSL raced, so the history exploded (up to 57120), went inf, and the bloom spread it into a block.
  - The translator now inserts `GroupMemoryBarrierWithGroupSync` at LDS write→read and read→write hazards.
  - Run 35 captures 10/11/12: inf=0 in every target, the 3200x1800 history peaks at 0.43, and the 1600x900 first stage peaks at 488-536 (was 1000+).

### 🟨 Audio
- **Done:**
  - The XMA2 decoder (crate `xma`, ported from FFmpeg wmaprodec) is within 1 LSB of ffmpeg. Hardware mode keeps frame 0, so output is 512 × frames.
  - The Opus decoder (crate `opusdec`, ported from libopus) is within 1 LSB in all modes.
  - AcpHal, SHAPE, xapu and chad run on the CPU.
  - Record/replay: `TOMBOX_AUDIO_RECORD=1` writes `%LOCALAPPDATA%\TomBox\audio-rec\*.tbrec`, or the folder in `TOMBOX_AUDIO_RECORD_DIR`. `target-audio\release\audio-replay.exe <file>` writes WAVs plus a stall report.
- **Decoder cost (release build, one stream):**
  - XMA2: 8-15 us per 512-sample frame, 945x real time. It was 299x before this round of optimisation.
  - Opus: 6-39 us per packet for CELT, SILK and hybrid; 143 us for 60 ms SILK stereo. That is 420x to 1780x real time.
- **Missing:**
  - SHAPE EQ, FILTVOL and PCM contexts: layouts unknown, and no title on disk drives them.
  - SRC paired with PCM contexts.
  - XMA loops are packet-granular, not sample-granular.
  - xapu codecs 1/3 and chad kinds 2/3 return E_NOTIMPL, because no title issues them.
  - SHAPE command types outside the known set return E_NOTIMPL.
  - Some xapu result fields are guessed: +0x1C, +0x1E, +0x22.
- **Recording caveats:**
  - EQ/FILTVOL arrays are not captured.
  - A title write that lands while a SHAPE op is running can be missed.
- **Open:** the XDefiant replay of a harness recording shows 1805 SRC_BLOCKED messages that the recorded run did not have. Being checked by the XAudio2/acphal agent.

### 🟨 System functions: 33 of 671 unfinished
- **xmem:** XMemTransferMemory, XMemFreePhysicalPages.
- **d3d12_xs:** D3D12XboxSetProcessDebugFlags.
- **XAudio2_9.dll:** ordinals #1, #2, #4, #5, #6.
- **MFPlat.DLL:** 16 functions. These now forward through the compat mfplat (233 forwarders, decoders bind).
  - Run 35 evidence (`media.json`, from `runs
un35	ombox_compat.log`): all 16 import slots plus the 2 MFReadWrite slots are bound in-game to the PC modules. Nothing failed.
  - **Missing:** in-game calls. Run 35 called only MFStartup (x3), MFCreateDXGIDeviceManager, MFResetDXGIDeviceManagerX and MFCreateAttributes. No source reader was opened, so 14 functions have no in-game call yet:
    - The `Movies/Bifrost/Comp 1-6.mp4` files are missing from the dump ("Failed to validate media source").
    - `ShouldShowVideo` returned false for SeasonTrailer.
- **MFReadWrite.dll:** 2 source-reader functions.
- **AcpHal:** 6 functions.
- **GameChat2.dll:** Fortnite's own DLL runs on PC; chatprobe passes all 14 imports in two peers, plus the opus/SILK codec tone round trip.
  - **Missing:** the emulated `XSystemGetAnalyticsInfo` family is `Xbox`. A real console returns `Windows.Xbox`, which puts GameChat2 in console mode (mode 2). With the family set to `Xbox`, GameChat2 never loads AudioHALWrapper and makes no Xbox Live calls. The GDK agent owns the switch in `gdk/system.rs` and the `gdk_runtime` test.
  - **Done (console path, proven with a temporary flip):**
    - `chatprobe --console` passes: AudioHALWrapper loads in both peers, and the RTA websocket connects. The mutelist and neverlist subscriptions are acked and later unsubscribed. 5 of 5 Xbox Live requests get JSON back, with nothing unhandled and no crash.
    - On the console path GameChat2 also loads the system `Windows.Media.MediaControl.dll`. It calls `CreateRenderAudioStateMonitorForCategory(8 = GameChat)`, then `IAudioStateMonitor::RegisterCallback` and `GetSoundLevel`.
      - `gdk/mediacontrol.rs` routes all 8 `Create*AudioStateMonitor*` exports through GetProcAddress and logs the category, apartment and sound level.
      - It holds the process MTA only when the caller has no COM apartment. The PC API's RegisterCallback fails with CO_E_NOTINITIALIZED there, but works on the console.
      - In the console run, the caller was already MTA (a GameChat2 thread) and the sound level was Full, as for a focused console title.
  - **Fixed:** the console-path crash at `GameChat2+0x52153`. The cause was an empty `XUserGetTokenAndSignature` signature: its UTF-8 to UTF-16 helper reads `[s + len - 1]`, which is out of bounds at length 0. Tokens now carry a real XSTS v1 signature: version, FILETIME and an ECDSA P-256 signature over method, path, token and body, from `gdk/xsign.rs`.

### 🟨 Online
- **Missing:** XMPP needs wss (the game refuses ws://).
- **Done:** an RTA V2 websocket (`online/rta.rs`, subprotocol `rta.xboxlive.com.V2`). It supports:
  - subscribe and unsubscribe acks;
  - `ConnectionId` data for sessiondirectory connections;
  - mutelist and neverlist data, plus `Added`/`Removed` events when privacy mute/avoid changes.
- **Done:** GameChat2's console requests in `online/xboxlive.rs`:
  - social `users/me/people` with paging;
  - privacy `permission/validate`, which now accepts capitalized keys;
  - `mutelist` and `neverlist`;
  - the profile `SpeechAccessibility` setting;
  - the userstats reputation stats.
- **Done:** an Xbox Live route that answers 200 with an empty body is now turned into a logged JSON 500.
- **Fixed:** net_guard dropped the websocket handle's HANDLE_CREATED callback, which WinHTTP sends synchronously inside `WebSocketCompleteUpgrade`, before the handle's parent is recorded. Handles created during the upgrade are now adopted on that thread; test: `tests/net_ws_async_rta.rs`. The console chatprobe run has no drops.
- **Done:** the cutscene video stream is emulated in-process.

### ✅ Resolved this session
- Cutscene blocks the lobby: fixed in run 26. The video plays and ends; mfplat now forwards every PC export, so the H.264/AAC decoders bind.
- GDK layer crash in EOS: fixed. Wrong argument order in the XNetworking slot.
- Frame-0 device removed (runs 24-25): fixed. Textures had been given a nonzero `GetGPUVirtualAddress`; it is now 0 as on PC.

### 🟥 Freezes when navigating menus
- **Seen:** run 35. Opening the item shop and other screens freezes the game. The log shows GPU hitches up to 1060 ms at about 329 draws, and game-thread hitches of about 305 ms. The lobby averaged 15.4 FPS while the user navigated and two 12-frame captures ran.
- **Suspects:** shader translation, compile and PSO creation run synchronously on first use (the console uses precompiled PSOs), waits on GPU sync points, and file IO for streaming.

### 🟨 3D scenes low detail
- **Seen:** run 35. Lobby structure surfaces are flat and untextured, the big screens are blank, and the challenge holotable map is a smeared blob.
- **Cause narrowed (run 35 capture 10):** texture residency, not shader translation.
  - GBufferC (base colour) and GBufferA (normals) show full detail on characters, but one flat value per environment object.
  - Material PS sampling checks out against the ISA (SampleBias operands correct). 257 PSOs translated, 0 failures.
  - The log has 400+ "upload unreadable placed texture" lines (the log cap) covering 179+ BC1/BC3/BC5 textures. Their mips 0-3 sit in MEM_RESERVE pages, and only the first 64 KB (low mips and mip tail) is committed. The title never commits or loads the high mips.
- **Found and fixed (target-shade7 xmem.dll): wrong memory stats.**
  - The game's FPlatformMemory reads XMemGetWorkingSetStatistics (flags 0x10000001) as +0 = title limit and +8 = title USED bytes. This is confirmed at five call sites, e.g. GetStats at 0x140964b14.
  - xmem returned PC total RAM at +0 and PC FREE RAM at +8. Run 36 logged "AppInit PhysicalAllocated 12459MB, PhysicalFree 20121MB": 12.4 GB "used" at boot, against a 13.5 GiB console budget, and falling as the game allocated.
  - It now reports a 13.5 GiB limit, with used = process private bytes + live physical pages.
  - To verify on the next run: the AppInit line shows small PhysicalAllocated, and the partly-unmapped texture count drops.
- **Missing:** find why texture streaming does not stream high mips in. Candidates are IoDispatcher bulk reads, the xmem commit/map of the reserved texture VA, and the streamer's budget/visibility inputs. The backdrop agent is on it.
- **Armed for next run:** `%TEMP%\tombox_trace_shaders.txt` lists base-pass PS hashes, so the next rtcap run dumps their SRV MostDetailedMip/MinLODClamp and placed mip counts.
- **Fixed alongside (target-shade7):** implicit-derivative samples inside loops were forced to LOD 0. They now use per-quad gradients (QuadReadLaneAt + SampleGrad, bias applied as a 2^bias scale), which matches the console.

### 🟨 Login with emulated XAsync: fixed on disk, confirming in run 37
- **Seen:** run 34. Login aborts at ConsolePrivileges when the emulated XAsync is the default. Run 35 (real XAsync) passes, with QueryProfile at 0.98 s (was 8.6 s).
- **Cause:** closing or terminating one composite queue cancelled the ports it shares with other composites, so later async work on them ended with E_ABORT (`XStoreQueryGameAndDlcPackageUpdatesResultCount failed with 0x80004004`). Fixed in `gdk/rt.rs`: each queue has its own terminate state, and ports count their users. `tests/gdk_multi_dll.rs` now runs the privilege + XStore update sequence on composite queues.
- **Also fixed:** one signed-in user got two different XUser handles across DLLs. The GDK owner now publishes one handle process-wide (`TOMBOX_GDK_USER_HANDLE`), and XUserCompare compares user identity. `xhost game_load_order` passes.

### ✅ Fortnite language paks 2/5/7/8/9 not mounting is correct console behaviour
- Layout: pakchunk2/5/7/8/9 are chunks 4/7/9/10/11 = de-DE, fr-FR, pl-PL, ru-RU, zh-CN. Fortnite skips them itself ("Locale not current", the en-US locale) before asking XPackage. No XPackage answer is involved.
- XPackage now reports real installed sizes from the layout and resolves language/tag/chunk/feature selectors. Selectors that match no chunk report Unavailable, and unknown package ids return E_NOT_FOUND. Test: `tests/gdk_package.rs`.

### 🟨 XGameSaveFiles: no title uses it
- None of Fortnite, XDefiant, Terraria or Trackmania calls XGameSaveFiles; their call sites only cover XGameSave. Its interface GUIDs are not in any dump, so it is not implemented. Add it when a title needs it.

### 🟨 Stale DLLs install a second, older GDK runtime
- **Seen:** 11-21 Terraria and Trackmania runs with `d3d12_x`/`xg_x`/`dxcompiler_x`/`xfrontpaneldisplay` from `target-xone
elease` (05:03-05:26). Their compat-common predates the single-owner claim, so they install a second runtime ("26 interfaces") after the owner. In Terraria that copy answered QueryApiImpl: the real XAsync and a placeholder XUser token.
- **Fix:** build every staged DLL from one tree and target. With all DLLs from `target-gdk
elease`, the log shows one install and every interface is emulated.

### 🟨 Match hosting (Workstream B): hooks on disk, no run yet
- **Built (11-21):** `os_host/fortnite_match.rs`. A 7-byte StartGameInstance patch makes `-REPLAY=` and a leading map URL on the command line work. In Play mode, a `UEngine::Browse` vtable hook (3 engine vtables) turns travel to the sentinel game server into a local `Apollo_Terrain?game=Athena_GameMode_C` load. All RVAs are checked against the dump by the `dump_constants_match` unit test. Launch with `run_fortnite.ps1 -Match Replay|Map|Play`; see `emulator\docs\MATCH_TEST.md`.
- **Unproven:** whether the patched StartGameInstance path plays BR_FortBench (replay version check); whether the client really reaches `Browse` with the sentinel URL, or stops at a reservation beacon (BEACONPORT_i 15009) first.
- **Expected next gates once the map loads (each needs its own hook file and run evidence):**
  - ReadyToStartMatch waits on a playlist (CurrentPlaylistInfo / FortPlaylistManager in a standalone game). The `Playlist=` URL option may not be read by this build.
  - MCP athena profile loading on the Map path, which skips the frontend login. The Play path keeps the login.
  - Pawn spawn ("AFortPlayerController::OnReadyToStartMatch: Marking player ... ready to start").
- **Outside B:** Apollo_Terrain is multi-GB. With xcrdapi on one worker the loading screen may sit for minutes. That is throughput, not a hook failure.

---

## Terraria (Xbox One, `d3d12_x`)

### 🟨 Runs 60 s without crashing, no frame presented yet
- **Fixed:**
  - `IDXGIDevice1` on the D3D12 device (QI, GetParent → the host adapter, private data, GPU thread priority, frame latency).
  - Xbox One queue descs (type 0x10 DMA → COPY, priority 1 → HIGH).
  - Descriptor heap type 4 → CBV_SRV_UAV. This fixed the AV at `EXE+0x7cbd32`.
  - DMA command allocators and lists (type 16).
  - `xg_x` is now the real GCN XG model. Tile modes, depth/stencil validation and texture layouts are byte-identical to the console `xg_x.dll` on about 1700 random cases. Before this, texture sizes came out as 0.
- **Missing:**
  - After `XGCreateTexture2DComputer` (2048x2048 fmt 65 mode 13), the log has no `PresentX`.
  - The cause is unknown. Frame events are not it: `SetFrameIntervalX` / `ScheduleFrameEventX` (types 0 and 2) arrive and the frame clock runs. It may be a fence wait (DMA/copy queue), or the upload path for Xbox One placed textures (`upload.rs` uses `xg_xs`, which Terraria does not load).
  - One run in two crashed after 6 s: an AV in ntdll on a networking/PlayFab Party thread, after a C++ exception.
  - 11-21 GDK sweep (40 s, one DLL set from target-gdk, d3d12_xs from target-xone): no E_NOTIMPL or unhandled GDK calls. Title view: Series X (device type 7), 7 cores, mask 0x7f. Still no `PresentX` in 40 s.
  - 11-21 thread dump at 30 s (`MDBG_HANGDUMP=30`): no title thread is blocked in a GDK call. The only GDK frame is Terraria's XTaskQueue dispatcher thread idling in `XTaskQueueDispatch` (manual queue, timed wait), which is normal. No XGameUi calls. The last GDK call is at about +17 s, the gamepad is never polled, and XGameSave is not initialized yet.
  - The IL2CPP game-logic thread waits in the kernel, called from `GameAssembly.dll +0x17f8ce`. Callers: +0x155b9a, +0x1b3dfc6, +0x1c70da, +0x1b3d409, +0x1361954, +0x135e60b, +0x1beaf5e, +0x1923c6, +0x168691. Unity threads wait at `Terraria.exe +0x55e2cf / +0x522eb5`. Next: name those GameAssembly frames from the IL2CPP metadata.
  - Online: `[online] unhandled POST /MultiplayerServer/ListPartyQosServers` (PlayFab Party QoS, `40467.playfabapi.com`). The run also fell back to the Fortnite service module, because Terraria had no title module.
    - Now on disk (`online/playfab`, not yet tried in a run): a Terraria/PlayFab module picked by `Terraria.exe`, and `*.playfabapi.com` is routed for every title.
    - It covers every PlayFab route found in `PartyWin.dll`, `PartyXboxLive.dll` and `PlayFabMultiplayerGDK.dll`: LoginWithXbox, ListPartyQosServers, GetCognitiveServicesToken, Party/RequestParty, Event/WriteEvents and WriteTelemetryEvents, Profile/GetTitlePlayersFromXboxLiveIDs, and all 20 Lobby/Match routes. It also covers the common Client/Authentication/Profile calls.
    - A UDP QoS responder on 127.0.0.1:3075 answers the 0xFFFF probe.
    - Missing: the Party relay. RequestParty gives out 127.0.0.1:30000, but the relay's DTLS protocol is not emulated, so CreateNewNetwork/ConnectToNetwork will fail after the REST calls.
    - Missing: PlayFab PubSub (SignalR websocket). Lobby and matchmaking changes are not pushed, and a title that waits for a push will stall.
    - Unknown: which of the ~295 Unity-SDK PlayFab calls in `global-metadata.dat` Terraria actually makes. Anything not implemented is logged as `[playfab]` unhandled.
- **Dump:** complete. All 19 layout file groups are present.

---

## Trackmania (Xbox One, `d3d12_x`)

### 🟨 Presents frames (60 fps at start, 34 fps average over 40 s); shaders not translated yet
- **11-21 GDK sweep (40 s, run_xone):** reaches the main menu at 55-60 fps (p99 17-50 ms). No E_NOTIMPL or unhandled GDK calls. Title view: Series X (device type 7), 7 cores, mask 0x7f.
- **Fixed:**
  - Packs on `G:\` and the Series device type (GDK layer).
  - `Close 0x80070057` is gone. The cause was a ResourceBarrier `RENDER_TARGET -> 0x4000c0`.
    - On Xbox the extra bit 0x400000 is combined with the shader-resource states. On PC that bit is `RAYTRACING_ACCELERATION_STRUCTURE`, which is valid only on its own.
    - `xone_barrier.rs` now strips the bit when it is combined with other states.
    - The device is no longer removed, and PresentX shows frames in the title window.
  - The bad command was found with `TOMBOX_CMDLIST_SPY=1` (`cmd_spy.rs`). It logs the last commands of a list, decodes their barriers, and prints them when Close fails.
- **Missing:**
  - 42 Xbox One (GCN/Durango) shaders.
    - `tombox-xshader/src/gcn.rs` parses the Durango blob and re-encodes gfx7 ISA as RDNA2 words.
      - 122/123 dumped shaders lower with no unknown opcodes.
      - The remaining one uses `s_getpc` (PC-relative code), which is not relocated yet.
    - HLSL translation: 0/123. The Durango user-SGPR binding model is not decoded yet.
      - Example: the CB pointer is in `s[3:4]` and a 32-bit table pointer in `s5`, while the RDNA2 path expects root tables at s8/s4.
      - The mapping is in the blob header and the tail after `code_end`.
    - Program range solved: the program starts at `code_start + 0x28` (after a 10-dword prefix) and its byte length is the u32 at header `0x1dc`. Data follows it. All 123 blobs decode exactly to a final `s_endpgm`.
    - MIMG `dim` is taken from the DA bit (2D or 2D array) until it can come from the resource info.
  - D3D12 debug layer: `d3d12SDKLayers.dll` (Agility 1.620.2) cannot be enabled.
    - `SetSDKVersion` needs Developer Mode.
    - `CreateDeviceFactory(620)` works, and the device is created through it, but the factory reports the SDK layers as not installed.
- **Dump:** complete. All 53 layout file groups are present, including `Packs\*.pak`.
- **Online (in-process, `online/trackmania`), not yet tried in a run:**
  - Done: Ubi session through the XDefiant module; Nadeo token exchange, refresh and nadeoservices; `/api/routes`; all 115 Core routes, stateful.
  - Missing: Live services (`live-services.trackmania.nadeo.live`) and Meet (`meet.trackmania.nadeo.club`) routes. These are logged as unhandled; the first run will list what the game calls.
  - Missing: real campaign, seasons and Track of the Day data. The game gets empty lists until the build's own map files are fed in.

## XDefiant rendering and startup (parked 2026-11-21)
- **Current state:** runs about 140 s at 60 fps. All graphics PSOs build, and command lists close cleanly after the aliasing-barrier fixes (`xone_barrier.rs`, `placed::is_committed_placement`). The presented frame is still all black. Captures at frames 1800 and 6900 show only compute dispatches and one ExecuteIndirect draw into a 960x540 target, and every target reads zero.
- **Aliased-VA render targets:**
  - The title creates pairs of `CreatePlacedResourceX` targets at the same Xbox VA (e.g. 0x3e2400000, 0x3e13f0000).
  - On console they share memory. Here each one is a separate committed resource, so a write through one alias never appears in the other.
  - Needs a real shared backing for same-VA placements: placed resources in one heap per VA range, or aliasing through a reserved resource.
- **`rta.xboxlive.com` websocket:**
  - The title retries `wss://rta.xboxlive.com/connect` every 10-25 s and never gets past startup.
  - RTA (real-time activity) must be emulated in-process: a websocket upgrade plus the RTA subscribe protocol.
  - Never make a real connection.
- **Texture-pool crash at `Rogue_scarlett_rtx.exe+0x67a8f9`:**
  - A NULL write about 140 s in (around frame 8400).
  - `rbx=0x80070057` and a nearby string `pool/rgba_float16 120x68x64/268` suggest a resource or view creation for a 120x68x64 R16G16B16A16_FLOAT 3D texture returned E_INVALIDARG. The title does not handle the failure.
  - `d3d12_xs` logged no failing call, so the failing method is an unlogged forwarder.
  - Run log: the ISA agent's scratchpad `xdrun19`.

## XDefiant online gaps
- Missing: catalog data (store items, seasons, battle pass). Nothing is loaded unless `xdefiant_catalog` or `<data_root>\xdefiant\catalog.json` exists; it must be built from the game's own files.
- Missing: Vivox voice transport (SIP/RTP). Tokens and config are issued locally, but there is no Vivox server emulation, so voice will not connect.

---

## Not started
- Keyboard input. The controller works through the virtual pad (`tools\dev\pad.ps1`).
