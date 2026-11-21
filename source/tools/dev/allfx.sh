#!/bin/bash
# Translate + FXC-compile every fixture shader; print one line each (OK / FAIL reason).
cd "D:/projects/TomBox"; SB="C:/Users/drych/AppData/Local/Temp/claude/71a104f8182872d2/161832fd-3a57-4517-85d3-2c387a0e8b8e/scratchpad"; FXC="/c/Program Files (x86)/Windows Kits/10/bin/10.0.26100.0/x64/fxc.exe"
for f in analysis/shaders/*.dxbc; do n=$(basename $f .dxbc); st=$(./target/release/xshader.exe $f 2>/dev/null | grep -oE "stage=[A-Za-z]+" | head -1); case "$st" in *Vertex) p=vs;; *Pixel) p=ps;; *Compute) p=cs;; *Geometry) p=gs;; *) p=xx;; esac
rm -f "$SB/t.hlsl" "$SB/t.cso"; r1=$(./target/release/xshader.exe $f --hlsl-out "$SB/t.hlsl" 2>&1 | grep -E "translate failed|extract failed")
if [ -n "$r1" ]; then echo "$n [$p]: FAIL $r1"; continue; fi
ver=5_0; if [ "$p" = ps ] && grep -qF "register(u" "$SB/t.hlsl"; then ver=5_1; fi
r2=$("$FXC" //nologo //T ${p}_${ver} //E main //Fo "$SB/t.cso" "$SB/t.hlsl" 2>&1 | grep -E "error" | head -1 | sed 's/.*t.hlsl//')
if [ -f "$SB/t.cso" ]; then echo "$n [$p]: fxc_ok"; else echo "$n [$p]: FAIL fxc $r2"; fi; done
