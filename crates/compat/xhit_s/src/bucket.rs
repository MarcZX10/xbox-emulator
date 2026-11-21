fn flags(b: u32, parts: &[(u32, &str)]) -> String {
    parts.iter().map(|&(n, s)| if (b >> n) & 1 != 0 { s } else { "" }).collect()
}

fn with(base: &str, b: u32, parts: &[(u32, &str)]) -> String {
    format!("{}{}", base, flags(b, parts))
}

const P01: [(u32, &str); 2] = [(0, "_p0"), (1, "_p1")];
const PIPES: [(u32, &str); 2] = [(0, "_Pipe0"), (1, "_Pipe1")];

pub fn idle(b: u32) -> String {
    if b & 3 == 0 {
        return match b {
            0 => "HangBucketType_Idle".into(),
            0x1000 => "HangBucketType_Idle_Sra_EraPresent".into(),
            0x2000 => "HangBucketType_Idle_Sra_Only".into(),
            0x3000 => "HangBucketType_Idle_Legacy".into(),
            _ => format!("HangBucket_TotallyIdleHung_Unknown0x{:08x}", b),
        };
    }
    let base = if b & 1 != 0 { "HangBucketType_Idle_EraInit" } else { "HangBucketType_Idle_EraOther" };
    let src = b & 0xf0;
    match src {
        0x00 => base.to_string(),
        0x10 => format!("{}_UMD_Unspecified", base),
        0x20 => format!("{}_Title", base),
        0x30 => format!("{}_KMD_ProcessTermination", base),
        0x40 => format!("{}_KMD_ProcessTermination_AsyncComputeHung", base),
        0x50 => format!("{}_KMD_FatalError", base),
        0x60 => format!("{}_UMD_Present", base),
        0x70 => format!("{}_UMD_FenceWait", base),
        0x80 => format!("{}_UMD_DeviceRemoved", base),
        0x90 => format!("{}_UMD_FailedInitialization", base),
        _ => format!("{}_UnknownSource_0x{:04x}", base, src),
    }
}

pub fn cpg_busy(b: u32) -> String {
    let s = match b {
        0x3002_0000 => "HangBucketType_CpgBusyGeneric",
        0x3002_0001 => "HangBucketType_CpgBusy_NoReason_Legacy",
        0x3002_0002 => "HangBucketType_CpgBusy_NoReason_SraHang",
        0x3002_0003 => "HangBucketType_CpgBusy_NoReason_EraHang",
        0x3002_1001 => "HangBucketType_CpgBusy_Corruption_Ce0",
        0x3002_1002 => "HangBucketType_CpgBusy_Corruption_Ce1",
        0x3002_1004 => "HangBucketType_CpgBusy_Corruption_Pfp0",
        0x3002_1008 => "HangBucketType_CpgBusy_Corruption_Pfp1",
        0x3002_1010 => "HangBucketType_CpgBusy_Corruption_Me0",
        0x3002_1020 => "HangBucketType_CpgBusy_Corruption_Me1",
        0x3002_2000 => "HangBucketType_CpgBusy_CeDe_QuickDraw",
        0x3002_2001 => "HangBucketType_CpgBusy_CeDe_QuickDraw2",
        0x3002_3000 => "HangBucketType_CpgBusy_DoublePush",
        _ => return format!("HangBucketType_CpgBusy_Unknown0x{:08x}", b),
    };
    s.to_string()
}

pub fn cpc_bad_packet(b: u32) -> String {
    let src = ["???", "Normal", "IbEndToEopq", "CpgPacketInCpc"];
    let loc = ["CpgPacketInCpc", "PQ", "IB", "IQ", "EOPQ", "ADC", "", ""];
    let i = b as i32;
    format!("HangBucketType_Cpc_BadPacket_{}_{}_Pipe{}_Queue{}", src[((i >> 12) & 3) as usize], loc[((i >> 8) & 7) as usize], (i >> 4) & 3, b & 0xf)
}

pub fn progressing(base: &str, b: u32) -> String {
    with(base, b, &[(0, "_ps0"), (1, "_ps1"), (2, "_vs0"), (3, "_vs1"), (4, "_gs0"), (5, "_gs1"), (6, "_es0"), (7, "_es1"), (8, "_hs0"), (9, "_hs1"), (10, "_ls0"), (11, "_ls1"), (12, "_csg"), (13, "_AsyncEra"), (14, "_AsyncNui"), (15, "_AsyncSra"), (16, "_AsyncUnknown")])
}

const MEC_P: [(u32, &str); 4] = [(4, "_MecP0"), (5, "_MecP1"), (6, "_MecP2"), (7, "_MecP3")];

pub fn name(b: u32) -> Option<String> {
    let class = b & 0xfffe_0000;
    let s: String = match class {
        0 => idle(b),
        0x0100_0000 => with("HangBucketType_SomethingInSrbmBusy", b, &[(0, "_Sdma0"), (1, "_Sdma1"), (2, "_Sdma2"), (3, "_Sdma3"), (4, "_Uvd"), (5, "_Vce"), (6, "_Sam")]),
        0x0200_0000 => "HangBucketType_NonStandardThingsAreBusy".into(),
        0x0302_0000 => "HangBucketType_EraMeWaitingForVblank".into(),
        0x0304_0000 => "HangBucketType_EraMeWaitingForScheduler".into(),
        0x0306_0000 => "HangBucketType_SraMeWaitingForScheduler".into(),
        0x0400_0000 => with("HangBucketType_McHubDeadlock", b, &[(0, "_WriteDeadlock"), (1, "_ReadDeadlock")]),
        0x3002_0000 => cpg_busy(b),
        0x3004_0000 => {
            if b == 0x3004_0000 {
                "HangBucketType_CpcBusyGeneric".into()
            } else {
                cpc_bad_packet(b)
            }
        }
        0x3006_0000 => "HangBucketType_CpcAndCpgBusyGeneric".into(),
        0x3008_0000 => match b {
            0x3008_0001 => "HangBucketType_IdleButCpfIsBusyForNoReason".into(),
            0x3008_0002 => "HangBucketType_CpfBusyPostWptr".into(),
            0x3008_0003 => "HangBucketType_CpfTimerBusy".into(),
            0x3008_0004 => "HangBucketType_CpfBusyUnknownReason".into(),
            _ => format!("CPF-related hang, unknown code 0x{:08x}\n", b),
        },
        0x300a_0000 => with("HangBucketType_WaitRegMem", b, &[(0, "_Ce0"), (1, "_Pfp0"), (2, "_Me0"), (3, "_Ce1"), (4, "_Pfp1"), (5, "_Me1"), (6, "_Sdma0"), (7, "_Sdma1"), (8, "_Sdma2"), (9, "_Sdma3"), (10, "_AsyncEraP0"), (11, "_AsyncEraP1"), (12, "_AsyncEraP2"), (13, "_AsyncSra"), (14, "_AsyncNui")]),
        0x3012_0000 => with("HangBucketType_RciuPriv", b, &[P01[0], P01[1], MEC_P[0], MEC_P[1], MEC_P[2], MEC_P[3]]),
        0x3016_0000 => with("HangBucketType_ReservedBit", b, &[(0, "_pfp0"), (1, "_pfp1"), (2, "_ce0"), (3, "_ce1"), MEC_P[0], MEC_P[1], MEC_P[2], MEC_P[3], (12, "_Unexplained")]),
        0x3018_0000 => with("HangBucketType_ExecViolation", b, &[(0, "_CpEra"), (1, "_CpSra"), (2, "_CpNui"), (3, "_CpOther"), (4, "_Sdma0Era"), (5, "_Sdma0Sra"), (6, "_Sdma1Era"), (7, "_Sdma1Sra"), (8, "_Sdma2Era"), (9, "_Sdma2Sra"), (10, "_Sdma3Era"), (11, "_Sdma3Sra")]),
        0x301a_0000 => with("HangBucketType_SurfaceSyncTc", b, &[P01[0], P01[1], (2, "_MecP0"), (3, "_MecP1"), (4, "_MecP2"), (5, "_MecP3")]),
        0x301c_0000 => flags(b, &[(0, "HangBucketType_CafeBabe_pfpIb1(p0)"), (1, "HangBucketType_CafeBabe_pfpIb1(p1)"), (2, "HangBucketType_CafeBabe_ceIb1(p0)"), (3, "HangBucketType_CafeBabe_ceIb1(p1)"), (4, "HangBucketType_CafeBabe_MecP0"), (5, "HangBucketType_CafeBabe_MecP1"), (6, "HangBucketType_CafeBabe_MecP2"), (7, "HangBucketType_CafeBabe_MecP3"), (8, "HangBucketType_DeadBeef_pfpIb2(p0)"), (9, "HangBucketType_DeadBeef_pfpIb2(p1)"), (10, "HangBucketType_DeadBeef_ceIb2(p0)"), (11, "HangBucketType_DeadBeef_ceIb2(p1)")]),
        0x301e_0000 => with("HangBucketType_SetPredication_LikelyCorruption", b, &PIPES),
        0x3100_0000 => format!("HangBucketType_SomethingBeyondCp{}{}", flags(b, &[(11, "_Ta")]), flags(b, &[(0, "_Wd"), (1, "_Ia"), (2, "_Vgt"), (3, "_Spi"), (4, "_Pa"), (5, "_Sc"), (6, "_Db"), (7, "_Cb"), (8, "_Sx"), (9, "_Bci"), (10, "_Coher")])),
        0x3102_0000 => with("HangBucketType_DroppedEvent", b, &[(0, "IncDe_P0"), (1, "IncDe_P1"), (2, "SwitchBuf_P0"), (3, "SwitchBuf_P1")]),
        0x3104_0000 => with("HangBucketType_StuckInWd", b, &[PIPES[0], PIPES[1], (2, "_OutOfRange0"), (3, "_OutOfRange1")]),
        0x3106_0000 => with("HangBucketType_BadDrawInitiator", b, &PIPES),
        0x4002_0000 => with("HangBucketType_BadInstancedDraw", b, &PIPES),
        0x4004_0000 => progressing("HangBucketType_StillProgressing", b),
        0x4006_0000 => progressing("HangBucketType_ShaderLooping", b),
        0x4008_0000 => with("HangBucketType_SurfaceSyncPhase2", b, &[(0, "_Cb"), (1, "_Db"), (2, "_Tc"), (3, "_Moving")]),
        0x400a_0000 => "HangBucketType_NotEop".into(),
        0x400c_0000 => with("HangBucketType_MissingScratch", b, &[(0, "_Gfx0"), (1, "_Gfx1"), (2, "_GfxCompute0"), (3, "_GfxCompute1"), (4, "_Async0"), (5, "_Async1"), (6, "_Async2"), (7, "_Async3")]),
        0x400e_0000 => with("HangBucketType_BadSurfaceSyncDb", b, &P01),
        0x4010_0000 => "HangBucketType_VgtHsStuck".into(),
        0x5000_0000 => progressing("HangBucketType_ShadersValid_Unknown", b),
        0x5002_0000 => progressing("HangBucketType_Shader_StuckOnBarrier", b),
        0x5004_0000 => progressing("HangBucketType_Shader_DeadbeefSaneAddr", b),
        0x5006_0000 => progressing("HangBucketType_Shader_DeadbeefInsaneAddr", b),
        0x5008_0000 => progressing("HangBucketType_Shader_BadInstr", b),
        0x500a_0000 => progressing("HangBucketType_Shader_StuckOnFetch", b),
        0x500c_0000 => progressing("HangBucketType_Shader_StuckOnWeird", b),
        0x500e_0000 => progressing("HangBucketType_Shader_MovingAndStuck", b),
        0x5010_0000 => progressing("HangBucketType_Shader_AllStuck", b),
        0x5012_0000 => progressing("HangBucketType_Shader_StuckOnExportGarbage  ", b),
        0x5014_0000 => progressing("HangBucketType_Shader_StuckOnExportWrongType", b),
        0x5100_0000 => match b - 0x5100_0000 {
            0 => "HangBucketType_Shader_LateAllocVs".into(),
            1 => "HangBucketType_Shader_LateAllocVs_p1".into(),
            2 => "HangBucketType_OnChipGSNeedsCu0ReservedForPs".into(),
            3 => "HangBucketType_OnChipGSNeedsCu0ReservedForPs_p1".into(),
            4 => "HangBucketType_NeedVsPartialFlushBeforeOnChipGS".into(),
            _ => format!("HangBucketType_LateAllocVs_(internal error 0x{:08x})", b),
        },
        0x5104_0000 => "HangBucketType_VsPsParameterPipelineHung".into(),
        0x6002_0000 => with("HangBucketType_GprWrapBug", b, &[(16, "AsyncCompute"), (8, "HS_Title"), (9, "HS_System"), (12, "GraphicsCompute")]),
        0x6004_0000 => "HangBucketType_DbMissingWrite".into(),
        0x6006_0000 => match b {
            0x6006_0001 => "HangBucketType_CbCmaskCorruption_Standard".into(),
            0x6006_0002 => "HangBucketType_CbCmaskCorruption_BlendBypass".into(),
            0x6006_0003 => "HangBucketType_CbCmaskCorruption_Standard2".into(),
            _ => format!("HangBucketType_CbCmaskCorruption_0x{:05x}", b & 0x1ffff),
        },
        0xf000_0000 => "HangBucketType_XhitFileIsCorrupted".into(),
        _ => format!("Unknown_0x{:08x}", b),
    };
    Some(s)
}
