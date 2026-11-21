use crate::{banner, print};

pub const REPORT_SIZE: usize = 0x328;
pub const R_FLAGS: usize = 0;
pub const R_WAIT_VALID: usize = 0x10;
pub const R_WAITS: usize = 0x18;
pub const R_WAIT_OBJS: usize = 0x258;
pub const R_FENCE_TABLE: usize = 0x318;
pub const R_BUCKET: usize = 0x320;
pub const WAIT_STRIDE: usize = 0x18;
pub const FENCE_ENTRIES: usize = 0x40;
pub const FENCE_STRIDE: usize = 0x40;
pub const F_CORRUPTION: u64 = 1 << 15;
pub const F_IA_ISSUING: u64 = 1 << 16;
pub const F_CMASK: u64 = 1 << 17;
pub const F_SHADER_SPIN: u64 = 1 << 18;
pub const F_NO_SCRATCH: u64 = 1 << 19;
pub const W_VALID: u64 = 1 << 40;
pub const W_MEMORY: u64 = 1 << 41;
pub const W_64BIT: u64 = 1 << 42;
pub const W_EXEC_SIGNAL: u64 = 1 << 46;
pub const VA_MASK: u64 = 0xff_ffff_ffff;
pub const PRESENT_SYNC_BASE: u64 = 0xc0_0010;
pub const BUCKET_IA_ISSUING: u32 = 0x4002_0000;
pub const BUCKET_NO_SCRATCH: u32 = 0x5010_0000;

pub const WAIT_ORDER: [usize; 17] = [0x18, 0x30, 0x48, 0x60, 0x78, 0x90, 0xa8, 0xc0, 0x1b0, 0x1c8, 0x1e0, 0x1f8, 0x210, 0x228, 0x108, 0x120, 0x138];

pub const ENGINES: [&str; 17] = [
    "Title command processor ME",
    "System command processor ME",
    "Title command processor PFP",
    "System command processor PFP",
    "SDMA 0",
    "SDMA 1",
    "SDMA 2",
    "SDMA 3",
    "Async compute pipe 0 queue 0",
    "Async compute pipe 0 queue 1",
    "Async compute pipe 0 queue 2",
    "Async compute pipe 0 queue 3",
    "Async compute pipe 0 queue 4",
    "Async compute pipe 0 queue 5",
    "Async compute pipe 1 queue 0",
    "Async compute pipe 1 queue 1",
    "Async compute pipe 1 queue 2",
];

pub const COMPARES: [&str; 8] = ["<always>", "<", "<=", "==", "!=", ">=", ">", "(null)"];

const MSG_FILE: &str = "The GPU hang dump data was written to file \"{}\".\n";
const MSG_DEVKIT: &str = "The GPU hang dump data was also written to development kit file \"{}\".\n";
const MSG_PIX: &str = "You can load an XHIT file into PIX or the XHIT command line tool to do deeper postmortem analysis.\nNote that the ID3D11DeviceX::SetHangCallbacks and ID3D12Device::SetHangCallbacksX APIs\ncan be used to automate collection of GPU hang data.\n\n";
const MSG_CORRUPTION: &str = "The GPU hang appears to have been caused by command buffer corruption. Common\ncauses include:\n- multiple game threads sharing the same D3D context without proper\n  synchronization\n- incorrect memory reuse with custom XMemAlloc allocators and deferred contexts\n\n";
const MSG_CMASK: &str = "One of the CB blocks is busy, with only its CMask sub-block busy.\nThis is often a sign of pixel shader corruption, or of bad pixel shader output\npatching.\n\n";
const MSG_IA: &str = "The IA is still successfully issuing new indices / instances.\nThis almost always means an indirect draw's draw descriptors are corrupted,\nwith an abnormally high instance & index count. Please check for missing cache\nflushes, bad addresses, and missing synchronization.\n\n";
const MSG_DEADLOCK: &str = "A fence deadlock was detected. Examine the fence deadlock table above to\ndetermine which engines are deadlocked. To help avoid deadlocks, ensure that\ncalls to InsertFence always kick off (use a Flags value of 0).\n\n";
const MSG_SCRATCH: &str = "All shaders are stuck and shader scratch memory has been disabled via device\ncreation parameters. It's likely that a shader needs to use scratch memory\nbut none has been configured. Use a debug build of the driver to determine\nwhich shader is using scratch memory, and either fix that shader or enable\nscratch memory.\n\n";
const MSG_SPIN: &str = "The sequencer indicates that a shader was spinning in an infinite loop.\nThe Context Location display can help locate the responsible shader.\nEnsure that input data to the shader is not corrupted.\n\n";
const MSG_HOPE: &str = "We hope this information helps you to investigate and fix the issue.\nIf investigation is unsuccessful, please contact Xbox Developer\nSupport (xhitsupport@microsoft.com) for further assistance.\n\n";
const MSG_SUPPORT: &str = "This GPU hang should be looked at by Xbox Developer Support. Please send the\nXHIT file and any relevant details to xhitsupport@microsoft.com.\n\n";
const MSG_NO_DETAIL: &str = "The fence location has no additional detail, because the XHIT session either originated from\na D3D11 driver, or the fence debug info table was not captured at the time of the\ncrash. Only the D3D12 debug and instrumented drivers provide fence debug information.\n";
const MSG_NO_MATCH: &str = "The fence writeback address does not correspond with any ID3D12Fence objects or\nregistered custom fence locations. Custom fence locations can be labeled with the\nID3D12Device::RegisterCustomFenceLocationX API.\n";

unsafe fn q(p: *const u8, off: usize) -> u64 {
    (p.add(off) as *const u64).read_unaligned()
}

unsafe fn d(p: *const u8, off: usize) -> u32 {
    (p.add(off) as *const u32).read_unaligned()
}

pub unsafe fn wide_str(p: *const u16) -> String {
    let mut n = 0usize;
    while *p.add(n) != 0 {
        n += 1;
    }
    String::from_utf16_lossy(core::slice::from_raw_parts(p, n))
}

pub unsafe fn fence_detail(report: *const u8, va: u64) {
    if va == 0 || report.is_null() {
        return;
    }
    if va.wrapping_sub(PRESENT_SYNC_BASE) <= 0x83 {
        print(0, &format!("The fence memory location 0x{:010x} corresponds to the present queue sync object {}.\nCheck for improper resource transition barriers on swap chain resources.\nPresent queue sync waits can also be a GPU hang false positive caused by failure to progress\non the CPU due to debugger activity or a CPU deadlock in the title.\n", va, (va.wrapping_sub(PRESENT_SYNC_BASE) as u32) >> 2));
        return;
    }
    let table = q(report, R_FENCE_TABLE) as *const u8;
    if table.is_null() {
        print(0, MSG_NO_DETAIL);
        return;
    }
    for i in 0..FENCE_ENTRIES {
        let e = table.add(i * FENCE_STRIDE);
        if q(e, 8) != va {
            continue;
        }
        let mut name = wide_str(e.add(0x10) as *const u16);
        if name.is_empty() {
            name = "<untitled>".to_string();
        }
        let obj = q(e, 0);
        if obj != 0 {
            print(0, &format!("The fence memory location 0x{:010x} corresponds to the ID3D12Fence object\n(0x{:016X} \"{}\").\n", va, obj, name));
        } else {
            print(0, &format!("The fence memory location 0x{:010x} corresponds to a custom fence location\nnamed \"{}\".\n", va, name));
        }
        return;
    }
    print(0, MSG_NO_MATCH);
}

pub fn hang_type_name(bucket: u32) -> Option<String> {
    crate::bucket::name(bucket)
}

pub unsafe fn high_level_summary(report: *const u8, file: Option<&str>, devkit_file: Option<&str>) {
    let mut c = [0u8; REPORT_SIZE];
    core::ptr::copy_nonoverlapping(report, c.as_mut_ptr(), REPORT_SIZE);
    let cp = c.as_ptr();
    banner("XHIT GPU Hang High Level Summary");
    let bucket = d(cp, R_BUCKET);
    let class = bucket & 0xfffe_0000;
    let mut idle = false;
    let mut known = false;
    let mut flags = q(cp, R_FLAGS);
    if class == 0 {
        idle = true;
    } else if class == BUCKET_IA_ISSUING {
        flags |= F_IA_ISSUING;
    } else {
        let n = hang_type_name(bucket).unwrap_or_default();
        print(0, &format!("Autocategorized hang type: {} (0x{:08x})\n", n, bucket));
    }
    if let Some(f) = file {
        print(0, &MSG_FILE.replace("{}", f));
        if let Some(k) = devkit_file {
            print(0, &MSG_DEVKIT.replace("{}", k));
        }
        print(0, MSG_PIX);
    }
    if flags & F_CORRUPTION != 0 {
        idle = false;
        print(0, MSG_CORRUPTION);
        known = true;
    }
    if flags & F_CMASK != 0 {
        idle = false;
        print(0, MSG_CMASK);
    }
    if flags & F_IA_ISSUING != 0 {
        idle = false;
        print(0, MSG_IA);
        known = true;
    }
    if flags & crate::DEADLOCK_BIT != 0 {
        idle = false;
        print(0, MSG_DEADLOCK);
        known = true;
    }
    if flags & F_NO_SCRATCH != 0 && class == BUCKET_NO_SCRATCH {
        idle = false;
        print(0, MSG_SCRATCH);
        known = true;
    }
    if c[R_WAIT_VALID] & 1 != 0 {
        let fence_class = class.wrapping_sub(0x3002_0000) & 0xfff7_ffff == 0;
        idle = false;
        for (i, &off) in WAIT_ORDER.iter().enumerate() {
            let w = q(cp, off);
            if w & W_VALID == 0 {
                continue;
            }
            let name = ENGINES[i];
            let va = w & VA_MASK;
            if va == 0 {
                print(0, &format!("The {} engine was waiting in an infinite loop for a memory location or\nregister to change value. No additional information is available.\nThis could be caused by a fence deadlock, memory corruption, usage of the\nInsertWaitOnMemory/Wait64BitValueX/Wait32BitValueX APIs, or a driver bug.\n\n", name));
                continue;
            }
            let wide = w & W_64BIT != 0;
            let lo = d(cp, off + 8) as u64;
            let hi = d(cp, off + 0xc) as u64;
            let mask = if wide { u64::MAX } else { lo };
            let reference = if wide { (lo << 32) | hi } else { hi };
            let cmp = COMPARES[((w >> 43) & 7) as usize];
            if w & W_MEMORY != 0 {
                let cur = q(cp, off + 0x10);
                print(0, &format!("The {} engine was waiting in an infinite loop for a memory location to\nchange value. The location is 0x{:010x}, and the wait was until the {}-bit value\nat that location was {} the reference value (0x{:x})\nAND the mask (0x{:x}).\nThe current value at that memory location is 0x{:x}.\nThis could be caused by a fence deadlock, memory corruption, usage of the\nInsertWaitOnMemory/Wait64BitValueX/Wait32BitValueX APIs, or a driver bug.\n\n", name, va, if wide { 64 } else { 32 }, cmp, reference, mask, cur));
                let idx = (off - R_WAITS) / WAIT_STRIDE;
                let obj = if idx < 0x18 { q(cp, R_WAIT_OBJS + idx * 8) } else { 0 };
                if obj != 0 {
                    if w & W_EXEC_SIGNAL != 0 {
                        print(0, &format!("The fence memory location 0x{:010x} corresponds to the execution signal writeback\nwithin command list API object 0x{:016X}.\nThis could be caused by problems with asynchronous command list execution,\nor improper reuse of command list allocators.\n", va, obj));
                    } else {
                        print(0, &format!("The fence memory location 0x{:010x} corresponds to the command list fence writeback\nwithin command list API object 0x{:016X}.\nThis could be caused by problems with resource barrier GPU pipeline synchronization,\nor improper reuse of command list allocators.\n", va, obj));
                    }
                } else {
                    fence_detail(report, va);
                }
                known = fence_class;
            } else {
                print(0, &format!("The {} engine was waiting in an infinite loop for a register to\nchange value. The register offset is 0x{:010x}, and the wait was until the value\nin that register was {} the reference value (0x{:x})\nAND the mask (0x{:x}).\nThis was probably caused by a driver bug.\n\n", name, va, cmp, reference, mask));
            }
        }
    }
    if flags & F_SHADER_SPIN != 0 {
        idle = false;
        print(0, MSG_SPIN);
        known = true;
    }
    banner("Next Steps");
    if known {
        print(0, MSG_HOPE);
    } else if !idle {
        print(0, MSG_SUPPORT);
    } else {
        print(0, "The GPU appears to be idle.\nIf you believe the GPU is actually hung by your title, please send the XHIT file to\nXbox Developer Support (xhitsupport@microsoft.com) for additional analysis.\n\n");
    }
}
