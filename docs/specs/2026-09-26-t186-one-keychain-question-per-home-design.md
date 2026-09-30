---
status: implemented
date: 2026-09-26
task: T186
---

# One Keychain question per home

On macOS, every update of MixLab asks for the login password once for **each** of `mixengined`'s
credentials. This task brings that down to once per home. It is a prerequisite of
[T182d](2026-09-26-t182d-an-uninstall-forgets-what-it-kept-in-the-credential-store-design.md), which
has to remove these entries and would otherwise meet the same questions.

## What is wrong

The Keychain decides whether it recognises a program by its code signature, and it asks once per
**item**. A release is not signed (T86a), so after every update `mixengined` is a stranger to every
item an earlier build wrote.

The window met this first and fixed it: everything it keeps under `MixLab` is one `vault` item on
macOS (`apps/desktop/src-tauri/src/secrets.rs`). The daemon never did. Under `mixengine` it keeps one
item per credential, at `<home-id>/<service-id>/<user>` (T126), and reads them:

- when a service starts, for each `EnvValue::Keyring` in its spec
  (`services/runner.rs`: MariaDB's and Postgres's superuser, and a PHP-FPM pool's site
  credentials);
- when `database.credentials` or `database.open` is asked for.

A machine with two MariaDBs and a Postgres set to start at login sees three dialogs in a row after
each update, before anything works. The window then reads the same items directly, for a saved
connection that points at a managed database (`secrets.rs`, `read_mixengine_entry`). Each of those
is another stranger, and another dialog.

On Windows and Linux none of this happens. Credential Manager never asks, and the Secret Service asks
once to unlock the collection.

## Decisions

### D1. On macOS, one item per home holds all of that home's credentials

Under service `mixengine`, account `<home-id>`, a JSON object:

```json
{ "version": 1, "entries": { "<service-id>/<user>": "<secret>" } }
```

The address callers use does not change. `<home-id>/<service-id>/<user>` is still what the daemon
composes, what `SecretAddress` carries, and what a saved connection's `keyringRef` holds. The vault
is a detail of how the macOS store keeps them: the key is split at its first `/` into item and
entry.

- **Per home, not one for the whole service.** Homes are independent (T126). One item for all of
  them would let one home's write race another's, and would make T182d's "remove this home's
  credentials" into an edit of something other homes use.
- **macOS only**, for the window's reason. Credential Manager refuses a secret over 2560 bytes, which
  a home with a dozen database accounts would pass. Neither Windows nor Linux has the problem this
  solves.
- **A key with no home in it** (the pre-T126 `<service-id>/<user>`) stays an item of its own. It
  belongs to no home, and only T126's fallback still reads it.

### D2. It lives in `mixengine-platform`, behind the existing `Keyring`

`sys::secrets` on macOS gains a `Vaulted` store that wraps the per-item one. `secret`, `set_secret`
and `forget_secret` keep their signatures. Putting it there does three things:

- The daemon's callers do not change, and T126's fallback in `mixengined`'s `secrets` module keeps
  working over it.
- The window can read the same vault without knowing its format. It already depends on
  `mixengine-platform` (the layering test allows it). `read_mixengine_entry` switches from its own
  `OsStore` to `mixengine_platform`'s keyring for service `mixengine`. The window's own `MixLab`
  store is unchanged.
- The file store (ADR 0052) and the other two systems are untouched.

**Caching**, as the window's vault does: an item is read once and held for the run, behind a
`Mutex`, so services starting in parallel wait for the first read instead of each raising a dialog.
Only the daemon writes, and one daemon holds a home (its lock), so the writer's cached copy is the
truth: a write changes one entry in it and writes the whole item back, without reading the Keychain
again. A second read would be a second chance for a person who pressed "Allow" rather than "Always
Allow" to be asked again. A reader (the window) that
misses in its cached copy reads the item once more before answering `None`, because the daemon may
have added the account since.

### D3. Nothing is moved

No release has been installed anywhere yet, so no machine holds per-item entries worth keeping. A
development machine that does can uninstall and install again. So there is no migration and no
fallback to the per-item address: on macOS a key with a home in it is read from the vault and
nowhere else.

### D4. `Keyring::keys`: the names under a service, never their values

The `keyring` crate cannot enumerate. T182d needs to, and a vault is the one place that knows which
keys it holds, so the capability is defined here, beside the vault:

```rust
/// The keys under `service`, never their secrets.
fn keys(&self, service: &str) -> Result<Vec<String>>;
```

| Store | How |
| --- | --- |
| Windows | `CredEnumerateW` over every credential, keeping the generic ones whose target ends in `.<service>`: `keyring` 3 names a generic credential `<user>.<service>`, and the API's filter only takes a prefix |
| macOS | `SecItemCopyMatching` on `kSecClassGenericPassword` with `kSecAttrService`, `kSecReturnAttributes`, `kSecMatchLimitAll`. Only attributes come back, never data, so nothing asks. A vault item contributes the keys inside it, which costs a read: the one read D2 caches anyway |
| Linux | Secret Service `SearchItems` on the `service` attribute `keyring` writes |
| file (ADR 0052) | the keys of `entries[service]` |

## What a person sees afterwards

| After an update, on macOS | Before | After |
| --- | --- | --- |
| `mixengined`, per home | one question per credential it reads | one |
| the window, for managed databases | one per saved connection it opens | one |
| the window, for its own `MixLab` items | one (`vault`) | one, unchanged |

Three at most, and none on Windows or Linux. Getting to zero needs a signed release, which is T94's
question, not this task's.

## What is deliberately left

- **A standalone client** reading `mixengine` entries by address (T84). On macOS it will no longer
  find what the daemon keeps in the vault, and it will show an empty password field. The window is
  the only database client (ADR 0038), and the standalone client's own history ended at 0.0.33.
- **The pre-T126 per-item entries.** They are unchanged, as D1 says.

## What changes

- ADR **0055**, *The daemon's credentials are one Keychain item per home on macOS*. It extends T126's
  addressing without changing the wire, and records D1's three choices.
- `mixengine-platform`: `Keyring::keys` (four implementations), `Vaulted` in `sys::secrets` for
  macOS. Unit tests over a map-backed store in the style of the window's `secrets.rs` tests: a
  count of how many times the store is touched, a write that does not lose another entry, and the
  reader re-reading on a miss. The real Keychain is checked in the `system` job on macOS, under a
  service name of the test's own.
- `apps/desktop/src-tauri`: `read_mixengine_entry` through `mixengine_platform`.
- `docs/architecture/platform-abstraction.md` (the `Keyring` row), `docs/architecture/security-model.md`
  where it describes where passwords live, a `Changed` line in `CHANGELOG.md`, and T186 in
  `docs/roadmap/phase-9-ship.md`, before T182d.
