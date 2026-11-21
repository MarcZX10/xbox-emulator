import ctypes, sys
from ctypes import wintypes as W

k32 = ctypes.WinDLL('kernel32', use_last_error=True)
dbg = ctypes.WinDLL('dbghelp', use_last_error=True)
psapi = ctypes.WinDLL('psapi', use_last_error=True)
pid = int(sys.argv[1])
want = set(int(x) for x in sys.argv[2:])

k32.OpenProcess.restype = W.HANDLE
k32.OpenThread.restype = W.HANDLE
hp = k32.OpenProcess(0x0410 | 0x0008, False, pid)
assert hp, ctypes.get_last_error()


class ADDRESS64(ctypes.Structure):
    _fields_ = [('Offset', ctypes.c_uint64), ('Segment', W.WORD), ('Mode', ctypes.c_int)]


class KDHELP64(ctypes.Structure):
    _fields_ = [('a', ctypes.c_uint64 * 20)]


class STACKFRAME64(ctypes.Structure):
    _fields_ = [('AddrPC', ADDRESS64), ('AddrReturn', ADDRESS64), ('AddrFrame', ADDRESS64), ('AddrStack', ADDRESS64), ('AddrBStore', ADDRESS64), ('FuncTableEntry', ctypes.c_void_p), ('Params', ctypes.c_uint64 * 4), ('Far', W.BOOL), ('Virtual', W.BOOL), ('Reserved', ctypes.c_uint64 * 3), ('KdHelp', KDHELP64)]


dbg.SymSetOptions(0x2 | 0x10)
dbg.SymInitializeW.argtypes = [W.HANDLE, W.LPCWSTR, W.BOOL]
dbg.SymInitializeW(hp, None, True)
dbg.SymFunctionTableAccess64.restype = ctypes.c_void_p
dbg.SymFunctionTableAccess64.argtypes = [W.HANDLE, ctypes.c_uint64]
dbg.SymGetModuleBase64.restype = ctypes.c_uint64
dbg.SymGetModuleBase64.argtypes = [W.HANDLE, ctypes.c_uint64]
FTA = ctypes.WINFUNCTYPE(ctypes.c_void_p, W.HANDLE, ctypes.c_uint64)
GMB = ctypes.WINFUNCTYPE(ctypes.c_uint64, W.HANDLE, ctypes.c_uint64)
fta = ctypes.cast(dbg.SymFunctionTableAccess64, FTA)
gmb = ctypes.cast(dbg.SymGetModuleBase64, GMB)
dbg.StackWalk64.argtypes = [W.DWORD, W.HANDLE, W.HANDLE, ctypes.POINTER(STACKFRAME64), ctypes.c_void_p, ctypes.c_void_p, FTA, GMB, ctypes.c_void_p]

mods = []
arr = (W.HMODULE * 2048)()
need = W.DWORD()
psapi.EnumProcessModulesEx(hp, arr, ctypes.sizeof(arr), ctypes.byref(need), 3)
for i in range(need.value // 8):
    h = arr[i]
    if not h:
        continue
    nm = ctypes.create_unicode_buffer(260)
    psapi.GetModuleBaseNameW(hp, W.HMODULE(h), nm, 260)

    class MI(ctypes.Structure):
        _fields_ = [('base', ctypes.c_void_p), ('size', W.DWORD), ('entry', ctypes.c_void_p)]
    mi = MI()
    psapi.GetModuleInformation(hp, W.HMODULE(h), ctypes.byref(mi), ctypes.sizeof(mi))
    mods.append((mi.base, mi.size, nm.value))


def where(a):
    for b, s, n in mods:
        if b <= a < b + s:
            return '%s +0x%x' % (n, a - b)
    return hex(a)


TH32 = ctypes.WinDLL('kernel32')


class TE32(ctypes.Structure):
    _fields_ = [('dwSize', W.DWORD), ('cntUsage', W.DWORD), ('th32ThreadID', W.DWORD), ('th32OwnerProcessID', W.DWORD), ('tpBasePri', W.LONG), ('tpDeltaPri', W.LONG), ('dwFlags', W.DWORD)]


k32.CreateToolhelp32Snapshot.restype = W.HANDLE
snap = k32.CreateToolhelp32Snapshot(4, 0)
te = TE32()
te.dwSize = ctypes.sizeof(TE32)
tids = []
ok = k32.Thread32First(snap, ctypes.byref(te))
while ok:
    if te.th32OwnerProcessID == pid:
        tids.append(te.th32ThreadID)
    ok = k32.Thread32Next(snap, ctypes.byref(te))
for tid in tids:
    if want and tid not in want:
        continue
    ht = k32.OpenThread(0x0002 | 0x0008 | 0x0010 | 0x0040, False, tid)
    if not ht:
        continue
    k32.SuspendThread(W.HANDLE(ht))
    buf = (ctypes.c_byte * (1232 + 16))()
    addr = (ctypes.addressof(buf) + 15) & ~15
    ctx = (ctypes.c_byte * 1232).from_address(addr)
    ctypes.c_uint32.from_address(addr + 0x30).value = 0x10000B
    if k32.GetThreadContext(W.HANDLE(ht), ctypes.c_void_p(addr)):
        rip = ctypes.c_uint64.from_address(addr + 0xF8).value
        rsp = ctypes.c_uint64.from_address(addr + 0x98).value
        rbp = ctypes.c_uint64.from_address(addr + 0xA0).value
        sf = STACKFRAME64()
        sf.AddrPC.Offset, sf.AddrPC.Mode = rip, 3
        sf.AddrStack.Offset, sf.AddrStack.Mode = rsp, 3
        sf.AddrFrame.Offset, sf.AddrFrame.Mode = rbp, 3
        frames = []
        for _ in range(64):
            if not dbg.StackWalk64(0x8664, hp, W.HANDLE(ht), ctypes.byref(sf), ctypes.c_void_p(addr), None, fta, gmb, None):
                break
            if sf.AddrPC.Offset == 0:
                break
            frames.append(where(sf.AddrPC.Offset))
        print('== tid=%d' % tid)
        for f in frames:
            print('   ', f)
    k32.ResumeThread(W.HANDLE(ht))
    k32.CloseHandle(W.HANDLE(ht))
