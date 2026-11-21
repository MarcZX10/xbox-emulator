import ctypes, os, sys, uuid

USAGE = "xsc.py <dump System32 dir> <src.hlsl> <out.dxbc> -T <profile> -E <entry> [dxc args...]"


def guid(s):
    return (ctypes.c_ubyte * 16).from_buffer_copy(uuid.UUID(s).bytes_le)


def vt(o, i, res, *at):
    f = ctypes.cast(ctypes.cast(o, ctypes.POINTER(ctypes.c_void_p))[0], ctypes.POINTER(ctypes.c_void_p))[i]
    return ctypes.WINFUNCTYPE(res, ctypes.c_void_p, *at)(f)


def blob(o):
    p = vt(o, 3, ctypes.c_void_p)(o)
    n = vt(o, 4, ctypes.c_size_t)(o)
    return ctypes.string_at(p, n) if p else b""


class Buf(ctypes.Structure):
    _fields_ = [("p", ctypes.c_void_p), ("n", ctypes.c_size_t), ("e", ctypes.c_uint)]


def main():
    if len(sys.argv) < 4:
        sys.exit(USAGE)
    sysdir, src_path, out_path, args = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4:]
    k = ctypes.WinDLL("kernel32", use_last_error=True)
    k.LoadLibraryExW.restype = ctypes.c_void_p
    for dep in ("xbsc_xs.dll", "newbe_xs.dll", "scdxil.dll", "sc_dll.dll", "dxcompiler_xs.dll"):
        if not k.LoadLibraryExW(os.path.join(sysdir, dep), None, 0x8):
            sys.exit(f"load {dep}: error {ctypes.get_last_error()}")
    m = ctypes.WinDLL(os.path.join(sysdir, "dxcompiler_xs.dll"))
    comp = ctypes.c_void_p()
    hr = m.DxcCreateInstance(ctypes.byref(guid("73e22d93-e6ce-47f3-b5bf-f0664f39c1b0")), ctypes.byref(guid("228B4687-5A6A-4730-900C-9702B2203F54")), ctypes.byref(comp))
    if hr:
        sys.exit(f"DxcCreateInstance {hr & 0xffffffff:#x}")
    src = open(src_path, "rb").read()
    sb = ctypes.create_string_buffer(src)
    b = Buf(ctypes.cast(sb, ctypes.c_void_p), len(src), 0)
    argv = (ctypes.c_wchar_p * len(args))(*args)
    res = ctypes.c_void_p()
    hr = vt(comp, 3, ctypes.c_long, ctypes.POINTER(Buf), ctypes.c_void_p, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p))(
        comp, ctypes.byref(b), argv, len(args), None, ctypes.byref(guid("58346CDA-DDE7-4497-9461-6F87AF5E0659")), ctypes.byref(res))
    if hr:
        sys.exit(f"Compile {hr & 0xffffffff:#x}")
    st = ctypes.c_long()
    vt(res, 3, ctypes.c_long, ctypes.POINTER(ctypes.c_long))(res, ctypes.byref(st))
    err = ctypes.c_void_p()
    vt(res, 5, ctypes.c_long, ctypes.POINTER(ctypes.c_void_p))(res, ctypes.byref(err))
    msg = blob(err).decode(errors="replace").strip() if err.value else ""
    if st.value:
        sys.exit(f"status {st.value & 0xffffffff:#x}\n{msg}")
    out = ctypes.c_void_p()
    vt(res, 4, ctypes.c_long, ctypes.POINTER(ctypes.c_void_p))(res, ctypes.byref(out))
    data = blob(out)
    open(out_path, "wb").write(data)
    print(f"{out_path}: {len(data)} bytes, XDAT={b'XDAT' in data}")


main()
