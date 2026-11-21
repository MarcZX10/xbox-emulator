use crate::win::*;
use crate::*;

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[link(name = "user32")]
extern "system" {
    fn DialogBoxIndirectParamW(inst: isize, tmpl: *const u8, parent: isize, proc_: *const (), init: isize) -> isize;
    fn EndDialog(h: isize, r: isize) -> i32;
    fn GetDlgItem(h: isize, id: i32) -> isize;
    fn GetParent(h: isize) -> isize;
    fn PostMessageW(h: isize, msg: u32, wp: usize, lp: isize) -> i32;
    fn SetDlgItemTextA(h: isize, id: i32, text: *const u8) -> i32;
    fn GetWindowRect(h: isize, r: *mut Rect) -> i32;
    fn GetDC(h: isize) -> isize;
    fn ReleaseDC(h: isize, dc: isize) -> i32;
    fn SetWindowPos(h: isize, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn GetDeviceCaps(dc: isize, i: i32) -> i32;
}

pub const IDD_EULA: u16 = 0xfa0;
pub const IDC_TEXT: u16 = 0xfa2;
pub const IDYES: isize = 6;
pub const IDNO: isize = 7;
pub const WM_CLOSE: u32 = 0x10;
pub const WM_INITDIALOG: u32 = 0x110;
pub const WM_COMMAND: u32 = 0x111;
pub const EM_SETSEL: u32 = 0xb1;

pub const TERMS_TEXT: &str = "Symbol server terms of use.\r\n\r\nSymbols downloaded from a Microsoft symbol server are provided for debugging only. Choose Yes to accept the terms of use for this symbol server, or No to decline. Your answer is saved next to symsrv.dll as symsrv.yes or symsrv.no; run symsrv.dll,RunDllEntry ResetTOU to clear it.\0";

pub fn module_dir() -> Vec<u16> {
    unsafe {
        let mut h = 0isize;
        if GetModuleHandleExW(6, module_dir as *const u8, &mut h) == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u16; MAX_PATH_W];
        let n = GetModuleFileNameW(h, buf.as_mut_ptr(), MAX_PATH_W as u32) as usize;
        if n == 0 {
            return Vec::new();
        }
        store::dir_of(&buf[..n])
    }
}

pub fn answer_file(yes: bool) -> Vec<u16> {
    join(&module_dir(), &w(if yes { "symsrv.yes" } else { "symsrv.no" }))
}

fn delete_answer(p: &[u16]) {
    unsafe {
        if exists(p) {
            DeleteFileW(z(p).as_ptr());
        } else {
            let e = GetLastError();
            SetLastError(map_missing(e));
        }
    }
}

pub fn reset_tou() {
    delete_answer(&answer_file(true));
    delete_answer(&answer_file(false));
}

fn push16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn push32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn pushw(v: &mut Vec<u8>, s: &str) {
    for c in s.encode_utf16() {
        push16(v, c);
    }
    push16(v, 0);
}

fn align4(v: &mut Vec<u8>) {
    while v.len() % 4 != 0 {
        v.push(0);
    }
}

fn item(v: &mut Vec<u8>, style: u32, rect: (i16, i16, i16, i16), id: u16, class: u16, title: &str) {
    align4(v);
    push32(v, style);
    push32(v, 0);
    for x in [rect.0, rect.1, rect.2, rect.3] {
        push16(v, x as u16);
    }
    push16(v, id);
    push16(v, 0xffff);
    push16(v, class);
    pushw(v, title);
    push16(v, 0);
}

pub fn dialog_template() -> Vec<u32> {
    let mut v = Vec::new();
    push32(&mut v, 0x8000_0000 | 0x00c0_0000 | 0x0008_0000 | 0x80);
    push32(&mut v, 0);
    push16(&mut v, 3);
    for x in [0i16, 0, 300, 200] {
        push16(&mut v, x as u16);
    }
    push16(&mut v, 0);
    push16(&mut v, 0);
    pushw(&mut v, "Symbol Server Terms of Use");
    item(&mut v, 0x4000_0000 | 0x1000_0000 | 0x0020_0000 | 0x0080_0000 | 0x4 | 0x800 | 0x40, (7, 7, 286, 160), IDC_TEXT, 0x81, "");
    item(&mut v, 0x4000_0000 | 0x1000_0000 | 0x0001_0000 | 1, (180, 175, 50, 14), IDYES as u16, 0x80, "&Yes");
    item(&mut v, 0x4000_0000 | 0x1000_0000 | 0x0001_0000, (240, 175, 50, 14), IDNO as u16, 0x80, "&No");
    align4(&mut v);
    v.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

pub unsafe fn center(h: isize, parent: isize) {
    let mut a = Rect::default();
    let mut b = Rect::default();
    GetWindowRect(h, &mut a);
    let (w, hh) = (a.right - a.left, a.bottom - a.top);
    GetWindowRect(parent, &mut b);
    let (pw, ph) = (b.right - b.left, b.bottom - b.top);
    let dc = GetDC(h);
    if dc == 0 {
        return;
    }
    let sw = GetDeviceCaps(dc, 8);
    let sh = GetDeviceCaps(dc, 10);
    ReleaseDC(h, dc);
    let mut x = (pw - w) / 2 + b.left;
    if x < 0 {
        x = 0;
    } else if x + w > sw {
        x = sw - w;
    }
    let mut y = (ph - hh) / 2 + b.top;
    if y < 0 {
        y = 0;
    } else if y + hh > sh {
        y = sh - hh;
    }
    SetWindowPos(h, 0, x, y, 0, 0, 5);
}

#[no_mangle]
pub unsafe extern "system" fn EulaDlgProc(h: isize, msg: u32, wp: usize, _lp: isize) -> isize {
    match msg {
        WM_CLOSE => {
            EndDialog(h, IDNO);
            1
        }
        WM_INITDIALOG => {
            center(h, GetParent(h));
            SetDlgItemTextA(h, IDC_TEXT as i32, TERMS_TEXT.as_ptr());
            PostMessageW(GetDlgItem(h, IDC_TEXT as i32), EM_SETSEL, usize::MAX, -1);
            1
        }
        WM_COMMAND => match (wp & 0xffff) as isize {
            IDYES => {
                EndDialog(h, IDYES);
                1
            }
            IDNO => {
                EndDialog(h, IDNO);
                1
            }
            _ => 0,
        },
        _ => 0,
    }
}

pub unsafe fn console_window() -> isize {
    let k = GetModuleHandleW(z(&w("kernel32")).as_ptr());
    if k == 0 {
        return 0;
    }
    let f = GetProcAddress(k, b"GetConsoleWindow\0".as_ptr());
    if f == 0 {
        return 0;
    }
    let f: unsafe extern "system" fn() -> isize = core::mem::transmute(f);
    f()
}

pub unsafe fn record_answer(r: isize) {
    let p = answer_file(r != IDNO);
    let h = CreateFileW(z(&p).as_ptr(), GENERIC_WRITE, 0, core::ptr::null(), 2, 0x80, 0);
    if h != INVALID {
        CloseHandle(h);
    }
}

pub fn answer_code(r: isize) -> u32 {
    match r {
        IDYES => 1,
        IDNO => 2,
        _ => 0,
    }
}

pub unsafe fn show_terms() -> u32 {
    if has(SSRVOPT_UNATTENDED) {
        return 2;
    }
    let parent = {
        let p = STATE.lock().unwrap().parent;
        if p != 0 {
            p
        } else {
            console_window()
        }
    };
    let mut inst = 0isize;
    GetModuleHandleExW(6, module_dir as *const u8, &mut inst);
    let t = dialog_template();
    let r = DialogBoxIndirectParamW(inst, t.as_ptr() as *const u8, parent, EulaDlgProc as *const (), 0);
    record_answer(r);
    answer_code(r)
}

#[no_mangle]
pub unsafe extern "system" fn RunDllEntry(hwnd: isize, _inst: isize, cmd: *const u16, _show: i32) {
    let c = String::from_utf16_lossy(wstr(cmd));
    if c.eq_ignore_ascii_case("ResetTOU") {
        reset_tou();
    } else if c.eq_ignore_ascii_case("terms") {
        {
            let mut s = STATE.lock().unwrap();
            if hwnd != 0 {
                s.options |= SSRVOPT_PARENTWIN;
            } else {
                s.options &= !SSRVOPT_PARENTWIN;
            }
        }
        reset_tou();
        show_terms();
    }
}
