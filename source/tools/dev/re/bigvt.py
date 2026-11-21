import sys,pefile,struct
pe=pefile.PE(sys.argv[1],fast_load=True);img=pe.get_memory_mapped_image();b=pe.OPTIONAL_HEADER.ImageBase
slot=int(sys.argv[2],16)//8
t=[s for s in pe.sections if s.Name.startswith(b'.text')][0];lo=b+t.VirtualAddress;hi=lo+t.Misc_VirtualSize
r=[s for s in pe.sections if s.Name.startswith(b'.rdata')][0]
s0=r.VirtualAddress;s1=s0+r.Misc_VirtualSize
o=s0;run=0;start=s0
while o<s1:
    v=struct.unpack_from('<Q',img,o)[0]
    if lo<=v<hi:
        if run==0: start=o
        run+=1
    else:
        if run>slot:
            f=struct.unpack_from('<Q',img,start+slot*8)[0]-b
            print('vtable +0x%x entries %d slot->0x%x'%(start,run,f))
        run=0
    o+=8
