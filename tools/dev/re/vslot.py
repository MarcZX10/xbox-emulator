import ctypes,sys,struct
from ctypes import wintypes as W
k32=ctypes.WinDLL('kernel32',use_last_error=True);ps=ctypes.WinDLL('psapi')
k32.OpenProcess.restype=W.HANDLE
pid=int(sys.argv[1]);hp=k32.OpenProcess(0x0410,False,pid)
def rd(a,n):
    b=ctypes.create_string_buffer(n);g=ctypes.c_size_t()
    k32.ReadProcessMemory(W.HANDLE(hp),ctypes.c_void_p(a),b,n,ctypes.byref(g));return b.raw[:g.value]
q=lambda a:struct.unpack('<Q',rd(a,8))[0]
arr=(W.HMODULE*2048)();need=W.DWORD()
ps.EnumProcessModulesEx(W.HANDLE(hp),arr,ctypes.sizeof(arr),ctypes.byref(need),3)
mods=[]
class MI(ctypes.Structure): _fields_=[('base',ctypes.c_void_p),('size',W.DWORD),('entry',ctypes.c_void_p)]
for i in range(need.value//8):
    nm=ctypes.create_unicode_buffer(260);ps.GetModuleBaseNameW(W.HANDLE(hp),W.HMODULE(arr[i]),nm,260)
    mi=MI();ps.GetModuleInformation(W.HANDLE(hp),W.HMODULE(arr[i]),ctypes.byref(mi),ctypes.sizeof(mi));mods.append((mi.base,mi.size,nm.value))
def where(a):
    for b,s,n in mods:
        if b<=a<b+s: return '%s+0x%x'%(n,a-b)
    return hex(a)
base=[m for m in mods if m[2].lower()=='terraria.exe'][0][0]
g=int(sys.argv[2],16);o=q(base+g);vt=q(o)
print('obj %x vtable %s'%(o,where(vt)))
for s in sys.argv[3:]:
    f=q(vt+int(s,16));print(' slot %s -> %s'%(s,where(f)))
