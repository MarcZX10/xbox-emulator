pub const TAG_PIPELINE_OUTPUT: u32 = 6;
pub const TAG_ROOT_SIGNATURE: u32 = 7;
pub const TAG_SERIALIZED_OUTPUT: u32 = 8;
pub const TAG_DESERIALIZED_OUTPUT: u32 = 9;
pub const TAG_SERIALIZED_SRC: u32 = 0xc;
pub const TAG_DESERIALIZED_SRC: u32 = 0xd;
pub const TAG_DISASSEMBLY: u32 = 0xe;
pub const TAG_ASSEMBLY: u32 = 0xf;

unsafe fn rd<T: Copy>(h: *const u8, off: usize) -> T {
    (h.add(off) as *const T).read_unaligned()
}

unsafe fn tag(h: *const u8) -> u32 {
    rd::<u32>(h, 0x18)
}

#[no_mangle]
pub unsafe extern "system" fn XBSC_GetPipelineCompileOutput(h: *const u8) -> *const u8 {
    if h.is_null() || tag(h) != TAG_PIPELINE_OUTPUT {
        return core::ptr::null();
    }
    rd(h, 0x28)
}

#[repr(C)]
pub struct CompiledRootSignature {
    pub a: u64,
    pub b: u64,
    pub c: u32,
}

#[no_mangle]
pub unsafe extern "system" fn XBSC_GetCompiledRootSignature(out: *mut CompiledRootSignature, h: *const u8) -> u32 {
    if h.is_null() || tag(h) != TAG_ROOT_SIGNATURE {
        return 1;
    }
    (*out).a = rd(h, 0x20);
    (*out).b = rd(h, 0x28);
    (*out).c = rd(h, 0x30);
    0
}

unsafe fn object(h: *const u8, want: u32, out: *mut *const u8) -> u32 {
    if out.is_null() || h.is_null() || tag(h) != want {
        return 1;
    }
    let p: *const u8 = rd(h, 0x28);
    if p.is_null() {
        return 1;
    }
    *out = p;
    0
}

#[no_mangle]
pub unsafe extern "system" fn XBSC_GetDeserializedPipelineCompileOutput(h: *const u8, out: *mut *const u8) -> u32 {
    object(h, TAG_DESERIALIZED_OUTPUT, out)
}

#[no_mangle]
pub unsafe extern "system" fn XBSC_GetDeserializedSrcPipeline(h: *const u8, out: *mut *const u8) -> u32 {
    object(h, TAG_DESERIALIZED_SRC, out)
}

unsafe fn serialized(h: *const u8, want: u32, data: *mut *const u8, size: *mut usize) -> u32 {
    if data.is_null() || size.is_null() || h.is_null() || tag(h) != want {
        return 1;
    }
    let p: *const u8 = rd(h, 0x28);
    if p.is_null() {
        return 1;
    }
    *data = p;
    *size = rd::<u32>(p, 8) as usize + 0x10;
    0
}

#[no_mangle]
pub unsafe extern "system" fn XBSC_GetSerializedPipelineCompileOutput(h: *const u8, data: *mut *const u8, size: *mut usize) -> u32 {
    serialized(h, TAG_SERIALIZED_OUTPUT, data, size)
}

#[no_mangle]
pub unsafe extern "system" fn XBSC_GetSerializedSrcPipeline(h: *const u8, data: *mut *const u8, size: *mut usize) -> u32 {
    serialized(h, TAG_SERIALIZED_SRC, data, size)
}

#[no_mangle]
pub unsafe extern "system" fn XBSC_GetDisassembly(h: *const u8, text: *mut *const u8, len: *mut usize) -> u32 {
    if h.is_null() {
        return 1;
    }
    let code: u32 = rd(h, 8);
    if code != 0 || tag(h) != TAG_DISASSEMBLY {
        return code;
    }
    let cap: usize = rd(h, 0x38);
    *text = if cap >= 0x10 { rd(h, 0x20) } else { h.add(0x20) };
    *len = rd(h, 0x30);
    0
}

#[no_mangle]
pub unsafe extern "system" fn XBSC_GetAssembledShader(h: *const u8, out: *mut *const u8) -> u32 {
    if h.is_null() {
        return 1;
    }
    let code: u32 = rd(h, 8);
    if code != 0 || tag(h) != TAG_ASSEMBLY || out.is_null() {
        return code;
    }
    *out = rd(h, 0x28);
    0
}
