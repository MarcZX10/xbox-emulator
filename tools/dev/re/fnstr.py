import sys, struct, pefile, capstone

pe = pefile.PE(sys.argv[1], fast_load=True)
pe.parse_data_directories(directories=[3])
img = pe.get_memory_mapped_image()
base = pe.OPTIONAL_HEADER.ImageBase
funcs = []
for e in getattr(pe, 'DIRECTORY_ENTRY_EXCEPTION', []):
    funcs.append((e.struct.BeginAddress, e.struct.EndAddress))
funcs.sort()
import bisect
keys = [f[0] for f in funcs]
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
md.detail = False


def sstr(rva):
    if rva <= 0 or rva >= len(img) - 4:
        return None
    b = img[rva:rva + 120]
    e = b.find(b'\0')
    if e >= 4 and all(32 <= c < 127 for c in b[:e]):
        return b[:e].decode()
    w = b[:240]
    try:
        t = img[rva:rva + 240].decode('utf-16-le').split('\0')[0]
        if len(t) >= 4 and all(32 <= ord(c) < 127 for c in t):
            return 'L"' + t + '"'
    except Exception:
        pass
    return None


for a in sys.argv[2:]:
    r = int(a, 16)
    i = bisect.bisect_right(keys, r) - 1
    while i >= 0 and funcs[i][1] <= r:
        i -= 1
    if i < 0:
        print(a, 'no func')
        continue
    s, e = funcs[i]
    code = img[s:e]
    strs = []
    calls = []
    for ins in md.disasm(code, base + s):
        if 'rip' in ins.op_str and ins.mnemonic in ('lea', 'mov'):
            try:
                disp = int(ins.op_str.split('rip')[1].split(']')[0].replace(' ', ''), 16)
            except Exception:
                continue
            t = ins.address + ins.size + disp - base
            st = sstr(t)
            if st:
                strs.append(st[:90])
    print('+0x%x in func +0x%x..+0x%x (%d bytes): %s' % (r, s, e, e - s, ' | '.join(dict.fromkeys(strs))[:900]))
