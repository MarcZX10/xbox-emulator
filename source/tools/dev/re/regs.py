import ctypes,sys,struct
from ctypes import wintypes as W
k32=ctypes.WinDLL('kernel32',use_last_error=True);ps=ctypes.WinDLL('psapi')
k32.OpenProcess.restype=W.HANDLE;k32.OpenThread.restype=W.HANDLE
pid=int(sys.argv[1]);tid=int(sys.argv[2]);hp=k32.OpenProcess(0x0410,False,pid)
def rd(a,n):
    b=ctypes.create_string_buffer(n);g=ctypes.c_size_t()
    k32.ReadProcessMemory(W.HANDLE(hp),ctypes.c_void_p(a),b,n,ctypes.byref(g));return b.raw[:g.value]
ht=k32.OpenThread(0x0008|0x0002|0x0040,False,tid)
k32.SuspendThread(W.HANDLE(ht))
buf=(ctypes.c_byte*(1232+16))();addr=(ctypes.addressof(buf)+15)&~15
ctypes.c_uint32.from_address(addr+0x30).value=0x10000B
k32.GetThreadContext(W.HANDLE(ht),ctypes.c_void_p(addr))
k32.ResumeThread(W.HANDLE(ht))
names=['rax','rcx','rdx','rbx','rsp','rbp','rsi','rdi','r8','r9','r10','r11','r12','r13','r14','r15']
regs={n:ctypes.c_uint64.from_address(addr+0x78+i*8).value for i,n in enumerate(names)}
for n in names: print(n,hex(regs[n]))
rsp=regs['rsp']
st=rd(rsp,0x400)
arr=(W.HMODULE*2048)();need=W.DWORD()
ps.EnumProcessModulesEx(W.HANDLE(hp),arr,ctypes.sizeof(arr),ctypes.byref(need),3)
for i in range(need.value//8):
    nm=ctypes.create_unicode_buffer(260);ps.GetModuleBaseNameW(W.HANDLE(hp),W.HMODULE(arr[i]),nm,260)
    if nm.value.lower()=='terraria.exe': base=arr[i]
for i in range(0,len(st),8):
    v=struct.unpack_from('<Q',st,i)[0]
    if base<=v<base+0x2000000: print('stk+%x -> Terraria+0x%x'%(i,v-base))
for r in sys.argv[3:]:
    a=regs[r] if r in regs else int(r,16)
    m=rd(a+0xb0,0x70)
    print(r,'%x'%a,'+e8..',rd(a+0xe8,2).hex(),'+70',rd(a+0x70,8).hex(),'+b8',m[8:16].hex(),'+c8',m[0x18:0x20].hex(),'+d8',m[0x28:0x38].hex(),'+110',rd(a+0x110,8).hex())
