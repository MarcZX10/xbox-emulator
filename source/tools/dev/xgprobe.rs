// Dev probe: load the real Xbox xg_xs.dll (user-supplied, runs natively on x64 Windows),
// create a texture address computer for a D3D12-style resource desc and dump what it reports.
// usage: xgprobe <path to xg_xs.dll> <w> <h> <arr> <mips> <dxgi fmt> <layout>
use std::ffi::c_void;
extern "system" {
    fn LoadLibraryW(n: *const u16) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, n: *const u8) -> *mut c_void;
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let w: Vec<u16> = a[1].encode_utf16().chain(Some(0)).collect();
    let n = |i: usize| a[i].parse::<u64>().unwrap();
    unsafe {
        let h = LoadLibraryW(w.as_ptr());
        assert!(!h.is_null(), "LoadLibrary failed");
        let f = GetProcAddress(h, b"XGCreateTextureComputer\0".as_ptr());
        assert!(!f.is_null());
        let create: unsafe extern "system" fn(*const u8, *mut *mut usize) -> i32 = std::mem::transmute(f);
        let mut d = [0u8; 0x38];
        d[0..4].copy_from_slice(&3u32.to_le_bytes());
        d[0x10..0x18].copy_from_slice(&n(2).to_le_bytes());
        d[0x18..0x1c].copy_from_slice(&(n(3) as u32).to_le_bytes());
        d[0x1c..0x1e].copy_from_slice(&(n(4) as u16).to_le_bytes());
        d[0x1e..0x20].copy_from_slice(&(n(5) as u16).to_le_bytes());
        d[0x20..0x24].copy_from_slice(&(n(6) as u32).to_le_bytes());
        d[0x24..0x28].copy_from_slice(&1u32.to_le_bytes());
        d[0x2c..0x30].copy_from_slice(&(n(7) as u32).to_le_bytes());
        let mut comp: *mut usize = std::ptr::null_mut();
        let hr = create(d.as_ptr(), &mut comp);
        println!("XGCreateTextureComputer hr={hr:#010x} comp={comp:p}");
        if hr < 0 { return; }
        let vt = *comp as *const usize;
        for i in 0..16 {
            let p = *vt.add(i);
            println!("  vt[{i}] = dll+{:#x}", p.wrapping_sub(h as usize));
        }
        let mut lay = vec![0u8; 0x4000];
        let get: unsafe extern "system" fn(*mut usize, *mut u8) -> i32 = std::mem::transmute(*vt.add(2));
        let hr = get(comp, lay.as_mut_ptr());
        println!("vt[2](layout) hr={hr:#010x}");
        let u64at = |o: usize| u64::from_le_bytes(lay[o..o + 8].try_into().unwrap());
        let u32at = |o: usize| u32::from_le_bytes(lay[o..o + 4].try_into().unwrap());
        println!("size={:#x} align={:#x} mips={} planes={}", u64at(0), u64at(8), u32at(0x10), u32at(0x14));
        // Hypothesis under test (public xg.h order): vt[3] size, vt[4] alignment,
        // vt[5] GetMipLevelOffsetBytes(plane, mip), vt[8] CopyIntoSubresource(pResource, plane,
        // subresource, pData, rowPitch, slicePitch), vt[9] CopyFromSubresource(pData, plane,
        // subresource, pResource, rowPitch, slicePitch).
        let sz: unsafe extern "system" fn(*mut usize) -> u64 = std::mem::transmute(*vt.add(3));
        let al: unsafe extern "system" fn(*mut usize) -> u64 = std::mem::transmute(*vt.add(4));
        let mo: unsafe extern "system" fn(*mut usize, u32, u32) -> u64 = std::mem::transmute(*vt.add(5));
        println!("vt[3]()={:#x} vt[4]()={:#x}", sz(comp), al(comp));
        let mips = u32at(0x10);
        for m in 0..mips { println!("  vt[5](0,{m}) = {:#x}", mo(comp, 0, m)); }
        type Copy = unsafe extern "system" fn(*mut usize, *mut u8, u32, u32, *const u8, u32, u32) -> i32;
        let into: Copy = std::mem::transmute(*vt.add(8));
        let from: Copy = std::mem::transmute(*vt.add(9));
        let total = u64at(0) as usize;
        let bpe = u32at(0x18 + 0x28) as usize; // bytes per element (block for BC)
        let block = if [70u64, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 94, 95, 96, 97, 98, 99].contains(&n(6)) { 4 } else { 1 };
        for m in 0..mips {
            let (w, hh) = (((n(2) >> m).max(1) as usize + block - 1) / block, ((n(3) >> m).max(1) as usize + block - 1) / block);
            let row = w * bpe;
            let lin: Vec<u8> = (0..row * hh).map(|i| (i as u8) ^ (m as u8 * 37) ^ ((i >> 8) as u8)).collect();
            let mut res = vec![0xEEu8; total + 64];
            let hr1 = into(comp, res.as_mut_ptr(), 0, m, lin.as_ptr(), row as u32, (row * hh) as u32);
            let changed: Vec<usize> = (0..total).filter(|&i| res[i] != 0xEE).collect();
            let mut back = vec![0x11u8; row * hh];
            let hr2 = from(comp, back.as_mut_ptr(), 0, m, res.as_ptr(), row as u32, (row * hh) as u32);
            println!("  mip {m}: {w}x{hh} elems row={row} into hr={hr1:#x} wrote {} bytes in [{:#x}..{:#x}] ; from hr={hr2:#x} roundtrip={} guard_ok={}",
                changed.len(), changed.first().copied().unwrap_or(0), changed.last().copied().unwrap_or(0),
                back == lin, res[total..].iter().all(|&b| b == 0xEE));
            {
                // vt[6] hypothesis: GetTexelElementOffsetBytes(plane, mip, x: u64, y, zOrSlice, sample) -> u64
                let teo: unsafe extern "system" fn(*mut usize, u32, u32, u64, u32, u32, u32, *mut u32) -> u64 = std::mem::transmute(*vt.add(6));
                let mut lin2 = vec![0u8; row * hh];
                for i in 0..w * hh { for b in 0..bpe { lin2[i * bpe + b] = if bpe >= 4 { (i as u32).to_le_bytes()[b % 4] ^ (b / 4) as u8 } else { (i * 7 + i / 251) as u8 }; } }
                let mut res2 = vec![0xEEu8; total + 64];
                into(comp, res2.as_mut_ptr(), 0, m, lin2.as_ptr(), row as u32, (row * hh) as u32);
                let mut ok = 0; let mut bad = 0; let mut contiguous_rows = true;
                for y in 0..hh { for x in 0..w {
                    let o = teo(comp, 0, m, x as u64, y as u32, 0, 0, std::ptr::null_mut()) as usize;
                    if o < total && o + bpe <= total && res2[o..o + bpe] == lin2[(y * w + x) * bpe..(y * w + x + 1) * bpe] { ok += 1 } else { bad += 1 }
                    if x > 0 && o != teo(comp, 0, m, 0, y as u32, 0, 0, std::ptr::null_mut()) as usize + x * bpe { contiguous_rows = false; }
                } }
                println!("    vt[6] element offsets: ok={ok} bad={bad} rows_contiguous={contiguous_rows}");
            }
            if m == 0 {
                // where does linear element (x=1,y=0) land?  shows whether the layout is tiled
                let off1 = (0..total - bpe).find(|&i| res[i..i + bpe] == lin[bpe..2 * bpe]);
                let offrow1 = (0..total - bpe).find(|&i| res[i..i + bpe] == lin[row..row + bpe]);
                println!("    element(1,0) at {:?}, element(0,1) at {:?}", off1, offrow1);
            }
        }
        let p = 0x18;
        println!("plane0 header:");
        for o in (0..0x30).step_by(8) { println!("  +{:#04x}: {:#018x}", o, u64at(p + o)); }
        let dump = std::env::var("XG_DUMP").map(|v| v.parse::<usize>().unwrap()).unwrap_or(0x200);
        for o in (0x28..dump).step_by(4) {
            if u32at(p + o) != 0 { println!("  plane+{:#05x}: {:#x}", o, u32at(p + o)); }
        }
    }
}
