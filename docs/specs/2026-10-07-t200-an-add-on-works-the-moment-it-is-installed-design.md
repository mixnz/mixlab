---
status: implemented
date: 2026-10-07
task: T200
---

# T200 — An add-on works the moment it is installed

Roadmap task T200, in a new phase 38. 2026-10-07.

## The problem

A person installed Adminer from MixLab's Add-ons screen and called the screen useless. Each step
they took failed in a different way:

1. **Installing showed no progress.** The plan dialog closed at once and the list stayed the same.
2. **Adminer appeared only after they left the MixEngine tab and came back.** It then showed up
   twice: under *Installed*, and under *Registry* with an *Installed* pill.
3. **Opening its site never worked.** `https://adminer.mixengine.test` showed MixEngine's
   *starting* page, which reloads three times and then says *"Open MixLab to see why"*. Nothing
   in MixLab says why.

### Steps 1 and 2: the window stops listening too early

`extension.install` is a job: it answers a `JobSummary` as soon as the job is accepted.
`PlanDialog` treats that answer as the end of the install. It closes, and the screen re-reads both
lists before the job has written anything. Nothing follows the job afterwards, so a failed install
is never reported at all.

### Step 3: the daemon never makes the pool reachable

Measured on the machine where it happened (home `%LOCALAPPDATA%\MixEngine`, Windows):

| | `php-fpm@8.4.26` (a runtime's pool) | `php-fpm@adminer` (the extension's pool) |
| --- | --- | --- |
| `services.port` | 9001 | 9000 |
| `services.activation_port` | 9010 | **NULL** |
| site file upstreams | `php_fastcgi 127.0.0.1:9001 127.0.0.1:9010` | `php_fastcgi 127.0.0.1:9000` |
| started after install | by its first request | **never**, no log line names it |

The daemon log shows `job=24 to=succeeded` at 02:51:42 and the first DNS query for the site at
02:52:03. After that the daemon never logs `php-fpm@adminer` at all. With no activator, no request
can wake the pool, so Caddy answers 502 and the site serves the starting page.

`runtime.install` had exactly this hole, and T72a closed it there
(`crates/mixengine-daemon/src/runtimes.rs`, the block after the pool hook). A pool created outside
boot needs two idempotent repairs:

- `services::activation::ensure` gives the pool its activation port;
- `services::activate::hold_all` binds the address the site file names.

`extension.install` creates a pool (T82a, `extensions::pools::create`) and runs neither repair. So
the activator arrives only at the next daemon start, and until then a `web-app` is unreachable.

### Behind step 3: an activator is never given back

`hold_all` remembers what it holds in `HOLDING`, a set of `ServiceId`s. Two problems follow:

- **Nothing ever removes an entry or stops the listener.** An activator outlives its service until
  the daemon exits.
- **An entry already in the set is skipped without comparing addresses.**

That makes the obvious recovery, uninstalling and installing again, fail too. Uninstalling Adminer
deletes its row but leaves the old activator listening. The reinstall writes a new row under the
same id, `php-fpm@adminer`, and allocation gives it a different activation port, because the old
port is still bound. `hold_all` then sees the id in `HOLDING` and binds nothing, so the site file
names a port nobody listens on.

`runtime.uninstall` followed by installing the same PHP version again has the same flaw.

### What the screen leaves out once an install does work

- A `web-app` has no *Open*. Its *State* column is a dash, and it has no Start or Stop. The site
  that is the whole point of a `web-app` can only be reached from the Sites screen.
- A `service` row shows **both** Start and Stop, whatever its state.
- An installed row does not say what the add-on is for. `ExtensionSummary` has no description;
  `ExtensionOffer` has one.
- The *Kind* column shows wire values (`web-app`, `service`) instead of words.
- An offer with no artifact for this machine (`artifact.type == "other_targets"`) still shows an
  enabled *Install*. Clicking it opens a plan dialog that has no plan, only `extension.plan`'s
  `ExtensionNoArtifact` refusal and a disabled *Install* button. The listing already knew the
  answer.

## Decisions

### D1 — `extension.install` makes the pool reachable before the job ends

In `Extensions::install`, the two repairs run **between `install::install` and `configure_one`**.
That is after the pool row exists and before the site is declared. They run in `runtime.install`'s
order:

1. `activation::ensure`;
2. then `activate::hold_all`.

The site file Caddy first loads therefore already names the activator. Today the first site file
names a single upstream.

Both repairs are reported and not fatal, on T72a's reasoning: the next daemon start repairs the
same thing. Both also run only when the install wrote a site. A `service` extension needs no
activator, because `extension.start` starts it.

The job does **not** start the pool. A `web-app` follows a project site: the first request starts
it, and the idle stop puts it back to sleep (T70). Starting it at install would put a php-fpm
master on the machine for a tool the person may open once.

### D2 — An activator is held per address, and given back with its service

`HOLDING` becomes a map from `ServiceId` to the address held and the handle of its accept task.

- **`hold_all`** skips a service only when the address it holds is the address the generator
  names now. A service whose address changed has its old listener stopped and the new one bound.
  This alone makes a reinstall work, on any path that reaches `hold_all`.
- **`activate::release(service)`** stops the accept loop, which drops the listener and frees the
  port, and removes the entry. Connections already being carried finish on their own tasks.
  `extension.uninstall` calls it after the pool's row is gone, and so does `runtime.uninstall`,
  which has the same flaw.

Releasing at uninstall is not just tidiness. An activation port held by a dead activator is a port
the allocator cannot hand out, and the person never chose that number.

### D3 — The window follows the install job to its end

`api.extensionInstall` is typed as answering a `JobSummary`. `PlanDialog` closes once the daemon
has accepted the job and hands the screen the job's id. The screen follows the job the way
`usePackages` follows a runtime install:

- `subscribeDaemonWatch` delivers the events;
- `applyJob` keeps the progress;
- `jobFinished` gives the ending, and for a failure the reason.

While the job runs, the add-on's row in *Available* shows the job's message and percent in place of
its *Install* button. Other rows can still be installed, and each one follows its own job.

When the job ends, the screen re-reads its lists. A failed job shows the daemon's own error in the
screen's `ErrorBanner`, through `errorMessage`, which is the same rendering a refused call gets.
For a failed install, `job_finished` is the only place the reason exists.

**Why the dialog does not stay open until the job ends:** it is `locked` while busy. A download
that takes a minute would hold the whole window behind a modal that cannot be dismissed. The row
is where the person looks next anyway.

**What this does not follow:** a job the screen did not start, for example one `mix extension
install` started, or one still running after MixLab restarted. Such a row keeps its *Install*
button until the next re-read, and a click on it is refused by name (`ExtensionAlreadyInstalled`)
once the job has written its row. Following those jobs would need the screen to read `job.list`,
which T199 left to `mix` on purpose.

### D4 — One add-on, one row

- *Installed* lists what is installed.
- *Registry* is renamed **Available**, since "registry" is our word and not the user's. It lists
  only offers with `installed == false`.

The *Installed* pill goes, because the add-on is now one card up. *Installed* with nothing in it
shows a single-line empty state, *"Nothing installed yet"*. Today it shows an empty table with
column headers.

The *N entries could not be read* note and the stale badge stay on *Available*, unchanged.

### D5 — Every row says what it is

`ExtensionSummary` gains `description`, read from the manifest stored in the extension's row
(`manifest.extension.description`, which every manifest already carries). `ExtensionOffer` already
has one. Per ADR 0019 the new member is optional on the wire, so an older daemon leaves it out and
the window shows nothing in its place.

- Both cards show the description on a second line under the name, muted, cut to one line, with
  the full text on hover.
- `mix extension list` prints it too, so the CLI and the window say the same thing.

*Kind* becomes a word through i18n: *Web app*, *Service*, *Config*. A kind this build does not know
is shown as the daemon wrote it, by the rule `serviceStateLabel.ts` already follows.

`bash packaging/bindings.sh` regenerates `bindings/`. No method is added, so the client surface
does not change.

### D6 — A `web-app` row is its site

The daemon already says how a `web-app` is controlled: `extension.start` refuses it with *"served
as a site"* (T81b, D10). So the window drives a `web-app` through `ExtensionSummary.site` and the
site it names in `sites()`, matched on the domain:

- **Open** is the Sites screen's *Open*: the same `siteVisit` helper and `openUrl`, so scheme and
  port are decided where they already are. With D1, the request wakes the pool.
- **State** is the site's: *On* for `enabled`, *Off* for `disabled`.
- **Turn on / Turn off** call `site.start` / `site.stop`, and only the one that applies is shown.

**Why not the pool's state.** A healthy add-on's pool is stopped most of the time, asleep until a
request wakes it, so showing *Stopped* on a working Adminer would tell the person something is
wrong when nothing is. The site's state is the answer to the question they are asking: will this
open?

A `web-app` whose site cannot be found, which only a broken install can produce, offers nothing but
*Uninstall*, and its state says the site is missing.

### D7 — A `service` row shows the action that applies

- Stopped or failed: *Start*.
- Running or starting: *Stop*.

Today both are always shown. The state pill is unchanged (`serviceStateTone` / `serviceStateKey`).
A `recipe` has neither action nor state, as today.

### D8 — An offer this machine cannot install says so in the list

When `artifact.type == "other_targets"`, *Install* is disabled and the reason sits beside it:
*"Not available for this system"*, with the published targets on hover. `published` and
`not_required` install as today.

## Out of scope

- **An Open for a `service` with a web interface** (Mailpit's UI on `ui_port`). The window cannot
  know which of a manifest's ports is a page, and guessing from a port's name is business logic in
  a client. The manifest has to declare it, which means a format change here and a publish in
  `mixengine-packages`. That is **T200a**.
- **The starting page's "Open MixLab to see why".** When a wake fails, the activator logs
  `Refused::WouldNotStart` in the daemon log. No screen in MixLab shows that line next to the site
  it belongs to, so the page's sentence promises something the window cannot keep. D1 and D2 make
  that page rare for an add-on but do not make the sentence true. That is **T200b**.
- **Upgrading an installed add-on** to a newer published version. Nothing in `extension.*` offers
  it today. One side effect: D4 hides an offer once it is installed, so a newer published version
  is not visible until that work exists.

## Tests

D1 and D2 are tested at two depths, because the rows and the real request fail in different ways.

**`crates/mixengine-daemon/tests/extension_sites.rs`**, rows only, the PHP declared with
`declare::php_pool` as the file already does. All of the following happen without restarting the
daemon:

- After installing the fixture `web-app`, the pool's row has an activation port wherever the recipe
  needs one.
- The second upstream the site file names accepts a connection.
- After an uninstall, that address refuses connections.
- After installing again, the new address accepts.

**`crates/mixengine-cli/tests/php_site.rs`**, through `harness::php_site` with a real PHP from
`MIXENGINE_PHP_RUNTIMES`:

- Install a `web-app` from a path. Its first request through the front end gets the application's
  answer, not a 502, with no daemon restart.

Per *tests-that-say-why*, each failure message carries the site file and the daemon log.

The window: `vitest` covers the pure halves:

- splitting offers into the two cards;
- the action a row offers, for each kind and state;
- the kind label.

The rest is checked by hand in `npm run dev:app`.

## MixLab

All of it is the **Add-ons** screen (`screens/Extensions/`), plus `PlanDialog`:

- D3: the install job followed on its row;
- D4: the two cards;
- D5: descriptions and kind labels;
- D6: Open / Turn on / Turn off / state for a `web-app`;
- D7: the single service action;
- D8: the disabled Install.

D1 and D2 are daemon-only. The screen has no part in them except that *Open* now works.

The client surface is unchanged:

- every method the screen newly calls (`site.start`, `site.stop`, `site.list`) is already reachable;
- D5 adds a response member, not a method.

Strings go into `en.ts` and `vi.ts` together, following *writing-user-facing-text*. A user-visible
change gets its line under `## [Unreleased]` in `CHANGELOG.md`:

- `Fixed`: an add-on's site opens without restarting MixEngine, including after a reinstall;
- `Changed`: the Add-ons screen.

## Acceptance

- On Windows, macOS and Linux, installing Adminer from the window and clicking *Open* shows
  Adminer's login page with no daemon restart. Uninstalling it and installing it again does the
  same.
- During the install the Adminer row shows progress. When the job ends the row moves to
  *Installed* without leaving the screen.
- A failed install says why, in the window.
- No add-on appears in both cards.
- Each row offers only actions that apply to its state.
