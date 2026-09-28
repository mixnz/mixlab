---
status: implemented
date: 2026-09-28
task: T190b
---

# T190b — The idle sweep does not render to look

Phase 7, after [T190a](2026-09-28-t190a-what-the-daemon-spends-while-it-waits-design.md).
2026-09-28.

## The case

T190a found that with nobody watching, `mixengined` on Windows spends 60–95 ms every 30 s, about
0.2–0.3% of a core and close to half of what it spends at rest. The burst is the idle sweep.

`services::idle::Sweeper::sweep` runs every `idle_check_seconds` (30 by default), and its first
line is `self.registry.graph()`. `graph()` asks the spec source for `declared()`. The spec source
is `Rendered(Generator)`, and `Generator::declared()` does not only render: **it installs**. For
every service it resolves certificates (`served::served`, reading every leaf pair), renders every
document, stages and compares each file against what is on disk, creates the directories the
configuration names, and records first-run and provisioning state. `graph()` then hands over,
remembers rituals and remembers provisioning. The registry needs that on every path that changes
something. The sweep needs none of it.

What the sweep actually reads from the graph (`idle.rs`):

- `graph.spec(id).idle()`: each running service's idle policy, which holds the probe and the
  delay;
- `exemptions(&graph, id, &running, &warm)`: who depends on whom, so a dependency of a running
  service is never stopped;
- `self.stop(&graph, id, after)`: the same graph again, when a verdict says stop.

Those change when a service, a site, a setting or a certificate changes, which happens a few times
a day. The sweep asks 2,880 times a day.

## Principle

**Looking is not changing.** A periodic question may be answered from what the last change left
behind. The one moment the sweep acts, stopping a service, it acts on the present.

## D1. The registry keeps the graph its last walk made

`ServicesRegistry` gains a cache of the last `ServiceGraph` that `graph()` produced, and the moment
it was produced. **Every existing call of `graph()` still renders and installs, and refreshes the
cache on the way out.** Nothing that changes a home skips a render because of this task.

The sweep asks a new `graph_for_sweep()` instead:

- **a graph younger than `SWEEP_GRAPH_AGE` (10 minutes) is returned as it is**;
- anything older, or no graph at all, goes through `graph()` as today.

Ten minutes is the most a sweep may observe with a stale policy. The cost of it is bounded by D2.
With nothing else happening, that is one render every ten minutes instead of every thirty seconds:
about 90 ms / 600 s, or 0.015% of a core.

**Every path that changes what the graph is made of already walks it.** Starting, stopping,
creating, a site change, `service idle` and the others reach `graph()` or `walk()` on their own way
(`services/mod.rs`, `api/create.rs`, `api/rpc.rs`, `extensions.rs`, `updates.rs`). So the cache is
refreshed by the change itself, not by a timer. The plan checks this path by path. A mutation that
turns out not to reach `graph()` gets an explicit `forget_graph()` call, and a test that says so.

## D2. A stop is decided on a fresh graph

When a sweep's verdict is `Stop`, **the sweep renders once more (`graph()`) before it acts**, and
recomputes the verdict's two inputs, the policy and the exemptions, from that graph. If the fresh
graph no longer has an idle policy for the service, or now exempts it, the stop does not happen and
the tally for that service is reset. Stopping is rare: it happens once per service per idle period,
thirty minutes for a PHP pool by default. So the extra render costs nothing that shows.

This is what makes the staleness in D1 safe. The worst a ten-minute-old graph can do is:

- **observe with an old probe** (an old port or URL). The observation comes back
  `Unmeasurable` or `Busy`, and the service is not stopped: the safe direction;
- **count towards a stop with an old delay.** The count is judged again against the fresh policy
  before anything is stopped.

## D3. What stops happening, said out loud

Today every sweep also **rewrites any generated file someone edited by hand within 30 s**, as a
side effect nobody designed. After this task that happens at the next change or within ten minutes.
`CLAUDE.md` already says generated configuration is disposable and never parsed back, and
`mix doctor` reports drift (T47b). Nothing documented promised a 30-second repair. The feature doc
for idle stopping gets one sentence saying the sweep does not regenerate configuration.

## D4. A database's health is asked in its own protocol, not by starting a program

*Added after D1–D3 were built and measured.* With the 30-second burst gone, the floor on the real
home still read 0.60% on average (twelve 20-second windows, median 0.51%), against a target of
0.45%. The largest part of what is left was measured directly:

| Source | Cost | How it was measured |
| --- | --- | --- |
| A `HealthProbe::Command` run: `mysqladmin ping` for MySQL, `redis-cli ping` for Redis, each every 10 s | **10.4 ms of CPU in the daemon per run**, so 2.1 ms/s, about **0.21%** | a bench calling `mixengine_supervisor::Surroundings::run` 30 times, reading the process's own CPU time |
| Four supervisors waking every 250 ms (`runner.rs` `WATCH`) | under 0.1%: below one 15.6 ms quantum in 10 s, even with sixteen loops | a bench of the same loop shape |

The ten milliseconds are what starting a process costs on Windows: a restricted token (ADR 0010),
`CreateProcessAsUser`, and a thread per pipe. The daemon pays that twice every ten seconds to ask
two servers whether they answer.

**Two new probes answer the same question without a process.**

- **`HealthProbe::MysqlGreeting { addr }`, for MySQL and MariaDB.** The probe opens a TCP
  connection and reads the first packet the server sends. The server sends that packet from its
  connection handler before any login, so an answer proves the server is accepting and serving
  connections, not only that a listener is bound. **Healthy** means a complete packet header and a
  payload whose first byte is `0x0a` (the handshake) or `0xff` (an error packet, such as
  *too many connections* or *host blocked*). Anything else is unhealthy: nothing within the
  timeout, a closed connection, or bytes that are not a MySQL packet. This is `mysqladmin ping`'s
  own rule: its exit status is 0 whenever the server answers, *access denied* included. That is why
  the probe needs no password, and why `MYSQL_PWD` is not read for it.
- **`HealthProbe::RedisPing { addr }`, for Redis.** The probe opens a TCP connection, sends `PING`
  as a RESP array, and reads one reply line. **Healthy** means `+PONG`, or any error reply except
  `-LOADING` (for example `-NOAUTH`, which proves the server answered). **Unhealthy** means
  `-LOADING` (the dataset is still loading and the server will not serve reads), no reply within
  the timeout, or a closed connection.

Both keep the `Command` probe's reason for existing. The doc comment on `HealthProbe::Command` says a
TCP accept "only proves the listener is up". A greeting and a `PONG` are both written by the
server's own request path, so they prove more than an accept. The interval, timeout and
failure counts of each recipe's `HealthCheck` are unchanged.

**What moves:**

- `mixengine-proto`: the two variants, documented beside the others; `bindings/` regenerated
  (`packaging/bindings.sh`). Neither MixLab nor `mix` renders a `HealthProbe`: `grep` finds it
  only in `bindings/`.
- `mixengine-supervisor`: `health.rs` learns both probes. The protocol parsing lives in a small
  module of its own, with a pure function per reply so it can be tested without a socket.
- Recipes: `mysql.rs` and `mariadb.rs` switch their `HealthCheck` to `MysqlGreeting`, and
  `redis.rs` to `RedisPing`. Their **readiness** checks stay `Command`: a readiness check runs once
  per start, not every ten seconds.

## MixLab

**No screen changes and no new method.** The Services screen and the tray draw the same states. A
PHP pool is still idle-stopped after the same delay, and started by the same request. A database is
still reported unhealthy by the same run of failures. The only difference a person can see is the
daemon's CPU figure in the tray and on the Dashboard with nobody using anything.

## Testing

- **A sweep does not render when the graph is fresh.** Use a counting spec source (the pattern of
  `services/fixture.rs`): two sweeps a few seconds apart call `declared()` once. After
  `SWEEP_GRAPH_AGE`, driven by tokio's paused clock, the next sweep calls it again.
- **A change refreshes what the sweep sees.** A service started through the registry between two
  sweeps is in the second sweep's graph without the sweep rendering.
- **A stop renders first, and a changed policy saves the service.** Set up a fake service whose
  cached policy says stop after one period, and whose fresh render says no idle policy. The sweep
  does not stop it, and it resets the tally.
- **A stop on a fresh graph still happens.** The same setup with the fresh render agreeing: the
  service is stopped, and `declared()` was called exactly once for the stop.
- **The two probes, without a socket (D4).** For MySQL: a handshake packet, an error packet, a
  truncated header, an empty read and a non-MySQL banner, each judged by the pure function. For
  Redis: `+PONG`, `-NOAUTH …`, `-LOADING …`, an empty read and garbage.
- **The two probes, against a socket (D4).** A `TcpListener` in the test plays the server: it
  writes a greeting, or `+PONG`, or `-LOADING`, or nothing and closes. `Health::probe` then answers
  healthy or unhealthy within the check's timeout. A listener that accepts and never writes is
  unhealthy when the timeout runs out, not later.
- **The recipes (D4).** The MySQL, MariaDB and Redis recipe tests assert the new probe and the
  unchanged interval, timeout and readiness check.
- **The measurement.** A per-second timeline of the daemon's CPU time, as in T190a, on the real
  home with MixLab closed: the 30-second burst is gone, and the floor is recorded before and after
  in twelve 20-second windows.

| Target, the machine of T190a | Before | After |
| --- | --- | --- |
| Burst every 30 s | 60–95 ms | none |
| One health probe of MySQL or Redis, CPU in the daemon | 10.4 ms | **< 1 ms** |
| Daemon, nobody watching | 0.83% | **≤ 0.45%** of one core |

### Measured

Release builds (`MIXENGINE_RELEASE=1`, `crt-static`), on the real home with four services running
(caddy, mysql@5.7, php-fpm@7.3.33, redis@main) and MixLab closed. Each build was started by `mix`.
"Mean" is the mean of the 20-second windows; the timeline is the daemon's CPU time over 120 s.

| Build | Mean of the 20 s windows | 120 s timeline | Burst every 30 s |
| --- | --- | --- | --- |
| Original, measured twice | 0.83% (6) / 0.81% (6) | 0.84% / 0.76% | 62–156 ms |
| D1–D3 | 0.60% (12) | 0.64% | none |
| **D1–D4** | **0.53%** (12) | **0.53%** | none |

| Bench | Before | After |
| --- | --- | --- |
| One health probe of MySQL, CPU in the probing process | 9.90 ms (`Command`) | **0.52 ms** (`MysqlGreeting`, the fake server included) |

**The first two targets are met; the third is not.** The floor fell by about 35%, from 0.82% to
0.53%, and the 30-second burst is gone. **0.45% was not reached, and the target is not moved.** A
60-second ETW trace of the D1–D4 build (`/OPT:NOICF`, frame pointers) names what remains:

| Samples in 58 s (ETW's own stack walking set apart) | What | How often | Without a trace |
| --- | --- | --- | --- |
| ~24 | `CreateToolhelp32Snapshot` in `Sampler::measure` | once a minute | ~7 ms a minute (T190's bench) |
| ~23 | `TcpStream::connect` and close for the health probes and the idle probe | four connections every 10 s | small; ETW inflates socket calls in the kernel most |
| ~14 | the `notify` crate's watcher thread over the runtimes' bindirs | wakes every 100 ms, inside `notify`'s Windows backend | ~0.02% |
| ~3–5 | the sharing check's interface list | every 30 s | ~0.01% |
| ~190 | tokio's scheduler and timers, the heap, context switches | spread | no single source |

**No single source remains above 1 ms/s.** What is left is the sum of several small periodic tasks
and the runtime underneath four supervised services. Going lower means making that whole family
less frequent (probes, watcher, timers). That is a design change beyond this task, and the owner
chose to ship this one with the gap recorded rather than hold it for that.

## Out of scope

- **Making `declared()` cheaper.** A render that installs is the right thing for every path that
  changes a home. This task stops calling it when nothing changed.
- **The other callers of `graph()`.** Each of them answers a request or follows a change, so it runs
  when somebody asked.
- **The sweep's period.** Thirty seconds stays. It is the observation that has to be frequent, not
  the rendering.
- **PostgreSQL's `pg_isready`.** Its protocol needs a startup message rather than a greeting, and no
  PostgreSQL was running on the measured home. It gets its own task if a measurement asks for it.
- **Making a process start cheaper.** The restricted token is ADR 0010's, and D4 removes the start
  from the loop that runs every ten seconds rather than shaving it.
