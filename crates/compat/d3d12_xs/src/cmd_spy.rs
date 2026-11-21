use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::cell::RefCell;
use compat_common::record;

const N: usize = 128;
const KEEP: usize = 256;
static ORIG: [AtomicUsize; N] = [const { AtomicUsize::new(0) }; N];
static ON: AtomicBool = AtomicBool::new(false);
static EXE: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static RING: RefCell<Vec<(u32, usize, u64, u64, u64, String)>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "system" fn spy_log(slot: u64, caller: usize, a1: u64, a2: u64, a3: u64) -> usize {
    RING.with(|r| {
        let mut r = r.borrow_mut();
        if slot == 10 {
            r.clear();
        }
        if r.len() >= KEEP {
            r.remove(0);
        }
        r.push((slot as u32, caller.wrapping_sub(EXE.load(Ordering::Relaxed)), a1, a2, a3, detail(slot, a1, a2)));
    });
    ORIG[(slot as usize).min(N - 1)].load(Ordering::Relaxed)
}

fn detail(slot: u64, a1: u64, a2: u64) -> String {
    if slot != 26 || a2 == 0 || a1 == 0 || a1 > 16 {
        return String::new();
    }
    let mut out = String::new();
    for i in 0..a1 as usize {
        let b = (a2 as usize + i * 32) as *const u32;
        let w: Vec<u32> = (0..8).map(|k| unsafe { b.add(k).read_unaligned() }).collect();
        out.push_str(&format!("[type={} flags={:#x} res={:#x} sub={:#x} before={:#x} after={:#x}] ", w[0], w[1], w[2] as u64 | (w[3] as u64) << 32, w[4], w[5], w[6]));
    }
    out
}

macro_rules! spies {
    ($(($name:ident, $slot:literal)),* $(,)?) => {
        $(
            #[unsafe(naked)]
            unsafe extern "system" fn $name() {
                core::arch::naked_asm!(
                    "push rcx",
                    "push rdx",
                    "push r8",
                    "push r9",
                    "sub rsp, 0x78",
                    "movdqu [rsp+0x30], xmm0",
                    "movdqu [rsp+0x40], xmm1",
                    "movdqu [rsp+0x50], xmm2",
                    "movdqu [rsp+0x60], xmm3",
                    "mov rax, [rsp+0x78]",
                    "mov [rsp+0x20], rax",
                    "mov r9, [rsp+0x80]",
                    "mov r8, [rsp+0x88]",
                    "mov rdx, [rsp+0x98]",
                    "mov ecx, {s}",
                    "call {f}",
                    "movdqu xmm0, [rsp+0x30]",
                    "movdqu xmm1, [rsp+0x40]",
                    "movdqu xmm2, [rsp+0x50]",
                    "movdqu xmm3, [rsp+0x60]",
                    "add rsp, 0x78",
                    "pop r9",
                    "pop r8",
                    "pop rdx",
                    "pop rcx",
                    "jmp rax",
                    s = const $slot, f = sym spy_log,
                );
            }
        )*
        fn spies() -> Vec<usize> { vec![$($name as usize),*] }
    };
}

spies!(
    (s0,0),(s1,1),(s2,2),(s3,3),(s4,4),(s5,5),(s6,6),(s7,7),(s8,8),(s9,9),(s10,10),(s11,11),(s12,12),(s13,13),(s14,14),(s15,15),
    (s16,16),(s17,17),(s18,18),(s19,19),(s20,20),(s21,21),(s22,22),(s23,23),(s24,24),(s25,25),(s26,26),(s27,27),(s28,28),(s29,29),(s30,30),(s31,31),
    (s32,32),(s33,33),(s34,34),(s35,35),(s36,36),(s37,37),(s38,38),(s39,39),(s40,40),(s41,41),(s42,42),(s43,43),(s44,44),(s45,45),(s46,46),(s47,47),
    (s48,48),(s49,49),(s50,50),(s51,51),(s52,52),(s53,53),(s54,54),(s55,55),(s56,56),(s57,57),(s58,58),(s59,59),(s60,60),(s61,61),(s62,62),(s63,63),
    (s64,64),(s65,65),(s66,66),(s67,67),(s68,68),(s69,69),(s70,70),(s71,71),(s72,72),(s73,73),(s74,74),(s75,75),(s76,76),(s77,77),(s78,78),(s79,79),
    (s80,80),(s81,81),(s82,82),(s83,83),(s84,84),(s85,85),(s86,86),(s87,87),(s88,88),(s89,89),(s90,90),(s91,91),(s92,92),(s93,93),(s94,94),(s95,95),
    (s96,96),(s97,97),(s98,98),(s99,99),(s100,100),(s101,101),(s102,102),(s103,103),(s104,104),(s105,105),(s106,106),(s107,107),(s108,108),(s109,109),(s110,110),(s111,111),
    (s112,112),(s113,113),(s114,114),(s115,115),(s116,116),(s117,117),(s118,118),(s119,119),(s120,120),(s121,121),(s122,122),(s123,123),(s124,124),(s125,125),(s126,126),(s127,127),
);

pub fn install(vt: &[AtomicUsize]) {
    if std::env::var("TOMBOX_CMDLIST_SPY").is_err() || ON.swap(true, Ordering::SeqCst) {
        return;
    }
    extern "system" {
        fn GetModuleHandleW(n: *const u16) -> *mut c_void;
    }
    EXE.store(unsafe { GetModuleHandleW(core::ptr::null()) } as usize, Ordering::Relaxed);
    let spy = spies();
    for i in 0..N.min(vt.len()).min(spy.len()) {
        if i == 1 || i == 2 {
            continue;
        }
        ORIG[i].store(vt[i].load(Ordering::Relaxed), Ordering::Relaxed);
        vt[i].store(spy[i], Ordering::Relaxed);
    }
    record("d3d12_xs", "cmdspy", "command-list spy installed on Xbox slots 0 and 3..127");
}

pub fn dump(tag: &str) {
    if !ON.load(Ordering::Relaxed) {
        return;
    }
    RING.with(|r| {
        let r = r.borrow();
        record("d3d12_xs", tag, &format!("last {} commands on this thread since Reset (slot caller-RVA arg1 arg2 arg3):", r.len()));
        for (k, (s, c, a1, a2, a3, d)) in r.iter().enumerate() {
            record("d3d12_xs", tag, &format!("  #{k} slot={s} caller={c:#x} a1={a1:#x} a2={a2:#x} a3={a3:#x} {d}"));
        }
    });
}
