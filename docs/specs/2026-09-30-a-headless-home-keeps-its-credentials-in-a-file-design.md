---
status: approved
date: 2026-09-30
task:
  - T194a
  - T194b
  - T194c
  - T194d
  - T194e
  - T194f
---

# A headless home keeps its credentials in a file

## The problem

A release on a Linux machine with no desktop cannot run a database.

A managed MariaDB, MySQL or PostgreSQL is bootstrapped with a generated root password, and the
password is kept in the operating system's credential store. On Linux that store is the D-Bus
Secret Service (`gnome-keyring`, `kwallet`), and a machine with no desktop has none:
[ADR 0013](../decisions/0013-reading-the-d-bus-error-name-to-tell-an-absent-store.md) measured it —
*a plain `ssh` login on a server: no `DBUS_SESSION_BUS_ADDRESS`, no `XDG_RUNTIME_DIR`, and
autolaunch refused*. The first run of every database therefore fails with `UnsupportedPlatform`,
and the workaround it prints (`dbus-run-session`, `gnome-keyring-daemon --unlock`) is a workaround
for a session, not for a server that has to come back on its own after a reboot. An extension's
configuration secret fares no better: with no store to keep it, it is silently empty.

That was a deliberate trade when [T33](2026-08-20-t33-mariadb-design.md#no-credential-store) made
it: *"the cost is borne by nobody using MixEngine as intended — every desktop, on all three
systems, has a store."* It is no longer true. The headless distribution (T105) is a way MixEngine
is meant to be used, and a mini PC in a cupboard, controlled from another machine, is the host
this design is the first step towards. On that machine the refusal is not an edge case: it is the
whole database half of the product.

## What changed since T33 refused a file

T33 named what a file would cost, *"so that whoever reverses this knows what they are trading"*:

1. **`EnvValue` grows a variant.** No longer: [T184](2026-09-24-t184-a-build-that-is-not-a-release-keeps-its-own-credentials-design.md)
   built the file as one more implementation of `mixengine_platform::Keyring`. A recipe asks the
   host's keyring and never learns which store answers it.
2. **A credential gains two sources of truth.** Only if one daemon could read both stores. It
   cannot: a daemon builds one host with one store (ADR 0052). What this design must add is the
   guarantee that *a home* always gets the same store, start after start — D1.
3. **A plaintext secret exists on disk where this project has never had one.** It has one now:
   every development home keeps `<root>/credentials.json`, owner-only, beside the CA private key
   that was already there. What remains true is that **a release** has never had one, and this
   design is the decision to allow it, in one place, said plainly — D5.

## Principle

**The credential store is a property of the home, chosen once by a person, and never switched by
MixEngine on its own.** A Linux release may choose the file; nothing else about a release changes.

## D1. The choice is recorded in the home

`settings` gains the key `credential_store`, value `"os"` or `"home"`. A home with no row is `os`
on a release and `home` on a development build — today's defaults, unchanged.

The daemon reads the row after `Store::open` and before it builds the host that reads credentials
([main.rs](../../crates/mixengine-daemon/src/main.rs)). The host built earlier, for `open_home`,
keeps the OS store and reads no credential, as its comment there already says.

**A flag per start cannot be the mechanism, because most starts pass no flag.** `mix` starts the
daemon on demand as `mixengined --detach --home <root>`
([autostart.rs](../../crates/mixengine-cli/src/autostart.rs)), and so will a systemd unit (T195)
and the window. With the choice on the command line, the first `mix` command after a reboot would
start a daemon on the build's default store, and the next first run would write there: two sources
of truth.

Precedence: `--credential-store` (or `MIXENGINE_CREDENTIAL_STORE`) > the recorded row > the build's
default. What the flag means depends on the build:

- **On a release**, a flag that differs from the row is a switch: it is checked as D4 checks one,
  recorded, and the start fails with D4's sentence when D4 refuses.
- **On a development build**, the flag overrides for that start and records nothing — today's
  behaviour, which CI's `services` and `bench` jobs rely on when they set
  `MIXENGINE_CREDENTIAL_STORE=os` for one run (ADR 0052).

## D2. A release may choose the file on Linux, and only when there is no store

A release accepts `home` on **Linux, and only while the operating system's store is absent** —
`Keyring::keys` answering `UnsupportedPlatform`, the reading ADR 0013 made exact. Everywhere else it
refuses, with today's sentence: `credentials::choose` for the flag, `daemon.set_credential_store`
for the method.

- **Linux** is the one system where a session that can run a daemon routinely has no store, and
  the only system the headless host is built for.
- **Only while absent**, because a Linux desktop with a working keyring has no reason to keep a
  file, and every such home would be one more whose passwords the window cannot read beside it.
  The headless machine is exactly the one that cannot answer `keys`.
- **Windows and macOS** always have a store in a logged-in session, which is the only session an
  attached host runs a daemon in. Allowing the file there would be the downgrade switch ADR 0052
  refused, bought for no case.

The absence is read **when a switch is asked for** — at the start that carries a differing flag, or
in `daemon.set_credential_store` — never assumed from the system alone. A home that already chose
the file keeps it if a keyring later appears: the choice is recorded (D1), and D4 governs any change
back.

This amends ADR 0052's rule *"a release refuses `home`"* and T33's *"it does not fall back to a
file"*. Both are accepted decisions, so the change is **a new ADR** that supersedes those two
sentences and nothing else in them.

## D3. Never automatic

A missing Secret Service does **not** switch a home to the file. The first run fails exactly as it
does today, and on a Linux release the hint gains MixEngine's own answer ahead of the D-Bus
workarounds:

> this machine has no credential store; to keep this home's passwords in a file only your account
> can read, run `mix daemon credential-store home`

The sentence is added where the daemon maps `UnsupportedPlatform` for the `Keyring` capability to
a wire error ([error.rs](../../crates/mixengine-daemon/src/error.rs)), not in
`mixengine-platform`: the platform layer does not know `mix` exists, and its own workarounds stay
as they are.

Why not fall back quietly: a Linux *desktop* whose keyring has not started yet — a daemon started
by a systemd unit before anybody logged in, or by a login whose `gnome-keyring` came up late — is
indistinguishable, at that moment, from a server with none. A fallback would put that desktop's
next password in a file and the rest in the keyring: two sources of truth, created by a race, on a
machine where the person never asked for either.

## D4. Switching is allowed only while nothing is stored

`daemon.set_credential_store { store }` records the row. It is **refused while the current store
holds anything for this home**, and the refusal names what it found.

The check asks the store itself, through `Keyring::keys`, for the entries under this home's
address (`<home-id>/…`): a service's root password, a database account from `database.create`, an
extension's configuration secret. State cannot answer it — nothing records a database account or
an extension's secret in SQLite, and a check that missed them would strand them in the store the
home is leaving. Two readings settle the edges:

- **A store that is absent holds nothing.** `keys` answering `UnsupportedPlatform` is the machine
  this design is for, and it allows the switch.
- **A store that refuses is not empty.** A locked keyring, an unreadable file: the switch is
  refused with the store's own reason, never treated as nothing to lose.

Moving the entries across is not built here. A home that must change store after it has stored
something recreates what stored it; a `--move` is a follow-up for when somebody has the case.

The switch takes effect **at the next start**: a running daemon keeps the host it was built with,
and the answer says so and names the way to get there — `mix daemon stop`, after which the next
`mix` command starts the daemon on the new store. Rebuilding the host under a running registry is
a second code path for a command run once in a home's life.

## D5. What the file protects, said plainly

The file is T184's, unchanged: `<root>/credentials.json`, version 1, written through
`write_private` (owner-only), an unreadable or unknown-version file an error and never an empty
store.

Against the Secret Service it gives up **nothing against another program of the same account** —
an unlocked `gnome-keyring` answers any process in the session, and a server's daemon runs as that
account anyway — and **gives up encryption at rest**: a backup of the home, or the disk taken out
of the machine, carries the passwords readable. So does the database's own data directory, which
is on the same disk and no password protects; encryption of the whole disk is what answers a stolen
disk, and the doctor note says so. Every surface that shows the store says which it is:

- the startup log: `<path> (chosen for this home)` rather than `(a development build)`;
- `daemon.status` gains an optional member (ADR 0019)
  `credentials: { store: "os" | "home", choosable: bool }` — `choosable` is whether this daemon
  would accept a switch (D2), decided by the daemon so no client repeats the rule. It is read once
  per run, from the same `keys` call D2 makes, and kept; the plan confirms that call raises no
  unlock prompt on a desktop's Secret Service before relying on it, and if it does, `choosable`
  answers from the system alone and the refusal carries the rest. `mix status` prints the store;
- `mix doctor` adds one check: `Ok` for `os`; for `home`, a `Note` naming the file, the sentence
  above, and whole-disk encryption as the answer to a stolen disk. A note, never a `Problem`:
  there is nothing to repair;
- `mix database create` and `mix database credentials`, which today print *"password in the
  mixengine credentials at …"*, say *"in this home's credentials file"* for a `home` store.

Nothing new copies the file out of the home: `daemon.bundle` packs a closed list and never walks
the root, and `mix uninstall` removes it with the home (ADR 0052's consequences, unchanged).

## D6. The window on the same machine is unchanged

Two places in the window read MixEngine's credentials straight from the OS store: *Explore data* on
the dashboard ([open_in_mixdb.rs](../../apps/desktop/src-tauri/src/modules/mixengine/open_in_mixdb.rs)),
and the `db` toolbox's saved connection, which keeps a `keyringRef` and resolves it on every load
([savedConnections.ts](../../apps/desktop/src/modules/db/savedConnections.ts)). Against a `home`
store both would find nothing — and **neither meets one**, because D2 lets a home choose the file
only on a machine with no store, and a machine with no store has no window beside the daemon.

So nothing in the window changes here. How a window reads a password it cannot read locally is the
question of a window controlling another machine, which can never read that machine's store,
whichever it is; it belongs to that design (see *Not in this design*).

## MixLab

**Settings → MixEngine** gains a *Credentials* section beside *Autostart*:

- it names the store (*your system's keyring* / *a file in this home*) from `daemon.status`, and
  for `home` shows the sentence from D5;
- when `choosable`, it offers the switch (`daemon.set_credential_store`); a refusal is shown in the
  daemon's words, and a success says the change applies at the next start;
- when not, it shows the store and offers nothing.

On the headless machine itself there is no window: the section is for a Linux machine whose daemon
the window can reach, and for the window that will one day control a host from elsewhere. No other
screen changes (D6).

## Testing

The suites run development builds, so what only a release does is proved where the rule lives.

- `credentials::choose`, unit: a Linux release accepts `home` when the store is absent and refuses
  it when the store answers; a Windows or macOS release refuses it with today's sentence; a
  development build is unchanged.
- Precedence, unit: flag > row > default, each pair, on both kinds of build; a release's differing
  flag is a switch and a development build's is not.
- Persistence, `mixengine-cli` suite: a home with the row set to `home`, the test's daemon started
  with `MIXENGINE_CREDENTIAL_STORE` removed from its environment (CI sets it); first-run a MariaDB;
  stop; start again through `mix`'s own autostart; the service starts, and `credentials.json` holds
  its one root entry.
- The absent-store path, Linux: the row set to `os`, `DBUS_SESSION_BUS_ADDRESS` pointing at a
  socket that is not there; the first run fails with `UnsupportedPlatform`, the hint names
  `mix daemon credential-store home` before the D-Bus workaround, and no file is written. (The
  release-only wording of the hint is a unit test on the mapping.)
- D4: with nothing stored, `daemon.set_credential_store` succeeds and `daemon.status` still reports
  the old store until a restart; after a database first-runs, it is refused and names the entry; a
  store that refuses `keys` refuses the switch.
- Nothing quotes a value: the existing `no_error_carries_a_stored_value` holds for every new error,
  including D4's refusal, which names addresses and never what is stored at them.

## Not in this design

- **Sealing the file with the machine's TPM** (`systemd-creds`, or `tss-esapi` directly). Not
  measured: the understanding when this was written is that per-user `systemd-creds` arrived in
  systemd 256 while Ubuntu 24.04 ships 255, that the system-scoped form needs root, and that
  `/dev/tpmrm0` is readable only by the `tss` group. All three are to be measured on the target
  distributions before anyone designs it; the file format carries a `version` so a sealed one can
  follow.
- **Moving credentials between stores** (`--move`). D4 refuses instead.
- **A window reading a password from a `home` store.** Belongs to the design for controlling a host
  from another machine, where the window must ask the daemon (`database.credentials`,
  [ADR 0025](../decisions/0025-a-credential-is-answered-only-by-a-method-that-exists-to-answer-it.md))
  whatever the store. One edge is left until then: a headless home that chose the file, on a
  machine that later gains a desktop and runs MixLab beside the daemon. *Explore data* there finds
  no password — no data is lost, and the person can reach it through `mix database credentials`.
- **Starting at boot without a login** (a systemd user unit and linger) — T195, with its own
  design; this one is what makes that start worth having.
- **T126's cross-home read** of the pre-T126 address. Unchanged and still separate work, as ADR
  0052 already says.
- **Windows and macOS headless hosts.** Not supported (see D2); the attached host uses the store
  its logged-in session already has.

## Tasks

**T194a–T194f** in [phase 34](../roadmap/phase-34-a-headless-host.md):

- **T194a** the ADR superseding the two sentences in D2;
- **T194b** `settings.credential_store`, precedence, `credentials::choose` per system (D1, D2);
- **T194c** the absent-store hint (D3);
- **T194d** `daemon.set_credential_store` and its check, `mix daemon credential-store`, the
  *Credentials* section (D4, MixLab);
- **T194e** `credentials` on `daemon.status`, `mix status`, the doctor check, the startup log, the
  `mix database` wording; bindings (D5);
- **T194f** `security-model.md` and `platform-abstraction.md`'s `Keyring` row, the changelog, this
  design flipped to `implemented`.
