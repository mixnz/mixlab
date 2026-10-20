---
status: approved
date: 2026-10-20
task: T195
---

# T195 — Start at boot without a login (design)

Roadmap task **T195**, phase 34: *"Start at boot without a login: a systemd user unit and linger,
enabled through `mixengine-elevate` once."* The last task before milestone **M34**: on an Ubuntu
24.04 server with no desktop, reached only over SSH, *"after a reboot with nobody logged in the
daemon is back with the database answering."*

T85b built the systemd user unit and deliberately stopped short of this: a user manager runs from
the account's first login to its last logout, and so did the daemon it starts
([linux/autostart.rs](../../crates/mixengine-platform/src/linux/autostart.rs), *"`loginctl
enable-linger` is deliberately not called"*). Linger is what makes systemd start a user's manager at
boot and keep it after logout, and with it the unit T85b already writes.

One thing this task changes about its own sentence, argued in D1: **nothing here goes through
`mixengine-elevate`**. On the machine the milestone names, an account may turn linger on for itself
without a password, and the elevation prompt could not have been shown there anyway.

## Goal

A person on a headless Linux host runs `mix autostart enable --at-boot` once over SSH, logs out,
reboots, and finds the daemon running — with nobody logged in — and its databases answering.
`mix autostart status` says the daemon starts at boot, and whether MixEngine is the one that
arranged it. `mix autostart disable` undoes exactly what MixEngine did and nothing the account had
already set up.

## Measured, not assumed

On 2026-10-20, on this repository's own development machine: Ubuntu 24.04.5 LTS, systemd 255
(255.4-1ubuntu8.18), reached over SSH, no desktop. Read-only; nothing was changed on the machine.

1. **A user manager is running for an SSH-only account.** `systemctl --user is-system-running` →
   `running`, `XDG_RUNTIME_DIR=/run/user/1001`. T85b's unit therefore works here as it is — until
   the last session closes.
2. **Linger is off by default.** `loginctl show-user ubuntu -p Linger` → `Linger=no`, and
   `/var/lib/systemd/linger/` is empty.
3. **polkit lets an account linger itself with no authentication.** systemd's own policy file ships
   `org.freedesktop.login1.set-self-linger` with `allow_any`, `allow_inactive` and `allow_active`
   all `yes`, and `pkcheck --action-id org.freedesktop.login1.set-self-linger --process $$` exits
   `0` from the SSH session. `set-user-linger` — lingering *another* account — is the one that
   asks for an administrator.
4. **The elevation prompt has nowhere to appear.** The SSH sessions have no seat, and
   [linux/prompt.rs](../../crates/mixengine-platform/src/linux/prompt.rs) runs `pkexec
   --disable-internal-agent` precisely so that a machine with no agent fails fast rather than
   prompting on a tty nobody watches. On the milestone's machine, an operation routed through
   `mixengine-elevate` answers `Unavailable`.
5. **`loginctl enable-linger` with no argument means the caller's *session*, not the caller.**
   systemd 255's manual: *"If no argument is specified, enables/disables lingering for the user of
   the session of the caller."* A daemon started by its unit at boot is in no session at all, so
   the user has to be named — by numeric uid, which needs no name lookup.
6. **Linger can be read without root.** `loginctl show-user 1001 --property=Linger --value` → `no`,
   exit 0. And `/var/lib/systemd/linger/` is `drwxr-xr-x root`, though that path is logind's
   implementation, not its interface, and is not what D1 reads.
7. **Nothing the daemon needs lives in the session.** Its socket is `<home>/run/mixengined.sock`
   ([unix/ipc.rs](../../crates/mixengine-platform/src/unix/ipc.rs)), not under
   `XDG_RUNTIME_DIR`, so a daemon started at boot and a `mix` run later over SSH find each other
   without anything else changing.

Not measured, and to be measured during implementation:

- `loginctl enable-linger <own uid>` and `disable-linger <own uid>` actually succeeding without a
  password from a process outside any session. logind is expected to check `set-self-linger` when
  the uid named is the caller's own; the system suite (see *Testing*) is what proves it.
- `loginctl show-user <uid>` for an account with no session and no linger, which logind answers
  with an error rather than `Linger=no` — what D1 parses that error as.
- A full reboot with linger on and nobody logged in — the milestone itself.
- A distribution whose polkit policy does *not* allow `set-self-linger` — the case D1's refusal
  path is for.

## Scope

**In:**

- `ServiceInstaller` gains a boot half: three methods whose default answers are "unsupported", and
  a Linux implementation that reads linger and turns it on and off for this account through
  `loginctl`, in a new `linux/linger.rs` (D6).
- API: `autostart.enable_at_boot` and `autostart.disable_at_boot`, and a `at_boot` field on
  `AutostartReport`. `autostart.disable` also undoes a linger MixEngine turned on.
- One setting in SQLite: whether linger is on because MixEngine turned it on (D3).
- CLI: `mix autostart enable --at-boot`, `mix autostart disable --at-boot`, and a line in
  `mix autostart status`.
- MixLab: a second checkbox in Settings → MixEngine → *Autostart* (see `## MixLab`).
- Documentation: a new ADR (D5), `overview.md`'s lifetime row, `daemon-and-ipc.md`,
  `platform-abstraction.md`'s `ServiceInstaller` row, `services.md`, the changelog, the roadmap
  sentence corrected.

**Out:**

- **Boot start on Windows and macOS.** A Task Scheduler task with a boot trigger needs the
  account's password stored or the S4U logon type T85b's D4 already refused; a launchd job that
  starts before login is a LaunchDaemon, which is root's. Both answer `at_boot: unsupported` and
  `autostart.enable_at_boot` refuses with a typed `Unsupported` naming why.
- **A system unit** (`/etc/systemd/system/mixengined@.service`, run as the user). It would need
  root to install and remove, a `PrivilegedOp` the closed list does not have, and it would put the
  daemon in a different process tree from the one `mix` autostarts — two ways for one home to be
  running.
- **`mix doctor` reporting linger.** Same reasoning as T85b's: the capability makes it askable,
  the finding set is T47's.
- **Uninstall.** T87's `uninstall` already calls `autostart.disable`; D3 makes that call take the
  linger with it, which is all uninstall needs.

## Decisions

### D1 — `loginctl` as the account, and never `mixengine-elevate`

The roadmap sentence says *"enabled through `mixengine-elevate` once"*. It is not, for three
reasons, the first two of them measured:

1. **It is not privileged here.** Measurement 3: `set-self-linger` is allowed to any session of the
   account, so `loginctl enable-linger` with no user argument succeeds as the user. Raising a
   password prompt to do something the OS already lets the account do is the prompt a person
   learns to click through.
2. **It could not have run.** Measurement 4: the one machine T195 is for has no polkit agent, and
   `pkexec --disable-internal-agent` is how this project chose to fail there.
3. **A new `PrivilegedOp` needs an ADR, and a helper that edits `/var/lib/systemd/linger` would be
   a root process acting on an account's lifetime** — a primitive bigger than the problem.

So the Linux implementation runs `loginctl enable-linger <uid>` and `loginctl disable-linger <uid>`
with an argument vector, where `<uid>` is this process's own `getuid()` — measurement 5 is why the
argument cannot be left out — and reads linger with
`loginctl show-user <uid> --property=Linger --value`:

- `yes` → on, `no` → off.
- An error saying the user is *not logged in or lingering* → off. That sentence is logind saying the
  account has neither, which is exactly "not lingering"; it is what an account answers when the
  daemon outlived the SSH session that started it.
- Anything else, or `loginctl` missing → the boot half is unsupported, never an error from `status`
  — T85b's rule that a status reports rather than refuses.

`/var/lib/systemd/linger/` is not read even though it is readable (measurement 6): it is where
logind keeps its state today, and `loginctl` is its interface.

**Where polkit says no** — an older or hardened distribution whose policy is `auth_admin_keep` —
`enable_at_boot` fails with a typed error carrying the command a person can run themselves:
`sudo loginctl enable-linger <user>`. That is the T85b refusal's shape (*"start the daemon yourself
… `mixengined --home`"*), and a person reached over SSH has a terminal to type it into. After they
do, `status` reports linger on, set by the account (D3).

### D2 — A switch of its own, not part of `enable`

Linger is not a property of MixEngine's unit; it is a property of the **account**: with it on,
*every* unit the account has enabled starts at boot and survives logout, not only `mixengined`.
That is a consent question one step louder than T85b's, so it is asked separately:

- `autostart.enable` keeps doing exactly what it does today, and never touches linger.
- `autostart.enable_at_boot` registers the unit (as `enable`) **and** turns linger on. Boot start
  without the unit would start nothing, so it implies the login entry. **The unit first, then
  linger**: if polkit refuses, the call fails with D1's hint and the login entry stays registered —
  it is what `enable` would have done and is useful on its own, and the error says it is in place.
  The other order could leave an account lingering for a unit that failed to be written.
- `autostart.disable_at_boot` turns off the linger MixEngine turned on, and leaves the login entry.
  Against a linger the account set (`on_by_account`) it changes nothing and says so with
  `changed: false`.
- `autostart.disable` removes the unit, and with it the linger MixEngine turned on — a daemon
  nobody asked to start at login has no business keeping an account lingering.

All four take no parameters, keeping ADR 0016's rule that an `autostart.*` method cannot be asked to
register anything but this daemon for this home.

`AutostartReport` gains:

```rust
pub enum AtBoot {
    /// No mechanism here starts a daemon before login: Windows, macOS, a Linux with no user manager.
    Unsupported,
    /// The daemon starts at login only.
    Off,
    /// Linger is on because MixEngine turned it on; `disable` will turn it off.
    On,
    /// Linger is on because the account turned it on; MixEngine will never turn it off.
    OnByAccount,
}
```

`at_boot` is composed by the daemon from the platform's reading and the setting in D3, so no client
works out whose linger it is. It is `#[serde(default)]` to `unsupported`, so a client reading an
answer from a daemon built before this task draws today's screen rather than failing to decode.

### D3 — MixEngine turns off only a linger it turned on

An account that already lingers — a person running their own user services on the host — must not
lose that because they unticked a MixEngine box. So the daemon records, in its settings table, that
**this home turned linger on**, at the moment `enable_at_boot` changed it from off to on:

- `enable_at_boot` finding linger already on records nothing, and answers `OnByAccount`.
- `disable_at_boot` and `disable` call `disable-linger` only when the record says MixEngine set it,
  then clear the record.
- **Any `autostart.*` call that reads linger off clears a stale record** (somebody ran `loginctl
  disable-linger` by hand), so a later `disable` cannot turn off a linger the account set in the
  meantime. `status` is one of those calls, so it can write — to MixEngine's own settings, never to
  the machine; the trait's `state` still changes nothing.

The window that is left: linger turned off by hand **and** on again by hand, with no `autostart.*`
call in between, still reads as MixEngine's. Closing it would need a read at every daemon start,
which T85b keeps autostart out of on purpose, for a sequence nobody performs by accident.

The record is per home, but linger is per account; the account has one unit and one home named in
it (ADR 0016), so in practice it is the home the unit names. A second home calling
`enable_at_boot` replaces the unit, finds linger already on, and answers `OnByAccount` — the
record stays with the first home, whose `disable` would then turn linger off under the second.
That edge is accepted and stated, not engineered around: two homes on one headless account fighting
over boot start is not a case this product promises to arbitrate.

### D4 — The unit does not change

T85b's unit is already what boot start needs: `WantedBy=default.target`, which a lingering
manager reaches at boot; `--home` on the command line rather than from the environment; and
`Restart=on-failure`, so `mix daemon stop` stays stopped. Two things are deliberately **not** added:

- **`After=network-online.target`.** A user manager cannot order against system targets, and the
  daemon binds loopback addresses it does not wait for.
- **`Restart=always`.** ADR 0016's rule holds at boot exactly as at login.

The credentials a booted daemon needs to start a database are in the file store T194 introduced,
which is what makes a daemon started with no session, and no Secret Service, useful at all.

**A daemon coming back is not a database coming back.** What the daemon starts on its own is the
services whose own `autostart` flag is on (T112, T113: `mix service autostart <id> --on`, or the ⋮
menu on the Dashboard). The milestone's *"the database answering"* depends on that flag being on
for the MariaDB, and the milestone check sets it explicitly rather than relying on how a service
stopped by systemd at shutdown is recorded. `mix autostart status` says nothing about services; a
line in its rendering points at `mix service list`'s `AUTOSTART` column when `at_boot` is on.

### D5 — A new ADR for the daemon's lifetime

[overview.md](../architecture/overview.md) states the daemon's lifetime as *login → logout*, and
T85b's code comments repeat it as the reason linger was not called. With `at_boot: on` it is
*boot → shutdown*. That is a cross-cutting statement in an architecture document and an accepted
design's reasoning, so it changes through **ADR 0064 — "The daemon may start at boot, by the
account's own linger"**: the lifetime becomes *login → logout, or boot → shutdown when asked*;
linger is turned on only by `enable_at_boot`, never by an install or an update; and no root process
or `PrivilegedOp` is involved. ADR 0016 is not edited — its decisions all still hold.

The module comment in `linux/autostart.rs` that says linger is *"deliberately not called"* is
rewritten to point at the ADR, since it would otherwise describe a decision that no longer holds.

### D6 — The shape in `mixengine-platform`

```rust
/// Whether this machine starts the user's manager, and so the unit, before anyone logs in.
pub enum BootStart { Unsupported, Off, On }

pub trait ServiceInstaller {
    // the three T85b methods, unchanged
    fn boot_state(&self) -> Result<BootStart> { Ok(BootStart::Unsupported) }
    fn enable_boot(&self) -> Result<BootStart> { Err(unsupported()) }
    fn disable_boot(&self) -> Result<BootStart> { Err(unsupported()) }
}
```

Default bodies, so Windows and macOS stay as they are and answer with a typed
`UnsupportedPlatform` naming why (*Out*), never `todo!()`. The platform knows only on or off; *who*
turned it on is the daemon's (D3), because it is MixEngine's state in SQLite and the platform crate
holds none. `enable_boot` reports whether it changed anything by comparing `boot_state` before and
after, which is what D3's record is written from. The mock records calls and holds a `BootStart`
a test sets.

The Linux half is a file of its own, `linux/linger.rs`: `linux/autostart.rs` is at 583 lines with
its tests, and linger is a second tool (`loginctl`) with its own parsing and its own system test.
`Unit` delegates the three methods to it.

## API

| Method | Params | Answers | Writes |
| --- | --- | --- | --- |
| `autostart.status` | none | `AutostartReport` | clears a stale D3 record |
| `autostart.enable` | none | `AutostartReport` | the unit |
| `autostart.disable` | none | `AutostartReport` | removes the unit; linger off if D3 |
| `autostart.enable_at_boot` | none | `AutostartReport` | the unit; linger on; D3 record |
| `autostart.disable_at_boot` | none | `AutostartReport` | linger off if D3 |

`AutostartReport.at_boot` is serialised `snake_case` (`unsupported`, `off`, `on`,
`on_by_account`). Bindings regenerated with `packaging/bindings.sh`.

Errors: `enable_at_boot` on a machine answering `Unsupported` → `Error::Unsupported` with the
reason; polkit refusing → the platform `Error::Command` carried to the wire with the `sudo loginctl
enable-linger <user>` hint.

## CLI

```
mix autostart status              # adds: "at boot: yes, before anyone logs in (MixEngine turned on linger for ubuntu)"
mix autostart enable --at-boot    # autostart.enable_at_boot
mix autostart disable --at-boot   # autostart.disable_at_boot — keeps starting at login
mix autostart disable             # unchanged meaning; also undoes MixEngine's linger
```

The render says *at boot* and *before anyone logs in*, not *linger*, except where it names what was
changed on the machine, because "linger" is systemd's word and the person asked for a server that
comes back after a reboot. The *at boot* line is printed only when `at_boot` is not `unsupported`,
so Windows and macOS print what they print today. `--at-boot` on a machine answering `unsupported`
is the daemon's refusal, rendered, not a flag the CLI hides by OS — the CLI holds no business logic.

## MixLab

Settings → MixEngine → **Autostart** ([AutostartSection.tsx](../../apps/desktop/src/modules/mixengine/screens/Settings/AutostartSection.tsx))
gains a second checkbox under *Start MixEngine when I log in*: **Also start at boot, before anyone
logs in**. It is drawn only when `at_boot` is not `unsupported`, so Windows and macOS see the screen
they see today.

- Ticking it calls `autostart.enable_at_boot` (and so also ticks the login box); unticking calls
  `autostart.disable_at_boot`.
- `on_by_account` draws the box ticked and disabled, with a muted line: *Your account already starts
  its services at boot; MixEngine leaves that setting alone.*
- An error carrying the `sudo loginctl …` hint goes to the section's existing error banner.

Both strings in `en.ts` and `vi.ts`. `client-surface.md` §9 gains the two methods, and
`check-client-surface.mjs` sees them reachable from `api.ts`.

The window matters here even though the milestone machine has no desktop: MixLab on a Linux desktop
can lose its daemon at logout too, and the later remote control of a headless host (phase 34's
goal) will draw this same section against that host.

## Testing

- **Platform, unit:** parsing `yes`, `no`, the *not logged in or lingering* error and anything else;
  the refusal text names `sudo loginctl enable-linger`; the mock recording linger on/off.
- **Daemon, unit:** every row of D3 against the mock — already on, set by us, cleared by hand, a
  second home — `at_boot` composed for each, and `enable_at_boot` leaving the unit registered when
  the linger half fails.
- **Platform, system suite** (`#[ignore]` + `MIXENGINE_SYSTEM_TESTS`, testing.md rule 1): run from
  a transient user unit (`systemd-run --user --wait`) as well as from the shell, so the case with no
  session — measurement 5 — is the one proven: `enable-linger <uid>`, read back, `disable-linger
  <uid>`, read back. Skipped with a printed reason when the account already lingers, so the suite
  never turns off a linger it did not turn on.
- **CLI:** render tests for the four `at_boot` values.
- **MixLab:** `settingsState` tests for the checkbox's three presentations.
- **Milestone M34, by hand on this machine:** with the file store chosen and a MariaDB first-run
  (T194), `mix service autostart <mariadb> --on` and `mix autostart enable --at-boot`; log out of
  every session; reboot; then over SSH, `systemctl --user show mixengined -p ActiveEnterTimestamp`
  is earlier than the login in `last`, and `mix database` connects with the stored password. The
  reboot is the person's to perform.
