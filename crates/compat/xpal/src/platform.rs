use crate::host::Host;
use crate::xconfig::ConfigStore;
use compat_common::gdk::{hook, identity, setting};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub const XBOX_HV_SIGNATURE: u32 = 0x3123_6258;
pub const HYPERV_SIGNATURE: u32 = 0x3123_7648;
pub const PRODUCT_XBOX_GAMEOS: u32 = 0xC2;
pub const PRODUCT_XBOX_KEYSTONE: u32 = 0xC6;
pub const SYSTEM_TYPE_SCARLETT_MIN: u16 = 0x60;
pub const SYSTEM_TYPE_ANACONDA: u16 = 0x64;
pub const SYSTEM_TYPE_SCARLETT_DEVKIT: u16 = 0x68;
pub const SYSTEM_TYPE_DURANGO_X_DEVKIT: u16 = 0x58;
pub const VM_TYPE_GAME: u8 = 3;
pub const CAP_INTERNAL_DEVKIT: u32 = 3;
pub const CAP_EXTERNAL_DEVKIT: u32 = 127;
pub const SECTION_SIZE: usize = 0xC00;
pub const SECTION_CONSOLE_ID: usize = 0x10;
pub const SECTION_CONSOLE_ID_TAIL: usize = 0x20;
pub const SECTION_SERIAL: usize = 0x230;
pub const SECTION_FACTORY: usize = 0x400;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Regs {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
}

#[derive(Clone, Debug)]
pub struct Leaves {
    pub interface: Regs,
    pub caps: Regs,
    pub caps_vm2: Regs,
    pub caps_vm3: Regs,
    pub system: Regs,
    pub factory: Regs,
}

impl Leaves {
    pub fn cpuid(&self, leaf: u32) -> Regs {
        match leaf {
            0x4000_0001 => self.interface,
            0x4000_0007 => self.caps,
            0x4000_0008 => self.caps_vm2,
            0x4000_0009 => self.caps_vm3,
            0x4000_000D => self.system,
            0x4000_000E => self.factory,
            _ => Regs::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostOs {
    pub major: u8,
    pub minor: u8,
    pub build: u16,
    pub revision: u16,
    pub extra: [u8; 2],
}

pub struct Spec {
    pub system_type: u16,
    pub console_mode: u16,
    pub vm_type: u8,
    pub caps: Vec<u32>,
    pub caps_vm2: Vec<u32>,
    pub caps_vm3: Vec<u32>,
    pub factory: Vec<u32>,
    pub server_type: u8,
    pub host_os: HostOs,
    pub product: u32,
    pub os_build: u32,
    pub console_id: [u8; 17],
    pub serial: [u8; 12],
    pub section: bool,
    pub vmctrl: bool,
    pub xconfig: bool,
    pub config_file: Option<PathBuf>,
    pub dump_dir: PathBuf,
}

pub struct Platform {
    pub leaves: Leaves,
    pub product: u32,
    pub os_build: u32,
    pub xbox_hv: bool,
    pub internal_devkit: bool,
    pub external_devkit: bool,
    pub section: Option<Box<[u8]>>,
    pub vmctrl_present: bool,
    pub xconfig_present: bool,
    pub host: Host,
    pub config: ConfigStore,
}

fn set_bits(r: &mut Regs, bits: &[u32]) {
    for &b in bits {
        let b = b & 127;
        let w = match b / 32 {
            0 => &mut r.eax,
            1 => &mut r.ebx,
            2 => &mut r.ecx,
            _ => &mut r.edx,
        };
        *w |= 1 << (b % 32);
    }
}

pub fn leaves_of(s: &Spec) -> Leaves {
    let mut caps = Regs { ecx: s.vm_type as u32, ..Regs::default() };
    set_bits(&mut caps, &s.caps);
    let mut caps_vm2 = Regs::default();
    set_bits(&mut caps_vm2, &s.caps_vm2);
    let mut caps_vm3 = Regs { ecx: VM_TYPE_GAME as u32, ..Regs::default() };
    set_bits(&mut caps_vm3, &s.caps_vm3);
    let mut factory = Regs::default();
    for &b in &s.factory {
        let b = b & 63;
        if b < 32 {
            factory.eax |= 1 << b;
        } else {
            factory.ebx |= 1 << (b - 32);
        }
    }
    let h = s.host_os;
    let system = Regs {
        eax: h.build as u32 | (h.extra[0] as u32) << 16 | (h.minor as u32) << 24,
        ebx: h.major as u32 | (h.extra[1] as u32) << 8,
        ecx: s.system_type as u32 | (s.console_mode as u32) << 16,
        edx: h.revision as u32 | (s.server_type as u32) << 16,
    };
    Leaves { interface: Regs { eax: XBOX_HV_SIGNATURE, ..Regs::default() }, caps, caps_vm2, caps_vm3, system, factory }
}

pub fn section_of(id: &[u8; 17], serial: &[u8; 12]) -> Box<[u8]> {
    let mut s = vec![0u8; SECTION_SIZE].into_boxed_slice();
    s[SECTION_CONSOLE_ID..SECTION_CONSOLE_ID + 16].copy_from_slice(&id[..16]);
    s[SECTION_CONSOLE_ID_TAIL..SECTION_CONSOLE_ID_TAIL + 2].copy_from_slice(&(id[16] as u16).to_le_bytes());
    s[SECTION_SERIAL..SECTION_SERIAL + 12].copy_from_slice(serial);
    s
}

impl Platform {
    pub fn new(s: Spec) -> Platform {
        let leaves = leaves_of(&s);
        let xbox_hv = leaves.interface.eax == XBOX_HV_SIGNATURE;
        let l7 = leaves.caps;
        let (internal_devkit, external_devkit) = if xbox_hv { ((l7.eax >> 3) & 1 == 1, l7.edx >> 31 == 1) } else { (l7.eax & 3 == 3, l7.eax & 1 == 1) };
        Platform {
            product: s.product,
            os_build: s.os_build,
            xbox_hv,
            internal_devkit,
            external_devkit,
            section: if s.section { Some(section_of(&s.console_id, &s.serial)) } else { None },
            vmctrl_present: s.vmctrl,
            xconfig_present: s.xconfig,
            host: Host::new(s.dump_dir),
            config: ConfigStore::new(s.config_file),
            leaves,
        }
    }

    pub fn cpuid(&self, leaf: u32) -> Regs {
        self.leaves.cpuid(leaf)
    }
}

fn hex_setting(key: &str) -> Option<u32> {
    setting(key).and_then(|v| u32::from_str_radix(v.trim_start_matches("0x").trim_start_matches("0X"), 16).ok())
}

fn list_setting(key: &str) -> Vec<u32> {
    setting(key).map(|v| v.split(',').filter_map(|x| x.trim().parse::<u32>().ok()).collect()).unwrap_or_default()
}

fn flag_setting(key: &str) -> Option<bool> {
    setting(key).map(|v| matches!(v.as_str(), "1" | "true" | "on" | "yes"))
}

pub fn parse_host_os(v: &str) -> Option<HostOs> {
    let p: Vec<u32> = v.split('.').map(|x| x.trim().parse::<u32>()).collect::<Result<_, _>>().ok()?;
    if p.len() != 4 || p[0] > 0xff || p[1] > 0xff || p[2] > 0xffff || p[3] > 0xffff {
        return None;
    }
    Some(HostOs { major: p[0] as u8, minor: p[1] as u8, build: p[2] as u16, revision: p[3] as u16, extra: [0, 0] })
}

pub fn console_id_of(xuid: u64) -> [u8; 17] {
    let a = xuid.rotate_left(17) ^ 0x5842_4F58_454D_5500;
    let b = xuid.swap_bytes() ^ 0x4C41_5058_0000_0000;
    let mut id = [0u8; 17];
    id[..8].copy_from_slice(&a.to_le_bytes());
    id[8..16].copy_from_slice(&b.to_le_bytes());
    id[16] = (xuid & 0xff) as u8;
    id
}

pub fn serial_of(xuid: u64) -> [u8; 12] {
    let mut v = xuid.wrapping_mul(0x9E37_79B9_7F4A_7C15) % 1_000_000_000_000;
    let mut s = [b'0'; 12];
    for i in (0..12).rev() {
        s[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    s
}

pub fn spec_from_settings() -> Spec {
    let device = identity::device();
    let ident = hook::console_mode(&device).map(|m| m as u16).unwrap_or(SYSTEM_TYPE_ANACONDA);
    let system_type = hex_setting("xpal_system_type").map(|v| v as u16).unwrap_or(ident);
    let console_mode = hex_setting("xpal_console_mode").map(|v| v as u16).unwrap_or(system_type);
    let devkit = matches!(system_type, SYSTEM_TYPE_SCARLETT_DEVKIT | SYSTEM_TYPE_DURANGO_X_DEVKIT);
    let mut caps = list_setting("xpal_caps");
    if flag_setting("xpal_internal_devkit").unwrap_or(false) {
        caps.push(CAP_INTERNAL_DEVKIT);
    }
    if flag_setting("xpal_external_devkit").unwrap_or(devkit) {
        caps.push(CAP_EXTERNAL_DEVKIT);
    }
    let os_build = setting("xpal_os_build").and_then(|v| v.parse().ok()).unwrap_or_else(compat_common::gameinput_ext::default_os_build);
    let detected = compat_common::title_os::title_os_version().filter(|v| v[2] as u32 == os_build);
    let host_os = setting("xpal_host_os").and_then(|v| parse_host_os(&v)).unwrap_or(match detected {
        Some(v) => HostOs { major: v[0] as u8, minor: v[1] as u8, build: v[2], revision: v[3], extra: [0, 0] },
        None => HostOs { major: 10, minor: 0, build: os_build.min(0xffff) as u16, revision: 0, extra: [0, 0] },
    });
    let xuid = compat_common::gdk::user::user().xuid;
    let root = compat_common::gdk::data_root().join("xpal");
    Spec {
        system_type,
        console_mode,
        vm_type: hex_setting("xpal_vm_type").map(|v| v as u8).unwrap_or(VM_TYPE_GAME),
        caps_vm3: caps.clone(),
        caps,
        caps_vm2: list_setting("xpal_caps_vm2"),
        factory: list_setting("xpal_factory"),
        server_type: hex_setting("xpal_server_type").map(|v| v as u8).unwrap_or(0),
        host_os,
        product: hex_setting("xpal_product").unwrap_or(PRODUCT_XBOX_GAMEOS),
        os_build,
        console_id: console_id_of(xuid),
        serial: setting("xpal_serial").filter(|s| s.len() == 12 && s.bytes().all(|c| c.is_ascii_digit())).map(|s| s.as_bytes().try_into().unwrap()).unwrap_or_else(|| serial_of(xuid)),
        section: flag_setting("xpal_section").unwrap_or(true),
        vmctrl: true,
        xconfig: true,
        config_file: Some(root.join("xconfig.bin")),
        dump_dir: root.join("dumps"),
    }
}

static PLATFORM: RwLock<Option<Arc<Platform>>> = RwLock::new(None);

pub fn platform() -> Arc<Platform> {
    if let Some(p) = PLATFORM.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return p.clone();
    }
    let mut w = PLATFORM.write().unwrap_or_else(|e| e.into_inner());
    w.get_or_insert_with(|| Arc::new(Platform::new(spec_from_settings()))).clone()
}

pub fn install(p: Platform) -> Arc<Platform> {
    let a = Arc::new(p);
    *PLATFORM.write().unwrap_or_else(|e| e.into_inner()) = Some(a.clone());
    a
}
