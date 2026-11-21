import ctypes, sys, re
from ctypes import wintypes

dbg = ctypes.WinDLL('dbghelp.dll', use_last_error=True)
proc = wintypes.HANDLE(ctypes.windll.kernel32.GetCurrentProcess())
dbg.SymSetOptions(0x2 | 0x10 | 0x200)
dbg.SymInitializeW.restype = wintypes.BOOL
dbg.SymInitializeW.argtypes = [wintypes.HANDLE, wintypes.LPCWSTR, wintypes.BOOL]
dbg.SymLoadModuleExW.restype = ctypes.c_uint64
dbg.SymLoadModuleExW.argtypes = [wintypes.HANDLE, wintypes.HANDLE, wintypes.LPCWSTR, wintypes.LPCWSTR, ctypes.c_uint64, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD]


class SYMBOL_INFOW(ctypes.Structure):
    _fields_ = [('SizeOfStruct', wintypes.ULONG), ('TypeIndex', wintypes.ULONG), ('Reserved', ctypes.c_uint64 * 2), ('Index', wintypes.ULONG), ('Size', wintypes.ULONG), ('ModBase', ctypes.c_uint64), ('Flags', wintypes.ULONG), ('Value', ctypes.c_uint64), ('Address', ctypes.c_uint64), ('Register', wintypes.ULONG), ('Scope', wintypes.ULONG), ('Tag', wintypes.ULONG), ('NameLen', wintypes.ULONG), ('MaxNameLen', wintypes.ULONG), ('Name', wintypes.WCHAR * 512)]


dbg.SymFromAddrW.argtypes = [wintypes.HANDLE, ctypes.c_uint64, ctypes.POINTER(ctypes.c_uint64), ctypes.POINTER(SYMBOL_INFOW)]
mods = {}
args = sys.argv[1:]
search = ';'.join({a.rsplit('\\', 1)[0] for a in args})
dbg.SymInitializeW(proc, search, False)
base = 0x1000000000
for a in args:
    name = a.rsplit('\\', 1)[-1].lower()
    b = dbg.SymLoadModuleExW(proc, None, a, None, base, 0x4000000, None, 0)
    if not b:
        print('load fail', a, ctypes.get_last_error(), file=sys.stderr)
    mods[name] = base
    base += 0x100000000
for line in sys.stdin:
    m = re.search(r'([\w.\-]+\.(?:exe|dll)) \+0x([0-9a-f]+)', line)
    out = line.rstrip()
    if m and m.group(1).lower() in mods:
        si = SYMBOL_INFOW()
        si.SizeOfStruct = 88
        si.MaxNameLen = 511
        disp = ctypes.c_uint64(0)
        if dbg.SymFromAddrW(proc, mods[m.group(1).lower()] + int(m.group(2), 16), ctypes.byref(disp), ctypes.byref(si)):
            out += '  ' + si.Name[:160] + '+0x%x' % disp.value
    print(out)
