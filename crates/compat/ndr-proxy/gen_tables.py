import sys, os, struct, pefile, capstone

src, out_bin, out_rs = sys.argv[1], sys.argv[2], sys.argv[3]
pe = pefile.PE(src)
img = pe.get_memory_mapped_image()
base = pe.OPTIONAL_HEADER.ImageBase
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)

iat = {}
for d in pe.DIRECTORY_ENTRY_IMPORT:
    for i in d.imports:
        iat[i.address - base] = (d.dll.decode(), i.name.decode())

sec = {s.Name.rstrip(b"\0").decode(): s for s in pe.sections}
rdata, data, text = sec[".rdata"], sec[".data"], sec[".text"]
lo = rdata.VirtualAddress
hi = data.VirtualAddress + data.Misc_VirtualSize
blob = bytes(img[lo:hi])


def q(r):
    return struct.unpack_from("<Q", img, r)[0]


def p(r):
    v = q(r)
    return v - base if v else 0


def rip_target(ins):
    disp = int(ins.op_str.split("rip")[1].split("]")[0].replace(" ", ""), 16)
    return ins.address + ins.size + disp - base


exports = {s.name.decode(): s.address for s in pe.DIRECTORY_ENTRY_EXPORT.symbols}
gpi = exports["GetProxyDllInfo"]
leas = [rip_target(i) for i in md.disasm(img[gpi:gpi + 24], base + gpi) if i.mnemonic == "lea"]
pfl, clsid = leas[0], leas[1]
dgc = exports["DllGetClassObject"]
factory = [rip_target(i) for i in md.disasm(img[dgc:dgc + 40], base + dgc) if i.mnemonic == "lea"][0]
if not (data.VirtualAddress <= factory < data.VirtualAddress + data.Misc_VirtualSize):
    raise SystemExit("factory buffer is not in .data")
for blk in pe.DIRECTORY_ENTRY_BASERELOC:
    for e in blk.entries:
        if e.type == 10 and factory <= q(e.rva) - base < factory + 0x20:
            raise SystemExit("a table points into the factory buffer")

files = []
k = 0
while p(pfl + 8 * k):
    fi = p(pfl + 8 * k)
    files.append((fi, p(fi + 32), struct.unpack_from("<H", img, fi + 40)[0]))
    k += 1
lookup_index = {lk: n for n, (fi, lk, ts) in enumerate(files)}

dlls = []
fixes = []
code_kinds = {}
for blk in pe.DIRECTORY_ENTRY_BASERELOC:
    for e in blk.entries:
        if e.type != 10:
            continue
        t = q(e.rva) - base
        if not (lo <= e.rva < hi):
            raise SystemExit(f"reloc slot outside blob {e.rva:#x}")
        off = e.rva - lo
        if lo <= t < hi:
            fixes.append((off, f"Fix::Data({t - lo:#x})"))
            continue
        if not (text.VirtualAddress <= t < text.VirtualAddress + text.Misc_VirtualSize):
            raise SystemExit(f"reloc target outside image sections {t:#x}")
        ins = list(md.disasm(img[t:t + 32], base + t))
        a = ins[0]
        if t in lookup_index:
            kind = f"Fix::Lookup({lookup_index[t]})"
        elif a.mnemonic == "jmp" and "rip" in a.op_str:
            dll, name = iat[rip_target(a)]
            if dll not in dlls:
                dlls.append(dll)
            kind = f"Fix::Import({dlls.index(dll)}, {name!r})".replace("'", '"')
        elif a.mnemonic == "lea" and "rip" in a.op_str and rip_target(a) == factory and ins[1].mnemonic == "jmp":
            name = iat[rip_target(ins[1])][1]
            kind = {"NdrCStdStubBuffer_Release": "Fix::StubRelease", "NdrCStdStubBuffer2_Release": "Fix::StubRelease2"}[name]
        elif a.mnemonic == "ret" or (a.mnemonic == "jmp" and a.op_str == "rax") or (a.mnemonic == "sub" and ins[1].op_str == "ecx, 0x100"):
            kind = "Fix::Runtime"
        else:
            raise SystemExit(f"unclassified code target {t:#x} {a.mnemonic} {a.op_str}")
        code_kinds[t] = kind
        fixes.append((off, kind))

runtime = [t for t, k in code_kinds.items() if k == "Fix::Runtime"]
lc = pe.OPTIONAL_HEADER.DATA_DIRECTORY[10]
pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG"]])
lcs = pe.DIRECTORY_ENTRY_LOAD_CONFIG.struct
guard_slots = {lcs.GuardCFCheckFunctionPointer - base, lcs.GuardCFDispatchFunctionPointer - base}
for off, k in fixes:
    if k == "Fix::Runtime":
        r = off + lo
        if r not in guard_slots and not (q(r - 8) == 0 and q(r + 8) == 0):
            raise SystemExit(f"runtime code pointer at {r:#x} is neither a CFG slot nor a CRT initializer table entry")

fixes.sort()
open(out_bin, "wb").write(blob)
rs = []
rs.append(f"pub const SOURCE_RVA: u32 = {lo:#x};")
rs.append(f"pub const PROXY_FILE_LIST: usize = {pfl - lo:#x};")
rs.append(f"pub const CLSID: usize = {clsid - lo:#x};")
rs.append(f"pub const FACTORY: usize = {factory - lo:#x};")
rs.append(f"pub const RDATA_LEN: usize = {rdata.Misc_VirtualSize:#x};")
rs.append(f"pub const FILE_COUNT: usize = {len(files)};")
rs.append("pub static FILES: &[usize] = &[" + ", ".join(f"{fi - lo:#x}" for fi, _, _ in files) + "];")
rs.append("pub static DLLS: &[&str] = &[" + ", ".join(f'"{d}"' for d in dlls) + "];")
rs.append("pub static FIXUPS: &[(usize, Fix)] = &[")
for off, k in fixes:
    rs.append(f"    ({off:#x}, {k}),")
rs.append("];")
open(out_rs, "w", newline="\n").write("use ndr_proxy::Fix;\n\n" + "\n".join(rs) + "\n")
print(os.path.basename(src), "blob", len(blob), "fixups", len(fixes), "files", len(files), "dlls", dlls,
      "runtime", [hex(t) for t in runtime], "loadcfg", hex(lc.VirtualAddress))
