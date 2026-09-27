# Client surface

**Goal**: prove, on paper, that the API is sufficient for a full graphical client — before a client
finds out by hitting a wall.

The desktop application under `apps/desktop/` is that client
([ADR 0027](../decisions/0027-the-desktop-client-lives-in-this-repository.md)), and it reaches the
daemon through the same JSON-RPC API and event stream the CLI uses, typed against `bindings/`.
Since ADR 0027 the list below is checked by a compiler as well as by reading: a type reshaped in
`mixengine-proto` fails the desktop typecheck in the same CI run. This page is not that
client's design; it is the list of things such a client must be able to *ask for*. Every line below
is a claim about the API, and each one is either satisfied by a method in
[architecture/daemon-and-ipc.md](../architecture/daemon-and-ipc.md) or it is a gap.

Since **T56** those types are not something a client generates for itself: they are committed as
[`bindings/`](../../bindings/), checked current by CI's `bindings` job, and published as
`mixengine-api-<version>-typescript.tar.gz` on every release, signed with the same key as the
binaries. What they state is what the daemon **writes** —
[ADR 0020](../decisions/0020-the-published-contract-is-the-shape-the-daemon-writes.md).

## Screens, and what each one demands of the API

1. **Dashboard** — per-service state, version (`ServiceSummary::version`, T183), port and uptime
   from `service.list`, and CPU % and RSS from the metrics stream, joined on
   `MetricsSubject::Service(id)`; start/stop/restart per service and a global stop-all; disk
   usage broken down by **five** categories from `daemon.disk_usage` — runtimes, data, logs,
   certs and `cache/` — each carrying what would reclaim it, with `daemon.cleanup` behind the
   button and only two of the five in its reach (**T96**); the recent slice of the event stream.

   **This line said "in one read" until 2026-09-06, and the correction is the point rather than the
   pedantry.** The join is exact — `MetricsSubject` wraps a real `ServiceId` and not a guess — but
   the two halves are read on different cadences on purpose, and collapsing them into one call would
   undo the invariant T71 built: the fast reading exists **only while somebody is subscribed**, so a
   `cpu_percent` answered by `service.list` is a way to poll a laptop at 1 Hz without ever opening
   the subscription that was supposed to gate it. The alternative — answering from the last minute's
   frame — puts a figure in a struct with nowhere to say *when* it was taken, which is exactly what
   `MetricsFrame.at` exists to prevent. So a dashboard holds the stream open and calls `service.list`
   again when the event stream says something changed: one list plus one stream, not two reads a
   frame. **Uptime is not a third thing**: it is `last_started_at` with `state`, and the daemon and
   the client share a clock because they share a machine.
2. **Sites** — list carrying domain, runtime version, HTTPS state and health, and its owner
   (**T81b**: `SiteOwner` is a project by name or an extension by id — an extension's site is shown,
   started and stopped, and every other edit is refused with the uninstall command that removes
   it); create and edit with
   doc root, kind, PHP version, linked services and extra domains; reveal the doc root path and a
   browsable URL so the client can open a browser, a file manager or a terminal itself; per-site LAN
   sharing toggle — **T74**: `site.share` answers the interface, the address and the URL, and
   `site.unshare` takes it back; a machine with more than one network refuses rather than choosing,
   and names the candidates so a client can offer them.

   **And what is behind the site, one prefix at a time — T135.** `SiteSummary.routes` is an ordered
   list of `SiteRoute`, **in match order** — longest prefix first, which is how the front end resolves
   an overlap and therefore the only order a client should draw. `SiteCreate.routes` and
   `SiteUpdate.routes` **replace** the list rather than merging into it, on `domains`' rule: with a
   merge there is no way to remove one. Three targets — a proxy to an address whose path, when it has
   one, replaces the matched prefix; a php-fpm pool, which the daemon resolves when the client names
   none; and a directory, whose prefix is stripped. On the list they are a count, because "does this
   site have more behind it" is a question about every site at once; on the detail they are the whole
   list. `mix` types them as `--proxy /api=http://127.0.0.1:3003/xyz`, `--php /admin[=<pool>]` and
   `--files /assets=dist`, with `--no-routes` for the empty list —
   [ADR 0035](../decisions/0035-a-site-is-a-kind-and-a-set-of-routes.md).

   **A second toggle beside HTTPS itself — T98.** `SiteSummary.https_redirect` and the same field on
   `SiteCreate`/`SiteUpdate` are what a client reads and writes; the one thing worth a client knowing
   ahead of asking is that the daemon refuses `https_redirect: true` whenever the site's own `https`
   resolves to `false` in that same request — `invalid_argument`, hinted `` `--https true` first, or
   leave `--https-redirect` unset ``. A client is free to enforce the same rule client-side (disabling
   the toggle until HTTPS is on) or to let the refusal surface; either is a legitimate reading of one
   `invalid_argument` case, and this page states the rule rather than the client's choice about it.

   **And the share that ends without anybody ending it — T76.** `site.share` takes an optional
   length (`for_seconds`), `SiteSharing` carries the deadline back, and
   `DaemonEvent::SiteSharingChanged` announces every change in either direction with a
   `SharingChange` saying why: somebody asked, the length ran out, or this machine left the network
   the site was shared on — that last one carrying both addresses, because the pair is the
   explanation.

   **This is the one place where `mix` is genuinely the weaker client, and it is written down rather
   than left to be discovered.** A share that ends while nobody is looking is exactly the change
   somebody needs to be told about, and a terminal is not where they are looking: with only the CLI,
   the reason is in `daemon.log` and on a stream nothing is reading. A graphical client is where
   `SiteSharingChanged` becomes a notification — the whole reason the variant carries its reason
   rather than only its state. No API is missing; the affordance is.
3. **Runtimes** — installed versions per kind with the default marked; available versions;
   install/uninstall as jobs reporting progress; PHP extension toggles per version.
   **And a version that is on disk but not listed — T182f**: a folder an earlier install left, which
   the daemon records on its own at start when it can check it. One it could not check is named by
   `daemon.doctor`, and `runtime.adopt` / `package.adopt` record it on request (`mix runtime adopt`,
   `mix package adopt`). A screen that offers *Install* for such a version gets `already_exists` with
   the adopt command in the hint, and offers that instead.
   **And a service's data an earlier install left — T182g**: `service.found` lists each data
   directory with no service row, with the installed version that opens it or the daemon's reason
   none does; `service.adopt` brings one back, stopped, with a new admin password. MixLab's Dashboard
   shows a card while `service.found` answers rows, and a list with **Adopt** where the answer has
   `opens_with` — the client decides nothing about versions.
   **And what an extension's plan names before anybody agrees to it — T82**: a `web-app` freezes two
   things at install, the php-fpm pool it runs on and the database it administers, and
   `ExtensionPlan.site` carries both. Which server an administrative interface opens onto is not a
   detail somebody should discover afterwards, so a client that shows the permissions shows this
   beside them; `mix extension plan` prints it on its own line.
   **And which account it would be signed in as — T82a**: a `web-app` declaring
   `[web-app.database].signs_in` is handed that server's superuser password in a php-fpm pool of its
   own, and `ExtensionPlan.site.signs_in` names the account. It is the most consequential thing an
   extension can be granted, so a client renders it *among* the permissions rather than beside the
   site's domain — and says the same three things `mix` says: which account, that the password comes
   from the OS keyring when the pool starts, and that nothing writes it to disk.
4. **Services** — the settings a service accepts (port, bind, data dir, limits, autostart, idle
   timeout) as data, not as a rendered form, which version of its program it runs
   (`ServiceSummary::version`, T183), and who left a stopped one stopped
   (`ServiceSummary::stopped_by`, T167d) — so a client draws a service MixEngine put to rest as
   resting rather than as a failure;
   **and `autostart` is one of them since T112, which it was not when this line was written.** The
   column existed from `0001_initial.sql`, `service.create` wrote it, and nothing read it: no
   member on `ServiceSummary`, no method to change it, and no caller in the boot path. It is now
   `ServiceSummary::autostart` and `service.set_autostart`, and **T113** is what acts on it — a
   start plan over the flagged services, walked once after the endpoint is serving, so a service
   something flagged depends on comes up too. That is the one thing in this page that a client
   could not draw, found by a person using the product rather than by reading; the generated config readable back for display only;
   credentials fetched on demand; validation failures returned per field, not as one string. **And,
   for a database service, where it can be opened — T83**: whether a desktop database client is
   installed to hand the connection to, and the handoff itself, answered per service so a client
   draws the affordance from data instead of probing the filesystem for an application
   ([extensions.md](extensions.md)). `database.client` answers
   `DatabaseClientReport { protocol, secret, client }` — `installed` with this install's window and
   its executable (T107), or `no_client` for an install with none (T165) — and `protocol: null` for a
   service no client opens, all of them states. `database.open` answers
   `DatabaseHandoff` with `launched: running | handed_on` and `secret`, the keyring address the
   password was read from, never the password: it went into the started process's environment and
   nowhere else. **And making one — T77a**: a client creating a project on a database stack needs
   `database.create`, which answers the database, the account, and the keyring address the credential
   sits at — and, since T77b, an optional `password` a person chose, so the flow that creates a
   project can offer *"use my own"* instead of always generating one. `database.create` itself never
   *answers* with the password: a client that wants to show one asks `database.credentials`
   ([ADR 0025](../decisions/0025-a-credential-is-answered-only-by-a-method-that-exists-to-answer-it.md)),
   which exists for exactly that and nothing else — T83's handoff is the other way a password
   reaches somewhere outside the keyring, into a process MixEngine starts rather than a screen.
   **The address is both halves, and `client` carries one too — T84**: `secret` is
   `{ service, key }` rather than the key alone, so a client renders *"stored in your credential
   store as …"* without hardcoding MixEngine's namespace — which is the business logic `CLAUDE.md`
   keeps out of clients. `database.client` composes it from the recipe and the service id, starting
   nothing and reading nothing, so the affordance can be drawn before anything is opened.
   **And repairing one whose password nothing knows — T127**: `service.reset_credential` takes
   `ResetCredential { service, wait }` and answers `ServiceWalk`, the same type a restart answers,
   because that is what it is — a stop, the recipe's own offline password step, and a start of what
   went down. Its own params type rather than `ServiceTarget`, because that type's `service` is
   optional and means *the whole home* when absent, which is not a thing a repair may be asked for.
   A client that draws this draws a **confirmation** with it: it stops the service and everything
   that depends on it, and rewrites a credential inside a data directory. What it must also say is
   the part that decides it for somebody — every database in that directory is kept. The account of
   what ran is the job of the same name, so a client already showing jobs has the log for free. A
   service that keeps no credential is refused before anything stops, so the affordance can be drawn
   from `ServiceSummary` and the refusal is a mistyped subject rather than an outage.
5. **Logs** — a live tail filterable by service, with the on-disk path so the client can reveal it.
   Log lines arrive on their own stream, never the event stream
   ([ADR 0009](../decisions/0009-logs-travel-on-their-own-stream.md)).
6. **Domains & TLS** — the per-domain diagnostic table of
   [domains-and-dns.md](domains-and-dns.md); CA status with install and uninstall; per-site
   certificate status and reissue. **Two trust answers and not one** — T49b: `cert.ca_status`
   carries `trust` for the system store and `browsers` for the NSS databases Firefox and Chrome read
   instead, one row each with the path and which browser owns it. A client that collapsed them would
   show a green tick beside a browser that shows a red padlock. The repair for the second is
   `daemon.doctor_repair` and raises no prompt, so it is a button and not an elevation flow.
   **And per-site certificate state — T50**: `cert.issue` answers one `SiteCertOutcome` per site with
   the names its certificate covers and how many days it has, so a client renders a table rather than
   asking per site. Reissuing is the same call, is idempotent, and raises no prompt.
7. **Blueprints** — capture the current project, list what is captured, import one somebody else
   wrote, apply one to a new project ([blueprints.md](blueprints.md)). **Two obligations a client
   cannot decline** — T78a: every listing that names a blueprint shows whether anything vouched for
   it, because `blueprint.import` decides that once and nothing ever raises it; and before an apply
   whose plan holds a `RunScaffold` step, the client shows that exact command with the trust state
   and sends a `ScaffoldConsent` naming both. The daemon refuses a consent that disagrees with
   either, so a client that skipped the marking would be a client whose applies fail. The plan
   carries `source` and `trusted` for exactly this, so no second call is needed. Job output — what
   the command prints — is `GET /logs/job/{id}`, on the log stream and never the event stream.
   **A third, added by T121: no consent must not be a silent answer.** `mix` asks `Run it? [y/N]`
   and cannot be walked past; a graphical client whose box is simply left untouched has to say, on
   the control that starts the apply, that the command will not run and the project directory will
   be left empty — otherwise an apply that downloads a runtime, a database and a web server ends in
   an empty folder nobody was told about. The same sentence is owed again afterwards, where the
   step comes back `NotRun`: `StepResult::NotRun`'s `why` ends in a `mix` flag, so a client with no
   command line writes its own.
7a. **Taking MixEngine off this machine is not a screen of a graphical client**
   ([ADR 0051](../decisions/0051-an-uninstall-ends-what-it-undoes.md)). A finished uninstall ends
   the daemon, so a window offering one would outlive what it undid. It belongs to the Windows
   uninstaller and to `mix uninstall`, which call `daemon.uninstall_plan` first and always, then
   `daemon.uninstall`. The plan names any process running from a directory that would go
   (`blocked`), and the act refuses while one is; `keep_home` and `keep_relocated` are the two
   offers to leave data alone. The report is a measurement of the machine afterwards, and the
   caller reads the `on_exit` rows back off disk once the daemon has gone.

8. **Extensions** — browse the registry, install and uninstall, per-extension settings, and whatever
   an extension needs to be opened ([extensions.md](extensions.md)).
9. **Settings** — root directory, managed TLDs, default web server, autostart, and
   `daemon.doctor_repair`. **Updates are not on this list for MixLab** (T187,
   [ADR 0056](../decisions/0056-mixlab-stands-without-mixengine.md)): MixLab updates itself, with
   or without the daemon, so its `mixengine` module draws none of `update.*`. Those methods stay for
   `mix self-update` and for any other client. **Autostart is a switch and `autostart.status` is what it reads** (T85b):
   the answer says which mechanism this machine has, where the entry lives, and — the one thing a
   client must not work out for itself — whether an entry that *is* registered belongs to this home
   or to another one, which is a switch that must read "on, for a different home" rather than "on".
   **Save battery is a switch over `service.save_resources` / `service.set_save_resources`** (T167b,
   ADR 0041): off unless a person turned it on, and while it is off nothing is idle-stopped. It is
   also `mix service save-resources`.
   **A `.pkg` copy updates through `update.hand_over` and `update.finish`** (T88f, ADR 0050):
   `update.status` carries `installer` for a copy the macOS `.pkg` installed (its placement stays
   `managed` on the wire) and `installed` once the new binary is on disk. `update.hand_over` opens
   the verified package in Installer.app and answers its path and the `installer` command, which a
   client shows rather than composes; `update.finish` then answers as `update.apply` does, and the
   daemon exits. They are `mix self-update` and `mix self-update --finish`.

   **"Default web server" is `ServiceSummary::role` and `service.set_front_end`** — **T97**, on
   [ADR 0026](../decisions/0026-the-active-front-end-is-a-row-and-switching-it-is-a-job.md). The
   reading is a member on every service summary, carrying which of the two programs a front end is,
   so a client picks the active row out of `service.list` and never maps a package name to a
   meaning. The switch is a job rather than a setting, which is what makes ADR 0004's "the switch is
   one setting" precise: on Linux the port-80 grant is written into the binary, so changing front
   end changes which binary needs it.

   **A client must render the three endings that leave the home where it started.** `NotGranted` is
   a machine where nobody allowed the new server to answer on 80 and 443 — the switch did not
   happen, the grant is waiting, and `elevation.grant` followed by the same call works.
   `RolledBack` is a configuration the new program's own checker refused. `Failed` is the ending
   nothing can prevent. `answering` beside them is measured after the walk, so a screen can say
   *this home's front end will not start* rather than implying the switch caused it.

   **The commands on this user's PATH are `path.status`, `path.install` and `path.uninstall`**, and
   since **T130**/**T131** the report says more than whether the directory is on the PATH. `commands`
   is what `<root>/bin` holds, read off the directory; `origins` says what put each name there —
   built in, a client of an installed package, or a tool somebody installed into a runtime — and
   `conflicts` names a command two installed packages both claimed and which of them won. A screen
   listing commands without their origins is a screen that cannot answer *why is `mysqldump` here*
   or *whose `mysql` is this*.

   **`path.rescan` is the fourth**, and it is the one a person types rather than a screen calls on a
   timer: the daemon already looks for a tool installed into a runtime every `[bin] rescan_seconds`,
   so this exists for the moment in between and for a home that slowed that key down.

**And one screen that is drawn before any of this** — roadmap task **T146**. A window that finds no
daemon running offers a choice of where the four growing directories go: `runtimes/`, `packages/`,
`data/` and `logs/`, one row each, with the directory the daemon would use and a chooser beside it.

**It is the one screen in this document that reaches no API method**, and it cannot: what it asks
about is where a daemon that has not started yet will put its files. The answer comes from
`mixengined --storage`, a one-shot that prints the layout and whether the choice is still free and
**creates nothing** — not the home, not `config.toml`, not the database — because a screen that
created the home it asked about would itself be the reason the choice was gone. What the person
picks is handed to the start as `--runtimes`, `--packages`, `--data` and `--logs`
([ADR 0036](../decisions/0036-a-flag-may-configure-a-home-rather-than-a-process.md)).

The daemon says whether the choice is free and, when it is not, what closed it — *"3 runtimes and 1
service are installed"*. A client neither counts that nor writes that sentence. And the screen is
not offered when no `mixengined` is installed, because the answer comes from the program that is
missing; such a machine sees it the first time it opens the window after installing, which is still
before its first runtime.

A tray or menu-bar item needs no more than the dashboard does: overall state, stop-all, and the site
list. MixLab's is built (T168,
[the design](../specs/2026-09-19-t168-mixengine-in-the-tray-design.md)): a
webview panel that also starts and stops single services and asks `daemon.shutdown` — every one of
them a method this document already lists, so the tray added nothing to the API.

## Rules the API is responsible for

These are not client style guidance. Each one is a constraint on what the daemon must send, and the
reason a client can behave well without inventing anything.

- **State is announced, never inferred.** A client shows `Starting…` because the event stream said
  so. The API must make optimistic rendering unnecessary — a toggle that lies about whether MariaDB
  is up is worse than a slow one, and no client should have to guess.
- **Nothing blocks.** Any operation that can outlast a request is a job with an id and progress, so
  a client can render inline per-row state instead of a spinner over everything.
- **Elevation is explainable before it is requested.** `ElevationRequired` carries every batched
  operation and what it will literally change — the exact hosts lines, the port, the store — so a
  client can show that and then raise one prompt. A decline is a supported outcome the API models,
  not an error ([ADR 0005](../decisions/0005-on-demand-elevation.md)).
- **Every error carries its own remedy.** `Error::hint` is what a client renders as the suggested
  action, and the diagnostics bundle is `daemon.bundle` (T93): one call assembles the archive and
  answers with its path, so "copy diagnostics" is a file to open rather than five readings to
  gather. What it refuses to carry it names, so a client can show that too rather than presenting
  the archive as complete.
- **Metrics are sampled at two rates, and the faster one is only while watched.** Opening
  `GET /metrics` streams a reading a second and closing it stops that — the connection *is* the
  subscription, so a client that crashes cannot leave a laptop being polled. With nobody watching the
  daemon still takes one reading a minute, and that is what the 24-hour history is made of: *"what
  was eating my battery last night"* is a question about a night nobody was watching, so a history
  kept only while somebody looked would hold exactly the minutes that needed no recording. One
  reading costs about 10 ms on Windows and about 2 ms on Linux — measured — which is a fiftieth of a
  percent of one core once a minute. This bullet said "sampled only while watched" until **T71**
  built it and found the two halves could not both be true.
- **A missing minute means nobody measured, never that nothing was used.** The service was stopped,
  or the machine was asleep, or the daemon was being replaced. A client draws a gap; joining the
  points across one invents a night of measurements that were never taken. The same rule inside a
  reading: `cpu_percent` is `null` where no figure could be taken, and a client that renders that as
  0% is claiming a service was idle in the second it was most expensive.
- **Per-service CPU and RSS are aggregated across the process group** — a php-fpm master and its
  workers are one row. Shared pages are counted once per process, so the number overstates a pool;
  it is an overestimate on every platform equally, which is the safe direction for a figure this
  project defends in its README. It is **not** the quantity a `memory_mb` limit is judged against
  where a kernel holds it — that is commit charge on Windows and charged pages on Linux — and it
  **is** exactly that quantity where the T71a watchdog holds it instead, which
  `LimitSupport::memory_measure` says by answering `resident`.
- **A memory control is drawn differently for `Advisory` than for `Hard`** — roadmap task **T71a**.
  `Hard` is a wall: at the ceiling the service is killed or its next allocation fails. `Advisory` is
  a watched line: the service may go over it and keep running, and what follows is a warning and —
  where the service's recipe permits — a restart. A client must offer the control in both cases and
  must not present the second as a guarantee. `Advisory { why }` carries a sentence only where the
  machine could be started differently; `null` means an operating system with nothing to fix, and a
  client that printed a placeholder there would be inventing advice.
- **Whether *this* service would be restarted is not on `LimitSupport`.** That type describes the
  machine and is handed no service. `service.limits` answers per service, in `watchdog`:
  `{ after_minutes, restarts }`, or `null` where nothing is watching — which is both a machine that
  enforces the ceiling itself and a service that declared none. A client showing only the restarting
  case would say nothing about the services most worth saying something about, since a database is
  deliberately warned about and left alone.
- **A secret is read at the moment it is handed over, and never rendered on the way.** A client that
  wants to open a database elsewhere asks for the handoff and receives something it can act on; it
  does not receive a password to paste into a command line, because the daemon fetching a credential
  from the keyring at that instant is the only version of this that keeps it out of a shell history,
  an argument list and a log. The same rule is why "reveal password" is a separate deliberate call
  and not a field on a service read. **Built by T83**: `database.open` reads the credential at that
  instant, starts this install's window itself with it in that process's environment, and answers
  with the keyring address it came from — a client is told *where*, and is never handed *what*.
- **A dead daemon is a legible state.** A client that loses the socket can tell the difference
  between "not running" and "not answering", and reconnects without being restarted.

## Left to the client

Layout, theming, accessibility, keyboard navigation, empty states, localisation and window chrome
belong to whoever builds the client. Nothing in this repository specifies them, and nothing in the
API should assume them.

**Help text is a URL and not a method, and this line is why** — T90,
[ADR 0021](../decisions/0021-the-handbook-is-one-corpus-published-three-ways.md). A `help.get`
carrying Vietnamese prose would be `mixengined` deciding a client's localisation policy, which the
paragraph above assigns elsewhere; and help that needed a daemon would stop working in the one
situation it exists for. So the handbook is published as plain Markdown at
`https://mixnz.github.io/mixlab/<locale>/<slug>.md`, with `index.json` listing every page, its
title, its summary, both URLs and the SHA-256 of its bytes. A client fetches those, or ships its own
copy; `mix` embeds them and answers with no socket open. The addresses are stable for exactly this
reason.

## Acceptance criteria

- Every mutating RPC in [architecture/daemon-and-ipc.md](../architecture/daemon-and-ipc.md) is
  reachable from the CLI — which, since the CLI is the only client here, is the whole guarantee that
  no capability is trapped behind a screen that does not exist.
- Every screen above can be assembled from documented methods and events, with no method existing
  solely to serve one of them.

**The second criterion was not met until 2026-09-06, and this is where that is written down rather
than discovered.** Two things it promised had no method behind them — the Dashboard's disk usage and
the Settings screen's default web server — and both were found by **MixDB reading this page against
the API** while writing its own Phase 4 spec, not by anybody here. That is the arrangement
[ADR 0011](../decisions/0011-no-gui-in-this-repository.md) accepted working as designed and costing
what it costs: a claim made on paper in this repository is checked by somebody else's code, later.
The first is closed by **T96** — `daemon.disk_usage` and `daemon.cleanup`, reachable as `mix disk`
and `mix cleanup`. The second is closed by **T97** — `ServiceSummary::role` and
`service.set_front_end`, reachable as `mix service front-end` and `mix service set-front-end`. With
both landed the criterion above holds for the first time since this page was written, which is
[phase 10](../roadmap/phase-10-client-surface.md)'s milestone.
