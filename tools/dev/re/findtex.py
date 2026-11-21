import ctypes,sys,struct
from ctypes import wintypes as W
k32=ctypes.WinDLL('kernel32',use_last_error=True)
k32.OpenProcess.restype=W.HANDLE
pid=int(sys.argv[1]);rsp=int(sys.argv[2],16);hp=k32.OpenProcess(0x0410,False,pid)
def rd(a,n):
    b=ctypes.create_string_buffer(n);g=ctypes.c_size_t()
    k32.ReadProcessMemory(W.HANDLE(hp),ctypes.c_void_p(a),b,n,ctypes.byref(g));return b.raw[:g.value]
st=rd(rsp,0x200)
seen=set()
for i in range(0,len(st),8):
    v=struct.unpack_from('<Q',st,i)[0]
    if v in seen or v<0x10000 or v>0x7fffffffffff: continue
    seen.add(v)
    m=rd(v,0x120)
    if len(m)<0x120: continue
    print('stk+%03x %x e8=%02x e9=%02x b0=%02x a0=%x b8=%x c8=%x d8=%x 110=%x 50=%x'%(i,v,m[0xe8],m[0xe9],m[0xb0],struct.unpack_from('<Q',m,0xa0)[0],struct.unpack_from('<Q',m,0xb8)[0],struct.unpack_from('<Q',m,0xc8)[0],struct.unpack_from('<Q',m,0xd8)[0],struct.unpack_from('<Q',m,0x110)[0],struct.unpack_from('<I',m,0x50)[0]))
