#!/bin/bash
# Build + deploy all compat DLLs, run the title under mdbg, print a summary.
# usage: iter.sh [seconds=240] [tail_lines=5]
cd "D:/projects/TomBox"
SB="C:/Users/drych/AppData/Local/Temp/claude/71a104f8182872d2/161832fd-3a57-4517-85d3-2c387a0e8b8e/scratchpad"
BIN="input/extracted/Mount/FortniteGame/Binaries/XSX"
export RUSTFLAGS="-C target-feature=+crt-static"
cargo build --release -p d3d12_xs -p tombox-xshader -p xmem -p xcrdapi -p acphal -p dxcompiler_xs -p mfplat 2>&1 | grep -E "^error" -A12 | head -30
# NOTE: xg_xs.dll in the title directory is the REAL Xbox library (runs natively) - never overwrite it.
for d in acphal d3d12_xs dxcompiler_xs mfplat xcrdapi xmem; do
  cp "target/release/$d.dll" "$BIN/$d.dll" || echo "LOCKED $d"
done
rm -rf /c/Users/drych/AppData/Local/Temp/tombox_shaders
# Network policy: every host the title asks for is redirected to the project owner's server
# (never Epic). Unset TOMBOX_NET_REDIRECT to fall back to deny-all.
export TOMBOX_NET_REDIRECT="${TOMBOX_NET_REDIRECT-horizon.halcy0n.net}"
# A process started under a debugger gets the NT debug heap, which makes the in-process
# shader compiler several times slower (long enough to trip the title watchdog).
export _NO_DEBUG_HEAP=1
export TOMBOX_LOG="C:/Users/drych/AppData/Local/Temp/tombox_g.log"; rm -f "$TOMBOX_LOG"
MDBG_LIMIT=${1:-240} timeout $((${1:-240}+30)) "$SB/mdbg.exe" "G:/FortniteGame/Binaries/XSX/FortniteClient-XSX-Shipping.exe" -unattended > "$SB/run.txt" 2>&1
grep -aE "shader_xlat FAIL|PSO_X (.*FAIL)|PSO_X untrans" "$TOMBOX_LOG" | cut -c15-240 | tail -5
grep -aE "PSO_X counts" "$TOMBOX_LOG" | tail -1 | cut -c15-200
grep -a "PresentX frame" "$TOMBOX_LOG" | tail -1 | cut -c15-80
echo "=== exit ==="
grep -aE "EXIT_PROCESS|\[EXC\] (AV|code=0x0000087)|Fatal error|Line: |time limit" "$SB/run.txt" | tail -3
grep -a -A10 "^\[STACK\]" "$SB/run.txt" | grep -v cfgmgr | head -6
grep -a "\[ODS\]" "$SB/run.txt" | grep -avE "Setting CVar|LogConfig|D3D12 WARN|Runnable|Malloc Size" | tail -${2:-5} | cut -c1-230
ls /c/Users/drych/AppData/Local/Temp/tombox_shaders 2>/dev/null | head
echo "=== network guard ==="
grep -a "net_guard" "$TOMBOX_LOG" | cut -c15-200 | sed 's/(total blocked calls: [0-9]*)//' | sort | uniq -c | sort -rn | head -8
grep -aE "LogWinHttp|LogHttp" "$SB/run.txt" | cut -c1-200 | tail -4
