# Blockers

Standing walls and what would knock them down.

---

## 🟨 Fortnite dump

**State:** waiting on decrypted ZIP download.

**Why:** TomBox needs an already-decrypted filesystem (`Mount/...` tree). The
archive.org ZIP is live (27.4 GB, HTTP 200) and matches the build TomBox is
pinned to (`2.0.1461.7919`).

**What would unblock it:** download completes.

---

## 🟨 Microsoft Gaming Services

**State:** not installed on dev machine.

**Why:** TomBox needs `xgameruntime.dll` from MS Gaming Services for platform
service calls at startup.

**What would unblock it:** install from Store
(`ms-windows-store://pdp/?productid=9MWPM2CQNLHN`). 30 s, no reboot.

---

## ⬜ Keyboard input

**State:** not started. Upstream input is mouse-only.

**Where in upstream code:** `crates/compat/xcrdapi` handles controller device
calls; keyboard would likely plug in through the same compat path.

**What would unblock it:** a small input shim mapping Windows keyboard events
to whatever Xbox virtual key API Fortnite polls.

---

## ⬜ 3D scene correctness

**State:** upstream reports lobby character renders cyan "hologram" and bloom
produces bright blocks along the top of the screen. No reference image to
compare against.

**What would unblock it:** shader-output pack replacement is currently
inferred, not observed (TomBox `docs/TECHNICAL_DEBT.md`). Needs validation
against rendered pixels.

---

## ⬜ Audio

**State:** not started. `acphal` `IAcpHal` is an inert object — XMA voices
never decode.

**What would unblock it:** an XMA decoder and a SHAPE DSP replacement. Large
undertaking.
