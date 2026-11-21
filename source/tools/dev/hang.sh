#!/bin/bash
# usage: hang.sh <dump_at_secs> <grep-regex of symbols selecting threads>
cd "D:/projects/TomBox"; EXE="input/extracted/Mount/FortniteGame/Binaries/XSX/FortniteClient-XSX-Shipping.exe"; SYM="analysis/fortnite_symbols.decompressed.bin"
SB="$(dirname "$0")"
MDBG_HANGDUMP=$1 bash "$SB/iter.sh" $(($1+8)) 1 >/dev/null 2>&1
awk '/^\[HANG\] tid/{t=$2} /^    \[rsp/{ if ($0 ~ /FortniteClient/) print t, $NF; else print t, "~" $2 $3 }' "$SB/run.txt" > "$SB/hf.txt"
grep -v "~" "$SB/hf.txt" | cut -d' ' -f2 | sort -u | sed 's/+0x//' > "$SB/rvas.txt"
for r in $(cat "$SB/rvas.txt"); do echo "$r $("./target/release/disasm.exe" "$EXE" $r 1 --sym "$SYM" 2>&1 | grep '====' | sed 's/.*) //; s/ ====//' | cut -c1-100)"; done > "$SB/rvasym.txt"
awk 'NR==FNR{k=$1; $1=""; s[k]=$0; next} { if ($2 ~ /^~/) print $1, $2; else { r=$2; sub(/\+0x/,"",r); print $1, s[r] } }' "$SB/rvasym.txt" "$SB/hf.txt" > "$SB/hsym.txt"
for t in $(grep -E "$2" "$SB/hsym.txt" | cut -d' ' -f1 | sort -u); do echo "=== $t"; grep "^$t " "$SB/hsym.txt" | cut -d' ' -f2- | head -${3:-26}; done
