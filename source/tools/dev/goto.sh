#!/bin/bash
# Move the in-game cursor to window pixel (x,y) of the 1280x720 host window and optionally click.
# The title accumulates raw mouse deltas in 3840x2160 space, so: clamp to the top-left corner,
# then move 3 counts per window pixel.   usage: goto.sh <x> <y> [click]
cd "D:/projects/TomBox"
powershell -NoProfile -ExecutionPolicy Bypass -Command "& 'tools/dev/mouse.ps1' -Dx (-4200) -Dy (-2500)" >/dev/null
powershell -NoProfile -ExecutionPolicy Bypass -Command "& 'tools/dev/mouse.ps1' -Dx ($1*3) -Dy ($2*3) -Click '$3'"
