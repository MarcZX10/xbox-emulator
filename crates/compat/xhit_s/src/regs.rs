use crate::print;

pub const SNAP_CONSOLE: usize = 0;
pub const SNAP_SRBM2: usize = 4;
pub const SNAP_SRBM: usize = 8;
pub const SNAP_GRBM: usize = 0xec;
pub const SNAP_GRBM2: usize = 0xf0;
pub const SNAP_SE0: usize = 0xf4;
pub const SNAP_SE1: usize = 0xf8;
pub const SNAP_VCE_STATUS: usize = 0x3a8;
pub const SNAP_VCE_RPTR: usize = 0x3b4;
pub const SNAP_VCE_WPTR: usize = 0x3b8;
pub const OUT_IDLE: u64 = 1;
pub const OUT_SDMA1: u64 = 2;
pub const OUT_SDMA2: u64 = 4;
pub const OUT_SDMA3: u64 = 8;

const LEAD: &str = "The following blocks are not idle:\n";
const SEP: &str = ", ";

pub fn console_name(t: u32) -> &'static str {
    match t {
        1 => "Xbox One S",
        2 => "Xbox One X",
        _ => "Xbox One",
    }
}

pub struct BusyList {
    pub first: bool,
}

impl BusyList {
    pub fn new() -> Self {
        BusyList { first: true }
    }

    pub fn item(&mut self, name: &str) {
        let lead = if self.first { LEAD } else { SEP };
        print(0, &format!("{}{}", lead, name));
        self.first = false;
    }

    pub fn item_se(&mut self, name: &str, se0: u32, se1: u32) {
        let suffix = if se0 == u32::MAX || se1 == u32::MAX {
            ""
        } else if se0 != 0 {
            if se1 != 0 {
                " (SE0&1)"
            } else {
                " (SE0)"
            }
        } else if se1 != 0 {
            " (SE1)"
        } else {
            ""
        };
        let lead = if self.first { LEAD } else { SEP };
        print(0, &format!("{}{}{}", lead, name, suffix));
        self.first = false;
    }
}

impl Default for BusyList {
    fn default() -> Self {
        Self::new()
    }
}

unsafe fn u(p: *const u8, off: usize) -> u32 {
    (p.add(off) as *const u32).read_unaligned()
}

fn bit(v: u32, n: u32) -> bool {
    (v >> n) & 1 != 0
}

pub unsafe fn decode_registers(snap: *const u8, out: *mut u64, _sq: *const u8, _sq_len: u32) -> bool {
    if out.is_null() {
        return false;
    }
    print(0, &format!("Capture was taken from an {} console.\n", console_name(u(snap, SNAP_CONSOLE))));
    if crate::verbosity() < 1 {
        return true;
    }
    let g = u(snap, SNAP_GRBM);
    let g2 = u(snap, SNAP_GRBM2);
    let s0 = u(snap, SNAP_SE0);
    let s1 = u(snap, SNAP_SE1);
    let mut l = BusyList::new();
    let se = |n: u32| ((s0 >> n) & 1, (s1 >> n) & 1);
    if bit(g2, 24) {
        l.item("RLC");
    }
    if bit(g, 29) {
        l.item("CP");
    }
    if bit(g, 28) {
        l.item("COHER");
    }
    if bit(g2, 30) {
        l.item("CPG");
    }
    if bit(g2, 28) {
        l.item("CPF");
    }
    if bit(g2, 29) {
        l.item("CPC");
    }
    if g & 0x21_0000 == 0x21_0000 {
        l.item("WD(DMA+DRAW)");
    }
    if bit(g, 16) && !bit(g, 21) {
        l.item("WD(DRAW, not DMA??)");
    }
    if !bit(g, 16) && bit(g, 21) {
        l.item("WD(DMA only)");
    }
    if g & 0xc_0000 == 0xc_0000 {
        l.item("IA(DMA+DRAW)");
    }
    if bit(g, 18) && !bit(g, 19) {
        l.item("IA(DRAW, not DMA??)");
    }
    if !bit(g, 18) && bit(g, 19) {
        l.item("IA(DMA only)");
    }
    let blocks: [(u32, &str, u32); 5] = [(17, "VGT", 23), (22, "SPI", 27), (25, "PA", 24), (24, "SC", 29), (14, "TA", 25)];
    for (gb, name, sb) in blocks {
        if bit(g, gb) {
            let (a, b) = se(sb);
            l.item_se(name, a, b);
        }
    }
    if bit(g2, 25) {
        l.item("TC");
    }
    if bit(g, 15) {
        l.item("GDS");
    }
    for (gb, name, sb) in [(20u32, "SX", 26u32), (23, "BCI", 22)] {
        if bit(g, gb) {
            let (a, b) = se(sb);
            l.item_se(name, a, b);
        }
    }
    if bit(g, 30) {
        l.item_se("CB", s0 >> 31, s1 >> 31);
    }
    if !bit(g, 13) {
        l.item_se("CB!Clean", !(s0 >> 2) & 1, !(s1 >> 2) & 1);
    }
    if bit(g, 26) {
        let (a, b) = se(30);
        l.item_se("DB", a, b);
    }
    if !bit(g, 12) {
        l.item_se("DB!Clean", !(s0 >> 1) & 1, !(s1 >> 1) & 1);
    }
    if bit(g, 31) {
        l.item("GUI");
    }
    if l.first {
        *out |= OUT_IDLE;
    }
    let s8 = u(snap, SNAP_SRBM);
    let s4 = u(snap, SNAP_SRBM2);
    for (b, name) in [(5u32, "SDMA"), (6, "SDMA1"), (10, "SDMA2"), (11, "SDMA3"), (12, "SAMSCP")] {
        if bit(s8, b) {
            l.item(name);
        }
    }
    for (b, name) in [(20u32, "SAMMSP"), (19, "UVD")] {
        if bit(s4, b) {
            l.item(name);
        }
    }
    let s8q = s8 as u64;
    *out = (*out & !(OUT_SDMA1 | OUT_SDMA2 | OUT_SDMA3)) | ((s8q >> 5) & OUT_SDMA1) | ((s8q >> 8) & (OUT_SDMA2 | OUT_SDMA3));
    if bit(s8, 7) {
        let hung = u(snap, SNAP_VCE_STATUS) & 0x30 != 0 || u(snap, SNAP_VCE_WPTR) != u(snap, SNAP_VCE_RPTR);
        l.item(if hung { "VCE(hung)" } else { "VCE(auto-mode, not hung)" });
    }
    for (b, name) in [(21u32, "VCN"), (8, "VMC"), (9, "MCB"), (10, "MCB_NON_DISPLAY"), (11, "MCC"), (12, "MCD"), (13, "VMC1"), (14, "SEM"), (17, "(IH)")] {
        if bit(s4, b) {
            l.item(name);
        }
    }
    print(0, "\n\n");
    true
}
