use compat_common::gdk::{self, save, xasync, Guid};
use core::ffi::c_void;

extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
}

#[repr(C)]
struct Block {
    queue: usize,
    context: usize,
    callback: usize,
    internal: [u8; 32],
}

fn answer(f: impl FnOnce(usize) + Send + 'static) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(10) {
            if let Some(h) = gdk::win::find_dialog() {
                std::thread::sleep(std::time::Duration::from_millis(100));
                f(h);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("dialog never appeared");
    })
}

fn block() -> Box<Block> {
    Box::new(Block { queue: 0, context: 0, callback: 0, internal: [0; 32] })
}

unsafe fn method(clsid: Guid, iid: Guid, slot: usize) -> (*mut c_void, usize) {
    let mut obj: *mut c_void = core::ptr::null_mut();
    assert_eq!(gdk::query_api_impl(&clsid, &iid, &mut obj), 0);
    let vt = *(obj as *const *const usize);
    (obj, *vt.add(slot))
}

unsafe fn wait(b: &mut Block) -> i32 {
    xasync::get_status(b as *mut Block as *mut c_void, true)
}

#[test]
fn async_families_on_the_real_pc_runtime() {
    let root = std::env::temp_dir().join(format!("tombox_gdk_real_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("TOMBOX_GDK_ROOT", &root);
    std::env::set_var("TOMBOX_GDK_XASYNC", "real");
    unsafe {
        let name: Vec<u16> = "xgameruntime.dll".encode_utf16().chain(Some(0)).collect();
        let h = LoadLibraryExW(name.as_ptr(), core::ptr::null_mut(), 0x800);
        if h.is_null() {
            eprintln!("SKIP: no System32 xgameruntime.dll (Gaming Services not installed)");
            return;
        }
        gdk::hook::patch_all();
        let q = GetProcAddress(h, b"QueryApiImpl\0".as_ptr());
        assert_eq!(q as usize, gdk::query_api_impl as usize, "GetProcAddress interposer did not substitute QueryApiImpl");
        assert!(gdk::real_query().is_some());
        let init = GetProcAddress(h, b"InitializeApiImpl\0".as_ptr());
        assert!(!init.is_null());
        let init: unsafe extern "system" fn(u32, u32) -> i32 = core::mem::transmute(init);
        let hr = init(0x4a61, 0x2712);
        if hr < 0 {
            eprintln!("SKIP: real InitializeApiImpl failed hr={hr:#010x}");
            return;
        }
        assert!(xasync::available(), "real XAsync interface unavailable through forwarding");

        let (u, add) = method(gdk::user::CLSID, gdk::user::IID_V1, 7);
        let add: unsafe extern "system" fn(*mut c_void, u32, *mut Block) -> i32 = core::mem::transmute(add);
        let (_, add_res) = method(gdk::user::CLSID, gdk::user::IID_V1, 8);
        let add_res: unsafe extern "system" fn(*mut c_void, *mut Block, *mut usize) -> i32 = core::mem::transmute(add_res);
        let mut b = block();
        assert_eq!(add(u, 1, &mut *b), 0);
        assert_eq!(wait(&mut b), 0);
        let mut user = 0usize;
        assert_eq!(add_res(u, &mut *b, &mut user), 0);
        assert_eq!(user, gdk::user::handle());

        let (_, tok) = method(gdk::user::CLSID, gdk::user::IID_V1, 23);
        let tok: unsafe extern "system" fn(*mut c_void, usize, u32, *const u8, *const u8, usize, *const c_void, usize, *const c_void, *mut Block) -> i32 = core::mem::transmute(tok);
        let (_, tok_size) = method(gdk::user::CLSID, gdk::user::IID_V1, 24);
        let tok_size: unsafe extern "system" fn(*mut c_void, *mut Block, *mut usize) -> i32 = core::mem::transmute(tok_size);
        let (_, tok_res) = method(gdk::user::CLSID, gdk::user::IID_V1, 25);
        let tok_res: unsafe extern "system" fn(*mut c_void, *mut Block, usize, *mut u8, *mut *mut u8, *mut usize) -> i32 = core::mem::transmute(tok_res);
        let mut b = block();
        assert_eq!(tok(u, user, 0, b"GET\0".as_ptr(), b"https://example.invalid/\0".as_ptr(), 0, core::ptr::null(), 0, core::ptr::null(), &mut *b), 0);
        assert_eq!(wait(&mut b), 0);
        let mut n = 0usize;
        assert_eq!(tok_size(u, &mut *b, &mut n), 0);
        let mut buf = vec![0u8; n];
        let mut ptr: *mut u8 = core::ptr::null_mut();
        assert_eq!(tok_res(u, &mut *b, n, buf.as_mut_ptr(), &mut ptr, core::ptr::null_mut()), 0);
        let token_ptr = *(ptr.add(16) as *const *const i8);
        let token = std::ffi::CStr::from_ptr(token_ptr).to_string_lossy().into_owned();
        assert_eq!(token, gdk::user::user().token);

        let ui = gdk::ui::CLSID;
        let (g, text) = method(ui, gdk::ui::IID_V1, 17);
        let text: unsafe extern "system" fn(*mut c_void, *mut Block, *const u8, *const u8, *const u8, u32, u32) -> i32 = core::mem::transmute(text);
        let (_, text_size) = method(ui, gdk::ui::IID_V1, 18);
        let text_size: unsafe extern "system" fn(*mut c_void, *mut Block, *mut u32) -> i32 = core::mem::transmute(text_size);
        let (_, text_res) = method(ui, gdk::ui::IID_V1, 19);
        let text_res: unsafe extern "system" fn(*mut c_void, *mut Block, u32, *mut u8, *mut u32) -> i32 = core::mem::transmute(text_res);
        let mut b = block();
        let helper = answer(|h| {
            gdk::win::set_child_text(h, gdk::win::EDIT_ID, "Player One");
            gdk::win::press(h, gdk::win::BUTTON_ID);
        });
        assert_eq!(text(g, &mut *b, b"Name\0".as_ptr(), b"\0".as_ptr(), b"Player One\0".as_ptr(), 0, 6), 0);
        assert_eq!(wait(&mut b), 0);
        helper.join().unwrap();
        let mut tn = 0u32;
        assert_eq!(text_size(g, &mut *b, &mut tn), 0);
        let mut t = vec![0u8; tn as usize];
        let mut tused = [0u32, 0xdead_beef];
        assert_eq!(text_res(g, &mut *b, tn, t.as_mut_ptr(), tused.as_mut_ptr()), 0);
        assert_eq!(tused, [7, 0xdead_beef]);
        assert_eq!(&t, b"Player\0");

        let (_, msg) = method(ui, gdk::ui::IID_V1, 3);
        let msg: unsafe extern "system" fn(*mut c_void, *mut Block, *const u8, *const u8, *const u8, *const u8, *const u8, u32, u32) -> i32 = core::mem::transmute(msg);
        let (_, msg_res) = method(ui, gdk::ui::IID_V1, 4);
        let msg_res: unsafe extern "system" fn(*mut c_void, *mut Block, *mut u32) -> i32 = core::mem::transmute(msg_res);
        let mut b = block();
        let helper = answer(|h| gdk::win::press(h, gdk::win::BUTTON_ID + 1));
        assert_eq!(msg(g, &mut *b, b"Title\0".as_ptr(), b"Pick one\0".as_ptr(), b"Yes\0".as_ptr(), b"No\0".as_ptr(), b"\0".as_ptr(), 0, 1), 0);
        assert_eq!(wait(&mut b), 0);
        helper.join().unwrap();
        let mut button = 9u32;
        assert_eq!(msg_res(g, &mut *b, &mut button), 0);
        assert_eq!(button, 1);

        let (s, sec) = method(gdk::net::CLSID, gdk::net::CLSID, 11);
        let sec: unsafe extern "system" fn(*mut c_void, *const u16, *mut Block) -> i32 = core::mem::transmute(sec);
        let (_, sec_res) = method(gdk::net::CLSID, gdk::net::CLSID, 13);
        let sec_res: unsafe extern "system" fn(*mut c_void, *mut Block, usize, *mut usize, *mut u8, *mut *mut u8) -> i32 = core::mem::transmute(sec_res);
        let url: Vec<u16> = "https://example.invalid/".encode_utf16().chain(Some(0)).collect();
        let mut b = block();
        assert_eq!(sec(s, url.as_ptr(), &mut *b), 0);
        assert_eq!(wait(&mut b), 0);
        let mut info = [0u8; 24];
        let mut used = [0usize, 0x5a5a];
        let mut sp: [*mut u8; 2] = [core::ptr::null_mut(), 0x5a5a as *mut u8];
        assert_eq!(sec_res(s, &mut *b, info.len(), used.as_mut_ptr(), info.as_mut_ptr(), sp.as_mut_ptr()), 0);
        assert_eq!(used, [24, 0x5a5a]);
        assert_eq!(sp[0], info.as_mut_ptr());
        assert_eq!(sp[1] as usize, 0x5a5a);
        assert_eq!(u32::from_le_bytes(info[0..4].try_into().unwrap()), 0x2800);

        let c = save::CLSID;
        let (o, init_async) = method(c, c, 4);
        let init_async: unsafe extern "system" fn(*mut c_void, usize, *const u8, bool, *mut Block) -> i32 = core::mem::transmute(init_async);
        let (_, init_res) = method(c, c, 5);
        let init_res: unsafe extern "system" fn(*mut c_void, *mut Block, *mut usize) -> i32 = core::mem::transmute(init_res);
        let mut b = block();
        assert_eq!(init_async(o, user, b"00000000-0000-0000-0000-000061234567\0".as_ptr(), false, &mut *b), 0);
        assert_eq!(wait(&mut b), 0);
        let mut provider = 0usize;
        assert_eq!(init_res(o, &mut *b, &mut provider), 0);
        assert_ne!(provider, 0);
        let (_, create) = method(c, c, 16);
        let create: unsafe extern "system" fn(*mut c_void, usize, *const u8, *mut usize) -> i32 = core::mem::transmute(create);
        let mut cont = 0usize;
        assert_eq!(create(o, provider, b"Slot0\0".as_ptr(), &mut cont), 0);
        let (_, upd) = method(c, c, 23);
        let upd: unsafe extern "system" fn(*mut c_void, usize, *const u8, *mut usize) -> i32 = core::mem::transmute(upd);
        let mut up = 0usize;
        assert_eq!(upd(o, cont, core::ptr::null(), &mut up), 0);
        let (_, write) = method(c, c, 25);
        let write: unsafe extern "system" fn(*mut c_void, usize, *const u8, *const u8, usize) -> i32 = core::mem::transmute(write);
        assert_eq!(write(o, up, b"blob\0".as_ptr(), b"data!".as_ptr(), 5), 0);
        let (_, submit) = method(c, c, 27);
        let submit: unsafe extern "system" fn(*mut c_void, usize) -> i32 = core::mem::transmute(submit);
        assert_eq!(submit(o, up), 0);
        let dir = save::provider_dir(&gdk::title().key, gdk::user::user().xuid, "00000000-0000-0000-0000-000061234567");
        assert!(dir.starts_with(&root));
        let st = save::Store::open(dir).unwrap();
        assert_eq!(st.read_blob("Slot0", "blob").unwrap(), b"data!");
        let (_, close_p) = method(c, c, 6);
        let close_p: unsafe extern "system" fn(*mut c_void, usize) = core::mem::transmute(close_p);
        close_p(o, provider);

        let mut obj: *mut c_void = core::ptr::null_mut();
        let tq = xasync::XASYNC;
        assert_eq!(gdk::query_api_impl(&tq, &tq, &mut obj), 0, "XAsync/XTaskQueue must forward to the real runtime");
        assert!(!obj.is_null());
    }
    let _ = std::fs::remove_dir_all(&root);
}
