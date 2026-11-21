#![allow(non_snake_case)]

use sc_dll::*;
use std::sync::Mutex;

fn c(f: Fwd, a: &[usize]) -> usize {
    let mut v = [0usize; 12];
    v[..a.len()].copy_from_slice(a);
    assert!(console::module() != 0, "console sc_dll.dll not found; set TOMBOX_SC_CONSOLE");
    unsafe { f(v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8], v[9], v[10], v[11]) }
}

fn adr<T>(p: &T) -> usize {
    p as *const T as usize
}

fn adrm<T>(p: &mut T) -> usize {
    p as *mut T as usize
}

#[repr(C)]
struct Alloc {
    alloc: unsafe extern "system" fn(*mut Mutex<Vec<usize>>, u32) -> usize,
    free: unsafe extern "system" fn(*mut Mutex<Vec<usize>>, usize) -> u32,
    ctx: *mut Mutex<Vec<usize>>,
}

unsafe extern "system" fn no_alloc(_: *mut Mutex<Vec<usize>>, _: u32) -> usize {
    0
}

unsafe extern "system" fn track_free(ctx: *mut Mutex<Vec<usize>>, p: usize) -> u32 {
    (*ctx).lock().unwrap().push(p);
    0
}

fn tracker() -> (Box<Mutex<Vec<usize>>>, Alloc) {
    let mut log = Box::new(Mutex::new(Vec::new()));
    let a = Alloc { alloc: no_alloc, free: track_free, ctx: &mut *log };
    (log, a)
}

fn frees(log: &Mutex<Vec<usize>>) -> Vec<usize> {
    log.lock().unwrap().clone()
}

fn freed_by(f: Fwd) {
    let (log, a) = tracker();
    c(f, &[0, adr(&a)]);
    assert!(frees(&log).is_empty());
    let block = [0u64; 4];
    c(f, &[adr(&block), adr(&a)]);
    assert_eq!(frees(&log), vec![adr(&block)]);
}

struct Vih {
    h: [u64; 4],
    _keep: (Box<[u8]>, Box<[u64; 4]>, Box<[u64; 16]>),
}

fn vih(tag: u32, data: &[u8]) -> Vih {
    let mut buf = data.to_vec();
    buf.extend_from_slice(&[0; 8]);
    let buf = buf.into_boxed_slice();
    let node = Box::new([0u64, buf.as_ptr() as u64, ((tag as u64) << 32) | data.len() as u64, 0]);
    let mut blk = Box::new([0u64; 16]);
    blk[6] = &*node as *const _ as u64;
    let h = [0u64, &*blk as *const _ as u64, 0, 0];
    Vih { h, _keep: (buf, node, blk) }
}

#[test]
fn SCGetExportFunctions_matches_console_table() {
    let base = console::module();
    assert!(base != 0);
    let real: unsafe extern "system" fn(*mut u8) -> u32 = unsafe { core::mem::transmute(console::proc("SCGetExportFunctions\0").unwrap()) };
    let mut theirs = [0xAAu8; EXPORT_TABLE_BYTES];
    let mut ours = [0xAAu8; EXPORT_TABLE_BYTES];
    assert_eq!(unsafe { real(theirs.as_mut_ptr()) }, 0);
    assert_eq!(unsafe { SCGetExportFunctions(ours.as_mut_ptr()) }, 0);
    assert_eq!(unsafe { SCGetExportFunctions(core::ptr::null_mut()) }, 3);
    let slot = |t: &[u8], o: usize| u64::from_le_bytes(t[o..o + 8].try_into().unwrap()) as usize;
    let mut named = std::collections::HashMap::new();
    for line in include_str!("../exports.txt").lines() {
        let n = line.split_whitespace().nth(1).unwrap();
        if let Some(p) = console::proc(&format!("{n}\0")) {
            named.insert(p as usize, n.to_string());
        }
    }
    let table = export_table();
    for o in (0..EXPORT_TABLE_BYTES).step_by(8) {
        let (t, u) = (slot(&theirs, o), slot(&ours, o));
        if ZEROED_SLOTS.contains(&o) {
            assert_eq!((t, u), (0, 0));
        } else if let Some(&(_, name, f)) = table.iter().find(|e| e.0 == o) {
            assert_eq!(f, u);
            match named.get(&t) {
                Some(n) => assert_eq!(name, n, "slot {o:#x}"),
                None => {
                    assert_eq!(o, 0x160, "unnamed console slot {o:#x}");
                    assert_eq!(unsafe { *(t as *const u8) }, 0xC2);
                }
            }
        } else {
            assert_eq!((t, u), (0xAAAA_AAAA_AAAA_AAAA, 0xAAAA_AAAA_AAAA_AAAA), "slot {o:#x} must stay untouched");
        }
    }
}

#[test]
fn SCCheckCompatibility_console_rules() {
    assert_eq!(SCCheckCompatibility(0x10000, 0x188), 0);
    assert_eq!(SCCheckCompatibility(0x1FFFF, 0x188), 0);
    assert_eq!(SCCheckCompatibility(0x20000, 0x188), 4);
    assert_eq!(SCCheckCompatibility(0, 0x187), 4);
    let real: unsafe extern "system" fn(u32, u32) -> u32 = unsafe { core::mem::transmute(console::proc("SCCheckCompatibility\0").unwrap()) };
    for (v, i) in [(0u32, 0x188u32), (0x30000, 0x188), (0xFFFF, 0x188), (0x10000, 0x189), (0x8000_0000, 0x188)] {
        assert_eq!(SCCheckCompatibility(v, i), unsafe { real(v, i) });
    }
}

#[test]
fn SCGetInstructionSet_matches_console() {
    let real: unsafe extern "system" fn(i32) -> u32 = unsafe { core::mem::transmute(console::proc("SCGetInstructionSet\0").unwrap()) };
    for chip in -2..200 {
        assert_eq!(SCGetInstructionSet(chip), unsafe { real(chip) }, "chip {chip}");
    }
    assert_eq!(SCGetInstructionSet(110), 11);
    assert_eq!(SCGetInstructionSet(130), 13);
}

#[test]
fn SCGetSCOption_reads_bits() {
    let opts = [0x8000_0001u32, 0x10];
    unsafe {
        assert_eq!(SCGetSCOption(opts.as_ptr(), 0), 1);
        assert_eq!(SCGetSCOption(opts.as_ptr(), 1), 0);
        assert_eq!(SCGetSCOption(opts.as_ptr(), 31), 1);
        assert_eq!(SCGetSCOption(opts.as_ptr(), 36), 1);
        assert_eq!(SCGetSCOption(opts.as_ptr(), 37), 0);
    }
}

#[test]
fn SCSetSCOption_sets_and_clears() {
    let mut opts = [0u32; 3];
    unsafe {
        SCSetSCOption(opts.as_mut_ptr(), 70, 1);
        SCSetSCOption(opts.as_mut_ptr(), 3, 5);
        assert_eq!(opts, [8, 0, 0x40]);
        SCSetSCOption(opts.as_mut_ptr(), 3, 0);
        assert_eq!(opts, [0, 0, 0x40]);
    }
}

#[test]
fn SCMergeShaderPair_null_check() {
    let x = 0u8;
    assert_eq!(SCMergeShaderPair(core::ptr::null_mut()), 3);
    assert_eq!(SCMergeShaderPair(&x as *const u8 as *mut _), 0);
}

#[test]
fn SCCompileShaderPair_null_check() {
    let x = 0u8;
    assert_eq!(SCCompileShaderPair(core::ptr::null_mut()), 3);
    assert_eq!(SCCompileShaderPair(&x as *const u8 as *mut _), 0);
}

macro_rules! empty_test {
    ($($t:ident => $f:ident),*) => {$(
        #[test]
        fn $t() {
            let body = console::proc(concat!(stringify!($f), "\0")).unwrap() as *const u8;
            assert_eq!(unsafe { *body }, 0xC2);
            assert_eq!(unsafe { (body.add(1) as *const u16).read_unaligned() }, 0);
            let mut sentinel = [0x3Cu8; 16];
            let real: unsafe extern "system" fn(*mut u8, *mut u8) = unsafe { core::mem::transmute(body) };
            unsafe { real(sentinel.as_mut_ptr(), sentinel.as_mut_ptr()) };
            $f();
            assert_eq!(sentinel, [0x3C; 16]);
        }
    )*};
}

empty_test!(
    SCDumpILShader_retail_noop => SCDumpILShader,
    SCDumpILTranslation_retail_noop => SCDumpILTranslation,
    SCDumpSrcShaderData_retail_noop => SCDumpSrcShaderData,
    SCDumpHwShader_retail_noop => SCDumpHwShader,
    SCDumpHwShaderData_retail_noop => SCDumpHwShaderData,
    SCDumpRawShader_retail_noop => SCDumpRawShader,
    SCDumpHwShaderRaw_retail_noop => SCDumpHwShaderRaw,
    SC_AssembleHandle_Destroy_retail_noop => SC_AssembleHandle_Destroy,
    SC_MetadataHandle_Destroy_retail_noop => SC_MetadataHandle_Destroy
);

#[test]
fn SC_Assemble_retail_returns_zero() {
    assert_eq!(SC_Assemble(), 0);
    let real: unsafe extern "system" fn() -> usize = unsafe { core::mem::transmute(console::proc("SC_Assemble\0").unwrap()) };
    assert_eq!(unsafe { real() }, 0);
}

#[test]
fn SC_AssembleHandle_GetShader_retail_false() {
    assert_eq!(SC_AssembleHandle_GetShader(), 0);
    let real: unsafe extern "system" fn() -> u8 = unsafe { core::mem::transmute(console::proc("SC_AssembleHandle_GetShader\0").unwrap()) };
    assert_eq!(unsafe { real() }, 0);
}

#[test]
fn SC_CompileShaderFromMetadata_retail_unsupported() {
    assert_eq!(SC_CompileShaderFromMetadata(), 4);
    let real: unsafe extern "system" fn() -> u32 = unsafe { core::mem::transmute(console::proc("SC_CompileShaderFromMetadata\0").unwrap()) };
    assert_eq!(unsafe { real() }, 4);
}

#[test]
fn OG_GetCompactOptionSet_copies_32_bytes() {
    let g = 1u8;
    let src: [u8; 40] = core::array::from_fn(|i| i as u8 + 1);
    let mut dst = [0xEEu8; 40];
    unsafe {
        assert_eq!(OG_GetCompactOptionSet(core::ptr::null_mut(), src.as_ptr(), dst.as_mut_ptr()), 3);
        assert_eq!(OG_GetCompactOptionSet(&g as *const u8 as *mut _, core::ptr::null(), dst.as_mut_ptr()), 3);
        assert_eq!(OG_GetCompactOptionSet(&g as *const u8 as *mut _, src.as_ptr(), dst.as_mut_ptr()), 0);
    }
    assert_eq!(&dst[..32], &src[..32]);
    assert_eq!(&dst[32..], &[0xEE; 8]);
}

#[test]
fn SC_DebugMapHandle_GetMap_tag_one() {
    let mut a = [0x401u32, 0];
    let b = [2u32, 0];
    unsafe {
        assert_eq!(SC_DebugMapHandle_GetMap(a.as_mut_ptr() as *mut _), a.as_mut_ptr() as *mut _);
        assert!(SC_DebugMapHandle_GetMap(b.as_ptr() as *mut _).is_null());
        assert!(SC_DebugMapHandle_GetMap(core::ptr::null_mut()).is_null());
    }
}

#[test]
fn SC_RegisterLifetimeHandle_GetMap_tag_two() {
    let mut a = [0x802u32, 0];
    let b = [1u32, 0];
    unsafe {
        assert_eq!(SC_RegisterLifetimeHandle_GetMap(a.as_mut_ptr() as *mut _), a.as_mut_ptr() as *mut _);
        assert!(SC_RegisterLifetimeHandle_GetMap(b.as_ptr() as *mut _).is_null());
        assert!(SC_RegisterLifetimeHandle_GetMap(core::ptr::null_mut()).is_null());
    }
}

#[test]
fn SCCreate_installs_default_allocator_then_rejects_null_params() {
    let mut desc = [0u64; 0x80];
    assert_eq!(c(SCCreate, &[adrm(&mut desc), 0]), 0);
    let base = console::module();
    assert!(desc[7] as usize > base && desc[8] as usize > base);
    let mut desc2 = [0u64; 0x80];
    desc2[8] = 1;
    let params = [0u64; 0x80];
    assert_eq!(c(SCCreate, &[adrm(&mut desc2), adr(&params)]), 0);
    assert_eq!(desc2[7], 0);
}

#[test]
fn SCDestroy_null_compiler() {
    assert_eq!(c(SCDestroy, &[0]) as u32, 3);
}

#[test]
fn SCCompileShader_null_compiler() {
    assert_eq!(c(SCCompileShader, &[0, 0, 0, 0, 0]) as u32, 3);
}

#[test]
fn SCSimpleCompileShader_rejects_conflicting_flags() {
    let mut input = [0u32; 0x400];
    input[0] = 0x18;
    let mut handle = [0x55u64, 0];
    let mut out = [0u64; 8];
    out[0] = adrm(&mut handle) as u64;
    let mut status = [0x77u32, 0];
    assert_eq!(c(SCSimpleCompileShader, &[adrm(&mut input), 0, 0, adrm(&mut out), adrm(&mut status), 0]) as u32, 3);
    assert_eq!(handle[0], 0);
    assert_eq!(status[0], 0);
    assert_eq!(input[0], 0x18);
}

#[test]
fn SCSimpleDumpShader_null_output_is_untouched() {
    let mut buf = [0x5Au8; 64];
    c(SCSimpleDumpShader, &[0, adrm(&mut buf)]);
    assert_eq!(buf, [0x5A; 64]);
}

#[test]
fn SC_OutputHandle_CreateFromExtent_null_extent() {
    assert_eq!(c(SC_OutputHandle_CreateFromExtent, &[0, 0, 0, 0, 0]), 0);
}

#[test]
fn SC_OutputHandle_CreateFromShaderObject_roundtrip() {
    let obj = [0i64, -1, 0, 0];
    let h = c(SC_OutputHandle_CreateFromShaderObject, &[adr(&obj), 0, 0]);
    assert!(h != 0);
    let base = console::module();
    let words = unsafe { core::slice::from_raw_parts(h as *const usize, 4) };
    assert!(words[0] > base && words[0] < base + 0x60_0000);
    assert_eq!(&words[1..], &[0, 0, 0]);
    assert_eq!(c(SC_OutputHandle_GetShaderObject, &[h]), 0);
    c(SC_OutputHandle_Destroy, &[h, 0]);
}

#[test]
fn SC_OutputHandle_GetSerializedObjectExtent_empty_handle() {
    let h = [0u64; 4];
    let mut a = [0u64; 4];
    let mut b = [0u64; 4];
    assert_eq!(c(SC_OutputHandle_GetSerializedObjectExtent, &[adr(&h), adrm(&mut a), 0, adrm(&mut b)]), 0);
}

#[test]
fn SC_OutputHandle_ReleaseSerializedObjectExtent_frees_with_caller_allocator() {
    let (log, a) = tracker();
    let block = [0u64; 2];
    c(SC_OutputHandle_ReleaseSerializedObjectExtent, &[adr(&block), adr(&a)]);
    assert_eq!(frees(&log), vec![adr(&block)]);
}

#[test]
fn SC_OutputHandle_GetShaderObject_layout() {
    let mut h = [0u64; 4];
    assert_eq!(c(SC_OutputHandle_GetShaderObject, &[adr(&h)]), 0);
    h[1] = 0x1000;
    assert_eq!(c(SC_OutputHandle_GetShaderObject, &[adr(&h)]), 0x1028);
}

fn destroys_output(f: Fwd) {
    let (log, a) = tracker();
    let mut inner = [0u8; 16];
    let ip = adrm(&mut inner);
    let mut h = [0u64, ip as u64, 16, 0];
    let hp = adrm(&mut h);
    c(f, &[hp, adr(&a)]);
    let inner_after: [u8; 16] = unsafe { core::ptr::read_volatile(ip as *const [u8; 16]) };
    let h_after: [u64; 4] = unsafe { core::ptr::read_volatile(hp as *const [u64; 4]) };
    assert_eq!(inner_after, [0xFF; 16]);
    assert_eq!(frees(&log), vec![ip, hp]);
    assert_eq!(h_after[1], 0);
    c(f, &[0, adr(&a)]);
    assert_eq!(frees(&log).len(), 2);
}

#[test]
fn SC_OutputHandle_Destroy_scrubs_and_frees() {
    destroys_output(SC_OutputHandle_Destroy);
}

#[test]
fn SC_VIH_CloseVIH_scrubs_and_frees() {
    destroys_output(SC_VIH_CloseVIH);
}

#[test]
fn SC_InstantiateVIHFromDXBC_null_dxbc() {
    assert_eq!(c(SC_InstantiateVIHFromDXBC, &[0, 0, 0, 0]), 0);
}

#[test]
fn SC_InstantiateTypedVIHFromDXBC_null_dxbc() {
    assert_eq!(c(SC_InstantiateTypedVIHFromDXBC, &[0, 0, 0, 0]), 0);
}

#[test]
fn SC_VIH_GetPDBName_copies_name_section() {
    let v = vih(6, b"name.pdb\0");
    let mut out = [0u8; 0x40];
    assert_eq!(c(SC_VIH_GetPDBName, &[adr(&v.h), adrm(&mut out), 0x40]) as u8, 1);
    assert_eq!(&out[..9], b"name.pdb\0");
    let empty = [0u64; 4];
    assert_eq!(c(SC_VIH_GetPDBName, &[adr(&empty), adrm(&mut out), 0x40]) as u8, 0);
}

#[test]
fn SC_VIH_GetPDBNameLengthInBytes_section_length() {
    let v = vih(6, b"name.pdb\0");
    assert_eq!(c(SC_VIH_GetPDBNameLengthInBytes, &[adr(&v.h)]) as u32, 9);
    let empty = [0u64; 4];
    assert_eq!(c(SC_VIH_GetPDBNameLengthInBytes, &[adr(&empty)]) as u32, 0);
}

#[test]
fn SC_VIH_GetNamedFlags_copies_16_byte_section() {
    let v = vih(4, b"ABCDEFGH");
    let mut out = [0x11u8; 0x20];
    assert_eq!(c(SC_VIH_GetNamedFlags, &[adr(&v.h), adrm(&mut out)]) as u8, 1);
    assert_eq!(&out[..8], b"ABCDEFGH");
    assert_eq!(&out[8..16], &[0; 8]);
    assert_eq!(&out[16..], &[0x11; 16]);
}

#[test]
fn SC_VIH_GetCompileSettings_copies_32_byte_section() {
    let v = vih(0xc, b"0123456789");
    let mut out = [0x11u8; 0x30];
    assert_eq!(c(SC_VIH_GetCompileSettings, &[adr(&v.h), adrm(&mut out)]) as u8, 1);
    assert_eq!(&out[..10], b"0123456789");
    assert_eq!(&out[10..32], &[0; 22]);
    assert_eq!(&out[32..], &[0x11; 16]);
}

#[test]
fn SC_VIH_GetUniqueHash_reads_hash_section() {
    let v = vih(0xa, &0x1234_5678u32.to_le_bytes());
    assert_eq!(c(SC_VIH_GetUniqueHash, &[adr(&v.h)]) as u32, 0x1234_5678);
    let w = vih(0xa, &[1, 2, 3]);
    assert_eq!(c(SC_VIH_GetUniqueHash, &[adr(&w.h)]) as u32, 0);
}

#[test]
fn SC_VIH_GetOutputStats_copies_0xc8_bytes() {
    let mut blk = [0u8; 0x200];
    for (i, b) in blk.iter_mut().enumerate() {
        *b = i as u8;
    }
    let h = [0u64, adr(&blk) as u64, 0, 0];
    let mut out = [0xEEu8; 0x100];
    assert_eq!(c(SC_VIH_GetOutputStats, &[adr(&h), adrm(&mut out)]) as u8, 1);
    assert_eq!(&out[..0xC8], &blk[0x38..0x38 + 0xC8]);
    assert_eq!(&out[0xC8..], &[0xEE; 0x38]);
    let empty = [0u64; 4];
    assert_eq!(c(SC_VIH_GetOutputStats, &[adr(&empty), adrm(&mut out)]) as u8, 0);
}

#[test]
fn SC_GetShaderStatsFromDXBC_null_stats() {
    assert_eq!(c(SC_GetShaderStatsFromDXBC, &[0, 0, 0, 0, 0x110]) as u8, 0);
}

#[test]
fn SC_GetOriginalShaderOffsetInBytes_null_dxbc() {
    assert_eq!(c(SC_GetOriginalShaderOffsetInBytes, &[0, 0, 0, 0]) as u32, 0);
}

#[test]
fn SC_DebugMapHandle_Destroy_frees() {
    freed_by(SC_DebugMapHandle_Destroy);
}

#[test]
fn SC_RegisterLifetimeHandle_Destroy_frees() {
    freed_by(SC_RegisterLifetimeHandle_Destroy);
}

#[test]
fn SC_IdMapHandle_Destroy_frees() {
    freed_by(SC_IdMapHandle_Destroy);
}

#[test]
fn SC_IdMap_Release_frees() {
    freed_by(SC_IdMap_Release);
}

#[test]
fn SC_CBUsageHandle_Destroy_frees() {
    freed_by(SC_CBUsageHandle_Destroy);
}

#[test]
fn SC_IdMapHandle_GetIdMap_rejects_wrong_handles() {
    assert_eq!(c(SC_IdMapHandle_GetIdMap, &[0, 0]), 0);
    let wrong_tag = [2u32, 0, 0, 1, 0, 0];
    assert_eq!(c(SC_IdMapHandle_GetIdMap, &[adr(&wrong_tag), 0]), 0);
    let not_packed = [3u32, 0, 0, 0, 0, 0];
    assert_eq!(c(SC_IdMapHandle_GetIdMap, &[adr(&not_packed), 0]), 0);
}

#[test]
fn SC_DXBC_GetExternalILInterface_fills_three_entry_points() {
    let base = console::module();
    let mut t = [0usize; 4];
    assert_eq!(c(SC_DXBC_GetExternalILInterface, &[adrm(&mut t)]), adr(&t));
    for p in &t[..3] {
        assert!(*p > base && *p < base + 0x60_0000);
    }
    assert_eq!(t[3], 0);
}

fn adler32(d: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in d {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

#[test]
fn SC_CompressExtent_writes_size_header_and_zlib() {
    let src: Vec<u8> = (0..256u32).map(|i| (i * 7) as u8).collect();
    let mut out = [0usize; 2];
    let mut len = [0u32; 2];
    assert_eq!(c(SC_CompressExtent, &[src.as_ptr() as usize, src.len(), adrm(&mut out), adrm(&mut len), 0]) as u32, 0);
    let n = len[0] as usize;
    assert!(n > 14);
    let bytes = unsafe { core::slice::from_raw_parts(out[0] as *const u8, n) };
    assert_eq!(u64::from_le_bytes(bytes[..8].try_into().unwrap()), 256u64 << 40);
    assert_eq!(bytes[8], 0x78);
    assert_eq!(u32::from_be_bytes(bytes[n - 4..].try_into().unwrap()), adler32(&src));
    assert_eq!(c(SC_CompressExtent_Destroy, &[out[0], 0]) as u32, 0);
}

#[test]
fn SC_CompressExtent_Destroy_frees_with_caller_allocator() {
    let (log, a) = tracker();
    let block = [0u64; 2];
    assert_eq!(c(SC_CompressExtent_Destroy, &[adr(&block), adr(&a)]) as u32, 0);
    assert_eq!(frees(&log), vec![adr(&block)]);
}

#[test]
fn SC_SCDX12TableDesc_Destroy_null_desc() {
    let (log, a) = tracker();
    c(SC_SCDX12TableDesc_Destroy, &[0, adr(&a)]);
    assert!(frees(&log).is_empty());
}

#[test]
fn SC_ConvertRootSignatureToSCInputs_null_inputs() {
    assert_eq!(c(SC_ConvertRootSignatureToSCInputs, &[0, 0, 0, 0, 0]) as u32, 3);
}

fn generator() -> usize {
    let sis = [0u8; 0x400];
    let mut g = 0usize;
    assert_eq!(c(OG_InitOptionGenerator, &[1, adr(&sis), adrm(&mut g)]) as u32, 0);
    assert!(g != 0);
    g
}

#[test]
fn OG_InitOptionGenerator_kinds() {
    let sis = [0u8; 0x400];
    let mut g = 0usize;
    assert_eq!(c(OG_InitOptionGenerator, &[7, adr(&sis), adrm(&mut g)]) as u32, 3);
    assert_eq!(c(OG_InitOptionGenerator, &[1, 0, adrm(&mut g)]) as u32, 3);
    let g = generator();
    assert_eq!(c(OG_OptionGeneratorDestroy, &[g]) as u32, 0);
}

#[test]
fn OG_OptionGeneratorDestroy_null_and_real() {
    assert_eq!(c(OG_OptionGeneratorDestroy, &[0]) as u32, 3);
    let g = generator();
    assert_eq!(c(OG_OptionGeneratorDestroy, &[g]) as u32, 0);
}

#[test]
fn OG_GetNextOptionSet_walks_sets() {
    assert_eq!(c(OG_GetNextOptionSet, &[0, 0]) as u32, 3);
    let g = generator();
    let mut a = 0usize;
    let mut b = 0usize;
    assert_eq!(c(OG_GetNextOptionSet, &[g, adrm(&mut a)]) as u32, 0);
    assert_eq!(c(OG_GetNextOptionSet, &[g, adrm(&mut b)]) as u32, 0);
    assert!(a != 0 && b != 0 && a != b);
    let mut compact = [0u8; 32];
    assert_eq!(unsafe { OG_GetCompactOptionSet(g as *mut _, a as *const u8, compact.as_mut_ptr()) }, 0);
    assert_eq!(c(OG_OptionGeneratorDestroy, &[g]) as u32, 0);
}

#[test]
fn OG_GetOptionSetID_null_args() {
    let x = 0u64;
    assert_eq!(c(OG_GetOptionSetID, &[0, adr(&x), adr(&x)]) as u32, 3);
    assert_eq!(c(OG_GetOptionSetID, &[adr(&x), 0, adr(&x)]) as u32, 3);
    assert_eq!(c(OG_GetOptionSetID, &[adr(&x), adr(&x), 0]) as u32, 3);
}

#[test]
fn OG_ApplyOptionSetToSIS_null_args() {
    let x = 0u64;
    assert_eq!(c(OG_ApplyOptionSetToSIS, &[0, adr(&x), adr(&x)]) as u32, 3);
    assert_eq!(c(OG_ApplyOptionSetToSIS, &[adr(&x), adr(&x), 0]) as u32, 3);
}

#[test]
fn OG_SetOptionPerf_null_args() {
    let x = 0u64;
    assert_eq!(c(OG_SetOptionPerf, &[0, adr(&x), adr(&x)]) as u32, 3);
    assert_eq!(c(OG_SetOptionPerf, &[adr(&x), 0, adr(&x)]) as u32, 3);
}

#[test]
fn OG_GetOptionPerf_null_args() {
    let x = 0u64;
    assert_eq!(c(OG_GetOptionPerf, &[0, adr(&x), adr(&x)]) as u32, 3);
    assert_eq!(c(OG_GetOptionPerf, &[adr(&x), adr(&x), 0]) as u32, 3);
}

#[test]
fn OG_GetBestOption_null_generator() {
    assert_eq!(c(OG_GetBestOption, &[0, 0, 0]) as u32, 3);
}

#[test]
fn OG_GetSerializedOptionSet_null_args() {
    let x = 0u64;
    assert_eq!(c(OG_GetSerializedOptionSet, &[0, adr(&x), adr(&x)]) as u32, 3);
    assert_eq!(c(OG_GetSerializedOptionSet, &[adr(&x), 0, adr(&x)]) as u32, 3);
}

fn compact_pattern() -> [u8; 32] {
    core::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(11))
}

#[test]
fn OG_ApplyCompactOptionSetToSIS_maps_fields() {
    let cp = compact_pattern();
    let mut sis = [0u8; 0x400];
    assert_eq!(c(OG_ApplyCompactOptionSetToSIS, &[0, adrm(&mut sis)]) as u32, 3);
    assert_eq!(c(OG_ApplyCompactOptionSetToSIS, &[adr(&cp), adrm(&mut sis)]) as u32, 0);
    assert_eq!(&sis[0x10..0x18], &cp[..8]);
    assert_eq!(u32::from_le_bytes(sis[0x18..0x1c].try_into().unwrap()) & 0x3ffff, u32::from_le_bytes(cp[8..12].try_into().unwrap()) & 0x3ffff);
}

#[test]
fn OG_ApplySISToCompactOptionSet_roundtrips() {
    let cp = compact_pattern();
    let mut sis = [0u8; 0x400];
    assert_eq!(c(OG_ApplyCompactOptionSetToSIS, &[adr(&cp), adrm(&mut sis)]) as u32, 0);
    let mut back = [0u8; 32];
    assert_eq!(c(OG_ApplySISToCompactOptionSet, &[0, adrm(&mut back)]) as u32, 3);
    assert_eq!(c(OG_ApplySISToCompactOptionSet, &[adr(&sis), adrm(&mut back)]) as u32, 0);
    assert_eq!(&back[..16], &cp[..16]);
}

#[test]
fn OG_GetSerializedCompactOptionSet_rejects_bad_args() {
    let cp = compact_pattern();
    let mut buf = [0u8; 64];
    assert_eq!(c(OG_GetSerializedCompactOptionSet, &[adr(&cp), 0, adrm(&mut buf), 0]) as u32, 3);
    let mut len = [64u32, 0];
    assert_eq!(c(OG_GetSerializedCompactOptionSet, &[adr(&cp), 2, adrm(&mut buf), adrm(&mut len)]) as u32, 3);
}

#[test]
fn console_loader_refuses_relative_and_missing_paths() {
    assert_eq!(console::load_from(std::path::Path::new("sc_dll.dll")), 0);
    assert_eq!(console::load_from(std::path::Path::new(r"C:\definitely\missing\sc_dll.dll")), 0);
    assert_eq!(FORWARDED.len() + 22, 67);
}
