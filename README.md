# xbox-pc-port

Progress notes for running Xbox Series X|S builds on PC.

Two parallel tracks:

- **TomBox** (upstream: drycheetah84/TomBox) — Xbox Series compatibility runtime.
  Uses a legally obtained, already-decrypted dump. Current target: Fortnite XSX
  `2.0.1461.7919`.
- **rage9** — a separate PC engine for RAGE games. Goal: a native build of GTA VI
  for PC. Blocked on RPF9 decryption (no game executable).

This repo tracks status, decisions, and blockers across both tracks. Code lives in
the upstream repos or stays local.

See [STATUS.md](STATUS.md) for the current snapshot.
