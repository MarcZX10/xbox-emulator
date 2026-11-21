import argparse, json, os, re

FUNCS = {
    "MFPlat.DLL": ["MFStartup", "MFShutdown", "MFCreateAttributes", "MFCreateMediaType", "MFCreateSample", "MFCreateMemoryBuffer",
                   "MFCreateAlignedMemoryBuffer", "MFCreateAsyncResult", "MFInvokeCallback", "MFPutWorkItem2", "MFCreateFile",
                   "MFCreateSourceResolver", "MFCreateWaveFormatExFromMFMediaType", "MFllMulDiv", "MFCreateDXGIDeviceManager",
                   "MFResetDXGIDeviceManagerX"],
    "MFReadWrite.dll": ["MFCreateSourceReaderFromByteStream", "MFCreateSourceReaderFromMediaSource"],
}
TARGETS = {"MFPlat.DLL": ("mfplat.dll", "mfplat_sys.dll", "rtworkq.dll"), "MFReadWrite.dll": ("mfreadwrite.dll",)}
TESTS = {
    "MFStartup": "proxy_dll::startup_and_console_only_exports_follow_console_semantics, fortnite_async (0x20070, MFSTARTUP_FULL)",
    "MFShutdown": "proxy_dll::startup_and_console_only_exports_follow_console_semantics",
    "MFCreateAttributes": "fortnite_async::create_attributes_with_fortnite_size_hint_grows_past_it (size hint 1)",
    "MFCreateMediaType": "proxy_dll::forwarded_exports_keep_full_signatures",
    "MFCreateSample": "proxy_dll::forwarded_exports_keep_full_signatures",
    "MFCreateMemoryBuffer": "proxy_dll::forwarded_exports_keep_full_signatures",
    "MFCreateAlignedMemoryBuffer": "proxy_dll::forwarded_exports_keep_full_signatures",
    "MFCreateAsyncResult": "fortnite_async::create_async_result_and_invoke_callback_deliver_status_object_and_state",
    "MFInvokeCallback": "fortnite_async::create_async_result_and_invoke_callback_deliver_status_object_and_state",
    "MFPutWorkItem2": "fortnite_async::put_work_item2_on_standard_queue_as_fortnite_calls_it (queue 1, priority 0)",
    "MFCreateFile": "proxy_dll::pc_source_reader_reads_through_proxy_from_byte_stream_and_media_source (READ, FAIL_IF_NOT_EXIST, NONE)",
    "MFCreateSourceResolver": "proxy_dll::pc_source_reader_reads_through_proxy_from_byte_stream_and_media_source",
    "MFCreateWaveFormatExFromMFMediaType": "proxy_dll::forwarded_exports_keep_full_signatures (flags 0)",
    "MFllMulDiv": "proxy_dll::forwarded_exports_keep_full_signatures",
    "MFCreateDXGIDeviceManager": "proxy_dll::source_reader_decodes_video_with_d3d12_manager_reset_by_console_export",
    "MFResetDXGIDeviceManagerX": "reset_device::reset_binds_real_d3d12_device_behind_wrapper",
    "MFCreateSourceReaderFromByteStream": "proxy_dll::pc_source_reader_reads_through_proxy_from_byte_stream_and_media_source",
    "MFCreateSourceReaderFromMediaSource": "proxy_dll::pc_source_reader_reads_through_proxy_from_byte_stream_and_media_source",
}
BIND = re.compile(r"\[mfplat\] trace bind (\S+)!(\S+) slot=\S+ -> (\S+?)(?:\+0x[0-9a-f]+)? \(")
CALL = re.compile(r"\[mfplat\] trace (\S+)!(\S+)#(\d+) (.*)")
HR = re.compile(r"hr=(0x[0-9a-f]{8})")


def main():
    ap = argparse.ArgumentParser(description="Build media.json from a Fortnite run's compat log (mfplat fortnite_trace lines). A function is resolved when its EXE import slot was bound in-game to the expected PC module and none of its in-game calls failed; tests listed per function reproduce Fortnite's call-site arguments.")
    ap.add_argument("log", nargs="?", default=os.path.join(os.environ.get("TEMP", ""), "tombox", "tombox_compat.log"))
    ap.add_argument("--run", default="")
    ap.add_argument("--out", default=r"D:\xbox-pc-port\emulator\analysis\progress_evidence\media.json")
    a = ap.parse_args()
    bound, calls = {}, {}
    for ln, line in enumerate(open(a.log, encoding="utf-8", errors="replace"), 1):
        m = BIND.search(line)
        if m:
            bound[(m.group(1).lower(), m.group(2))] = (m.group(3).lower(), ln)
            continue
        m = CALL.search(line)
        if m:
            k = (m.group(1).lower(), m.group(2))
            c = calls.setdefault(k, {"calls": 0, "failed": 0, "first_line": ln})
            c["calls"] = max(c["calls"], int(m.group(3)))
            h = HR.search(m.group(4))
            if h and int(h.group(1), 16) >= 0x80000000:
                c["failed"] += 1
    modules = {}
    for dll, funcs in FUNCS.items():
        res, unres, detail = [], [], {}
        for f in funcs:
            b = bound.get((dll.lower(), f))
            c = calls.get((dll.lower(), f), {"calls": 0, "failed": 0})
            ok = b is not None and b[0] in TARGETS[dll] and not (c["calls"] and c["failed"] >= c["calls"])
            (res if ok else unres).append(f)
            detail[f] = {"bound_to": b[0] if b else None, "bind_line": b[1] if b else None, "in_game_calls_logged": c["calls"],
                         "failed_logged_calls": c["failed"], "first_call_line": c.get("first_line"), "test": TESTS[f]}
        modules[dll] = {"loaded": any(bound.get((dll.lower(), f)) for f in funcs), "resolved": res, "unresolved": unres, "functions": detail}
    out = {"title": "Fortnite", "source": "%s %s (mfplat fortnite_trace) + crates/compat/mfplat tests" % (a.log, a.run), "modules": modules}
    os.makedirs(os.path.dirname(a.out), exist_ok=True)
    json.dump(out, open(a.out, "w", encoding="utf-8"), indent=1)
    for dll, m in modules.items():
        print(dll, "loaded", m["loaded"], "resolved", len(m["resolved"]), "unresolved", m["unresolved"])
    print("wrote", a.out)


if __name__ == "__main__":
    main()
