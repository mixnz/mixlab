---
status: implemented
date: 2026-09-26
task: T182d
---

# An uninstall forgets what it kept in the credential store

Follows [T182](2026-09-24-t182-removing-mixlab-is-one-act-design.md) and
[T182b](2026-09-25-t182b-a-helper-that-keeps-up-and-an-uninstall-that-finishes-design.md), and is
built after [T186](2026-09-26-t186-one-keychain-question-per-home-design.md), which provides
`Keyring::keys` and makes the daemon's entries one Keychain item per home on macOS. Found on
2026-09-26: MixLab was uninstalled on Windows, installed again, and the Sync screen was still signed
in to the old account.

## What is wrong

Two programs write to the operating system's credential store, and nothing ever removes what they
wrote:

| Writer | Service | Accounts | Holds |
| --- | --- | --- | --- |
| `mixengined` (release only, ADR 0052) | `mixengine` | `<home-id>/<service-id>/<user>` (T126), and pre-T126 `<service-id>/<user>` | the managed databases' passwords |
| the MixLab window | `MixLab` | Windows/Linux: one per saved connection (a uuid), plus `sync-master-key`. macOS: everything in one `vault` item | saved connections' passwords, and sync's refresh token and master key `MK` |

`mix uninstall` promises that it lists and removes everything MixLab wrote outside its home, and
that its report is measured rather than asserted. Neither store appears in the plan or in the
report. After a complete uninstall (the home deleted), this machine still holds:

- a sync refresh token and `MK`. A new install reads them and signs in without asking;
- passwords for databases whose `data/` no longer exists.

A development build keeps its credentials in `<root>/credentials.json` (ADR 0052), which already
goes with the home. This spec is about the OS store, which in practice means a release.

## Decisions

### D1. Credentials follow the home

The daemon's entries for this home, and the window's entries, are removed when the home is removed
and kept when it is kept (`--keep-home`, or "Also delete MixLab's data" unticked). The reason: a
kept `data/` without its passwords is a database nobody can open. A kept window-data folder without
its passwords is a list of connections that all fail.

The daemon's entries also follow what **actually** happens to the home, not just what was asked for.
If `arm_the_home` keeps the home because something outside it is still there, the credentials are
kept too. A later run then still finds a home and its passwords together.

### D2. Two new rows in the report

`ResidueId` gains two ids. Neither needs the helper: the credential store belongs to the user, so
there is no token and no prompt from our side.

| Id | What | Location | Present when |
| --- | --- | --- | --- |
| `Credentials` | "N passwords this home's databases use" | `mixengine` · `<home-id>/…` | the OS store holds at least one key with this home's prefix |
| `WindowCredentials` | "MixLab's saved passwords and sync sign-in" | `MixLab` | `is_the_windows_home` and the store holds at least one key under `MixLab` |

The window's row has the same guard as `WindowData`. The window's entries belong to one user, not
one home, so only an uninstall of the home the release window drives may take them. A row that has
nothing is omitted, as the window's folders are.

The outcomes use the existing vocabulary: `Planned`, `Kept { because }`, and after the act either
`Removed` or `Failed`, from a second reading of the store.

### D3. Found with `Keyring::keys`, removed with `forget_secret`

The rows are read with T186's `Keyring::keys`, which lists names and never values. The daemon's row
keeps the keys with this home's prefix, and the window's row keeps everything under `MixLab`.
Removal goes one key at a time through the existing `forget_secret`, so there is no new write path
and no OS call outside `mixengine-platform`. On macOS, T186's vault means forgetting this home's keys
removes one item.

### D4. The daemon removes them, just before the home goes

Just before `arm_the_home`, and only when no other row is `Failed` or `Enqueued` (the same question
`arm_the_home` asks), the daemon forgets both rows' keys. It then reads the store again and settles
each row as `Removed` or `Failed`. A failure here counts like any other: the home is kept. So there
is never a removed home whose passwords are still in the store, nor a kept home whose passwords are
gone. When another row is not clear, both rows become `Kept`. Waiting until the process exits would
be too late, because the report has already been sent by then. And unlike the directories, a
credential has no open handle that has to close first.

### D5. What is deliberately left

- **The pre-T126 `<service-id>/<user>` entries under `mixengine`.** They have no home prefix, so no
  uninstall of one home can claim them. No release predates T126, so no user's machine has any.
- **The `<Old>` service.** A standalone client may still be installed and in use (T104, D4).
- **The device on the sync server.** The uninstall forgets this machine's sign-in but does not tell
  the server. The device stays in the account's device list until it is removed from another
  machine. Revoking it would mean the daemon talking to the sync server, which is the window's job,
  not the daemon's.

### D6. The window must not write them back

The Windows uninstaller already closes MixLab before `mix uninstall`. On macOS and Linux the window
can still be open. It holds the vault and the sync session in memory, and the next saved connection
or token refresh writes them back. Until T182a gives those systems an uninstaller of their own,
the handbook says to quit MixLab before running `mix uninstall`, and the plan's row says so as well.

## macOS: how many questions

Windows' Credential Manager never asks, and the Linux Secret Service asks at most once, to unlock
the collection. On macOS, listing returns attributes only and does not ask. Deleting an item may ask
when the Keychain does not recognise the program by its signature. After T186 that is at most two
items: this home's vault and the window's `vault`. So at most two questions, and the uninstall's one elevation prompt stays one.

## What changes

- `mixengine-proto`: two `ResidueId`s, `ALL` from 16 to 18, then `bash packaging/bindings.sh`.
- `mixengine-daemon`: `inventory::credential_rows`, the act in `arm_the_home`, and
  `needs_the_helper` stays at seven.
- `mixengine-cli`: rendering the two rows. `tests/uninstall.rs` is extended with the file store,
  which a development daemon already uses.
- Documentation: the list in `docs/guide/{en,vi}/uninstalling.md`, a `Fixed` line in
  `CHANGELOG.md`, a case in `packaging/windows/uninstall-check.md`, and T182d in
  `docs/roadmap/phase-9-ship.md` after T182c.
