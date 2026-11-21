# Library discovery log

Console and title libraries found while mapping the Xbox Series system surface. Every dump stays read-only.

## 2026-11-21 — Trackmania (Nadeo GBX engine, Xbox One GameCore) dump added

- Package `Ubisoft.TrackmaniaNext` 1.6.2.0 (TitleId `700107ED`, StoreId `9N01MR5LF53S`). `GameData/ApplicationBuildId` is `2024-04-30_c8c94a9ed_s1389bfd3_d028b741f_XB1`.
- Runtime: native C++ on Nadeo's GBX engine (ManiaPlanet lineage, `.pak`/`.Gbx` packs in `Mount/Packs`, including `GpuCache_XboxOne.zip`). The PDB path is `Release_Gaming.Xbox.XboxOne.x64`. UbiServices SDK 2020.Release.62.
- `MicrosoftGame.Config` (`configVersion="1"`) declares `TargetDeviceFamily="XboxOne"`, so like Terraria this is an **Xbox One** GameCore build. A config comment says it was copied from `XBox\MicrosoftGame_XBoxOne.config`. The config has no GDK version field. The EXE carries the string `October 2022 GRDK`, and GameChat2/XCurl are 2210.0.0.0, which agrees. `appxmanifest.xml` lists `Microsoft.GameOs` 10.0.18362.0.
- `Mount` modules: GbxGame.exe (44,931,072 bytes), GameChat2.dll, XCurl.dll, msvcp140 / vcruntime140 / vcruntime140_1 (MSVC 14.29).
  - `Mount/aegir/aegir.dll` (36 MB) is a WebKit/JavaScriptCore build: 12,887 exports in `WebCore` / `JSC`, used as the UI engine.
  - Neither GameChat2.dll nor aegir.dll is in GbxGame.exe's import table. aegir is loaded at run time (`aegir\loader\src\dllhelper.cpp` strings); how GameChat2 gets loaded was not confirmed. Both are classified like any shipped binary.
- Imports:
  - GbxGame.exe: 603 imports from 65 modules.
  - All 7 binaries: 1,579 import rows (934 distinct module+function pairs from 79 modules).
  - XGameRuntime is loaded by name (`xgameruntime.dll` string), not imported.
- Classification (sync.py/gen.py method): 1,567 / 1,579 covered (99.24%): pc 1,284, title 241, hooked 36, implemented 6, stub 1, missing 11. Static analysis only.
- Console OS (EmbeddedXvd System32): build **10.0.22621.1852**, the newest of the four dumps, with 251 export-bearing DLLs. Against XDefiant/Terraria (10.0.22000.4429):
  - 1 new DLL: `CorePrivacySettingsStore.dll`, 26 exports.
  - 107 new exports in shared DLLs. 230 shared DLLs carry a different file version.
  - Against Fortnite: 19 new DLLs and 617 new exports.
  - The console library map grows from 250 libraries / 23,356 exports to 251 / 23,489.

### Missing or unhandled imports (all in GbxGame.exe)

| module | functions | status | notes |
|---|---|---|---|
| `d3d12_x.dll` | D3D12XboxCreateDevice, D3D12SerializeRootSignature, D3DMapEsramMemory | missing | Xbox One D3D12.X; also needed by Terraria |
| `d3d12_x.dll` | D3D12GetDebugInterface, D3D12SerializeVersionedRootSignature | missing | only Trackmania |
| `xg_x.dll` | XGCreateTextureComputer, XGComputeOptimalTileMode | missing | also Terraria |
| `xg_x.dll` | XGComputeOptimalDepthStencilTileModes, XGComputeTexture2DLayout | missing | only Trackmania |
| `dxcompiler_x.dll` | DxcCreateInstance | missing | Xbox One DXC; emulator has `dxcompiler_xs.dll` (stub) only. Only Trackmania; it compiles shaders at run time. |
| `xmem.dll` | XMemGetWin32HeapStatistics | missing | not exported by the emulator's xmem; only Trackmania |
| `xmem.dll` | XMemGetWorkingSetStatistics | stub | shared with all titles |

### Console OS: new in 10.0.22621.1852 versus 10.0.22000.4429

| DLL | new exports | removed exports | new export names (truncated) |
|---|---|---|---|
| `CorePrivacySettingsStore.dll` | 26 (new DLL) | - | |
| `advapi32.dll` | 7 | 2 | OpenTraceFromBufferStream, OpenTraceFromFile, OpenTraceFromRealTimeLogger, OpenTraceFromRealTimeLoggerWithAllocationOptions, ProcessTraceAddBufferToBufferStream, ProcessTraceBufferDecrementReference, ProcessTraceBufferIncrementReference |
| `advapi32legacy.dll` | 0 | 6 |  |
| `chad.dll` | 2 | 0 | XDspConnectWithMaximumStreamLimit, XDspSubmitCommandWithEnvelope |
| `dbgeng.dll` | 2 | 0 | #501, #503 |
| `dbghelp.dll` | 2 | 0 | StackWalk2, SymSetServiceManager |
| `DiagnosticDataSettings.dll` | 3 | 0 | TelGetNumericPolicy, TelGetStringPolicy, TelIsRestrictivePolicySet |
| `dnsapi.dll` | 6 | 0 | CreateSendBlob, DnsConnectionGetProxyInfoForHostUrlEx, DnsGetDdrInfo, DnsGetDdrInfoCancel, DnsGetDdrInfoInternal, Query_InProcSetCompletion |
| `imagehlp.dll` | 2 | 0 | StackWalk2, SymSetServiceManager |
| `kernel32.dll` | 1 | 7 | RtlCompareMemory |
| `KernelBase.dll` | 19 | 0 | BuildIoRingFlushFile, BuildIoRingWriteFile, CommandLineToArgvW, GetPackageGraphRevisionId, GlobalFlags, GlobalHandle, GlobalLock, GlobalReAlloc, GlobalSize, GlobalUnlock, LocalFlags, LocalSize, LogStagedFeatureUsage, TestControlReporting, WilFailureN ... |
| `MMDevAPI.dll` | 1 | 0 | #34 |
| `npmproxy.dll` | 2 | 0 | NlmDiagSendWlanConnectionAttemptDiagnostics, NlmDiagSendWlanDisconnectionDiagnostics |
| `ntdll.dll` | 17 | 0 | NtCopyFileChunk, NtCreateCpuPartition, NtOpenCpuPartition, NtQueryInformationCpuPartition, NtSetInformationCpuPartition, RtlCallEnclave, RtlInterlockedClearBitRunEx, RtlLengthCurrentClearRunBackwardEx, RtlLengthCurrentClearRunForwardEx, RtlOverwriteF ... |
| `schannel.dll` | 1 | 0 | SslDeserializeCertificateStore |
| `sechost.dll` | 8 | 0 | OpenTraceFromBufferStream, OpenTraceFromFile, OpenTraceFromRealTimeLogger, OpenTraceFromRealTimeLoggerWithAllocationOptions, ProcessTraceAddBufferToBufferStream, ProcessTraceBufferDecrementReference, ProcessTraceBufferIncrementReference, ReparseServi ... |
| `TimeBrokerServer.dll` | 1 | 1 | SvchostPushServiceGlobalsEx |
| `wer.dll` | 1 | 0 | WerpHasOobeCompleted |
| `WerXboxGc.dll` | 1 | 0 | XerShouldWerManageRootDirectory |
| `winhttp.dll` | 8 | 0 | WinHttpConnectionOnlyConvert, WinHttpConnectionOnlyReceive, WinHttpConnectionOnlySend, WinHttpFreeProxySettingsEx, WinHttpGetProxySettingsEx, WinHttpGetProxySettingsResultEx, WinHttpRegisterProxyChangeNotification, WinHttpUnregisterProxyChangeNotific ... |
| `wldp.dll` | 8 | 0 | WldpCanExecuteBuffer, WldpCanExecuteFile, WldpCanExecuteStream, WldpChangeVulnerableDriverBlocklistState, WldpIsVulnerableDriverBlocklistDisabled, WldpIsVulnerableDriverBlocklistEligibleToDisable, WldpSendSmartAppControlBlockToast, WldpSendSmartAppCo ... |
| `xcrdapi.dll` | 7 | 1 | XCrdGrowDynamicXvd, XCrdMoveSidecarFiles, XCrdSetTitleQuirks, XCrdStorageCreateDirectory, XCrdStorageDeleteDirectory, XCrdStorageMoveBlob, XCrdStorageSetConsoleAssociation |
| `xpal.dll` | 8 | 2 | XpalGetConfiguration, XpalGetConsoleCertificate, XpalGetConsoleIdCch, XpalGetConsoleSerialNumberCch, XpalGetFactorySettings, XpalRequestCrashDump, XpalSetConfiguration, XpalSetInformation |

### Modules imported by Trackmania but not by Fortnite, XDefiant or Terraria

| module | functions |
|---|---|
| `api-ms-win-core-file-l1-2-2.dll` | AreFileApisANSI, GetTempPathA |
| `api-ms-win-core-threadpool-l1-2-0.dll` | CloseThreadpoolWork, CreateThreadpoolWork, FreeLibraryWhenCallbackReturns, SubmitThreadpoolWork |
| `api-ms-win-core-util-l1-1-0.dll` | DecodePointer, EncodePointer |
| `dxcompiler_x.dll` | DxcCreateInstance |

### Shared modules: functions Trackmania uses that no earlier title does

| module | new functions |
|---|---|
| `api-ms-win-core-com-l1-1-0.dll` | StringFromCLSID |
| `api-ms-win-core-fibers-l1-1-0.dll` | FlsGetValue |
| `api-ms-win-core-file-l1-1-0.dll` | CompareFileTime, FileTimeToLocalFileTime, GetDiskFreeSpaceA, GetDiskFreeSpaceW, GetFileInformationByHandle, GetFileTime, GetFinalPathNameByHandleW, GetFullPathNameA, LockFile, LockFileEx, ReadFileEx, UnlockFile, WriteFileEx |
| `api-ms-win-core-file-l1-2-0.dll` | GetTempPathW |
| `api-ms-win-core-file-l2-1-0.dll` | CopyFile2, CreateHardLinkW, CreateSymbolicLinkW |
| `api-ms-win-core-handle-l1-1-0.dll` | DuplicateHandle |
| `api-ms-win-core-heap-l1-1-0.dll` | HeapCreate, HeapDestroy, HeapSize |
| `api-ms-win-core-kernel32-legacy-l1-1-0.dll` | MoveFileW, RegisterWaitForSingleObject, UnregisterWait |
| `api-ms-win-core-localization-l1-2-0.dll` | EnumSystemLocalesA, GetCPInfo, GetLocaleInfoA, LCMapStringEx |
| `api-ms-win-core-memory-l1-1-0.dll` | CreateFileMappingW, MapViewOfFile, UnmapViewOfFile |
| `api-ms-win-core-processthreads-l1-1-0.dll` | ExitThread, GetExitCodeThread, GetProcessTimes, GetThreadId, OpenThread |
| `api-ms-win-core-processthreads-l1-1-1.dll` | GetThreadContext, GetThreadTimes, OpenProcess |
| `api-ms-win-core-psapi-l1-1-0.dll` | K32QueryWorkingSet |
| `api-ms-win-core-rtlsupport-l1-1-0.dll` | RtlPcToFileHeader, RtlUnwind, RtlUnwindEx |
| `api-ms-win-core-string-l1-1-0.dll` | CompareStringEx, CompareStringW, GetStringTypeW |
| `api-ms-win-core-synch-l1-2-0.dll` | InitOnceBeginInitialize, InitOnceComplete, InitOnceExecuteOnce, SleepConditionVariableSRW |
| `api-ms-win-core-sysinfo-l1-1-0.dll` | GetSystemDirectoryW, GetVersionExW, GlobalMemoryStatusEx |
| `api-ms-win-core-sysinfo-l1-2-0.dll` | GetSystemTimePreciseAsFileTime |
| `api-ms-win-crt-convert-l1-1-0.dll` | _atoi_l, _itoa_s, _strtol_l, _strtoll_l, _strtoul_l, _strtoull_l |
| `api-ms-win-crt-filesystem-l1-1-0.dll` | _access, _fstat64, _fstat64i32, _stat64, _unlink, _wstat64i32 |
| `api-ms-win-crt-math-l1-1-0.dll` | _fdsign, acosh, asinh, atanh, cbrt, fmaf, lrint, lround, lroundf, nextafter, nextafterf, remainder |
| `api-ms-win-crt-private-l1-1-0.dll` | __current_exception, __current_exception_context, _o__dclass, _o_abort, _o_sqrtf |
| `api-ms-win-crt-runtime-l1-1-0.dll` | __sys_nerr, _fpreset, _getpid, _wassert |
| `api-ms-win-crt-stdio-l1-1-0.dll` | _lseek, _lseeki64, _open, clearerr, getc |
| `api-ms-win-crt-string-l1-1-0.dll` | wcspbrk |
| `api-ms-win-crt-utility-l1-1-0.dll` | qsort_s |
| `bcrypt.dll` | BCryptDecrypt, BCryptDestroyKey, BCryptEncrypt, BCryptExportKey, BCryptFinalizeKeyPair, BCryptGenerateKeyPair, BCryptImportKeyPair, BCryptSignHash, BCryptVerifySignature |
| `d3d12_x.dll` | D3D12GetDebugInterface, D3D12SerializeVersionedRootSignature |
| `ext-ms-win-rtcore-ntuser-cursor-l1-1-0.dll` | ShowCursor |
| `ext-ms-win-rtcore-ntuser-message-l1-1-0.dll` | SetTimer |
| `ext-ms-win-rtcore-ntuser-window-l1-1-0.dll` | UnregisterClassW |
| `iphlpapi.dll` | NotifyAddrChange |
| `msvcp140.dll` | ??0?$codecvt@_WDU_Mbstatet@@@std@@QEAA@_K@Z, ??1?$codecvt@_WDU_Mbstatet@@@std@@MEAA@XZ, ??4?$_Yarn@D@std@@QEAAAEAV01@PEBD@Z, ?_Locimp_Addfac@_Locimp@locale@std@@CAXPEAV123@PEAVfacet@23@_K@Z, ?_New_Locimp@_Locimp@locale@std@@CAPEAV123@AEBV123@@Z, ?clog@std@@3V?$basic_ostream@DU?$char_traits@D@std@@@1 ... (11 total) |
| `xcurl.dll` | curl_easy_reset, curl_global_init_mem, curl_multi_setopt, curl_multi_wakeup, curl_version_info |
| `xg_x.dll` | XGComputeOptimalDepthStencilTileModes, XGComputeTexture2DLayout |
| `xmem.dll` | XMemGetWin32HeapStatistics |

## 2026-11-21 — Terraria (Unity IL2CPP, Xbox One GameCore) dump added

- Package `505GAMESS.P.A.84240166-5fcf-4ee1-b892-f5ff6a6094e1` 4.4.14.1081 (505 Games, TitleId `1738CA2A`, StoreId `BTNPS60N3114`).
- Runtime: Unity 2021.3.16f1 (`4016570cf34f`) GameCore player. The EXE embeds the PDB name `GameCoreXboxOnePlayerNoDevelopment_Master` from build folder `GameCoreXboxOne_VS2019_220604`. Scripting is IL2CPP, not XNA/FNA/.NET: no PE has a CLR header, the C# is AOT-compiled into `Data/Native/GameAssembly.dll` (49 MB), and `Data/Native/Data/Metadata/global-metadata.dat` sits beside it. Audio is FMOD (GameCoreXboxOne build, XAudio2 output, XMA codec).
- `MicrosoftGame.Config` (`configVersion="1"`) records no GDK version. It declares `TargetDeviceFamily="XboxOne"`. This is an **Xbox One** GameCore build that runs on Series hardware in Xbox One mode. `appxmanifest.xml` lists `Microsoft.GameOs` 10.0.18362.0. GDK edition **inferred** (not stated): June 2022, from the Unity build folder suffix `220604`, XCurl 2206.4.0.0, PartyWin 1.7.2206, PartyXboxLive 1.2.2207 and PlayFabMultiplayerGDK 1.4.2210.
- `Mount` modules: Terraria.exe (25,889,280 bytes), XCurl.dll, baselib.dll (Unity baselib), and the MSVC 14.28 CRT (concrt140, msvcp140, msvcp140_1, msvcp140_2, msvcp140_codecvt_ids, vccorlib140, vcruntime140, vcruntime140_1). `Mount/Data/Native`: GameAssembly.dll plus a second MSVC CRT set at 14.29 (adds msvcp140_atomic_wait). `Mount/Data/Plugins` (`nativePlugins.txt`): PartyWin.dll, PartyXboxLive.dll, PlayFabMultiplayerGDK.dll, TerrariaNative.dll and a second XCurl.dll. Unity data is `Data/data.unity3d`.
- Imports: Terraria.exe has 424 imports from 64 modules. All 26 binaries together have 3,496 import rows (1,106 distinct module+function pairs from 83 modules).
- `tools/progress/sync.py` now takes an optional per-title `binary_dirs`, so all three folders are scanned. Binaries outside `Mount` are keyed by relative path, e.g. `Data/Native/GameAssembly.dll`.
- XGameRuntime is not in any import table. Terraria.exe loads `xgameruntime.dll` by name and resolves `QueryApiImpl`, the statically linked GDK pattern.
- Classification (sync.py/gen.py method): 3,476 / 3,496 covered (99.43%). Counts: pc 2,513, title 877, hooked 74, forwarded 7, implemented 5, stub 4, missing 16. Static analysis only; the title has not been run.
- Console OS (EmbeddedXvd System32): build 10.0.22000.4429, 262 DLLs. It is **byte-identical** to the XDefiant dump (same DLL set, 0 files differ). So there are no new console DLLs or exports beyond what XDefiant added. Against Fortnite (10.0.19041.3948) it shows the same delta already logged below: 18 new export-bearing DLLs and 534 new exports in shared DLLs. The console library map stays at 250 libraries / 23,356 exports (before Trackmania).

### Missing or unhandled imports (all in Terraria.exe)

| module | functions | status | notes |
|---|---|---|---|
| `d3d12_x.dll` | D3D12XboxCreateDevice, D3D12SerializeRootSignature, D3D12XboxSetProcessDebugFlags, D3DMapEsramMemory | missing | Xbox One D3D12.X driver; the emulator only provides `d3d12_xs.dll`. `D3DMapEsramMemory` has no Series equivalent (ESRAM). Only Terraria imports it. |
| `xg_x.dll` | XGCreateTextureComputer, XGCreateTexture2DComputer, XGComputeOptimalTileMode | missing | Xbox One XG tiling library; emulator has `xg_xs.dll` (XGCreateTextureComputer stub) only. Only Terraria imports it. |
| `XFrontPanelDisplay.dll` | XFrontPanelIsSupported, XFrontPanelGetScreenDimensions, XFrontPanelPresentBuffer, XFrontPanelGetLightStates, XFrontPanelSetLightStates, XFrontPanelGetButtonStates | missing | Devkit front-panel display (Unity `GXDKFrontPanelDisplay`). Present in the console OS image (`System32\xfrontpaneldisplay.dll`) but not on PC, and the emulator has no stand-in; the static import fails until one is provided. Only Terraria imports it. |
| `AcpHal.dll` | AcpHalReleaseShapeContexts, ApuIsVirtualAddressValid, ApuMapVirtualAddress | missing | also imported by XDefiant |
| `AcpHal.dll` | AcpHalCreate, AcpHalAllocateShapeContexts, ApuFree | stub | shared with Fortnite and XDefiant |
| `xmem.dll` | XMemGetWorkingSetStatistics | stub | shared with Fortnite and XDefiant |

Runtime caveats from the emulator source:
- The `crates/compat/xcurl` crate is not counted. Its Cargo.toml points at `src/lib.rs`, which does not exist yet; only `sys.rs` is there. XCurl imports stay `title` because the title ships XCurl.dll.
- compat-common's XUser/XStore layer (`gdk_user.rs`) installs only when the host EXE has Fortnite's build timestamp, so Terraria gets no local user.
- The gamepad hook (`gamepad.rs`) patches only the host EXE's `GameInputCreate` import slot. `TerrariaNative.dll` imports `GameInput.dll` #1 itself and would not see the virtual pad.
- The network guard patches ws2_32/winhttp exports in place, so it covers PartyWin's and GameAssembly's direct socket and WinHTTP use (the 74 `hooked` rows).

### Modules imported by Terraria but not by Fortnite or XDefiant

| module | functions |
|---|---|
| `api-ms-win-core-file-l2-1-2.dll` | CopyFileW |
| `api-ms-win-core-kernel32-legacy-l1-1-0.dll` | GetComputerNameW |
| `api-ms-win-core-processthreads-l1-1-3.dll` | SetThreadDescription |
| `baselib.dll` | ?Baselib_DynamicLibrary_Close@il2cpp_baselib@@YAXUBaselib_DynamicLibrary_Handle@1@@Z, ?Baselib_DynamicLibrary_FromNativeHandle@il2cpp_baselib@@YA?AUBaselib_DynamicLibrary_Handle@1@_KIPEAUBaselib_ErrorState@1@@Z, ?Baselib_DynamicLibrary_GetFunction@il2cpp_baselib@@YAPEAXUBaselib_DynamicLibrary_Handle ... (11 total) |
| `d3d12_x.dll` | D3D12SerializeRootSignature, D3D12XboxCreateDevice, D3D12XboxSetProcessDebugFlags, D3DMapEsramMemory |
| `ext-ms-win-rtcore-ntuser-cursor-l1-1-0.dll` | LoadCursorW |
| `sspicli.dll` | AcceptSecurityContext, AcquireCredentialsHandleW, DecryptMessage, DeleteSecurityContext, EncryptMessage, FreeContextBuffer, FreeCredentialsHandle, InitializeSecurityContextW, QueryContextAttributesW |
| `xfrontpaneldisplay.dll` | XFrontPanelGetButtonStates, XFrontPanelGetLightStates, XFrontPanelGetScreenDimensions, XFrontPanelIsSupported, XFrontPanelPresentBuffer, XFrontPanelSetLightStates |
| `xg_x.dll` | XGComputeOptimalTileMode, XGCreateTexture2DComputer, XGCreateTextureComputer |

### Shared modules: functions Terraria uses that Fortnite and XDefiant do not

| module | new functions |
|---|---|
| `advapi32.dll` | CryptAcquireContextW, RegOpenKeyExW |
| `api-ms-win-core-com-l1-1-0.dll` | CoCreateFreeThreadedMarshaler, CoGetApartmentType, IIDFromString |
| `api-ms-win-core-file-l1-1-0.dll` | GetDiskFreeSpaceExW |
| `api-ms-win-core-heap-l2-1-0.dll` | LocalAlloc, LocalFree |
| `api-ms-win-core-interlocked-l1-1-0.dll` | InterlockedFlushSList |
| `api-ms-win-core-io-l1-1-0.dll` | CreateIoCompletionPort, GetQueuedCompletionStatus |
| `api-ms-win-core-processenvironment-l1-1-0.dll` | GetEnvironmentVariableA |
| `api-ms-win-core-processthreads-l1-1-0.dll` | TerminateThread |
| `api-ms-win-core-psapi-l1-1-0.dll` | K32GetProcessMemoryInfo |
| `api-ms-win-core-synch-l1-1-0.dll` | CreateWaitableTimerExW, SetWaitableTimer |
| `api-ms-win-core-synch-l1-2-0.dll` | WaitOnAddress, WakeByAddressAll, WakeByAddressSingle |
| `api-ms-win-core-synch-l1-2-1.dll` | CreateWaitableTimerW |
| `api-ms-win-crt-convert-l1-1-0.dll` | _ecvt_s, _i64toa_s, _i64tow_s, _ui64toa_s, _ui64tow_s, _wcstod_l |
| `api-ms-win-crt-filesystem-l1-1-0.dll` | _wstat64 |
| `api-ms-win-crt-heap-l1-1-0.dll` | _expand, _msize |
| `api-ms-win-crt-locale-l1-1-0.dll` | _free_locale |
| `api-ms-win-crt-math-l1-1-0.dll` | _fpclass, copysign, fmax, fmaxf, fminf, hypot, log2, lrintf, nearbyintf |
| `api-ms-win-crt-stdio-l1-1-0.dll` | __stdio_common_vsprintf_p, _dup, _dup2 |
| `api-ms-win-crt-string-l1-1-0.dll` | _isctype_l, _wcsicmp, isprint, strcat_s, towlower, wcstok |
| `api-ms-win-crt-time-l1-1-0.dll` | _mkgmtime64 |
| `bcrypt.dll` | BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptGetProperty, BCryptHashData, BCryptOpenAlgorithmProvider |
| `crypt32.dll` | CertAddEncodedCertificateToStore, CertAddStoreToCollection, CertCreateSelfSignCertificate, CertGetCertificateChain, CertVerifyCertificateChainPolicy, CertVerifyTimeValidity, CryptEncodeObjectEx, CryptHashCertificate2 |
| `ext-ms-win-rtcore-ntuser-message-l1-1-0.dll` | PostQuitMessage, SendMessageW |
| `ext-ms-win-rtcore-ntuser-window-l1-1-0.dll` | RegisterClassExW, SetWindowLongPtrW |
| `iphlpapi.dll` | GetAdaptersAddresses, GetIfEntry, GetNetworkParams |
| `kernel32.dll` | CancelIoEx, CopyFile2, CopyFileExW, CreateMutexW, FreeEnvironmentStringsW, GetComputerNameW, GetCurrentDirectoryW, GetEnvironmentStringsW, GetLogicalDrives, GetModuleFileNameA, GetOverlappedResultEx, GetThreadContext, GlobalAlloc, GlobalFree, K32GetProcessImageFileNameW, ReleaseMutex, ReplaceFileW,  ... (18 total) |
| `mfreadwrite.dll` | MFCreateSourceReaderFromURL |
| `msvcp140.dll` | ??0_Init_locks@std@@QEAA@XZ, ??1_Init_locks@std@@QEAA@XZ, ?_Rethrow_future_exception@std@@YAXVexception_ptr@1@@Z, ?_Throw_future_error@std@@YAXAEBVerror_code@1@@Z, ?_XGetLastError@std@@YAXXZ, ?__ExceptionPtrAssign@@YAXPEAXPEBX@Z, ?__ExceptionPtrCopyException@@YAXPEAXPEBX1@Z, ?__ExceptionPtrToBool@@Y ... (34 total) |
| `ole32.dll` | CoInitializeEx, StringFromGUID2 |
| `ws2_32.dll` | FreeAddrInfoExW, GetAddrInfoExOverlappedResult, GetAddrInfoExW, WSAGetOverlappedResult, WSAPoll, WSARecv, WSARecvFrom, WSASend, WSASendTo, WSASocketW, gethostbyaddr |
| `xcurl.dll` | curl_multi_wait |

## 2026-11-21 — XDefiant (Snowdrop) dump added

- Title: `Rogue_scarlett_rtx.exe` (97,231,360 bytes), 661 imports from 71 modules (Fortnite: 684 imports from 68 modules).
- Console OS (EmbeddedXvd System32): Fortnite dump 229 DLLs / 20,084 exports; XDefiant dump 249 DLLs / 20,746 exports (newer OS build).

### Modules imported by XDefiant but not by Fortnite

| module | functions |
|---|---|
| `advapi32.dll` | CryptAcquireContextA, CryptGenRandom, CryptReleaseContext, RegCloseKey, RegOpenKeyW, RegQueryValueExW |
| `api-ms-win-core-errorhandling-l1-1-1.dll` | AddVectoredExceptionHandler, RemoveVectoredExceptionHandler |
| `api-ms-win-core-fibers-l1-1-0.dll` | FlsAlloc, FlsFree, FlsSetValue |
| `api-ms-win-core-kernel32-legacy-l1-1-1.dll` | SetDllDirectoryW |
| `api-ms-win-core-kernel32-legacy-l1-1-5.dll` | GetDllDirectoryW |
| `api-ms-win-core-psapi-l1-1-0.dll` | K32EnumProcessModules, K32EnumProcessModulesEx, K32GetModuleInformation |
| `api-ms-win-core-synch-l1-2-1.dll` | CreateSemaphoreW |
| `api-ms-win-core-sysinfo-l1-2-0.dll` | GetNativeSystemInfo |
| `bink2scarlett.dll` | BinkClose, BinkDoFramePlane, BinkFreeGlobals, BinkGetFrameBuffersInfo, BinkGetKeyFrame, BinkGetRealtime, BinkGetTrackID, BinkGoto, BinkNextFrame, BinkOpen, BinkPause, BinkRegisterFrameBuffers, BinkSetFileOffset, BinkSetFrameRate, BinkSetMemory, BinkSetOSFileCallbacks, BinkSetSoundSystem, BinkSetSoun |
| `chad.dll` | XDspActivate, XDspConnect, XDspDeactivate, XDspDisconnect, XDspSubmitCommand |
| `ext-ms-win-rtcore-ntuser-window-ansi-l1-1-0.dll` | CreateWindowExA, GetWindowLongPtrA, RegisterClassExA, SetWindowLongPtrA |
| `kernel32.dll` | GetStartupInfoW, IsProcessorFeaturePresent, LoadLibraryExA, LocalFree, SleepConditionVariableSRW, UnhandledExceptionFilter, __C_specific_handler |
| `windows.media.mediacontrol.dll` | CreateRenderAudioStateMonitorForCategoryAndDeviceRole |
| `xapu.dll` | XApuConnect, XApuDequeueResult, XApuDisconnect, XApuEnqueueCommand, XApuGetResultCount |
| `xcurl.dll` | curl_easy_cleanup, curl_easy_getinfo, curl_easy_init, curl_easy_perform, curl_easy_setopt, curl_easy_strerror, curl_global_cleanup, curl_global_init, curl_multi_add_handle, curl_multi_cleanup, curl_multi_info_read, curl_multi_init, curl_multi_perform, curl_multi_poll, curl_multi_remove_handle, curl_ |

### Shared Xbox modules: functions XDefiant uses that Fortnite does not

| module | new functions |
|---|---|
| `acphal.dll` | AcpHalReleaseShapeContexts, ApuIsVirtualAddressValid, ApuMapApuAddress, ApuMapVirtualAddress |
| `api-ms-win-core-com-l1-1-0.dll` | CoInitializeEx |
| `api-ms-win-core-file-l1-1-0.dll` | CreateDirectoryA, CreateFileA, DeleteFileA, FindFirstFileExA, FindNextFileA, GetFileAttributesA, GetFileAttributesExA, GetFileAttributesW, GetFileSize, GetFullPathNameW, SetFileAttributesA, SetFilePointerEx |
| `api-ms-win-core-file-l2-1-0.dll` | GetFileInformationByHandleEx |
| `api-ms-win-core-interlocked-l1-1-0.dll` | InterlockedPopEntrySList, InterlockedPushEntrySList |
| `api-ms-win-core-libraryloader-l1-2-0.dll` | GetModuleFileNameA, GetModuleHandleExW, LoadLibraryExW |
| `api-ms-win-core-localization-l1-2-0.dll` | FormatMessageA |
| `api-ms-win-core-processthreads-l1-1-0.dll` | ExitProcess, GetThreadPriority |
| `api-ms-win-core-processthreads-l1-1-1.dll` | GetThreadIdealProcessorEx, SetThreadIdealProcessorEx |
| `api-ms-win-core-processtopology-obsolete-l1-1-0.dll` | SetProcessAffinityMask |
| `api-ms-win-core-synch-l1-1-0.dll` | CreateEventA, CreateSemaphoreExW, InitializeCriticalSectionEx, ReleaseSemaphore, SetCriticalSectionSpinCount, SleepEx, TryAcquireSRWLockExclusive, TryAcquireSRWLockShared, WaitForMultipleObjectsEx |
| `api-ms-win-core-synch-l1-2-0.dll` | InitializeConditionVariable, SleepConditionVariableCS, WakeAllConditionVariable |
| `api-ms-win-core-sysinfo-l1-1-0.dll` | GetTickCount, GetTickCount64 |
| `api-ms-win-core-windowserrorreporting-l1-1-0.dll` | WerUnregisterFile |
| `d3d12_xs.dll` | D3D12ConfigureVirtualMemoryX |
| `msvcp140.dll` | ??0_Locinfo@std@@QEAA@PEBD@Z, ??0facet@locale@std@@IEAA@_K@Z, ??1_Locinfo@std@@QEAA@XZ, ??1facet@locale@std@@MEAA@XZ, ??6?$basic_ostream@DU?$char_traits@D@std@@@std@@QEAAAEAV01@G@Z, ??6?$basic_ostream@DU?$char_traits@D@std@@@std@@QEAAAEAV01@K@Z, ??6?$basic_ostream@DU?$char_traits@D@std@@@std@@QEAAAE |
| `vcruntime140.dll` | set_unexpected |
| `winhttp.dll` | WinHttpCrackUrl, WinHttpWebSocketClose |
| `ws2_32.dll` | WSAAddressToStringA, gethostbyname, inet_ntoa, inet_ntop |
| `xg_xs.dll` | XGCreateTextureComputerFromDescriptor |

### Console DLLs present only in the newer OS (XDefiant dump)

| DLL | exports |
|---|---|
| `advapi32res.dll` | 0 |
| `bindfltapi.dll` | 11 |
| `chad.dll` | 5 |
| `container.dll` | 32 |
| `diagnosticdatasettings.dll` | 7 |
| `nlansp_c.dll` | 2 |
| `nrtapi.dll` | 5 |
| `ntfsres.dll` | 0 |
| `onesettingsclient.dll` | 13 |
| `ttdloader.dll` | 4 |
| `ttdplm.dll` | 5 |
| `ttdrecord.dll` | 1 |
| `ttdrecordcpu.dll` | 42 |
| `utcapi.dll` | 13 |
| `wc_storage.dll` | 8 |
| `wci.dll` | 14 |
| `xdxr.dll` | 1 |
| `xdxr_d.dll` | 1 |
| `xdxr_i.dll` | 1 |
| `xpal.dll` | 44 |

### Console DLLs whose export set grew (newer OS)

| DLL | Fortnite dump | XDefiant dump | added exports |
|---|---|---|---|
| `advapi32legacy.dll` | 193 | 207 | EnumServicesStatusA, EnumServicesStatusW, InitiateShutdownA, InitiateSystemShutdownW, LsaConfigureAutoLogonCredentials, LsaDisablePasswordLessCurrentUser, LsaDisableUserArso, LsaEnablePasswordLessCurr … |
| `aepic.dll` | 11 | 22 | #100, #101, #102, #103, #104, #105, #106, #107, #108, #109, GetPrivacyLevel |
| `audioses.dll` | 10 | 12 | #6, #7 |
| `bcd.dll` | 73 | 74 | BcdExportStoreEx |
| `cabapi.dll` | 13 | 14 | Cab_CheckForFile |
| `cfgmgr32.dll` | 278 | 279 | CM_Export_Pnp_StateW |
| `combase.dll` | 591 | 600 | #225, #226, #227, #228, #229, CoIsOle1Class, CoTreatAsClass, InternalOle1ClassFromCLSID2, InternalRegOpenClassKey |
| `d3d12_x.dll` | 53 | 54 | D3D12XboxUnloadDriverModule |
| `d3d12_xs.dll` | 52 | 55 | D3D12XboxUnloadDriverModule, D3D12ZeroMemoryX, GetFrameBufferPreserveMemorySizeBytes |
| `dab.dll` | 4 | 5 | DabNotifyPowerEvent, DabNotifySessionChange, DabNotifyTimeChange |
| `dbgeng.dll` | 6 | 15 | #354, #367, #386, #393, #417, #438, #441, #453, #462 |
| `dbghelp.dll` | 257 | 264 | SymGetDiaSource, SymGetSourceFileFromTokenByTokenName, SymGetSourceFileFromTokenByTokenNameW, SymGetSourceFileTokenByTokenName, SymGetSourceFileTokenByTokenNameW, SymRegisterGetSourcePathPartCallback, … |
| `dnsapi.dll` | 290 | 315 | DnsAddrArrayAddAddr, DnsAddrArrayAddIp4, DnsAddrArrayCreate, DnsAddrBuildFromDnsRecord, DnsApiOnNetworkChange, DnsDeleteSettings, DnsFreeCustomServers, DnsFreeWellKnownServerProperty, DnsFreeWellKnown … |
| `fwpuclnt.dll` | 276 | 278 | FwpmDynamicKeywordSubscribe0, FwpmDynamicKeywordUnsubscribe0, FwpmEventProviderFireNetEventEx0, FwpmNetEventCreateEnumHandleEx |
| `gameconfighelper.dll` | 22 | 28 | GetGameConfigForExePath, GetPackageFamilyNameFromConfig, GetPackageFullNameFromConfig, PopulateGameConfigInRegistry, PopulateGameConfigStringInRegistry, ReadGameConfigStream |
| `gamingtelemetry.dll` | 36 | 38 | TelemetryWriteCustomJsonEventWithGroupIdType, TelemetryWriteGamingEventWithGroupIdType |
| `gcxaudioext.dll` | 11 | 12 | SetXAudio2DebugConfiguration |
| `hix_xs.dll` | 4 | 8 | AddData, AddStreamingApiObject, DebugLive, SaveDeveloperBlobInfo |
| `httpapi.dll` | 46 | 49 | HttpCreateRequestQueueEx, HttpIsFeatureSupported, HttpSetRequestProperty |
| `imagehlp.dll` | 155 | 162 | SymGetDiaSource, SymGetSourceFileFromTokenByTokenName, SymGetSourceFileFromTokenByTokenNameW, SymGetSourceFileTokenByTokenName, SymGetSourceFileTokenByTokenNameW, SymRegisterGetSourcePathPartCallback, … |
| `iphlpapi.dll` | 296 | 307 | GetInterfaceActiveTimestampCapabilities, GetInterfaceSupportedTimestampCapabilities, InternalCreateOrRefIpForwardEntry2, InternalGetTcpDynamicPortRange, InternalGetUdp6Table2, InternalGetUdpDynamicPor … |
| `kernel32legacy.dll` | 299 | 303 | BackupSeek, IsSystemResumeAutomatic, RequestWakeupLatency, SetSystemPowerState |
| `kernelbase.dll` | 1897 | 1964 | AddPackageDependency, AllocateUserPhysicalPages2, AreShortNamesEnabled, ArmFeatureUsageSubscriberFlushNotification, BuildIoRingCancelRequest, BuildIoRingReadFile, BuildIoRingRegisterBuffers, BuildIoRi … |
| `lsaadt.dll` | 137 | 137 | LsapAdtAuditingEnabledByLogonId, LsapAdtAuditingEnabledBySubCategory, LsapAdtAuditingEnabledHint, LsapAdtInitParametersArray, LsapAdtRundownSecurityEventSource, LsapUnregisterAuditEvent |
| `lsasrv.dll` | 314 | 324 | IsTraceLevelEnabled, LsaIContextToHandleNoRef, LsaICopyToTokenInfoFromHandle, LsaIDereferenceCtxtHandle, LsaIExtractTargetInfo, LsaIFilterInboundNamespace, LsaIFreeFilterInboundNamespaceResult, LsaIGe … |
| `mmdevapi.dll` | 35 | 36 | #33 |
| `mobilenetworking.dll` | 9 | 81 | AcquireReadLock, AcquireWriteLock, AllocateAndCopyPointerData, AllocateMemory, BindNetworkInterfaceIdToAccountId, CheckRpcClientTokenMembership, ConvertByteArrayToWideStr, ConvertSDDLToValidSecurityDe … |
| `msdelta.dll` | 15 | 16 | ApplyDeltaGetReverseB |
| `msvcrt.dll` | 1317 | 1330 | ?_Doraise@bad_cast@@MEBAXXZ, ?_Doraise@bad_typeid@@MEBAXXZ, ?_inconsistency@@YAXXZ, ?_is_exception_typeof@@YAHAEBVtype_info@@PEAU_EXCEPTION_POINTERS@@@Z, ?name@type_info@@QEBAPEBDPEAU__type_info_node@ … |
| `mswsock.dll` | 63 | 64 | ProcessSocketNotifications |
| `ncrypt.dll` | 149 | 154 | SslDeriveSessionTicketKey, SslExpandNextGenTrafficKey, SslGetSessionTicketProtectionHeaderSize, SslProtectSessionTicket, SslUnprotectSessionTicket |
| `netprofm.dll` | 4 | 5 | InitHelperDll |
| `npmproxy.dll` | 5 | 7 | LANIdFreeCollection, LANIdRetrieveCollection |
| `ntdll.dll` | 2430 | 2468 | LdrHotPatchNotify, MicrosoftTelemetryAssertTriggeredUM, NtChangeProcessState, NtChangeThreadState, NtCreateIoRing, NtCreateProcessStateChange, NtCreateThreadStateChange, NtQueryIoRingCapabilities, NtQ … |
| `oleaut32.dll` | 427 | 429 | #503, #504 |
| `pixlivemerge.dll` | 6 | 7 | SetImageFileNameCacheUsage |
| `powrprof.dll` | 126 | 135 | PowerCleanupOverrides, PowerEnumerateSettings, PowerGetProfiles, PowerReadProfileAlias, PowerRefreshProfileSettings, PowerRestoreACProfileIndex, PowerRestoreDCProfileIndex, PowerWriteACProfileIndex, P … |
| `rpcrt4.dll` | 556 | 557 | RpcBindingInqMaxCalls |
| `samsrv.dll` | 328 | 332 | SampFlagsToAccountControlEx, SampQueryPolicyValue, SampSetCheckpointInfo, SampSetErrorInfo |
| `sechost.dll` | 217 | 235 | #1000, #1001, #1002, DecodeAttributeName, EncodeAttributeName, FreeOperandValue, GetCharFromDigit, GetDigitFromChar, GetOperandValue, GetOperatorCodeAtIndex, GetOperatorIndexByToken, GetOperatorUnaryA … |
| `shcore.dll` | 201 | 202 | #262, #263, #264, #265 |
| `srvcli.dll` | 61 | 62 | LocalServerCertificateMappingModify |
| `sspicli.dll` | 105 | 107 | LsaConnectLocalUser, LsaSetMachineCertificate |
| `tdh.dll` | 35 | 36 | TdhEnumerateProvidersForDecodingSource |
| `userenv.dll` | 78 | 79 | #219 |
| `wdbase.dll` | 27 | 28 | ?IsHelp@ParseArgs@@QEAA_NXZ, ?Parse@ParseArgs@@QEAA?AW4ParseResult@1@IPEAPEBG_N@Z |
| `wdscore.dll` | 95 | 99 | DecrementPartialMsgIndent, DestructPartialMsg, GetPartialMsgIndent, IncrementPartialMsgIndent |
| `webio.dll` | 36 | 40 | #46, #47, #48, #49 |
| `werxboxgc.dll` | 8 | 18 | XerEncryptDump, XerEnumerateReportFiles, XerEnumerateReportMetadata, XerGetMaxProcessHoldMilliseconds, XerProcessHeapDumpRequest, XerTransportEventBegin, XerTransportEventFinish, XerTransportEventUplo … |
| `winhttp.dll` | 69 | 74 | WinHttpFreeQueryConnectionGroupResult, WinHttpQueryConnectionGroup, WinHttpQueryHeadersEx, WinHttpReadDataEx, WinHttpSetSecureLegacyServersAppCompat |
| `wldp.dll` | 21 | 29 | WldpIsProductionConfiguration, WldpIsProductionConfigurationInProc, WldpIsWcosProductionConfiguration, WldpQueryDeviceSecurityInformation, WldpQueryPolicySettingEnabled, WldpQueryPolicySettingEnabled2 … |
| `ws2_32.dll` | 195 | 196 | ProcessSocketNotifications |
| `xbsc_xs.dll` | 39 | 41 | XBSC_CreatePixDwarfApi, XBSC_DisassembleHwPipelineWithCallback |
| `xbthunk.dll` | 9 | 11 | XbSetCallHeapFreeCallbackForRealloc, XbSetWinHttpHooks |
| `xbtp.dll` | 131 | 133 | XbtpLinkHandleControl, XbtpMarkToolingCritical |
| `xcrdapi.dll` | 109 | 129 | DStorageGetMappedControlPage, XCrdCopyExtendedAttributes, XCrdCreateXvdPointer, XCrdGetDeviceIdMapping, XCrdGetStreamingRoamableXvdConsoleMode, XCrdGetUabBitmapOffset, XCrdGetXvcOffsetFromHeader, XCrd … |
| `xg_xs.dll` | 19 | 23 | XGCreateBVHComputer, XGInitializeLibrary, XGTerminateLibrary, XGValidateDescriptor |
| `xgameruntime.dll` | 5 | 6 | DllCanUnloadNow |
| `xgs12_10.dll` | 18 | 21 | CpuMemAlloc, CpuMemFree, GMemSetAllocationHooks |
| `xgs12_10d.dll` | 18 | 21 | CpuMemAlloc, CpuMemFree, GMemSetAllocationHooks |
| `xgs12_10i.dll` | 18 | 21 | CpuMemAlloc, CpuMemFree, GMemSetAllocationHooks |
| `xmem.dll` | 28 | 36 | XMemGetWin32HeapName, XMemGetWriteWatch, XMemHistoryData, XMemRegisterGraphicsMemoryConfiguration, XMemResetWriteWatch, XMemResumeWriteWatch, XMemSetWin32HeapName, XMemSetWinHttpHooks |
