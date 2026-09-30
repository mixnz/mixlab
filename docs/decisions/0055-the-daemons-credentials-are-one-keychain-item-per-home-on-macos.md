# 0055. The daemon's credentials are one Keychain item per home on macOS

**Status**: Accepted. It changes how the macOS store keeps the addresses of
[0032](0032-a-keyring-address-names-the-home-it-belongs-to.md), and leaves the addresses as they are.
**Date**: 2026-09-26

## Context

The Keychain decides whether it recognises a program by its code signature, and it asks once per
**item**. A release is not signed (T86a), so after every update `mixengined` is a program the
Keychain has never seen, for every item an earlier build wrote.

The window met this first and fixed it: everything it keeps under `MixLab` is one `vault` item on
macOS. The daemon never did. It kept one item per credential under `mixengine`, at
`<home-id>/<service-id>/<user>`. It reads them when a service starts (MariaDB's and Postgres's
superuser, a PHP-FPM pool's site credentials) and when `database.credentials` or `database.open` is
asked. A machine with two MariaDBs and a Postgres set to start at login saw three dialogs in a row
after each update. The window then read the same items directly for a saved connection to a managed
database, and each of those was another dialog.

Windows' Credential Manager never asks, and the Linux Secret Service asks once to unlock the
collection.

## Decision

**On macOS, one Keychain item per home holds all of that home's credentials.** Service `mixengine`,
account `<home-id>`, value `{"version":1,"entries":{"<service-id>/<user>":"<secret>"}}`.

- **The address does not change.** `<home-id>/<service-id>/<user>` is still what the daemon
  composes, what `SecretAddress` carries and what a saved connection's `keyringRef` holds. The macOS
  store splits it at the first `/` into item and entry.
- **Per home, not one item for the service.** Homes are independent (0032), and one item for all of
  them would let one home's write race another's.
- **macOS only.** Credential Manager refuses a secret over 2560 bytes, which a home with a couple
  of dozen accounts would pass, and neither Windows nor Linux asks per item.
- **A key with no home in it** (the pre-0032 `<service-id>/<user>`) stays an item of its own.
- **It lives in `mixengine-platform`, behind `Keyring`.** The daemon's callers do not change, and
  the window reads managed-database passwords through `mixengine_platform` rather than its own
  store, so it finds them inside the vault without knowing its format.
- **An item is read once and held for the run.** The daemon is the only writer of a home (its
  lock), so once it has written, its copy is the truth. A reader's copy may be stale, so a miss
  there reads the item once more.
- **Nothing is moved.** No release had been installed anywhere when this was decided. A machine
  with per-item entries uninstalls and installs again.
- `Keyring::keys` lists a service's keys, never their values, on every store. An uninstall needs
  it (T182d), and a vault is the one place that knows which keys it holds.

The design is [the T186 spec](../specs/2026-09-26-t186-one-keychain-question-per-home-design.md).

## Consequences

- After an update on macOS: one question for the daemon per home, one for the window reading
  managed databases, one for the window's own `vault`. Three at most, none on the other two
  systems. Zero needs a signed release.
- A standalone client reading `mixengine` entries by address no longer finds them on macOS. The
  window is the only database client (0038).
- A per-item entry left from before this is not read. It stays in the Keychain until somebody
  removes it.
