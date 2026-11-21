import ctypes,sys,struct
from ctypes import wintypes as W
k32=ctypes.WinDLL('kernel32',use_last_error=True);ps=ctypes.WinDLL('psapi')
k32.OpenProcess.restype=W.HANDLE
pid=int(sys.argv[1]);hp=k32.OpenProcess(0x0410,False,pid)
def rd(a,n):
    b=ctypes.create_string_buffer(n);g=ctypes.c_size_t()
    k32.ReadProcessMemory(W.HANDLE(hp),ctypes.c_void_p(a),b,n,ctypes.byref(g));return b.raw[:g.value]
q=lambda a:struct.unpack('<Q',rd(a,8))[0]
d=lambda a:struct.unpack('<i',rd(a,4))[0]
arr=(W.HMODULE*2048)();need=W.DWORD()
ps.EnumProcessModulesEx(W.HANDLE(hp),arr,ctypes.sizeof(arr),ctypes.byref(need),3)
base=None
for i in range(need.value//8):
    nm=ctypes.create_unicode_buffer(260);ps.GetModuleBaseNameW(W.HANDLE(hp),W.HMODULE(arr[i]),nm,260)
    if nm.value.lower()=='terraria.exe': base=arr[i]
o=q(base+0x1891b28)
print('base %x aum %x'%(base,o))
print('pending@2530',d(o+0x2530),'cfgMB@2534',d(o+0x2534),'persist@2538',rd(o+0x2538,1).hex(),'bufsize@253c',d(o+0x253c),'resizing@1870',rd(o+0x1870,1).hex(),'req@1874',d(o+0x1874))
print('cur item @28 %x'%q(o+0x28))
it=q(o+0x28)
if it:
    r=q(it+8); print(' item req %x size@38 %d @54 %d @58 %x name %r'%(r,d(r+0x38),d(r+0x54),q(r+0x58),rd(q(r+0x28),120).split(b'\0')[0] if q(r+0x28) else b''))
rb=q(o+0x1858)
if rb: print('ring %x head %x tail %x data %x size %d'%(rb,q(rb),q(rb+8),q(rb+0x10),d(rb+0x18)))
st=[d(o+0x80+i*0xc0) for i in range(32)]
print('slot states',st)
for i in range(32):
    if st[i]!=3:
        s=o+0x58+i*0xc0
        print(' slot',i,'state',st[i],'raw',rd(s,0xc0).hex())
        break
