# Blockers

What the emulator is still missing, per title, and who is on it. Updated every time a run finds a gap. Game dumps are complete and untouched; every entry here is an emulator gap.

Last update: 2026-11-21 (runs 23-27, first Terraria and Trackmania boots).

---

## Fortnite (Xbox Series X|S)

### 🟨 Lobby 3D backdrop not drawn
- **Seen:** run 22-27. The scenery around the character never reaches the GPU. The character, floor and stage discs draw.
- **Cause found:** Fortnite culls far objects with GPU occlusion queries. It nests a dummy outer query around the per-object queries. Xbox allows that; PC D3D12 allows one active occlusion query per command list.
- **Done:** `query_x.rs` un-nests them. Run 27 confirmed the nesting: 245 un-nested queries.
- **Still missing:** every sampled query result is still 0, so everything still reads as occluded. Next: the occlusion-box draw (VS `f89dddcf70078d07`), depth state, and the resolve offsets.
- **After that:** the older build drew the backdrop black (PS `31dd3b33133e9ca3`). Its material constant buffers (cb1/cb2) are bound as root CBVs, and the trace does not capture those yet.

### 🟨 Flashing rectangle (bloom inf)
- **Seen:** a top-left cream block, about 46% wide and 31% tall.
- **Cause:** infinite values in the R11G11B10F bloom chain.
- **Done:** run 27 clamps overflowing UAV stores toward zero, as AMD hardware does. Very bright stage-light texels no longer reach large inf counts.
- **Still missing:** #093, #095, #125 and #126 still have thousands of inf texels next to near-zero data. That points to garbage input or a target that is never cleared, not overflow. The pass that writes them is being traced.

### 🟨 Audio
- **Missing:** XMA2 and Opus decoders (being ported from FFmpeg/libopus sources), and the console-name `XAudio2_9.dll` with XMA source voices.
- **Status:** AcpHal, SHAPE, xapu and chad emulation are in progress.

### 🟨 System functions: 33 of 671 unfinished
- **xmem:** XMemTransferMemory, XMemFreePhysicalPages.
- **d3d12_xs:** D3D12XboxSetProcessDebugFlags.
- **XAudio2_9.dll:** ordinals #1, #2, #4, #5, #6.
- **MFPlat.DLL:** 16 functions. These now forward through the compat mfplat (233 forwarders, decoders bind, in-game proof pending).
- **MFReadWrite.dll:** 2 source-reader functions.
- **AcpHal:** 6 functions.
- **GameChat2.dll:** not emulated yet.

### 🟨 Online
- **Missing:** XMPP needs wss (the game refuses ws://).
- **Done:** the cutscene video stream is emulated in-process.

### ✅ Resolved this session
- Cutscene blocks the lobby: fixed in run 26. The video plays and ends; mfplat now forwards every PC export, so the H.264/AAC decoders bind.
- GDK layer crash in EOS: fixed. Wrong argument order in the XNetworking slot.
- Frame-0 device removed (runs 24-25): fixed. Textures had been given a nonzero `GetGPUVirtualAddress`; it is now 0 as on PC.

---

## Terraria (Xbox One, `d3d12_x`)

### 🟥 Exits 1 s after start
- **Reached:** the GDK runtime services are created, and the Xbox One D3D12 device is created through `d3d12_x` → `d3d12_xs`.
- **Missing:** `IDXGIDevice` on the D3D12 device. The `QueryInterface {54ec77fa-1377-44e6-8c32-88fd5f44c84c}` call returns E_NOINTERFACE. The game uses it to reach the adapter, factory and swap chain, so it throws and exits.
- **Dump:** complete. All 19 layout file groups are present.

---

## Trackmania (Xbox One, `d3d12_x`)

### 🟥 Init fails after 5 s
- **Reached:** the GDK services, XCurl, and UbiServices (emulated, no Ubisoft traffic).
- **Missing:** the XPackage answers the game needs for its resource packs. The log shows `InstallGameResourcePacks failed` and then `CGbxApp::TheApp->Init() failed`.
- **Also missing:**
  - The default process task queue. The log shows `XERROR HR:80070006 Handle null (TaskQueue)`.
  - A Windows app-model call that needs package identity. It causes an access violation in `twinapi.appcore.dll` during teardown.
- **Dump:** complete. All 53 layout file groups are present, including `Packs\*.pak`.

---

## Not started
- Keyboard input. The controller works through the virtual pad (`tools\dev\pad.ps1`).
