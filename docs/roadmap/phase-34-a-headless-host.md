# Phase 34 — A headless host

*Goal: a release on a Linux machine with no desktop runs databases, keeps their passwords across
restarts, and comes back on its own after a reboot — the host that MixLab on another machine, and
later a phone, will control.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-09-30-a-headless-home-keeps-its-credentials-in-a-file-design.md](../specs/2026-09-30-a-headless-home-keeps-its-credentials-in-a-file-design.md)
(T194).

---

- [x] **T194a** The ADR: a Linux release may keep a home's credentials in a file, superseding ADR
      0052's "a release refuses `home`" and T33's "it does not fall back to a file".
- [x] **T194b** `settings.credential_store`, its precedence under the flag and over the build's
      default, and `credentials::choose` accepting `home` on a Linux release only. **(P)**
- [x] **T194c** The absent-store hint names `mix daemon credential-store home` ahead of the D-Bus
      workarounds; nothing switches on its own.
- [x] **T194d** `daemon.set_credential_store`, refused while the current store holds anything for
      this home, effective at the next start; `mix daemon credential-store`, and MixLab's
      *Credentials* section in Settings → MixEngine.
- [x] **T194e** `credentials` on `daemon.status` and in `mix status`, the doctor note, the startup
      log, the `mix database` wording; bindings.
- [x] **T194f** `security-model.md`, `platform-abstraction.md`'s `Keyring` row, the changelog lines,
      and the design flipped to `implemented`.
- [ ] **T195** Start at boot without a login: a systemd user unit and linger, enabled through
      `mixengine-elevate` once. Design to come.

**Milestone M34**: on an Ubuntu 24.04 server with no desktop, reached only over SSH, a release
chooses the file store, first-runs a MariaDB, keeps its password across a daemon restart, and after
a reboot with nobody logged in the daemon is back with the database answering.
