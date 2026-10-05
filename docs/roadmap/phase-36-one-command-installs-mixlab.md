# Phase 36 — One command installs MixLab

*Goal: on a new machine, one line in a terminal picks the right installer, checks it, and installs
MixLab or the headless programs.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-10-05-t197-one-command-installs-mixlab-design.md](../specs/2026-10-05-t197-one-command-installs-mixlab-design.md).

---

- [x] **T197** **(P)** `install.sh` and `install.ps1` on `master`, published by the handbook's site;
      they choose the file for the system, check its checksum and signature, and run the installer.
      The dry-run and headless-install checks against the newest release, and the install page in
      English and Vietnamese.
