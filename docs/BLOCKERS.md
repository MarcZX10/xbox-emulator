# Blockers

What the emulator is still missing, per title, and who is on it. Updated every time a run finds a gap. Game dumps are complete and untouched; every entry here is an emulator gap.

Last update: 2026-11-21 (run 33: backdrop diag, 45 FPS lobby).

---

## Online policy (user decision, 2026-11-21): offline, private sessions only
- The user's words: "keep it offline, like its just going to be private lobbys and private games".
- No external traffic, ever, for every title. Content that exists only online (VOD, CDN art) gets placeholders served in-process.
- Matchmaking and ready-up land in private lobbies and private or custom matches, hosted by the in-process backend and the Party relay. Public queues are never used.
- Party, Meet and session work targets private sessions between local or LAN clients.
- Applied so far: a PlayFab matchmaking ticket resolves at once into a private match with the local tickets in its queue, and does not wait for strangers.
- **Scope change (user, 2026-11-21): single player only for now.** Multiplayer work is stopped:
  - The shared LAN backend (`online/lan.rs`) is built and tested, including a two-process loopback test. It is off by default (`lan=off`) and not being extended.
  - The PlayFab Party relay and PubSub are not emulated and are not being worked on.
  - Nadeo Meet (Trackmania) is not implemented and is not being worked on. Meet calls are logged as unhandled and get a Nadeo 404.
- Single-player online answers, all in-process: Xbox Live achievements, peoplehub and userpresence (`online/xbl_title.rs`), RTA, PlayFab login/QoS/profile/events, Ubi sessions, Nadeo Core and Nadeo Live services. Every request that is not answered is logged as `[online] unhandled [<title>] ...` in that title's `tombox_compat.log`.

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
  - **Run 42 (gpuprof live) confirms waits:**
    - **Normal windows:** summed GPU 6.2-8.9 ms/frame against a wall span 0.7-1.0 ms/frame larger.
    - **Window 2:** the only one that overlaps hitches (f744-f784). It lasted 1540 ms for 16 frames, but GPU timestamps cover only 374 ms of that, and only 129 ms of it is GPU work. No single in-list interval was over 100 ms. So the GPU sat idle with nothing submitted.
    - **47 "Hitch in GPU", 20.6 s total.** 12 of them (6.7 s) line up with shader-translation waits ("caller thread blocked N ms") plus driver CreateGraphicsPipelineState time in the same frame. UE agrees: STAT_PSOGraphicsFindOrCreateTime appears 14 times (7.9 s).
    - **The rest have no emulator log line.** The worst are f689 (2.86 s) and f692 (2.61 s): IO and prewarm are idle and d3d12_xs logs only perf. The UE scopes there are TaskGraphThreadNP STAT_ParallelChainTranslate (2.79 s, the RHI recording into our command-list wrappers), PoolThread MipCalc (2.2 s) and GameThread FAudioCommandFence_Wait (2.37 s).
    - **Open:** a CPU-side stall inside a wrapped call or a process-wide stall. Candidates are a synchronous GPU wait while holding a lock (vmem_x D3D12ZeroMemoryX waits on its fence with `GPU` held) or the upload pump (max 60 ms per present). It needs per-call slow-call timing on the command-list and queue wrappers.
  - **Fixed:** gpuprof labelled every row 0. `shader_xlat::ORIGIN` is only filled when rtcap or hot_reload is on. There is now a hash-only map that is always filled (`note_origin_hash` / `origin_hash`), and gpuprof uses it.
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
  - xapu is asynchronous, like the console. The console's `XApuEnqueueCommand` only copies the command into a shared submit ring (xapu.dll sub_37b0). `XApuGetResultCount` reads a completion ring the device fills. Our version now queues each command to a host worker pool (cores minus one, at most 8). Per-stream order is kept, results commit in submission order, and the result event fires on commit. `TOMBOX_XAPU_SYNC=1` decodes on the calling thread instead. Async and sync produce identical results and PCM (test `async_results_commit_in_submission_order`).
  - Fortnite "enables with input but no output" are resolved. With the recorder race fixed, a fresh harness recording shows 0 of 60104 enables (it was 109, then 362). All of them were recorder artifacts, not a real end of stream and not a SHAPE bug.
  - Record/replay: `TOMBOX_AUDIO_RECORD=1` writes `%LOCALAPPDATA%\TomBox\audio-rec\*.tbrec`, or the folder in `TOMBOX_AUDIO_RECORD_DIR`. `target-audio\release\audio-replay.exe <file>` writes WAVs plus a stall report.
- **Decoder cost (release build, one stream):**
  - XMA2: 8-15 us per 512-sample frame, 945x real time. It was 299x before this round of optimisation.
  - Opus: 6-39 us per packet for CELT, SILK and hybrid; 143 us for 60 ms SILK stereo. That is 420x to 1780x real time.
  - xapu data command: 41 us mean for hybrid stereo, of which 39 us is the decode.
  - At console maximum concurrency the load nears one core: 512 stereo XMA contexts take about 70%; 500 Opus hybrid streams take about 97%.
- **Missing:**
  - SHAPE EQ, FILTVOL and PCM contexts: layouts unknown, and no title on disk drives them.
  - SRC paired with PCM contexts.
  - XMA loops are packet-granular, not sample-granular.
  - xapu codecs 1/3 and chad kinds 2/3 return E_NOTIMPL, because no title issues them.
  - SHAPE command types outside the known set return E_NOTIMPL.
  - XMA output ring fills to c-1 blocks and never clears d1 bit31. On the console it fills all c blocks and clears OutputBufferValid when full: Fortnite 0x1406f2fce takes the cold path at 0x14262da2a, reading r==w with bit31 clear as a full ring. It is left as is because how the hardware sets bit31 again after a drain is unknown. No title on disk fills the ring (Fortnite asks for at most 8 of 31 blocks and drains every pass; Wwise pulls one block at a time). XmaState::service and pull_frame should change together once a title or a console trace shows the re-validate rule.
  - Some xapu result fields are guessed: +0x1C, +0x1E, +0x22.
- **Recording caveats:**
  - EQ/FILTVOL arrays are not captured.
  - Fixed: a title write that landed while a SHAPE op was running could be absorbed into the shadow and missed. The engine's `wr`/`set_bits` now log what they write during recording, and the shadow takes only those bits.
  - Recording costs about 8 us per op. The Fortnite harness takes 1.5-2.1 s with recording, against 0.6 s without.
  - Fixed: the XDefiant replay mismatch. The packet cache was not cleared when APU memory was freed. Same-build record and replay now match for all three titles.

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
  - Found 11-21: the cause is texture GPU VA = 0 (see below). Frame events are not it: `SetFrameIntervalX` / `ScheduleFrameEventX` (types 0 and 2) arrive and the frame clock runs. It may be a fence wait (DMA/copy queue), or the upload path for Xbox One placed textures (`upload.rs` uses `xg_xs`, which Terraria does not load).
  - One run in two crashed after 6 s: an AV in ntdll on a networking/PlayFab Party thread, after a C++ exception.
  - 11-21 GDK sweep (40 s, one DLL set from target-gdk, d3d12_xs from target-xone): no E_NOTIMPL or unhandled GDK calls. Title view: Series X (device type 7), 7 cores, mask 0x7f. Still no `PresentX` in 40 s.
  - 11-21 thread dump at 30 s (`MDBG_HANGDUMP=30`): no title thread is blocked in a GDK call. The only GDK frame is Terraria's XTaskQueue dispatcher thread idling in `XTaskQueueDispatch` (manual queue, timed wait), which is normal. No XGameUi calls. The last GDK call is at about +17 s, the gamepad is never polled, and XGameSave is not initialized yet.
  - The IL2CPP game-logic thread waits in the kernel, called from `GameAssembly.dll +0x17f8ce`. Callers: +0x155b9a, +0x1b3dfc6, +0x1c70da, +0x1b3d409, +0x1361954, +0x135e60b, +0x1beaf5e, +0x1923c6, +0x168691. Unity threads wait at `Terraria.exe +0x55e2cf / +0x522eb5`. (That thread turned out to be the `CacheFileManager::FileUpload` worker, not the main thread.)
  - **Cause of the first-frame hang (found 11-21, the fix belongs to the Xbox One D3D layer, `d3d12_xs/src/gpu_va.rs`):**
    - A proper unwind of the main thread shows the C# path `XNAUnityRunner::Start` → `Main::Initialize` → `ClientInitialize` → `LoadSettings` → `GUIMap::set_ActiveFrameStyle` → `GUIMap::LoadContent` → `ContentManager::LoadTexture2D` → `Texture2D::get_UnityTexture` → `TextureAtlasDB::AltlasTexture`. Unity then waits forever in `Terraria.exe +0x4873c0` (the sync wait for an async texture upload): `WaitOnAddress` loop until `texture+0xe9` is set.
    - `+0x442a60` starts that upload. It asks the texture-memory object (`[+0x1892a88]`, vtable slot 0x598 = `+0x7f6b30`) for a CPU pointer to the texture's memory. That calls `ID3D12Resource::GetGPUVirtualAddress` (vtable slot 11) and uses the result as a CPU address, because on the console the GPU VA and the CPU VA are the same unified memory.
    - Our `get_gpu_va_x` returns 0 for committed textures unless `TOMBOX_TEXTURE_VA=1`. With 0, Unity takes the error branch at `+0x442bbe`, the "async texture load: ... (unable to get texture memory)" path. That message never appears in our logs. Unity drops the request there and never sets the completion flag. Live memory at 30 s matches: texture `e8=1 e9=0`, AsyncUploadManager idle (32 free slots, nothing pending), render thread and AsyncRead thread idle.
    - Check: the same run with `TOMBOX_TEXTURE_VA=1` gets past the hang (CopyTextureRegionX, PSO creation, shader_xlat). It then dies at 8.5 s. Two upload-heap buffers fail `CreateCommittedResource` (E_INVALIDARG; heap type 2, state 0x8c3, flags 0x8 deny-SRV, 256x1 and 52x1), and two VS shaders fail with "inflated N bytes but magic=[1,0,0,0]". Unity then logs "D3D12: Removing Device" and an AV follows at `EXE+0x7e7048`.
    - Needed: texture VA on by default, with CPU-visible memory per texture that is synced into the real texture before the GPU uses it.
      - Unity writes the texels there in the tiled layout that `xg_x` reported for the texture (mode 13), so the sync must detile with that layout. It cannot copy the bytes as linear.
      - Today `back_committed_texture` memory never reaches the real texture.
    - Needed: translate upload-heap creation in `CreateCommittedResource`. On PC an upload heap (type 2) must start in `GENERIC_READ` (0xAC3); Terraria passes 0x8c3, which has no `COPY_SOURCE`. `DENY_SHADER_RESOURCE` (0x8) is only valid together with `ALLOW_DEPTH_STENCIL`.
    - Needed: handle that shader container form.
    - No GDK call is involved.
    - 11-21 fixes on disk (d3d12_xs, target-xone, not yet run):
      - Texture VA is on by default for Xbox One titles (when `d3d12_x.dll` is loaded; `TOMBOX_TEXTURE_VA=0` turns it off). The backing is sized to at least the `xg_x` layout size.
      - `upload.rs` builds layouts from `xg_x.dll` when `xg_xs.dll` is absent, with the GCN layout's bytes-per-element at 0x38. So committed backings registered in `placed` are detiled with the mode-13/14 element offsets of `xg_x`'s computer (slot 6).
      - `CreateCommittedResource`: an upload heap starts in GENERIC_READ (0xAC3), a readback heap in COPY_DEST, and DENY_SHADER_RESOURCE without ALLOW_DEPTH_STENCIL is dropped.
      - VS containers: the `magic=[1,0,0,0]` blob is a 144/184-byte variant table. The DXIL part holds three Durango variants (header word 0x20 = 4/2/0). The extractor now takes the variant that exports a position.
      - Those two VS still fail: every variant starts with `s_swappc_b64 s[0:1], s[0:1]`, a call to a fetch shader that the console runtime builds from the input layout. Needed: synthesize the attribute fetch (VGPR assignment from the Durango input metadata).
  - Online: `[online] unhandled POST /MultiplayerServer/ListPartyQosServers` (PlayFab Party QoS, `40467.playfabapi.com`). The run also fell back to the Fortnite service module, because Terraria had no title module.
    - Now on disk (`online/playfab`, not yet tried in a run): a Terraria/PlayFab module picked by `Terraria.exe`, and `*.playfabapi.com` is routed for every title.
    - It covers every PlayFab route found in `PartyWin.dll`, `PartyXboxLive.dll` and `PlayFabMultiplayerGDK.dll`: LoginWithXbox, ListPartyQosServers, GetCognitiveServicesToken, Party/RequestParty, Event/WriteEvents and WriteTelemetryEvents, Profile/GetTitlePlayersFromXboxLiveIDs, and all 20 Lobby/Match routes. It also covers the common Client/Authentication/Profile calls.
    - A UDP QoS responder on 127.0.0.1:3075 answers the 0xFFFF probe. The 7:14 run logged bind error 10048 on 3075, most likely because a unit test held the port at the same time. Unit tests now use port 0 (`TOMBOX_PLAYFAB_QOS_PORT`, cfg `playfab_qos_port`), and the responder retries the bind on the next QoS call.
    - Single-player world load: the 7:14 run made only the ListPartyQosServers call and then crashed in rendering (AV at `Terraria.exe+0x7e7048`). That crash is not an online gap.
    - Paused (multiplayer, out of scope): the Party relay. RequestParty gives out 127.0.0.1:30000, but the relay's DTLS protocol is not emulated, so CreateNewNetwork/ConnectToNetwork will fail after the REST calls.
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
- **11-21 GDK run (run_xone, overlay `run-xone-gdk`, all DLLs from `target-gdk\release`):** 115-129 fps (the d3d12_xs frame clock currently follows the host's 144 Hz; `TOMBOX_FRAME_HZ=console` restores the console interval). It crashed at 42.7 s with an AV write at 0x8 in `GbxGame+0x153017d`. The call came from libHttpClient's HCCleanupAsync termination callback `+0x1542270`, running on a thread spawned by our `XTaskQueueTerminate`.
  - Cause (GDK layer, `gdk/rt.rs`), two bugs:
    - Our XAsync passed the provider a stack `XAsyncProviderData`. libHttpClient keeps that pointer and reads `[data+0x18]` (the context) later.
    - `XTaskQueueTerminate(wait=false)` fired the callback at once on a new thread. It should run after the pending items, through the work port and then the completion port; on manual queues it should run during dispatch.
  - Fix on disk: a persistent ProviderData in each async state, and the termination is delivered through `submit_marker` on the work and completion ports. Test: `tests/gdk_rt_terminate.rs`. Not re-run in game: in-game QA is paused until the progress map reaches 100%.
- **Dump:** complete. All 53 layout file groups are present, including `Packs\*.pak`.
- **Online (in-process, `online/trackmania`), not yet tried in a run:**
  - Done: Ubi session through the XDefiant module; Nadeo token exchange, refresh and nadeoservices; `/api/routes`; all 115 Core routes, stateful.
  - Done (`online/trackmania/live.rs`): Live services (`live-services.trackmania.nadeo.live`), with the NadeoLiveServices token check. Covered:
    - official campaigns (v1 and v2), Track of the Day months, cup of the week, weekly grands and shorts, map campaign info, and maniapubs;
    - map info, get-multiple, uploaded maps, and favourites (add, remove, list);
    - leaderboards: player records, position for a score, top, surround, medals, campaign ranking and top, and trophies. These are computed from the Core map records, so a solo personal best shows up at once;
    - clubs, stored locally: create, edit, delete, mine, search, members, join, pin, tag, activities, news, folders, and club campaigns.
  - Not handled, logged as unhandled: club rooms, competitions, map review, player-server accounts and club leaderboards. These are multiplayer features.
  - Meet (`meet.trackmania.nadeo.club`): not implemented. Multiplayer is out of scope.
  - Missing: real official campaign, Track of the Day and weekly data. Lists come from cfg `trackmania_live_data` or `<data_root>\trackmania\live.json`; without that file they are empty, and the log says so.
    - Campaign maps also need Core map entries. A `"maps"` array in the same file (`{mapUid, name, authorScore, goldScore, silverScore, bronzeScore, file}`, with `file` relative to the JSON) registers each map in Core at startup, along with its file under `storageObjects`. Then `/maps/?mapUidList=`, the downloads and the Live map info resolve.
    - Where the official map files are has not been found yet. A plain-text search of `Trackmania.Title.Pack.Gbx`, `Titles.pak` and `Stadium.pak` turned up nothing, but those packs are compressed or encrypted.
  - Meet's main-menu polls (for example the current cup of the day) get a logged Nadeo 404 where the real service may answer 204. The first Trackmania run that reaches online will show whether that matters.
  - The 6:27 run reached no online calls; it stopped before the online stage.

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

- **First blocking wait:** not found yet. The 11-21 GDK pass did not run XDefiant, because in-game QA is paused until the progress map reaches 100%.

## XDefiant online gaps
- The 6:58 run used stale DLLs. It logged `xdefiant-stub ... (generic empty answer)`, a string that no longer exists in the source, and those DLLs had no RTA emulation. The current build (`D:\TomBox\target-xcurl\release`) has:
  - in-process RTA (`online/rta.rs`; Fortnite's 7:47 run shows `[online] websocket [rta] /connect opened` and subscriptions answered);
  - achievements, peoplehub and userpresence (`online/xbl_title.rs`), the three calls the run made before RTA.
- Transport: `Rogue_scarlett_rtx.exe` uses libHttpClient, with HTTP over XCurl (`XCurl.dll`, `XCurlMulti`) and websockets over WinHTTP (`WINHTTP.dll`: `WinHttpWebSocketCompleteUpgrade`, `Send`, `Receive`, `Close`). There is no `curl_ws_*`. That is the same path net_guard and `rta.rs` already serve for Fortnite. In the stale run, no WinHTTP traffic followed the RTA security query, which fits DLLs built before the WinHTTP websocket fix.
- Missing: catalog data (store items, seasons, battle pass). Nothing is loaded unless `xdefiant_catalog` or `<data_root>\xdefiant\catalog.json` exists; it must be built from the game's own files.
- Missing: Vivox voice transport (SIP/RTP). Tokens and config are issued locally, but there is no Vivox server emulation, so voice will not connect.

---

## Not started
- Keyboard input. The controller works through the virtual pad (`tools\dev\pad.ps1`).

## Fortnite solo party and single-player path (2026-11-21, party agent)

- Party state is real now (online/fortnite/party.rs): create, GET party, GET user current, party PATCH and member-meta PATCH with revision and a 409 stale_revision check, plus PARTY_UPDATED, MEMBER_STATE_UPDATED and MEMBER_JOINED pushed from xmpp-admin. The run36 lobby sequence is replayed by the test run36_lobby_party_replay, and the notifications arrive in well under 2 s. Not yet seen in game: every run so far (35 to 41) predates this code. Next run must show no "TIMING OUT TASK [UpdateParty]" and must show "[party] PATCH ... rev N->N+1 published 1".
- The real PATCH body keys are not captured yet. Bodies are now logged as "[party] PATCH <path> body=..." in tombox_compat.log. If the client sends a revision the server does not expect, it gets 409, so check for "stale rev sent" lines.
- Single player: every playlist, including Playlist_Tutorial_1, BattleLab and Playground, is logged as "(Server Only)". The XSX client has no offline or local-match path in its strings. The only way in with no server is the client as its own authority (NM_Standalone): the StartGameInstance command-line patch at RVA 0xB46800 plus a leading map URL (run_fortnite.ps1 -Match Map, os_host/fortnite_match.rs). Not yet run in game. Open: the game-mode class path, ReadyToStartMatch without a playlist, MCP and inventory loading when the frontend is skipped.
- On user request, multiplayer matchmaking, MPSD, QoS and the LAN party work are paused. That code stays on disk, tested, but is not being extended.
- Run 42 update (party agent): the solo party fix is confirmed in game (0 UpdateParty timeouts). Play failed with invalid_bucket_id because QoS left the region at NONE (bucket `14457890:0:NONE:playlist_defaultsquad`). The ticket now treats NONE as the local region (`TOMBOX_FN_REGION`, default NAE). It still rejects the pre-init bucket `0:0:NONE:0`.
- Likely QoS cause: two datacenters (VA and OH) pointed at the same echo port, so both replies were credited to VA and OH stayed at 9999 ms. NAE now has one datacenter only. Unconfirmed whether the region is then selected.
- The session now writes the ticket's playlist into TOMBOX_FN_PLAYLIST, so the Play-mode Browse rewrite loads Apollo_Terrain with `?Playlist=<selected>`. Every Battle Royale playlist uses Apollo_Terrain. Creative and Playground playlists would need their own map plus game mode; not done.
- The lobby → Play → local match flow only works when the game is launched with `run_fortnite.ps1 -Match Play`. Run 42 was launched in Off mode: its log has no fn-match lines.
- Not verified, and the next risk after Play: the content beacon on BEACONPORT 15009 (failure should not cancel, because ContentBeaconFailureCancelsMatchmaking=0) and any reservation beacon. Watch for Beacon lines.

## Fortnite movies and loading screens (run41, 2026-11-21)
- Bifrost `Movies/Bifrost/Comp 1-6.mp4`: not in the dump (checked the extracted tree, EmbeddedXvd and the pak indexes). The six "Failed to validate media source" errors at 12:56:07 are expected. The game continues cleanly: the frontend loads, the second loading screen hides at 12:56:09.9, and the lobby clip opens, plays and ends at 12:56:59–57:00.
- No startup movies: DefaultGame.ini, XSXGame.ini and BaseXSXGame.ini (read with uex) have no MoviePlayerSettings/StartupMovies. The engine loading screen is the Slate FortniteEngineLoadingScreen, and the in-game loading screens use pak textures (Content/2dAssets/LoadingScreens/*). So no local mp4 is opened on the normal boot → frontend path; run41 has no source reader, which is correct. Onboarding, Wargames and HomebaseReturn movies play only in their own flows.
- Lobby clip path, verified in game (run41): Electra → H.264 MFT → D3D12 NV12 → `video.copy` into an R8 texture. The readback gave Y avg 15 / max 16, i.e. video black, which is what the old placeholder contained. The placeholder is now a 640x360 blue-gradient H.264 Main@3.0 clip (backend_data/fortnite/video/init.mp4 + seg0.m4s), so the next capture can show whether it reaches UMG. Test: mfplat proxy_dll `lobby_video_placeholder_decodes_to_nonblack_frames_on_every_path`.
- Offline MF harness (mfplat tests/proxy_dll.rs, per-mode tests): SatelliteConnection, Onboarding_Appended_Intro, WargamesVictory and HombaseReturn_OutpostIntro decode to non-black 1920x1080 frames in software and as D3D12 NV12 on the console device (MFResetDXGIDeviceManagerX).
- OPEN (host MF/driver, not on the Fortnite path): with a D3D12 manager, a source reader asked for RGB32 output gives all-zero frames for 1920x1080. 640x360 works. The game uses NV12.
- OPEN (host MF/driver, intermittent): a second D3D12 source reader on the same device sometimes fails its first ReadSample with DXGI_ERROR_DEVICE_HUNG (GetDeviceRemovedReason 0x887a0006). It reproduces with the plain IMFDXGIDeviceManager::ResetDevice as well, so MFResetDXGIDeviceManagerX is not the cause. Flushing the reader and shutting down the source don't help. Risk: a second in-game video on the same device. Not seen in a game yet.
- OPEN (most likely the user's "loading screen doesn't show"): from 12:55:29.68 (PresentX frame 3) to the splash at about 12:55:52, the game thread sits in engine init/LoadMap at frame [5] while the EngineLoadingScreen preload screen is active ("HasActivePreLoadScreenType(EngineLoadingScreen) Is True"). Yet frames 3→300 span 29.68→60.54 s, and the splash alone accounts for about 300 frames, so almost nothing is presented for about 22 s. Also logged: "LogMoviePlayer: PassLoadingScreenWindowBackToGame failed. No Window". Next step: run once with TOMBOX_RTCAP=1 and emulator/tools/dev/loading_capture.ps1. A "still-pending" mark means no present happened in that window. Then check the SlateLoadingThread / render-thread present path in d3d12_xs (owner: the d3d12/present agent).
- Measurement for the startup present stall (run41, from the d3d12_xs upload pump, which runs once per PresentX): presents 1–31 fall in 12:55:29.67–31.02, then nothing until present 32 at 12:55:37.95, a gap of 6.93 s. Between 37.95 and present 300 at 12:56:00.54 there are only 268 presents, against ~79/s in the lobby (presents 300→600 took 3.8 s), so this whole init/preload window presents rarely. Watcher for the capture run: emulator/tools/dev/loading_capture.ps1 -LiveLog <live ODS log> (frames=1, marks in UTC at %TEMP%\tombox_capture_marks.txt).
- mfplat proxy_dll: `fortnite_movies_decode_to_nonblack_nv12_on_console_d3d12_manager` and `..._rgb32_...` are #[ignore] with the two host-MF reasons above. The software test and `lobby_video_placeholder_decodes_to_nonblack_frames_on_every_path` stay active. d3d12_device() now panics if device creation fails after a device has existed, so a lost device can no longer pass as a skip.
- Run 43 crash (party agent, 07:53 build fixes the likely cause): an access violation in ntdll inside a winhttp websocket callback. The path was net_guard `cb_wrap` (xmem +0xd5b60/+0xd5efa) → the game's callback (EXE +0xbd93ec) → Websocket.dll. The faulting object held the RTA websocket handle 0x2910724f0 and looked freed and reused (its critical-section fields were garbage).
- Root cause found in net_guard: the game-callback table was keyed by raw handle value, was never cleaned when a handle closed, and child handles found their owner by walking the parent chain at callback time. WinHTTP reuses handle values. So when the upgrade request closed and a new request took the same value with its own callback, the websocket's events went to the wrong callback and context.
- Fix: a child now takes a copy of its parent's callback when it is created (as WinHTTP does). A creation purges any stale entry for that value, and HANDLE_CLOSING removes the closed handle's entry.
- Unit tests `net_guard::callback_owner_tests` cover it, and net_ws_async_rta and net_ws_winhttp still pass. Whether this was the run 43 crash is not proven: it was a single, timing-dependent crash. Run 42 used the same code paths and did not crash.

## xmem (emulator, all titles) - 2026-11-21, imports agent
- OPEN: the console XMemAllocatePhysicalPages (xmem.dll 0x1a40 -> 0x3704) refuses allocations with ERROR_NOT_ENOUGH_MEMORY once paged+nonpaged pool bytes (SystemPerformanceInformation +0x70/+0x74) exceed the hard limit, and warns once past the soft limit. The emulator keeps both limits (DllMain base + 32/30 MB, raised by XMemTransferMemory directions 0/1, reported by XMemGetWorkingSetStatistics internal flag 8) but does not apply the gate: on PC the kernel pool is shared with every other process, so a 32 MB headroom would fail title allocations at random. Needs a title-only pool measure before the gate can be wired.
- OPEN: the XMemPreallocateFreeSpace / hysteresis granule cache is used by XMemAlloc only for allocations that are exactly one granule (512 KB, or 2 MB with option 6 = 0; 4 MB on the 19041 console build that Fortnite ships, selected through the gameinput_os_build setting, which defaults to 22000 and is not set for Fortnite yet); the console carves every page-pool allocation out of granules. Alt pools (index type | 0x10) keep their own cache and statistics but no emulator allocation path draws from them, because which attributes select the alt pool is not decoded yet.
