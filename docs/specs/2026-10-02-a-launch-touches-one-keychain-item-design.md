---
status: approved
date: 2026-10-02
---

# A launch touches one Keychain item

## The problem

`apps/desktop/CLAUDE.md` says a launch asks for the Keychain at most once: everything MixLab keeps
under service `MixLab` lives in the one `vault` item in `apps/desktop/src-tauri/src/secrets.rs`.
On 2026-10-02 an Apple-silicon Mac (Darwin 24.6.0) showed the first launch of a new build asking
twice, before any connection was opened:

- A 0.0.13 control (CI run 36920716165, linker-signed) asked for two items, both answered
  *Always Allow*: first `mixengine`, then `MixLab`. Relaunching asked nothing.
- The `MixLab`/`vault` item's modification date moved on every launch (`20261002083320Z`, then
  `114001Z`, then `114517Z`). Something writes the vault at start-up.
- After that control had written the vault, reinstalling a build that had already been allowed
  (CI run 36973490349) asked again for `MixLab`, but not for `mixengine`. Not verified: a write may
  leave the item's ACL trusting only the build that wrote it.
- No `mixengine` item's modification date changed, so MixLab only reads it.

## What reads a `mixengine` item at start-up

Exactly one function reads MixEngine's Keychain: `read_mixengine_entry` in `secrets.rs`, reached
through the `secrets_resolve_mixengine` command. It has two callers.

- `modules/mixengine/explore_data.rs`, from *Explore data* on a service. A person clicked that, so
  it is not start-up.
- **`loadSavedConnections()` in `src/modules/db/savedConnections.ts`, which resolves the
  `keyringRef` of every saved connection that has one** (`resolveKeyringRef`, line 132). It does
  this every time the list is read, and the list is read at start-up in two ways:
  1. **A restored Database tab.** `savedConnectionsStore.ts` loads the list the first time a
     `DbTab` mounts, and the tab strip restores the last session's tabs at launch. The list is
     loaded before anything is connected.
  2. **Sync**, when the `connections` or `connection-secrets` row is on. `startSync()` runs a full
     run at launch, and both collections' `read` (and their `write`) call
     `loadSavedConnections()`. Neither uses the password it resolves:
     `connectionToSync` strips every credential, and `connectionSecretsToSync` goes through
     `readSecrets(config, keyringRef)`, which leaves out a referenced password on purpose.

So one saved connection handed over by MixEngine (T83) is enough for every launch of a new build to
ask for MixEngine's item as well as for MixLab's. It also breaks ADR 0056: MixLab reads MixEngine's
credential store on start-up for nothing it does itself.

`import.rs` does not touch `mixengine`. It reads `MixDB` (`read_legacy`), and only on the one
launch that finds the standalone client's data and no marker. That launch can ask more than once,
and nothing can prevent it: the old items are separately guarded (T104). It is out of scope here.

## What writes `MixLab`/`vault` at start-up

Every write goes through `Keeper::flush`, called by `Keeper::save`, `Keeper::load` (moving a
pre-vault entry in, once per entry) and `Keeper::replace_own` (`write_own` / `forget_own`).
**`save` and `replace_own` flush whether or not the vault changed.** Callers at or near start-up:

1. **Sync's refresh-token rotation.** On a launch where any sync row is on, the first request goes
   through `SyncState::session`, which has no session in memory, so it refreshes. The server
   hands back a new refresh token and spends the old one, and `keep` → `write_own` writes the vault.
   This also repeats whenever the access token lapses during a run. The value really changed, so
   this write is correct and stays.
2. **Sync's `write` for `connections` and `connection-secrets`** (`src/modules/db/sync.ts`) calls
   `updateConnection` for every connection a page touched, and `persistEntry` saves its secrets
   even when they are the same ones. The terminal's `sync.ts` does the same through
   `updateSavedTarget`. These run when a pull brings anything.
3. **`updateConnection` for local-only fields**: pin, read-only, sidebar width, Redis scan limit.
   Each one rewrites the vault with secrets that did not change. They are not start-up, but they are
   the same unnecessary write.
4. `loadSavedConnections`' one-time move of credentials still in `connections.json`, and the
   import. Both happen once and are legitimate.

Which of 1 and 2 moved the timestamps on the test Mac depends on whether it was signed in to sync
with rows on. The code cannot say that; see *Open questions*. Either way the fix below covers it.

## D1. A saved connection's MixEngine password is resolved only to connect

`loadSavedConnections()` stops resolving `keyringRef`. An entry with a reference comes out of the
list with `keyringRef` set and no `config.password`, which is exactly what is on disk. Everything
that only lists, syncs, pins or resizes now never reaches `secrets_resolve_mixengine`.

The password is resolved where it is used:

- **Connecting** (`DbTab`'s `connect`, and any other path that dials with the form's config): when the form holds a
  `keyringRef` and its password box is empty, it calls `secrets_resolve_mixengine` first and
  connects with what comes back. `undefined` means MixEngine no longer has the entry, and the
  connect goes ahead with no password, which is today's behaviour for a stale reference.
- **The form** shows a referenced password as empty, with a new hint (en and vi) saying that the
  password comes from MixEngine when connecting. The form has no such hint today, because the box
  was always filled. It does not resolve the password to fill a masked box. Typing in the box clears
  `keyringRef`, as it does now (`DbTab.tsx:102`).

A side effect: `stableStringify({ name, config })` snapshots, which `savedChange` uses to detect
edits from another tab or from sync, now compare like with like. Today the list holds a resolved
password that the file does not.

**A restored tab that was connected.** `DbTab` reconnects a restored tab that was `connected` at
the last close (`openAndConnect`, line 525). For an entry with a `keyringRef`, that reconnect is a
second item on a launch nobody started a connection on. **Such a tab comes back as its form, not
connected**, and one click on Connect resolves and connects. Entries without a reference reconnect
as today, because their secrets are in the vault the launch reads anyway. (Agreed 2026-10-02.)

## D2. The vault is written only when it changes

`Keeper::save` and `Keeper::replace_own` compare the next vault with the cached one. When they are
equal they do not flush. `save` still calls `forget(id)`. That is a lookup of the connection's
pre-vault item, which is no write and raises no dialog when the item is not there. When a leftover
is there, removing it is the cleanup `save` exists to do: a connection deleted before anything read
it would otherwise leave its old password behind for good.

`Keeper::load`'s move-in and the non-vaulted path (Windows, Linux) are unchanged. Per-entry stores
never ask the question, and `environmentsStore.ts` already skips unchanged writes on its own side.

Not doing this on the frontend instead: `persistEntry` would need the stored secrets to compare
against, which is a read through the same command. The Keeper already holds the vault in memory,
so the comparison costs nothing there and covers every caller: db, terminal, REST, sync and import.

## D3. What stays a write on every launch

Sync's refresh rotation (writer 1) still writes once per launch for a signed-in machine with rows
on. It writes after the launch has read the vault, so it raises no prompt of its own. With every
row off, a signed-in machine writes nothing (D4). If the
unverified ACL hypothesis holds, the write narrows the item's trust to the current build. For a
person who only moves forward through releases that costs nothing, because every new build asks
once anyway. Going back to a build that was allowed earlier would ask again, which matches the
reinstall observation. Keeping the refresh token outside the Keychain to avoid this would be a
security change for a downgrade convenience, so it is not proposed.

## D4. The closing date at launch opens no session

Found on the Mac (see *Outcome*): with every sync row off, a signed-in machine still wrote the
vault once per launch. `Workspace.tsx` reads the server's closing date (D4b of the T177 spec) at
launch through `sync_closing_here`, and `SyncState::closing_on` got it by opening a session. Opening
one refreshes, and the refresh is writer 1 above, whatever the rows say.

`closing_on` no longer opens a session. A run that has one reads `limits.closing_on` from it; a run
that has none asks the saved server's `/v1/capabilities`, which needs no session, as the sign-in
form already does through `sync_server_closing`. The banner still shows at launch. A signed-in
machine with every row off makes that one unauthenticated request and writes nothing.

Test: `the_closing_date_at_launch_refreshes_nothing` in `sync/session.rs`, against a local server
that answers capabilities and a `Keeping` that counts what it keeps.

## D5. A service start waits for the person answering the Keychain

Also found on the Mac. Starting MixEngine's MariaDB from a build the `mixengine` item did not trust
yet raised the Keychain dialog, and the service went to `Failed` three seconds later while the
dialog was still on screen: `daemon.log` said `no answer within 3s`. The start path in
`crates/mixengine-daemon/src/services/runner.rs` bounded the read by `ENVIRONMENT`, which exists so
that a read that never returns cannot hold a stop or a daemon shutdown.

The start path now waits on the runner's cancellation beside the read, so a stop or a shutdown ends
the wait at once and leaves the service `Stopped`, and it is bounded by `START_ENVIRONMENT`, two
minutes, long enough for a person to answer. The read is still kept for the next attempt to join.
The stop path keeps `ENVIRONMENT` and its budget unchanged. This is MixEngine's daemon, not MixLab:
the window only shows the service as `Starting` for longer.

Test: `a_stop_ends_a_start_still_waiting_on_the_keyring` in `runner.rs`, and the existing
`a_start_that_gave_up_on_a_keyring_read_does_not_start_a_second_one`.

## MixLab

All of this is MixLab's own window, except D5, which is the daemon's start path and adds no screen. The `db` module's saved connection list and connect path
(D1), and `secrets.rs`'s Keeper (D2). No daemon method is added or changed, and MixEngine is not
involved at start-up any more, which is the point of D1 under ADR 0056. A person sees no new
screen. A launch of a new build asks once, and a restored MixEngine-backed tab waits for Connect.

## Tests

- `secrets.rs` (`MemoryStore`, which gains a write counter):
  - `saving_what_is_already_there_writes_nothing`: save `a`, then save `a` again with the same
    secrets. One write in total.
  - `writing_the_same_own_value_writes_nothing`: the same for `write_own`.
  - A changed value still writes. The existing tests stay green, including
    `sync_and_the_connections_are_one_visit_to_the_store`.
- A frontend test over a mocked `invoke` and store: `loadSavedConnections()`,
  `connectionsSyncable.read` and `connectionSecretsSyncable.read` never invoke
  `secrets_resolve_mixengine`, and an entry with a `keyringRef` comes back with no password.
- The connect path's helper: a `keyringRef` with an empty password resolves once and uses the
  result, a typed password is used as typed, and no `keyringRef` resolves nothing.
- The restore rule: a `connected` tab reconnects unless its entry has a `keyringRef`.
- `CHANGELOG.md` `### Fixed`: a new build asks for the Keychain once on macOS, not twice.

## Verification on a Mac

The unit tests prove the counts against a `MemoryStore`. Only macOS proves the dialogs. On a Mac,
with a saved connection that has a `keyringRef` and a restored Database tab, and once with sync
signed in and the `connections` row on:

1. Install a CI build of the branch over an allowed earlier build. Launch: exactly one prompt, for
   `MixLab`.
2. `security find-generic-password -s MixLab -a vault` before and after a second launch. The
   modification date is unchanged when sync is off, and moves once when sync is on (D3).
3. Connect the MixEngine-backed connection: the `mixengine` prompt appears then, not before.

## Open questions

1. Was the test Mac signed in to sync, and which rows were on? That tells which writer moved the
   timestamps. The fix covers both, but the Outcome should say which.
