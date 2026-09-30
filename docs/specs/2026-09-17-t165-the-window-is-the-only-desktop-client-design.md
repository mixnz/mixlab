---
status: implemented
date: 2026-09-17
task: T165
---

# T165 — The window is the only desktop client

Roadmap task [T165](../roadmap/phase-12-one-product.md), on
[T107](2026-09-09-t107-where-the-daemon-and-the-window-are-design.md) and
[T84](2026-09-04-t84-the-window-in-the-registry-and-one-keyring-design.md). 2026-09-17.

`desktop-app` was an extension kind with exactly one entry ever published: the standalone client. On 2026-09-17
that entry was withdrawn from `mixnz/mixengine-packages` (`20076b7`, registry re-cut the same day),
because the standalone client *is* MixLab now — the window every installer ships, which `mix database open` has
reached without any extension since T107. What is left in this workspace is a general mechanism
nothing uses, a precedence rule written for one product (`scheme == <old>`), and one sentence that
is now wrong: `mix database open` on an install with no window still says
``mix extension install <old>` adds the standalone client`, naming an entry the registry no longer has.

This task removes the kind, and with it every path by which a database opens in anything but this
install's own window.

## What is already true

- **The window is found without an extension.** `Databases::locate_client` asks
  `DesktopApps::locate_window` first (T107); an extension is only consulted when there is none, or
  when its scheme is `<old>`.
- **The registry tolerates an entry it cannot read.** `Registry::listing` counts it in
  `unreadable` and lists the rest, so a cached `extensions.json` from before today — which still
  holds `<old>` — does not break `extension.list_available` once the kind is gone.
- **`<old>://` is the window's scheme and stays** (T107's `window::SCHEME`, the merge design's D10).
  Nothing here renames it, `open_in_<old>.rs`, or the keyring convention of T84's D5/D6, which the
  window's `db` module reads.

## D1 — Scope: the kind goes, whole

Removed, in every layer:

| Layer | What goes |
| --- | --- |
| manifest (`mixengine-core`) | `Body::DesktopApp`, `DesktopApp`, `DetectHints`, the `[desktop-app]` table |
| proto | `ExtensionKind::DesktopApp`, `DesktopPresence`, `DesktopAppSummary`, `ExtensionInspection.opens`, `ExtensionPlan.client`, `DesktopClient::NotInstalled`, `DesktopClient::Installed.extension` |
| daemon | `Databases::desktop_extension`, the extension half of `locate_client`, `extensions::presence` |
| platform | `DesktopApps::locate` and the three lookups behind it — App Paths and the uninstall table, Spotlight, XDG desktop entries — plus `InstalledApp.args`, which only a desktop entry's `Exec=` ever filled |
| CLI | the `not_installed` rendering, the plan's *found / not on this machine* lines, the install warning |
| window | `openChoices`' `external`, its Dashboard branch and string, the plan dialog's presence line |
| testkit | `fixtures/extensions/<old>.toml` and `extension::<OLD>` |

Kept: `DesktopApps::locate_window`, `DesktopApps::launch`, `Located`, `Started`, `Launch`,
`DesktopClient::{Installed, NoClient}`, `database.client` and `database.open` themselves, and
`ArtifactAvailability::NotRequired`, which a `recipe` still is.

## D2 — `database.client` and `database.open` answer the window or nothing

```
locate_window found it  -> installed { name: "MixLab", program }
otherwise               -> no_client
```

The handoff URL's scheme is `window::SCHEME`, always; `Client.scheme` stops being optional and
`handoff.rs`' doc stops saying *out of its manifest*. Nothing else about the handoff changes: the
password still goes into the started window's environment and nowhere else.

`no_client`'s CLI sentence becomes what is true on a headless install:

```
  this install has no MixLab window to open a database in
  MixLab comes with MixEngine's installers; the headless archive has none
```

## D3 — The wire: nothing is bumped, and why that does not contradict ADR 0019

ADR 0019 bumps `PROTOCOL_VERSION` for "a member removed". Every member removed here is one **only
a daemon writes**, and a daemon built from this task never writes any of them — so an *older*
client reading a *newer* daemon sees exactly the shapes it already handles: `extension` and
`client` were optional and skipped when absent, `opens` was `null` for every kind but this one, and
`not_installed` and `desktop-app` simply never arrive.

The other direction is the one that can fail: a **newer** client reading an **older** daemon that
still holds a `desktop-app` row, or a registry cache listing one. That skew lives between a binary
replacement and the daemon's restart, and `updates::apply` restarts the daemon in the same step.
Bumping the protocol would turn that brief window into one where `mix daemon stop` — the way out —
is refused by the handshake, which is ADR 0019's own reason for not bumping. ADR 0038 records this
as a decision rather than letting it read as an oversight.

## D4 — A home that installed a `desktop-app` loses the row on upgrade

Migration `0024_no_desktop_app_extensions.sql`:

```sql
DELETE FROM extensions WHERE kind = 'desktop-app';
```

- **Before anything reads the table**: `Store::open` migrates first, so no `manifest_json` naming
  the kind ever reaches the new reader.
- **`extension_ports` needs nothing** — `ON DELETE CASCADE`, and a `desktop-app` declares no port.
  `services.extension_id` is `RESTRICT`, and a `desktop-app` never had a service row.
- **The `CHECK (kind IN (…))` keeps `'desktop-app'`.** Dropping it means rebuilding `extensions`
  under `-- no-transaction`, which is 0016's whole ceremony for a constraint that now only admits a
  value nothing writes.
- **The two directories an install created are left on disk.** Both were created empty — a
  `desktop-app` downloads nothing and runs nothing that writes — and deleting paths a row named
  after the row is gone is a file operation a migration cannot do.

`crates/mixengine-core/tests/upgrade.rs` asserts every row survives an upgrade, and
`schema-0017.sql` holds a `<old>` row. The frozen fixture is not edited; `EMPTIED` gains a sibling,
`REMOVED: &[(i64, &str, &str)]` — version, table, the `WHERE` it deletes — with a test proving that
exactly those rows are gone and every other row of the table survives, the way
`the_tables_two_migrations_empty_really_are_emptied` proves `EMPTIED`.

## D5 — A manifest that still says `desktop-app` is refused by the reader

`mix extension install --path` on such a manifest fails in `manifest::read` like any unknown kind,
naming the three that exist. No dedicated sentence: the kind was never in anyone's hands but
The standalone client's, and a special case for it would be the rule this task removes, kept as an error message.

## D6 — The one real handoff test keeps a real client

`crates/mixengine-cli/tests/mariadb.rs`'s
`the_root_credential_reaches_the_client_through_its_environment_and_not_the_url` is the only test
where a real credential reaches a real process, and it reached one through a `desktop-app`
extension naming a fake XDG entry. It moves to the window's path: the test copies `mixengined` into
a directory of its own, writes the fake client beside it as `mixlab`, and starts that copy — so
`locate_window`'s *beside the running program* finds it, which is the lookup every Linux install
uses. The harness gains the one helper that starts a daemon from a given binary. Still Linux only,
still `#[ignore]` behind the `mariadb` CI step, and every assertion on the URL and the environment
is unchanged.

## D7 — The window stops offering another application

`openChoices` returns `["builtIn"]` or `["builtInAfterEnabling"]` for an installed client and `[]`
otherwise — the only application the daemon can name is this one, so *external* would start a
second MixLab to forward a URL back to the first, which its own doc comment already forbids.
`api.databaseOpen` and the `mixengine_database_open` command go with it if nothing else calls them.
`PlanDialog` loses the presence line; `openDatabaseExternal`, `plan.clientInstalled` and
`plan.clientNotInstalled` leave both dictionaries.

## D8 — Documents

- **ADR 0038** — *The window is the only desktop database client, and `desktop-app` is not an
  extension kind*: D1–D3, and the alternative of keeping a general mechanism with no entry.
- **`docs/features/extensions.md`** — the kinds table loses its row; "the standalone client integration
  (`desktop-app`)" becomes "Opening a database in MixLab", keeping the handoff contract and the
  keyring convention, which are the window's now.
- `client-surface.md`, `services.md`, `architecture/platform-abstraction.md`,
  `architecture/daemon-and-ipc.md`, `docs/guide/{en,vi}/extensions.md`, `docs/guide/en/cli.md` —
  the sentences that name the kind or the standalone client-as-extension.
- **`CHANGELOG.md`** — the unreleased *Changed* line promising "another installed database
  application where MixEngine found one" loses that clause (unreleased, so no `Fixed`), and one
  *Changed* line: desktop-app extensions are gone, `mix database open` opens MixLab, an install
  that added the standalone client as an extension has it removed on upgrade.
- **Roadmap** — T165 in phase 12 after T111, and `todo.md`'s row.
- `bindings/` regenerated by `packaging/bindings.sh`; `.sqlx` untouched, since the migration is
  plain SQL and no `query!` changes.

**Not edited:** past specs, `schema-0017.sql`, `window::SCHEME`, `open_in_<old>.rs` and
`databaseOpenInthe standalone client` — names that say `<old>` about the window's own scheme, not about the kind.

## Testing

- **`mixengine-core`** — the manifest reader refuses `kind = "desktop-app"`; migration 24 removes
  exactly the `desktop-app` rows (`upgrade.rs`, D4); `extension_publish.rs` and `manifest.rs` lose
  their the standalone client cases.
- **`mixengine-daemon`** — on the mock: window present is `installed` with no extension and a
  `<old>://` URL; no window is `no_client` and starts nothing; the existing Redis, MongoDB and
  credential-in-the-environment tests run against the window instead of `a_<old>`.
- **`mixengine-platform`** — the per-OS `locate` tests go; `locate_window` and `launch` tests stay.
- **`mixengine-cli`** — `database.rs` keeps *no client* with the new sentence and loses the
  `nowhere` fixture; `render.rs` loses the two plan tests; `mariadb.rs` per D6.
- **`mixengine-proto`** — the `DesktopClient` encoding tests lose their `not_installed` and
  extension cases; `bindings` job green after regeneration.
- **Window** — `openChoices.test.ts` loses `external`; `npm run build`, `npm test`, `npm run lint`.
- **Cross-OS** — the Windows and macOS lookups are deleted code, so clippy for all three targets is
  what proves nothing else reached them: `cargo clippy` here, the macOS cross-check, WSL for Linux.

## Risks

- **The copied daemon in D6 may need something beside it.** If installing a package reaches for a
  sibling binary (the shim, for `bin`), the test copies that too; the plan finds out before it is
  written.
- **Somebody's hand-installed `desktop-app` from `--path`** is removed by D4 exactly as the standalone client's is.
  Nothing else ever used the kind, so this is the same person.
