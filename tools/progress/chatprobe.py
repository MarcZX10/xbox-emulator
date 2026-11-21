import argparse, ctypes, json, os, queue, shutil, subprocess, sys, tempfile, threading, time
from ctypes import wintypes as wt

FORTNITE_IMPORTS = [
    "GameChatFailFastWithInform", "ChatManagerInitialize", "ChatManagerCleanup", "ChatManagerSetAudioEncodingBitrate",
    "ChatManagerStartProcessingDataFrames", "ChatManagerFinishProcessingDataFrames", "ChatManagerAddLocalUser",
    "ChatManagerAddRemoteUser", "ChatManagerRemoveUser", "ChatManagerProcessIncomingPacket", "ChatUserSetMicrophoneMuted",
    "ChatUserGetRemoteUserMuted", "ChatUserSetRemoteUserMuted", "ChatUserGetIsLocal",
]
PEERS = {"A": ("2535405290000001", 1), "B": ("2535405290000002", 2)}
MAX_USERS = 20
RELATIONSHIP_SEND_MIC_RECEIVE_AUDIO = 9
RESTRICTIVE, STT_AUTOMATIC, MANIP_NONE, BITRATE_24K = 1, 0, 0, 2


class DataFrame(ctypes.Structure):
    _fields_ = [("targetEndpointCount", ctypes.c_uint32), ("targetEndpoints", ctypes.POINTER(ctypes.c_uint64)),
                ("transportRequirement", ctypes.c_int32), ("packetByteCount", ctypes.c_uint32),
                ("packetBuffer", ctypes.POINTER(ctypes.c_uint8))]


def emit(step, **kw):
    print(json.dumps({"step": step, **kw}), flush=True)


def hx(hr):
    return "%#010x" % (hr & 0xffffffff)


def bind(gc):
    H = ctypes.c_void_p
    PF = ctypes.POINTER(ctypes.POINTER(DataFrame))
    sig = {
        "ChatManagerInitialize": [ctypes.c_uint32, ctypes.c_float, ctypes.c_int32, ctypes.c_int32, ctypes.c_int32, ctypes.c_int32],
        "ChatManagerSetAudioEncodingBitrate": [ctypes.c_int32],
        "ChatManagerAddLocalUser": [wt.LPCWSTR, ctypes.POINTER(H)],
        "ChatManagerAddRemoteUser": [wt.LPCWSTR, ctypes.c_uint64, ctypes.POINTER(H)],
        "ChatManagerRemoveUser": [H],
        "ChatUserGetIsLocal": [H, ctypes.POINTER(ctypes.c_bool)],
        "ChatUserSetMicrophoneMuted": [H, ctypes.c_bool],
        "ChatUserSetRemoteUserMuted": [H, H, ctypes.c_bool],
        "ChatUserGetRemoteUserMuted": [H, H, ctypes.POINTER(ctypes.c_bool)],
        "ChatManagerStartProcessingDataFrames": [ctypes.POINTER(ctypes.c_uint32), ctypes.POINTER(PF)],
        "ChatManagerFinishProcessingDataFrames": [PF],
        "ChatManagerProcessIncomingPacket": [ctypes.c_uint64, ctypes.c_uint32, ctypes.c_void_p],
        "ChatManagerCleanup": [],
    }
    for n, args in sig.items():
        f = getattr(gc, n)
        f.argtypes = args
        f.restype = ctypes.c_long
    return H, PF


def child(d, peer):
    d3d = ctypes.WinDLL(os.path.join(d, "d3d12_xs.dll"))
    d3d.D3D12XboxSetProcessDebugFlags.argtypes = [ctypes.c_uint32]
    d3d.D3D12XboxSetProcessDebugFlags(0)
    ws = ctypes.WinDLL("ws2_32")
    res = ctypes.c_void_p()
    blocked = ws.getaddrinfo(b"privacy.xboxlive.com", b"443", None, ctypes.byref(res)) != 0
    emit("net_guard", blocked=blocked)
    if not blocked:
        emit("abort", reason="network policy not active; refusing to load GameChat2 with internet access")
        return 3
    gc = ctypes.WinDLL(os.path.join(d, "GameChat2.dll"))
    k32 = ctypes.WinDLL("kernel32")
    k32.GetProcAddress.restype = ctypes.c_void_p
    k32.GetProcAddress.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
    resolved = [f for f in FORTNITE_IMPORTS if k32.GetProcAddress(gc._handle, f.encode())]
    emit("resolve", resolved=resolved, unresolved=[f for f in FORTNITE_IMPORTS if f not in resolved])
    H, PF = bind(gc)
    ok = True

    def step(name, hr, **kw):
        nonlocal ok
        ok = ok and hr >= 0
        emit(name, hr=hx(hr), ok=hr >= 0, **kw)
        return hr >= 0

    def quiet(name, hr):
        nonlocal ok
        if hr < 0:
            ok = False
            emit(name, hr=hx(hr), ok=False)
        return hr >= 0

    args = [MAX_USERS, 1.0, RELATIONSHIP_SEND_MIC_RECEIVE_AUDIO, RESTRICTIVE, STT_AUTOMATIC, MANIP_NONE]
    if not step("ChatManagerInitialize", gc.ChatManagerInitialize(*args), args=args):
        return 1
    step("ChatManagerSetAudioEncodingBitrate", gc.ChatManagerSetAudioEncodingBitrate(BITRATE_24K))
    me, other = PEERS[peer], PEERS["B" if peer == "A" else "A"]
    remote_ep = other[1]
    local, remote = H(), H()
    step("ChatManagerAddLocalUser", gc.ChatManagerAddLocalUser(me[0], ctypes.byref(local)), xuid=me[0])
    step("ChatManagerAddRemoteUser", gc.ChatManagerAddRemoteUser(other[0], remote_ep, ctypes.byref(remote)), xuid=other[0], endpoint=remote_ep)
    if not (local.value and remote.value):
        gc.ChatManagerCleanup()
        return 1
    il, ir = ctypes.c_bool(), ctypes.c_bool(True)
    step("ChatUserGetIsLocal", gc.ChatUserGetIsLocal(local, ctypes.byref(il)), value=il.value)
    step("ChatUserGetIsLocal", gc.ChatUserGetIsLocal(remote, ctypes.byref(ir)), value=ir.value)
    ok = ok and il.value and not ir.value
    step("ChatUserSetMicrophoneMuted", gc.ChatUserSetMicrophoneMuted(local, False))
    inbox = queue.Queue()

    def reader():
        for line in sys.stdin:
            try:
                inbox.put(bytes.fromhex(json.loads(line)["packet"]))
            except Exception:
                pass
    threading.Thread(target=reader, daemon=True).start()
    counts = {"sent": 0, "received": 0}

    def pump(seconds):
        t = time.time()
        while time.time() - t < seconds:
            n, arr = ctypes.c_uint32(), PF()
            if not quiet("ChatManagerStartProcessingDataFrames", gc.ChatManagerStartProcessingDataFrames(ctypes.byref(n), ctypes.byref(arr))):
                return
            for i in range(n.value):
                f = arr[i].contents
                eps = [f.targetEndpoints[j] for j in range(f.targetEndpointCount)]
                print(json.dumps({"to": eps, "transport": f.transportRequirement, "packet": bytes(f.packetBuffer[:f.packetByteCount]).hex()}), flush=True)
                counts["sent"] += 1
            if not quiet("ChatManagerFinishProcessingDataFrames", gc.ChatManagerFinishProcessingDataFrames(arr)):
                return
            while not inbox.empty():
                pkt = inbox.get()
                buf = ctypes.create_string_buffer(pkt, len(pkt))
                if not quiet("ChatManagerProcessIncomingPacket", gc.ChatManagerProcessIncomingPacket(remote_ep, len(pkt), buf)):
                    return
                counts["received"] += 1
            time.sleep(0.02)

    pump(4.0)
    emit("traffic", **counts)
    ok = ok and counts["sent"] > 0 and counts["received"] > 0
    m = ctypes.c_bool()
    step("ChatUserSetRemoteUserMuted", gc.ChatUserSetRemoteUserMuted(local, remote, True))
    step("ChatUserGetRemoteUserMuted", gc.ChatUserGetRemoteUserMuted(local, remote, ctypes.byref(m)), muted=m.value)
    ok = ok and m.value
    step("ChatUserSetRemoteUserMuted", gc.ChatUserSetRemoteUserMuted(local, remote, False))
    step("ChatUserGetRemoteUserMuted", gc.ChatUserGetRemoteUserMuted(local, remote, ctypes.byref(m)), muted=m.value)
    ok = ok and not m.value
    pump(1.0)
    step("ChatManagerRemoveUser", gc.ChatManagerRemoveUser(remote), which="remote")
    pump(0.5)
    step("ChatManagerRemoveUser", gc.ChatManagerRemoveUser(local), which="local")
    pump(0.5)
    step("ChatManagerCleanup", gc.ChatManagerCleanup())
    emit("done", ok=ok, **counts)
    return 0 if ok else 1


def main():
    ap = argparse.ArgumentParser(description="Load Fortnite's own GameChat2.dll (read-only copy) in two peer processes on this PC with the emulator's AudioHALWrapper and network policy, relay their data frames to each other, run the 14 functions Fortnite imports, and write chatprobe.json only if every step succeeds in both peers.")
    ap.add_argument("--child")
    ap.add_argument("--peer", default="A")
    ap.add_argument("--title-dll", default=r"D:\xbox-pc-port\games\Fortnite\extracted\Mount\FortniteGame\Binaries\XSX\GameChat2.dll")
    ap.add_argument("--target", default=r"D:\xbox-pc-port\emulator\target-sysfn\debug")
    ap.add_argument("--out", default=r"D:\xbox-pc-port\emulator\analysis\progress_evidence\chatprobe.json")
    ap.add_argument("--log", default=os.path.join(tempfile.gettempdir(), "chatprobe_last.jsonl"))
    a = ap.parse_args()
    if a.child:
        sys.exit(child(a.child, a.peer))
    d = tempfile.mkdtemp(prefix="chatprobe_")
    shutil.copyfile(a.title_dll, os.path.join(d, "GameChat2.dll"))
    for n in ("AudioHALWrapper.dll", "d3d12_xs.dll"):
        shutil.copyfile(os.path.join(a.target, n), os.path.join(d, n))
    env = dict(os.environ)
    env.pop("TOMBOX_NET_REDIRECT", None)
    env.pop("TOMBOX_ALLOW_NETWORK", None)
    procs, results = {}, {}
    for peer in PEERS:
        e = dict(env, TOMBOX_LOG=os.path.join(d, "tombox_compat_%s.log" % peer))
        procs[peer] = subprocess.Popen([sys.executable, "-I", os.path.abspath(__file__), "--child", d, "--peer", peer], cwd=d, env=e,
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
    relayed = {p: 0 for p in PEERS}

    def relay(src):
        dst = "B" if src == "A" else "A"
        steps = results.setdefault(src, [])
        for line in procs[src].stdout:
            if not line.startswith("{"):
                continue
            msg = json.loads(line)
            if "packet" not in msg:
                steps.append(msg)
            elif PEERS[dst][1] in msg["to"]:
                try:
                    procs[dst].stdin.write(json.dumps({"packet": msg["packet"]}) + "\n")
                    procs[dst].stdin.flush()
                    relayed[src] += 1
                except Exception:
                    pass
    ts = [threading.Thread(target=relay, args=(p,), daemon=True) for p in procs]
    for t in ts:
        t.start()
    codes = {}
    for peer, pr in procs.items():
        try:
            codes[peer] = pr.wait(timeout=120)
        except subprocess.TimeoutExpired:
            pr.kill()
            codes[peer] = "timeout"
    for t in ts:
        t.join(timeout=5)
    errs = {peer: pr.stderr.read()[-2000:] for peer, pr in procs.items()}
    steps = [dict(s, peer=peer) for peer in PEERS for s in results.get(peer, [])]
    compat = {}
    for peer in PEERS:
        lp = os.path.join(d, "tombox_compat_%s.log" % peer)
        compat[peer] = open(lp, encoding="utf-8", errors="replace").read()[-1500:] if os.path.isfile(lp) else ""
    with open(a.log, "w", encoding="utf-8") as f:
        for s in steps:
            f.write(json.dumps(s) + "\n")
        f.write(json.dumps({"exit": codes, "relayed": relayed, "stderr_tail": errs, "compat_log_tail": compat}) + "\n")
    for s in steps:
        print(json.dumps(s))
    print("exit", codes, "relayed", relayed)
    dones = [s for s in steps if s["step"] == "done"]
    res = next((s for s in steps if s["step"] == "resolve"), {"resolved": [], "unresolved": FORTNITE_IMPORTS})
    if all(c == 0 for c in codes.values()) and len(dones) == 2 and all(x["ok"] for x in dones) and not res["unresolved"]:
        os.makedirs(os.path.dirname(a.out), exist_ok=True)
        src = ("tools/progress/chatprobe.py: Fortnite's own GameChat2.dll (2008.4) loaded on PC in two peer processes with the emulator "
               "AudioHALWrapper.dll under net_guard deny, data frames relayed (A->B %d, B->A %d); Fortnite's ChatManagerInitialize "
               "arguments (1.0, relationship 9, restrictive, STT automatic, no manipulation) and 24 kbps bitrate; passed: %s"
               % (relayed["A"], relayed["B"], ", ".join(sorted({s["step"] for s in steps if s.get("ok") is True}))))
        json.dump({"title": "Fortnite", "source": src,
                   "modules": {"GameChat2.dll": {"loaded": True, "resolved": res["resolved"], "unresolved": []}}},
                  open(a.out, "w", encoding="utf-8"), indent=1)
        print("wrote", a.out)
    else:
        print("probe did not pass; chatprobe.json not written")
    shutil.rmtree(d, ignore_errors=True)


if __name__ == "__main__":
    main()
