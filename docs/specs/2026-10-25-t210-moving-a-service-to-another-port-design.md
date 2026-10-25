---
status: approved
date: 2026-10-25
task: T210
---

# T210 — Moving a service to another port

Roadmap task [T210](../roadmap/phase-48-a-database-a-person-picks.md), phase 48. 2026-10-25.

**The case this comes from.** `mysql@57` holds 3306 and `mysql@8` was given 3307, because it was
created second. The person deletes `mysql@57` and wants `mysql@8` on 3306. Deleting a row rightly
promotes nobody into its port (`services.md`, *Ports, and who gets 3306*), so today the only way
there is to delete `mysql@8` as well, recreate it, and lose the data directory's place in the
layout — or the data, for somebody who does not know `data_dir` outlives a delete.

## Principle

**A port moves only when a person moves it.** Nothing about that changes: no delete, upgrade or
restart ever renumbers a row. What this task adds is the person's way to do it, checked as strictly
as the number they were first given, and honest about what the daemon cannot follow — a project's
`.env`, a saved connection, somebody's shell history.

## Goals

- `service.set { id, port }` — the method — and `mix service set <id> --port <n>`.
- The new port is checked the way an allocated one is at create: not held by another row, and
  bindable on this machine right now, with the holder named when it is not. Check and write are one
  critical section.
- The row changes, `etc/<id>/` is regenerated, and everything else rendered from that row's port
  follows (see *What follows the port*). A running instance is restarted; a stopped one stays
  stopped.
- The answer says what moved and names what the daemon cannot change.
- MixLab: a **Change port…** item in each service row's ⋮ menu on the Dashboard, and a **Port**
  panel on the service's detail screen beside Idle and Limits. Both open or show the same check and
  the same confirmation.

## Non-goals

- **Editing overrides after create.** The method is shaped so they can join it (one optional field
  per setting), but no field other than `port` is accepted in this task.
- **Moving a front end or a php-fpm pool.** See D2.
- **Moving `activation_port`.** It is in a rendered site file and nowhere a person reads; it keeps
  its own rule in `services/activation.rs`.
- **Rewriting anything outside the home.** A `.env` written by a blueprint apply, a saved
  connection in MixLab's `db` module and shell history are named, never edited by the daemon.

## Anti-goals

- A move that leaves the instance on neither port. If the restart on the new port fails, the
  instance goes back to the old one (D5).
- A refusal that says only "port in use". Every refusal names who holds it, as create's allocator
  does.
- A client deciding which services may move. The daemon says so, per row (D7).

## Design

### D1 — One method, `service.set`, with one field today

```jsonc
// request
{ "id": "mysql@8", "port": 3306 }
// response: ServiceSet
{
  "service": { /* ServiceSummary, after the move */ },
  "port": { "from": 3307, "to": 3306 },
  "restarted": true,           // false for an instance that was stopped
  "not_followed": [            // what the daemon cannot change, in sentences a client prints
    "a project's .env that names 3307",
    "a saved connection in MixLab that names 3307",
    "shell history and scripts that name 3307"
  ]
}
```

`port` is `Option<u16>` in the request type, so a later `overrides` field sits beside it. A request
with no field set is `invalid_argument` ("nothing to set"). The type lives in
`mixengine-proto/src/service_api.rs` (870 lines; splitting it first if the addition crosses 1,000,
per `source-files.md`), and `bash packaging/bindings.sh` regenerates `bindings/`.

`not_followed` is the daemon's wording, not a client's — `mix` prints it as a list and MixLab shows
it in the dialog. Whether a `.env` line is named only when an apply actually wrote one for this
service is the builder's call; listing it always is acceptable.

### D2 — Which services may move

**A service may move when the catalogue has a recipe for it whose source is `Source::Package`,
which has a `preferred_port`, and whose role is not `Role::FrontEnd`.** Today that is MariaDB,
MySQL, PostgreSQL, Redis, Memcached and MongoDB. The refusals:

- front end — `precondition_failed`: "caddy listens on 80 and 443, which every site is reached
  through; they do not move".
- php-fpm pool (`Source::Runtime`) — `invalid_argument`: "php-fpm@8.3.33's port is internal: sites
  reach it through the front end, and nothing a person types names it".
- a service an extension declared (no catalogue recipe; its ports are its manifest's and live
  partly in `extension_ports`) — `invalid_argument`: "mailpit's ports are set by the extension".
  Moving them is a later task if anybody asks.
- a row with no port — `invalid_argument`: "<id> listens on no port".

The rule lives in `mixengine-core` beside the recipe (one function, `port_movable(recipe) -> bool`),
because the daemon both enforces it and publishes it (D7).

### D3 — The check, inside the allocator's lock

In `mixengine-core/src/services/ports.rs`, a new `claim(store, host, bind, service, port)` that
reuses what `allocate` already has:

1. Take `ports::hold()` and keep it until the row's `UPDATE` has landed.
2. `port` equal to the row's current port → nothing to do; the call answers success with
   `from == to`, `restarted: false`, and `mix` says "mysql@8 is already on 3306".
3. `held(store)` contains `port` (any row's `port`, `activation_port`, or `extension_ports`) →
   `ServiceError::PortHeld { port, holder: "<service-id or extension id>" }`, `already_exists` on
   the wire: "3306 belongs to mariadb@main".
4. `bindable(bind, port)` is false → `ServiceError::PortUnavailable { port, pid, program }` from
   `moved()`'s same `port_owner()` reading, `precondition_failed`: "3306 is held by mysqld.exe (pid
   4120), which MixEngine does not manage". A port below 1024 on Linux/macOS fails the bind for the
   same reason and gets the same sentence plus a hint that ports under 1024 need privileges
   MixEngine does not run with.
5. `UPDATE services SET port = ? WHERE id = ?`, then release the lock.

`held()` needs to return who holds each number for step 3; today it returns numbers only. Widen it
to `(port, holder)` and keep `allocate`/`allocate_activation` reading only the number.

The bind test cannot meet this service's own idle hold (D4): that hold is on the *old* port, and
step 2 has already answered `from == to`. A port the daemon holds for *another* idle service is in
that service's row and is refused at step 3, by name, before any bind.

### D4 — What follows the port

In the daemon, `api/service_settings.rs` (313 lines) gains `service_set`, in this order:

1. Check and write (D3). Nothing has stopped yet, so a refusal leaves everything as it was.
2. **Running or supervised** → `stop_then_start` with the plans `services::restart` builds (a
   restart, so dependents that were up come back up). The start renders `etc/<id>/` from the new
   row. A port change is one of the changes a reload cannot apply, so no reload is attempted.
3. **Stopped by a person** → render only (`self.services.graph()`), which regenerates `etc/<id>/`.
   It stays stopped.
4. **Idle-stopped** (the daemon holds the database's own address in `services/hold.rs`) → release
   the hold on the old address and hold the new one, so the next client to dial the new port wakes
   it and nothing answers on the old one. Then render, as in 3.
5. **Extensions that point at this database** — a `web-app` such as phpMyAdmin whose generated
   `config.inc.php` names the endpoint (`extensions::config::of`), and an extension pool that
   carries the endpoint (`extensions/pools.rs`) — are rewritten with `configure_one`, and a running
   pool is restarted so it reads the new value. The builder confirms the full list by searching for
   every reader of `services.port` (`extensions::database::endpoint`, `services::handoff::address`,
   `services::client`); the design's claim is "every file rendered from the row", not this list.
6. `services::client` (the shim's `MYSQL_TCP_PORT`/`PGPORT`) reads the row on every run, so it
   follows with nothing done — and `mysql@8` on 3306 becomes what a bare `mysql` picks by its
   "product's own port" rule. Nothing to change; a test says so.

The front-end lock (`one_front_end_change_at_a_time`) is not taken: D2 refuses front ends before
anything is held.

### D5 — A restart that fails puts it back

When step 2's start fails on the new port, the call writes the old port back (through the same
`claim`, so a port somebody took in the meantime is reported rather than overwritten), renders, and
starts the instance again on the old port. The answer is the error the new port's start gave, with
a hint saying the instance is back on its old port — `package.upgrade`'s rollback, with the same
sentence shape. If the second start fails too, that failure is reported as it is; the row is on the
old port either way.

### D6 — `mix service set <id> --port <n>`

Beside `set-limits`, `set-idle` and `set-front-end`. `crates/mixengine-cli/src/commands/service.rs`
is 969 lines, so the new subcommand goes into a file of its own under `commands/service/` (and the
existing file is split first if it would cross 1,000).

- **A running instance asks first:** "mysql@8 is running; moving it to 3306 restarts it, and every
  open connection is dropped. Continue? [y/N]", through `confirm::ask`. `--yes` answers in
  advance; `--json` without `--yes`, and a non-interactive terminal, refuse with `unanswered()` —
  `set-front-end`'s rule exactly. A stopped instance does not ask.
- **Whether it is running is the daemon's answer** (`service.status` for that id before the call),
  not the CLI's guess. The window between that read and the call is accepted: the worst case is a restart nobody
  was asked about on an instance that started in the meantime, which `--yes` would have allowed.
- Output: "mysql@8 moved from 3307 to 3306, and was restarted." then the `not_followed` list under
  "Not changed by MixEngine:". `--json` prints `ServiceSet`.
- `docs/guide/en/cli.md` and `docs/guide/vi/` gain the command.

### D7 — The daemon says which rows may move

`ServiceSummary` gains `port_movable: Option<bool>` — optional on the wire per ADR 0019 (`None` =
a daemon built before the member). It is `port_movable(recipe)` from D2. MixLab shows **Change
port…** only where it is `Some(true)`; everywhere else the item is not drawn at all, rather than
greyed, because a greyed item invites a click that can only be refused.

## MixLab

All of it lives in `src/modules/mixengine/`; the `db` module's part is in *Saved connections*.

### One dialog, two ways in

`screens/ServicesDetail/PortDialog.tsx` (a new component, using `src/components/` primitives —
`using-shared-components`) is the whole interaction:

- A number field pre-filled with the current port.
- **Change** runs `service.set`. A refusal is shown inline under the field, verbatim from the
  daemon (D3's sentences), and the dialog stays open.
- When `ServiceSummary.state` is running, the dialog says so above the button — "mysql@8 is
  running. Changing its port restarts it and drops open connections." — and the button reads
  **Change and restart**. That sentence and that label are the confirmation; there is no second
  dialog on top.
- On success the dialog shows what moved and the `not_followed` list, with **Done**, and — when the
  `db` module is visible — **Update saved connections…** (next section).

The two ways in:

1. **Dashboard, the row's ⋮ menu** (`screens/Dashboard/ServiceRowMenu.tsx`): **Change port…**,
   directly under *Copy port*, drawn only when `port_movable === true`. The ellipsis is the promise
   that it opens a question, as *Reset credential…* does.
2. **The service's detail screen** (`screens/ServicesDetail/ServicesDetail.tsx`): a **Port** panel
   beside `AutostartPanel` and `IdlePanel`, showing the current port and a **Change…** button that
   opens the same dialog. For a row whose `port_movable` is false, the panel shows the port and the
   reason it does not move, in one line, and no button.

`api.ts` gains `serviceSet(id, port)`, typed against `bindings/`; the Rust side gains
`mixengine_service_set` in `src-tauri/src/modules/mixengine/`, following `adding-a-command.md`.
Every string goes through `t()` in both `en.ts` and `vi.ts` (`writing-user-facing-text`).

### Saved connections

The `mixengine` module may not import from `db`, and `db` may not dial the daemon. So the update is
**handed over through the shell's tab request**, the path *Explore data* already uses:

- **Update saved connections…** asks the backend to queue a `db` tab request whose state is
  `{ kind: "serviceMoved", service: "mysql@8", from: 3307, to: 3306 }`.
- The `db` module, opening that request, finds its saved connections whose `keyringRef` starts with
  `mysql@8/` and whose `port` is 3307 — a connection made from MixEngine carries that reference
  (`types.ts`) — and asks once: "2 saved connections still point at mysql@8 on 3307: *root*,
  *app*. Update them to 3306?" with **Update** and **Leave them**. With none, it says so and closes.
- Connections with no `keyringRef` (typed by hand) are not touched: the module cannot know they
  mean this instance. The `not_followed` list already told the person.

This is my call rather than a requirement: the offer lands in the Database tab rather than inside
the Port dialog, because only `db` can read its own list. The alternative — a shell-level message
bus between modules — is a larger change than this task.

## Documentation

- `docs/features/services.md`, *Ports, and who gets 3306*: the last two bullets are rewritten —
  a port belongs to its row until a person moves it with `mix service set`, which checks it the way
  create's allocator does and restarts a running instance. The module notes in `ports.rs` and
  `ServiceSummary::port`'s doc say the same.
- `docs/features/client-surface.md`: `service.set` and its two screens.
- `CHANGELOG.md`, `## [Unreleased]` → `### Added`: one line for `mix service set --port` and
  MixLab's *Change port…*.
- `node scripts/check-client-surface.mjs` passes with no new exception.

## Tests

- core: `claim` refuses a port another row holds (naming it), an extension port, an unbindable port
  (naming the holder via the mock host); accepts its own current port as a no-op; is serialised
  against a concurrent `create` (the existing lock test, extended).
- daemon: running instance → restarted on the new port, `etc/<id>/` names it; stopped → stays
  stopped, rendered; idle-held → the hold moves; a start that fails on the new port → back on the
  old one and running; front end and pool → refused with D2's sentences.
- CLI (`crates/mixengine-cli/tests/`, `tests-that-say-why`): M48's first half — on a home with two
  MySQL instances, delete the first, `mix service set mysql@8 --port <first's port> --yes`, the data
  directory is untouched and a bare `mysql` (through the shim) reaches it. `--json` without
  `--yes` on a running instance refuses.
- MixLab: vitest for the dialog's pure half (which label, which sentence, inline refusal), and for
  the `db` module's matching of saved connections by `keyringRef` + port.

## Left to the builder

- The exact wire error codes, if an existing one fits better than those named above.
- Whether `not_followed` names a `.env` only when an apply wrote one.
- Where `port_movable` is computed if `recipe` is not at hand in `service::summary`.
- Splits forced by the 1,000-line rule (`commands/service.rs`, `service_api.rs`).
