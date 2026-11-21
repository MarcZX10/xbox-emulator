# Library discovery log

Console and title libraries found while mapping the Xbox Series system surface. Every dump stays read-only.

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
