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
- [x] **T211** The ADR: on a machine with no graphical agent, `mix` in a terminal raises the
      prompt itself, through `sudo` on the helper — amending ADR 0005. Found on an Ubuntu 24.04
      server over SSH (2026-10-21): `polkitd` is installed and running, but `pkexec` is not — a
      separate package since Ubuntu 24.04 — and with no polkit agent over SSH it could not prompt
      anyway, so `mix elevation grant` and the privileged half of `mix uninstall` never run there,
      and nothing names a way to do it by hand. The error itself is wrong too: it says *"pkexec is
      not on PATH, so polkit is not installed"*, which T211c corrects. Linux and macOS alike: a Mac
      over SSH has the same gap. ADR 0065;
      design: [2026-10-21-t211-elevation-from-a-terminal-design.md](../specs/2026-10-21-t211-elevation-from-a-terminal-design.md).
- [x] **T211a** The daemon writes the request and hands its path to a client that has a terminal,
      instead of refusing in `elevation/require.rs` before the request exists;
      `mixengine-elevate` still validates every request itself. **(P)**
- [x] **T211b** `mix elevation grant` and `mix uninstall` run `sudo <helper> <request>` in the
      person's own terminal, and the daemon reads the report as it does after `pkexec`. **(P)**
- [x] **T211c** Ask whether this machine can raise a prompt before `continue? [y/N]`, not after
      the answer; where it cannot, the error names the command a person can run by hand.
- [x] **T211d** `docs/guide/en/uninstalling.md` names `mixengine-headless` beside `mixlab` for
      `apt remove` and `dnf remove`, `mix uninstall`'s last line names the package command, and
      `--keep-home`'s help stops saying the daemon keeps running (ADR 0051).
- [x] **T195** Start at boot without a login: a systemd user unit and linger, turned on as the
      account and never through `mixengine-elevate` — polkit lets an account linger itself, and a
      headless host has no prompt to show.
      Design: [2026-10-20-t195-start-at-boot-without-a-login-design.md](../specs/2026-10-20-t195-start-at-boot-without-a-login-design.md).

**Milestone M34**: on an Ubuntu 24.04 server with no desktop, reached only over SSH, a release
chooses the file store, first-runs a MariaDB, keeps its password across a daemon restart, and after
a reboot with nobody logged in the daemon is back with the database answering; over the same SSH
session, `mix elevation grant` applies its queue and `mix uninstall` finishes.
