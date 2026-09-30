# 0052. A build that is not a release keeps its own credentials

**Status**: Accepted. It extends [0024](0024-a-build-that-is-not-a-release-keeps-its-own-home.md)
from the home to the credential store, and leaves 0024 as it is.
**Date**: 2026-09-24

## Context

`cargo test --workspace` on a developer's Mac stopped more than once per run for this dialog:

> mixengined wants to use your confidential information stored in "mixengine" in your keychain.

A daemon out of `target/debug` is not signed. The Keychain decides whether it recognises a program
by its signature, so every rebuild is a program it has never seen, and it asks for the login
password before handing over anything an earlier build wrote.

A test home is new and holds nothing an earlier build wrote, with one exception. T126's migration
reads the pre-T126 address `<service>/<user>` when the new `<home-id>/<service>/<user>` is empty,
and every home on the machine shares the old address. On a machine that also *uses* MixEngine, that
entry is the developer's real password. `crates/mixengine-cli/tests/upgrade.rs` was caught doing
this on 2026-09-24: it starts a daemon on a pre-T126 fixture that has `mariadb@main` set to
autostart. Had the dialog been answered, the test daemon would have copied the real MariaDB root
password into its temporary home.

ADR 0024 already gives a development build a home of its own, for the same underlying reason: a
working tree is not a release and must not touch a release's state. The credential namespace
(`mixengine`) was left shared. `cargo run` and `tauri dev` therefore ask the same question after
every rebuild. On Linux, a desktop session asks to unlock its keyring. On Windows, nothing asks, but
tests still write to the developer's Credential Manager.

## Decision

**The default credential store is decided by where the binary came from, exactly as the default
home is.**

| Build | Store | Where |
| --- | --- | --- |
| release (`MIXENGINE_RELEASE` set by `packaging/stage.sh`) | the OS store | Credential Manager / Keychain / Secret Service |
| anything else | a file in the home | `<root>/credentials.json` |

- `mixengined --credential-store <os|home>` (also `MIXENGINE_CREDENTIAL_STORE`) overrides the
  default. **A release refuses `home`** and fails the start with a sentence saying why, before it
  opens a home. A release that could be talked into keeping passwords in a plain file would have a
  downgrade switch.
- The file is one more implementation of `mixengine_platform::Keyring`, reached through
  `mixengine_platform::host_with(Credentials::File(…))`. `host()` keeps the OS store, so the
  platform suite still tests the real one. The daemon builds a single host with the chosen store
  and hands it both to the registry and to the elevation queue, whose `host()` the API passes on to
  extensions and databases.
- The file is `{ "version": 1, "entries": { "<service>": { "<key>": "<secret>" } } }`, written through
  `write_private`, so it is owner-only on all three systems. A file that does not parse, or has a
  version this build does not read, is an error naming the file and never an empty store. Treating
  it as empty would make first-run code generate new passwords over existing ones.
- A default rather than an opt-in: 21 places across 19 test files start `mixengined`, and a flag
  each one has to remember is a flag the next one will forget.

The design, with the reasoning for each point, is
[the T184 spec](../specs/2026-09-24-t184-a-build-that-is-not-a-release-keeps-its-own-credentials-design.md).

## Consequences

- **A test run never raises a credential dialog, on any system**, and never reads another home's
  credential through T126's fallback. The fallback now looks in the test home's own file, where
  nothing was ever written.
- **The standalone client and the desktop window read a managed database's password from the OS store**, at the
  `SecretAddress` the daemon hands them. Against a development daemon on the file store they find
  nothing. Someone working on that hand-off starts the daemon with `--credential-store os`.
- **CI's `services` and `bench` jobs set `MIXENGINE_CREDENTIAL_STORE=os`.** They run real servers
  against real credentials, and they are where the OS store is exercised end to end.
  `.github/scripts/test-no-network.sh` forwards the variable into its network namespace.
- **Plain text on disk, for development homes only.** The file is protected exactly as much as the
  CA private key in `certs/`. Nothing copies it out of the home:
  - `daemon.bundle` packs a closed list of parts and never walks the root.
  - The desktop window's sync reads the window's own data, not a daemon home.
  - `mix uninstall` removes it with the home, and `--keep-home` keeps it with the home.
- **A release is unchanged.** T126's cross-home read still exists in a release, and fixing it is
  separate work.
- Entries that test runs before this change left in a developer's store stay there until the
  developer removes them. Nothing lists them yet.
