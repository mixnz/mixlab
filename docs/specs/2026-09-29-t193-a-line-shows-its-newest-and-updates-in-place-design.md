---
status: implemented
date: 2026-09-29
task:
  - T193a
  - T193b
  - T193c
  - T193d
---

# T193 — A line shows its newest, and updates in place

Roadmap tasks T193a–T193d, in [phase 33](../roadmap/phase-33-a-line-shows-its-newest.md).
2026-09-29.

## The problem

Today every line has exactly one release in the index — PHP 7.3 is 7.3.33, 8.4 is 8.4.24, and the
same holds for Node.js, MariaDB and the rest — so the *available* list looks tidy. It will not stay
that way, for two reasons.

**The list grows with every patch.** `runtime.list_available` and `package.list_available`
(`crates/mixengine-daemon/src/runtimes.rs`, `packages.rs`) return one row per published version,
and `mix runtime available`, `mix package available` and MixLab's Packages screen draw every one of
them. Six PHP lines with a few patches each, plus Node.js, Python, Ruby, Go, Java, Composer and
every server, will soon be hundreds of rows. Most people choose a line (PHP 8.4, Node 22,
MariaDB 11.4), and only a few need an exact patch — but those few still have to be able to get it.

**A newer patch is not an update, only a different version.** Somebody on 8.4.24 who sees 8.4.25
can only install it beside the old one. Nothing moves to the new version:

- `sites.php_service_id` still names `php-fpm@8.4.24`;
- a `web-app` extension's pool keeps its `runtime_install_id`;
- the extension choices stay on 8.4.24's row, and so does the default;
- a `mariadb@main` instance keeps the `packages` row it was created from.

No method moves any of these, so the new patch is never used.

Version resolution is already correct: a constraint `8.4` picks the newest installed 8.4. What is
missing is the listing and the move.

## Decisions taken with the person who asked

1. **An update replaces.** The old version is removed once everything has moved off it, unless
   the person asks to keep it (`--keep`, and a *Keep 8.4.24* checkbox in MixLab), or unless
   something only the old version can answer still needs it (D5 step 9).
2. **Servers are in scope.** MariaDB, MySQL, PostgreSQL, Redis, Memcached, MongoDB, Caddy and Nginx
   update within a line in the same way. Their instances move to the new version.
3. **Pins MixEngine holds are rewritten; pins the person wrote are not.** A project pin in SQLite
   that names the old patch exactly is moved to the new one. A `mixengine.toml` is the person's file
   and is never edited. It is named in the plan, and the old version is kept for as long as that
   file needs it.

## Principle

**The daemon decides what a line is, what is newest in it, and what an update moves.** A client
filters rows on a flag, counts rows that share a line, and draws a button. It never compares
versions to find out whether something is newer: two clients doing that could disagree, and that is
business logic, which clients may not hold.

**Nothing updates unasked.** Updates are computed only inside the answer to a request for the
available list. Nothing checks in the background or announces anything on its own, and an update
runs only when a person asks for it. A headless install on a server behaves exactly as it does today.

**The old version is untouched until the new one is proven.** Every design choice below follows from
this: the new version is installed, smoke-tested and started before anything is pointed at it, and
the old version is removed only after everything has moved and answered.

## D1. A line, defined once in core

Add a function `mixengine_core::lines::line_of(name, version) -> String`, where `name` is a runtime
kind or a package name:

| name | line | example |
| --- | --- | --- |
| `php`, `python`, `ruby`, `go`, `composer` | major.minor | `8.4`, `3.12`, `3.3`, `1.23`, `2.2` |
| `node`, `java` | major | `22`, `21` |
| `postgres` | major | `17` |
| `mariadb`, `mysql`, `redis`, `memcached`, `mongodb`, `caddy`, `nginx` | major.minor | `11.4`, `8.4`, `7.4` |
| any other name | major.minor | — |

The pre-release suffix is not part of the line: `8.5.0RC1` is in line `8.5`.

Composer's major.minor is deliberate. 2.2 is the LTS line for projects on PHP older than 7.2.5
(`runtime-versions.md`), so it must not be merged into 2.x.

**A line never crosses a data series.** `adopt::instances::series_of` already says which part of a
version a data directory is tied to: major.minor for MariaDB and MySQL, major for PostgreSQL. A line
is always the series or narrower than it, so an update within a line can never hand data to a
server that cannot open it. It also means `.mixengine-ready`, which records the version that
bootstrapped the data, still names a version of the same series afterwards, and
`adopt::instances::opens` keeps answering correctly. A test in core proves this for every package
that has a series, which keeps the two functions from drifting apart.

**Newest in a line.** A line that has a stable release uses its highest stable release by
`cmp_precedence`. A line with only pre-releases (8.5.0RC1 before 8.5.0 ships) uses its highest
pre-release.

**Which version an installed one updates to.**

- A stable install is offered the line's newest stable release, and never a pre-release.
- A pre-release install is offered whichever is highest in its line, stable or pre-release, so
  8.5.0RC1 is offered 8.5.0RC2, and then 8.5.0 once it ships.

## D2. The catalogue carries the line and the updates

Additive changes in `mixengine-proto`, regenerated into `bindings/` (`packaging/bindings.sh`):

- `RuntimeRelease` and `PackageRelease` gain `line: String` and `newest_in_line: bool`.
- `RuntimeCatalogue` gains `updates: Vec<RuntimeUpdate>`, and `PackageCatalogue` gains
  `updates: Vec<PackageUpdate>`. An update is
  `{ kind | package, from: PackageVersion, to: PackageVersion, to_installed: bool, needs }`.
  - `needs` is `to`'s requirements, computed exactly as they are for a release. MixLab can then ask
    the same question before an update that it asks before an install (T151).
  - There is one entry for each installed version whose line has, by the rule in D1, a newer
    release in the index for this machine.
  - When two patches of one line are installed (8.4.23 and 8.4.24), each has its own entry.
  - When `to` is already installed, the entry is still there, with `to_installed` set. Nothing will
    be downloaded, but the sites have not moved yet.
  - An installed version whose line has vanished from the index has no entry.

The updates come with the available list, not the installed list, on purpose.
`runtime.list_installed` and `mix runtime list` never read the index today, and they stay that way:
listing what is on the disk must not cost a network request. A stale cached index still answers,
with `stale` set as it is now.

The filters (`RuntimeFilter`, `PackageFilter`) do not change: the daemon still returns the whole
catalogue, and choosing which rows to draw belongs to rendering.

## D3. Listing: one row per line

**`mix runtime available [--kind K] [--line L] [--all]`**

- By default it prints only the `newest_in_line` rows, one per line. A `more` column says how many
  other releases that line has.
- `--all` prints every row, as the command does today.
- `--line 8.4`, which requires `--kind`, prints every release of that line.
- When `updates` is not empty, lines above the table name each update together with the command
  that applies it, for example `php 8.4.24 → 8.4.25   mix runtime upgrade php 8.4.24`.

**`mix package available [--package P] [--line L] [--all]`** behaves the same way.

The `--line` and `--all` filtering is done on the fields the daemon sent, which is rendering, and
needs no new method.

**Installing by line.** `mix runtime install php 8.4` already means the newest release that
satisfies `8.4` (`newest_satisfying`). An exact patch is still `mix runtime install php 8.4.24`.
Nothing changes here. The spec only states it, because it is what makes a list with one row per line
enough.

## D4. Four new methods: a plan and a job for each kind

Following `daemon.uninstall_plan` and `extension.plan`, the question and the action are separate
methods, so each has one answer type:

```
runtime.upgrade_plan { kind, from, to? }                          -> RuntimeUpgradePlan
runtime.upgrade      { kind, from, to?, keep,
                       install_prerequisites, ignore_requirements } -> JobSummary
package.upgrade_plan { package, from, to? }                       -> PackageUpgradePlan
package.upgrade      { package, from, to?, keep,
                       install_prerequisites, ignore_requirements } -> JobSummary
```

- `to` absent means the version D1 says `from` updates to.
- Both methods are refused with `invalid_argument` in three cases, before a job exists:
  - `from` is not installed.
  - `to` is not newer than `from`.
  - `to` is in another line. **Moving between lines is a switch, not an update.** PHP 8.3 → 8.4
    changes which extensions exist, and MariaDB 11.4 → 11.8 needs `mariadb-upgrade`. Each is
    a decision for a person, and neither belongs in a button labelled *Update*.
- The two requirement flags mean exactly what they mean on `runtime.install` and `package.install`,
  and the check runs on `to`.
- The job kinds are `runtime.upgrade` and `package.upgrade`. `JobKind` is a validated string, so
  nothing is added to a list.
- The job's progress messages name the step it is on. Its result is the plan from D7, marking what
  was done and what was skipped.
- The job emits the events that already exist for what it changes: a runtime installed or removed,
  a service's state, a site re-rendered. No new event type is needed, and every open client
  refreshes as it does today.

## D5. What a runtime update moves

The job runs these steps in order. Each step records what it did.

1. **Install `to`** through the existing install pipeline, smoke test included, unless `to` is
   already installed. For PHP, the install hook creates `php-fpm@<to>`, as it does for every
   install.
2. **Carry the PHP extension choices.** `from`'s `extension_choices_json` is copied to `to`, keeping
   only the names `to`'s build actually ships. A name `to` lacks is listed in the result as
   dropped. `etc/php/<to>/conf.d` is regenerated.
3. **Carry the pool's settings.** `php-fpm@<to>` takes over `php-fpm@<from>`'s
   `config_overrides_json`, `limits_json`, `idle_minutes` and `autostart`. Its own port and
   activation port stay the ones its row was given.
4. **Bring the new pool to the old one's state:**
   - If `php-fpm@<from>` is running, `php-fpm@<to>` is started and waited on until ready. If it
     fails, the job stops here. Nothing has moved, both versions stay installed, and the result names
     the pool's log.
   - If the old pool was stopped by the daemon (idle, `stopped_by = daemon`), the new one is left
     stopped the same way, so on-demand activation wakes it on the next request.
   - If a person stopped it, the new one stays stopped as well.
5. **Decide which extension pools move.** A `web-app`'s pool (`php-fpm@phpmyadmin`) moves only if
   `to` satisfies its manifest's `[web-app.runtime].requires`. T81b froze a PHP into that row at
   install so that a daemon restart could not move an application silently. This move is one a
   person asked for, which that rule allows. A pool whose `requires` `to` does not satisfy stays on
   `from` and is listed in the result.
6. **Move everything that pointed at `from`, in one SQLite transaction:**
   - `sites.php_service_id` changes from `php-fpm@<from>` to `php-fpm@<to>`.
   - Each extension pool chosen in step 5 changes its `runtime_install_id` to `to`'s row. Its id
     does not change, because it is named after the extension rather than the version.
   - If `from` was the default, `to` becomes the default.
   - Every project pin in SQLite that matches `from` but not `to` is rewritten to `to`'s full
     version. A pin such as `8.4` or `^8.4` already matches `to` and is left alone.
7. **Render the sites and reload the front end**, then restart every moved extension pool that was
   running, so it runs the new binary. Site rendering is judged as a whole, and a refusal writes
   nothing (T43, D3). If the front end refuses the new set, the step-6 transaction is reversed, the
   new pool is stopped, and the job ends with both versions installed and the refusal in its result.
8. **Stop `php-fpm@<from>`.**
9. **Remove `from`** through the same code path as `runtime.uninstall`, unless `keep` is set. It is
   kept even without `keep` when something still needs it:
   - `projects::pins_broken_by(kind, from)` is not empty. That check already reads every registered
     project's `mixengine.toml` in effective order, and it counts only a pin that `from` answers and
     no remaining version does.
   - a discovered tool (`npm install -g yarn`) lives only in `from`'s bindir;
   - an extension pool stayed on `from` in step 5.

   The result names each reason — the file, the tool or the extension — together with the command to
   run once it has been dealt with (`mix runtime uninstall php 8.4.24`). A removal that fails does
   not fail the update, and the result reports the version as kept. On Windows the usual cause is a
   program a shim started earlier (`php -S` in a terminal) that still holds the old files open.

Runtimes other than PHP (Node.js, Python, Ruby, Go, Java, Composer) have no pool. For them steps
2–5, 7 and 8 do nothing, and the update is: install, move the default and the pins, then remove.

What the daemon cannot see is out of reach:

- a `mixengine.toml` in a directory that is not a registered project;
- a `MIXENGINE_PHP=8.4.24` exported in somebody's shell.

After a removal, both fail the way a missing version fails today: the shim's `dependency_missing`
names the exact `mix runtime install` command.

**Tools installed with a package manager are not carried over.** `npm install -g` writes into the
old install, and replaying it would run arbitrary package scripts on the person's behalf. The plan
lists these tools, together with the command that reinstalls each one under `to`. Step 9 keeps
`from` while any of them exist there alone.

**The alternative not taken: naming the pool after its line** (`php-fpm@8.4`). An update would then
change nothing about which pool a site points at. It is rejected because it needs a new ADR and a
data migration for every existing pool, and because it forbids what `process-supervision.md` allows
today: two patches of one line installed side by side, each with its own pool. Moving the sites
costs one transaction and keeps that ability.

## D6. What a package update moves

1. **Install `to`**, which is always in `from`'s data series (D1), unless it is already installed.
2. **Ask for the Linux port grant first, when a front end is involved.** When one of `from`'s
   instances is the active Caddy or Nginx, the step follows `api/front_end.rs`'s rule. On Linux the
   port-80 grant lives in the binary's `security.capability`, so `to`'s binary has none. The grant
   is probed and asked for (`PrivilegedOp::PortAccessGrant`) **before anything is stopped**.
   - The rule is *do not make it worse*: if `from` has a grant and `to` cannot get one, the job
     ends here with the same `NotGranted` answer a front-end switch gives, and nothing has moved.
   - Windows grants nothing and needs nothing. macOS redirects by packet filter, which does not
     depend on the binary.
3. **Move each of `from`'s instances, one at a time** (`mariadb@main`, then `mariadb@legacy`):
   1. note its state and `stopped_by`;
   2. stop it if it is running;
   3. point `package_id` at `to`'s row;
   4. render `etc/<id>/` again;
   5. start it if it was running, and wait for it to become ready.

   Nothing else changes: the data directory, the port, the credentials in the keyring (addressed by
   the service id, which does not change) and the overrides stay as they were.
4. **An instance that fails to start on `to` is moved back.** Its `package_id` is pointed at `from`
   again, its configuration is rendered again and it is started. The job carries on with the next
   instance, and the result names the one that failed and the lines its log printed. If `from` also
   refuses to start it, the instance is left stopped on `from`, both versions stay installed, and the
   result says so plainly. The next item is the one known way this can happen.
5. **MySQL 8.0 cannot go back.** Within 8.0 a newer patch upgrades the data dictionary on its first
   start, and an older 8.0 patch cannot open the result. From 8.4 LTS on, MySQL supports downgrade
   between patches of one LTS. The plan for a `mysql` 8.0 instance says, before anyone agrees, that
   a failed start cannot be undone by moving back. MariaDB treats `mariadb-upgrade` within a series
   as advisory, and MixEngine does not run it — the position `adopt::instances::opens` already takes.
6. **A front-end update is a restart of the front end.** Every site is unreachable for the length of
   one restart, and the plan says so.
7. **Refresh `bin/`**, so client commands such as `mysqldump` and `psql` come from `to`.
8. **Remove `from`**, unless `keep` is set, or unless an instance is still on it after step 4.

## D7. The plan

`*.upgrade_plan` answers what a person should see before they agree:

- `from` → `to`, whether `to` is already installed, and the download size;
- `to`'s `needs`, as in D2;
- the sites and extension pools that move, those that stay (with the `requires` that keeps each),
  and which are running;
- extension choices that `to` does not ship;
- project pins that will be rewritten;
- what will keep `from` installed: `mixengine.toml` files, tools that live only in `from`, and
  extension pools that stay;
- for a package: each instance and whether it will be restarted, the front-end restart, whether a
  port grant will be asked for, and the MySQL 8.0 warning;
- whether `from` will be removed or kept, and why.

The job's result is the same structure, with each entry marked as done, skipped or failed.

The plan reads only this home's tables, the index and registered projects' manifests, and changes
nothing. Asking for it twice gives the same answer twice.

**`mix runtime upgrade <kind> <from> [--to V] [--keep] [--yes] [--dry-run]`**, and the same shape
for `mix package upgrade <package> <from>`:

- the command asks for the plan and prints it;
- `--dry-run` stops there;
- otherwise it asks `y/N`, unless `--yes` is given, then starts the job and follows it like
  `mix runtime install` does.

## Error handling

- An index that cannot be obtained at all fails both methods with the same error `*.install` gives.
  A cached index still answers, and the plan says it is stale.
- An upgrade already running for the same `(kind, from)` is answered with its job, following the
  rule the installs use.
- These are refused with `conflict` while an upgrade job runs:
  - an upgrade whose `from` or `to` is being installed, uninstalled or upgraded by another job;
  - an uninstall of that `from` or `to`.
- A person who stops or starts a service while it is being moved is obeyed. The step that finds the
  service in an unexpected state records it and does not fight it.
- A daemon that stops mid-job leaves one of these states, and each can be recovered:
  - **before step 6 (D5) or 3 (D6):** `to` is installed and nothing points at it. A new pool left
    running is idle-stopped like any other.
  - **after it:** everything points at `to` and `from` is still installed.
  - **after the last step:** done.

  Running the same upgrade again continues from where it stopped, because every step is skipped when
  its effect is already there. Once `from` is gone, running it again is refused as *not installed*,
  which is what finished looks like.

## MixLab

The **Packages** screen, `Languages.tsx` and `PackageList.tsx`, under
`apps/desktop/src/modules/mixengine/screens/Packages/`.

- **Available**: one row per line (`newest_in_line`), showing the newest version, its channel and
  EOL. A *N more* toggle on a row expands the other releases of that line, and each has its own
  *Install*. The existing search matches every release. A search that matches only an older patch
  expands its line, so `8.4.2` finds 8.4.24 even when 8.4.25 is the row shown.
- **Installed**: a version that has an entry in `updates` shows *8.4.25 available* and an *Update*
  button next to it.
  1. Clicking it asks for `*.upgrade_plan` and opens a dialog. The dialog shows the plan (D7) and a
     *Keep 8.4.24* checkbox, which is ticked and disabled, with its reason, when the plan already
     says `from` will be kept.
  2. When `needs` is not empty, the same requirement question an install asks comes first (T151).
  3. Confirming starts the job, whose progress appears on the row, just as *Install* does today.
- The screen already asks for the available list when it opens, so updates appear without any new
  request. Nothing appears in the tray, on the Dashboard or in a notification. An update is found by
  the person looking at the screen, which follows the rule that nothing updates unasked.
- The four methods are reached through four Tauri commands in `modules/mixengine/commands.rs` and
  their wrappers in `api.ts`, following `adding-a-command.md`. Every string goes into `en.ts` and
  `vi.ts`.
- MixLab without MixEngine is unaffected. Everything here lives in the `mixengine` module.

`docs/features/client-surface.md` gains the four methods, and
`scripts/check-client-surface.mjs` passes with no new exception.

## Testing

- **Core:**
  - a `line_of` table test covering every kind and package, including a pre-release;
  - the invariant that a line is never wider than its series;
  - newest in a line, and the version an installed one updates to, for stable and pre-release
    installs;
  - pin rewriting: an exact pin moves, while `8.4` and `^8.4` stay.
- **Daemon:**
  - `list_available` marks `newest_in_line` and computes `updates`, including `to_installed` and
    two patches of one line, from a fixture index and a fixture store;
  - both methods refuse the three `invalid_argument` cases;
  - `*.upgrade_plan` writes nothing;
  - a step-7 refusal reverses step 6;
  - an interrupted job resumes when asked again.
- **Integration**, following `tests-that-say-why`:
  - PHP 8.4.24 → 8.4.25 with one site. Afterwards `phpinfo()` through the site reports 8.4.25, the
    site's extension choices survive, and 8.4.24 is gone.
  - The same with `keep`: both versions remain installed.
  - The same with a registered project whose `mixengine.toml` pins `8.4.24`: 8.4.24 is kept and the
    result names the file.
  - A `web-app` whose `requires` 8.4.25 does not satisfy stays on 8.4.24, and 8.4.24 is kept.
  - MariaDB 11.4.x → 11.4.y with a database in it: the database is readable afterwards.
  - A package update whose new build cannot start: the instance is back on the old version and
    running.
  - On Linux, a Caddy update in a home with a grant asks for the grant before stopping Caddy. When
    the grant is refused, Caddy is still running on the old version.
- **CLI:** snapshot tests of the grouped table, `--all`, `--line`, the updates lines, and a printed
  plan.
- **MixLab:** vitest for grouping, and for expanding a line when a search matches an older patch.

## Not in this design

- **Moving between lines** (PHP 8.3 → 8.4, MariaDB 11.4 → 11.8). That is a switch with its own
  decisions: which extensions exist, and when `mariadb-upgrade` or `pg_upgrade` runs.
- **Updating several versions with one click.** Each update is its own job with its own plan.
- **Backing up a database before its server is updated.** A patch update within a series does not
  change the data format, apart from the MySQL 8.0 case the plan warns about.
- **What the index keeps.** `mixengine-packages` decides how many old patches remain published.
  Nothing here depends on it, because an exact patch the index no longer offers is still installed
  and still works.
- **Replaying `npm install -g` and similar** under the new version (D5).
- **Telling anyone that an update exists** outside the Packages screen and `mix … available`.

## Tasks

- **T193a** Core `line_of` with its tests; `line`, `newest_in_line` and `updates` in the proto and
  bindings; the daemon's two `list_available`; the grouped `mix runtime available` and
  `mix package available`; MixLab's grouped *Available* list and the *N available* label on
  installed rows.
- **T193b** `runtime.upgrade_plan` and `runtime.upgrade`, covering every step in D5;
  `mix runtime upgrade`; MixLab's *Update* dialog on runtime rows.
- **T193c** `package.upgrade_plan` and `package.upgrade`, covering every step in D6, the Linux port
  grant included; `mix package upgrade`; the same dialog on package rows. **(P)**
- **T193d** `runtime-versions.md`, `services.md`, `client-surface.md`, the lines in
  `CHANGELOG.md`, and this spec flipped to `implemented`.
