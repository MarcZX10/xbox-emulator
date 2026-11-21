#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Guid {
    pub d1: u32,
    pub d2: u16,
    pub d3: u16,
    pub d4: [u8; 8],
}

const fn hex(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => panic!("bad guid hex digit"),
    }
}

const fn field(s: &[u8], at: usize, len: usize) -> u64 {
    let mut v = 0u64;
    let mut i = 0;
    while i < len {
        v = (v << 4) | hex(s[at + i]) as u64;
        i += 1;
    }
    v
}

pub const fn guid(text: &str) -> Guid {
    let s = text.as_bytes();
    assert!(s.len() == 36 && s[8] == b'-' && s[13] == b'-' && s[18] == b'-' && s[23] == b'-');
    let mut d4 = [0u8; 8];
    d4[0] = field(s, 19, 2) as u8;
    d4[1] = field(s, 21, 2) as u8;
    let mut i = 0;
    while i < 6 {
        d4[2 + i] = field(s, 24 + i * 2, 2) as u8;
        i += 1;
    }
    Guid { d1: field(s, 0, 8) as u32, d2: field(s, 9, 4) as u16, d3: field(s, 14, 4) as u16, d4 }
}

impl Guid {
    pub fn bytes_le(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(&self.d1.to_le_bytes());
        b[4..6].copy_from_slice(&self.d2.to_le_bytes());
        b[6..8].copy_from_slice(&self.d3.to_le_bytes());
        b[8..16].copy_from_slice(&self.d4);
        b
    }

    pub unsafe fn read(p: *const core::ffi::c_void) -> Option<Guid> {
        if p.is_null() {
            None
        } else {
            Some(core::ptr::read_unaligned(p as *const Guid))
        }
    }
}

impl core::fmt::Display for Guid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let d = &self.d4;
        write!(
            f,
            "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
            self.d1, self.d2, self.d3, d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
        )
    }
}

pub const IID_IUNKNOWN: Guid = guid("00000000-0000-0000-C000-000000000046");
pub const IID_IMALLOC: Guid = guid("00000002-0000-0000-C000-000000000046");
pub const IID_INOMARSHAL: Guid = guid("ECC8691B-C1DB-4DC0-855E-65F6C551AF49");

pub const CLSID_DXC_COMPILER: Guid = guid("73E22D93-E6CE-47F3-B5BF-F0664F39C1B0");
pub const CLSID_DXC_LINKER: Guid = guid("EF6A8087-B0EA-4D56-9E45-D07E1A8B7806");
pub const CLSID_DXC_DIA_DATA_SOURCE: Guid = guid("CD1F6B73-2AB0-484D-8EDC-EBE7A43CA09F");
pub const CLSID_DXC_COMPILER_ARGS: Guid = guid("3E56AE82-224D-470F-A1A1-FE3016EE9F9D");
pub const CLSID_DXC_LIBRARY: Guid = guid("6245D6AF-66E0-48FD-80B4-4D271796748C");
pub const CLSID_DXC_UTILS: Guid = CLSID_DXC_LIBRARY;
pub const CLSID_DXC_VALIDATOR: Guid = guid("8CA3E215-F728-4CF3-8CDD-88AF917587A1");
pub const CLSID_DXC_ASSEMBLER: Guid = guid("D728DB68-F903-4F80-94CD-DCCF76EC7151");
pub const CLSID_DXC_CONTAINER_REFLECTION: Guid = guid("B9F54489-55B8-400C-BA3A-1675E4728B91");
pub const CLSID_DXC_OPTIMIZER: Guid = guid("AE2CD79F-CC22-453F-9B6B-B124E7A5204C");
pub const CLSID_DXC_CONTAINER_BUILDER: Guid = guid("94134294-411F-4574-B4D0-8741E25240D2");
pub const CLSID_DXC_INTELLISENSE: Guid = guid("3047833C-D1C0-4B8E-9D40-102878605985");
pub const CLSID_DXC_REWRITER: Guid = guid("B489B951-E07F-40B3-968D-93E124734DA4");
pub const CLSID_DXC_PDB_UTILS: Guid = guid("54621DFB-F2CE-457E-AE8C-EC355FAEEC7C");

pub const CLSID_XBOX_DXC_CONTAINER: Guid = guid("B3D47A7F-0970-414A-900B-BB140101EC66");
pub const CLSID_XBOX_DXC_SHADER_STATS: Guid = guid("EC07AED8-07D5-4B40-9458-C3EB2A80C01A");
pub const CLSID_XBOX_DXC_BACKEND_DEBUG_INFO: Guid = guid("439EE57D-768C-4A3F-AB56-EDEDC26C293F");
pub const CLSID_XBOX_DXC_REG_PRESSURE: Guid = guid("ED5F5B28-9F1B-4580-AB5C-4DCDA20F0C7C");
pub const CLSID_XDXR_COMPILER: Guid = guid("16BB3C85-006D-4B72-9E10-63CD97DF57F0");

pub const IID_IDXC_BLOB: Guid = guid("8BA5FB08-5195-40E2-AC58-0D989C3A0102");
pub const IID_IDXC_BLOB_ENCODING: Guid = guid("7241D424-2646-4191-97C0-98E96E42FC68");
pub const IID_IDXC_BLOB_UTF8: Guid = guid("3DA636C9-BA71-4024-A301-30CBF125305B");
pub const IID_IDXC_BLOB_UTF16: Guid = guid("A3F84EAB-0FAA-497E-A39C-EE6ED60B2D84");
pub const IID_IDXC_LIBRARY: Guid = guid("E5204DC7-D18C-4C3C-BDFB-851673980FE7");
pub const IID_IDXC_UTILS: Guid = guid("4605C4CB-2019-492A-ADA4-65F20BB7D67F");
pub const IID_IDXC_COMPILER: Guid = guid("8C210BF3-011F-4422-8D70-6F9ACB8DB617");
pub const IID_IDXC_COMPILER2: Guid = guid("A005A9D9-B8BB-4594-B5C9-0E633BEC4D37");
pub const IID_IDXC_COMPILER3: Guid = guid("228B4687-5A6A-4730-900C-9702B2203F54");
pub const IID_IDXC_VALIDATOR: Guid = guid("A6E82BD2-1FD7-4826-9811-2857E797F49A");
pub const IID_IDXC_CONTAINER_REFLECTION: Guid = guid("D2C21B26-8350-4BDC-976A-331CE6F4C54C");
pub const IID_IDXC_CONTAINER_BUILDER: Guid = guid("334B1F50-2292-4B35-99A1-25588D8C17FE");
pub const IID_IDXC_LINKER: Guid = guid("F1B5BE2A-62DD-4327-A1C2-42AC1E1E78E6");
pub const IID_IDXC_OPERATION_RESULT: Guid = guid("CEDB484A-D4E9-445A-B991-CA21CA157DC2");
pub const IID_IDXC_RESULT: Guid = guid("58346CDA-DDE7-4497-9461-6F87AF5E0659");
pub const IID_IDXC_OPTIMIZER: Guid = guid("25740E2E-9CBA-401B-9119-4FB42F39F270");
pub const IID_IDXC_ASSEMBLER: Guid = guid("091F7A26-1C1F-4948-904B-E6E3A8A771D5");
pub const IID_IDXC_REWRITER: Guid = guid("C012115B-8893-4EB9-9C5A-111456EA1C45");
pub const IID_IDXC_INTELLISENSE: Guid = guid("B1F99513-46D6-4112-8169-DD0D6053F17D");
pub const IID_IDXC_COMPILER_ARGS: Guid = guid("73EFFE2A-70DC-45F8-9690-EFF64C02429D");
pub const IID_IDXC_VERSION_INFO: Guid = guid("B04F5B50-2059-4F12-A8FF-A1E0CDE1CC7E");
pub const IID_IDXC_INCLUDE_HANDLER: Guid = guid("7F61FC7D-950D-467F-B3E3-3C02FB49187C");
pub const IID_IDXC_PDB_UTILS: Guid = guid("E6C9647E-9D6A-4C3B-B94C-524B5A6C343D");

pub const IID_IXBOX_DXC_CONTAINER: Guid = guid("7F3D5E18-D4F0-4BA1-AAC1-CD5C5DA4EBBF");
pub const IID_IXBOX_DXC_SHADER_STATS: Guid = guid("1F0D2587-018C-4FB9-BC94-5F269802AAE5");
pub const IID_IXBOX_DXC_BACKEND_DEBUG_INFO: Guid = guid("B3998EF9-0357-49CD-9636-3E3FD4760500");
pub const IID_IXBOX_DXC_REG_PRESSURE: Guid = guid("096867DF-4DB0-4DEA-818F-9FC2842FC3F3");
pub const IID_IXDXR_COMPILER: Guid = CLSID_XDXR_COMPILER;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Pc,
    XboxOnly,
}

pub const CLASSES: &[(Guid, &str, Kind)] = &[
    (CLSID_DXC_COMPILER, "CLSID_DxcCompiler", Kind::Pc),
    (CLSID_DXC_LINKER, "CLSID_DxcLinker", Kind::Pc),
    (CLSID_DXC_DIA_DATA_SOURCE, "CLSID_DxcDiaDataSource", Kind::Pc),
    (CLSID_DXC_COMPILER_ARGS, "CLSID_DxcCompilerArgs", Kind::Pc),
    (CLSID_DXC_LIBRARY, "CLSID_DxcLibrary/CLSID_DxcUtils", Kind::Pc),
    (CLSID_DXC_VALIDATOR, "CLSID_DxcValidator", Kind::Pc),
    (CLSID_DXC_ASSEMBLER, "CLSID_DxcAssembler", Kind::Pc),
    (CLSID_DXC_CONTAINER_REFLECTION, "CLSID_DxcContainerReflection", Kind::Pc),
    (CLSID_DXC_OPTIMIZER, "CLSID_DxcOptimizer", Kind::Pc),
    (CLSID_DXC_CONTAINER_BUILDER, "CLSID_DxcContainerBuilder", Kind::Pc),
    (CLSID_DXC_INTELLISENSE, "CLSID_DxcIntelliSense", Kind::Pc),
    (CLSID_DXC_REWRITER, "CLSID_DxcRewriter", Kind::Pc),
    (CLSID_DXC_PDB_UTILS, "CLSID_DxcPdbUtils", Kind::Pc),
    (CLSID_XBOX_DXC_CONTAINER, "CLSID_DxcXboxContainer", Kind::XboxOnly),
    (CLSID_XBOX_DXC_SHADER_STATS, "CLSID_DxcXboxShaderStats", Kind::XboxOnly),
    (CLSID_XBOX_DXC_BACKEND_DEBUG_INFO, "CLSID_DxcXboxBackendDebugInfo", Kind::XboxOnly),
    (CLSID_XBOX_DXC_REG_PRESSURE, "CLSID_DxcXboxRegPressure", Kind::XboxOnly),
    (CLSID_XDXR_COMPILER, "CLSID_XdxrCompiler", Kind::XboxOnly),
];

pub const INTERFACES: &[(Guid, &str, Kind)] = &[
    (IID_IUNKNOWN, "IUnknown", Kind::Pc),
    (IID_IMALLOC, "IMalloc", Kind::Pc),
    (IID_INOMARSHAL, "INoMarshal", Kind::Pc),
    (IID_IDXC_BLOB, "IDxcBlob", Kind::Pc),
    (IID_IDXC_BLOB_ENCODING, "IDxcBlobEncoding", Kind::Pc),
    (IID_IDXC_BLOB_UTF8, "IDxcBlobUtf8", Kind::Pc),
    (IID_IDXC_BLOB_UTF16, "IDxcBlobUtf16", Kind::Pc),
    (IID_IDXC_LIBRARY, "IDxcLibrary", Kind::Pc),
    (IID_IDXC_UTILS, "IDxcUtils", Kind::Pc),
    (IID_IDXC_COMPILER, "IDxcCompiler", Kind::Pc),
    (IID_IDXC_COMPILER2, "IDxcCompiler2", Kind::Pc),
    (IID_IDXC_COMPILER3, "IDxcCompiler3", Kind::Pc),
    (IID_IDXC_VALIDATOR, "IDxcValidator", Kind::Pc),
    (IID_IDXC_CONTAINER_REFLECTION, "IDxcContainerReflection", Kind::Pc),
    (IID_IDXC_CONTAINER_BUILDER, "IDxcContainerBuilder", Kind::Pc),
    (IID_IDXC_LINKER, "IDxcLinker", Kind::Pc),
    (IID_IDXC_OPERATION_RESULT, "IDxcOperationResult", Kind::Pc),
    (IID_IDXC_RESULT, "IDxcResult", Kind::Pc),
    (IID_IDXC_OPTIMIZER, "IDxcOptimizer", Kind::Pc),
    (IID_IDXC_ASSEMBLER, "IDxcAssembler", Kind::Pc),
    (IID_IDXC_REWRITER, "IDxcRewriter", Kind::Pc),
    (IID_IDXC_INTELLISENSE, "IDxcIntelliSense", Kind::Pc),
    (IID_IDXC_COMPILER_ARGS, "IDxcCompilerArgs", Kind::Pc),
    (IID_IDXC_VERSION_INFO, "IDxcVersionInfo", Kind::Pc),
    (IID_IDXC_INCLUDE_HANDLER, "IDxcIncludeHandler", Kind::Pc),
    (IID_IDXC_PDB_UTILS, "IDxcPdbUtils", Kind::Pc),
    (IID_IXBOX_DXC_CONTAINER, "IDxcXboxContainer", Kind::XboxOnly),
    (IID_IXBOX_DXC_SHADER_STATS, "IDxcXboxShaderStats", Kind::XboxOnly),
    (IID_IXBOX_DXC_BACKEND_DEBUG_INFO, "IDxcXboxBackendDebugInfo", Kind::XboxOnly),
    (IID_IXBOX_DXC_REG_PRESSURE, "IDxcXboxRegPressure", Kind::XboxOnly),
    (IID_IXDXR_COMPILER, "IXdxrCompiler", Kind::XboxOnly),
];

fn find(table: &'static [(Guid, &'static str, Kind)], g: &Guid) -> Option<(&'static str, Kind)> {
    table.iter().find(|e| e.0 == *g).map(|e| (e.1, e.2))
}

pub fn class(g: &Guid) -> Option<(&'static str, Kind)> {
    find(CLASSES, g)
}

pub fn interface(g: &Guid) -> Option<(&'static str, Kind)> {
    find(INTERFACES, g)
}

pub fn describe(g: Option<Guid>, class_table: bool) -> String {
    match g {
        None => "(null)".to_string(),
        Some(g) => {
            let n = if class_table { class(&g) } else { interface(&g) };
            match n {
                Some((name, Kind::XboxOnly)) => format!("{name} [xbox-only] {g}"),
                Some((name, Kind::Pc)) => format!("{name} {g}"),
                None => format!("unknown {g}"),
            }
        }
    }
}
