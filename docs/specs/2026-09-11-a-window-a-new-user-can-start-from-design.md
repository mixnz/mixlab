---
status: implemented
date: 2026-09-11
---

# A window a new user can start from

Roadmap phase [14](../roadmap/phase-14-a-window-a-new-user-can-start-from.md), on
[the desktop client design](2026-09-08-the-desktop-client-in-this-repository-design.md) and on
[ADR 0027](../decisions/0027-the-desktop-client-lives-in-this-repository.md). 2026-09-11.

Phases 11 to 13 brought the window home, made it one product, and let a person who never wanted a
database client hide one. Every one of them was about what MixLab *is*. This phase is the first one
about what happens in the ten minutes after somebody installs it.

## The complaint

Four sentences from somebody using the finished product, and each one is a separate defect:

1. **A new user opening MixLab expects to get a website.** What they get is a list of things to do
   first: download a PHP, download Caddy, create a project, create a site, then start each service
   by hand. Six actions across four screens before a browser shows anything.
2. **PHP has no visible way to turn an extension on or off.** `redis`, `mongodb`, `xdebug` —
   somebody who needs one cannot find where to say so.
3. **After the machine restarts, nothing is running.** The services have to be started by hand every
   time, and there is no way to say *this one starts with MixEngine*.
4. **The navigation is a flat list of eleven items** in no particular order, with two of them called
   almost the same thing.

Three of the four are about affordances over an API that already answers; one of them —
number 3 — is a genuine hole in the API, and
[client-surface.md](../features/client-surface.md) has been claiming it was filled
since it was written.

## What is already true

Written down first so that nothing below is built twice.

- **`blueprint.apply` already does almost all of the first complaint's work.** Six blueprints ship
  inside the binary and are seeded into every home — `django`, `laravel`, `nextjs`, `static`,
  `symfony`, `wordpress`. An apply plans and then executes `RegisterProject`, `InstallRuntime`,
  `InstallPackage`, `EnsureService`, `CreateDatabase`, `CreateSite`, `AddDomain`,
  `IssueCertificate`, `SetPhpExtension` and `RunScaffold`, every action an *ensure*, resumable by
  running it again, with the version questions answered in the request and the elevation queued
  rather than prompted. The desktop already draws all of it: `screens/Blueprints/ApplyDialog.tsx`
  handles the plan, the answers and the scaffold consent.
- **PHP extension toggles already exist end to end.** `runtime.list_extensions` and
  `runtime.set_extension` (T28), `mix runtime ext enable|disable`, and
  `screens/Packages/ExtensionsPanel.tsx` — which renders a checkbox per extension, marks the
  statically linked ones, and says whether the pool reloaded, needs a restart or was not running.
  `igbinary`, `redis` and `mongodb` are loaded by build default and `xdebug` is one click away.
- **The `services` table has carried an `autostart` column since `0001_initial.sql`**,
  `ServiceCreate.autostart` writes it, `mix service create --autostart` sets it, and
  `core::services::declaration` reads it back.
- **`ServiceGraph::start_order()` is documented as "what autostart at boot walks."**
- **Starting a front end on a reserved port does not prompt.** `PrivilegedOp::PortAccessGrant` is
  enqueued once, when the front end is created or switched (`api/front_end.rs`), and grants whole
  state; after it is spent, binding 80 and 443 needs nothing.
- **`service.start` with no target starts everything this home declares**, in dependency order —
  `None => Ok(graph.start_order())` in `api/rpc.rs`.

## What is not true, and has been claimed

- **Nothing reads the `autostart` column to start anything.** `start_order()` has no caller in the
  boot path; the only two callers are `service.start` with no target and a test. The column is
  written, carried across a front-end switch, and never acted on. **Complaint 3 is real, and it is
  the one thing in this phase that is an API hole rather than an affordance.**
- **There is no way to change it after creation.** There is `service.set_limits`, `service.set_idle`
  and `service.set_front_end`, and no `service.set_autostart`. A person who created MariaDB without
  the flag has to delete the service to get it.
- **No client can even display it.** `ServiceSummary` has no `autostart` member, so `service.list`
  cannot report it, so the Services screen
  [client-surface.md](../features/client-surface.md) §4 describes — "the settings a
  service accepts (port, bind, data dir, limits, **autostart**, idle timeout)" — cannot be drawn.
  That line has been a gap in the surface since it was written.
- **A blueprint that declares a `[site]` does not ensure there is a front end to serve it.**
  `core::sites` is explicit that "a home with no front end renders nothing and this succeeds". So
  `blueprint.apply wordpress` on a fresh machine ends with a project, a database, a site row, a
  domain and a certificate — and nothing listening. **This is the exact shape of complaint 1 and it
  is not a missing feature; it is a planner that stops one action short.**
  [services.md](../features/services.md) already names the gap in its own words:
  *"Nothing installs a front end … a first run that offers to do it for them is not built and has no
  task of its own yet."* T115 is that task.
- **An apply starts nothing.** No `PlanAction` variant starts a service, and this is correct — see
  D3 — but it means the last step of "get me a website" is still manual.

## What this is not

- **Not a redesign of the window.** The tab strip, the `[+]` menu, the shortcuts, the Settings
  dialog and the module boundary are untouched. Phase 13's note stands: redesigning the window *for*
  the MixEngine profile is not this phase.
- **Not a new kind of blueprint, a new manifest key, or a seventh gallery entry.** The manifest
  schema does not change, so a blueprint captured before this phase applies after it and a blueprint
  captured after it applies on a build from before.
- **Not an installer change, and not a first-run wizard that blocks the window.** The Quick Start is
  a card on a screen that already exists, and closing it leaves a usable application.
- **Not a change to what a client is allowed to decide.** Everything below is either a method the
  daemon answers or a rendering of one.
- **Not a supervision change.** Restart policy, health checks, the idle sweeper and the memory
  watchdog keep every rule they have.

## Decisions

### D1 — Autostart is a service setting, read and written like the other two

`service.set_limits` and `service.set_idle` are the shape: one method, one service, one settings
struct in, the service's summary out. `service.set_autostart { service, autostart }` answers
`ServiceSummary`, and `ServiceSummary` gains `autostart: bool` so a client can render a switch whose
position it did not have to remember.

**`ServiceRecord` is where the column is read**, not `Declaration`. `core::services::record` and
`core::services::records` already select from `services` and are already what `api::rpc::summary`
is handed; `declaration` is a four-table join per service and using it for a listing would make
`service.list` quadratic in the number of services to report one boolean. One column added to two
queries.

**`#[serde(default)]` on the new member**, because ADR 0020 says the published contract is the shape
the daemon *writes* and a client older than this build must keep parsing what a newer one says.

### D2 — What starts at boot is a plan, not a loop over flagged rows

`ServiceGraph::start_plan(autostart roots)` and not "for each row where `autostart = 1`, start it".
A php-fpm pool with `autostart = 1` that depends on MariaDB with `autostart = 0` must bring MariaDB
up, because a pool whose database is missing is a pool that fails a health check, and the graph
already knows this. **So a service with the flag off can be started at boot by something that
depends on it, and that is correct** — it is the same rule `mix service start php-fpm@8.3` follows
and the same sentence `start_plan`'s own documentation uses. It is written into the CLI's help and
the desktop's hint so nobody reads a stopped-looking switch as a promise.

A home where nothing carries the flag plans nothing and starts nothing. This is the default, and it
is what every existing home and every test fixture has.

### D3 — The boot walk runs after the endpoint is served, and never blocks it

Beside the certificate-renewal clock, the idle sweeper, the metrics sampler and the memory watchdog
in `serve` — the same family, the same `shutdown` token — with one constraint of its own that none
of those four have: **it runs after the API is bound and serving**, not before.

The four existing loops are started before the endpoint on purpose, because each one is a *sweep*
whose first pass must not race a client. This one is the opposite: it starts real programs, it can
take seconds, and a daemon that will not answer `daemon.status` until MariaDB has finished its first
run is a daemon that looks hung at exactly the moment somebody is looking at it. Every state change
it makes is announced on the event stream a client is already reading, so the Dashboard fills in as
it goes rather than appearing complete.

It runs **after `services.recover()`**, for `recover`'s own reason: a reading must never be taken of
a service this daemon has not decided about yet. A service recovery adopted is already running and
the plan finds it satisfied.

### D4 — One attempt, no retry, and the failure is a state a person can see

A service that fails to start at boot is left failed, with the `StateReason` that says why, exactly
as a failed `service.start` is. **No retry, no backoff, no second pass**: the two reasons a boot
start fails are a port somebody else holds and a program that is not there any more, and neither is
fixed by trying again thirty seconds later. Both are already reported — `PortInUse` names the
process holding it, `SpawnFailed` names the spawn — and both are already on the Dashboard.

`StateReason` gains one variant, `Autostart`, so that the event says *why* the service started
rather than implying a person asked. It is `#[non_exhaustive]`, so this costs no client anything:
a build that does not know the word reads it as "none recorded", which is the rule the blueprint
trust column already follows.

### D5 — Autostart and the idle sweeper are orthogonal, and the screen says so

Phase 7's promise is that thirty idle minutes leave only the daemon and the web server. A service
with `autostart = 1` and an idle timeout starts at boot and is stopped again when nothing uses it,
and **that is the correct behaviour of two settings that answer different questions** — "is this
running when I sit down" and "is this running while I am not using it". It is also, obviously, a
combination somebody will read as a bug.

So the two are rendered in one panel, next to each other, with one line of text between them. They
are not merged, they are not made to override one another, and no rule is added that exempts an
autostart service from the sweeper. Making one setting quietly cancel the other is how a product
ends up with two switches that each only work sometimes.

### D6 — A blueprint with a `[site]` can be asked for a front end, with the actions that already exist

The planner learns one rule: **a manifest that declares a `[site]` needs a front end, and a home
with none gets one planned — when the caller asks.** It is expressed with
`PlanAction::InstallPackage` and `PlanAction::EnsureService` — the two variants a `[[services]]`
entry already produces — so the plan a person reads gains two familiar lines and the proto gains one
defaulted boolean, `BlueprintApply.front_end`.

**This paragraph said "always" until the task was built, and the suite is what corrected it.**
`crates/mixengine-cli/tests/blueprint.rs` is offline by construction — its module note says so and
says why — and an unconditional rule made every apply in it reach the package index for a web server
none of those tests is about. The finding generalises past the suite: an apply is about a *project*,
and provisioning the machine it runs on is a wider thing that should be asked for. So `front_end`
travels in the request, defaulted off, and the caller that sets it is the one whose sentence is *get
me a working site* — the Quick Start, and `mix blueprint apply --with-front-end`. The flag travels
on the dry run as well, which is what keeps `--dry-run` matching the real run.

**And the instance name comes from the recipe.** A front end is `Instancing::Single` — there is one
Caddy, and `service.create` refuses `caddy@main` in as many words — so a hardcoded `"main"` planned
a step the executor was guaranteed to be refused on. Asked of the catalogue, so a future front end
that is not a singleton is planned correctly without the line being revisited.

**No new `PlanAction` variant, and no new manifest key.** A front end is not something a blueprint
should be able to *choose*: exactly one runs at a time per home (T37), the choice is the home's and
lives in `service.set_front_end`, and a manifest that named one would be a manifest that applies
differently on a machine that had already chosen. What travels between machines is "this project is
served over HTTP", which the `[site]` section already says.

Which one is planned: **the front end this home already has, or `caddy` where it has none.** Caddy
is the documented default and the one the warm-start budget was measured on. The version question
goes through the machinery that already exists — `Disposition::Choice` and
`AnswerSubject::Service { id }`, because a front end *is* a service and `caddy@main` is a
`ServiceId` — so a client that can already answer "PHP 8.2.23 is not installed" can answer this one
with no new code at all.

A home that already has a front end plans `Satisfied` and nothing happens. **Every existing
blueprint, captured or built in, gains this for free**, which is the point: the gallery sells a
stack, and a stack nothing serves is not one.

### D7 — An apply can ask that what it creates starts with MixEngine, and still starts nothing itself

`BlueprintApply` gains `autostart: bool`, defaulted to `false`. When it is true, **every service
this apply creates** is created with the column set. It changes nothing about a service the apply
found already there: a shared MariaDB somebody deliberately leaves stopped is not something a second
project gets to re-decide.

**And the apply still starts nothing.** This was the first thing in this design to be wrong. A
`PlanAction::StartServices` at the end of the job reads well and is unshippable, because
[blueprints.md](../features/blueprints.md) is explicit that an apply never raises an
elevation prompt — it *queues* what needs one and the client spends the single prompt afterwards. A
job that started the front end before that prompt was spent would serve a site at a domain the hosts
file does not resolve and a certificate no store trusts, and the person would watch a progress bar
finish and then get a browser error. The start has to happen **after** the elevation, which means it
happens after the job, which means it belongs to the caller.

So the sequence, for both clients, is: apply → spend the elevation → start → open. Three calls the
daemon already answers, in an order the feature document already describes.

### D8 — "Start" means `service.start` with no target, and says so in those words

The last step of the sequence is `service.start` with no target: **start everything this home
declares**, in dependency order, which is a sentence the daemon has answered since phase 1.

The alternative — start only what this apply made — would require a client to derive a service set
from a finished plan, which is business logic in a client and is forbidden here. It would also be
wrong on the second apply, where the front end is `Satisfied` and therefore not in the set, and the
site would come up with nothing serving it.

So both clients say the true thing rather than the flattering one. `mix blueprint apply --start`
documents itself as *start every service this home declares, once the apply is done*, and the Quick
Start's button says the same. **On the home the Quick Start appears on, the two sets are identical**
— the card is only drawn when `site.list` is empty, so everything this home declares is what this
apply just made — and on a home where they differ, the person reading the button is told which one
they are getting.

### D9 — The Quick Start is a card, not a wizard, and it disappears by being finished

On the Dashboard, above the service table, drawn **only when `site.list` answers empty**. A stack
picker filled from `blueprint.list`, a project name, a folder, and one button.

Not a modal: a modal over a daemon that is still installing a runtime is a modal in the way. Not a
route of its own: a screen somebody can navigate back to after they have twenty sites is a screen
that has to justify itself forever. Not a `localStorage` "dismissed" flag either — **the condition
is the state of the home, so a machine whose sites were all deleted gets the card back**, which is
right, and there is nothing to migrate, reset or hand-edit.

Pressing the button runs the sequence of D7, showing the same plan, the same version questions and
the same scaffold consent `ApplyDialog` already renders — it is the same component, opened with the
fields filled in. It ends on the URL, as a link.

### D10 — PHP extensions get a screen, and the two "Extensions" stop colliding

No API changes. `screens/Packages/ExtensionsPanel.tsx` is lifted into a screen of its own,
`phpExtensions`, with a version selector above it, and stays reachable from the Runtimes screen
where it is today — the panel is one component rendered in two places, not two copies.

**And the name collision is fixed in the same stroke**, because it is half the reason nobody found
the feature. MixEngine's own add-ons — Mailpit, phpMyAdmin — are called *Extensions* in the sidebar
today, sitting four rows below *Runtimes*, where the PHP ones are hidden. The add-ons screen's label
becomes **Add-ons** in `en.ts` and `vi.ts`. The screen id, the module, the `extension.*` methods and
every document keep the word: this is a label, and the daemon's vocabulary is not a client's to
rename.

### D11 — The sidebar is five groups, and the groups do not collapse

Eleven flat items become twelve in five groups, in an order that matches what somebody does rather
than what the API is shaped like:

| Group | Screens |
| --- | --- |
| Overview | Dashboard, Metrics, Logs |
| Websites | Projects, Sites, Domains |
| Environment | Runtimes, PHP Extensions, Services |
| Library | Blueprints, Add-ons |
| — | Settings, pinned to the bottom |

**Static headings, not collapsible sections.** A section that collapses is a place for the thing
somebody is looking for to hide, and its state is something to persist, migrate and get wrong. The
list is twelve items; it fits.

The order stays a single table in `Sidebar.tsx`, as it is today, and remains the only place the
order is decided — which is what keeps this change from touching `tabState.ts` for anything except
the one new screen id.

## The work

Eight tasks, `T112`–`T119`. The first three are the API hole; the next three are the first site; the
last two are the affordances.

### T112 — A service says whether it starts with MixEngine, and can be told to

`ServiceRecord.autostart` from one more column in `core::services::record` and
`core::services::records`; `ServiceSummary.autostart` from it in `api::rpc::summary`;
`service.set_autostart` beside `service.set_idle` in `rpc.rs` and `method::SERVICE_SET_AUTOSTART` in
`proto::rpc`. `mix service autostart <id> --on|--off`, and an `AUTOSTART` column in
`mix service list`. Bindings regenerated.

**`mix service autostart` and not `mix autostart`.** The second one is taken, by T85b, and means
something else: whether this *machine* starts a daemon for this home at login. The two read as one
sentence in the right order — `mix autostart` is about the daemon, `mix service autostart` is about
one service — and it is the same split `mix service idle` already lives on. A single word for both
would have made `mix autostart` mean two things depending on how many arguments followed it.

Tests: the round trip over a real socket in `crates/mixengine-daemon/tests/lifecycle.rs`, the column
in `crates/mixengine-cli/tests/service.rs`, and the existing `service.create --autostart` coverage
extended to assert the value comes back on `service.list` rather than only being written.

### T113 — The daemon starts what asked to start

`crate::services::autostart` in the daemon: read the ids whose column is set, `start_plan` over
them, walk it, announce every change. Started in `serve` after `services.recover()` **and after the
endpoint is serving** (D3), spawned rather than awaited, cancelled by the same `shutdown` token as
its four siblings. `StateReason::Autostart`.

Tests, in `crates/mixengine-daemon/tests/lifecycle.rs` beside the recovery ones: a home with two
services where one carries the flag and depends on the other starts both; a home with none starts
nothing; a daemon whose autostart service cannot bind still answers `daemon.status` and reports the
failure with `PortInUse`.

### T114 — The desktop shows it and sets it

The switch in `screens/ServicesDetail`, in one panel with the idle timeout and the sentence of D5
between them; the value in the Dashboard's service table so the answer to "what comes back after a
reboot" is visible without opening anything. Strings in `en.ts` and `vi.ts`.

### T115 — A blueprint with a site can be asked for something to serve it

The planner rule of D6, in `core::blueprints::plan`, expressed with `InstallPackage` and
`EnsureService`, behind `BlueprintApply.front_end` and `mix blueprint apply --with-front-end`. The
instance name comes from the recipe's `Instancing`, which for a front end is `Single` — so the id is
`caddy` and not `caddy@main`.

Tests: `--with-front-end --dry-run` on a home with no front end plans two more actions; the same
command without the flag plans neither; `--dry-run` output still matches the real run exactly, which
is one of this feature's own acceptance criteria. What a home that *has* a front end plans is left
to a suite that has a real web server: `blueprint.rs` is offline by construction and a fake Caddy is
not a Caddy — `core::sites` renders and reloads a front end when a site is created, and what it runs
has to be the real program.

### T116 — An apply can hand on the autostart flag

`BlueprintApply.autostart`, defaulted false, threaded into every create the apply performs and into
nothing it finds. `mix blueprint apply --autostart`. Bindings regenerated.

### T117 — One action gets a new user a website

`mix blueprint apply --start`, documented in D8's words, and the Quick Start card of D9 on the
Dashboard, over the same `ApplyDialog`. The card's button sends `autostart: true`.

### T118 — PHP extensions get a screen, and the add-ons get their name back

D10: the `phpExtensions` screen id in `tabState.ts`, the version selector, the shared panel, the
label change in both dictionaries.

### T119 — The sidebar is grouped

D11: the five groups in `Sidebar.tsx` and the CSS for a heading.

## Risks, and what each one changed

**A prompt at login.** The first version of D3 had the boot walk before the endpoint was bound, and
the first version of this risk list said starting a front end on port 80 would raise an elevation
prompt at every login on macOS and Linux. It does not: `PortAccessGrant` is whole-state, spent once
when the front end is created, and `elevation.rs` enqueues rather than prompts in any case. **But
the walk still must not run before the endpoint**, for the reason D3 gives, and that is why it is
the one member of its family that starts last.

**A boot storm.** Five services at login on a laptop. The plan is tiered and walked in order, the
same walk `mix service start` performs, and the measured warm figure for Caddy + MariaDB + Redis is
875 ms on macOS, 2133 ms on Windows and 3189 ms on Linux (M3). It is bounded by the same budget as
everything else and it is not on the daemon's critical path.

**The `bench` job.** `crates/mixengine-cli/tests/warm_start.rs` times a single `mix service start`
against a home it builds itself, and a boot walk racing that measurement would make a green
benchmark meaningless. It cannot: `ServiceCreate.autostart` defaults to false and the suite never
sets it, so the plan is empty and nothing is walked. **This is a real constraint on T116** — the
`autostart` flag on an apply must default to false, or the next fixture that uses a blueprint
changes what the benchmark measures.

**A service that fails at boot, forever.** Covered by D4 as a decision rather than as a mechanism:
one attempt, the failure is a state, the state is on the Dashboard, and the two things that cause it
are both named in the reason.

**Autostart as an attack surface.** It is a boolean in the home's own SQLite, written through an
unprivileged method, that decides whether programs the same user already installed and configured
are started under that user at that user's login. It grants nothing, runs nothing new, and reaches
no privileged operation: **a service that needed an elevation to start would need it whether a
person or a clock asked**, and none of the eight recipes does. `mixengine-elevate` is untouched by
this phase.

**A blueprint that installs Caddy on a machine that did not want one.** This was the risk that
changed the design rather than being mitigated by it. T115 was written to change what an apply does
on *every* home with no front end, and the first thing to notice was `crates/mixengine-cli/tests/
blueprint.rs`, which is offline by construction: every apply in it suddenly reached the package
index for a web server none of those tests is about. A person deliberately running their own Nginx
outside MixEngine is the same case with a worse consequence. So the rule is opt-in —
`BlueprintApply.front_end`, `--with-front-end`, defaulted off — and the two mitigations that were
going to carry it on their own now only have to carry the caller who asked: the plan is shown before
anything happens and `--dry-run` prints it, and a home that has chosen a front end through
`service.set_front_end` — Nginx included — plans `Satisfied` whatever the flag says.

**Two settings that look like they contradict each other.** D5, deliberately not resolved by making
one win.

**A hand-edited `enabledModules` or a stale session naming `phpExtensions`.** `parseMixEngineTabState`
already refuses a screen id it does not recognise and falls back to `dashboard`, so a session
written by this build and read by an older one degrades rather than breaks. Adding the id to
`SCREENS` is the whole of the compatibility work.

**Quadratic listing.** Named in D1, and the reason `ServiceRecord` rather than `Declaration` carries
the column.

## Acceptance criteria

1. `mix service autostart mariadb@main --on`, then `mix service list`, shows the flag; the daemon is
   stopped and started, and MariaDB is running without anybody asking it to.
2. A php-fpm pool with the flag, depending on a MariaDB without it, brings MariaDB up at boot; the
   CLI's help and the desktop's hint both say this happens.
3. A daemon whose autostart service cannot bind its port answers `daemon.status` immediately and
   reports the service failed with the name of the process holding the port.
4. A home with no service carrying the flag starts exactly what it starts today: nothing.
5. `mix blueprint apply static --with-front-end --dry-run` on a home with no front end names
   installing and creating one; the same command without the flag names neither; and the dry run
   matches the real run action for action, the flag included.
6. On a fresh install: open MixLab, press one button on the Dashboard, answer the elevation prompt
   once, and a browser opens on a working `https://<name>.test`.
7. That machine is restarted, MixLab is opened, and the site is serving with nothing pressed.
8. A PHP extension can be turned on from a screen reachable in one click from the sidebar, and the
   sidebar no longer contains two entries called *Extensions*.
9. `cargo clippy --workspace -- -D warnings`, `cargo fmt --all --check`, `cargo test --workspace`,
   the rustdoc run, `cargo sqlx prepare`, `packaging/bindings.sh`, and the desktop's
   `npm run build && npm test && npm run lint` are all clean, on all three operating systems in CI.

## Assumptions

Recorded because they were decided here rather than asked:

- **Caddy is the front end a blueprint plans on a home with none.** It is this product's default and
  the server the warm-start budget was measured against. Nginx remains one `service.set_front_end`
  away, and a home that has made that choice is left alone.
- **The Quick Start's condition is an empty `site.list`**, not a stored flag, not the first-run
  screen's profile, and not the presence of a project. A person with three projects and no site has
  not got a website yet.
- **The five sidebar groups are not translated as a new concept.** They are five short headings in
  both dictionaries; they carry no state and nothing routes to them.
- **`autostart: false` stays the default everywhere** — on `service.create`, on `blueprint.apply`,
  and on every fixture. Nothing that exists today changes behaviour when this phase lands until
  somebody turns something on.
