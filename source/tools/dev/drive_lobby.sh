#!/bin/bash
# Drive a freshly started title to the Battle Royale lobby with synthetic mouse input:
# splash click -> wait for login -> safe-zone DONE -> PLAY. Captures to <outdir>/d_*.png.
# usage: drive_lobby.sh <trace file> <outdir>
cd "D:/projects/TomBox"; T="$1"; O="$2"
W(){ ping -n $(($1+1)) 127.0.0.1 >/dev/null; }
cap(){ powershell -NoProfile -ExecutionPolicy Bypass -File tools/dev/capture.ps1 "$O/$1" >/dev/null; }
dead(){ grep -aq "EXIT_PROCESS" "$T"; }
for i in $(seq 1 90); do grep -aq "Setting user focus to widget Splash_Screen\|EXIT_PROCESS" "$T" 2>/dev/null && break; W 2; done
for k in 1 2 3 4; do W 8; grep -aq "StartLogin\|EXIT_PROCESS" "$T" && break
  powershell -NoProfile -ExecutionPolicy Bypass -Command "& 'tools/dev/mouse.ps1' -Dx 3 -Dy 3 -Click click" >/dev/null; done
for i in $(seq 1 60); do grep -aq "HandleLoginRequestComplete\|EXIT_PROCESS" "$T" && break; W 2; done
dead && { echo "exited before/at login"; exit 1; }
W 20; cap d_1_after_login.png
bash tools/dev/goto.sh 640 636 click >/dev/null; W 30; cap d_2_after_done.png
dead && { echo "exited after DONE"; exit 1; }
bash tools/dev/goto.sh 640 592 click >/dev/null; W 75; cap d_3_after_play.png
dead && { echo "exited after PLAY"; exit 1; }
echo "alive after PLAY"
