---
status: implemented
date: 2026-10-07
task:
  - T200a
  - T200b
  - T200c
---

# T200a, T200b — An add-on's own page, and why a site is down

Roadmap tasks T200a and T200b, in phase 38. 2026-10-07. They follow
[T200](2026-10-07-t200-an-add-on-works-the-moment-it-is-installed-design.md), whose hand check is
where both were found.

## The problem

### T200a: Mailpit has a web page and no way to open it

Mailpit catches mail on its SMTP port and shows it on a web page on its `ui_port`. After T200 the
Add-ons screen can start and stop Mailpit, but nothing opens the page.

- The window cannot open it on its own. It would have to guess which of the manifest's ports is a
  page, and guessing from a port's name is business logic in a client.
- The daemon cannot tell either. `[ports]` is a list of numbers, and a `ready` check over HTTP
  says the service answers HTTP, not that a person has anything to look at. MeiliSearch answers
  HTTP and has no page.

So the manifest has to say it.

### T200b: "Open MixLab to see why" points at nothing

When a request cannot wake what a site needs, MixEngine's starting page ends with *"MixEngine
could not start what this site needs. Open MixLab to see why, then reload this page."* MixLab has
nowhere that says why:

- The supervisor writes `failed` to the service's row and nothing else. `StateReason::SpawnFailed`
  carries no sentence, and the error that explains it, for example *"the environment entry
  MIXENGINE_DB_PASSWORD: no credential is stored at …"*, exists only in `daemon.log`.
- `ServiceSummary` has `state` and `last_exit_code`. A spawn that failed has no exit code.
- The Sites screen does not show the state of the services a site runs on.
- The `events` table that `0001_initial.sql` created for "recent events" has never had a row
  written to it.

The phpMyAdmin keyring bug (fixed in #251) was this exactly: a pool `failed` with nothing on
screen, and the reason found only by reading `daemon.log`.

## Decisions

### D1 — `[ui]` names the port a person opens (T200a)

A `service` extension may carry:

```toml
[ui]
port = "ui_port"   # a key of [ports]
path = "/"         # optional, "/" when left out
```

**Rules, checked at parse time:**
- `port` must be a key of `[ports]`.
- `path` must start with `/`.
- `[ui]` is refused on a `web-app`, which already has a site, and on a `recipe`, which runs
  nothing.

**No host and no scheme.** The address renders as `http://127.0.0.1:<allocated port><path>`.
`127.0.0.1` is where the person's browser is, whatever `permissions.network` lets other machines
reach. There is no `https` because the service listens in plain HTTP; its port is not behind the
front end. A host written anywhere in a manifest is refused already (T80), and this keeps that
rule.

**It goes through `Raw` as well as the checked type.** An installed extension's row keeps its
manifest as `manifest::to_value`, which serialises `Raw`, and the registry's generator renders
entries the same way. A `[ui]` the checked type held and `Raw` dropped would vanish from every
installed row and every published entry.

**Why a table and not a flag on a port.** `[ports]` is `name = number` today. Giving it a second
shape would make every reader of that table learn a new one, for the sake of a single entry.

### D2 — `ExtensionSummary.ui` is the address, and the daemon renders it (T200a)

`ExtensionSummary` gains `ui: Option<String>`, the rendered address, and it is optional on the wire
(ADR 0019). The daemon renders it from the manifest stored in the extension's row and the port
held in `extension_ports`. The client only opens what it is given.

### D3 — Where it shows (T200a)

- **MixLab.** A `service` row with `ui` offers **Open**. If the service is not running, Open starts
  it first (`extension.start`), then opens the address with `openUrl`, the same way the Sites
  screen's Open starts a project's services before opening. If the start fails, the address is not
  opened and the error goes to the banner.
- **`mix extension list`.** The `SITE` column becomes `OPENS AT`. It holds the site's domain for a
  `web-app` and the `ui` address for a service.

### D4 — Mailpit's `[ui]` is published once `master` can read it (T200a → T200c)

The manifest reader uses `deny_unknown_fields`. Every MixEngine already released counts a
registry entry with a `[ui]` table as *"an entry this build cannot read"*, and drops it from
`mix extension available` and from MixLab's Available card.

So the change ships in two steps:

1. **This task (T200a):** the reader, the surfaces, and the testkit fixture
   `fixtures/extensions/mailpit.toml` with `[ui] port = "ui_port"`.
2. **T200c, as soon as T200a is on `master`:** the same lines in
   `mixengine-packages/data/extensions/mailpit.toml`, published by its `publish-extensions`
   workflow at that merge's full commit SHA. The generator is built from that commit, so it is the
   earliest commit whose reader accepts `[ui]`. That repository's `check-extensions` builds the
   generator from `master` too, so the manifest change is pushed only after the merge.

**Published ahead of a release, and what that costs.** This document first said T200c would wait
for the release that carries T200a. It was decided instead to publish when T200a reaches
`master`, so the work ends in one piece rather than as a task left waiting on a release date.
Until that release ships, a MixEngine already installed (v0.0.15 and earlier) reads Mailpit's
entry as *"an entry this build cannot read"* and says to update. It no longer offers Mailpit in
`mix extension available` or in MixLab's Available card. A Mailpit already installed keeps
running as before, because its row holds the manifest it was installed with. The release that
carries T200a lifts the restriction.

**An installed Mailpit keeps the manifest it was installed with.** Nothing re-reads the registry
into an installed row (T81). It gets Open after it is uninstalled and installed again, and the
changelog line for T200c says so.

### D5 — A service's last failure is kept, with the sentence that explains it (T200b)

Migration `0002_service_last_failure.sql` adds `services.last_failure_json`, a nullable column. It
is the first migration since the fold into `0001_initial.sql`, and it is additive, so a database
written by any released build migrates in place.

- **Written on every transition into `failed`, in the same transaction as the state.** That is
  `services::transition`'s job, not the runner's: a note written beside the transition could be
  missing on the one row where the write after it failed. The runner's `give_up` passes its
  sentence through a variant of `transition` that takes one, and the six other callers pass none.
  The note holds:
  - `at`: when;
  - `reason`: the `StateReason`, exactly as the transition wrote it;
  - `detail`: always a sentence (see below).
- **Cleared on the next transition into `running`.** A failure that has been recovered from is
  history, and showing it would send a person after a problem that is gone.

**`detail` comes from where the error is already known.**
- The runner's two `SpawnFailed` sites (the environment did not resolve, the program could not be
  started) pass the error they already log, in its full chain (#251 made that chain readable).
- For every other reason (`ReadyTimeout`, `PortInUse`, `CrashLoop` and the rest), the reason's own
  `Display` is the sentence.

So a client never has to turn a `StateReason` into words. Rust already has that table, and a
second copy in TypeScript would drift from it.

**Reported only while the service is `failed`**, on `stopped_by`'s rule (T167d). A failed service
that a person then stops keeps its column, but no listing shows a failure the state no longer
claims.

**A column and not the `events` table.** What the question needs is the *current* failure of one
service. A column answers it with one read. An event log would need a query for the newest
failure since the newest success, and that table's 30-day trim, plus every other kind of event
nobody writes yet. When "recent events" is built, it can write there too.

`ServiceSummary` gains `last_failure: Option<ServiceFailureNote>` with `{ at, reason, detail }`,
optional on the wire. Every client that lists services gets it with no extra call.

### D6 — Where a failure shows (T200b)

The page sends a person to MixLab, so the reason has to be where the site is:

- **Sites screen.** A site whose pool is `failed` gets a danger line under its domain:
  *"php-fpm@phpmyadmin could not start: the environment entry MIXENGINE_DB_PASSWORD: no credential
  is stored at …"*, plus a **Start again** button that calls `service.start` on the pool.
  - The pool is `SiteSummary.kind`'s, matched against `services()`, so the list needs no extra
    call per site.
  - The services a site is *linked* to (its database, its cache) are only in `site.show`. The list
    does not name them, so their failures show on Dashboard and in `mix site show`, not under the
    domain. A pool is what a request wakes, so it is what the starting page is about.
- **Add-ons screen.** A `web-app` whose pool failed shows the same line under its name, and a
  failed `service` shows its own. The pool comes from the site's `kind`, as on Sites.
- **Dashboard.** The failed pill's tooltip carries the detail. The pill and its colour are
  unchanged.

**`mix` says the same.** `mix service list` ends with one line per failed service that has a
note: `php-fpm@phpmyadmin failed 2 min ago: …`. `mix site show <domain>` names the failed service
and the sentence. Neither adds a method: both read `service.list` and `site.show`, which already
answer.

### D7 — The starting page says where (T200b)

The page's sentence becomes *"MixEngine could not start what this site needs. MixLab's Sites
screen says why, and `mix site show {{ domain }}` does too. Reload this page once it is fixed."*

- **Two places, because the page is MixEngine's**, and a headless install has no window.
- **The reason itself is not printed on the page.** A shared site is served to the local network
  (T52), and a sentence naming a credential store's address, or a path under somebody's home,
  must not reach a phone on the same Wi-Fi.

## Out of scope

- **"Recent events."** The `events` table stays unwritten. D5 is a column on purpose.
- **Restarting a failed service on its own.** T200's activator already tries a failed service
  again, at most every 10 s. This task makes the failure visible; it does not change when anything
  starts.
- **`[ui]` for a `web-app`.** A web-app's page is its site, opened since T200.

## MixLab

- **Add-ons screen** (`screens/Extensions/`):
  - Open on a service with `ui`, starting it first when needed (D3);
  - the failure line on a web-app or service row (D6).
- **Sites screen** (`screens/Sites/`): the failure line and Start again on a site whose pool
  failed (D6).
- **Dashboard:** the failed pill's tooltip (D6).

The client surface is unchanged: `extension.start`, `service.start`, `service.list` and
`site.list` are already reachable, and D2 and D5 add response members, not methods. Strings go
into `en.ts` and `vi.ts` together. The changelog gets:
- `Added`: Mailpit's page opens from Add-ons; a Mailpit installed before needs installing again;
- `Changed`: a site that cannot start says why in Sites, Add-ons and `mix site show`.

## Acceptance

- **T200a.** A service extension installed from a folder with `[ui]` lists an `ui` address in
  `extension.list` and in `mix extension list`. In MixLab, Open starts it when it is stopped and
  opens that address. A manifest whose `[ui].port` names no port, or a `[ui]` on a web-app, is
  refused at parse with the field named.
- **T200b.** A pool whose keyring entry is missing reaches `failed` with a `last_failure` whose
  `detail` names the missing entry. After that:
  - `mix site show` prints that sentence for its site;
  - Sites shows it under the domain with Start again;
  - after the cause is fixed and the service reaches `running`, the note is gone.

  The starting page names Sites and `mix site show`, and contains no failure text.
- **T200c**: Mailpit installed from the registry opens its page from Add-ons.
