# Blockers

Standing walls and what would knock them down.

---

## ✅ Fortnite dump — resolved

Downloaded, byte-verified, extracted, kept pristine behind a run overlay.

## ✅ DirectStorage — resolved

PC DirectStorage 1.3.0 staged in the overlay.

## ✅ `xgameruntime.dll` — resolved

Present on this machine (loaded at startup); Gaming Services install not needed.

## 🟨 Media Foundation init

**State:** fix staged (`mfplat_sys.dll`), run 3 in progress.

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
