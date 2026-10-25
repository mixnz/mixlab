---
status: implemented
date: 2026-10-25
task: T209h
---

# No task outlives shutdown (design)

`docs/standards/rust.md`, *Async*, says a task that runs past the call that started it is held by
something shutdown waits for and stops on a token derived from the daemon's root token. The daemon
already has the thing shutdown waits for — `background::Background`, a named `JoinSet` that
`accept::stop_everything` drains beside the clients, inside the client grace — but only the start
can reach it, because it is a `&mut` borrow owned by `serve`. Everything spawned later, or from a
module the start does not hand a `&mut` to, falls back to a bare `tokio::spawn` whose handle
nobody keeps. This design lists every one of those, gives `Background` a shape the rest of the
daemon can spawn into, and moves each task that touches the store into it. What stays outside is
written down here with the reason.

## The rule this applies

A detached `tokio::spawn` in non-test daemon code is one of:

1. **Owned and waited for** — in `Background`, in a `JoinSet` that is drained, or a `JoinHandle`
   that something on the shutdown path awaits — and it stops on the root token or a child of it.
2. **Safe to drop**, with the reason in a comment at the spawn: it touches neither the store nor a
   file that is state, and it ends on its own once what it drives is dropped.

A task that writes state — a row, a certificate, a file under `etc/` or `bin/`, a service started
or stopped — is never case 2.

## `Background` becomes shareable

Today: `Background { tasks: JoinSet<()>, names: HashMap<Id, &'static str> }`, `spawn(&mut self)`.

After:

- `#[derive(Clone)] struct Background(Arc<Mutex<Inner>>)`, with `Inner { tasks, names, closed }`.
  `spawn(&self, name, work)` takes the lock for the insert only; it is never held across an
  `.await`.
- `shut_down(&self, grace)` sets `closed`, takes the `JoinSet` out of the lock with `mem::take`, and
  then does exactly what it does today: join within `grace`, name and abort what is left, and await
  the aborts (`JoinSet::shutdown`).
- **A spawn after `closed` does not run.** The root token is already cancelled by then, so the work
  would end at its first `select!`; running it would only reopen the race with `Store::close`. It is
  dropped with a `debug!` naming it.
- **A spawn reaps what has finished.** Before inserting, `spawn` drains `try_join_next_with_id`,
  logging a failed task the way `shut_down` does. Today's set only ever holds a handful of start
  jobs and is reaped once, at shutdown; with one entry per RPC request and per proxied connection
  (below), a set reaped only at the end would grow for as long as the daemon runs.
- `name` stays `&'static str`. Every caller below has a fixed name.

The handle reaches the rest of the daemon two ways, both clones of the one `serve` makes:

- through `api::Shutdown`, which already carries the root token and the grace:
  `Shutdown::new(token, grace, background.clone())` and `api.shutdown().background()`;
- through the registry, for the activation splices below: `Registry::with_background(background)`,
  the builder shape `with_welcome` already has, and `Registry::background()`. `Registry::new`
  starts with a `Background` of its own, so the fixtures that build a registry need no change.

The start's own steps keep taking it as a parameter, now by `&`.

The existing two tests stay; two are added: a spawn after `shut_down` has begun never runs, and a
finished task is gone from the set by the next spawn.

## Inventory — the daemon

Non-test `tokio::spawn` calls in `crates/mixengine-daemon/src` on 2026-10-25, with what each one
becomes. `spawn_blocking` calls are all awaited where they are made and are not listed.

### Moved into `Background`

| Site | What it does | Why it cannot be dropped |
| --- | --- | --- |
| `certs/renewal.rs:146` | renewal clock | writes leaf certificates, then `reconfigure` writes `etc/` |
| `services/idle.rs:422` | idle sweep | stops services: rows and processes |
| `services/watchdog.rs:307` | memory watchdog | stops services |
| `metrics/sampler.rs:440` | sampler | writes `metrics` rows |
| `sites/revoke.rs:160` | share expiry | ends shares: rows, `etc/`, mDNS |
| `services/autostart.rs:49` | autostart walk | starts services |
| `api/service.rs:390` | a walk nobody waits for | starts and stops services |
| `bin_watch.rs:44` | `bin/` watch | rewrites `bin/` and reads the store; today its handle is kept as `_watching` but never awaited and it does not watch the token at all — it gains a `select!` on it |
| `api/uninstall.rs:47` | follows the uninstall job, then cancels the root token | reads the store. **Also a defect**: once the root token is cancelled, `Jobs::wait` returns at once with an unfinished job, and this loop spins on `status` reads until the process exits. It gains a `select!` on the token |
| `api/logs.rs:300` | ends a `job logs -f` stream when its job ends | reads the store; already ends on a child token, so only its ownership changes |

All of these already select on the root token (except `bin_watch` and `uninstall`, fixed above),
so draining them costs a shutdown one turn of each, inside the grace it already has.

`clocks::start_clocks`, `started::after_the_api` and `autostart::start` take `&Background` instead
of spawning; the API handlers reach it through `self.shutdown.background()`.

### The RPC handler

`api/rpc.rs:171` spawns every request's handler so that a panic is caught per request. The spawn is
awaited by the connection task, but the connection tasks are a `JoinSet` that `serve::shut_down`
drops after the client grace — and **dropping the connection does not drop the handler**: it runs
on, detached, into `Store::close`. `services/shutdown.rs:441`, `services/start.rs:161` and
`services/mod.rs:236` each describe a defect this caused and the guard written against it.

Decided: the handler is spawned into `Background` (name `"rpc handler"`) and the connection awaits
its answer through a oneshot. A `JoinSet` hands back no result, so the panic is caught inside the
task with `futures_util::FutureExt::catch_unwind` (the daemon's `futures-util` gains its `std`
feature) and sent as a value. A sender dropped without an answer — the task stopped by the
shutdown, or refused because the set was already closed — is answered `conflict`, saying the daemon
is stopping: `internal` would tell a client polling `daemon.status` through a stop that it had met
a bug. After the grace a
handler still running is aborted with the rest: the same treatment a connection gets today, now
reaching the work the connection was waiting on. The three guards stay; their comments change from
"a spawn nothing joins" to what is now true.

### Already owned — no change

| Site | Held by |
| --- | --- |
| `jobs.rs:273` | `Jobs::running`; `Jobs::shut_down` waits for each `finished` token within the shutdown budget, and a job left past it is the next daemon's `abandon` |
| `services/start.rs:342` | the runner map; `Registry::shut_down` drains and awaits it |
| `services/hold.rs:108` | `Held { cancel, task }` |
| `services/activate.rs:109` | the returned `accepting` handle |
| `serve/accept.rs:64` | the connections `JoinSet` |

### The per-connection splices

`services/activate.rs:129` and `services/hold.rs:199` spawn one task per accepted connection, and
`carry` may start the service it was asked for, which writes rows from inside that task. Their
parent (`accepting`, `Held::task`) is owned; they are not.

**They must outlive their accept loop, so a `JoinSet` local to the loop is not an option.** Both
places say so: `activate::release` ends the accepting and lets "a connection already being carried
finish on its own task", and in `hold.rs` the start a splice asks for calls `Holder::release`,
which cancels and awaits the very loop that accepted it. A set owned by the loop would abort the
connection that woke the service, mid-start. So they go into `Background` through
`services.background()` (names `"activation splice"` and `"held address splice"`), which is what
the reaping above is for: an entry lives as long as its connection.

### Safe to drop — a comment at the spawn says so

| Site | Why |
| --- | --- |
| `dns/server.rs:165` | ends on the root token with `shutdown_gracefully`; holds sockets, touches no state |
| `mdns.rs:209` | waits for a child token, then shuts the responder down; touches no state |
| `services/runner/attempt.rs:56` | relays a child's output into its log; ends when the capture's sender drops with the process. A log is not state |

### Test code — out of scope

`certs/handshake.rs:239`, `services/shutdown.rs:400` and `:567`, `services/runner/environment.rs:604`,
`services/activate.rs:589` and `:591` are inside `#[cfg(test)]`.

## Inventory — `apps/desktop/src-tauri`

MixLab has no shutdown that waits: `RunEvent::Exit` stops the tunnels, and the process ends. So the
question there is narrower — does any detached task write state that a quit mid-way would leave
half-written.

| Sites | Verdict |
| --- | --- |
| `modules/mixengine/rpc.rs:132`, `events.rs:66`, `metrics.rs:56`, `logs.rs:63`, `transport.rs:77` | drive a hyper connection; end when its `SendRequest` drops. Safe, comment added |
| `modules/mixengine/events.rs:80`, `metrics.rs:68`, `logs.rs:75` | read a stream into a `Channel`; end on the token kept in `MixEngineState` or when the window goes. Safe, comment added |
| `modules/terminal/stream.rs`, `local.rs:359`/`:380`, `remote.rs`, `ssh/mod.rs:589`/`:635`/`:646`, `db/drivers/redis.rs:671`/`:675` | belong to a session or tunnel whose state holds the handle or the channel that ends them; write nothing. Checked one by one during the work, comment where the owner is not obvious |
| `launch.rs:225`, `instance.rs` | the single-instance endpoint, alive for the app's life; `RunEvent::Exit` removes it. Safe |
| `lib.rs:144` | **writes state**: finishes an interrupted update and removes stale downloads. Proposed: its `JoinHandle` is kept in managed state and awaited, bounded by two seconds, in `RunEvent::Exit` before the process ends |

`std::thread::spawn` is outside this rule, which is about tasks. One is worth naming:
`import.rs:294` writes the vault on a thread nobody joins. It runs once, at first launch after an
import, and is left as it is.

## What changes in the documents

- `docs/standards/rust.md`, *Async*: the closing sentence ("listed and fixed by T209h") is replaced
  by where to spawn — `api.shutdown().background()` — and the two cases above.
- The roadmap ticks T209h and this spec becomes `implemented` in the same commit.

## MixLab

No screen changes and no method is added. The one MixLab change is `lib.rs`'s update recovery
being awaited at exit, which a user never sees: a quit no longer leaves an update recovery half
done for the next launch to redo.

## Testing

- `background.rs`: the new spawn-after-close test.
- `api/uninstall.rs`: a cancelled root token ends the follow task in one turn (the spin above).
- The existing shutdown suites — `services/shutdown.rs`, the daemon integration tests that stop a
  daemon right after it starts — are the evidence that draining the clocks costs nothing measurable.
- `cargo test --workspace`, `bash scripts/gate.sh`, and in `apps/desktop/src-tauri`
  `cargo clippy --locked --all-targets -- -D warnings`.
