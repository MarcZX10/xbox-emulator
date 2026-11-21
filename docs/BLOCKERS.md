# Blockers

What the emulator is still missing, per title, and who is on it. Updated every time a run finds a gap. Game dumps are complete and untouched; every entry here is an emulator gap.

Last update: 2026-11-21 (run 33: backdrop diag, 45 FPS lobby).

---

## Fortnite (Xbox Series X|S)

### 🟨 Lobby 3D backdrop not drawn
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

### 🟥 Device removed after the D3D12 device is created
- **Fixed (GDK layer):**
  - The game reads its packs from the console drive `G:\`; those calls are now redirected to the title folder, and the packs install.
  - The device type now reports Series X for Xbox One titles, which fixed the crash at `GbxGame+0x800950`.
  - The `twinapi.appcore` AV at teardown and the null TaskQueue error are gone.
- **Missing:**
  - Xbox One (GCN/Durango) shader translation: 42 lines of `shader_xlat FAIL … magic=[…]`, because Xbox One shader containers differ from Series ones.
  - A valid command list: `Close FAILED 0x80070057` → device removed → AV at `GbxGame+0x894c3b`.
- **Dump:** complete. All 53 layout file groups are present, including `Packs\*.pak`.

---

## Not started
- Keyboard input. The controller works through the virtual pad (`tools\dev\pad.ps1`).
