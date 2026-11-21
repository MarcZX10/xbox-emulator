import argparse, ctypes, json, os, queue, re, shutil, subprocess, sys, tempfile, threading, time
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


def child(d, peer, seconds=4.0):
    d3d = ctypes.WinDLL(os.path.join(d, "d3d12_xs.dll"))
    d3d.D3D12XboxSetProcessDebugFlags.argtypes = [ctypes.c_uint32]
    d3d.D3D12XboxSetProcessDebugFlags(0)
    ws = ctypes.WinDLL("ws2_32")
    res = ctypes.c_void_p()
    rc = ws.getaddrinfo(b"privacy.xboxlive.com", b"443", None, ctypes.byref(res))
    addrs = []
    p = res.value if rc == 0 else None
    while p:
        sa = ctypes.c_void_p.from_address(p + 32).value
        if sa and ctypes.c_uint16.from_address(sa).value == 2:
            addrs.append(".".join(str(b) for b in ctypes.string_at(sa + 4, 4)))
        p = ctypes.c_void_p.from_address(p + 40).value
    blocked = rc != 0 or (bool(addrs) and all(a.startswith("127.") for a in addrs))
    emit("net_guard", blocked=blocked, resolved_to=addrs)
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

    pump(seconds)
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

TONE_HZ, TONE_AMP, RATE, FRAME = 440.0, 8000, 24000, 480


def tone(frames):
    import math
    return [int(TONE_AMP * math.sin(2 * math.pi * TONE_HZ * i / RATE)) for i in range(frames * FRAME)]


def tone_fit(pcm, skip):
    import math
    x = pcm[skip:]
    n = len(x)
    if n < RATE // 10:
        return 0.0, -99.0
    w = 2 * math.pi * TONE_HZ / RATE
    sc = [(math.sin(w * i), math.cos(w * i)) for i in range(n)]
    ss = sum(a * a for a, _ in sc)
    cc = sum(b * b for _, b in sc)
    sx = sum(a * b for a, b in sc)
    xs = sum(v * a for v, (a, _) in zip(x, sc))
    xc = sum(v * b for v, (_, b) in zip(x, sc))
    det = ss * cc - sx * sx
    a = (xs * cc - xc * sx) / det
    b = (xc * ss - xs * sx) / det
    amp = math.hypot(a, b)
    res = sum((v - a * s1 - b * c1) ** 2 for v, (s1, c1) in zip(x, sc)) / n
    snr = 10 * math.log10((amp * amp / 2) / res) if res > 0 else 99.0
    return amp, snr


def codec_child(d):
    import array
    ah = ctypes.WinDLL(os.path.join(d, "AudioHALWrapper.dll"))
    P, U32, U16, I16 = ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint16, ctypes.c_int16
    sig = {
        "CreateOpusEncoderHardwareInterface": [ctypes.POINTER(P)], "CreateOpusDecoderHardwareInterface": [ctypes.POINTER(P)],
        "CreateSilkHardwareInterface": [ctypes.POINTER(P)],
        "AcquireEncoder": [P, U32, U32, U32, U32, ctypes.POINTER(U32)], "AcquireDecoder": [P, U32, U32, ctypes.POINTER(U32)],
        "EncodeFrame": [P, U32, ctypes.c_void_p, U16], "GetEncodeOutput": [P, U32, ctypes.c_uint8, ctypes.POINTER(U32), ctypes.c_void_p, ctypes.POINTER(U16)],
        "DecodePacket": [P, U32, ctypes.c_void_p, U16], "GetDecodeOutput": [P, U32, ctypes.c_uint8, ctypes.POINTER(U32), ctypes.c_void_p, ctypes.POINTER(U16)],
        "ReleaseEncoder": [P, U32], "ReleaseDecoder": [P, U32],
        "SilkAcquireEncoder": [P, ctypes.POINTER(U32)], "SilkAcquireDecoder": [P, ctypes.POINTER(U32)],
        "SilkEncodeFrame": [P, U32, ctypes.c_void_p, I16, ctypes.c_void_p, ctypes.POINTER(I16), ctypes.POINTER(ctypes.c_int32 * 8), ctypes.c_uint8],
        "SilkDecodeFrame": [P, U32, ctypes.c_void_p, U32, ctypes.c_void_p, ctypes.POINTER(I16), ctypes.POINTER(ctypes.c_int32 * 6), ctypes.c_uint8, ctypes.c_uint8],
        "SilkReleaseEncoder": [P, U32], "SilkReleaseDecoder": [P, U32],
    }
    for n, a in sig.items():
        getattr(ah, n).argtypes = a
        getattr(ah, n).restype = ctypes.c_long
    for n in ("DestroyOpusEncoderHardwareInterface", "DestroyOpusDecoderHardwareInterface", "DestroySilkHardwareInterface"):
        getattr(ah, n).argtypes = [P]
        getattr(ah, n).restype = None
    frames = 50
    src = tone(frames)
    ok = True

    def chk(name, hr):
        nonlocal ok
        if hr < 0:
            ok = False
            emit("codec_call", call=name, hr=hx(hr), ok=False)
        return hr >= 0

    enc, dec = P(), P()
    opus_out, opus_bytes = [], 0
    if chk("CreateOpusEncoderHardwareInterface", ah.CreateOpusEncoderHardwareInterface(ctypes.byref(enc))) and \
       chk("CreateOpusDecoderHardwareInterface", ah.CreateOpusDecoderHardwareInterface(ctypes.byref(dec))):
        ei, di = U32(), U32()
        if chk("AcquireEncoder", ah.AcquireEncoder(enc, RATE, 1, 24000, 0, ctypes.byref(ei))) and \
           chk("AcquireDecoder", ah.AcquireDecoder(dec, RATE, 1, ctypes.byref(di))):
            for f in range(frames):
                pcm = array.array("h", src[f * FRAME:(f + 1) * FRAME])
                buf = (ctypes.c_uint8 * 0x2e8)()
                sz, ticks = U16(len(buf)), U32()
                if not (chk("EncodeFrame", ah.EncodeFrame(enc, ei, pcm.buffer_info()[0], FRAME * 2)) and
                        chk("GetEncodeOutput", ah.GetEncodeOutput(enc, ei, 1, ctypes.byref(ticks), buf, ctypes.byref(sz)))):
                    break
                opus_bytes += sz.value
                out = (ctypes.c_int16 * 0x2d00)()
                osz = U16(ctypes.sizeof(out))
                if not (chk("DecodePacket", ah.DecodePacket(dec, di, buf, sz.value)) and
                        chk("GetDecodeOutput", ah.GetDecodeOutput(dec, di, 1, None, out, ctypes.byref(osz)))):
                    break
                opus_out.extend(out[:osz.value // 2])
            ah.ReleaseEncoder(enc, ei)
            ah.ReleaseDecoder(dec, di)
        ah.DestroyOpusEncoderHardwareInterface(enc)
        ah.DestroyOpusDecoderHardwareInterface(dec)
    amp, snr = tone_fit(opus_out, RATE // 5)
    opus_ok = len(opus_out) == frames * FRAME and opus_bytes > 0 and 0.5 * TONE_AMP < amp < 1.5 * TONE_AMP and snr > 10
    emit("codec_opus", ok=opus_ok, frames=frames, samples_out=len(opus_out), packet_bytes=opus_bytes, tone_amp=round(amp, 1), snr_db=round(snr, 1))
    ok = ok and opus_ok
    silk = P()
    silk_out, silk_bytes = [], 0
    if chk("CreateSilkHardwareInterface", ah.CreateSilkHardwareInterface(ctypes.byref(silk))):
        se, sd = U32(), U32()
        if chk("SilkAcquireEncoder", ah.SilkAcquireEncoder(silk, ctypes.byref(se))) and chk("SilkAcquireDecoder", ah.SilkAcquireDecoder(silk, ctypes.byref(sd))):
            ectl = (ctypes.c_int32 * 8)(RATE, RATE, FRAME, 24000, 0, 2, 0, 0)
            dctl = (ctypes.c_int32 * 6)(RATE, 0, 0, 0, 0, 0)
            for f in range(frames):
                pcm = array.array("h", src[f * FRAME:(f + 1) * FRAME])
                buf = (ctypes.c_uint8 * 0x4ff)()
                nb = I16(len(buf))
                if not chk("SilkEncodeFrame", ah.SilkEncodeFrame(silk, se, pcm.buffer_info()[0], FRAME, buf, ctypes.byref(nb), ctypes.byref(ectl), 1)):
                    break
                silk_bytes += nb.value
                out = (ctypes.c_int16 * 4800)()
                ns = I16(ctypes.sizeof(out))
                if not chk("SilkDecodeFrame", ah.SilkDecodeFrame(silk, sd, buf, nb.value, out, ctypes.byref(ns), ctypes.byref(dctl), 1, 1)):
                    break
                silk_out.extend(out[:ns.value])
            ah.SilkReleaseEncoder(silk, se)
            ah.SilkReleaseDecoder(silk, sd)
        ah.DestroySilkHardwareInterface(silk)
    amp, snr = tone_fit(silk_out, RATE // 5)
    silk_ok = len(silk_out) == frames * FRAME and silk_bytes > 0 and 0.5 * TONE_AMP < amp < 1.5 * TONE_AMP and snr > 10
    emit("codec_silk", ok=silk_ok, frames=frames, samples_out=len(silk_out), packet_bytes=silk_bytes, tone_amp=round(amp, 1), snr_db=round(snr, 1))
    ok = ok and silk_ok
    emit("codec_done", ok=ok)
    return 0 if ok else 1


def peer_host(d, peer):
    home = os.path.dirname(sys.executable)
    pd = os.path.join(d, "host_" + peer)
    os.makedirs(pd, exist_ok=True)
    for n in os.listdir(home):
        if n.lower().endswith((".exe", ".dll")) and os.path.isfile(os.path.join(home, n)):
            shutil.copyfile(os.path.join(home, n), os.path.join(pd, n))
    with open(os.path.join(pd, "pyvenv.cfg"), "w", encoding="utf-8") as f:
        f.write("home = %s\ninclude-system-site-packages = false\n" % home)
    with open(os.path.join(pd, "tombox.cfg"), "w", encoding="utf-8") as f:
        f.write("xuid=%s\ngamertag=ChatProbe%s\n" % (PEERS[peer][0], peer))
    return os.path.join(pd, os.path.basename(sys.executable))


def main():
    ap = argparse.ArgumentParser(description="Load Fortnite's own GameChat2.dll (read-only copy) in two peer processes on this PC with the emulator's GDK runtime and network policy, relay their data frames to each other, run the 14 functions Fortnite imports, and write chatprobe.json only if every step succeeds in both peers.")
    ap.add_argument("--child")
    ap.add_argument("--peer", default="A")
    ap.add_argument("--title-dll", default=r"D:\xbox-pc-port\games\Fortnite\extracted\Mount\FortniteGame\Binaries\XSX\GameChat2.dll")
    ap.add_argument("--target", default=r"D:\xbox-pc-port\emulator\target-sysfn\debug")
    ap.add_argument("--out", default=r"D:\xbox-pc-port\emulator\analysis\progress_evidence\chatprobe.json")
    ap.add_argument("--log", default=os.path.join(tempfile.gettempdir(), "chatprobe_last.jsonl"))
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--seconds", type=float, default=4.0)
    ap.add_argument("--codec-child")
    ap.add_argument("--console", action="store_true", help="require GameChat2's console path: AudioHALWrapper loaded, RTA connected with acked subscriptions, every Xbox Live request answered")
    a = ap.parse_args()
    if a.codec_child:
        sys.exit(codec_child(a.codec_child))
    if a.child:
        sys.exit(child(a.child, a.peer, a.seconds))
    d = tempfile.mkdtemp(prefix="chatprobe_")
    shutil.copyfile(a.title_dll, os.path.join(d, "GameChat2.dll"))
    for n in ("AudioHALWrapper.dll", "d3d12_xs.dll", "opus.dll", "SKP_SILK_SDK.dll"):
        shutil.copyfile(os.path.join(a.target, n), os.path.join(d, n))
    env = dict(os.environ)
    env.pop("TOMBOX_NET_REDIRECT", None)
    env.pop("TOMBOX_ALLOW_NETWORK", None)
    env.pop("TOMBOX_NET_LOCAL_PORT", None)
    env.update({"TOMBOX_NET_BACKEND": "builtin", "TOMBOX_NET_TITLE": "fortnite"})
    procs, results = {}, {}
    for peer in PEERS:
        e = dict(env, TOMBOX_LOG=os.path.join(d, "tombox_compat_%s.log" % peer))
        procs[peer] = subprocess.Popen([peer_host(d, peer), "-I", os.path.abspath(__file__), "--child", d, "--peer", peer, "--seconds", str(a.seconds)], cwd=d, env=e,
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
    cp = subprocess.run([sys.executable, "-I", os.path.abspath(__file__), "--codec-child", d], cwd=d,
                        env=dict(env, TOMBOX_LOG=os.path.join(d, "tombox_compat_codec.log")), capture_output=True, text=True, timeout=300)
    codes["codec"] = cp.returncode
    errs["codec"] = cp.stderr[-2000:]
    steps += [dict(json.loads(l), peer="codec") for l in cp.stdout.splitlines() if l.startswith("{")]
    compat = {}
    for peer in list(PEERS) + ["codec"]:
        lp = os.path.join(d, "tombox_compat_%s.log" % peer)
        compat[peer] = open(lp, encoding="utf-8", errors="replace").read()[-1500:] if os.path.isfile(lp) else ""
    console = {}
    for peer in PEERS:
        lp = os.path.join(d, "tombox_compat_%s.log" % peer)
        text = open(lp, encoding="utf-8", errors="replace").read() if os.path.isfile(lp) else ""
        console[peer] = {
            "audiohal_loaded": "late_load AudioHALWrapper.dll" in text,
            "rta_connected": "[online] rta connected" in text,
            "rta_acks": len(re.findall(r"\[online\] rta subscribe \d+ \S+ -> subscription \d+", text)),
            "rta_rejects": len(re.findall(r"\[online\] rta subscribe \d+ \S+ -> status", text)) + text.count("[online] rta malformed"),
            "xbl_requests": len(re.findall(r"redirect WinHttpConnect port 443: '[a-z]+\.xboxlive\.com'", text)),
            "unhandled": text.count("[online] unhandled"),
            "empty_bodies": text.count("[online] empty_body"),
        }
    on_console = all(c["audiohal_loaded"] for c in console.values())
    console_ok = all(c["audiohal_loaded"] and c["rta_connected"] and c["rta_acks"] > 0 and not c["rta_rejects"] and not c["unhandled"] and not c["empty_bodies"] for c in console.values())
    steps.append({"step": "console_path", "ok": console_ok, "peer": "both", **{p: c for p, c in console.items()}})
    with open(a.log, "w", encoding="utf-8") as f:
        for s in steps:
            f.write(json.dumps(s) + "\n")
        f.write(json.dumps({"exit": codes, "relayed": relayed, "stderr_tail": errs, "compat_log_tail": compat}) + "\n")
    for s in steps:
        print(json.dumps(s))
    print("exit", codes, "relayed", relayed)
    dones = [s for s in steps if s["step"] == "done"]
    res = next((s for s in steps if s["step"] == "resolve"), {"resolved": [], "unresolved": FORTNITE_IMPORTS})
    codec = {s["step"]: s for s in steps if s.get("peer") == "codec"}
    codec_ok = codec.get("codec_done", {}).get("ok") is True
    if all(c == 0 for c in codes.values()) and len(dones) == 2 and all(x["ok"] for x in dones) and not res["unresolved"] and codec_ok and (console_ok or not (a.console or on_console)):
        os.makedirs(os.path.dirname(a.out), exist_ok=True)
        src = ("tools/progress/chatprobe.py: Fortnite's own GameChat2.dll (2008.4) loaded on PC in two peer processes, each a copied "
               "python host with its own tombox.cfg xuid so the emulated XUser (gdk QueryApiImpl, routed into the delay-loaded "
               "GameChat2 by the gdk late-load hook) is the local user added with ChatManagerAddLocalUser; net_guard builtin backend "
               "redirects every host to the in-process loopback server; %s; data frames relayed "
               "(A->B %d, B->A %d, control traffic only, no microphone audio); voice codec path proven separately through the emulator "
               "AudioHALWrapper.dll exports with libopus opus.dll and SKP_SILK_SDK.dll on a synthesized 440 Hz tone, 50 x 20 ms "
               "frames at 24 kHz: opus %s, silk %s; Fortnite's "
               "ChatManagerInitialize arguments (1.0, relationship 9, restrictive, STT automatic, no manipulation) and 24 kbps "
               "bitrate; passed: %s"
               % (("console path (XSystemGetAnalyticsInfo family 'Windows.Xbox'): AudioHALWrapper loaded in both peers, the RTA websocket "
                   "(rta.xboxlive.com.V2) connected with %d/%d subscriptions acked, %d/%d Xbox Live requests (social people, privacy "
                   "permission/validate, profile SpeechAccessibility, userstats reputation) answered with JSON, signed with XSTS v1 P-256 "
                   "signatures, none unhandled"
                   % (console["A"]["rta_acks"], console["B"]["rta_acks"], console["A"]["xbl_requests"], console["B"]["xbl_requests"]))
                  if console_ok else "desktop path (family not 'Windows.Xbox'): GameChat2 issued no Xbox Live requests and did not load AudioHALWrapper",
                  relayed["A"], relayed["B"],
                  "%d bytes, tone amp %s, SNR %s dB" % (codec["codec_opus"]["packet_bytes"], codec["codec_opus"]["tone_amp"], codec["codec_opus"]["snr_db"]),
                  "%d bytes, tone amp %s, SNR %s dB" % (codec["codec_silk"]["packet_bytes"], codec["codec_silk"]["tone_amp"], codec["codec_silk"]["snr_db"]),
                  ", ".join(sorted({s["step"] for s in steps if s.get("ok") is True}))))
        doc = json.load(open(a.out, encoding="utf-8")) if os.path.isfile(a.out) else {}
        doc["title"] = doc.get("title", "Fortnite")
        sources = doc.get("sources", {})
        if "source" in doc and "GameChat2.dll" not in sources:
            sources["other_modules"] = doc["source"]
        sources["GameChat2.dll"] = src
        doc["sources"] = sources
        doc["source"] = "; ".join("%s: %s" % kv for kv in sources.items())
        entry = doc.setdefault("modules", {}).setdefault("GameChat2.dll", {})
        entry.update({"loaded": True, "resolved": res["resolved"], "unresolved": []})
        json.dump(doc, open(a.out, "w", encoding="utf-8"), indent=1)
        print("wrote", a.out)
    else:
        print("probe did not pass; chatprobe.json not written")
    if a.keep:
        print("kept", d)
    else:
        shutil.rmtree(d, ignore_errors=True)


if __name__ == "__main__":
    main()
