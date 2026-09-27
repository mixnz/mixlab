---
status: approved
date: 2026-09-27
task:
  - T182f
  - T182g
  - T182h
---

# T182f–h — A reinstall finds what the last one kept

Roadmap tasks T182f, T182g and T182h, in [phase 9](../roadmap/phase-9-ship.md), after T182e.
2026-09-27.

## The problem

Reported from a real Windows machine:

1. MixLab was set up with `runtimes/`, `packages/`, `data/` and `logs/` moved to
   `C:\Users\…\mixlab_data` (the storage picker's *one folder for all four*). PHP, Node.js and more
   were installed.
2. MixLab was uninstalled, **without** ticking *remove the folders moved out of the home*. The
   uninstaller removed the home, and `mixengine.db` with it, and kept `mixlab_data` as asked (T182,
   `keep_relocated`).
3. MixLab was installed again and pointed at the same folders.
4. The MixEngine tab shows nothing installed. *Install* on Node.js 24.19.0 fails with
   `MixEngine refused: C:\…\mixlab_data/runtimes\node\24.19.0 is already installed`.

**Everything MixEngine knows is in SQLite, and SQLite lives in the home.** A kept folder is a folder
of files nobody knows about. An install refuses a directory that exists (`install.rs`, "an installed
version is never overwritten"), and nothing records one it did not unpack itself. The same is true of
`packages/` (MariaDB, Redis, Caddy) and, worse, of `data/`, which holds the person's databases.
Projects, sites, domains and ports exist only as rows and cannot be found on disk at all.

Keeping the folders was an explicit choice in the uninstaller, so the next install has to be able to
use them.

## Decisions taken with the person who reported it

- Runtimes and packages found on disk are **recorded automatically and silently**. Recording them
  starts nothing and changes no file.
- Service instances found in `data/` are **listed and adopted on request**, because adopting one
  allocates a port, resets a password and starts touching somebody's database.
- An adopted database gets a **new generated admin password**, set through the T127 repair path. The
  old one was removed from the credential store by the uninstall (T182d) and nobody knows it.
- Projects, sites and domains come back through a **copy of the state** the uninstaller leaves in the
  kept folder. That helps only uninstalls made by a build carrying this change, so the disk scan is
  what rescues the machine above.

## D1 — Every install leaves a marker (T182f)

Every runtime and package install writes `.mixengine-install.json` into its own directory,
**inside the staging directory before the rename**, so a directory that exists always has its marker:

```json
{
  "schema": 1,
  "what": "runtime",
  "kind": "node",
  "version": "24.19.0",
  "channel": "stable",
  "url": "https://…",
  "sha256": "…",
  "bytes": 31457280,
  "provides": { "node": "node.exe", "npm": "npm.cmd" },
  "extension_dir": null,
  "extensions": { "shared": [], "enabled": [], "compiled_in": [] },
  "installed_at": "2026-09-27T03:00:00Z"
}
```

It is `runtimes::Installation` (or the package row) as it was recorded, and nothing a person
changes later: extension choices, the default and pins stay in SQLite, and come back through D5.
`provides_json` already has a stable encoding, and the marker reuses it.

## D2 — An install that finds its directory adopts it (T182f)

In `Installer::install`, when `into` exists:

- **A marker that parses and names this kind and version, whose `sha256` is the one the index offers
  for this host** → the rows are written from the marker, the job ends `Adopted` rather than
  `Installed`, and nothing is downloaded. `mix` and MixLab say *found on disk, recorded*.
- Otherwise the install is refused as today, with a hint that names the directory and the two ways
  on: remove it, or run `mix runtime adopt <kind> <version>` (D3's rule, by hand).

## D3 — The daemon records what it finds on disk (T182f)

At every start, and after `storage` changes a `[paths]` key, the daemon walks
`runtimes/<kind>/<version>/` and `packages/<name>/<version>/` for directories without a row:

- **With a marker:** recorded from it, as D2.
- **Without one** (every install made before this change, including the machine above): recorded
  when the index for this host lists that kind and version, **every path in its `provides` exists
  under the directory**, and the kind's smoke test runs and prints the version. The row takes the
  index's facts (url, sha256, provides, extension_dir), and the extension listing is probed the way an
  install probes it. The marker is written at the same time, so the next start does not ask again.
- A directory that passes neither is left alone and listed by `mix doctor` as *on disk but not
  recorded*, with the reason. An index that cannot be read (offline) leaves the walk for the next
  start. Nothing is ever deleted.
- **A kind with no default after this gets its newest recorded version as the default**, the rule a
  first install already follows. `bin/` is refreshed once at the end (T185b).

The walk is `mixengine_core::adopt`, called from the daemon. The same rule by hand is
`runtime.adopt` / `package.adopt` and `mix runtime adopt` / `mix package adopt`, so the CLI has it
too (`client-surface.md`).

## D4 — Service instances are found, then adopted on request (T182g)

The generator's layout is `data/<package>/<instance>/`. A directory there with no `services` row is
a **found instance**:

- `service.found` lists them, with the service id they would become, the directory, the version
  that bootstrapped them, and the installed version that can open them or why none can. **No size**:
  walking a database's files on every listing costs more than the number is worth. `mix service found` prints the same list, `mix doctor` counts
  them, and MixLab's Dashboard shows *Found N services from an earlier install* with a *Review* link
  to a list with one **Adopt** button per row.
- **Which version can open it** is read from the data, never guessed — from `.mixengine-ready`, which
  every first run MixEngine finished writes with the version that ran it, and which `first_run`
  already trusts. MariaDB and MySQL need an installed version of the **same major.minor**: a newer
  series needs `mariadb-upgrade`, which MixEngine does not run yet, and an older server opening newer
  data corrupts it. PostgreSQL needs the **same major**. Anything else opens with any installed
  version. The newest that fits is chosen. A directory with no marker never finished its first run
  and cannot be adopted; a row that cannot be opened says which version to install.
  (Amended while building T182g: this replaces reading `PG_VERSION` / `mariadb_upgrade_info`.)
- `service.adopt {service}` (and `mix service adopt mariadb@main`) writes the `services`
  row with the existing `data_dir`, allocates a port through the normal allocator (the old port may be
  taken by now), generates a new admin secret, and runs the recipe's `reset_steps` against the
  existing data, which is `service.reset_credential`'s path (T127). MariaDB, MySQL and PostgreSQL all
  have one. The reset starts back what it repaired, so the adopt stops it again: the instance is left
  **stopped**.
- The databases and accounts inside are untouched. The apps that used them keep their own passwords.

## D5 — The uninstaller leaves a copy of the state in what it keeps (T182h)

When an uninstall keeps at least one relocated directory, it writes
`<kept directory>/.mixengine-state.db` into **each** kept one, through the store's existing
`VACUUM INTO` backup, before the home is armed for removal. The copy leaves out what belongs to the
old home and not to the things kept: `jobs`, `events`, `metrics_minutes`, `pending_privileged_ops`,
`ca` and `certificates`. It records the migration version it was taken at.

The uninstall's residue row for a kept directory says: *kept, with a copy of your projects and sites
so the next install can bring them back*.

## D6 — A fresh home offers to restore it (T182h)

On a start whose database has just been created, the daemon looks in the configured `runtimes`,
`packages` and `data` directories for `.mixengine-state.db`. It restores nothing on its own:

- `home.previous` answers what the newest copy holds: counts of projects, sites, services and
  runtimes, and when it was taken. The storage report carries the same answer, so MixLab's first-run
  picker can show *Restore from your earlier install: 3 projects, 5 sites, 2 databases* right after a
  folder is picked. `mix home restore` does the same from the CLI.
- **Restore** runs in one transaction on the new database, after migrating the copy in a temporary
  file when it is older than this build. A copy newer than this build is refused by name.
  - `runtime_installs` and `packages` rows come back when their directory still exists. Extension
    choices and defaults come back with them.
  - `services`, `projects`, `blueprints`, `sites`, `site_domains`, `site_service_links` and
    `site_routes` come back when what they depend on came back.
  - Paths are kept as written. Everything under the old home (`etc/`, `certs/`, `logs/` when it was
    not moved) is regenerated, which is already the rule for generated config.
- After the transaction: every restored database service goes through D4's credential reset;
  domains go into the hosts file in one elevation batch, the existing hosts sync; HTTPS sites get
  certificates from the new home's authority through the existing issue path; `bin/` is refreshed.
- A restored copy is renamed `.mixengine-state.restored.db` and removed at the next uninstall that
  writes a new one. D3 and D4 still run afterwards and pick up anything the copy did not know about.

## D7 — What does not change

- An install still never overwrites a directory. Adopting is recording, never unpacking over.
- Nothing is deleted by a walk, a restore or an adopt.
- A home with nothing relocated behaves exactly as today: the uninstaller removes it whole, and there
  is nothing to find.
- `mix`, `mixengined` and MixLab all reach the same methods. The window adds no capability of its own.

## Tests

- T182f: an install writes its marker before the rename; an install over a directory with a matching
  marker records it and downloads nothing; one with a mismatched sha is refused with the new hint; the
  start walk records a marked directory, records an unmarked one that passes the provides and smoke
  checks, and leaves one that fails with a `mix doctor` row; a kind with no default gets its newest.
- T182g: `service.found` lists an orphan data directory with the right openability for each of the
  three rules; `service.adopt` writes the row, a free port and a new secret, and the reset steps run
  against a fixture data directory; adopting one nothing installed can open is refused naming the
  version.
- T182h: an uninstall with `keep_relocated` writes the copy without the excluded tables; a fresh home
  finds it; restore brings back a project, its site, domains and service, skips a runtime whose
  directory is gone, and refuses a copy from a newer build.
- By hand on Windows, the machine above: start the reinstalled build and see Node.js 24.19.0 and the
  PHP versions recorded, with *Install* no longer offered.
