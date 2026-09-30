---
status: implemented
date: 2026-09-24
task: T184
---

# A build that is not a release keeps its own credentials

## The problem

`cargo test --workspace` on a developer's Mac raises this, more than once per run:

> **mixengined wants to use your confidential information stored in "mixengine" in your keychain.**
> To allow this, enter the "login" keychain password.

It was caught in the act on 2026-09-24, with a process watcher running beside the suite and a
screenshot of the dialog. One occurrence is pinned to a single test binary: at 22:45:05 the only
thing this worktree was running was `crates/mixengine-cli/tests/upgrade.rs` and the daemon it had
started, and no other checkout was running anything. The other occurrence (22:35:07) could not be
pinned, because another checkout was running its own suite at the same time.

**Why the Keychain asks.** A daemon out of `target/debug` is not signed, so every rebuild is a
program the Keychain has never seen. The Keychain asks before it hands an item to a program it does
not recognise, and it answers that question with the program's signature. So an item one build
wrote is a stranger's item to the next build. `apps/desktop/src-tauri/src/secrets.rs` measured the
same thing for the standalone client and designed its vault around it.

**Why a test daemon reads an item it did not write.** Every test starts its daemon on a fresh
temporary home. That home cannot hold anything an earlier build wrote, except for one path: T126's
migration in `crates/mixengine-daemon/src/secrets.rs`. When a read finds nothing at
`<home-id>/<service>/<user>`, it tries the pre-T126 address `<service>/<user>`, which is **shared by
every home on the machine**. `upgrade.rs` starts a daemon on a fixture database that holds
`mariadb@main` with `autostart = 1`. That fixture is genuinely a pre-T126 home, so the migration does
exactly what it was designed to do. It reads `mixengine` / `mariadb@main/root`, and on a machine
that also *uses* MixEngine that entry is the developer's real MariaDB root password, written by a
different binary. So the Keychain asks.

**Allowing it is worse than the dialog.** If the password is given, the test daemon copies the real
credential into its temporary home. That is the cross-home leak the module's own documentation
admits ("An entry at the old address may already be another home's").

**It is not only tests.** `cargo run -p mixengine-daemon` and `tauri dev` start the same unsigned
daemon against the developer's `MixEngine-dev` home (ADR 0024). The home is already separate, but the
credential namespace is not, so that daemon asks again after every rebuild for every credential it
reads.

The same shape exists off macOS, with smaller symptoms:

- **Linux**: a desktop session asks to unlock the Secret Service collection.
- **Windows**: Credential Manager never asks, but test daemons still write to, and read from, the
  developer's real store.

## What this is not

- **Not the elevation prompt.** `osascript … with administrator privileges`, UAC and `pkexec` are
  raised only by `Elevation::run`. The tests that reach it are `#[ignore]`d **and** gated on
  `MIXENGINE_SYSTEM_TESTS=1` (testing rule 1). Closing that gap was a separate change.
- **Not a change to released builds.** A release keeps using the OS credential store, exactly as
  today.
- **Not a fix for T126's cross-home read** in released builds. That is the next section's open
  question 2, not this spec's work.

## Design

### D1. The credential store is chosen by where the binary came from, like the home

`mixengine_platform::RELEASE` already decides the default home (ADR 0024). It now decides the
default credential store too:

| Build | Default store | Where it lives |
| --- | --- | --- |
| release (`MIXENGINE_RELEASE` set by `packaging/stage.sh`) | the OS store | Credential Manager / Keychain / Secret Service |
| anything else (`cargo build`, `cargo run`, `cargo test`, `tauri dev`) | the home's own file | `<root>/credentials.json` |

**Default-safe rather than opt-in.** There are 21 places across 19 test files that spawn
`mixengined`. A flag each of them must pass is a flag the 22nd will forget, and forgetting it brings
the dialog back without any test failing. With a default, nothing has to remember.

### D2. One flag to override it, refused in a release

`mixengined --credential-store <os|home>` (also `env = "MIXENGINE_CREDENTIAL_STORE"`, the way
`--home` takes `MIXENGINE_HOME`), parsed in `Args` like every other piece of configuration.

- **In a development build**, `os` is the way back to the real store. Use it for the one workflow
  that needs it: the standalone client or the desktop app reading a managed database's password through
  `SecretAddress` (D6).
- **In a release**, `home` **fails the start** with a sentence saying why. A release that could be
  talked into keeping passwords in a plain file is a release with a downgrade switch. The flag is
  still accepted with `os`, so a launchd plist that spells out the default is not broken.

### D3. The file store lives in `mixengine-platform`, behind the existing `Keyring` trait

`Keyring` is already the seam: `secret`, `set_secret`, `forget_secret`, with `(service, key)` as the
whole address. The file store is one more implementation of it, so nothing above `mixengine-platform`
learns that two stores exist.

- **Construction.** `mixengine_platform::host()` stays as it is, and the OS store remains its
  keyring, so `crates/mixengine-platform/tests/secrets.rs` goes on testing the real OS store. A second
  constructor takes the store to use, and the daemon calls it once, in `main`.
- **One host in the daemon reaches the store.** The daemon calls `mixengine_platform::host()` in 13
  places, but only five modules call `keyring()` (`secrets`, `databases`, `extensions`,
  `services::databases`, `services::first_run`), and `mixengine-core` never calls it. Those five
  are reached through two hosts: the registry's, and the elevation queue's, which the API hands on
  to extensions and databases. The end-to-end test found the second when it was still a separate
  `host()`. `serve` now builds one host with the chosen store and gives it to both. The other
  eleven calls reach pools, activation, shims, autostart and machine facts, and are left alone. A `disallowed_methods` lint was considered and dropped: `clippy.toml` is workspace-wide, and
  the lint would flag about thirty calls in eleven files that have nothing to do with credentials.
  The guard is the end-to-end test below. It fails if a credential written through the daemon ends
  up anywhere other than the home's file.
- **The file.** `<root>/credentials.json`, written through `write_private`: owner-only on all three
  systems, and restricted before any byte reaches it. It sits at the root rather than in `data/`,
  because a `[paths]` override can move `data/` out of the home, and the credentials belong to the
  home. Its content is `{ "version": 1, "entries": { "<service>": { "<key>": "<secret>" } } }`.
- **Writes** replace the whole file, from a copy of the document held under an in-process `Mutex`.
  One daemon owns a home (the home lock), so there is no second writer to coordinate with. A missing
  file is an empty store. A file that does not parse is `Error::Secret` naming the path, never
  "empty": reading an unreadable store as empty would make first-run code generate new passwords
  over the existing ones.
- **Plain text, and said so.** The file is as protected as the home's CA private key in
  `certs/`, and no more. That is acceptable for a home no release will ever open (ADR 0024), and it
  is the reason for D2's refusal.

### D4. The T126 fallback reads the same store it was given

Nothing changes in `secrets.rs`. A home on the file store looks for the pre-T126 address **in that
file**, where no other build ever wrote anything, so it finds nothing and asks nobody. `upgrade.rs`
keeps proving what it proves today, which is that the daemon starts and reads an old database. It
stops touching the developer's Keychain as a side effect.

### D5. Seeing which store is in use

- The daemon logs one line at start: `credentials: <root>/credentials.json (development build)` or
  `credentials: the operating system's store`.
- `mix doctor` does not report the credential store today, and this spec does not add a row for it.
  The log line is enough for the people this affects, who are developers. A row, or a field in
  `status`, is a later task if the desktop Settings screen ever needs to show it.

### D5a. Nothing that copies a home takes the file with it

`daemon.bundle` already includes only a closed list of five parts, so `credentials.json` cannot get
into a bug report. The plan checks every other path that copies or uploads part of a home (backups,
sync, `uninstall`'s inventory) and states, for each one, that it leaves this file out or deletes it
together with the home.

### D6. The cost: handoff in a development build

The standalone client and the desktop app read a managed database's password straight from the OS store, at the
`SecretAddress` the daemon hands them (`mixengine-proto/src/database.rs`,
`apps/desktop/src-tauri/src/modules/db/handoff.rs`). On a development daemon using the file store,
that read finds nothing.

This is accepted, not worked around. The alternative is the dialog on every rebuild, which is what
that workflow costs today anyway. A developer working on the handoff itself starts the daemon with
`--credential-store os`. `docs/operations/` gains that sentence where it describes running a
development build.

**CI is not a release either**, so its daemons would move to the file too. That would quietly drop
the one place the OS store is exercised end to end: the `services` and `bench` jobs, which run real
servers against real credentials on runners with nobody to prompt. Those jobs set
`MIXENGINE_CREDENTIAL_STORE=os`. `.github/scripts/test-no-network.sh` forwards a fixed list of
variables into its namespace, and the variable joins that list, or the Linux leg would switch stores
with every job still green.

## Tests

- **Platform (file store, all three systems).** Each test runs against a `TempDir`:
  - a round trip;
  - reading an absent entry gives `None`;
  - `forget_secret` twice succeeds;
  - a second key does not disturb the first;
  - an unparsable file is `Error::Secret`, not an empty store;
  - the file is owner-only (a mode check on Unix, `is_private_file` on Windows).
- **Daemon (the decision).** The choice is a pure function of `(RELEASE, flag)`, with a table test
  over all four cells, including release plus `home` refused.
- **Daemon (end to end).** `crates/mixengine-cli/tests/extension_lifecycle.rs` already installs an
  extension whose configuration asks for `{secret}`, which is written through the daemon's keyring.
  It gains three assertions: the value lands in `<root>/credentials.json` under
  `<home-id>/extensions/phpmyadmin/config`, it is the value written into the rendered file, and the
  uninstall removes it. Today that test writes to the developer's real Keychain, and its first read
  goes through T126's fallback to `extensions/phpmyadmin/config`, which is shared by every home on
  the machine.
- **The regression itself.** Run `cargo test --workspace` beside the process watcher used to find
  this, and expect no `SecurityAgent` launch. That run is how the change is verified, not a test in
  the suite: a test cannot observe a dialog the OS draws for another process.

## Documents

- **New ADR 0052**, *A build that is not a release keeps its own credentials*. It extends
  ADR 0024 rather than editing it, as the working agreements require.
- `docs/features/services.md`: the sentence saying where a service's credential lives gains the
  development-build row.
- `docs/standards/testing.md`: a line under rule 2 saying that a test home's credentials are in the
  home, like everything else about it.
- `docs/operations/`: the `--credential-store os` sentence from D6.
- The roadmap: a task in the phase this belongs to, which the plan names.

## Open questions

1. **Settled: T184, in phase 24** (*A test job that scales*). A test run that stops for a password
   dialog does not scale either.
2. **T126's fallback in released builds.** It still reads an address every home on a machine shares,
   and still copies whatever it finds into the reading home. Restricting it to a home whose database
   predates T126 would close the leak for real users too. That is a product change with its own
   spec, and this one only records the finding.
3. **Old entries on developer machines.** Test runs before this change may have left `mixengine`
   entries in the developer's store. They cannot be removed automatically, because a release's
   entries sit in the same namespace, and nothing lists them yet. `secrets.rs` defers exactly that
   listing to a later task. This spec does not change that, so those entries stay until the
   developer removes them by hand.
