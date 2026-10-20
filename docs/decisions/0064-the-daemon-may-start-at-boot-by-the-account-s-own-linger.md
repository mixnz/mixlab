# 0064. The daemon may start at boot, by the account's own linger

**Status**: Accepted
**Date**: 2026-10-21

## Context

[overview.md](../architecture/overview.md) gave the daemon one lifetime: *login → logout*. T85b
built the Linux autostart entry on it — a systemd **user** unit, wanted by `default.target` — and
said in its code that `loginctl enable-linger` was *deliberately not called*, because a user manager
that stops at logout is exactly that lifetime.

Phase 34's milestone needs the other one. A release on an Ubuntu server with no desktop, reached only
over SSH, has to come back after a reboot **with nobody logged in**, with its databases answering.
Nobody logs in to that machine to start a session; without one, a user manager never starts, and
neither does the unit.

systemd's answer is linger: an account that lingers gets its user manager at boot, and keeps it
after logout. Three things were measured on 2026-10-20 and 2026-10-21, on Ubuntu 24.04.5 with
systemd 255 (the
[T195 design](../specs/2026-10-20-t195-start-at-boot-without-a-login-design.md) has the detail):

1. systemd's own polkit policy allows `org.freedesktop.login1.set-self-linger` to any session of the
   account, with no authentication. `loginctl enable-linger <own uid>` and `disable-linger` succeed
   as the user, from an SSH shell and from a transient user unit that is in no session.
2. `loginctl enable-linger` with **no** argument means the user of the caller's *session*, and a
   daemon started by its unit at boot has none.
3. On that machine `mixengine-elevate` cannot run at all: there is no polkit agent, and
   `pkexec --disable-internal-agent` is how this project chose to fail when there is none.

## Decision

**The daemon's lifetime is *login → logout*, or *boot → shutdown* when somebody asks for it.**

- **Only `autostart.enable_at_boot` turns linger on**, reachable as `mix autostart enable --at-boot`
  and as a checkbox in MixLab. No installer, no update and no other method does. It is ADR 0016's
  rule — written only when somebody asks — applied to one more setting.
- **As the account, never elevated.** `loginctl enable-linger <uid>` and `disable-linger <uid>` run
  as the user, with the uid named. No `PrivilegedOp` is added and `mixengine-elevate` is not
  involved. Where polkit says no, the call fails with the command a person can run themselves,
  `sudo loginctl enable-linger $USER`, and the login entry it registered first stays.
- **A switch of its own, not part of `autostart.enable`.** Linger starts *every* unit the account has
  enabled, not only `mixengined`, so it is asked for separately. `autostart.disable_at_boot` keeps
  the login entry.
- **MixEngine turns off only a linger it turned on.** The home records, in its settings, that it
  changed linger from off to on. `autostart.disable` and `autostart.disable_at_boot` turn linger off
  only on that record. An account that already lingered keeps lingering, and the answer says so
  (`at_boot: on_by_account`).
- **Windows and macOS answer `unsupported`.** Starting before login there is a boot-triggered task
  with a stored password, or a LaunchDaemon, which is root's; neither is the account's own.

ADR 0016 is not edited: every decision in it still holds, and this one adds a setting beside it.

## Consequences

- `docs/architecture/overview.md`'s lifetime row reads *login → logout, or boot → shutdown when
  asked*.
- A daemon started at boot has no session and no Secret Service, so what makes it useful is the file
  credential store of [ADR 0059](0059-a-linux-release-may-keep-a-homes-credentials-in-a-file.md). The daemon coming back is not a service coming back: a service starts with it only if its
  own `autostart` flag is on.
- **One window is left open, and stated.** Linger turned off by hand and on again by hand, with no
  `autostart.*` call between, still reads as MixEngine's, and a later `disable` would turn it off.
  Closing it would need a read at every daemon start, which autostart is kept out of on purpose.
- Two homes on one account share one linger. The record stays with the home that turned it on.

## Alternatives considered

- **Through `mixengine-elevate`, as the roadmap first said.** Refused on all three measurements: the
  operation is not privileged where it matters, the elevated path cannot run on the machine it is
  for, and a helper editing an account's lifetime as root is a bigger primitive than the problem.
- **Turning linger on inside `autostart.enable`.** Refused: it changes when every unit of the account
  runs, which is a consent question of its own.
- **A system unit** (`/etc/systemd/system/mixengined@.service`, run as the user). Refused: root to
  install and remove, a new `PrivilegedOp`, and a daemon in a different process tree from the one
  `mix` starts, which is two ways for one home to be running.
